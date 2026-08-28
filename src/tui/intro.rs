//! Pre-lock conversation core (SPEC §4.22): the first-run introduction and the
//! conversational start suggestion both run BEFORE a topic exists. On the
//! model's `TOPIC:` line the caller locks the slug, and `stitch` replays the
//! turns into the real session + transcript — the introduction IS the head of
//! the session, not a separate phase (that is what closes blocker H2: by any
//! flush, the topic exists, so the ordinary closing contract owns the output).
//!
//! The two branch bodies (`run_first_run`, `run_suggest`) live here rather than
//! inline in `run.rs` so that file stays inside its 600-line budget — `run.rs`
//! keeps only the call sites.

use std::path::Path;

use anyhow::Result;
use crossterm::event::{Event, EventStream};
use futures_util::StreamExt;

use crate::anthropic::Message;
use crate::backend::Backend;
use crate::session::Session;
use crate::transcript::Recorder;
use crate::tui::editor::{Action, InputBox};
use crate::tui::status::Status;
use crate::tui::term::Tui;

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

/// Wait for one submitted user line at the intro prompt. `None` = quit
/// (Esc/Ctrl-C via `Action::Exit`, or the event stream ended). Empty Enter is
/// swallowed — there is no resume sentinel before a topic exists. Modeled on
/// `entry::ask_topic`'s event loop.
async fn read_user_line(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
) -> Result<Option<String>> {
    loop {
        crate::tui::page::draw(tui, editor, &Status::Idle, None, 0, None)?;
        match events.next().await {
            Some(Ok(Event::Key(k))) => match editor.handle_key(k) {
                Action::Submit(line) => {
                    let raw = line.trim().to_string();
                    if raw.is_empty() {
                        continue;
                    }
                    return Ok(Some(raw));
                }
                Action::Exit => return Ok(None),
                Action::None => {}
            },
            Some(Ok(Event::Paste(s))) => editor.insert_str(&s),
            Some(Ok(Event::Resize(_, _))) => crate::tui::page::handle_resize(tui)?,
            Some(Ok(_)) | Some(Err(_)) => {}
            None => return Ok(None), // stream ended — don't spin (spec B4)
        }
    }
}

/// What to page for one intro reply: strip a trailing `[[show: ...]]` marker
/// from the DISPLAYED text, using the same `extract_show_marker` helper
/// every other reply path uses (SOUL.md contract) — a model may end a reply
/// with the marker mid-introduction just as well as post-lock. The marker is
/// NOT acted on here: launching the visual explainer during the pre-lock
/// introduction is out of scope for this stage, it is simply not shown.
/// `text` is either `parse_topic_marker`'s `display` (TOPIC: line already
/// stripped) or the raw reply — `run_intro`'s `turns` always keeps the RAW
/// reply, this only affects the screen.
fn strip_show_for_display(text: &str) -> String {
    crate::visual::extract_show_marker(text).0
}

/// The pre-lock conversation loop (SPEC §4.22). `opening` is the hidden first
/// user turn (`introduction_prompt` or `start_here_prompt`); the model speaks
/// first. Every model reply is checked for the final-line `TOPIC:` marker —
/// on a hit the RAW reply stays in the turns (role alternation is preserved
/// even for a marker-only reply) while only the stripped text is displayed.
/// A failed/cancelled FIRST call returns `Fallback` (manual topic entry);
/// mid-conversation failures wait for the user instead of aborting.
pub(crate) async fn run_intro(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
    backend: &mut Backend,
    system: &str,
    opening: &str,
) -> Result<IntroOutcome> {
    let mut turns = vec![IntroTurn {
        user: true,
        text: opening.to_string(),
    }];
    loop {
        match crate::tui::ask::ask_live(
            tui,
            editor,
            events,
            backend,
            system,
            &messages(&turns),
            None,
        )
        .await
        {
            Ok(crate::tui::ask::AskOutcome::Reply(reply)) => {
                let w = crate::tui::page::current_width(tui);
                match crate::topic::parse_topic_marker(&reply.text) {
                    Some((slug, display)) => {
                        let clean = strip_show_for_display(&display);
                        if !clean.is_empty() {
                            crate::tui::page::page_reply(tui, &clean, w)?;
                        }
                        turns.push(IntroTurn {
                            user: false,
                            text: reply.text,
                        });
                        return Ok(IntroOutcome::Topic { slug, turns });
                    }
                    None => {
                        let clean = strip_show_for_display(&reply.text);
                        crate::tui::page::page_reply(tui, &clean, w)?;
                        turns.push(IntroTurn {
                            user: false,
                            text: reply.text,
                        });
                    }
                }
            }
            Ok(crate::tui::ask::AskOutcome::Cancelled) | Err(_) if turns.len() == 1 => {
                backend.reset_session();
                return Ok(IntroOutcome::Fallback);
            }
            Ok(crate::tui::ask::AskOutcome::Cancelled) => {
                // The turns list stays as-is (the user keeps typing); the CLI
                // session is half-done — don't resume it, the next call should
                // go with the full transcript (run.rs main-loop precedent).
                backend.reset_session();
                crate::tui::page::page_notice(tui, "cancelled — keep typing, or /quit")?;
            }
            Err(e) => {
                // Same reasoning as the Cancelled arm above: the CLI session is
                // half-done, so reset uniformly rather than rely on backend.rs's
                // partial self-heal on the error path.
                backend.reset_session();
                crate::tui::page::page_error(tui, &format!("error: {e}"))?
            }
        }
        // User turn.
        loop {
            let Some(raw) = read_user_line(tui, editor, events).await? else {
                return Ok(IntroOutcome::Quit);
            };
            if crate::help::is_help_command(&raw) {
                crate::tui::page::page_notice(tui, crate::help::help_text())?;
                continue;
            }
            if raw.eq_ignore_ascii_case("/quit") {
                return Ok(IntroOutcome::Quit);
            }
            if crate::visual::parse_show_command(&raw).is_some()
                || crate::slash::parse_watch_command(&raw).is_some()
            {
                crate::tui::page::page_notice(
                    tui,
                    "that command works inside a session — we're still getting acquainted",
                )?;
                continue;
            }
            crate::tui::page::page_user_echo(tui, &raw)?;
            turns.push(IntroTurn {
                user: true,
                text: raw,
            });
            break;
        }
    }
}

/// Materials side of both pre-lock branches: convert any new PDFs (notices go
/// to the screen) and return the digest that anchors the opening prompt. The
/// `else` branch of the ordinary opening turn does exactly this — the intro
/// paths run it here instead, because they skip that turn.
fn materials_digest(tui: &mut Tui, project_root: &Path) -> Result<Option<String>> {
    for note in crate::materials::convert_pdfs(project_root) {
        crate::tui::page::page_notice(tui, &note)?;
    }
    Ok(crate::materials::combined_digests(&crate::materials::scan(
        project_root,
    )))
}

/// Print the outcome's notice/error and hand the outcome back unchanged.
fn report(tui: &mut Tui, outcome: IntroOutcome, failed: &str) -> Result<IntroOutcome> {
    match &outcome {
        IntroOutcome::Topic { slug, .. } => {
            crate::tui::page::page_notice(tui, &format!("topic: {slug}"))?
        }
        IntroOutcome::Fallback => crate::tui::page::page_error(tui, failed)?,
        IntroOutcome::Quit => {}
    }
    Ok(outcome)
}

/// First run ever (no marker, no evidence): the introduction REPLACES the topic
/// prompt — the identity welcome is printed without its question line and the
/// model opens the conversation; its `TOPIC:` line locks the slug. `Fallback`
/// leaves the caller's manual entry loop in charge.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_first_run(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
    backend: &mut Backend,
    global: &Path,
    project_root: &Path,
    today: &str,
    local: &[String],
    other: &[String],
    project_known: bool,
    week_sessions: u32,
    streak: u32,
) -> Result<IntroOutcome> {
    let profile = std::fs::read_to_string(global.join("USER.md")).ok();
    crate::tui::entry::print_identity_welcome(
        tui,
        profile.as_deref(),
        &backend.label(),
        &crate::tui::paint::short_dir(project_root),
        local,
        other,
        project_known,
        week_sessions,
        streak,
        false,
        true,
    )?;
    let digest = materials_digest(tui, project_root)?;
    let system = intro_system(global, project_root, today);
    let opening = crate::progress::introduction_prompt(project_known, digest.as_deref());
    let outcome = run_intro(tui, editor, events, backend, &system, &opening).await?;
    // NOTE: the "introduction completed" marker is NOT written here — it moves
    // to run.rs, after the lock is acquired and the session is built, so a
    // rejected lock-conflict confirmation (no session, no lock) doesn't strand
    // the marker on disk with nothing to show for it (SPEC §4.22 review).
    report(tui, outcome, "introduction failed — type a topic")
}

/// Conversational start suggestion: a returning user pressed Enter with a
/// filled `mentor/PROJECT.md`. Runs on the FULL brain — profile and PROJECT.md
/// ride along — and agreement happens in conversation instead of a yes/no gate,
/// so "no, something easier" now works.
pub(crate) async fn run_suggest(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
    backend: &mut Backend,
    global: &Path,
    project_root: &Path,
    today: &str,
) -> Result<IntroOutcome> {
    let digest = materials_digest(tui, project_root)?;
    let system = intro_system(global, project_root, today);
    let opening = crate::progress::start_here_prompt(digest.as_deref());
    let outcome = run_intro(tui, editor, events, backend, &system, &opening).await?;
    report(tui, outcome, "suggestion failed — type a topic")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;
    use crate::transcript::Recorder;

    #[test]
    fn strip_show_for_display_strips_trailing_show_marker() {
        let stripped = strip_show_for_display("Let's start.\n[[show: tcp handshake]]");
        assert_eq!(stripped, "Let's start.");
    }

    #[test]
    fn strip_show_for_display_leaves_text_without_marker_untouched() {
        assert_eq!(strip_show_for_display("no marker here"), "no marker here");
    }

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
        let turns = sample_turns();
        stitch(&turns, &mut session, &recorder);
        // EVERY replayed role is pinned on BOTH surfaces, and the transcript is
        // parsed rather than counted — a corrupted role on the final turn used
        // to slip through the old count-only assertions.
        let want: Vec<&str> = turns
            .iter()
            .map(|t| if t.user { "user" } else { "assistant" })
            .collect();
        let history: Vec<&str> = session.history().iter().map(|m| m.role.as_str()).collect();
        assert_eq!(history, want);
        let raw = std::fs::read_to_string(dir.join("rec.jsonl")).unwrap();
        let logged: Vec<serde_json::Value> = raw
            .lines()
            .map(|l| serde_json::from_str(l).expect("transcript line is JSON"))
            .collect();
        let roles: Vec<&str> = logged
            .iter()
            .map(|v| v["role"].as_str().expect("transcript line has a role"))
            .collect();
        assert_eq!(roles, want);
        let texts: Vec<&str> = logged.iter().map(|v| v["text"].as_str().unwrap()).collect();
        let sent: Vec<&str> = turns.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, sent);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Wiring pin (polite.rs H3 precedent): if the entry path stops calling the
    /// introduction, this test fails instead of the flow silently disappearing.
    /// Asserted against `run.rs` only — the branch BODIES live in this file
    /// (`run_first_run`/`run_suggest`, because run.rs is at its 600-line
    /// budget) and a self-`include_str!` would be satisfied by the needle list
    /// below. The in-module wiring (`run_intro`, `introduction_prompt`,
    /// `start_here_prompt`) is pinned mechanically instead: each has exactly
    /// one caller, so dropping it turns the callee into dead code and trips
    /// the zero-warning clippy rule. `mark_intro_done` is explicitly needled
    /// below instead — it has a SECOND caller (the seeding path in
    /// `setup::intro_needed`), so losing the run.rs call would NOT produce a
    /// dead-code warning; that silent gap is exactly what re-runs the
    /// introduction on every launch (SPEC §4.22 review, Fix 1). The needle
    /// pins its POSITION, not just its presence (Fix 3): a bare
    /// `"mark_intro_done"` substring needle stays green even if the call
    /// moves back above the lock-conflict confirmation, which re-opens the
    /// bug commit 8bd72ba fixed — a user who declines that confirmation gets
    /// no session but does get the marker, and is never introduced. Pinning
    /// the exact contiguous text from `build_session` through the
    /// `mark_intro_done` call means any code moved in between (or the call
    /// relocated elsewhere) breaks the match.
    #[test]
    fn run_wires_intro_flow() {
        let src = include_str!("run.rs");
        for needle in [
            "intro_needed",
            "run_first_run",
            "run_suggest",
            "IntroOutcome",
            // Pins the STITCH-PATH reset specifically (Fix 3): "reset_session()"
            // alone occurs 4 times in run.rs, 3 of them pre-existing and
            // unrelated — a needle that only matches the substring would stay
            // green even if this exact reset were deleted, which would leave
            // the CLI backend resuming under the pre-lock system prompt.
            "crate::tui::intro::stitch(turns, &mut session, &recorder);\n            backend.reset_session();",
            // Pins mark_intro_done AFTER build_session (Fix 3, see doc comment
            // above): the doc comment between them is part of the needle on
            // purpose, so an edit that merely reorders statements without
            // touching the comment still breaks this exact contiguous match.
            "let (mut session, recorder, lock, has_progress) =\n        crate::lifecycle::build_session(global, project_root, &topic, today)?;\n\n    // The first-run introduction only counts as completed once a real session\n    // exists — writing this any earlier (e.g. the moment the model returns a\n    // topic) would strand the marker on disk if the lock-conflict confirmation\n    // above is declined, and the user would never be introduced (SPEC §4.22\n    // review, Fix 1).\n    if first_run_completed {\n        crate::setup::mark_intro_done(global, \"completed\");\n    }",
        ] {
            assert!(
                src.contains(needle),
                "run.rs lost its intro wiring: {needle}"
            );
        }
        // The one-shot suggestion must be GONE from run.rs.
        assert!(!src.contains("start_suggest_system"));
        assert!(!src.contains("parse_start_suggestion"));
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
