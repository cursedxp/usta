//! Bottom line of the viewport: spinner + context indicator (the TUI
//! counterpart of ui::context_gauge — lives in place instead of printing a separate line).

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::tui::theme;

pub enum Status {
    Idle,
    Thinking { frame: usize, cancel_hint: bool },
}

/// Context gauge style: amber (WARN) once the window is ≥70% full, quiet dim
/// otherwise — the threshold is the design-tokens gauge rule. Pure, so the
/// boundary is unit-tested without rendering.
fn gauge_style(ratio: f64) -> Style {
    if ratio >= 0.7 {
        theme::warn()
    } else {
        theme::info()
    }
}

/// Single-line status: spinner if thinking (+ cancel hint), gauge whenever tokens are present.
pub fn render_status(s: &Status, tokens: Option<u64>, window: u64, watching: Option<bool>) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::new();
    if let Some(on) = watching {
        let txt = if on { "👁 watching " } else { "watch off " };
        spans.push(Span::styled(txt.to_string(), theme::info()));
    }
    if let Status::Thinking { frame, cancel_hint } = s {
        let hint = if *cancel_hint {
            " (press ctrl-c again to quit · esc to stop)"
        } else {
            " (esc to stop)"
        };
        spans.push(Span::styled(
            format!("{} Usta is thinking…{hint} ", theme::SPINNER[frame % theme::SPINNER.len()]),
            theme::info(),
        ));
    }
    if let Some(t) = tokens {
        let ratio = (t as f64 / window as f64).min(1.0);
        let filled = ((ratio * 8.0).round() as usize).min(8);
        spans.push(Span::styled(
            format!("{}{} context {}k/{}k", "▓".repeat(filled), "░".repeat(8 - filled), t / 1000, window / 1000),
            gauge_style(ratio),
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
    fn gauge_style_flips_to_amber_at_seventy_percent() {
        // Below the threshold the gauge is quiet dim (no hard color).
        assert_eq!(gauge_style(0.69).fg, theme::info().fg);
        assert!(gauge_style(0.69).add_modifier.contains(ratatui::style::Modifier::DIM));
        // At/above 70% it turns amber — the exact WARN index, never Color::Yellow.
        assert_eq!(gauge_style(0.70).fg, Some(theme::WARN));
        assert_ne!(gauge_style(0.70).fg, Some(ratatui::style::Color::Yellow));
        assert_eq!(gauge_style(1.0).fg, Some(theme::WARN));
    }

    #[test]
    fn spinner_frame_comes_from_theme_set() {
        // The thinking spinner draws from the single-source SPINNER set.
        for (i, f) in theme::SPINNER.iter().enumerate() {
            let l = render_status(&Status::Thinking { frame: i, cancel_hint: false }, None, 1, None);
            assert!(text(&l).starts_with(*f), "frame {i} should lead with {f}");
        }
    }

    #[test]
    fn watch_indicator_shows_when_some() {
        assert!(text(&render_status(&Status::Idle, None, 1_000_000, Some(true))).contains("watching"));
        assert!(text(&render_status(&Status::Idle, None, 1_000_000, Some(false))).contains("watch off"));
        assert!(!text(&render_status(&Status::Idle, None, 1_000_000, None)).contains("watch"));
    }
}
