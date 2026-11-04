//! `/scenario` work with a live screen: the blocking command runs off the UI
//! task while the spinner turns and resize/paste are handled, like `ask_live`
//! does for a model turn. Unlike a model turn it cannot be cancelled: a step
//! that is half done (image build, server start) must finish so its reply
//! (task statement, mentor turn) is not lost. Only shutdown ends the wait.

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode};
use futures_util::StreamExt;

use crate::scenario::command::{Reply, ScenarioCmd};
use crate::scenario::slot::Slot;
use crate::tui::editor::InputBox;
use crate::tui::status::Status;
use crate::tui::term::Tui;

const BUSY: &str = "finishing this step, one moment";

/// `Some(reply)` when the command finished; `None` on a shutdown signal.
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
    let mut nudged = false;
    loop {
        if crate::shutdown::is_requested() {
            return Ok(None);
        }
        let status = Status::Working { frame };
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
                    // Esc / Ctrl-C do not stop the step; say so once.
                    let cancel = matches!(k.code, KeyCode::Esc)
                        || matches!(crate::tui::paint::classify_locked_key(k), crate::tui::paint::LockedKey::CancelRequest);
                    if cancel {
                        if !nudged {
                            nudged = true;
                            crate::tui::page::page_notice(tui, BUSY)?;
                        }
                    } else if !matches!(k.code, KeyCode::Enter) {
                        let _ = editor.handle_key(k);
                    }
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(120)) => { frame += 1; }
        }
    }
}
