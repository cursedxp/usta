//! Live input box: ratatui-textarea editing engine + Vec-based up/down
//! history. The TUI-path counterpart of Rustyline. Spec §6.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use ratatui_textarea::{CursorMove, TextArea};

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
    ta: TextArea<'static>,
    history: Vec<String>,
    /// None = fresh text; Some(i) = history[i] is being shown.
    cursor: Option<usize>,
    /// Fresh text from before entering history — comes back with Down.
    stash: String,
}

/// TextArea holding `s`, cursor at the very end (recall / reset).
fn area_with(s: &str) -> TextArea<'static> {
    let mut ta = TextArea::new(s.split('\n').map(str::to_string).collect());
    ta.move_cursor(CursorMove::Bottom);
    ta.move_cursor(CursorMove::End);
    ta
}

impl InputBox {
    pub fn new() -> Self {
        Self {
            ta: TextArea::default(),
            history: Vec::new(),
            cursor: None,
            stash: String::new(),
        }
    }

    pub fn value(&self) -> String {
        self.ta.lines().join("\n")
    }
    pub fn lines(&self) -> &[String] {
        self.ta.lines()
    }
    /// (data row, char column) of the cursor.
    pub fn cursor_rc(&self) -> (usize, usize) {
        let c = self.ta.cursor();
        (c.0, c.1)
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        let m = key.modifiers;
        let ctrl = m.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('d')) {
            return Action::Exit;
        }
        // Newline: Shift/Ctrl/Alt+Enter (kitty protocol) or Ctrl+J (universal). Bare Enter submits.
        let newline = (key.code == KeyCode::Enter
            && m.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::CONTROL))
            || (ctrl && key.code == KeyCode::Char('j'));
        if newline {
            self.ta.insert_newline();
            return Action::None;
        }
        let (row, _) = self.cursor_rc();
        let last_row = self.ta.lines().len() - 1;
        let shift = m.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Enter => {
                let line = self.value().trim().to_string();
                if line.is_empty() {
                    return Action::None;
                }
                self.history.push(line.clone());
                self.clear();
                Action::Submit(line)
            }
            // opencode: Up/Down move the cursor inside multiline text; history only from the edge row.
            KeyCode::Up if shift || row > 0 => {
                self.move_to(CursorMove::Up, shift);
                Action::None
            }
            KeyCode::Down if shift || row < last_row => {
                self.move_to(CursorMove::Down, shift);
                Action::None
            }
            KeyCode::Up => {
                self.recall_prev();
                Action::None
            }
            KeyCode::Down => {
                self.recall_next();
                Action::None
            }
            // Edits keep `self.cursor`: `edited_recall()` compares the text instead.
            _ => {
                self.edit(key);
                Action::None
            }
        }
    }

    fn move_to(&mut self, mv: CursorMove, select: bool) {
        if select {
            if !self.ta.is_selecting() {
                self.ta.start_selection();
            }
        } else {
            self.ta.cancel_selection();
        }
        self.ta.move_cursor(mv);
    }

    /// opencode 2.1.4 default input keybinds (Ctrl+C/D handled by the caller).
    fn edit(&mut self, key: KeyEvent) {
        let m = key.modifiers;
        let ctrl = m.contains(KeyModifiers::CONTROL);
        let alt = m.contains(KeyModifiers::ALT);
        let shift = m.contains(KeyModifiers::SHIFT);
        let mv = match key.code {
            KeyCode::Left if ctrl || alt => Some(CursorMove::WordBack),
            KeyCode::Right if ctrl || alt => Some(CursorMove::WordForward),
            KeyCode::Char('b' | 'B') if alt => Some(CursorMove::WordBack),
            KeyCode::Char('f' | 'F') if alt => Some(CursorMove::WordForward),
            KeyCode::Left => Some(CursorMove::Back),
            KeyCode::Right => Some(CursorMove::Forward),
            KeyCode::Char('b') if ctrl => Some(CursorMove::Back),
            KeyCode::Char('f') if ctrl => Some(CursorMove::Forward),
            KeyCode::Char('a' | 'A') if ctrl => Some(CursorMove::Head),
            KeyCode::Char('e' | 'E') if ctrl => Some(CursorMove::End),
            KeyCode::Home if shift => Some(CursorMove::Top),
            KeyCode::End if shift => Some(CursorMove::Bottom),
            KeyCode::Home => Some(CursorMove::Head),
            KeyCode::End => Some(CursorMove::End),
            _ => None,
        };
        if let Some(mv) = mv {
            // Shift+Home/End = select to buffer start/end; Ctrl+Shift+A/E = select to line edge.
            let select = shift && !matches!(key.code, KeyCode::Char('B' | 'F'));
            self.move_to(mv, select);
            return;
        }
        let ta = &mut self.ta;
        match key.code {
            KeyCode::Backspace if ctrl || alt => {
                ta.delete_word();
            }
            KeyCode::Backspace => {
                ta.delete_char();
            }
            KeyCode::Delete if ctrl => {
                ta.delete_next_word();
            }
            KeyCode::Delete => {
                ta.delete_next_char();
            }
            KeyCode::Char('d') if alt => {
                ta.delete_next_word();
            }
            KeyCode::Char('w') if ctrl => {
                ta.delete_word();
            }
            KeyCode::Char('k') if ctrl => {
                ta.delete_line_by_end();
            }
            KeyCode::Char('u') if ctrl => {
                ta.delete_line_by_head();
            }
            KeyCode::Char('-') if ctrl => {
                ta.undo();
            }
            KeyCode::Char('.') if ctrl => {
                ta.redo();
            }
            KeyCode::Esc => ta.cancel_selection(),
            KeyCode::Char(c) if !ctrl && !alt => ta.insert_char(c),
            _ => {}
        }
    }

    /// Drop any half-typed text (confirm cancelled — don't leak it into the next prompt).
    pub fn clear(&mut self) {
        self.ta = TextArea::default();
        self.cursor = None;
    }

    /// Insert pasted text at the cursor (bracketed paste). Line breaks are PRESERVED as
    /// real lines; CRLF / CR → LF.
    pub fn insert_str(&mut self, s: &str) {
        let cleaned = s.replace("\r\n", "\n").replace('\r', "\n");
        self.ta.insert_str(cleaned);
    }

    /// True when the shown text was edited after a recall — opencode won't overwrite it.
    fn edited_recall(&self) -> bool {
        matches!(self.cursor, Some(i) if self.history[i] != self.value())
    }

    fn recall_prev(&mut self) {
        if self.history.is_empty() || self.edited_recall() {
            return;
        }
        let next = match self.cursor {
            None => {
                self.stash = self.value();
                self.history.len() - 1
            }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.cursor = Some(next);
        self.ta = area_with(&self.history[next]);
    }

    fn recall_next(&mut self) {
        if self.edited_recall() {
            return;
        }
        match self.cursor {
            None => {}
            Some(i) if i + 1 < self.history.len() => {
                self.cursor = Some(i + 1);
                self.ta = area_with(&self.history[i + 1]);
            }
            Some(_) => {
                self.cursor = None;
                self.ta = area_with(&std::mem::take(&mut self.stash));
            }
        }
    }

    /// Draw the box: a single rule above (no side or bottom borders) + `> ` prefix +
    /// cursor. Long text WRAPS TO THE NEXT LINE at the full width minus the
    /// prefix (no horizontal scrolling); if it exceeds the inner line count,
    /// the vertical window follows the cursor.
    pub fn render(&self, f: &mut Frame, area: Rect) {
        let inner_w = area.width.saturating_sub(2) as usize; // "> " prefix
        let visible = area.height.saturating_sub(1).max(1) as usize; // lines under the rule
        let value = self.value();
        let (r, c) = self.cursor_rc();
        let idx = self.lines()[..r]
            .iter()
            .map(|l| l.chars().count() + 1)
            .sum::<usize>()
            + c;
        let (rows, cur_row, cur_col) = wrap_visual(&value, inner_w, idx);
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
                .borders(Borders::TOP)
                .border_style(Style::default().fg(theme::DIM)),
        );
        f.render_widget(para, area);
        let x = area.x + 2 + cur_col as u16;
        let y = area.y + 1 + (cur_row - start) as u16;
        f.set_cursor_position((
            x.min(area.x + area.width - 1),
            y.min(area.y + area.height - 1),
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

    /// Render the box into a `TestBackend` and return (rows as strings, cursor x/y).
    fn render_rows(b: &InputBox, w: u16, h: u16) -> (Vec<String>, (u16, u16)) {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| {
            let a = f.area();
            b.render(f, a);
        })
        .unwrap();
        let buf = t.backend().buffer().clone();
        let rows = (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect())
            .collect();
        let pos = t.get_cursor_position().unwrap();
        (rows, (pos.x, pos.y))
    }

    #[test]
    fn render_draws_only_a_top_rule() {
        let b = InputBox::new();
        let (rows, _) = render_rows(&b, 20, 5);
        assert_eq!(rows[0], "─".repeat(20), "top rule");
        assert!(rows[1].starts_with("> "), "prompt row: {:?}", rows[1]);
        for (i, r) in rows.iter().enumerate().skip(1) {
            assert!(!r.contains('─'), "row {i} has a rule: {r:?}");
        }
        for (i, r) in rows.iter().enumerate() {
            for c in ['│', '╭', '╮', '╰', '╯'] {
                assert!(!r.contains(c), "row {i} has {c:?}: {r:?}");
            }
        }
    }

    #[test]
    fn render_wraps_at_width_minus_prefix() {
        let mut b = InputBox::new();
        type_str(&mut b, &"a".repeat(18)); // 20 - "> " = 18 fits one row
        let (rows, _) = render_rows(&b, 20, 5);
        assert_eq!(rows[1], format!("> {}", "a".repeat(18)));
        assert_eq!(rows[2].trim(), "", "nothing wrapped: {:?}", rows[2]);

        let mut b = InputBox::new();
        type_str(&mut b, &"a".repeat(19)); // one over → second row
        let (rows, _) = render_rows(&b, 20, 5);
        assert_eq!(rows[1], format!("> {}", "a".repeat(18)));
        assert_eq!(rows[2], format!("  a{}", " ".repeat(17)));
    }

    #[test]
    fn render_places_cursor_after_prompt_and_clamps_to_last_column() {
        let b = InputBox::new();
        let (_, cur) = render_rows(&b, 20, 5);
        assert_eq!(cur, (2, 1), "empty input: cursor right after \"> \"");

        let mut b = InputBox::new();
        type_str(&mut b, &"a".repeat(18)); // cursor col 18 → x 20, clamped to 19
        let (_, cur) = render_rows(&b, 20, 5);
        assert_eq!(cur, (19, 1));
    }

    #[test]
    fn render_uses_the_freed_row_for_a_fourth_content_line() {
        let mut b = InputBox::new();
        type_str(&mut b, &"a".repeat(55)); // 18 + 18 + 18 + 1 → four visual rows
        let (rows, cur) = render_rows(&b, 20, 5);
        assert_eq!(rows[1], format!("> {}", "a".repeat(18)));
        assert_eq!(rows[2], format!("  {}", "a".repeat(18)));
        assert_eq!(rows[3], format!("  {}", "a".repeat(18)));
        assert_eq!(rows[4], format!("  a{}", " ".repeat(17)));
        assert_eq!(cur, (3, 4), "cursor after the last char on row 4");
    }

    fn mods(k: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(k, m)
    }

    #[test]
    fn shift_enter_and_ctrl_j_insert_real_lines() {
        let mut b = InputBox::new();
        type_str(&mut b, "bir");
        b.handle_key(mods(KeyCode::Enter, KeyModifiers::SHIFT));
        type_str(&mut b, "iki");
        b.handle_key(mods(KeyCode::Char('j'), KeyModifiers::CONTROL));
        type_str(&mut b, "üç");
        assert_eq!(b.value(), "bir\niki\nüç");
        assert_eq!(b.lines().len(), 3);
        assert_eq!(b.cursor_rc(), (2, 2));
    }

    #[test]
    fn up_moves_cursor_between_lines_before_history() {
        let mut b = InputBox::new();
        type_str(&mut b, "eski");
        b.handle_key(code(KeyCode::Enter));
        type_str(&mut b, "a");
        b.handle_key(mods(KeyCode::Enter, KeyModifiers::SHIFT));
        type_str(&mut b, "b");
        b.handle_key(code(KeyCode::Up)); // row 1 -> row 0, no history
        assert_eq!(b.value(), "a\nb");
        assert_eq!(b.cursor_rc().0, 0);
        b.handle_key(code(KeyCode::Up)); // row 0 -> history
        assert_eq!(b.value(), "eski");
        b.handle_key(code(KeyCode::Down)); // single line = last row -> back to stash
        assert_eq!(b.value(), "a\nb");
    }

    #[test]
    fn edited_recall_is_not_overwritten_by_up() {
        let mut b = InputBox::new();
        type_str(&mut b, "bir");
        b.handle_key(code(KeyCode::Enter));
        type_str(&mut b, "iki");
        b.handle_key(code(KeyCode::Enter));
        b.handle_key(code(KeyCode::Up));
        assert_eq!(b.value(), "iki");
        type_str(&mut b, "X");
        b.handle_key(code(KeyCode::Up));
        assert_eq!(b.value(), "ikiX");
    }

    #[test]
    fn emacs_line_and_word_keys() {
        let ctrl = KeyModifiers::CONTROL;
        let alt = KeyModifiers::ALT;
        let mut b = InputBox::new();
        type_str(&mut b, "merhaba güzel dünya");
        b.handle_key(mods(KeyCode::Char('w'), ctrl));
        assert_eq!(b.value(), "merhaba güzel ");
        b.handle_key(mods(KeyCode::Char('a'), ctrl));
        assert_eq!(b.cursor_rc(), (0, 0));
        b.handle_key(mods(KeyCode::Char('f'), alt));
        b.handle_key(mods(KeyCode::Char('k'), ctrl));
        assert_eq!(b.value(), "merhaba ");
        b.handle_key(mods(KeyCode::Char('e'), ctrl));
        b.handle_key(mods(KeyCode::Char('u'), ctrl));
        assert_eq!(b.value(), "");
        b.handle_key(mods(KeyCode::Char('-'), ctrl)); // undo
        assert_eq!(b.value(), "merhaba ");
        b.handle_key(mods(KeyCode::Char('.'), ctrl)); // redo
        assert_eq!(b.value(), "");
    }

    #[test]
    fn paste_keeps_lines_and_normalizes_crlf() {
        let mut b = InputBox::new();
        b.insert_str("a\r\nb\rc");
        assert_eq!(b.value(), "a\nb\nc");
        assert_eq!(b.cursor_rc(), (2, 1));
    }
}
