//! The TUI session loop's paging layer: pushing persistent content into
//! scrollback and redrawing the live bottom region — extracted from `run.rs`
//! (cleanup round, Task 4).

use anyhow::Result;
use ratatui::layout::{Constraint, Layout, Size};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Paragraph, Widget};

use crate::tui::convert::ansi_to_text;
use crate::tui::editor::InputBox;
use crate::tui::status::{render_status, Status};
use crate::tui::term::{Tui, VIEWPORT_H};
use crate::tui::theme;
use crate::ui;

/// Push persistent content above the viewport (into scrollback).
pub(crate) fn page(tui: &mut Tui, text: Text<'static>) -> Result<()> {
    let h = text.height() as u16;
    tui.terminal.insert_before(h, |buf| {
        Paragraph::new(text).render(buf.area, buf);
    })?;
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
/// Falls back to 80 if measurement fails (wrapping doesn't break, just gets narrow).
pub(crate) fn current_width(tui: &Tui) -> u16 {
    tui.terminal.size().map(|s| s.width).unwrap_or(80)
}

/// Row (from the top of the terminal) where the inline viewport should be
/// anchored after a resize: the last `VIEWPORT_H` rows of a screen `height`
/// rows tall. Saturates so a screen shorter than the viewport doesn't underflow.
#[allow(dead_code)]
pub(crate) fn anchor_row(height: u16) -> u16 {
    height.saturating_sub(VIEWPORT_H)
}

/// How far to walk up from the cursor's position within the old frame, and
/// how many rows to erase from there, to clear exactly the stale viewport.
#[allow(dead_code)]
pub(crate) struct ErasePlan {
    pub(crate) up: u16,
    pub(crate) rows: u16,
}

/// Build an [`ErasePlan`] from the cursor's offset within the old frame.
#[allow(dead_code)]
pub(crate) fn erase_plan(off: u16) -> ErasePlan {
    ErasePlan {
        up: off,
        rows: VIEWPORT_H,
    }
}

/// Whether the terminal size actually changed. A `Resize` event reporting an
/// unchanged size — common during drag-resizing — should be a no-op.
#[allow(dead_code)]
pub(crate) fn size_changed(prev: Size, now: Size) -> bool {
    prev != now
}

/// Refresh the inline viewport after a terminal resize. Without this the
/// viewport keeps drawing at its stale pre-resize area and the bottom region
/// garbles (duplicated/shifted lines); the caller's loop redraws on its next
/// iteration. A `Resize` event reporting an unchanged size is now a no-op:
/// `autoresize()` only calls `resize()` when the area differs from before.
pub(crate) fn handle_resize(tui: &mut Tui) -> Result<()> {
    // ratatui 0.30's `resize()` (invoked by `autoresize()` on every size change) already
    // clears the viewport unconditionally and full-clears on horizontal shrink, so a
    // manual `clear()` here is redundant. It is also an avoidable second cursor-position
    // (CPR) query: 0.30's public `clear()` added its own CPR read that 0.29's `clear()`
    // never made — CPR itself was never removed from the resize path, it has always been
    // queried unconditionally by the inline-viewport resize computation.
    tui.terminal.autoresize()?;
    Ok(())
}

/// Draw the bottom region: input box (top) + status line (bottom).
pub(crate) fn draw(
    tui: &mut Tui,
    editor: &InputBox,
    status: &Status,
    tokens: Option<u64>,
    window: u64,
    watch: Option<(bool, bool, usize)>,
    verify_failing: bool,
) -> Result<()> {
    tui.terminal.draw(|f| {
        let [box_area, status_area] =
            Layout::vertical([Constraint::Length(VIEWPORT_H - 1), Constraint::Length(1)])
                .areas(f.area());
        editor.render(f, box_area);
        f.render_widget(
            render_status(status, tokens, window, watch, verify_failing),
            status_area,
        );
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_row_puts_the_viewport_at_the_bottom_and_saturates() {
        // The tracked cursor goes stale when the terminal reflows on resize, so
        // handle_resize stops asking where it is and puts it somewhere known.
        // Seeding at h - VIEWPORT_H makes compute_inline_size land the viewport on
        // the last VIEWPORT_H rows without appending (and therefore scrolling) a
        // single line.
        assert_eq!(anchor_row(30), 30 - VIEWPORT_H);
        assert_eq!(anchor_row(VIEWPORT_H), 0);
        assert_eq!(anchor_row(4), 0, "a short screen must not underflow");
    }

    #[test]
    fn erase_plan_walks_up_by_the_cursor_offset_and_erases_the_whole_frame() {
        // The offset of the cursor WITHIN the frame survives a reflow: the terminal
        // moves the cursor together with the content it sits in. That makes a
        // relative walk exact where an absolute row is not.
        let p = erase_plan(2);
        assert_eq!(p.up, 2);
        assert_eq!(p.rows, VIEWPORT_H);
        assert_eq!(
            erase_plan(0).up,
            0,
            "no MoveUp when the cursor is on the top row"
        );
    }

    #[test]
    fn size_changed_is_false_for_an_identical_size() {
        // Drag-resizing emits a burst of Resize events; rebuilding on every one of
        // them would strobe.
        assert!(!size_changed(Size::new(80, 24), Size::new(80, 24)));
        assert!(size_changed(Size::new(80, 24), Size::new(81, 24)));
        assert!(size_changed(Size::new(80, 24), Size::new(80, 25)));
    }

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
}
