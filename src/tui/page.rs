//! The TUI session loop's paging layer: pushing persistent content into
//! scrollback and redrawing the live bottom region — extracted from `run.rs`
//! (cleanup round, Task 4).

use anyhow::Result;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Text};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::chat::{ChatBuffer, Source};
use crate::tui::editor::InputBox;
use crate::tui::status::{render_status, Status};
use crate::tui::term::{viewport_height, Tui};
use crate::tui::theme;
use crate::ui;

/// Push persistent content into the chat pane (soft-wrapped on resize).
pub(crate) fn page(tui: &mut Tui, text: Text<'static>) -> Result<()> {
    tui.chat.push(Source::Text(text));
    Ok(())
}

/// Like `page`, but the lines are pre-sized panels (welcome box): clipped on a
/// narrower terminal instead of re-wrapped, so box borders stay intact.
pub(crate) fn page_fixed(tui: &mut Tui, text: Text<'static>) -> Result<()> {
    tui.chat.push(Source::Fixed(text));
    Ok(())
}

/// Usta's reply: orange ● line + markdown + blank line. The markdown is
/// rendered by the chat pane for the CURRENT width, so `_width` is unused
/// (kept so callers stay unchanged).
pub(crate) fn page_reply(tui: &mut Tui, reply: &str, _width: u16) -> Result<()> {
    tui.chat.push(Source::Reply(reply.to_string()));
    Ok(())
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

/// Push the user's submitted line to the chat pane (wrapped at render time).
pub(crate) fn page_user_echo(tui: &mut Tui, line: &str) -> Result<()> {
    tui.chat.push(Source::Echo(line.to_string()));
    Ok(())
}

/// Current terminal width — keeps wrapping correct after a resize (spec B3).
/// Falls back to 80 if measurement fails (wrapping doesn't break, just gets narrow).
pub(crate) fn current_width(tui: &Tui) -> u16 {
    tui.terminal.size().map(|s| s.width).unwrap_or(80)
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

/// One frame: chat pane (fills the rest) / editor (rule + 1..=8 rows) / status bar.
/// Pure over its inputs so it can be driven by a `TestBackend`.
#[allow(dead_code)] // called by the fullscreen loop in Task 3
pub(crate) fn render_screen(
    f: &mut Frame,
    chat: &mut ChatBuffer,
    editor: &InputBox,
    status: Line<'static>,
) {
    let area = f.area();
    let box_h = viewport_height(editor.desired_rows(area.width)) - 1;
    let [chat_area, box_area, status_area] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(box_h),
        Constraint::Length(1),
    ])
    .areas(area);
    let lines = chat.visible(chat_area.width, chat_area.height);
    f.render_widget(Paragraph::new(lines), chat_area);
    editor.render(f, box_area);
    f.render_widget(status, status_area);
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
    let width = current_width(tui);
    tui.set_height(viewport_height(editor.desired_rows(width)))?;
    let box_h = tui.height().saturating_sub(1);
    tui.terminal.draw(|f| {
        let [box_area, status_area] =
            Layout::vertical([Constraint::Length(box_h), Constraint::Length(1)]).areas(f.area());
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
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn screen_rows(w: u16, h: u16, chat: &mut ChatBuffer, editor: &InputBox) -> Vec<String> {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| render_screen(f, chat, editor, Line::raw("status")))
            .unwrap();
        let b = term.backend().buffer();
        (0..h)
            .map(|y| (0..w).map(|x| b[(x, y)].symbol()).collect::<String>())
            .collect()
    }

    #[test]
    fn layout_80x24_is_chat_then_editor_then_status() {
        let mut chat = ChatBuffer::default();
        chat.push(Source::Echo("hello".into()));
        chat.push(Source::Reply("**hi there**".into()));
        let editor = InputBox::new();
        let rows = screen_rows(80, 24, &mut chat, &editor);
        // chat pane = rows 0..21, rule row 21, one input row 22, status row 23
        assert!(rows[..21].iter().any(|r| r.contains("hello")));
        assert!(rows[..21].iter().any(|r| r.contains("hi there")));
        assert!(rows[21].starts_with('─'), "rule: {:?}", rows[21]);
        assert!(rows[22].starts_with("> "), "input: {:?}", rows[22]);
        assert!(rows[23].starts_with("status"));
    }

    #[test]
    fn layout_narrow_and_tiny_do_not_panic() {
        let mut chat = ChatBuffer::default();
        chat.push(Source::Echo(
            "a long line that must wrap in a narrow pane".into(),
        ));
        let editor = InputBox::new();
        let rows = screen_rows(20, 8, &mut chat, &editor);
        assert!(rows[5].starts_with('─'));
        assert!(rows[6].starts_with("> "));
        assert!(rows[7].starts_with("status"));
        assert!(
            rows[..5]
                .iter()
                .filter(|r| r.contains("pane") || r.contains("long"))
                .count()
                >= 1
        );
        // no room for a chat pane at all: status still lands on the last row
        let rows = screen_rows(20, 3, &mut chat, &editor);
        assert!(rows[2].starts_with("status"));
    }

    #[test]
    fn page_pushes_to_the_chat_buffer_not_the_terminal() {
        let prod = include_str!("page.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(
            !prod.contains("insert_before"),
            "page.rs must not use insert_before"
        );
        assert!(prod.contains("Source::Reply"));
        assert!(prod.contains("Source::Echo"));
        assert!(prod.contains("Source::Fixed"));
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
    fn draw_resizes_the_viewport_before_painting() {
        let src = include_str!("page.rs");
        let draw = src.split("pub(crate) fn draw(").nth(1).expect("draw var");
        let set = draw.find("set_height(").expect("draw set_height çağırmalı");
        let paint = draw
            .find("terminal.draw(")
            .expect("draw terminal.draw çağırmalı");
        assert!(set < paint, "yükseklik çizimden önce ayarlanmalı");
    }
}
