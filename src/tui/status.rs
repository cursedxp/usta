//! Bottom line of the viewport: spinner + context indicator (the TUI
//! counterpart of ui::context_gauge — lives in place instead of printing a separate line).

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

const FRAMES: [&str; 4] = ["⠋", "⠙", "⠸", "⠴"];

pub enum Status {
    Idle,
    Thinking { frame: usize, cancel_hint: bool },
}

/// Single-line status: spinner if thinking (+ cancel hint), gauge whenever tokens are present.
pub fn render_status(s: &Status, tokens: Option<u64>, window: u64, watching: Option<bool>) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::new();
    if let Some(on) = watching {
        let (txt, col) = if on { ("👁 watching ", Color::DarkGray) } else { ("watch off ", Color::DarkGray) };
        spans.push(Span::styled(txt.to_string(), Style::default().fg(col)));
    }
    if let Status::Thinking { frame, cancel_hint } = s {
        let hint = if *cancel_hint {
            " (press ctrl-c again to quit · esc to stop)"
        } else {
            " (esc to stop)"
        };
        spans.push(Span::styled(
            format!("{} Usta is thinking…{hint} ", FRAMES[frame % FRAMES.len()]),
            Style::default().fg(Color::DarkGray),
        ));
    }
    if let Some(t) = tokens {
        let ratio = (t as f64 / window as f64).min(1.0);
        let filled = ((ratio * 8.0).round() as usize).min(8);
        let color = if ratio >= 0.7 { Color::Yellow } else { Color::DarkGray };
        spans.push(Span::styled(
            format!("{}{} context {}k/{}k", "▓".repeat(filled), "░".repeat(8 - filled), t / 1000, window / 1000),
            Style::default().fg(color),
        ));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(l: &ratatui::text::Line) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn idle_without_tokens_is_empty() {
        assert_eq!(text(&render_status(&Status::Idle, None, 1_000_000, None)), "");
    }

    #[test]
    fn thinking_shows_spinner_frame() {
        let l = render_status(&Status::Thinking { frame: 0, cancel_hint: false }, None, 1_000_000, None);
        assert!(text(&l).contains("thinking"));
        assert!(text(&l).contains("esc to stop"));
    }

    #[test]
    fn thinking_with_cancel_hint_shows_hint() {
        let l = render_status(&Status::Thinking { frame: 0, cancel_hint: true }, None, 1_000_000, None);
        assert!(text(&l).contains("ctrl-c again"));
    }

    #[test]
    fn gauge_shows_ratio() {
        let l = render_status(&Status::Idle, Some(500_000), 1_000_000, None);
        assert!(text(&l).contains("context 500k/1000k"));
        assert!(text(&l).contains("▓▓▓▓░░░░"));
    }

    #[test]
    fn watch_indicator_shows_when_some() {
        assert!(text(&render_status(&Status::Idle, None, 1_000_000, Some(true))).contains("watching"));
        assert!(text(&render_status(&Status::Idle, None, 1_000_000, Some(false))).contains("watch off"));
        assert!(!text(&render_status(&Status::Idle, None, 1_000_000, None)).contains("watch"));
    }
}
