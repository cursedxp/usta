//! The TUI session loop's paging layer: pushing persistent content into
//! the chat pane and redrawing the whole screen — extracted from `run.rs`
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

/// Refresh ratatui's idea of the screen size after a terminal resize. The next
/// `draw()` (the caller's loop redraws every iteration) repaints the whole
/// screen and `ChatBuffer` re-wraps from source at the new width. An unchanged
/// size is a no-op (`autoresize()` only resizes when the area differs).
pub(crate) fn handle_resize(tui: &mut Tui) -> Result<()> {
    tui.terminal.autoresize()?;
    Ok(())
}

/// One frame: chat pane (fills the rest) / editor (rule + 1..=8 rows) / status bar.
/// Pure over its inputs so it can be driven by a `TestBackend`.
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

/// Paint the whole screen: chat pane + input box + status line.
pub(crate) fn draw(
    tui: &mut Tui,
    editor: &InputBox,
    status: &Status,
    tokens: Option<u64>,
    window: u64,
    watch: Option<(bool, bool, usize)>,
    verify_failing: bool,
) -> Result<()> {
    let line = render_status(status, tokens, window, watch, verify_failing);
    let chat = &mut tui.chat;
    tui.terminal
        .draw(|f| render_screen(f, chat, editor, line))?;
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
    fn draw_paints_through_render_screen() {
        let src = include_str!("page.rs");
        let draw = src.split("pub(crate) fn draw(").nth(1).expect("draw var");
        let draw = draw.split("#[cfg(test)]").next().unwrap();
        assert!(
            draw.contains("render_screen("),
            "draw must use render_screen"
        );
        assert!(
            !draw.contains("set_height("),
            "no viewport rebuild in fullscreen"
        );
    }
}
