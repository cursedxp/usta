//! Canlı girdi kutusu: tui-input editör state'i + Vec tabanlı up/down
//! tarihçesi. Rustyline'ın TUI yolundaki karşılığı. Spec §6.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;
use tui_input::backend::crossterm::to_input_request;
use tui_input::Input;

/// Tuş işlemenin sonucu — döngü buna göre davranır.
#[derive(Debug)]
pub enum Action {
    None,
    /// Trim'lenmiş, boş olmayan satır gönderildi.
    Submit(String),
    /// Ctrl-C / Ctrl-D — kapanış akışı.
    Exit,
}

pub struct InputBox {
    input: Input,
    history: Vec<String>,
    /// None = taze satır; Some(i) = history[i] gösteriliyor.
    cursor: Option<usize>,
    /// Tarihçeye girmeden önceki taze metin — Down ile geri gelir.
    stash: String,
}

impl InputBox {
    pub fn new() -> Self {
        Self { input: Input::default(), history: Vec::new(), cursor: None, stash: String::new() }
    }

    // Editör kamu API'si — döngü satırı Action::Submit'ten alır, imleci editör
    // kendi çizer, bu yüzden şu an çağrılmıyorlar (value() testlerde kullanılır).
    pub fn value(&self) -> &str { self.input.value() }
    #[allow(dead_code)]
    pub fn visual_cursor(&self) -> usize { self.input.visual_cursor() }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('d'))
        {
            return Action::Exit;
        }
        match key.code {
            KeyCode::Enter => {
                let line = self.input.value().trim().to_string();
                if line.is_empty() { return Action::None; }
                self.history.push(line.clone());
                self.input.reset();
                self.cursor = None;
                Action::Submit(line)
            }
            KeyCode::Up => { self.recall_prev(); Action::None }
            KeyCode::Down => { self.recall_next(); Action::None }
            _ => {
                self.cursor = None;
                if let Some(req) = to_input_request(&Event::Key(key)) {
                    self.input.handle(req);
                }
                Action::None
            }
        }
    }

    /// Yapıştırılan metni imleç konumuna ekle (bracketed paste). Satır sonları
    /// KORUNUR — model liste/log yapısını görsün; kutu tek satır olduğundan
    /// render'da ⏎ olarak gösterilir. CRLF → LF normalize edilir.
    pub fn insert_str(&mut self, s: &str) {
        self.cursor = None;
        let cleaned = s.replace("\r\n", "\n").replace('\r', "\n");
        for ch in cleaned.chars() {
            self.input.handle(tui_input::InputRequest::InsertChar(ch));
        }
    }

    fn recall_prev(&mut self) {
        if self.history.is_empty() { return; }
        let next = match self.cursor {
            None => { self.stash = self.input.value().to_string(); self.history.len() - 1 }
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

    /// Kutuyu çiz: yuvarlak kenar + `> ` öneki + imleç. Uzun metin kutunun
    /// iç genişliğinde ALT SATIRA SARILIR (yatay kaydırma yok); iç satır
    /// sayısını aşarsa dikey pencere imleci takip eder.
    pub fn render(&self, f: &mut Frame, area: Rect) {
        let inner_w = area.width.saturating_sub(4) as usize; // kenarlar + "> " öneki
        let visible = area.height.saturating_sub(2).max(1) as usize; // iç satırlar
        let (rows, cur_row, cur_col) = wrap_visual(self.input.value(), inner_w, self.input.visual_cursor());
        // Dikey pencere: imleç görünür kalacak şekilde son `visible` satır.
        let start = (cur_row + 1).saturating_sub(visible);
        let lines: Vec<Line> = rows
            .iter()
            .enumerate()
            .skip(start)
            .take(visible)
            .map(|(i, r)| {
                let prefix = if i == 0 { "> " } else { "  " };
                Line::from(vec![
                    Span::styled(prefix, Style::default().fg(Color::Indexed(208))),
                    Span::raw(r.clone()),
                ])
            })
            .collect();
        let para = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        f.render_widget(para, area);
        let x = area.x + 3 + cur_col as u16;
        let y = area.y + 1 + (cur_row - start) as u16;
        f.set_cursor_position((x.min(area.x + area.width - 2), y.min(area.y + area.height - 2)));
    }
}

/// Değeri görsel satırlara sar — genişlik HÜCRE bazlı (unicode-width),
/// `\n` satırı böler ve satır sonunda ⏎ olarak görünür. İmlecin (karakter
/// indeksi) satır/sütun karşılığını da döndürür. Saf — render bundan çizer,
/// Submit edilen değer DEĞİŞMEZ.
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

    fn key(c: char) -> KeyEvent { KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE) }
    fn code(k: KeyCode) -> KeyEvent { KeyEvent::new(k, KeyModifiers::NONE) }

    fn type_str(b: &mut InputBox, s: &str) {
        for c in s.chars() { assert!(matches!(b.handle_key(key(c)), Action::None)); }
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
        // Yapıştırma Submit tetiklemez; Enter sonrası tek mesaj, yapı korunur.
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
        // 10 hücre genişlik, 25 karakter → 3 satır (10+10+5).
        let v = "a".repeat(25);
        let (rows, cur_row, cur_col) = wrap_visual(&v, 10, 25);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].chars().count(), 10);
        assert_eq!(rows[2].chars().count(), 5);
        // İmleç sondaysa son satırın sonunda.
        assert_eq!((cur_row, cur_col), (2, 5));
    }

    #[test]
    fn wrap_visual_breaks_on_newline_with_visible_marker() {
        let (rows, _, _) = wrap_visual("ab\ncd", 10, 0);
        assert_eq!(rows, vec!["ab⏎".to_string(), "cd".to_string()]);
    }

    #[test]
    fn wrap_visual_cursor_mid_text_lands_on_correct_row() {
        let v = "a".repeat(15); // genişlik 10 → satır0: 0..9, satır1: 10..14
        let (_, cur_row, cur_col) = wrap_visual(&v, 10, 12);
        assert_eq!((cur_row, cur_col), (1, 2));
    }

    #[test]
    fn wrap_visual_turkish_chars_count_one_cell() {
        let (rows, _, _) = wrap_visual("çğşöüiçğşöüi", 6, 0); // 12 karakter, 6 hücre → 2 satır
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
}
