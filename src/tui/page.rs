//! The TUI session loop's paging layer: pushing persistent content into
//! scrollback and redrawing the live bottom region — extracted from `run.rs`
//! (cleanup round, Task 4).

use anyhow::Result;
use ratatui::layout::Size;
use ratatui::text::{Line, Text};

use crate::tui::convert::{ansi_to_text, text_to_ansi_lines};
use crate::tui::editor::InputBox;
use crate::tui::status::{render_status, Status};
use crate::tui::term::Tui;
use crate::tui::theme;
use crate::ui;

/// Push persistent content into the scrollback above the live bottom region.
/// The region is erased first; the caller's loop repaints it on its next
/// iteration (K5: this content is printed once and never redrawn).
pub(crate) fn page(tui: &mut Tui, text: Text<'static>) -> Result<()> {
    let lines = text_to_ansi_lines(&text, current_width(tui));
    tui.screen.page(&lines)?;
    Ok(())
}

/// Print Usta's reply in the visual language: orange ● line + markdown + blank line.
pub(crate) fn page_reply(tui: &mut Tui, reply: &str, width: u16) -> Result<()> {
    let ansi = ui::render_markdown(reply, width as usize);
    let mut t = ansi_to_text(&format!(
        "\x1b[38;5;{}m{}\x1b[0m\n{ansi}\n",
        theme::BRAND_IDX,
        theme::G_BRAND
    ));
    t.lines.push(Line::raw(""));
    page(tui, t)
}

pub(crate) fn page_notice(tui: &mut Tui, msg: &str) -> Result<()> {
    page(tui, crate::tui::paint::notice_line(msg))
}
pub(crate) fn page_warn(tui: &mut Tui, msg: &str) -> Result<()> {
    page(tui, crate::tui::paint::warn_line(msg))
}
pub(crate) fn page_error(tui: &mut Tui, msg: &str) -> Result<()> {
    page(tui, crate::tui::paint::error_line(msg))
}

/// Flush the notices buffered by ui::notice/ui::warn while the TUI was live,
/// routing each to the right scan-level: a leading `⚠ ` (from ui::warn) renders
/// as the amber warning layer; everything else is a dim `·` info line.
pub(crate) fn flush_notices(tui: &mut Tui) -> Result<()> {
    for m in ui::drain_tui_notices() {
        match m.strip_prefix(&format!("{} ", theme::G_WARN)) {
            Some(rest) => page_warn(tui, rest)?,
            None => page_notice(tui, &m)?,
        }
    }
    Ok(())
}

/// Push the user's submitted line to scrollback — wrapped to the current width.
pub(crate) fn page_user_echo(tui: &mut Tui, line: &str) -> Result<()> {
    let w = current_width(tui);
    page(tui, crate::tui::paint::user_echo_text(line, w))
}

/// Current terminal width — keeps wrapping correct after a resize (spec B3).
/// This is the width the bottom region is currently painted for, so content
/// and frame never disagree. Falls back to 80 if it is degenerate (wrapping
/// doesn't break, just gets narrow).
pub(crate) fn current_width(tui: &Tui) -> u16 {
    match tui.screen.size().width {
        0 => 80,
        w => w,
    }
}

/// Adopt the new terminal size after a resize. Without this the bottom region
/// keeps its stale pre-resize line count and garbles (duplicated/shifted
/// lines) on the next paint; the caller's loop repaints on its next iteration.
/// `crossterm::terminal::size()` is a SIZE query — it reads no reply off
/// stdin, unlike the cursor-position query this rewrite removed (K3).
pub(crate) fn handle_resize(tui: &mut Tui) -> Result<()> {
    let (w, h) = crossterm::terminal::size().unwrap_or((80, 24));
    tui.screen.resize(Size::new(w, h))?;
    Ok(())
}

/// Draw the bottom region: input frame (top) + status line (bottom).
///
/// The status line is appended as the block's LAST line, so the cursor index
/// `frame_lines` returns — an index into the frame's own lines, counted from
/// the top — stays valid unchanged.
pub(crate) fn draw(
    tui: &mut Tui,
    editor: &InputBox,
    status: &Status,
    tokens: Option<u64>,
    window: u64,
    watch: Option<(bool, bool, usize)>,
    verify_failing: bool,
) -> Result<()> {
    let size = tui.screen.size();
    let (mut lines, cursor_line, cursor_col) = editor.frame_lines(size.width, size.height);
    let status_line = render_status(status, tokens, window, watch, verify_failing);
    lines.extend(text_to_ansi_lines(&Text::from(status_line), size.width));
    tui.screen.paint(&lines, cursor_line, cursor_col)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn resize_events_are_handled_in_every_event_loop() {
        // Source pin (same pattern as run_rs_wiring_call_sites_are_pinned):
        // the inline-viewport TUI can't be driven from a unit test, and Resize
        // was silently ignored in every loop — which garbled the display on
        // terminal resize (v0.24.6). Guard against the arms being deleted.
        for (name, src) in [
            ("run.rs", include_str!("run.rs")),
            ("ask.rs", include_str!("ask.rs")),
            ("entry.rs", include_str!("entry.rs")),
        ] {
            assert!(
                src.contains("Event::Resize"),
                "{name} no longer handles Event::Resize"
            );
        }
        let own = include_str!("page.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap()
            .to_string();
        assert!(
            own.contains("fn handle_resize"),
            "page.rs lost its handle_resize helper"
        );
    }

    /// Source pin: every event loop repaints the bottom region at the top of
    /// each iteration. `Screen::page` and `Screen::resize` both leave the
    /// block erased and forgotten, so a loop that stops calling `draw` shows
    /// no input frame at all — and nothing else in the suite notices, because
    /// the live TUI cannot be driven from a unit test.
    ///
    /// The expected COUNT is asserted, not mere presence: `ask.rs` holds TWO
    /// event loops (`ask_live` and `tui_confirm`), so a presence-only check
    /// stays green after the confirm loop's repaint is deleted — passing
    /// against the exact regression it exists to catch. Only the production
    /// half is scanned, so a `page::draw(` inside a test body cannot satisfy
    /// it either. Adding a real event loop means bumping the number here.
    #[test]
    fn every_event_loop_repaints_the_bottom_region() {
        for (name, expected, src) in [
            ("run.rs", 1, include_str!("run.rs")),
            ("ask.rs", 2, include_str!("ask.rs")),
            ("entry.rs", 1, include_str!("entry.rs")),
            ("intro.rs", 1, include_str!("intro.rs")),
        ] {
            let prod = src.split("#[cfg(test)]").next().unwrap();
            let found = prod.matches("page::draw(").count();
            assert_eq!(
                found, expected,
                "{name} should repaint the bottom region {expected} time(s), found {found}"
            );
        }
    }

    /// Source pin: the paging layer talks to `Screen` and nothing else. No
    /// ratatui inline viewport (`insert_before`, K1) and no absolute row
    /// addressing (`MoveTo(`, K3) — the two mechanisms this rewrite removed.
    #[test]
    fn page_rs_uses_neither_insert_before_nor_absolute_addressing() {
        let prod = include_str!("page.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(
            !prod.contains("insert_before"),
            "page.rs must not push content through ratatui's inline viewport"
        );
        assert!(
            !prod.contains("MoveTo("),
            "page.rs must not address an absolute row"
        );
    }
}
