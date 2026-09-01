//! Terminal lifecycle: set up raw mode and the live bottom region, restore it
//! NO MATTER WHAT. A shell left in broken raw mode = the worst user
//! experience; Drop + panic hook double safety net.

use std::io::Stdout;

use anyhow::Result;
use crossterm::event::{
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::layout::Size;

use crate::tui::screen::Screen;

pub struct Tui {
    pub(crate) screen: Screen<Stdout>,
}

/// Raw mode + the live bottom region. Restore is chained onto the panic hook —
/// the previous hook is preserved (the test harness's hook isn't overwritten).
pub fn setup() -> Result<Tui> {
    crossterm::terminal::enable_raw_mode()?;
    // Bracketed paste: the paste arrives as a single Event::Paste — line breaks
    // aren't counted as Enter and don't split the message. Silently skipped on
    // unsupported terminals (falls back to old behavior).
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::EnableBracketedPaste);
    // Kitty keyboard protocol: lets modern terminals disambiguate Shift+Enter / Alt+Enter
    // from bare Enter. Unsupported terminals are skipped silently (Ctrl+J still works).
    if matches!(
        crossterm::terminal::supports_keyboard_enhancement(),
        Ok(true)
    ) {
        let _ = crossterm::execute!(
            std::io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
    }
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        prev(info);
    }));
    // A SIZE query, not a cursor query — it reads no reply off stdin, so it
    // races with nothing. Like the CPR seed it replaced, it must never fail
    // setup(): an unavailable size falls back to a conventional 80x24, which
    // the first `Resize` event corrects.
    let (w, h) = crossterm::terminal::size().unwrap_or((80, 24));
    Ok(Tui {
        screen: Screen::new(std::io::stdout(), Size::new(w, h)),
    })
}

/// Turn off raw mode — idempotent, swallows errors (no panics on the shutdown path).
pub fn restore() {
    let _ = crossterm::execute!(std::io::stdout(), PopKeyboardEnhancementFlags);
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableBracketedPaste);
    let _ = crossterm::terminal::disable_raw_mode();
}

impl Drop for Tui {
    fn drop(&mut self) {
        // Clear the bottom region so shutdown messages print on a clean slate.
        let _ = self.screen.clear_block();
        restore();
        println!();
    }
}

#[cfg(test)]
mod tests {
    /// Source pin: the terminal lifecycle owns no absolute cursor addressing
    /// and no ratatui inline viewport. The live bottom region is drawn by
    /// `Screen` alone (K1), and the row-addressing bug class this rewrite
    /// removes cannot come back through setup's old CPR seed (K3).
    ///
    /// It also pins `term::setup()` BEFORE `EventStream::new()` in `run.rs`.
    /// The CPR seed that originally motivated that ordering is gone, but the
    /// ordering is still load-bearing: `setup()` calls
    /// `crossterm::terminal::supports_keyboard_enhancement()`, which is itself
    /// a terminal QUERY — it writes `ESC[?u ESC[c` and then polls/reads the
    /// reply off stdin, taking the same global reader lock an `EventStream`
    /// holds. Crossterm documents it: "this function will block and possibly
    /// time out while `crossterm::event::read` or `crossterm::event::poll` are
    /// being called."
    ///
    /// Invert the ordering and the failure is SILENT: a ~2 s startup stall,
    /// then `supports_keyboard_enhancement()` returns `Err`, the `matches!`
    /// gate skips the kitty push, and Shift+Enter / Alt+Enter disambiguation is
    /// lost — with the whole suite still green. Do not delete this assertion on
    /// the grounds that the CPR seed is gone; the query that replaced it has
    /// the same stdin-contention contract.
    #[test]
    fn setup_has_no_cpr_seed_and_no_inline_viewport() {
        let prod = include_str!("term.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(
            !prod.contains("cursor::position()"),
            "term.rs must not query the cursor position"
        );
        assert!(
            !prod.contains("Viewport::Inline"),
            "term.rs must not build a ratatui inline viewport"
        );
        assert!(
            !prod.contains("insert_before"),
            "term.rs must not push content through ratatui's inline viewport"
        );

        let run = include_str!("run.rs");
        let setup_at = run
            .find("term::setup(")
            .expect("run.rs no longer calls term::setup()");
        let stream_at = run
            .find("EventStream::new(")
            .expect("run.rs no longer builds an EventStream");
        assert!(
            setup_at < stream_at,
            "term::setup() must run before EventStream::new(): \
             supports_keyboard_enhancement() reads its reply off stdin and would \
             contend with the event stream's reader, silently losing the kitty \
             keyboard protocol after a 2 s stall"
        );
    }
}
