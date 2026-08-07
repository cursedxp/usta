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

    pub fn value(&self) -> &str { self.input.value() }
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

    /// Kutuyu çiz: yuvarlak kenar + `> ` öneki + imleç. Uzun satırda
    /// tui-input'un visual_scroll'u son kısmı gösterir (iç kaydırma).
    pub fn render(&self, f: &mut Frame, area: Rect) {
        let inner_w = area.width.saturating_sub(4) as usize; // kenarlar + "> "
        let scroll = self.input.visual_scroll(inner_w);
        let shown: String = self.input.value().chars().skip(scroll).collect();
        let para = Paragraph::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Indexed(208))),
            Span::raw(shown),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        f.render_widget(para, area);
        let x = area.x + 3 + (self.input.visual_cursor().saturating_sub(scroll)) as u16;
        f.set_cursor_position((x.min(area.x + area.width - 2), area.y + 1));
    }
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
    fn turkish_chars_edit_correctly() {
        let mut b = InputBox::new();
        type_str(&mut b, "çğşü");
        b.handle_key(code(KeyCode::Backspace));
        assert_eq!(b.value(), "çğş");
    }
}
