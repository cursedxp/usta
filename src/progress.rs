//! Persistent memory: at session close we have Usta summarize the session and
//! rewrite `.usta/learner/progress/<topic>.md` with its FULL content.
//! The next session loads this file into the system prompt (brain.rs) → Usta
//! doesn't re-explain what it already knows, it targets the gaps. Implements SPEC §9.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Progress file path for a topic: `<project>/.usta/learner/progress/<topic>.md`.
pub fn progress_path(project_root: &Path, topic: &str) -> PathBuf {
    project_root
        .join(".usta/learner/progress")
        .join(format!("{topic}.md"))
}

/// Topic-specific approach file: `.usta/approaches/<topic>.md`.
pub fn approach_path(project_root: &Path, topic: &str) -> PathBuf {
    project_root.join(".usta/approaches").join(format!("{topic}.md"))
}

/// The topic's curriculum map: `.usta/learner/curriculum/<topic>.md`.
pub fn curriculum_path(project_root: &Path, topic: &str) -> PathBuf {
    project_root
        .join(".usta/learner/curriculum")
        .join(format!("{topic}.md"))
}

/// Closing-reply delimiter — the model starts every file with this.
pub const FILE_DELIM: &str = "===DOSYA:";

/// Introduction instruction appended to opening turns while the profile is empty (spec Ç3a).
const MEET_BLOCK: &str = "\n[PROFILE EMPTY] You don't know the user yet. Introduce \
yourself briefly at the start of the conversation — ask their name, their background \
with this topic, how they like to learn. At most 1-2 questions, not a form; don't \
delay getting into the topic. If the user already introduced themselves, don't ask \
again. What you learn will be written to their profile at session close.\n";

/// Split the closing reply into (name, content) pairs. If there's no delimiter,
/// the whole reply counts as a single "progress" file — keeps the old format backward compatible.
pub fn split_files(reply: &str) -> Vec<(String, String)> {
    if !reply.contains(FILE_DELIM) {
        return vec![("progress".to_string(), clean_markdown_reply(reply))];
    }
    let mut out = Vec::new();
    for chunk in reply.split(FILE_DELIM).skip(1) {
        let Some((header, body)) = chunk.split_once("===") else {
            continue;
        };
        let name = header.trim().to_string();
        if name.is_empty() {
            continue;
        }
        out.push((name, clean_markdown_reply(body)));
    }
    out
}

/// User-turn content for the closing call: the current state of the three files +
/// generation rules. progress is always generated; approach/curriculum are living
/// documents — generated on the first session or when they change (TEACHING.md "Scope Guarding").
pub fn closing_prompt(
    topic: &str,
    progress: Option<&str>,
    approach: Option<&str>,
    curriculum: Option<&str>,
    profile: Option<&str>,
) -> String {
    let p = progress.unwrap_or("(dosya henüz yok)");
    let a = approach.unwrap_or("(dosya henüz yok)");
    let c = curriculum.unwrap_or("(dosya henüz yok)");
    let pr = profile.unwrap_or("(dosya henüz yok)");
    format!(
        "[SESSION CLOSING — FILE UPDATE]\n\
         Task: produce whichever of the files below need updating. Start each file with \
         this line: `===DOSYA: <name>===` (name: progress | approach | curriculum | \
         profile — e.g. if generating the profile, `===DOSYA: profile===`).\n\n\
         Current progress ({topic}):\n---\n{p}\n---\n\n\
         Current approach:\n---\n{a}\n---\n\n\
         Current curriculum:\n---\n{c}\n---\n\n\
         Current profile:\n---\n{pr}\n---\n\n\
         Rules:\n\
         - `progress` is ALWAYS generated. Structure: `# {topic} — İlerleme` heading + \
         `## Seviye` / `## Kapatılanlar` / `## Gap'ler` (PROVE IT) / \
         `## Geri çağırma soruları` (3-5 questions + one-line answer each; drop settled \
         old ones, add new ones from this session) / `## Hata günlüğü` (`type | count | \
         last example`, 3+ repeats = GAP CANDIDATE) / `## İpucu merdiveni` / `## Hedef \
         Durumu` — write ONLY if approach defines a `## Hedef`: time remaining (compute \
         from the TODAY section), map progress (%), pace assessment (on track / at risk \
         / behind + one-line rationale), measurement log (`date | measurement | score` — \
         mock exam, writing assessment, etc.). If there's no goal, don't write this \
         section at all.\n\
         - `approach` is generated only on the first session or if the approach changed \
         this session — a living document that answers the three questions from \
         _default.md (practice / output / feedback). For goal-directed learning, approach \
         includes a `## Hedef` section: what (certificate/level/output), exam/assessment \
         date (YYYY-MM-DD), passing threshold, exam/assessment format.\n\
         - `curriculum` is extracted as the FULL map on the first session (topic/subtopic \
         tree; each item with a `görülmedi/görüldü/oturdu/derinleşildi` status; draw on \
         web research if needed); on later sessions it's generated only if a status \
         changed. An uncovered critical item must stay visible on the map.\n\
         - Don't let the files bloat: if `Kapatılanlar` exceeds 20 items, collapse the \
         oldest into a one-line period summary; in `Hata günlüğü`, remove resolved \
         entries not seen in a long time; keep curriculum sections that haven't changed \
         as-is (don't regenerate them).\n\
         - Don't add anything without evidence from this session; keep the valid \
         information already in the existing files (the user may have hand-edited them \
         — don't overwrite their edits).\n\
         - `profile` is generated only if new/changed permanent information about the \
         user was learned this session: name, background/experience, learning style, \
         preferences, recurring strengths/weaknesses. NO TOPIC KNOWLEDGE — 'learned \
         concept X' is progress's job; 'likes to learn from examples' goes in profile. \
         KEEP the valid information already in the current profile (the user may have \
         hand-edited it), ~1 page cap, merge duplicates. If nothing changed, don't \
         generate this file at all.\n\
         - Write no explanation/greeting outside the delimiter lines; every file is pure \
         markdown."
    )
}

/// Opening-drill turn: if progress exists, Usta speaks first at the start of the
/// session and asks a recall question (testing effect — TEACHING.md "Opening Drill" rule).
/// Where it hooks into main.rs: Task 3 (opening-drill trigger).
pub fn opening_prompt(topic: &str, profile_generic: bool) -> String {
    let meet_block = if profile_generic { MEET_BLOCK } else { "" };
    format!(
        "[SESSION OPENING — RECALL DRILL]\n{meet_block}\
         Topic: {topic}. Pick 2-3 questions from the 'Recall questions' section of your \
         progress file and ASK me — don't answer them yourself, don't explain them. Keep \
         it short: a 2-minute warm-up, then we move to today's work. If progress has no \
         questions, come up with 2 small recall questions suited to my level. When the \
         drill is done, say one sentence from the map: where we are, what's next (your \
         curriculum file is in the system prompt)."
    )
}

/// New-topic introduction turn: no approach + curriculum map yet — Usta
/// derives them through open conversation (TEACHING.md "New Topic Introduction"). Not a fixed
/// form: it's derived from what the user says, direction stays with the user.
pub fn onboarding_prompt(topic: &str, intro: Option<&str>, profile_generic: bool) -> String {
    // The raw text the user typed when opening the topic IS the FIRST ANSWER of the
    // introduction — if it's reduced to a slug and discarded, the model just re-asks
    // what was already said ("I'll set up Coolify for my client, Fedora..." → "what are you after?" disaster).
    let intro_block = match intro {
        Some(s) if !s.trim().is_empty() => format!(
            "\nWhen the user opened the topic, they wrote this — treat it as the FIRST \
             ANSWER of the introduction:\n\
             \"{}\"\n\
             USE this information: don't ask again what they already said; start by \
             picking up on what they said and only ask about what's still missing.\n",
            s.trim()
        ),
        _ => String::new(),
    };
    let meet_block = if profile_generic { MEET_BLOCK } else { "" };
    format!(
        "[NEW TOPIC — INTRODUCTION]\n\
         Topic: {topic}. This topic has no approach or curriculum map yet.\n{intro_block}{meet_block}\
         Have a short, NATURAL introduction — this is not a form: ask at most two \
         questions in a single message, continue based on the answer; don't dump a \
         numbered question list. Find out: what they want to do/learn, what they \
         already have. Whether this is exploration or goal-directed — infer it \
         YOURSELF, don't ask the user using these terms; if it's not clear from what \
         they said, ask one jargon-free question: 'are you preparing for a deadline or \
         exam, or is this just out of curiosity?'. If it's goal-directed, gather the \
         what/date/threshold/format info during the conversation — it will go into the \
         approach's `## Hedef` section; the map is built from the official framework \
         (exam syllabus / exam guide / CEFR) — research it on the web. If you don't \
         know the domain well enough, research it on the web. At session close you'll \
         be asked for the approach + FULL curriculum map CONTENT; the shell writes the \
         files, don't try to write files yourself during the session (Hard Rule 6) — \
         deepen the introduction accordingly but don't turn it into a lecture, keep it \
         short."
    )
}

/// Strip any ```-fence wrapper from the model's reply — pure markdown is written to the file.
pub fn clean_markdown_reply(reply: &str) -> String {
    let t = reply.trim();
    if let Some(rest) = t.strip_prefix("```") {
        // First line is the fence tag (```markdown etc.) — drop it.
        let body = rest.split_once('\n').map(|(_, b)| b).unwrap_or("");
        let body = body.trim_end();
        let body = body.strip_suffix("```").unwrap_or(body);
        return body.trim().to_string();
    }
    t.to_string()
}

/// Atomic write: write to tmp, then move it over the target — never leaves a half-written file.
pub fn write_atomic(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory: {}", parent.display()))?;
    }
    // Back up the previous version — a bad model output can be restored from the single copy.
    if path.exists() {
        let bak = path.with_extension("md.bak");
        let _ = std::fs::copy(path, &bak);
    }
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, content)
        .with_context(|| format!("failed to write: {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("failed to move: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn progress_path_builds_expected_layout() {
        let p = progress_path(Path::new("/proje"), "rust");
        assert_eq!(
            p,
            Path::new("/proje/.usta/learner/progress/rust.md")
        );
    }

    #[test]
    fn closing_prompt_embeds_topic_and_existing() {
        let s = closing_prompt("rust", Some("- Seviye: orta"), None, None, None);
        assert!(s.contains("rust"));
        assert!(s.contains("- Seviye: orta"));
    }

    #[test]
    fn closing_prompt_marks_missing_file() {
        let s = closing_prompt("rust", None, None, None, None);
        assert!(s.contains("(dosya henüz yok)"));
    }

    #[test]
    fn closing_prompt_includes_pruning_rule() {
        let s = closing_prompt("rust", None, None, None, None);
        assert!(s.contains("20 items"));
    }

    #[test]
    fn closing_prompt_requests_rich_sections() {
        let s = closing_prompt("rust", None, None, None, None);
        assert!(s.contains("Geri çağırma soruları"));
        assert!(s.contains("Hata günlüğü"));
        assert!(s.contains("İpucu merdiveni"));
    }

    #[test]
    fn paths_build_expected_layout() {
        assert_eq!(
            approach_path(Path::new("/proje"), "gtm"),
            Path::new("/proje/.usta/approaches/gtm.md")
        );
        assert_eq!(
            curriculum_path(Path::new("/proje"), "gtm"),
            Path::new("/proje/.usta/learner/curriculum/gtm.md")
        );
    }

    #[test]
    fn split_files_without_delimiter_is_progress() {
        let out = split_files("# Rust — İlerleme\niçerik");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "progress");
        assert!(out[0].1.contains("içerik"));
    }

    #[test]
    fn split_files_separates_three_files() {
        let reply = "===DOSYA: progress===\nP İÇERİK\n===DOSYA: approach===\nA İÇERİK\n===DOSYA: curriculum===\nC İÇERİK\n";
        let out = split_files(reply);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0], ("progress".to_string(), "P İÇERİK".to_string()));
        assert_eq!(out[1], ("approach".to_string(), "A İÇERİK".to_string()));
        assert_eq!(out[2], ("curriculum".to_string(), "C İÇERİK".to_string()));
    }

    #[test]
    fn split_files_cleans_fenced_content() {
        let reply = "===DOSYA: progress===\n```markdown\n# başlık\n```\n";
        let out = split_files(reply);
        assert_eq!(out[0].1, "# başlık");
    }

    #[test]
    fn closing_prompt_embeds_all_three_currents_and_delimiter() {
        let s = closing_prompt("rust", Some("PMEVCUT"), Some("AMEVCUT"), Some("CMEVCUT"), None);
        assert!(s.contains("PMEVCUT"));
        assert!(s.contains("AMEVCUT"));
        assert!(s.contains("CMEVCUT"));
        assert!(s.contains("===DOSYA:"));
        assert!(s.contains("görülmedi/görüldü/oturdu/derinleşildi"));
    }

    #[test]
    fn closing_prompt_defines_goal_sections() {
        let s = closing_prompt("almanca", None, None, None, None);
        assert!(s.contains("## Hedef Durumu"));
        assert!(s.contains("## Hedef"));
        assert!(s.contains("pace assessment"));
    }

    #[test]
    fn closing_prompt_defines_profile_rules() {
        let s = closing_prompt("rust", None, None, None, Some("CURRENT PROFILE"));
        assert!(s.contains("===DOSYA: profile==="));
        assert!(s.contains("CURRENT PROFILE"));
        assert!(s.contains("NO TOPIC KNOWLEDGE"));
        assert!(s.contains("only")); // generated only if new/changed info exists
    }

    #[test]
    fn onboarding_prompt_carries_user_intro_and_forbids_reasking() {
        let s = onboarding_prompt(
            "hosting",
            Some("müşterimin hesabına coolify kuracağım, Fedora, temel güvenlik lazım"),
            false,
        );
        assert!(s.contains("coolify kuracağım"));
        assert!(s.contains("FIRST ANSWER"));
        assert!(s.contains("don't ask again"));
        // If there's no intro, the block doesn't appear at all.
        let bare = onboarding_prompt("hosting", None, false);
        assert!(!bare.contains("FIRST ANSWER"));
    }

    #[test]
    fn onboarding_prompt_infers_goal_without_jargon_and_limits_questions() {
        let s = onboarding_prompt("almanca", None, false);
        // Exploration/goal terms are NOT asked to the user — the model infers them itself.
        assert!(!s.contains("keşif mi"));
        assert!(s.contains("infer it YOURSELF"));
        // Jargon-free fallback question + question limit.
        assert!(s.contains("a deadline or exam"));
        assert!(s.contains("at most two questions"));
        assert!(s.contains("## Hedef"));
    }

    #[test]
    fn opening_prompt_embeds_topic_and_asks_to_quiz() {
        let s = opening_prompt("rust", false);
        assert!(s.contains("rust"));
        assert!(s.contains("RECALL DRILL"));
        assert!(s.contains("ASK"));
    }

    #[test]
    fn onboarding_prompt_embeds_topic_and_open_conversation() {
        let s = onboarding_prompt("linux-guvenlik", None, false);
        assert!(s.contains("linux-guvenlik"));
        assert!(s.contains("INTRODUCTION"));
        assert!(s.contains("form"));
    }

    #[test]
    fn onboarding_prompt_does_not_tell_model_it_writes_files() {
        // Hard Rule 6: the model has no file-writing tool — it produces the closing
        // content, the shell writes the file. The prompt must not push the model to try writing.
        let s = onboarding_prompt("rust", None, false);
        assert!(!s.contains("you will write files"));
        assert!(s.contains("shell writes"));
    }

    #[test]
    fn opening_prompt_mentions_curriculum_position() {
        let s = opening_prompt("rust", false);
        assert!(s.contains("map"));
    }

    #[test]
    fn opening_prompts_include_meet_block_only_when_profile_generic() {
        let on = onboarding_prompt("rust", None, true);
        assert!(on.contains("[PROFILE EMPTY]"));
        assert!(on.contains("1-2 questions"));
        assert!(!onboarding_prompt("rust", None, false).contains("[PROFILE EMPTY]"));

        let op = opening_prompt("rust", true);
        assert!(op.contains("[PROFILE EMPTY]"));
        assert!(!opening_prompt("rust", false).contains("[PROFILE EMPTY]"));
    }

    #[test]
    fn clean_reply_strips_fenced_block() {
        let raw = "```markdown\n# Rust — İlerleme\n- Seviye: orta\n```";
        assert_eq!(
            clean_markdown_reply(raw),
            "# Rust — İlerleme\n- Seviye: orta"
        );
    }

    #[test]
    fn clean_reply_passes_plain_text_through() {
        assert_eq!(clean_markdown_reply("  # Başlık\niçerik  "), "# Başlık\niçerik");
    }

    #[test]
    fn write_atomic_creates_parents_and_writes() {
        let base = std::env::temp_dir().join(format!(
            "usta_progress_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let target = base.join("derin/dizin/rust.md");
        write_atomic(&target, "içerik").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "içerik");
        // No tmp file should remain.
        assert!(!target.with_extension("md.tmp").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn write_atomic_backs_up_previous_version() {
        let base = std::env::temp_dir().join(format!(
            "usta_progress_bak_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let target = base.join("rust.md");
        write_atomic(&target, "ilk sürüm").unwrap();
        assert!(!target.with_extension("md.bak").exists()); // no backup on the first write
        write_atomic(&target, "ikinci sürüm").unwrap();
        assert_eq!(
            std::fs::read_to_string(target.with_extension("md.bak")).unwrap(),
            "ilk sürüm"
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "ikinci sürüm");
        let _ = std::fs::remove_dir_all(&base);
    }
}
