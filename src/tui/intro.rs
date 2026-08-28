//! Pre-lock conversation core (SPEC §4.22): the first-run introduction and the
//! conversational start suggestion both run BEFORE a topic exists. On the
//! model's `TOPIC:` line the caller locks the slug, and `stitch` replays the
//! turns into the real session + transcript — the introduction IS the head of
//! the session, not a separate phase (that is what closes blocker H2: by any
//! flush, the topic exists, so the ordinary closing contract owns the output).
//!
//! Task 5 wires the interactive loop and `run.rs` call sites; until then this
//! module's public surface has no non-test caller (per-item `#[allow]` would
//! just repeat across every item, so the allow is scoped to the module —
//! matching the convention in `src/tui/theme.rs`).
#![allow(dead_code)]

use std::path::Path;

use crate::anthropic::Message;
use crate::session::Session;
use crate::transcript::Recorder;

/// One conversation turn — plain text, so it can be replayed into both the
/// Session history and the transcript without unwrapping serde values.
pub(crate) struct IntroTurn {
    pub user: bool,
    pub text: String,
}

/// How the pre-lock conversation ended.
pub(crate) enum IntroOutcome {
    /// The model emitted the topic marker — lock `slug`, stitch `turns`.
    Topic { slug: String, turns: Vec<IntroTurn> },
    /// The FIRST model call failed or was cancelled — fall back to manual
    /// topic entry (the old "suggestion failed — type a topic" resilience).
    Fallback,
    /// The user quit — no session, no lock, no files (spec H2: quit before
    /// lock writes nothing, mirroring today's quit-at-topic-prompt).
    Quit,
}

/// Turns → backend messages. Assistant text is wrapped exactly like
/// `Session::push_assistant` wraps it (`Value::String`).
pub(crate) fn messages(turns: &[IntroTurn]) -> Vec<Message> {
    turns
        .iter()
        .map(|t| {
            if t.user {
                Message::user(t.text.as_str())
            } else {
                Message::assistant_raw(serde_json::Value::String(t.text.clone()))
            }
        })
        .collect()
}

/// Replay the pre-lock conversation into the real session + transcript.
/// Call AFTER `build_session`; the caller must also `backend.reset_session()`
/// so the refreshed system prompt (real topic) rides the next call — the CLI
/// backend's `--resume` would otherwise keep the pre-lock system.
pub(crate) fn stitch(turns: &[IntroTurn], session: &mut Session, recorder: &Recorder) {
    for t in turns {
        if t.user {
            session.push_user(&t.text);
            recorder.user(&t.text);
        } else {
            session.push_assistant(t.text.clone());
            recorder.assistant(&t.text);
        }
    }
}

/// System prompt for the pre-lock conversation: the FULL brain, loaded with
/// the placeholder topic — no topic-scoped files exist yet. (Accepted edge,
/// SPEC §4.22: a user who really has a topic named like the placeholder leaks
/// that progress into the intro's context; harmless, nothing is written.)
pub(crate) fn intro_system(global: &Path, project_root: &Path, today: &str) -> String {
    crate::brain::load_system_prompt(
        global,
        Some(project_root),
        crate::tokens::DEFAULT_TOPIC,
        today,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;
    use crate::transcript::Recorder;

    fn sample_turns() -> Vec<IntroTurn> {
        vec![
            IntroTurn {
                user: true,
                text: "[FIRST RUN — INTRODUCTION] ...".into(),
            },
            IntroTurn {
                user: false,
                text: "Hi — what brings you here?".into(),
            },
            IntroTurn {
                user: true,
                text: "I want to learn Rust".into(),
            },
        ]
    }

    #[test]
    fn messages_maps_roles_in_order() {
        let msgs = messages(&sample_turns());
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[2].role, "user");
    }

    #[test]
    fn stitch_replays_into_session_and_transcript() {
        let mut session = Session::new("rust", "SYSTEM");
        let dir = std::env::temp_dir().join(format!("usta_intro_stitch_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let recorder = Recorder::new(dir.join("rec.jsonl"));
        stitch(&sample_turns(), &mut session, &recorder);
        assert_eq!(session.history().len(), 3);
        assert_eq!(session.history()[0].role, "user");
        assert_eq!(session.history()[1].role, "assistant");
        let raw = std::fs::read_to_string(dir.join("rec.jsonl")).unwrap();
        assert_eq!(raw.lines().count(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn intro_system_loads_brain_with_default_topic() {
        let dir = std::env::temp_dir().join(format!("usta_intro_sys_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (global, project) = (dir.join("g"), dir.join("p"));
        std::fs::create_dir_all(&global).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(global.join("SOUL.md"), "SOUL-CONTENT").unwrap();
        let sys = intro_system(&global, &project, "2026-08-28");
        assert!(sys.contains("SOUL-CONTENT"));
        assert!(sys.starts_with("===== TODAY =====\n2026-08-28"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
