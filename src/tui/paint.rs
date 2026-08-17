//! Pure text/style builders and key classifiers for the TUI session loop —
//! nothing here touches `&mut Tui` (spec B2/S1).

use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::text::{Line, Span, Text};

use crate::tui::convert::ansi_to_text;
use crate::tui::theme;

/// System-notice voice, three scan-levels sharing one shape: glyph + one space +
/// the message, color reinforcing the glyph (design tokens 02, mockup 03). The
/// message text is never altered — only the glyph+color prefix is chosen. These
/// pure builders are unit-tested; the `page_*` wrappers just push them to scrollback.
/// info `·` dim (the quiet default).
pub(crate) fn notice_line(msg: &str) -> Text<'static> {
    ansi_to_text(&format!("\x1b[2m{} {msg}\x1b[0m", theme::G_INFO))
}
/// warning `⚠` amber — something needs noticing.
pub(crate) fn warn_line(msg: &str) -> Text<'static> {
    ansi_to_text(&format!(
        "\x1b[38;5;{}m{} {msg}\x1b[0m",
        theme::WARN_IDX,
        theme::G_WARN
    ))
}
/// error `✗` red — a genuine stop.
pub(crate) fn error_line(msg: &str) -> Text<'static> {
    ansi_to_text(&format!(
        "\x1b[38;5;{}m{} {msg}\x1b[0m",
        theme::ERROR_IDX,
        theme::G_ERR
    ))
}

/// User block: blank separator line + orange `❯ ` prefix + NORMAL-colored text.
/// DO NOT USE DIM — it blended into the background and became invisible on dark
/// themes (spec S1). In multi-line submissions, continuation lines are indented
/// 2 spaces — the pasted structure is preserved.
pub(crate) fn user_echo_text(line: &str, width: u16) -> Text<'static> {
    // Prefix is 2 columns ("❯ " / "  "); the text wraps to the width minus this
    // allowance so a long message isn't cut off on one line (page_reply already
    // wraps markdown — echo was getting truncated when it didn't wrap). The first
    // VISUAL line gets ❯, the rest get 2 spaces — both multi-line paste and
    // single-line wrap read aligned this way.
    let inner = (width as usize).saturating_sub(2).max(1);
    let mut lines: Vec<Line> = vec![Line::raw("")];
    let mut first_visual = true;
    for logical in line.split('\n') {
        for chunk in wrap_cells(logical, inner) {
            let prefix = if first_visual {
                Span::styled(format!("{} ", theme::G_PROMPT), theme::brand())
            } else {
                Span::raw("  ")
            };
            lines.push(Line::from(vec![prefix, Span::raw(chunk)]));
            first_visual = false;
        }
    }
    Text::from(lines)
}

/// Split text to CELL width (unicode-width) — character-based, not word-based,
/// consistent with the input box's `wrap_visual`. Empty input → single blank line.
fn wrap_cells(s: &str, width: usize) -> Vec<String> {
    use unicode_width::UnicodeWidthChar;
    let width = width.max(1);
    let mut rows: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut col = 0usize;
    for ch in s.chars() {
        let w = ch.width().unwrap_or(0).max(1);
        if col + w > width && !cur.is_empty() {
            rows.push(std::mem::take(&mut cur));
            col = 0;
        }
        cur.push(ch);
        col += w;
    }
    rows.push(cur);
    rows
}

/// Meaning of a keypress in locked mode — pure, testable (spec B2).
pub(crate) enum LockedKey {
    /// Key to be processed by the editor (including Enter — Enter is swallowed but counts as an edit).
    Edit,
    /// Ctrl-C / Ctrl-D — cancel-request step.
    CancelRequest,
}

pub(crate) fn classify_locked_key(k: KeyEvent) -> LockedKey {
    use crossterm::event::KeyModifiers;
    if k.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(k.code, KeyCode::Char('c') | KeyCode::Char('d'))
    {
        LockedKey::CancelRequest
    } else {
        LockedKey::Edit
    }
}

/// Project directory with `$HOME` → `~` abbreviation.
pub(crate) fn short_dir(p: &Path) -> String {
    let s = p.display().to_string();
    match dirs::home_dir() {
        Some(h) => s.replace(&h.display().to_string(), "~"),
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::Modifier;

    fn line_text(l: &ratatui::text::Line) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn user_echo_prefixes_first_line_and_indents_rest() {
        // Wide width → no wrapping, only \n splitting.
        let t = user_echo_text("satır1\nsatır2", 80);
        let lines: Vec<String> = t.lines.iter().map(line_text).collect();
        // [0] blank separator line, [1] ❯ + text, [2] indented continuation.
        assert_eq!(lines[0], "");
        assert_eq!(lines[1], "❯ satır1");
        assert_eq!(lines[2], "  satır2");
    }

    #[test]
    fn user_echo_wraps_long_line_to_width() {
        // 50 'a's, width 20 → inner width 18 → 18+18+14 = 3 content lines.
        // A long single line is NOT cut off, it wraps (bug: page_reply wraps, echo didn't).
        let t = user_echo_text(&"a".repeat(50), 20);
        let lines: Vec<String> = t.lines.iter().map(line_text).collect();
        assert_eq!(lines[0], "");
        assert!(
            lines[1].starts_with("❯ "),
            "ilk görsel satır ❯: {:?}",
            lines[1]
        );
        assert_eq!(
            lines[1].chars().filter(|c| *c == 'a').count(),
            18,
            "ilk satır iç genişlik kadar"
        );
        assert!(
            lines[2].starts_with("  "),
            "devam satırı girintili: {:?}",
            lines[2]
        );
        let total: usize = lines
            .iter()
            .map(|l| l.chars().filter(|c| *c == 'a').count())
            .sum();
        assert_eq!(total, 50, "hiçbir karakter kaybolmaz");
        assert!(
            t.lines.len() >= 4,
            "birden çok görsel satıra bölündü: {}",
            t.lines.len()
        );
    }

    #[test]
    fn notice_layers_carry_glyph_and_color() {
        // info: `·` dim, no hard fg color (survives light/dark).
        let n = notice_line("resuming: rust");
        assert!(line_text(&n.lines[0]).starts_with("· "));
        assert!(line_text(&n.lines[0]).contains("resuming: rust"));

        // warn: `⚠` amber (theme::WARN index), text verbatim.
        let w = warn_line("context filling up");
        assert!(line_text(&w.lines[0]).starts_with("⚠ "));
        let wfg = w.lines[0].spans.iter().find_map(|s| s.style.fg);
        assert_eq!(wfg, Some(theme::WARN));

        // error: `✗` red (theme::ERROR index), text verbatim.
        let e = error_line("no goal set for this topic");
        assert!(line_text(&e.lines[0]).starts_with("✗ "));
        let efg = e.lines[0].spans.iter().find_map(|s| s.style.fg);
        assert_eq!(efg, Some(theme::ERROR));
    }

    #[test]
    fn user_echo_text_is_not_dim() {
        let t = user_echo_text("merhaba", 80);
        // No span carries DIM — that was the root of the visibility issue (spec S1).
        for l in &t.lines {
            for s in &l.spans {
                assert!(
                    !s.style.add_modifier.contains(Modifier::DIM),
                    "DIM span: {:?}",
                    s.content
                );
            }
        }
    }

    #[test]
    fn user_echo_prefix_is_orange() {
        let t = user_echo_text("x", 80);
        let first = &t.lines[1].spans[0];
        assert_eq!(first.content.as_ref(), "❯ ");
        assert_eq!(first.style.fg, Some(theme::BRAND));
    }

    #[test]
    fn classify_locked_key_ctrl_c_and_d_are_cancel_requests() {
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert!(matches!(
            classify_locked_key(ctrl_c),
            LockedKey::CancelRequest
        ));
        assert!(matches!(
            classify_locked_key(ctrl_d),
            LockedKey::CancelRequest
        ));
    }

    #[test]
    fn classify_locked_key_enter_and_chars_are_edits() {
        assert!(matches!(
            classify_locked_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            LockedKey::Edit
        ));
        assert!(matches!(
            classify_locked_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
            LockedKey::Edit
        ));
    }
}
