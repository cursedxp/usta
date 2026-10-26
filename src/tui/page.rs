//! The TUI session loop's paging layer: pushing persistent content into
//! the chat pane and redrawing the whole screen — extracted from `run.rs`
//! (cleanup round, Task 4).

use anyhow::Result;
use crossterm::event::{Event, KeyCode, MouseButton, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Text};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::image_attach::PastedImage;
use crate::tui::chat::ChatBuffer;
use crate::tui::editor::InputBox;
use crate::tui::status::{render_status, Status};
use crate::tui::term::{viewport_height, Tui};
use crate::tui::theme;
use crate::ui;

/// A pre-sized panel (welcome box): `f` builds it for the current
/// width on every render, so a resize re-lays it out; narrower than its minimum it is clipped.
pub(crate) fn page_fixed(tui: &mut Tui, f: impl Fn(u16) -> Text<'static> + 'static) -> Result<()> {
    tui.chat.panel(f);
    Ok(())
}

/// Usta's reply: orange ● line + markdown + blank line. The markdown is
/// rendered by the chat pane for the CURRENT width, so `_width` is unused
/// (kept so callers stay unchanged).
pub(crate) fn page_reply(tui: &mut Tui, reply: &str, _width: u16) -> Result<()> {
    tui.chat.reply(reply);
    Ok(())
}

pub(crate) fn page_notice(tui: &mut Tui, msg: &str) -> Result<()> {
    tui.chat.notice(msg);
    Ok(())
}
pub(crate) fn page_warn(tui: &mut Tui, msg: &str) -> Result<()> {
    tui.chat.warn(msg);
    Ok(())
}
pub(crate) fn page_error(tui: &mut Tui, msg: &str) -> Result<()> {
    tui.chat.error(msg);
    Ok(())
}

/// Flush the notices buffered by ui::notice/ui::warn while the TUI was live
/// (see `ChatBuffer::flush_notices` for the routing).
pub(crate) fn flush_notices(tui: &mut Tui) -> Result<()> {
    tui.chat.flush_notices(ui::drain_tui_notices());
    Ok(())
}

/// Push the user's submitted line to the chat pane (wrapped at render time).
pub(crate) fn page_user_echo(tui: &mut Tui, line: &str) -> Result<()> {
    tui.chat.user_echo(line);
    Ok(())
}

/// One `image N (W×H)` line per attached image, right under the user's echo.
pub(crate) fn page_image_echo(tui: &mut Tui, images: &[PastedImage]) -> Result<()> {
    tui.chat.image_echo(images);
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

/// Chat navigation + selection shared by every event loop: wheel, PageUp/PageDown,
/// left-button drag to select (copied on release). Returns true when the event was
/// consumed — the caller then skips its own handling. Other mouse events (motion,
/// other buttons) are swallowed too: mouse capture reports them and no prompt wants them.
pub(crate) fn handle_nav(tui: &mut Tui, ev: &Event) -> bool {
    nav(&mut tui.chat, ev, &mut crate::tui::select::copy)
}

fn nav(chat: &mut ChatBuffer, ev: &Event, copy: &mut dyn FnMut(&str)) -> bool {
    match ev {
        Event::Mouse(m) => {
            match m.kind {
                MouseEventKind::ScrollUp => chat.scroll_up(WHEEL_LINES),
                MouseEventKind::ScrollDown => chat.scroll_down(WHEEL_LINES),
                MouseEventKind::Down(MouseButton::Left) => chat.mouse_down(m.column, m.row),
                MouseEventKind::Drag(MouseButton::Left) => chat.mouse_drag(m.column, m.row),
                MouseEventKind::Up(MouseButton::Left) => {
                    if let Some(text) = chat.mouse_up() {
                        copy(&text);
                    }
                }
                _ => {}
            }
            true
        }
        Event::Paste(_) => {
            chat.clear_flash();
            false
        }
        Event::Key(k) => {
            chat.clear_flash();
            match k.code {
                KeyCode::PageUp => {
                    chat.page_up();
                    true
                }
                KeyCode::PageDown => {
                    chat.page_down();
                    true
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// Refresh ratatui's idea of the screen size after a terminal resize. The next
/// `draw()` (the caller's loop redraws every iteration) repaints the whole
/// screen and `ChatBuffer` re-wraps from source at the new width. An unchanged
/// size is a no-op (`autoresize()` only resizes when the area differs).
pub(crate) fn handle_resize(tui: &mut Tui) -> Result<()> {
    tui.chat.clear_selection(); // flat indices change with the width
    tui.terminal.autoresize()?;
    Ok(())
}

/// Paint `label` right-aligned on the editor's rule row (decoration only), keeping
/// the rest of the rule. Shared slot for the "new message" indicator.
pub(crate) fn rule_label(buf: &mut Buffer, box_area: Rect, label: &str, style: Style) {
    let w = UnicodeWidthStr::width(label) as u16;
    if box_area.height == 0 || box_area.width < w {
        return;
    }
    buf.set_string(box_area.x + box_area.width - w, box_area.y, label, style);
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
    let lines = chat.visible_in(chat_area);
    f.render_widget(Paragraph::new(lines), chat_area);
    if let Some(sel) = chat.selection() {
        let top = chat.view_top();
        crate::tui::select::highlight(f.buffer_mut(), chat_area, top, &sel);
    }
    editor.render(f, box_area);
    if let Some(label) = chat.flash() {
        rule_label(f.buffer_mut(), box_area, label, theme::brand());
    } else if chat.unread() {
        rule_label(f.buffer_mut(), box_area, NEW_MESSAGE, theme::brand());
    }
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
    // Every loop draws: flushing here shows notices raised in intro/entry/confirm too.
    flush_notices(tui)?;
    let line = render_status(status, tokens, window, watch, verify_failing);
    let chat = &mut tui.chat;
    tui.terminal
        .draw(|f| render_screen(f, chat, editor, line))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::chat::Source;
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ratatui::backend::TestBackend;
    use ratatui::style::Modifier;
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
        let chat = include_str!("chat.rs")
            .split("#[cfg(test)]\nmod tests")
            .next()
            .unwrap();
        assert!(chat.contains("Source::Reply"));
        assert!(chat.contains("Source::Echo"));
        assert!(chat.contains("Source::Panel"));
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
            draw.contains("flush_notices(tui)"),
            "draw must flush buffered notices"
        );
        assert!(
            !draw.contains("set_height("),
            "no viewport rebuild in fullscreen"
        );
    }

    #[test]
    fn run_loop_no_longer_flushes_notices_itself() {
        let src = include_str!("run.rs");
        let production = src.split("#[cfg(test)]").next().unwrap();
        assert!(
            !production.contains("flush_notices("),
            "draw owns the notice flush"
        );
    }

    fn mouse_at(kind: MouseEventKind, column: u16, row: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    }

    fn mouse(kind: MouseEventKind) -> Event {
        mouse_at(kind, 0, 0)
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
        assert!(nav(&mut c, &mouse(MouseEventKind::ScrollUp), &mut |_| {}));
        assert_eq!(c.visible(80, 5)[0].spans[0].content, "l12");
        assert!(nav(&mut c, &mouse(MouseEventKind::ScrollDown), &mut |_| {}));
        assert!(c.is_following());
        // motion / clicks are swallowed but change nothing
        assert!(nav(&mut c, &mouse(MouseEventKind::Moved), &mut |_| {}));
        assert!(c.is_following());
    }

    #[test]
    fn nav_page_keys_scroll_a_page_and_other_keys_pass_through() {
        let mut c = filled_chat(40);
        let key = |code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
        assert!(nav(&mut c, &key(KeyCode::PageUp), &mut |_| {}));
        assert!(!c.is_following());
        assert!(nav(&mut c, &key(KeyCode::PageDown), &mut |_| {}));
        assert!(c.is_following());
        assert!(!nav(&mut c, &key(KeyCode::Char('a')), &mut |_| {}));
        c.visible_in(Rect::new(0, 0, 80, 5));
        c.mouse_down(0, 1);
        c.mouse_drag(2, 2);
        c.mouse_up();
        assert!(c.flash().is_some());
        assert!(!nav(&mut c, &Event::Paste("x".into()), &mut |_| {}));
        assert!(c.flash().is_none(), "paste clears the flash");
    }

    #[test]
    fn nav_drag_copies_on_release_and_flashes() {
        let mut c = filled_chat(20);
        c.visible_in(Rect::new(0, 0, 80, 5));
        let mut copied = Vec::new();
        let mut cp = |t: &str| copied.push(t.to_string());
        let left = MouseButton::Left;
        assert!(nav(
            &mut c,
            &mouse_at(MouseEventKind::Down(left), 0, 1),
            &mut cp
        ));
        assert!(nav(
            &mut c,
            &mouse_at(MouseEventKind::Drag(left), 2, 2),
            &mut cp
        ));
        assert!(nav(
            &mut c,
            &mouse_at(MouseEventKind::Up(left), 2, 2),
            &mut cp
        ));
        assert_eq!(copied, vec!["l16\nl17".to_string()]);
        assert_eq!(c.flash(), Some(crate::tui::chat::COPIED));
        assert!(c.selection().is_none());
        // any key clears the flash and still passes non-nav keys through
        let key = Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(!nav(&mut c, &key, &mut |_| {}));
        assert!(c.flash().is_none());
    }

    #[test]
    fn nav_drag_below_the_pane_scrolls_down() {
        let mut c = filled_chat(40);
        c.visible_in(Rect::new(0, 0, 80, 5));
        c.scroll_up(6);
        let before = c.offset();
        let left = MouseButton::Left;
        nav(
            &mut c,
            &mouse_at(MouseEventKind::Down(left), 0, 0),
            &mut |_| {},
        );
        nav(
            &mut c,
            &mouse_at(MouseEventKind::Drag(left), 0, 9),
            &mut |_| {},
        );
        assert_eq!(c.offset(), before - 1);
    }

    #[test]
    fn selection_renders_reversed_and_flash_takes_the_rule_slot() {
        let mut chat = filled_chat(60);
        let editor = InputBox::new();
        screen_rows(80, 24, &mut chat, &editor); // records the 80x21 pane
        chat.mouse_down(0, 19);
        chat.mouse_drag(2, 20);
        let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
        term.draw(|f| render_screen(f, &mut chat, &editor, Line::raw("status")))
            .unwrap();
        let b = term.backend().buffer();
        assert!(b[(0, 19)].modifier.contains(Modifier::REVERSED));
        assert!(b[(2, 20)].modifier.contains(Modifier::REVERSED));
        assert!(!b[(3, 20)].modifier.contains(Modifier::REVERSED));
        chat.mouse_up();
        chat.scroll_up(3);
        chat.push(Source::Text(Text::raw("fresh"))); // unread too: flash wins the slot
        let rows = screen_rows(80, 24, &mut chat, &editor);
        assert!(rows[21].starts_with('─'));
        assert!(rows[21].trim_end().ends_with("copied"), "{:?}", rows[21]);
    }

    #[test]
    fn indicator_sits_on_the_rule_row_not_the_last_chat_line() {
        let mut chat = filled_chat(60);
        let editor = InputBox::new();
        chat.scroll_up(3);
        chat.push(Source::Text(Text::raw("fresh")));
        let rows = screen_rows(80, 24, &mut chat, &editor);
        assert!(
            !rows[20].trim().is_empty(),
            "last chat row keeps content: {:?}",
            rows[20]
        );
        assert!(!rows[20].contains("new message"));
        assert!(rows[21].starts_with('─'), "{:?}", rows[21]);
        assert!(
            rows[21].trim_end().ends_with("↓ new message"),
            "{:?}",
            rows[21]
        );
        chat.scroll_down(100);
        let rows = screen_rows(80, 24, &mut chat, &editor);
        assert!(!rows[21].contains("new message"));
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
