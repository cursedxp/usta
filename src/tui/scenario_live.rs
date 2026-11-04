//! `/scenario` work with a live screen: the blocking command runs off the UI
//! task while the spinner turns and keys are handled, like `ask_live` does for
//! a model turn. Esc or a second Ctrl-C stops WAITING, never the work: a step
//! that is half done (image build, server start) finishes by itself and the
//! record-first design plus the startup sweep own any cleanup.

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode};
use futures_util::StreamExt;

use crate::scenario::command::{Reply, ScenarioCmd};
use crate::scenario::slot::Slot;
use crate::tui::editor::InputBox;
use crate::tui::status::Status;
use crate::tui::term::Tui;

const STOPPED: &str = "stopped waiting; the step is still finishing on its own. Give it a moment, then /scenario shows where things stand";

/// `Some(reply)` when the command finished; `None` when the learner stopped
/// waiting or a shutdown signal arrived (the caller checks `shutdown`).
pub(crate) async fn run_live(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
    slot: &Slot,
    cmd: ScenarioCmd,
    tokens: Option<u64>,
    window: u64,
) -> Result<Option<Reply>> {
    let fut = slot.handle(cmd);
    tokio::pin!(fut);
    let mut frame = 0usize;
    let mut cancel_armed = false;
    loop {
        if crate::shutdown::is_requested() {
            return Ok(None);
        }
        let status = Status::Thinking {
            frame,
            cancel_hint: cancel_armed,
        };
        crate::tui::page::draw(tui, editor, &status, tokens, window, None, false)?;
        tokio::select! {
            biased;
            _ = crate::shutdown::requested() => return Ok(None),
            r = &mut fut => return Ok(Some(r)),
            Some(Ok(ev)) = events.next() => {
                if crate::tui::page::handle_nav(tui, &ev) { continue; }
                if let Event::Paste(s) = &ev {
                    editor.paste(s);
                } else if let Event::Resize(_, _) = ev {
                    crate::tui::page::handle_resize(tui)?;
                } else if let Event::Key(k) = ev {
                    let stop = match crate::tui::paint::classify_locked_key(k) {
                        crate::tui::paint::LockedKey::CancelRequest if cancel_armed => true,
                        crate::tui::paint::LockedKey::CancelRequest => { cancel_armed = true; false }
                        crate::tui::paint::LockedKey::Edit => {
                            if !matches!(k.code, KeyCode::Enter | KeyCode::Esc) { let _ = editor.handle_key(k); }
                            matches!(k.code, KeyCode::Esc)
                        }
                    };
                    if stop {
                        crate::tui::page::page_notice(tui, STOPPED)?;
                        return Ok(None);
                    }
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(120)) => { frame += 1; }
        }
    }
}
