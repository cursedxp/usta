//! Live input box: tui-input editor state + Vec-based up/down
//! history. The TUI-path counterpart of Rustyline. Spec §6.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use tui_input::backend::crossterm::to_input_request;
use tui_input::Input;

use crate::tui::convert;
use crate::tui::theme;

/// Cap on the input frame's content-line count (see `InputBox::frame_lines`),
/// also bounded by half the screen height, whichever is smaller.
pub(crate) const INPUT_MAX_ROWS: usize = 10;

/// Number of visual content rows `value` wraps to at `width` (the TERMINAL
/// width — the 2-cell `> ` / `  ` prefix is subtracted internally), capped
/// at `INPUT_MAX_ROWS`. Always returns at least 1. It shares the `wrap_visual`
/// computation with `frame_lines`, but NOT the cap: `frame_lines` additionally
/// clamps to half the screen height, so on screens shorter than
/// `2 * INPUT_MAX_ROWS` the two can return different numbers — they only
/// diverge once the wrapped row count exceeds the half-screen cap. This is
/// the row count before any screen-height cap; the two are not substitutable.
//
// Not reached from production today: `frame_lines` returns the frame's lines
// directly, so the paging layer never needs the row count on its own. Kept,
// with its tests, as the pinned definition of the frame's row arithmetic.
#[allow(dead_code)]
pub(crate) fn content_rows(value: &str, width: u16) -> usize {
    let inner_w = width.saturating_sub(2) as usize;
    let (rows, _cur_row, _cur_col) = wrap_visual(value, inner_w, 0);
    rows.len().min(INPUT_MAX_ROWS)
}

/// Result of key handling — the loop behaves accordingly.
#[derive(Debug)]
pub enum Action {
    None,
    /// Trimmed, non-empty line was submitted.
    Submit(String),
    /// Ctrl-C / Ctrl-D — shutdown flow.
    Exit,
}

pub struct InputBox {
    input: Input,
    history: Vec<String>,
    /// None = fresh line; Some(i) = history[i] is being shown.
    cursor: Option<usize>,
    /// Fresh text from before entering history — comes back with Down.
    stash: String,
}

impl InputBox {
    pub fn new() -> Self {
        Self {
            input: Input::default(),
            history: Vec::new(),
            cursor: None,
            stash: String::new(),
        }
    }

    // Editor's public API — the loop gets the line from Action::Submit, and the
    // editor draws the cursor itself, so these aren't called right now (value() is used in tests).
    pub fn value(&self) -> &str {
        self.input.value()
    }
    #[allow(dead_code)]
    pub fn visual_cursor(&self) -> usize {
        self.input.visual_cursor()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('d'))
        {
            return Action::Exit;
        }
        // Newline insert (multi-line input): Shift+Enter / Alt+Enter (modern terminals via
        // kitty keyboard protocol) or Ctrl+J (LF — universal fallback, works everywhere).
        // Bare Enter still submits.
        let newline = (matches!(key.code, KeyCode::Enter)
            && key
                .modifiers
                .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT))
            || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('j'));
        if newline {
            self.cursor = None;
            self.input.handle(tui_input::InputRequest::InsertChar('\n'));
            return Action::None;
        }
        match key.code {
            KeyCode::Enter => {
                let line = self.input.value().trim().to_string();
                if line.is_empty() {
                    return Action::None;
                }
                self.history.push(line.clone());
                self.input.reset();
                self.cursor = None;
                Action::Submit(line)
            }
            KeyCode::Up => {
                self.recall_prev();
                Action::None
            }
            KeyCode::Down => {
                self.recall_next();
                Action::None
            }
            _ => {
                self.cursor = None;
                if let Some(req) = to_input_request(&Event::Key(key)) {
                    self.input.handle(req);
                }
                Action::None
            }
        }
    }

    /// Insert pasted text at the cursor position (bracketed paste). Line breaks
    /// are PRESERVED — so the model can see list/log structure; since the box is
    /// single-line, they're shown as ⏎ in the render. CRLF → LF is normalized.
    /// Drop any half-typed text (confirm cancelled — don't leak it into the next prompt).
    pub fn clear(&mut self) {
        self.input.reset();
        self.cursor = None;
    }

    pub fn insert_str(&mut self, s: &str) {
        self.cursor = None;
        let cleaned = s.replace("\r\n", "\n").replace('\r', "\n");
        for ch in cleaned.chars() {
            self.input.handle(tui_input::InputRequest::InsertChar(ch));
        }
    }

    fn recall_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next = match self.cursor {
            None => {
                self.stash = self.input.value().to_string();
                self.history.len() - 1
            }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.cursor = Some(next);
        self.input = Input::new(self.history[next].clone());
    }

    fn recall_next(&mut self) {
        match self.cursor {
            None => {}
            Some(i) if i + 1 < self.history.len() => {
                self.cursor = Some(i + 1);
                self.input = Input::new(self.history[i + 1].clone());
            }
            Some(_) => {
                self.cursor = None;
                self.input = Input::new(std::mem::take(&mut self.stash));
            }
        }
    }

    /// Borderless input frame: a full-width `─` rule (styled `theme::DIM`),
    /// the wrapped content (1..=`INPUT_MAX_ROWS` lines, further capped to at
    /// most half of `screen_h`), and a matching rule below — `N + 2` lines
    /// total, index 0 being the top rule. Long text wraps at `width - 2`
    /// (only the `> ` / `  ` prefix is subtracted, no side borders); once the
    /// cap is hit, the vertical window follows the cursor. Returns the frame's ANSI-styled lines, the
    /// cursor's line index into that vec (always in `1..=content_rows`), and
    /// the cursor's column (prefix-adjusted, clamped to `width - 1`).
    /// Degenerate `width`/`screen_h` (0, 1, 2) never panic.
    pub(crate) fn frame_lines(&self, width: u16, screen_h: u16) -> (Vec<String>, u16, u16) {
        let inner_w = width.saturating_sub(2) as usize;
        let (rows, cur_row, cur_col) =
            wrap_visual(self.input.value(), inner_w, self.input.visual_cursor());
        let half_screen = (screen_h as usize) / 2;
        let cap = half_screen.clamp(1, INPUT_MAX_ROWS);
        let n = rows.len().min(cap);
        let start = (cur_row + 1).saturating_sub(n);

        let rule = "─".repeat(width as usize);
        let mut text_lines: Vec<Line> = Vec::with_capacity(n + 2);
        text_lines.push(Line::from(Span::styled(
            rule.clone(),
            Style::default().fg(theme::DIM),
        )));
        for (i, r) in rows.iter().enumerate().skip(start).take(n) {
            let prefix = if i == 0 { "> " } else { "  " };
            text_lines.push(Line::from(vec![
                Span::styled(prefix, theme::brand()),
                Span::raw(r.clone()),
            ]));
        }
        text_lines.push(Line::from(Span::styled(
            rule,
            Style::default().fg(theme::DIM),
        )));

        let lines = convert::text_to_ansi_lines(&Text::from(text_lines), width);
        let cursor_line = 1 + (cur_row - start) as u16;
        let cursor_col = (2 + cur_col as u16).min(width.saturating_sub(1));
        (lines, cursor_line, cursor_col)
    }
}

/// Wrap the value into visual lines — width is CELL-based (unicode-width),
/// `\n` splits the line and shows as ⏎ at line end. Also returns the
/// row/column equivalent of the cursor (character index). Pure — render draws
/// from this, the Submit value is UNCHANGED.
fn wrap_visual(value: &str, width: usize, cursor: usize) -> (Vec<String>, usize, usize) {
    use unicode_width::UnicodeWidthChar;
    let width = width.max(1);
    let mut rows: Vec<String> = vec![String::new()];
    let mut col = 0usize;
    let (mut cur_row, mut cur_col) = (0usize, 0usize);
    for (i, ch) in value.chars().enumerate() {
        let (ch, w, breaks) = if ch == '\n' {
            ('⏎', 1usize, true)
        } else {
            (ch, ch.width().unwrap_or(0).max(1), false)
        };
        if col + w > width {
            rows.push(String::new());
            col = 0;
        }
        if i == cursor {
            cur_row = rows.len() - 1;
            cur_col = col;
        }
        rows.last_mut().expect("rows boş olamaz").push(ch);
        col += w;
        if breaks {
            rows.push(String::new());
            col = 0;
        }
    }
    if cursor >= value.chars().count() {
        cur_row = rows.len() - 1;
        cur_col = col;
    }
    (rows, cur_row, cur_col)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }
    fn code(k: KeyCode) -> KeyEvent {
        KeyEvent::new(k, KeyModifiers::NONE)
    }

    fn type_str(b: &mut InputBox, s: &str) {
        for c in s.chars() {
            assert!(matches!(b.handle_key(key(c)), Action::None));
        }
    }

    #[test]
    fn typing_and_submit_returns_trimmed_line_and_clears() {
        let mut b = InputBox::new();
        type_str(&mut b, "  merhaba usta  ");
        match b.handle_key(code(KeyCode::Enter)) {
            Action::Submit(s) => assert_eq!(s, "merhaba usta"),
            other => panic!("Submit bekleniyordu: {other:?}"),
        }
        assert_eq!(b.value(), "");
    }

    #[test]
    fn empty_submit_is_none() {
        let mut b = InputBox::new();
        assert!(matches!(b.handle_key(code(KeyCode::Enter)), Action::None));
    }

    #[test]
    fn ctrl_c_and_ctrl_d_exit() {
        let mut b = InputBox::new();
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(matches!(b.handle_key(ctrl_c), Action::Exit));
        let ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert!(matches!(b.handle_key(ctrl_d), Action::Exit));
    }

    #[test]
    fn history_up_down_recalls_submitted_lines() {
        let mut b = InputBox::new();
        type_str(&mut b, "ilk");
        b.handle_key(code(KeyCode::Enter));
        type_str(&mut b, "iki");
        b.handle_key(code(KeyCode::Enter));
        b.handle_key(code(KeyCode::Up));
        assert_eq!(b.value(), "iki");
        b.handle_key(code(KeyCode::Up));
        assert_eq!(b.value(), "ilk");
        b.handle_key(code(KeyCode::Down));
        assert_eq!(b.value(), "iki");
        b.handle_key(code(KeyCode::Down));
        assert_eq!(b.value(), "");
    }

    #[test]
    fn insert_str_preserves_newlines_and_normalizes_crlf() {
        let mut b = InputBox::new();
        b.insert_str("satır1\r\nsatır2\rsatır3");
        assert_eq!(b.value(), "satır1\nsatır2\nsatır3");
        // Pasting doesn't trigger Submit; after Enter there's a single message, structure preserved.
        match b.handle_key(code(KeyCode::Enter)) {
            Action::Submit(s) => assert_eq!(s, "satır1\nsatır2\nsatır3"),
            other => panic!("Submit bekleniyordu: {other:?}"),
        }
    }

    #[test]
    fn insert_str_appends_at_cursor_after_typing() {
        let mut b = InputBox::new();
        type_str(&mut b, "log: ");
        b.insert_str("a\nb");
        assert_eq!(b.value(), "log: a\nb");
    }

    #[test]
    fn wrap_visual_wraps_long_text_at_cell_width() {
        // 10-cell width, 25 characters → 3 lines (10+10+5).
        let v = "a".repeat(25);
        let (rows, cur_row, cur_col) = wrap_visual(&v, 10, 25);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].chars().count(), 10);
        assert_eq!(rows[2].chars().count(), 5);
        // If the cursor is at the end, it's at the end of the last line.
        assert_eq!((cur_row, cur_col), (2, 5));
    }

    #[test]
    fn wrap_visual_breaks_on_newline_with_visible_marker() {
        let (rows, _, _) = wrap_visual("ab\ncd", 10, 0);
        assert_eq!(rows, vec!["ab⏎".to_string(), "cd".to_string()]);
    }

    #[test]
    fn wrap_visual_cursor_mid_text_lands_on_correct_row() {
        let v = "a".repeat(15); // width 10 → row0: 0..9, row1: 10..14
        let (_, cur_row, cur_col) = wrap_visual(&v, 10, 12);
        assert_eq!((cur_row, cur_col), (1, 2));
    }

    #[test]
    fn wrap_visual_turkish_chars_count_one_cell() {
        let (rows, _, _) = wrap_visual("çğşöüiçğşöüi", 6, 0); // 12 characters, 6 cells → 2 lines
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], "çğşöüi");
    }

    #[test]
    fn wrap_visual_empty_value_single_empty_row() {
        let (rows, cur_row, cur_col) = wrap_visual("", 10, 0);
        assert_eq!(rows, vec![String::new()]);
        assert_eq!((cur_row, cur_col), (0, 0));
    }

    #[test]
    fn turkish_chars_edit_correctly() {
        let mut b = InputBox::new();
        type_str(&mut b, "çğşü");
        b.handle_key(code(KeyCode::Backspace));
        assert_eq!(b.value(), "çğş");
    }

    #[test]
    fn shift_enter_inserts_newline_not_submit() {
        let mut b = InputBox::new();
        type_str(&mut b, "a");
        let se = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
        assert!(matches!(b.handle_key(se), Action::None));
        type_str(&mut b, "b");
        assert_eq!(b.value(), "a\nb");
        match b.handle_key(code(KeyCode::Enter)) {
            Action::Submit(s) => assert_eq!(s, "a\nb"),
            o => panic!("Submit bekleniyordu: {o:?}"),
        }
    }

    #[test]
    fn ctrl_j_inserts_newline() {
        let mut b = InputBox::new();
        type_str(&mut b, "x");
        let cj = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL);
        assert!(matches!(b.handle_key(cj), Action::None));
        assert_eq!(b.value(), "x\n");
    }

    #[test]
    fn alt_enter_inserts_newline() {
        let mut b = InputBox::new();
        let ae = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);
        assert!(matches!(b.handle_key(ae), Action::None));
        assert_eq!(b.value(), "\n");
    }

    // ── Borderless frame (frame_lines / content_rows) ──────────────────────

    /// Strip ANSI escape sequences so assertions can check the DISPLAY
    /// content of a `frame_lines` line without tripping over the styling
    /// `text_to_ansi_lines` wraps around it.
    fn strip_ansi(s: &str) -> String {
        let bytes = s.as_bytes();
        let mut out = String::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == 0x1b {
                let mut j = i + 1;
                if j < bytes.len() && bytes[j] == b'[' {
                    j += 1;
                    while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
                        j += 1;
                    }
                    if j < bytes.len() {
                        j += 1;
                    }
                }
                i = j;
                continue;
            }
            let ch = s[i..].chars().next().expect("i is a char boundary");
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    #[test]
    fn content_rows_empty_is_one() {
        assert_eq!(content_rows("", 20), 1);
    }

    #[test]
    fn content_rows_exactly_fits_is_one() {
        // width 12 -> inner width 10; a 10-char value fits on one row.
        let v = "a".repeat(10);
        assert_eq!(content_rows(&v, 12), 1);
    }

    #[test]
    fn content_rows_one_char_over_is_two() {
        let v = "a".repeat(11);
        assert_eq!(content_rows(&v, 12), 2);
    }

    #[test]
    fn content_rows_newline_increases_it() {
        assert_eq!(content_rows("ab\ncd", 20), 2);
    }

    #[test]
    fn content_rows_caps_at_input_max_rows() {
        let v = "x\n".repeat(30);
        assert_eq!(content_rows(&v, 20), INPUT_MAX_ROWS);
    }

    #[test]
    fn frame_lines_empty_input_is_three_lines() {
        let b = InputBox::new();
        let (lines, cursor_line, _cursor_col) = b.frame_lines(20, 40);
        assert_eq!(lines.len(), 3);
        assert!((1..=1).contains(&cursor_line));
    }

    #[test]
    fn frame_lines_first_and_last_line_are_full_width_rule() {
        let b = InputBox::new();
        let (lines, _, _) = b.frame_lines(20, 40);
        let expected = "─".repeat(20);
        assert_eq!(strip_ansi(&lines[0]), expected);
        assert_eq!(strip_ansi(lines.last().unwrap()), expected);
    }

    #[test]
    fn frame_lines_no_side_border_characters_anywhere() {
        let mut b = InputBox::new();
        type_str(&mut b, "hello world this is a longer line of text");
        let (lines, _, _) = b.frame_lines(15, 40);
        for line in &lines {
            for ch in ['│', '╭', '╰', '╮', '╯'] {
                assert!(
                    !line.contains(ch),
                    "unexpected border char {ch:?} in {line:?}"
                );
            }
        }
    }

    #[test]
    fn frame_lines_wraps_at_width_minus_two() {
        let mut b = InputBox::new();
        // width 12 -> inner width 10.
        type_str(&mut b, &"a".repeat(10));
        let (lines, _, _) = b.frame_lines(12, 40);
        assert_eq!(lines.len(), 3); // rule + 1 content + rule, fits exactly.

        let mut b2 = InputBox::new();
        type_str(&mut b2, &"a".repeat(11));
        let (lines2, _, _) = b2.frame_lines(12, 40);
        assert_eq!(lines2.len(), 4); // rule + 2 content + rule, one over.
    }

    #[test]
    fn frame_lines_cap_pins_line_count_and_keeps_cursor_visible() {
        let mut b = InputBox::new();
        // 30 single-char rows via newlines, cursor ends up at the last row.
        for _ in 0..30 {
            type_str(&mut b, "x");
            b.insert_str("\n");
        }
        let (lines, cursor_line, _) = b.frame_lines(20, 100); // large screen -> cap is INPUT_MAX_ROWS.
        assert_eq!(lines.len(), INPUT_MAX_ROWS + 2);
        // Cursor stays visible: it's the last content line of the window.
        assert_eq!(cursor_line as usize, INPUT_MAX_ROWS);
    }

    #[test]
    fn frame_lines_cursor_line_within_content_range() {
        let mut b = InputBox::new();
        type_str(&mut b, "abc\ndef\nghi");
        let (lines, cursor_line, _) = b.frame_lines(20, 40);
        let content_rows = lines.len() as u16 - 2;
        assert!((1..=content_rows).contains(&cursor_line));
    }

    #[test]
    fn frame_lines_short_screen_does_not_exceed_half_screen_height() {
        let mut b = InputBox::new();
        let v = "x\n".repeat(30); // far more than half of any small screen.
        b.insert_str(&v);
        let (lines, _, _) = b.frame_lines(20, 4); // screen_h 4 -> half = 2.
        assert_eq!(lines.len(), 4); // 2 content lines + 2 rules.
    }

    #[test]
    fn frame_lines_degenerate_width_and_screen_do_not_panic() {
        let mut b = InputBox::new();
        type_str(&mut b, "abc");
        for width in [0u16, 1, 2] {
            for screen_h in [0u16, 1] {
                let (lines, cursor_line, cursor_col) = b.frame_lines(width, screen_h);
                assert!(!lines.is_empty());
                assert!(cursor_line >= 1);
                let _ = cursor_col; // just must not panic / overflow.
            }
        }
    }
}
