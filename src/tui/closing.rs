//! End-of-session save while the TUI is still up. The closing flush is one
//! long model call; running it after the alternate screen was left put a bare
//! spinner on the normal screen in cooked mode, where typed keys echoed into it
//! and leaked to the shell after exit. Here the fullscreen UI stays, the status
//! line says what is happening, and every key is read and dropped.

use std::path::Path;

use anyhow::Result;
use crossterm::event::{Event, EventStream};
use futures_util::StreamExt;

use crate::backend::Backend;
use crate::session::Session;
use crate::tui::editor::InputBox;
use crate::tui::status::Status;
use crate::tui::term::Tui;

/// Run the closing flush (`lifecycle::flush_progress`, history line recorded)
/// under `Status::Saving` and return its result for main's closing steps. Keys
/// are discarded, Ctrl+C included: cancelling would drop the model call midway
/// and leave progress unwritten, so the save is deliberately not cancellable.
/// Draw errors are ignored — a failed frame must never abort the save.
/// Notices raised by the flush stay buffered (`draw_keep_notices`) and print on
/// the normal screen after exit via `ui::flush_tui_notices`.
pub(crate) async fn save_live(
    tui: &mut Tui,
    editor: &InputBox,
    events: &mut EventStream,
    backend: &mut Backend,
    session: &Session,
    project_root: &Path,
    tokens: Option<u64>,
) -> Result<()> {
    let window = backend.context_window();
    let fut = crate::lifecycle::flush_progress(backend, session, project_root, true);
    tokio::pin!(fut);
    let mut frame = 0usize;
    let mut input_open = true; // stream ended → stop polling it (no hot loop, spec B4)
    loop {
        let _ = crate::tui::page::draw_keep_notices(
            tui,
            editor,
            &Status::Saving { frame },
            tokens,
            window,
        );
        tokio::select! {
            r = &mut fut => return r,
            ev = events.next(), if input_open => match ev {
                Some(Ok(Event::Resize(_, _))) => { let _ = crate::tui::page::handle_resize(tui); }
                Some(_) => {} // typed keys, paste, mouse: dropped
                None => input_open = false,
            },
            _ = tokio::time::sleep(std::time::Duration::from_millis(120)) => { frame += 1; }
        }
    }
}

#[cfg(test)]
mod tests {
    fn prod(src: &'static str) -> &'static str {
        src.split("#[cfg(test)]").next().unwrap()
    }

    /// Source pin: the save runs the real closing flush and paints the TUI's own
    /// busy status — keys are read and dropped, never handed to the editor.
    #[test]
    fn save_live_runs_flush_under_saving_status_and_drops_keys() {
        let body = prod(include_str!("closing.rs"));
        let body = body
            .split("pub(crate) async fn save_live(")
            .nth(1)
            .expect("save_live");
        assert!(body.contains("crate::lifecycle::flush_progress("));
        assert!(body.contains("Status::Saving"));
        assert!(
            body.contains("draw_keep_notices("),
            "notices must survive for the normal screen"
        );
        assert!(
            body.contains("events.next()"),
            "keys are read during the save"
        );
        assert!(
            !body.contains("handle_key("),
            "typed keys must not reach the editor"
        );
        assert!(!body.contains("Cancelled"), "the save is not cancellable");
    }

    /// Source pin: the TUI loop saves after it exits, while `tui` is still alive,
    /// and hands the flush result back to main.
    #[test]
    fn run_saves_after_the_loop_before_returning() {
        let src = prod(include_str!("run.rs"));
        let tail = src.rsplit("    loop {").next().unwrap();
        let save = tail
            .find("crate::tui::closing::save_live(")
            .expect("save_live call");
        let ret = tail
            .find("Ok(Some((session, recorder, lock, flushed)))")
            .expect("return");
        assert!(save < ret);
    }

    /// Source pin: main no longer runs the closing flush for the TUI path (that
    /// happened on the normal screen in cooked mode — the bug); plain keeps it.
    #[test]
    fn main_flushes_only_on_the_plain_path() {
        let src = prod(include_str!("../main.rs"));
        let tui = src
            .split("if !ui::is_plain() {")
            .nth(1)
            .expect("tui branch");
        let (tui_branch, plain_branch) = tui.split_once("    } else {").expect("plain branch");
        assert!(!tui_branch.contains("flush_progress("));
        assert!(plain_branch.contains("flush_progress("));
    }
}
