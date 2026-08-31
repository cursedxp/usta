//! Terminal lifecycle: set up the inline viewport, restore it NO MATTER WHAT.
//! A shell left in broken raw mode = the worst user experience; Drop +
//! panic hook double safety net.

use std::io::Stdout;

use anyhow::Result;
use crossterm::event::{
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Position, Size};
use ratatui::{Terminal, TerminalOptions, Viewport};

use crate::tui::backend_wrap::{fallback_seed, TrackedBackend};

/// Bottom region: input box (3-5 lines) + status line (1).
pub const VIEWPORT_H: u16 = 6;

pub struct Tui {
    pub terminal: Terminal<TrackedBackend<Stdout>>,
    /// Wired into resize handling in Task 3 — measured now so setup() and
    /// resize share the same seeding logic from the start.
    pub last_size: Size,
}

/// Build a fresh inline-viewport terminal seeded at a known cursor position.
/// No CPR here — the seed comes from the caller (either setup()'s one real
/// query, or, from Task 3 onward, a position computed after erasing the old
/// frame). Kept separate so callers never need to issue a second CPR.
pub(crate) fn rebuild_inline(seed: Position) -> Result<Terminal<TrackedBackend<Stdout>>> {
    let terminal = Terminal::with_options(
        TrackedBackend::new(CrosstermBackend::new(std::io::stdout()), seed),
        TerminalOptions {
            viewport: Viewport::Inline(VIEWPORT_H),
        },
    )?;
    Ok(terminal)
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
    let terminal = rebuild_inline(seed)?;
    let last_size = terminal.size()?;
    Ok(Tui {
        terminal,
        last_size,
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
        // Clear the viewport region so shutdown messages print on a clean slate.
        let _ = self.terminal.clear();
        restore();
        println!();
    }
}

#[cfg(test)]
mod tests {
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
        assert!(
            prod.contains("fn rebuild_inline("),
            "term.rs lost the seed-parameterised inline viewport builder"
        );
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
