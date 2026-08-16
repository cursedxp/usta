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

/// System prompt for the one-shot start suggestion (spec: project-aware start).
/// Mirrors the slug mini-session: single call, session reset afterwards.
pub(crate) fn start_suggest_system() -> String {
    "You are Usta, a Socratic engineering mentor. The user has a project \
     definition (given in the user message) but does NOT know where to start \
     learning. Propose the single best starting topic. Reply in the language \
     of the project file. FIRST line must be exactly `KONU: <topic-slug>` \
     (lowercase, hyphenated, 1-3 words). Then 2-4 sentences: why this topic \
     first, and ONE concrete first step small enough to start today. No \
     greeting, no markdown headings, nothing after the suggestion."
        .to_string()
}

/// Parse the suggestion reply: first `KONU:` line → slug (normalized through
/// slugify_topic), remaining lines → suggestion text shown to the user.
/// No `KONU:` marker or empty slug → None (caller falls back to manual entry).
pub(crate) fn parse_start_suggestion(reply: &str) -> Option<(String, String)> {
    let mut lines = reply.trim().lines();
    let first = lines.next()?.trim();
    let rest_raw = first.strip_prefix("KONU:")?;
    // `slugify_topic` never returns an empty string — it falls back to
    // tokens::DEFAULT_TOPIC for empty/whitespace input. So the emptiness check MUST
    // happen here, before slugify_topic runs, or a blank `KONU:` line would wrongly
    // parse to Some((tokens::DEFAULT_TOPIC, ...)) instead of None.
    if rest_raw.trim().is_empty() {
        return None;
    }
    // `slugify_topic` splits on whitespace only, so a hyphen already inside
    // the KONU value (e.g. "rust-temelleri") would otherwise be stripped and
    // the words glued together ("rusttemelleri"). Turn hyphens/underscores
    // into spaces first, same trick `finalize_slug` uses for model replies.
    let slug = slugify_topic(&rest_raw.replace(['-', '_'], " "));
    let text = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    Some((slug, text))
}

/// New-topic confirmation text (for TUI tui_confirm). The plain path uses its own
/// `[y/N]` rustyline format — the wording is deliberately different, the two surfaces are separate.
/// Display advertises `y` only; `e` stays silently accepted (tui_confirm key match).
pub(crate) fn new_topic_confirm_msg(slug: &str) -> String {
    format!("new topic: {slug} — open it? [y = yes / any other key = go back]")
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
pub(crate) fn interpret_topic_input(raw: &str, local: &[String], project_known: bool) -> Option<TopicChoice> {
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
        let d: String = raw.chars().map(deasciify).collect::<String>().to_lowercase();
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

/// Turn free text into a topic slug — pure function, testable.
/// Rule: simplify Turkish characters, lowercase, take at most the FIRST 3
/// words, keep only ascii alphanumeric characters in each word, join words
/// with hyphens. Empty result → `tokens::DEFAULT_TOPIC`.
/// "temel Linux güvenliği" → `temel-linux-guvenligi`.
pub fn slugify_topic(input: &str) -> String {
    // Filler words, compared against their deasciified (ç→c…) form — kept out
    // of the slug, so "ben rust ile bir todo yapmak istiyorum" → "rust-todo".
    const STOPWORDS: &[&str] = &[
        "ben", "bir", "ile", "ve", "icin", "bu", "su", "yapmak", "yapmayi",
        "istiyorum", "ogrenmek", "ogreniyorum", "istiyor", "bana", "de", "da",
        "the", "a", "an", "to", "learn", "want", "make", "build",
    ];
    let words: Vec<String> = input
        .split_whitespace()
        .map(|w| {
            w.chars()
                .map(deasciify)
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
        })
        .filter(|w| !w.is_empty() && !STOPWORDS.contains(&w.as_str()))
        .take(3)
        .collect();
    if words.is_empty() {
        tokens::DEFAULT_TOPIC.to_string()
    } else {
        words.join("-")
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
        assert_eq!(slugify_topic("temel Linux güvenliği"), "temel-linux-guvenligi");
        assert_eq!(slugify_topic("todo app"), "todo-app");
    }

    #[test]
    fn slugify_drops_non_alnum_chars() {
        assert_eq!(slugify_topic("C++"), "c");
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
        assert_eq!(finalize_slug("ben golang öğrenmek istiyorum", "golang-web"), "golang-web");
        // If the model returns noisy output it still gets slugified.
        assert_eq!(finalize_slug("x", "Rust Todo"), "rust-todo");
    }

    #[test]
    fn finalize_slug_falls_back_to_raw_when_model_gives_genel() {
        // If the model says "general", derive a local slug from the raw input instead.
        assert_eq!(finalize_slug("temel linux güvenliği", "general"), "temel-linux-guvenligi");
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
    fn start_suggest_system_defines_konu_contract() {
        let s = start_suggest_system();
        assert!(s.contains("KONU:"));
        assert!(s.contains("first step"));
    }

    #[test]
    fn parse_start_suggestion_splits_slug_and_text() {
        let reply = "KONU: rust-temelleri\nStart with Rust because the backend is Rust.\nFirst step: cargo new.";
        let (slug, text) = parse_start_suggestion(reply).unwrap();
        assert_eq!(slug, "rust-temelleri");
        assert!(text.contains("First step"));
        assert!(!text.contains("KONU:"));
    }

    #[test]
    fn parse_start_suggestion_normalizes_messy_slug_line() {
        let (slug, _) = parse_start_suggestion("KONU: Rust Temelleri!\ngerekçe").unwrap();
        assert_eq!(slug, "rust-temelleri");
    }

    #[test]
    fn parse_start_suggestion_tolerates_missing_text_rejects_missing_konu() {
        let (slug, text) = parse_start_suggestion("KONU: rust").unwrap();
        assert_eq!(slug, "rust");
        assert_eq!(text, "");
        assert!(parse_start_suggestion("just prose, no marker").is_none());
        assert!(parse_start_suggestion("KONU:   \ntext").is_none());
    }

    #[test]
    fn interpret_empty_resumes_latest_or_swallows() {
        let local = vec!["son-konu".to_string(), "eski".to_string()];
        assert!(matches!(interpret_topic_input("", &local, false), Some(TopicChoice::Resume(t)) if t == "son-konu"));
        assert!(interpret_topic_input("  ", &[], false).is_none()); // no topic → swallow
    }

    #[test]
    fn interpret_digit_selects_from_list_out_of_range_is_new() {
        let local = vec!["a".to_string(), "b".to_string()];
        assert!(matches!(interpret_topic_input("2", &local, false), Some(TopicChoice::Resume(t)) if t == "b"));
        assert!(matches!(interpret_topic_input("5", &local, false), Some(TopicChoice::New(r)) if r == "5"));
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
        for s in ["devam", "devam edelim", "kaldığımız yerden devam", "continue", "resume"] {
            assert!(matches!(interpret_topic_input(s, &local, false), Some(TopicChoice::Resume(t)) if t == "son-konu"), "{s}");
        }
        // >4 words → goes to the LLM/new-topic flow (K2 catches it).
        assert!(matches!(
            interpret_topic_input("devam edelim ama bu sefer docker öğrenelim", &local, false),
            Some(TopicChoice::New(_))
        ));
        // Resume pattern but no topic exists → new topic.
        assert!(matches!(interpret_topic_input("devam", &[], false), Some(TopicChoice::New(_))));
    }

    #[test]
    fn interpret_other_input_is_new() {
        let local = vec!["son-konu".to_string()];
        assert!(matches!(interpret_topic_input("docker compose", &local, false), Some(TopicChoice::New(r)) if r == "docker compose"));
    }

    #[test]
    fn empty_enter_suggests_when_no_local_topics_and_project_known() {
        assert!(matches!(
            interpret_topic_input("", &[], true),
            Some(TopicChoice::Suggest)
        ));
        assert!(matches!(interpret_topic_input("  ", &[], true), Some(TopicChoice::Suggest)));
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
        assert!(m.contains("[y"));
        assert!(!m.contains("[e")); // e silently accepted, never advertised
    }
}
