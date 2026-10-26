//! Live input box: ratatui-textarea editing engine + Vec-based up/down
//! history. The TUI-path counterpart of Rustyline. Spec §6.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use ratatui_textarea::{CursorMove, TextArea};

use crate::image_attach::PastedImage;
use crate::image_paste::{self, Clip};
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
    /// Images attached to the text being composed, with their `[Image #N]` number.
    images: Vec<(usize, PastedImage)>,
    /// Next token number (1-based, restarts after every submit / clear).
    next_image: usize,
    /// Images of the last submitted line - drained by `take_images`.
    submitted: Vec<PastedImage>,
}

/// TextArea holding `s`, cursor at the very end (recall / reset).
fn area_with(s: &str) -> TextArea<'static> {
    let mut ta = TextArea::new(s.split('\n').map(str::to_string).collect());
    ta.move_cursor(CursorMove::Bottom);
    ta.move_cursor(CursorMove::End);
    ta
}

fn image_token(n: usize) -> String {
    format!("[Image #{n}]")
}

impl InputBox {
    pub fn new() -> Self {
        Self {
            ta: TextArea::default(),
            history: Vec::new(),
            cursor: None,
            stash: String::new(),
            images: Vec::new(),
            next_image: 1,
            submitted: Vec::new(),
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
        if ctrl && key.code == KeyCode::Char('v') {
            self.paste_clipboard();
            return Action::None;
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
                self.submitted = self.referenced_images(&line);
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
            // Selecting moves (opencode): Shift+Left/Right, Shift+Home/End (buffer edges),
            // Ctrl+Shift+A/E (line edges), Alt+Shift+Left/Right and Alt+Shift+B/F (words).
            let select = shift
                && match key.code {
                    KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End => true,
                    KeyCode::Char('a' | 'A' | 'e' | 'E') => ctrl,
                    KeyCode::Char('b' | 'B' | 'f' | 'F') => alt,
                    _ => false,
                };
            self.move_to(mv, select);
            // Top/Bottom keep the column; Shift+Home/End go to the very start/end.
            match (key.code, mv) {
                (KeyCode::Home, CursorMove::Top) => self.ta.move_cursor(CursorMove::Head),
                (KeyCode::End, CursorMove::Bottom) => self.ta.move_cursor(CursorMove::End),
                _ => {}
            }
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
            KeyCode::Char('-' | '_' | '7' | 'z') if ctrl => {
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
        self.images.clear();
        self.next_image = 1;
    }

    /// Insert pasted text at the cursor (bracketed paste). Line breaks are PRESERVED as
    /// real lines; CRLF / CR → LF.
    pub fn insert_str(&mut self, s: &str) {
        let cleaned = s
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\t', "    ");
        self.ta.insert_str(cleaned);
    }

    /// Insert an `[Image #N]` token at the cursor and keep the image for submit.
    pub fn attach_image(&mut self, img: PastedImage) {
        let n = self.next_image;
        self.next_image += 1;
        self.ta.insert_str(image_token(n));
        self.images.push((n, img));
    }

    /// Bracketed paste: a dragged-in image file (the terminal pastes its path) becomes a
    /// token; anything else is ordinary text.
    pub fn paste(&mut self, s: &str) {
        match image_paste::load_dropped(s) {
            Some(img) => self.attach_image(img),
            None => self.insert_str(s),
        }
    }

    /// Ctrl+V: clipboard image => token; clipboard text => inserted; nothing => no-op.
    pub fn paste_clipboard(&mut self) {
        self.apply_clip(image_paste::read_clipboard());
    }

    fn apply_clip(&mut self, clip: Clip) {
        match clip {
            Clip::Image(img) => self.attach_image(img),
            Clip::Text(t) => self.insert_str(&t),
            Clip::Empty => {}
        }
    }

    /// Pending images whose `[Image #N]` token survives in `line`, in attach order.
    fn referenced_images(&mut self, line: &str) -> Vec<PastedImage> {
        std::mem::take(&mut self.images)
            .into_iter()
            .filter(|(n, _)| line.contains(&image_token(*n)))
            .map(|(_, img)| img)
            .collect()
    }

    /// Images of the last submitted line (drained). Call right after `Action::Submit`.
    pub fn take_images(&mut self) -> Vec<PastedImage> {
        std::mem::take(&mut self.submitted)
    }

    /// True when the shown text was edited after a recall — opencode won't overwrite it.
    fn edited_recall(&self) -> bool {
        matches!(self.cursor, Some(i) if !self.value().is_empty() && self.history[i] != self.value())
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

    /// Text rows this box wants at `width` (prefix deducted), clamped to 1..=MAX_INPUT_ROWS.
    pub fn desired_rows(&self, width: u16) -> u16 {
        let inner = width.saturating_sub(2) as usize;
        let (rows, _, _) = wrap_rows(self.lines(), inner, self.cursor_rc());
        rows.len().min(MAX_INPUT_ROWS as usize).max(1) as u16
    }

    /// Draw the box: a single rule above (no side or bottom borders) + `> ` prefix +
    /// cursor. Rows break at word boundaries (or mid-word when a word is wider
    /// than the row) at the full width minus the prefix; logical lines are real
    /// lines, no ⏎ marker. If rows exceed the inner line count, the vertical
    /// window follows the cursor.
    pub fn render(&self, f: &mut Frame, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let inner_w = area.width.saturating_sub(2) as usize; // "> " prefix
        let visible = area.height.saturating_sub(1).max(1) as usize; // lines under the rule
        let (rows, cur_row, cur_col) = wrap_rows_src(self.lines(), inner_w, self.cursor_rc());
        let sel = self.ta.selection_range();
        // Vertical window: last `visible` lines so the cursor stays visible.
        let start = (cur_row + 1).saturating_sub(visible);
        let lines: Vec<Line> = rows
            .iter()
            .enumerate()
            .skip(start)
            .take(visible)
            .map(|(i, (text, drow, scol))| {
                let prefix = if i == 0 { "> " } else { "  " };
                let mut spans = vec![Span::styled(prefix, theme::brand())];
                spans.extend(row_spans(text, *drow, *scol, sel));
                Line::from(spans)
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
            x.min((area.x + area.width).saturating_sub(1)),
            y.min((area.y + area.height).saturating_sub(1)),
        ));
    }
}

/// Split one visual row into plain / REVERSED (selected) spans. `sel` is
/// `selection_range()` in data coords; `scol` is the row's first char column.
fn row_spans(
    text: &str,
    drow: usize,
    scol: usize,
    sel: Option<((usize, usize), (usize, usize))>,
) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let Some((a, b)) = sel else {
        return vec![Span::raw(text.to_string())];
    };
    if drow < a.0 || drow > b.0 {
        return vec![Span::raw(text.to_string())];
    }
    let from = if drow == a.0 {
        a.1.saturating_sub(scol).min(n)
    } else {
        0
    };
    let to = if drow == b.0 {
        b.1.saturating_sub(scol).min(n)
    } else {
        n
    };
    if from >= to {
        return vec![Span::raw(text.to_string())];
    }
    let seg = |r: std::ops::Range<usize>| chars[r].iter().collect::<String>();
    vec![
        Span::raw(seg(0..from)),
        Span::styled(
            seg(from..to),
            Style::default().add_modifier(Modifier::REVERSED),
        ),
        Span::raw(seg(to..n)),
    ]
}

/// Most text rows the box shows before it scrolls (opencode-style growth, 1..=8).
pub const MAX_INPUT_ROWS: u16 = 8;

fn cell(ch: char) -> usize {
    use unicode_width::UnicodeWidthChar;
    ch.width().unwrap_or(0).max(1)
}

/// Visual-row start indices for one logical line: break at the last space that fits,
/// or mid-word when a single word is wider than the row. Width is cell-based.
fn row_starts(chars: &[char], width: usize) -> Vec<usize> {
    let mut starts = vec![0];
    let (mut w, mut last_space) = (0usize, None::<usize>);
    let mut i = 0;
    while i < chars.len() {
        let start = *starts.last().expect("starts boş olamaz");
        if w + cell(chars[i]) > width && i > start {
            let s = match last_space {
                Some(sp) if sp + 1 > start && sp < i => sp + 1,
                _ => i,
            };
            starts.push(s);
            w = chars[s..i].iter().map(|&c| cell(c)).sum();
            last_space = None;
            continue;
        }
        if chars[i] == ' ' {
            last_space = Some(i);
        }
        w += cell(chars[i]);
        i += 1;
    }
    starts
}

/// Wrap logical lines into visual rows; also map the data cursor (row, char col)
/// to (visual row, cell column). Pure — render draws from this, the value is unchanged.
fn wrap_rows(
    lines: &[String],
    width: usize,
    cursor: (usize, usize),
) -> (Vec<String>, usize, usize) {
    let (rows, cur_row, cur_col) = wrap_rows_src(lines, width, cursor);
    (rows.into_iter().map(|r| r.0).collect(), cur_row, cur_col)
}

/// `wrap_rows` plus, per visual row, its source `(data row, start char col)`.
fn wrap_rows_src(
    lines: &[String],
    width: usize,
    cursor: (usize, usize),
) -> (Vec<(String, usize, usize)>, usize, usize) {
    let width = width.max(1);
    let mut rows = Vec::new();
    let (mut cur_row, mut cur_col) = (0, 0);
    for (r, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let starts = row_starts(&chars, width);
        for (k, &s) in starts.iter().enumerate() {
            let e = starts.get(k + 1).copied().unwrap_or(chars.len());
            if r == cursor.0 && cursor.1 >= s && (cursor.1 < e || k + 1 == starts.len()) {
                cur_row = rows.len();
                cur_col = chars[s..cursor.1].iter().map(|&c| cell(c)).sum();
            }
            rows.push((chars[s..e].iter().collect(), r, s));
        }
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
    fn wrap_breaks_at_word_boundary() {
        let lines = vec!["aaa bbb ccc".to_string()];
        let (rows, r, c) = wrap_rows(&lines, 8, (0, 11));
        assert_eq!(rows, vec!["aaa bbb ".to_string(), "ccc".to_string()]);
        assert_eq!((r, c), (1, 3));
    }

    #[test]
    fn wrap_splits_a_word_longer_than_the_row() {
        let lines = vec!["abcdefghij".to_string()];
        let (rows, _, _) = wrap_rows(&lines, 4, (0, 0));
        assert_eq!(rows, vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn wrap_counts_wide_chars_by_cell() {
        let lines = vec!["日本語".to_string()]; // 6 cells
        let (rows, r, c) = wrap_rows(&lines, 4, (0, 3));
        assert_eq!(rows, vec!["日本", "語"]);
        assert_eq!((r, c), (1, 2));
    }

    #[test]
    fn wrap_keeps_logical_lines_and_turkish_cursor() {
        let lines = vec!["şeker".to_string(), "çay".to_string()];
        let (rows, r, c) = wrap_rows(&lines, 20, (1, 2));
        assert_eq!(rows, vec!["şeker", "çay"]);
        assert_eq!((r, c), (1, 2));
    }

    #[test]
    fn desired_rows_grows_and_caps_at_eight() {
        let mut b = InputBox::new();
        assert_eq!(b.desired_rows(40), 1);
        b.insert_str("1\n2\n3");
        assert_eq!(b.desired_rows(40), 3);
        b.insert_str(&"\nx".repeat(20));
        assert_eq!(b.desired_rows(40), MAX_INPUT_ROWS);
    }

    #[test]
    fn render_rule_prefix_and_cursor() {
        let mut b = InputBox::new();
        b.insert_str("ab\ncd");
        let (rows, cur) = render_rows(&b, 20, 4);
        assert!(rows[0].chars().all(|c| c == '─'));
        assert!(rows[1].starts_with("> ab"));
        assert!(rows[2].starts_with("  cd"));
        assert_eq!(cur, (4, 2));
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

    fn reversed_text(b: &InputBox, w: u16, h: u16) -> String {
        use ratatui::backend::TestBackend;
        use ratatui::style::Modifier;
        use ratatui::Terminal;
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| {
            let a = f.area();
            b.render(f, a);
        })
        .unwrap();
        let buf = t.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                if buf[(x, y)].modifier.contains(Modifier::REVERSED) {
                    out.push_str(buf[(x, y)].symbol());
                }
            }
        }
        out
    }

    #[test]
    fn selection_is_highlighted_and_typing_replaces_it() {
        let shift = KeyModifiers::SHIFT;
        let mut b = InputBox::new();
        type_str(&mut b, "merhaba");
        b.handle_key(mods(KeyCode::Left, shift));
        b.handle_key(mods(KeyCode::Left, shift));
        assert_eq!(reversed_text(&b, 20, 3), "ba");
        type_str(&mut b, "X");
        assert_eq!(b.value(), "merhaX");
        assert_eq!(reversed_text(&b, 20, 3), "");
    }

    #[test]
    fn selection_highlight_spans_wrapped_rows_and_lines() {
        let shift = KeyModifiers::SHIFT;
        let mut b = InputBox::new();
        b.insert_str("ab\ncd");
        b.handle_key(mods(KeyCode::Home, shift)); // select to buffer start
        assert_eq!(reversed_text(&b, 20, 4), "abcd");
    }

    #[test]
    fn shift_home_end_select() {
        let shift = KeyModifiers::SHIFT;
        let mut b = InputBox::new();
        b.insert_str("ab\ncd");
        b.handle_key(mods(KeyCode::Home, shift));
        type_str(&mut b, "Z");
        assert_eq!(b.value(), "Z");
        let mut b = InputBox::new();
        b.insert_str("ab\ncd");
        b.handle_key(mods(KeyCode::Up, KeyModifiers::NONE));
        b.handle_key(mods(KeyCode::Home, KeyModifiers::NONE));
        b.handle_key(mods(KeyCode::End, shift)); // select to buffer end
        type_str(&mut b, "Z");
        assert_eq!(b.value(), "Z");
    }

    #[test]
    fn edited_recall_does_not_block_when_text_is_empty() {
        let mut b = InputBox::new();
        type_str(&mut b, "bir");
        b.handle_key(code(KeyCode::Enter));
        b.handle_key(code(KeyCode::Up));
        assert_eq!(b.value(), "bir");
        b.handle_key(mods(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(b.value(), "");
        b.handle_key(code(KeyCode::Up));
        assert_eq!(b.value(), "bir");
    }

    #[test]
    fn render_into_zero_sized_area_does_not_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::layout::Rect;
        use ratatui::Terminal;
        let b = InputBox::new();
        let mut t = Terminal::new(TestBackend::new(10, 3)).unwrap();
        t.draw(|f| {
            b.render(f, Rect::new(0, 0, 10, 0));
            b.render(f, Rect::new(0, 0, 0, 3));
        })
        .unwrap();
    }

    #[test]
    fn paste_converts_tabs_to_spaces() {
        let mut b = InputBox::new();
        b.insert_str("a\tb");
        assert_eq!(b.value(), "a    b");
    }

    #[test]
    fn undo_works_on_non_kitty_aliases() {
        for c in ['z', '_', '7'] {
            let mut b = InputBox::new();
            type_str(&mut b, "abc");
            b.handle_key(mods(KeyCode::Char('u'), KeyModifiers::CONTROL));
            assert_eq!(b.value(), "");
            b.handle_key(mods(KeyCode::Char(c), KeyModifiers::CONTROL));
            assert_eq!(b.value(), "abc", "Ctrl+{c}");
        }
    }

    #[test]
    fn paste_keeps_lines_and_normalizes_crlf() {
        let mut b = InputBox::new();
        b.insert_str("a\r\nb\rc");
        assert_eq!(b.value(), "a\nb\nc");
        assert_eq!(b.cursor_rc(), (2, 1));
    }

    use crate::image_attach::PastedImage;
    use crate::image_paste::Clip;

    fn pasted(tag: u8) -> PastedImage {
        PastedImage {
            media_type: "image/png",
            data: vec![tag],
            width: 1,
            height: 1,
        }
    }

    fn backspaces(b: &mut InputBox, n: usize) {
        for _ in 0..n {
            b.handle_key(code(KeyCode::Backspace));
        }
    }

    #[test]
    fn attach_image_inserts_numbered_tokens() {
        let mut b = InputBox::new();
        b.attach_image(pasted(1));
        b.attach_image(pasted(2));
        assert_eq!(b.value(), "[Image #1][Image #2]");
    }

    #[test]
    fn submit_keeps_only_images_whose_token_is_still_in_the_text() {
        let mut b = InputBox::new();
        b.attach_image(pasted(1));
        b.attach_image(pasted(2));
        backspaces(&mut b, "[Image #2]".chars().count()); // delete token 2
        type_str(&mut b, " what is this");
        let Action::Submit(line) = b.handle_key(code(KeyCode::Enter)) else {
            panic!("expected Submit")
        };
        assert_eq!(line, "[Image #1] what is this");
        assert_eq!(b.take_images(), vec![pasted(1)]);
    }

    #[test]
    fn submit_with_every_token_deleted_has_no_images() {
        let mut b = InputBox::new();
        b.attach_image(pasted(1));
        backspaces(&mut b, "[Image #1]".chars().count());
        type_str(&mut b, "hi");
        assert!(matches!(
            b.handle_key(code(KeyCode::Enter)),
            Action::Submit(_)
        ));
        assert!(b.take_images().is_empty());
    }

    #[test]
    fn token_ten_does_not_keep_image_one_alive() {
        let mut b = InputBox::new();
        for i in 1..=10 {
            b.attach_image(pasted(i));
        }
        // delete tokens 1..=9 from the front: Home, then Delete the first 9 tokens.
        b.handle_key(code(KeyCode::Home));
        for _ in 0.."[Image #1]".len() * 9 {
            b.handle_key(code(KeyCode::Delete));
        }
        assert_eq!(b.value(), "[Image #10]");
        assert!(matches!(
            b.handle_key(code(KeyCode::Enter)),
            Action::Submit(_)
        ));
        assert_eq!(b.take_images(), vec![pasted(10)]);
    }

    #[test]
    fn take_images_drains() {
        let mut b = InputBox::new();
        b.attach_image(pasted(1));
        b.handle_key(code(KeyCode::Enter));
        assert_eq!(b.take_images().len(), 1);
        assert!(b.take_images().is_empty());
    }

    #[test]
    fn clear_resets_images_and_numbering() {
        let mut b = InputBox::new();
        b.attach_image(pasted(1));
        b.clear();
        b.attach_image(pasted(2));
        assert_eq!(b.value(), "[Image #1]");
    }

    #[test]
    fn paste_of_an_image_path_becomes_a_token() {
        let dir = std::env::temp_dir().join(format!("usta editor paste {}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("shot one.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255]))
            .save(&p)
            .unwrap();
        let mut b = InputBox::new();
        b.paste(&p.to_str().unwrap().replace(' ', "\\ "));
        assert_eq!(b.value(), "[Image #1]");
        assert!(matches!(
            b.handle_key(code(KeyCode::Enter)),
            Action::Submit(_)
        ));
        let imgs = b.take_images();
        assert_eq!((imgs[0].width, imgs[0].height), (4, 4));
    }

    #[test]
    fn paste_of_ordinary_text_is_inserted_verbatim() {
        let mut b = InputBox::new();
        b.paste("hello\nworld");
        assert_eq!(b.value(), "hello\nworld");
        b.clear();
        b.paste("/no/such/file.png");
        assert_eq!(b.value(), "/no/such/file.png");
    }

    #[test]
    fn clipboard_image_text_and_empty_are_applied() {
        let mut b = InputBox::new();
        b.apply_clip(Clip::Image(pasted(5)));
        b.apply_clip(Clip::Text(" and text".into()));
        b.apply_clip(Clip::Empty);
        assert_eq!(b.value(), "[Image #1] and text");
    }
}
