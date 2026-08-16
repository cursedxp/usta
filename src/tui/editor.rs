//! Live input box: tui-input editor state + Vec-based up/down
//! history. The TUI-path counterpart of Rustyline. Spec §6.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;
use tui_input::backend::crossterm::to_input_request;
use tui_input::Input;

use crate::tui::theme;

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

    /// Draw the box: rounded border + `> ` prefix + cursor. Long text WRAPS TO
    /// THE NEXT LINE at the box's inner width (no horizontal scrolling); if it
    /// exceeds the inner line count, the vertical window follows the cursor.
    pub fn render(&self, f: &mut Frame, area: Rect) {
        let inner_w = area.width.saturating_sub(4) as usize; // borders + "> " prefix
        let visible = area.height.saturating_sub(2).max(1) as usize; // inner lines
        let (rows, cur_row, cur_col) =
            wrap_visual(self.input.value(), inner_w, self.input.visual_cursor());
        // Vertical window: last `visible` lines so the cursor stays visible.
        let start = (cur_row + 1).saturating_sub(visible);
        let lines: Vec<Line> = rows
            .iter()
            .enumerate()
            .skip(start)
            .take(visible)
            .map(|(i, r)| {
                let prefix = if i == 0 { "> " } else { "  " };
                Line::from(vec![
                    Span::styled(prefix, theme::brand()),
                    Span::raw(r.clone()),
                ])
            })
            .collect();
        let para = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme::DIM)),
        );
        f.render_widget(para, area);
        let x = area.x + 3 + cur_col as u16;
        let y = area.y + 1 + (cur_row - start) as u16;
        f.set_cursor_position((
            x.min(area.x + area.width - 2),
            y.min(area.y + area.height - 2),
        ));
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
}
