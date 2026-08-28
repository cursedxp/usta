//! Puts a question to the user and waits: the live model turn (`ask_live`) and
//! the single-key confirm (`tui_confirm`) — extracted from `run.rs` (cleanup
//! round, Task 5).

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode};
use futures_util::StreamExt;

use crate::anthropic::Message;
use crate::backend::Backend;
use crate::tui::editor::{Action, InputBox};
use crate::tui::status::Status;
use crate::tui::term::Tui;

/// Result of ask_live: either a reply arrived or the user cancelled with double Ctrl-C.
pub(crate) enum AskOutcome {
    Reply(crate::backend::Reply),
    Cancelled,
}

/// Wait for the LLM call with a live interface: spinner spins, keys are processed
/// by the editor but Submit/Exit are LOCKED (single-turn principle) — Enter is
/// swallowed. Can be cancelled with double Ctrl-C (or Ctrl-D): the first press
/// lights up a hint on the status line, the second drops the future
/// (kill_on_drop kills the child).
pub(crate) async fn ask_live(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
    backend: &mut Backend,
    system: &str,
    history: &[Message],
    tokens: Option<u64>,
) -> Result<AskOutcome> {
    let window = backend.context_window();
    let fut = backend.complete(system, history);
    tokio::pin!(fut);
    let mut frame = 0usize;
    let mut cancel_armed = false; // true after the first Ctrl-C — the counter doesn't reset (spec B2)
    loop {
        crate::tui::page::draw(
            tui,
            editor,
            &Status::Thinking {
                frame,
                cancel_hint: cancel_armed,
            },
            tokens,
            window,
            None,
            false,
        )?;
        tokio::select! {
            r = &mut fut => return Ok(AskOutcome::Reply(r?)),
            Some(Ok(ev)) = events.next() => {
                // Paste is still processed by the editor even while locked (doesn't submit).
                if let Event::Paste(s) = &ev {
                    editor.insert_str(s);
                } else if let Event::Resize(_, _) = ev {
                    crate::tui::page::handle_resize(tui)?;
                } else if let Event::Key(k) = ev {
                    // Single Esc = instant cancel (drops fut → kill_on_drop kills the child).
                    if matches!(k.code, KeyCode::Esc) {
                        return Ok(AskOutcome::Cancelled);
                    }
                    match crate::tui::paint::classify_locked_key(k) {
                        crate::tui::paint::LockedKey::CancelRequest if cancel_armed => {
                            // fut drops → kill_on_drop kills the child (backend.rs).
                            return Ok(AskOutcome::Cancelled);
                        }
                        crate::tui::paint::LockedKey::CancelRequest => { cancel_armed = true; }
                        crate::tui::paint::LockedKey::Edit => {
                            if !matches!(k.code, KeyCode::Enter) {
                                let _ = match editor.handle_key(k) {
                                    Action::Exit => Action::None, // never reached here (CancelRequest catches it) — safety net
                                    other => other,
                                };
                            }
                        }
                    }
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(120)) => { frame += 1; }
        }
    }
}

/// Interpret a typed confirmation line. `None` = not an answer — ask again.
/// Only an explicit yes/no (English or Turkish, full word or initial) decides;
/// a bare Enter or a stray word must never cancel (v0.24.7).
pub(crate) fn parse_confirm_answer(line: &str) -> Option<bool> {
    match line.trim().to_lowercase().as_str() {
        "yes" | "y" | "evet" | "e" => Some(true),
        "no" | "n" | "hayır" | "hayir" | "h" => Some(false),
        _ => None,
    }
}

/// Typed confirmation in the TUI: print the message, then read a full line —
/// the user types yes/no and presses Enter (`parse_confirm_answer`). An
/// unrecognized answer re-prompts instead of cancelling; Esc/Ctrl-C cancels.
pub(crate) async fn tui_confirm(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
    msg: &str,
) -> Result<bool> {
    crate::tui::page::page_notice(tui, msg)?;
    loop {
        crate::tui::page::draw(tui, editor, &Status::Idle, None, 0, None, false)?;
        match events.next().await {
            Some(Ok(Event::Key(k))) => {
                if matches!(k.code, KeyCode::Esc) {
                    editor.clear();
                    return Ok(false);
                }
                match editor.handle_key(k) {
                    Action::Submit(line) => match parse_confirm_answer(&line) {
                        Some(v) => return Ok(v),
                        None => crate::tui::page::page_notice(tui, "please type yes or no")?,
                    },
                    Action::Exit => {
                        editor.clear();
                        return Ok(false);
                    }
                    Action::None => {}
                }
            }
            Some(Ok(Event::Paste(s))) => editor.insert_str(&s),
            Some(Ok(Event::Resize(_, _))) => crate::tui::page::handle_resize(tui)?,
            Some(Ok(_)) | Some(Err(_)) => {} // other events — ignore
            None => return Ok(false),        // stream ended — don't spin in a hot loop (spec B4)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_confirm_answer;

    #[test]
    fn parse_confirm_accepts_typed_yes_no_variants() {
        for yes in ["yes", "y", "YES", " Yes ", "evet", "e", "E"] {
            assert_eq!(parse_confirm_answer(yes), Some(true), "{yes:?}");
        }
        for no in ["no", "n", "NO", " No ", "hayır", "hayir", "h", "H"] {
            assert_eq!(parse_confirm_answer(no), Some(false), "{no:?}");
        }
    }

    #[test]
    fn parse_confirm_reasks_on_empty_or_unrecognized() {
        // A stray key or bare Enter must never decide — the accidental-cancel
        // bug this replaces (v0.24.7): any wrong key used to mean "no".
        for other in ["", "  ", "x", "yep", "nope", "evt", "q"] {
            assert_eq!(parse_confirm_answer(other), None, "{other:?}");
        }
    }
}
