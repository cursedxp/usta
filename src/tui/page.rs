//! The TUI session loop's paging layer: pushing persistent content into
//! the chat pane and redrawing the whole screen — extracted from `run.rs`
//! (cleanup round, Task 4).

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::image_attach::PastedImage;
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
    tui.chat.jump_bottom();
    tui.chat.push(Source::Echo(line.to_string()));
    Ok(())
}

/// One `image N (W×H)` line per attached image, right under the user's echo.
pub(crate) fn page_image_echo(tui: &mut Tui, images: &[PastedImage]) -> Result<()> {
    for (i, img) in images.iter().enumerate() {
        page_notice(tui, &img.echo_label(i + 1))?;
    }
    Ok(())
}

/// Current terminal width — keeps wrapping correct after a resize (spec B3).
/// Falls back to 80 if measurement fails (wrapping doesn't break, just gets narrow).
pub(crate) fn current_width(tui: &Tui) -> u16 {
    tui.terminal.size().map(|s| s.width).unwrap_or(80)
}

/// Wheel notch = this many chat lines.
const WHEEL_LINES: usize = 3;
const NEW_MESSAGE: &str = "↓ new message";

/// Chat navigation shared by every event loop: wheel + PageUp/PageDown.
/// Returns true when the event was navigation (consumed) — the caller then
/// skips its own handling. Other mouse events (motion, clicks) are swallowed
/// too: mouse capture reports them and no prompt wants them.
pub(crate) fn handle_nav(tui: &mut Tui, ev: &Event) -> bool {
    nav(&mut tui.chat, ev)
}

fn nav(chat: &mut ChatBuffer, ev: &Event) -> bool {
    match ev {
        Event::Mouse(m) => {
            match m.kind {
                MouseEventKind::ScrollUp => chat.scroll_up(WHEEL_LINES),
                MouseEventKind::ScrollDown => chat.scroll_down(WHEEL_LINES),
                _ => {}
            }
            true
        }
        Event::Key(KeyEvent {
            code: KeyCode::PageUp,
            ..
        }) => {
            chat.page_up();
            true
        }
        Event::Key(KeyEvent {
            code: KeyCode::PageDown,
            ..
        }) => {
            chat.page_down();
            true
        }
        _ => false,
    }
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
    if chat.unread() && chat_area.height > 0 {
        let row = Rect {
            y: chat_area.y + chat_area.height - 1,
            height: 1,
            ..chat_area
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(NEW_MESSAGE, theme::brand())).right_aligned()),
            row,
        );
    }
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
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
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

    fn mouse(kind: MouseEventKind) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        })
    }

    fn filled_chat(n: usize) -> ChatBuffer {
        let mut c = ChatBuffer::default();
        for i in 0..n {
            c.push(Source::Text(Text::raw(format!("l{i}"))));
        }
        c.visible(80, 5);
        c
    }

    #[test]
    fn nav_wheel_scrolls_three_lines_and_consumes_mouse_events() {
        let mut c = filled_chat(20);
        assert!(nav(&mut c, &mouse(MouseEventKind::ScrollUp)));
        assert_eq!(c.visible(80, 5)[0].spans[0].content, "l12");
        assert!(nav(&mut c, &mouse(MouseEventKind::ScrollDown)));
        assert!(c.is_following());
        // motion / clicks are swallowed but change nothing
        assert!(nav(&mut c, &mouse(MouseEventKind::Moved)));
        assert!(c.is_following());
    }

    #[test]
    fn nav_page_keys_scroll_a_page_and_other_keys_pass_through() {
        let mut c = filled_chat(40);
        let key = |code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
        assert!(nav(&mut c, &key(KeyCode::PageUp)));
        assert!(!c.is_following());
        assert!(nav(&mut c, &key(KeyCode::PageDown)));
        assert!(c.is_following());
        assert!(!nav(&mut c, &key(KeyCode::Char('a'))));
        assert!(!nav(&mut c, &Event::Paste("x".into())));
    }

    #[test]
    fn indicator_shows_only_while_unread() {
        let mut chat = filled_chat(30);
        let editor = InputBox::new();
        let rows = screen_rows(40, 10, &mut chat, &editor); // pane = 7 rows
        assert!(!rows[6].contains("new message"));
        chat.scroll_up(3);
        chat.push(Source::Text(Text::raw("fresh")));
        let rows = screen_rows(40, 10, &mut chat, &editor);
        assert!(
            rows[6].trim_end().ends_with("↓ new message"),
            "{:?}",
            rows[6]
        );
        chat.scroll_down(100);
        let rows = screen_rows(40, 10, &mut chat, &editor);
        assert!(!rows[6].contains("new message"));
    }

    /// Source pin: every loop that reads the EventStream routes through handle_nav
    /// (otherwise the wheel is silently ignored in that prompt, like Resize once was).
    #[test]
    fn mouse_and_page_keys_are_handled_in_every_event_loop() {
        for (name, src, loops) in [
            ("run.rs", include_str!("run.rs"), 1),
            ("ask.rs", include_str!("ask.rs"), 2),
            ("entry.rs", include_str!("entry.rs"), 1),
            ("intro.rs", include_str!("intro.rs"), 1),
        ] {
            let prod = src.split("#[cfg(test)]").next().unwrap();
            assert_eq!(
                prod.matches("page::handle_nav(").count(),
                loops,
                "{name}: expected {loops} handle_nav call(s)"
            );
        }
        let own = include_str!("page.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(own.contains("Event::Mouse"));
        assert!(own.contains("KeyCode::PageUp") && own.contains("KeyCode::PageDown"));
    }
}
