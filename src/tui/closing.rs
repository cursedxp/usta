//! End-of-session save while the TUI is still up. The closing flush is one
//! long model call; running it after the alternate screen was left put a bare
//! spinner on the normal screen in cooked mode, where typed keys echoed into it
//! and leaked to the shell after exit. Here the fullscreen UI stays, the status
//! line says what is happening, and every key is read and dropped.

use std::path::Path;

use anyhow::Result;
use crossterm::event::{Event, EventStream};
use futures_util::{FutureExt, StreamExt};

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
    // Fixed tick outside the loop: a key storm can't keep resetting it, so the
    // spinner keeps turning; dropped keys don't force a redraw.
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(120));
    let mut frame = 0usize;
    let mut redraw = true;
    let mut input_open = true; // stream ended or erroring → stop polling it (no spin, spec B4)
    let result = loop {
        if redraw {
            let _ = crate::tui::page::draw_keep_notices(
                tui,
                editor,
                &Status::Saving { frame },
                tokens,
                window,
            );
            redraw = false;
        }
        tokio::select! {
            r = &mut fut => break r,
            ev = events.next(), if input_open => match ev {
                Some(Ok(Event::Resize(_, _))) => {
                    let _ = crate::tui::page::handle_resize(tui);
                    redraw = true;
                }
                Some(Ok(_)) => {} // typed keys, paste, mouse: dropped
                Some(Err(_)) | None => input_open = false,
            },
            _ = tick.tick() => { frame += 1; redraw = true; }
        }
    };
    // Drop input queued at the very end while the stream is still alive:
    // `term::restore`'s zero-timeout poll can lose the race with the dropping
    // EventStream's reader thread. Non-blocking — stops at the first Pending.
    while let Some(Some(Ok(_))) = events.next().now_or_never() {}
    result
}

#[cfg(test)]
mod tests {
    fn prod(src: &'static str) -> &'static str {
        src.split("#[cfg(test)]").next().unwrap()
    }

    /// Production code of `save_live` with `//` comment lines removed, so a pin
    /// can't be satisfied by prose.
    fn save_live_code() -> String {
        prod(include_str!("closing.rs"))
            .split("pub(crate) async fn save_live(")
            .nth(1)
            .expect("save_live")
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Source pin: the save runs the real closing flush and paints the TUI's own
    /// busy status — keys are read and dropped, never handed to the editor and
    /// never inspected (so Ctrl+C can't cancel it).
    #[test]
    fn save_live_runs_flush_under_saving_status_and_drops_keys() {
        let body = save_live_code();
        assert!(body.contains("crate::lifecycle::flush_progress("));
        assert!(
            body.contains("r = &mut fut => "),
            "the flush future is awaited to completion"
        );
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
        assert!(
            !body.contains("KeyCode"),
            "no key is special-cased (Ctrl+C ignored)"
        );
    }

    /// Source pin: the spinner ticks on a fixed interval (a key storm can't
    /// freeze it), and input queued at the end is drained while the stream is
    /// still alive — `restore()`'s zero-timeout poll can lose the race with the
    /// dropping EventStream's reader thread.
    #[test]
    fn save_live_ticks_on_interval_and_drains_before_returning() {
        let body = save_live_code();
        assert!(body.contains("tokio::time::interval("));
        assert!(!body.contains("tokio::time::sleep("));
        let done = body.find("r = &mut fut => ").expect("flush arm");
        let drain = body.find("now_or_never()").expect("non-blocking drain");
        assert!(done < drain);
    }

    /// Source pin: the TUI loop saves after the main loop closes, while `tui` is
    /// still alive, awaits it, and hands the flush result back to main.
    #[test]
    fn run_saves_after_the_loop_before_returning() {
        let src = prod(include_str!("run.rs"));
        let tail = src.rsplit("\n    loop {\n").next().unwrap();
        let close = tail.find("\n    }\n").expect("main loop close");
        let save = tail
            .find("crate::tui::closing::save_live(")
            .expect("save_live call");
        let ret = tail
            .find("Ok(Some((session, recorder, lock, flushed)))")
            .expect("return");
        assert!(close < save && save < ret);
        assert!(tail[save..ret].contains(".await"));
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
        let (tui_branch, plain_branch) = tui
            .split_once("resolve_topic(&mut backend")
            .expect("plain branch");
        assert!(tui_branch.contains("tui::run::run("));
        assert!(!tui_branch.contains("flush_progress("));
        assert!(plain_branch.contains("flush_progress("));
    }
}
