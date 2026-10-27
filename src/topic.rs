//! Topic slug derivation and topic-input interpretation — extracted from
//! `main.rs` (module split, Task 2): the most-depended-on cluster, moved first
//! so later modules have a stable target to `use`.

use crate::tokens;

/// System prompt that extracts a topic slug from a sentence — used by both the plain
/// path (`derive_slug`) and the TUI topic entry.
pub(crate) const SLUG_SYSTEM: &str = "Reduce what the user wants to learn/do to A SINGLE short \
    file-name slug. Rules: lowercase only, ascii (no accented characters), words separated \
    by hyphens, AT MOST 3 words, filler words (i/a/with/make/want) are dropped. \
    RETURN ONLY the slug — no explanation, no quotes, no punctuation. \
    Example: 'i want to build a todo app with rust' -> rust-todo";

/// Slug system prompt — if there are saved topics, resume-awareness is added
/// (spec K2): the model converts intent-to-continue into the existing slug, and the flow counts it as Resume.
pub(crate) fn slug_system(known: &[String]) -> String {
    if known.is_empty() {
        return SLUG_SYSTEM.to_string();
    }
    format!(
        "{SLUG_SYSTEM}\n\nExisting topics: {list}. If what the user wrote is a request \
         to CONTINUE one of these topics (picking up the same work, 'where we left \
         off', referencing prior work), return ONLY that topic's slug VERBATIM. If \
         it's a new topic, generate a new slug.",
        list = known.join(", ")
    )
}

/// Convert the model's slug reply into the final slug — turn hyphens into spaces
/// and guarantee it via `slugify_topic`; if it falls back to "general", derive a
/// local slug from the raw input instead. Pure.
pub(crate) fn finalize_slug(raw: &str, model_reply: &str) -> String {
    let s = slugify_topic(&model_reply.trim().replace(['-', '_'], " "));
    if s == "general" || s == "genel" {
        slugify_topic(raw)
    } else {
        s
    }
}

/// Parse the pre-lock conversation's topic lock (SPEC §4.22): if the reply's
/// FINAL non-empty line is `TOPIC: <slug>`, return (slug, display text without
/// that line). Mid-reply markers do NOT lock — only the final line counts, so
/// prose that merely mentions the token can't hijack the flow. Blank slug →
/// None (checked BEFORE slugify_topic, which would otherwise fall back to
/// tokens::DEFAULT_TOPIC).
pub(crate) fn parse_topic_marker(reply: &str) -> Option<(String, String)> {
    let trimmed = reply.trim_end();
    let (head, last) = match trimmed.rsplit_once('\n') {
        Some((h, l)) => (h, l.trim()),
        None => ("", trimmed.trim()),
    };
    let rest = last.strip_prefix(tokens::TOPIC_MARKER)?;
    if rest.trim().is_empty() {
        return None;
    }
    // Hyphens already inside the value would be stripped by slugify_topic's
    // whitespace split — turn them into spaces first (finalize_slug's trick).
    let slug = slugify_topic(&rest.replace(['-', '_'], " "));
    Some((slug, head.trim().to_string()))
}

/// New-topic confirmation text (for TUI tui_confirm). The plain path uses its own
/// `[y/N]` rustyline format — the wording is deliberately different, the two surfaces are separate.
/// The answer is typed + Enter (`parse_confirm_answer`): yes/no, Turkish variants accepted silently.
pub(crate) fn new_topic_confirm_msg(slug: &str) -> String {
    format!("new topic: {slug} — open it? (yes/no)")
}

/// Interpret the topic input: resume or new topic? (spec K1)
#[derive(Debug)]
pub(crate) enum TopicChoice {
    /// Resume an existing project-local topic.
    Resume(String),
    /// New-topic flow — raw input (the caller slugifies it).
    New(String),
    /// Empty Enter with no resumable topic but a filled mentor/PROJECT.md —
    /// Usta proposes where to start (spec: project-aware start).
    Suggest,
}

/// Deterministic selection rules — order follows spec §3/K1's table. `None` =
/// swallow the input (empty + no topic to resume). No LLM; sentences return
/// `New`, K2 (slug_system) kicks in there.
pub(crate) fn interpret_topic_input(
    raw: &str,
    local: &[String],
    project_known: bool,
) -> Option<TopicChoice> {
    let raw = raw.trim();
    // 1-2: empty Enter.
    if raw.is_empty() {
        return match local.first() {
            Some(t) => Some(TopicChoice::Resume(t.clone())), // resume wins over suggest
            None if project_known => Some(TopicChoice::Suggest),
            None => None,
        };
    }
    // 3: numeric selection.
    if let Ok(n) = raw.parse::<usize>() {
        if n >= 1 && n <= local.len() {
            return Some(TopicChoice::Resume(local[n - 1].clone()));
        }
    }
    // 4: slug match.
    let slug = slugify_topic(raw);
    if let Some(t) = local.iter().find(|t| **t == slug) {
        return Some(TopicChoice::Resume(t.clone()));
    }
    // 5: short resume pattern (substring after deasciify).
    if !local.is_empty() && raw.split_whitespace().count() <= 4 {
        let d: String = raw
            .chars()
            .map(deasciify)
            .collect::<String>()
            .to_lowercase();
        const RESUME_WORDS: &[&str] = &["devam", "kaldigimiz", "kaldigim", "continue", "resume"];
        if RESUME_WORDS.iter().any(|w| d.contains(w)) {
            return Some(TopicChoice::Resume(local[0].clone()));
        }
    }
    // 6: new topic.
    Some(TopicChoice::New(raw.to_string()))
}

/// Reduce a Turkish letter to ascii + lowercase; lowercase everything else.
fn deasciify(c: char) -> char {
    match c {
        'ç' | 'Ç' => 'c',
        'ğ' | 'Ğ' => 'g',
        'ı' | 'İ' | 'I' => 'i',
        'ö' | 'Ö' => 'o',
        'ş' | 'Ş' => 's',
        'ü' | 'Ü' => 'u',
        other => other.to_ascii_lowercase(),
    }
}

/// Latin letters beyond Turkish, folded to ascii (review P2-10). `c` is already
/// lowercase.
fn latin_fold(c: char) -> Option<&'static str> {
    Some(match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ą' => "a",
        'æ' => "ae",
        'ć' | 'č' => "c",
        'ď' | 'đ' | 'ð' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' | 'ě' => "e",
        'ì' | 'í' | 'î' | 'ï' | 'ī' => "i",
        'ł' => "l",
        'ñ' | 'ń' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ø' | 'ō' | 'ő' => "o",
        'œ' => "oe",
        'ř' => "r",
        'ś' | 'š' => "s",
        'ß' => "ss",
        'ť' => "t",
        'þ' => "th",
        'ù' | 'ú' | 'û' | 'ū' | 'ů' | 'ű' => "u",
        'ý' | 'ÿ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        _ => return None,
    })
}

/// One word's slug part: the Turkish mapping first (unchanged), then Latin
/// diacritics; `+` becomes `p` and `#` becomes `sharp` once the word has
/// started (`c++` -> `cpp`, `c#` -> `csharp`); anything else is dropped.
fn slug_word(word: &str) -> String {
    let mut out = String::new();
    for c in word.chars().map(deasciify) {
        match c {
            '+' if !out.is_empty() => out.push('p'),
            '#' if !out.is_empty() => out.push_str("sharp"),
            c if c.is_ascii_alphanumeric() => out.push(c),
            c => {
                for l in c.to_lowercase() {
                    if let Some(s) = latin_fold(l) {
                        out.push_str(s);
                    }
                }
            }
        }
    }
    out
}

/// Stable slug for a topic written without Latin letters: `t-` + FNV-1a 32-bit
/// of the trimmed, lowercased input. Inline on purpose: `DefaultHasher` may
/// change between Rust versions and the slug names files on disk.
fn hash_slug(input: &str) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for b in input.trim().to_lowercase().bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("t-{h:08x}")
}

/// Turn free text into a topic slug — pure function, testable.
/// Rule: fold Turkish and Latin letters to ascii, spell out `+`/`#` inside a
/// word, lowercase, take at most the FIRST 3 content words, join with hyphens.
/// No ascii left but non-Latin letters present → `t-<hash>`; otherwise empty →
/// `tokens::DEFAULT_TOPIC`.
/// "temel Linux güvenliği" → `temel-linux-guvenligi`.
/// A typed topic argument: a string already shaped like a slug (what
/// `usta topics` prints) is used as-is, anything else is slugified.
pub fn topic_arg_slug(input: &str) -> String {
    let t = input.trim();
    let is_slug = !t.is_empty()
        && t.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        });
    if is_slug {
        t.to_string()
    } else {
        slugify_topic(input)
    }
}

pub fn slugify_topic(input: &str) -> String {
    // Filler words, compared against their deasciified (ç→c…) form — kept out
    // of the slug, so "ben rust ile bir todo yapmak istiyorum" → "rust-todo".
    const STOPWORDS: &[&str] = &[
        "ben",
        "bir",
        "ile",
        "ve",
        "icin",
        "bu",
        "su",
        "yapmak",
        "yapmayi",
        "istiyorum",
        "ogrenmek",
        "ogreniyorum",
        "istiyor",
        "bana",
        "de",
        "da",
        "the",
        "a",
        "an",
        "to",
        "learn",
        "want",
        "make",
        "build",
    ];
    let words: Vec<String> = input
        .split_whitespace()
        .map(slug_word)
        .filter(|w| !w.is_empty() && !STOPWORDS.contains(&w.as_str()))
        .take(3)
        .collect();
    if !words.is_empty() {
        return words.join("-");
    }
    // Only non-Latin letters: a stable per-topic slug, so two such topics never
    // share `general`'s progress file. Symbol-only / ascii-stopword-only input
    // stays `general`.
    if input.chars().any(|c| c.is_alphanumeric() && !c.is_ascii()) {
        hash_slug(input)
    } else {
        tokens::DEFAULT_TOPIC.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_lowercases_simple_word() {
        assert_eq!(slugify_topic("JavaScript"), "javascript");
    }

    #[test]
    fn slugify_hyphenates_short_phrase_and_deasciifies() {
        assert_eq!(
            slugify_topic("temel Linux güvenliği"),
            "temel-linux-guvenligi"
        );
        assert_eq!(slugify_topic("todo app"), "todo-app");
    }

    #[test]
    fn slugify_spells_out_plus_and_sharp() {
        assert_eq!(slugify_topic("C++"), "cpp");
        assert_eq!(slugify_topic("C#"), "csharp");
        assert_eq!(slugify_topic("F#"), "fsharp");
        assert_eq!(slugify_topic("C"), "c");
        assert_eq!(slugify_topic("#rust"), "rust");
    }

    #[test]
    fn slugify_folds_latin_diacritics() {
        assert_eq!(slugify_topic("straße"), "strasse");
        assert_eq!(slugify_topic("café"), "cafe");
        assert_eq!(slugify_topic("Ñandú"), "nandu");
        assert_eq!(slugify_topic("Œuvre Æsir"), "oeuvre-aesir");
        // Turkish results unchanged.
        assert_eq!(
            slugify_topic("temel Linux güvenliği"),
            "temel-linux-guvenligi"
        );
        assert_eq!(slugify_topic("İstanbul Işık"), "istanbul-isik");
    }

    #[test]
    fn slugify_non_latin_topics_get_a_stable_hash_slug() {
        // FNV-1a 32 of the trimmed, lowercased input: pinned values, so a slug
        // naming files on disk never changes between builds.
        assert_eq!(slugify_topic("日本語"), "t-805f5ce7");
        assert_eq!(slugify_topic("Привет мир"), "t-8113ba70");
    }

    #[test]
    fn slugify_symbols_and_ascii_stopwords_only_stay_general() {
        assert_eq!(slugify_topic("!!!"), "general");
        assert_eq!(slugify_topic("the learn"), "general");
    }

    #[test]
    fn slugify_caps_at_three_content_words() {
        assert_eq!(slugify_topic("alfa beta gama delta"), "alfa-beta-gama");
    }

    #[test]
    fn slugify_strips_stopwords_from_sentence() {
        // "ben ... ile bir ... yapmak istiyorum" — filler words are dropped.
        assert_eq!(
            slugify_topic("ben rust ile bir todo uygulaması yapmak istiyorum"),
            "rust-todo-uygulamasi"
        );
        assert_eq!(slugify_topic("Rust öğreniyorum"), "rust");
    }

    #[test]
    fn slugify_blank_input_falls_back_to_general() {
        assert_eq!(slugify_topic("   "), "general");
        assert_eq!(slugify_topic(""), "general");
    }

    #[test]
    fn finalize_slug_uses_model_reply_then_slugifies() {
        // Model returns a hyphenated slug → hyphens are preserved, slugify guarantees it.
        assert_eq!(
            finalize_slug("ben golang öğrenmek istiyorum", "golang-web"),
            "golang-web"
        );
        // If the model returns noisy output it still gets slugified.
        assert_eq!(finalize_slug("x", "Rust Todo"), "rust-todo");
    }

    #[test]
    fn finalize_slug_falls_back_to_raw_when_model_gives_genel() {
        // If the model says "general", derive a local slug from the raw input instead.
        assert_eq!(
            finalize_slug("temel linux güvenliği", "general"),
            "temel-linux-guvenligi"
        );
    }

    #[test]
    fn slug_system_injects_known_topics() {
        let s = slug_system(&["linux-guvenlik".to_string(), "rust".to_string()]);
        assert!(s.contains("linux-guvenlik, rust"));
        assert!(s.contains("CONTINUE"));
    }

    #[test]
    fn slug_system_without_topics_is_base_only() {
        let s = slug_system(&[]);
        assert!(s.contains("slug"));
        assert!(!s.contains("Existing topics"));
    }

    #[test]
    fn interpret_empty_resumes_latest_or_swallows() {
        let local = vec!["son-konu".to_string(), "eski".to_string()];
        assert!(
            matches!(interpret_topic_input("", &local, false), Some(TopicChoice::Resume(t)) if t == "son-konu")
        );
        assert!(interpret_topic_input("  ", &[], false).is_none()); // no topic → swallow
    }

    #[test]
    fn interpret_digit_selects_from_list_out_of_range_is_new() {
        let local = vec!["a".to_string(), "b".to_string()];
        assert!(
            matches!(interpret_topic_input("2", &local, false), Some(TopicChoice::Resume(t)) if t == "b")
        );
        assert!(
            matches!(interpret_topic_input("5", &local, false), Some(TopicChoice::New(r)) if r == "5")
        );
    }

    #[test]
    fn interpret_existing_slug_match_resumes() {
        let local = vec!["linux-guvenlik".to_string()];
        // Slugify match: Turkish spelling is caught too.
        assert!(matches!(
            interpret_topic_input("Linux Güvenlik", &local, false),
            Some(TopicChoice::Resume(t)) if t == "linux-guvenlik"
        ));
    }

    #[test]
    fn interpret_resume_phrases_short_input_only() {
        let local = vec!["son-konu".to_string()];
        for s in [
            "devam",
            "devam edelim",
            "kaldığımız yerden devam",
            "continue",
            "resume",
        ] {
            assert!(
                matches!(interpret_topic_input(s, &local, false), Some(TopicChoice::Resume(t)) if t == "son-konu"),
                "{s}"
            );
        }
        // >4 words → goes to the LLM/new-topic flow (K2 catches it).
        assert!(matches!(
            interpret_topic_input("devam edelim ama bu sefer docker öğrenelim", &local, false),
            Some(TopicChoice::New(_))
        ));
        // Resume pattern but no topic exists → new topic.
        assert!(matches!(
            interpret_topic_input("devam", &[], false),
            Some(TopicChoice::New(_))
        ));
    }

    #[test]
    fn interpret_other_input_is_new() {
        let local = vec!["son-konu".to_string()];
        assert!(
            matches!(interpret_topic_input("docker compose", &local, false), Some(TopicChoice::New(r)) if r == "docker compose")
        );
    }

    #[test]
    fn empty_enter_suggests_when_no_local_topics_and_project_known() {
        assert!(matches!(
            interpret_topic_input("", &[], true),
            Some(TopicChoice::Suggest)
        ));
        assert!(matches!(
            interpret_topic_input("  ", &[], true),
            Some(TopicChoice::Suggest)
        ));
    }

    #[test]
    fn empty_enter_resume_beats_suggest_when_local_exists() {
        let local = vec!["rust".to_string()];
        assert!(matches!(
            interpret_topic_input("", &local, true),
            Some(TopicChoice::Resume(t)) if t == "rust"
        ));
    }

    #[test]
    fn empty_enter_without_project_stays_none() {
        assert!(interpret_topic_input("", &[], false).is_none());
    }

    #[test]
    fn new_topic_confirm_msg_names_slug_and_keys() {
        let m = new_topic_confirm_msg("rust-cli");
        assert!(m.contains("rust-cli"));
        assert!(m.contains("(yes/no)"));
        assert!(!m.contains("evet")); // Turkish variants silently accepted, never advertised
    }

    #[test]
    fn parse_topic_marker_extracts_slug_and_display() {
        let reply =
            "Great — let's begin with ownership.\nFirst step: cargo new.\nTOPIC: rust-ownership";
        let (slug, display) = parse_topic_marker(reply).unwrap();
        assert_eq!(slug, "rust-ownership");
        assert!(display.contains("First step"));
        assert!(!display.contains("TOPIC:"));
    }

    #[test]
    fn parse_topic_marker_normalizes_messy_slug() {
        let (slug, _) = parse_topic_marker("ok\nTOPIC: Rust Temelleri!").unwrap();
        assert_eq!(slug, "rust-temelleri");
    }

    #[test]
    fn parse_topic_marker_only_matches_final_line() {
        // Marker mid-reply is NOT a lock — only the final line counts.
        assert!(parse_topic_marker("TOPIC: rust\nbut then more prose").is_none());
    }

    #[test]
    fn parse_topic_marker_rejects_blank_or_missing() {
        assert!(parse_topic_marker("no marker here").is_none());
        // Blank slug must be None BEFORE slugify_topic's "general" fallback kicks in.
        assert!(parse_topic_marker("text\nTOPIC:   ").is_none());
    }

    #[test]
    fn parse_topic_marker_accepts_marker_only_reply() {
        let (slug, display) = parse_topic_marker("TOPIC: golang-web").unwrap();
        assert_eq!(slug, "golang-web");
        assert_eq!(display, "");
    }

    #[test]
    fn topic_arg_slug_keeps_an_existing_slug() {
        assert_eq!(
            topic_arg_slug("temel-linux-guvenligi"),
            "temel-linux-guvenligi"
        );
        assert_eq!(topic_arg_slug("t-805f5ce7"), "t-805f5ce7");
        assert_eq!(topic_arg_slug("  rust-async "), "rust-async");
        assert_eq!(topic_arg_slug("C++"), "cpp");
        assert_eq!(topic_arg_slug("Temel Linux"), "temel-linux");
        assert_eq!(topic_arg_slug("a--b"), slugify_topic("a--b"));
    }
}
