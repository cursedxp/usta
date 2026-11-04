//! `/scenario` work with a live screen: the blocking command runs off the UI
//! task while the spinner turns and resize/paste are handled, like `ask_live`
//! does for a model turn. A step may report stages, which print as they happen.
//! A step cannot be cancelled: a half-done step (image build, server start) must
//! finish so its reply (task statement, mentor turn) is not lost. The one
//! exception is generation, which has changed nothing the learner can see.
//! Only shutdown otherwise ends the wait.

use std::sync::Arc;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode};
use futures_util::StreamExt;

use crate::scenario::command::{Reply, ScenarioCmd};
use crate::scenario::model::Progress;
use crate::scenario::slot::Slot;
use crate::tui::editor::InputBox;
use crate::tui::status::Status;
use crate::tui::term::Tui;

const BUSY: &str = "finishing this step, one moment";
/// Said when Esc lands, while the current stage still runs to its end.
const STOPPED: &str =
    "Stopping after this step. The new task will be thrown away and nothing will be started.";

/// Esc stops generation; any other step must finish, and says so once.
fn on_cancel_key(slot: &Slot, stopped: &mut bool, nudged: &mut bool) -> Option<&'static str> {
    if slot.cancel_generation() {
        return (!std::mem::replace(stopped, true)).then_some(STOPPED);
    }
    (!std::mem::replace(nudged, true)).then_some(BUSY)
}

/// `Some(reply)` when the command finished; `None` on a shutdown signal.
#[allow(clippy::too_many_arguments)] // the live screen's handles plus the command
pub(crate) async fn run_live(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
    slot: &Slot,
    cmd: ScenarioCmd,
    said: Vec<String>,
    tokens: Option<u64>,
    window: u64,
) -> Result<Option<Reply>> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let progress: Progress = Arc::new(move |m: &str| {
        let _ = tx.send(m.to_string());
    });
    let fut = slot.handle_with(cmd, said, progress);
    tokio::pin!(fut);
    let mut frame = 0usize;
    let (mut stopped, mut nudged) = (false, false);
    loop {
        if crate::shutdown::is_requested() {
            return Ok(None);
        }
        let status = Status::Working { frame };
        crate::tui::page::draw(tui, editor, &status, tokens, window, None, false)?;
        tokio::select! {
            biased;
            _ = crate::shutdown::requested() => return Ok(None),
            r = &mut fut => {
                while let Ok(line) = rx.try_recv() {
                    crate::tui::page::page_notice(tui, &line)?;
                }
                return Ok(Some(r));
            }
            Some(line) = rx.recv() => { crate::tui::page::page_notice(tui, &line)?; }
            Some(Ok(ev)) = events.next() => {
                if crate::tui::page::handle_nav(tui, &ev) { continue; }
                if let Event::Paste(s) = &ev {
                    editor.paste(s);
                } else if let Event::Resize(_, _) = ev {
                    crate::tui::page::handle_resize(tui)?;
                } else if let Event::Key(k) = ev {
                    // Esc / Ctrl-C stop generation only; any other step says so once.
                    let cancel = matches!(k.code, KeyCode::Esc)
                        || matches!(crate::tui::paint::classify_locked_key(k), crate::tui::paint::LockedKey::CancelRequest);
                    if cancel {
                        if let Some(line) = on_cancel_key(slot, &mut stopped, &mut nudged) {
                            crate::tui::page::page_notice(tui, line)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::model::NoModel;
    use std::path::Path;
    use std::sync::Arc;

    #[test]
    fn esc_stops_generation_with_one_line_and_otherwise_keeps_the_busy_note() {
        let slot = Slot::new(
            Path::new("/nonexistent-g"),
            Path::new("/nonexistent-p"),
            "linux",
            Arc::new(NoModel),
        );
        let (mut stopped, mut nudged) = (false, false);
        assert_eq!(on_cancel_key(&slot, &mut stopped, &mut nudged), Some(BUSY));
        assert_eq!(on_cancel_key(&slot, &mut stopped, &mut nudged), None);
        let cancel = slot.cancel_for_test();
        let _guard = cancel.open();
        assert_eq!(
            on_cancel_key(&slot, &mut stopped, &mut nudged),
            Some(STOPPED)
        );
        assert_eq!(on_cancel_key(&slot, &mut stopped, &mut nudged), None);
        assert!(cancel.requested());
    }
}
