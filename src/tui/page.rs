//! The TUI session loop's paging layer: pushing persistent content into
//! scrollback and redrawing the live bottom region — extracted from `run.rs`
//! (cleanup round, Task 4).

use anyhow::Result;
use ratatui::layout::{Constraint, Layout};
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

/// Draw the bottom region: input box (top) + status line (bottom).
pub(crate) fn draw(
    tui: &mut Tui,
    editor: &InputBox,
    status: &Status,
    tokens: Option<u64>,
    window: u64,
    watch: Option<(bool, bool)>,
) -> Result<()> {
    tui.terminal.draw(|f| {
        let [box_area, status_area] =
            Layout::vertical([Constraint::Length(VIEWPORT_H - 1), Constraint::Length(1)])
                .areas(f.area());
        editor.render(f, box_area);
        f.render_widget(render_status(status, tokens, window, watch), status_area);
    })?;
    Ok(())
}
