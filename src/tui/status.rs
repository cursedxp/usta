//! Viewport'un alt satırı: spinner + bağlam göstergesi (ui::context_gauge'un
//! TUI karşılığı — ayrı satır basmak yerine yerinde yaşar).

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

const FRAMES: [&str; 4] = ["⠋", "⠙", "⠸", "⠴"];

pub enum Status {
    Idle,
    Thinking { frame: usize },
}

/// Tek satır durum: düşünüyorsa spinner, her durumda token varsa gauge.
pub fn render_status(s: &Status, tokens: Option<u64>, window: u64) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::new();
    if let Status::Thinking { frame } = s {
        spans.push(Span::styled(
            format!("{} Usta düşünüyor… ", FRAMES[frame % FRAMES.len()]),
            Style::default().fg(Color::DarkGray),
        ));
    }
    if let Some(t) = tokens {
        let ratio = (t as f64 / window as f64).min(1.0);
        let filled = ((ratio * 8.0).round() as usize).min(8);
        let color = if ratio >= 0.7 { Color::Yellow } else { Color::DarkGray };
        spans.push(Span::styled(
            format!("{}{} bağlam {}k/{}k", "▓".repeat(filled), "░".repeat(8 - filled), t / 1000, window / 1000),
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
        assert_eq!(text(&render_status(&Status::Idle, None, 1_000_000)), "");
    }

    #[test]
    fn thinking_shows_spinner_frame() {
        let l = render_status(&Status::Thinking { frame: 0 }, None, 1_000_000);
        assert!(text(&l).contains("düşünüyor"));
    }

    #[test]
    fn gauge_shows_ratio() {
        let l = render_status(&Status::Idle, Some(500_000), 1_000_000);
        assert!(text(&l).contains("bağlam 500k/1000k"));
        assert!(text(&l).contains("▓▓▓▓░░░░"));
    }
}
