//! The TUI session loop's paging layer: pushing persistent content into
//! scrollback and redrawing the live bottom region — extracted from `run.rs`
//! (cleanup round, Task 4).

use anyhow::Result;
use crossterm::cursor::{MoveDown, MoveTo, MoveToColumn, MoveUp};
use crossterm::terminal::{Clear, ClearType};
use ratatui::layout::{Constraint, Layout, Position, Size};
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
pub(crate) fn anchor_row(height: u16) -> u16 {
    height.saturating_sub(VIEWPORT_H)
}

/// How far to walk up from the cursor's position within the old frame, and
/// how many rows to erase from there, to clear exactly the stale viewport.
pub(crate) struct ErasePlan {
    pub(crate) up: u16,
    pub(crate) rows: u16,
}

/// Build an [`ErasePlan`] from the cursor's offset within the old frame.
pub(crate) fn erase_plan(off: u16) -> ErasePlan {
    ErasePlan {
        up: off,
        rows: VIEWPORT_H,
    }
}

/// Whether the terminal size actually changed. A `Resize` event reporting an
/// unchanged size — common during drag-resizing — should be a no-op.
pub(crate) fn size_changed(prev: Size, now: Size) -> bool {
    prev != now
}

/// Refresh the inline viewport after a terminal resize.
///
/// `TrackedBackend` never issues a CPR query, so it only knows about cursor
/// moves usta itself made. A width change makes the terminal reflow the
/// scrollback above the viewport on its own — the real cursor travels with
/// that content, but the tracked position does not, so it goes stale. Ratatui
/// then anchors the new viewport off that stale row, and its inline
/// `clear_viewport` only erases DOWNWARD from there: whatever old paint sat
/// ABOVE the new anchor survives as a ghost frame.
///
/// The fix rests on an assumption, not a guarantee: `off = tracked_cursor.y -
/// viewport_area.y`, both pre-resize tracked values, survives the reflow of
/// the scrollback ABOVE the frame — which is what the ghost comes from — and
/// holds only as long as the frame's own rows are not rewrapped by the
/// resize. Walk up by `off`, erase exactly `VIEWPORT_H` lines, then plant the
/// cursor at a KNOWN absolute row and rebuild the inline viewport from that
/// seed — no CPR involved. But `off` is applied as `MoveUp(off)` against the
/// REAL, post-reflow cursor: if a width narrowing rewraps the frame's own
/// lines, the real distance grows, the walk-up falls short of the true frame
/// top, and some residue can survive.
///
/// `Terminal::resize` (and `autoresize`, which calls it) cannot be used here:
/// it force-clears the whole screen on a horizontal shrink, which would take
/// the user's transcript down with the ghost.
///
/// The erase sequence below writes through a private `std::io::stdout()`
/// handle rather than through `tui.terminal`'s backend (spec-mandated, matching
/// `term.rs`'s existing raw-sequence pattern). That bypasses `TrackedBackend`,
/// so its tracked cursor position is left desynchronised by these moves —
/// harmless only because the very next step replaces the terminal wholesale
/// via `rebuild_inline`. The sequence is also not transactional: if a write
/// fails partway through, the function returns `Err` with the old frame
/// partially erased and `last_size` left stale. `last_size` is deliberately
/// assigned last, after the fallible rebuild, so a retry WOULD pick up from
/// a correct state if one occurred — but in the current call graph every
/// call site propagates the error with `?`, so an `Err` here unwinds out of
/// the event loop and ends the session; there is no next `Resize` event to
/// retry on.
///
/// Limitation: `off` is the useful measurement for a WIDTH change — the case
/// that produces the ghost frames this fixes — subject to the rewrap caveat
/// above; it is not exact whenever the frame's own rows get rewrapped. On a
/// HEIGHT shrink, `get_cursor_position()` goes through
/// `TrackedBackend::get_cursor_position`, which clamps the tracked
/// row against a freshly measured, post-resize screen height
/// (`clamp_to_screen`, added in v0.26.2 so a vertical shrink cannot anchor the
/// viewport below the terminal) — while `get_frame().area().y` is still the
/// pre-resize viewport anchor. The clamp can pull the cursor row to a row
/// numerically lower than (visually above) the stale frame row,
/// `saturating_sub` yields `off == 0`, and the walk-up is skipped. That is no
/// worse than the previous behavior, but no better on that axis either.
pub(crate) fn handle_resize(tui: &mut Tui) -> Result<()> {
    let size = tui.terminal.size()?;
    if !size_changed(tui.last_size, size) {
        return Ok(());
    }

    let cursor_y = tui.terminal.get_cursor_position()?.y;
    let frame_y = tui.terminal.get_frame().area().y;
    let off = cursor_y.saturating_sub(frame_y);

    let plan = erase_plan(off);
    let mut stdout = std::io::stdout();
    if plan.up > 0 {
        crossterm::execute!(stdout, MoveUp(plan.up))?;
    }
    crossterm::execute!(stdout, MoveToColumn(0))?;
    for row in 0..plan.rows {
        crossterm::execute!(stdout, Clear(ClearType::CurrentLine))?;
        if row + 1 < plan.rows {
            crossterm::execute!(stdout, MoveDown(1))?;
        }
    }

    let anchor = anchor_row(size.height);
    crossterm::execute!(stdout, MoveTo(0, anchor))?;
    tui.terminal = crate::tui::term::rebuild_inline(Position { x: 0, y: anchor })?;
    tui.last_size = size;
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

    #[test]
    fn handle_resize_erases_only_its_own_frame() {
        // Ghost frames (v0.29.0): after a width change the terminal reflows, the
        // tracked cursor goes stale, and ratatui anchored the new viewport at a
        // wrong row — inline clear_viewport only erases DOWNWARD from that row, so
        // the old frame's top rows survived. The fix walks UP by the (reflow-proof)
        // cursor offset and erases exactly VIEWPORT_H lines.
        //
        // The negative needles are the point: clearing the whole screen or calling
        // Terminal::resize (which force-clears on a horizontal shrink) would take
        // the user's transcript down with the ghost. That is not a fix.
        let prod = include_str!("page.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let after = prod
            .split("fn handle_resize")
            .nth(1)
            .expect("page.rs lost its handle_resize helper");
        let body = after.split("\npub(crate) fn ").next().unwrap();
        assert!(body.contains("ClearType::CurrentLine"));
        assert!(body.contains("MoveUp"));
        assert!(body.contains("rebuild_inline"));
        assert!(
            !body.contains("ClearType::All"),
            "handle_resize must never wipe the screen"
        );
        assert!(
            !body.contains("autoresize"),
            "autoresize routes into the screen-clearing resize path"
        );
        assert!(
            !body.contains(".resize("),
            "Terminal::resize force-clears on horizontal shrink"
        );
    }
}
