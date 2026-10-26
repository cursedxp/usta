//! Terminal lifecycle: set up the inline viewport, restore it NO MATTER WHAT.
//! A shell left in broken raw mode = the worst user experience; Drop +
//! panic hook double safety net.

use std::io::Stdout;

use anyhow::Result;
use crossterm::event::{
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Position;
use ratatui::{Terminal, TerminalOptions, Viewport};

use crate::tui::backend_wrap::{fallback_seed, TrackedBackend};
use crate::tui::editor::MAX_INPUT_ROWS;

/// Bottom region: rule + 1..=8 input rows + status line, height follows the text.
pub fn viewport_height(rows: u16) -> u16 {
    rows.clamp(1, MAX_INPUT_ROWS) + 2
}

pub struct Tui {
    pub terminal: Terminal<TrackedBackend<Stdout>>,
    height: u16,
}

/// Raw mode + inline viewport. Restore is chained onto the panic hook — the
/// previous hook is preserved (the test harness's hook isn't overwritten).
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
    // The ONE real CPR query in the whole app — safe here because no
    // `EventStream` exists yet (run.rs constructs the first one only after
    // setup() returns), so nothing else holds the stdin reader lock the query
    // needs. Must never fail setup: both possible errors (the query itself, or
    // the terminal-size lookup used only for the fallback) are swallowed into
    // a silent bottom-row fallback.
    let seed = crossterm::cursor::position()
        .map(|(x, y)| Position { x, y })
        .unwrap_or_else(|_| fallback_seed(crossterm::terminal::size().map_or(0, |(_, h)| h)));
    let height = viewport_height(1);
    let terminal = inline_terminal(seed, height)?;
    Ok(Tui { terminal, height })
}

fn inline_terminal(seed: Position, h: u16) -> Result<Terminal<TrackedBackend<Stdout>>> {
    Ok(Terminal::with_options(
        TrackedBackend::new(CrosstermBackend::new(std::io::stdout()), seed),
        TerminalOptions {
            viewport: Viewport::Inline(h),
        },
    )?)
}

impl Tui {
    pub fn height(&self) -> u16 {
        self.height
    }

    /// ratatui 0.30 has no API to change an inline viewport's height. Clear the old
    /// region, then rebuild the inline terminal seeded at the old viewport's top row —
    /// growing near the bottom scrolls the screen up (ratatui's own inline placement),
    /// shrinking leaves the region top-anchored. No CPR query: the seed is known.
    pub fn set_height(&mut self, h: u16) -> Result<()> {
        if h == self.height {
            return Ok(());
        }
        let top = self.terminal.get_frame().area().y;
        self.terminal.clear()?;
        // clear() puts the real cursor back on the input caret, but the rebuild's
        // `append_lines` prints its newlines from the REAL cursor while the new
        // TrackedBackend assumes the seed row — park it on the seed so both agree,
        // or every growth at the bottom leaves blank rows above the region.
        self.terminal
            .set_cursor_position(Position { x: 0, y: top })?;
        self.terminal = inline_terminal(Position { x: 0, y: top }, h)?;
        self.height = h;
        Ok(())
    }
}

/// Turn off raw mode — idempotent, swallows errors (no panics on the shutdown path).
pub fn restore() {
    let _ = crossterm::execute!(std::io::stdout(), PopKeyboardEnhancementFlags);
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableBracketedPaste);
    let _ = crossterm::terminal::disable_raw_mode();
}

impl Drop for Tui {
    fn drop(&mut self) {
        // Clear the viewport region so shutdown messages print on a clean slate.
        let _ = self.terminal.clear();
        restore();
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_is_rule_plus_rows_plus_status() {
        assert_eq!(viewport_height(1), 3);
        assert_eq!(viewport_height(8), 10);
    }

    /// Source pin: the rebuild's `append_lines` writes from the REAL cursor, so it
    /// must sit on the seed row before the new terminal is built.
    #[test]
    fn set_height_parks_the_cursor_on_the_seed_before_rebuilding() {
        let src = include_str!("term.rs");
        let body = src.split("pub fn set_height(").nth(1).expect("set_height");
        let park = body.find("set_cursor_position(").expect("cursor parked");
        let rebuild = body.find("inline_terminal(").expect("rebuild");
        assert!(park < rebuild);
    }

    /// Source pin: the ONLY real CPR happens in setup(), before any
    /// `EventStream` exists (run.rs creates the stream after calling setup).
    /// Guards both halves so neither can silently regress.
    #[test]
    fn cpr_seed_happens_before_event_stream() {
        let term_src = include_str!("term.rs");
        let prod = term_src.split("#[cfg(test)]").next().unwrap();
        // Exactly ONE real CPR in the whole TUI layer — here in setup(). A second
        // one anywhere would reintroduce the stdin race this fix exists to kill
        // (v0.26.2: pin the absence, not just the presence).
        assert_eq!(prod.matches("cursor::position()").count(), 1);
        for (name, src) in [
            ("backend_wrap.rs", include_str!("backend_wrap.rs")),
            ("page.rs", include_str!("page.rs")),
            ("run.rs", include_str!("run.rs")),
            ("ask.rs", include_str!("ask.rs")),
            ("entry.rs", include_str!("entry.rs")),
        ] {
            let p = src.split("#[cfg(test)]").next().unwrap();
            assert!(
                !p.contains("cursor::position()"),
                "{name} must not issue a real CPR query"
            );
        }
        // Named needle: the wiring call, not just the import at the top of the file.
        assert!(prod.contains("TrackedBackend::new("));
        let run_src = include_str!("run.rs");
        let setup_at = run_src.find("term::setup(").expect("run.rs calls setup");
        let stream_at = run_src
            .find("EventStream::new(")
            .expect("run.rs builds the stream");
        assert!(
            setup_at < stream_at,
            "EventStream must be created after setup's CPR seed"
        );
    }
}
