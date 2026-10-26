//! Terminal lifecycle: take over the whole window (alternate screen + mouse
//! capture) and give it back NO MATTER WHAT. A shell left in raw mode or on the
//! alternate screen = the worst user experience; Drop + panic hook double
//! safety net.

use std::io::Stdout;

use anyhow::Result;
#[cfg(windows)]
use crossterm::event::EnableMouseCapture;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::{Terminal, TerminalOptions, Viewport};

use crate::tui::chat::ChatBuffer;
use crate::tui::editor::MAX_INPUT_ROWS;

/// Bottom region under the chat pane: rule + 1..=8 input rows + status line.
pub fn viewport_height(rows: u16) -> u16 {
    rows.clamp(1, MAX_INPUT_ROWS) + 2
}

pub struct Tui {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
    pub chat: ChatBuffer,
}

/// Button-event mouse tracking with SGR coordinates (`?1000h ?1002h ?1006h`): presses,
/// releases, wheel and drag-while-held, never bare any-motion tracking, so idle mouse
/// movement doesn't wake every loop for a redraw (spec C6). `DisableMouseCapture`
/// turns all of these off again.
struct EnableButtonMouse;

impl crossterm::Command for EnableButtonMouse {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?1000h\x1b[?1002h\x1b[?1006h")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> std::io::Result<()> {
        crossterm::Command::execute_winapi(&EnableMouseCapture)
    }
}

/// Raw mode + alternate screen + mouse capture. Restore is chained onto the
/// panic hook — the previous hook is preserved (the test harness's hook isn't
/// overwritten).
pub fn setup() -> Result<Tui> {
    crossterm::terminal::enable_raw_mode()?;
    // Own the whole window; wheel and drag events reach us (Shift+drag still selects natively).
    if let Err(e) = crossterm::execute!(std::io::stdout(), EnterAlternateScreen, EnableButtonMouse)
    {
        restore();
        return Err(e.into());
    }
    // Bracketed paste: the paste arrives as a single Event::Paste — line breaks
    // aren't counted as Enter and don't split the message. Silently skipped on
    // unsupported terminals (falls back to old behavior).
    let _ = crossterm::execute!(std::io::stdout(), EnableBracketedPaste);
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
    let terminal = match Terminal::with_options(
        CrosstermBackend::new(std::io::stdout()),
        TerminalOptions {
            viewport: Viewport::Fullscreen,
        },
    ) {
        Ok(t) => t,
        Err(e) => {
            restore();
            return Err(e.into());
        }
    };
    Ok(Tui {
        terminal,
        chat: ChatBuffer::default(),
    })
}

/// Give the terminal back — idempotent, swallows errors (no panics on the shutdown path).
pub fn restore() {
    let _ = crossterm::execute!(std::io::stdout(), PopKeyboardEnhancementFlags);
    let _ = crossterm::execute!(std::io::stdout(), DisableBracketedPaste);
    let _ = crossterm::execute!(
        std::io::stdout(),
        DisableMouseCapture,
        LeaveAlternateScreen,
        crossterm::cursor::Show
    );
    let _ = crossterm::terminal::disable_raw_mode();
}

impl Drop for Tui {
    fn drop(&mut self) {
        // Leaving the alternate screen restores the user's previous content;
        // anything printed afterwards lands on the normal screen.
        restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prod() -> &'static str {
        include_str!("term.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap()
    }

    #[test]
    fn viewport_is_rule_plus_rows_plus_status() {
        assert_eq!(viewport_height(1), 3);
        assert_eq!(viewport_height(8), 10);
    }

    /// Source pin: setup takes the whole window with button-event mouse tracking only.
    #[test]
    fn setup_enables_button_event_mouse_only() {
        let body = prod().split("pub fn setup(").nth(1).expect("setup");
        let body = body.split("pub fn restore(").next().unwrap();
        assert!(body.contains("EnterAlternateScreen"));
        assert!(body.contains("EnableButtonMouse"));
        assert!(!body.contains("EnableMouseCapture"));
        assert!(body.contains("Viewport::Fullscreen"));
        assert!(
            !prod().contains("1003h"),
            "no any-motion tracking (spec C6)"
        );
    }

    #[test]
    fn button_mouse_writes_button_event_and_sgr_modes() {
        let mut s = String::new();
        crossterm::Command::write_ansi(&EnableButtonMouse, &mut s).unwrap();
        assert_eq!(s, "\x1b[?1000h\x1b[?1002h\x1b[?1006h");
    }

    /// Source pin: restore() undoes both, mouse capture first, and the panic hook calls it.
    #[test]
    fn restore_leaves_the_alternate_screen_and_disables_mouse_capture() {
        let body = prod().split("pub fn restore(").nth(1).expect("restore");
        let mouse = body.find("DisableMouseCapture").expect("mouse capture off");
        let leave = body.find("LeaveAlternateScreen").expect("alt screen left");
        assert!(mouse < leave);
        assert!(body.contains("disable_raw_mode"));
        assert!(
            prod().contains("restore();\n        prev(info);"),
            "panic hook must restore"
        );
    }

    /// Source pin: the inline-viewport machinery is gone for good.
    #[test]
    fn no_inline_viewport_remnants() {
        for needle in [
            "Viewport::Inline",
            "TrackedBackend",
            "cursor::position()",
            "set_height",
            "inline_terminal",
            "println!",
        ] {
            assert!(!prod().contains(needle), "term.rs still contains {needle}");
        }
    }

    #[test]
    fn drop_only_restores() {
        let body = prod().split("impl Drop for Tui").nth(1).expect("Drop impl");
        assert!(body.contains("restore();"));
        assert!(!body.contains("clear()"));
    }
}
