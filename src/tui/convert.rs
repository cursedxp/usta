//! Convert termimad ANSI output to ratatui Text — insert_before bridge — and
//! the reverse: ratatui `Text` to plain ANSI-escaped lines for the
//! relative-render bottom region.

use ratatui::style::{Color, Modifier};
use ratatui::text::Text;
use unicode_width::UnicodeWidthChar;

/// ANSI string → ratatui Text. On conversion error, drop styling and print
/// plain text — content is never lost.
pub fn ansi_to_text(s: &str) -> Text<'static> {
    use ansi_to_tui::IntoText;
    s.into_text().unwrap_or_else(|_| Text::raw(s.to_string()))
}

/// ratatui `Text` → plain ANSI-escaped lines, clipped to `width` display
/// cells. Only the style facets actually produced anywhere in `src/tui/` are
/// round-tripped: `Color::Indexed` foreground and the `Modifier::DIM` and
/// `Modifier::BOLD` bits — this is not a general ANSI style engine.
#[allow(dead_code)] // Task 4 wires this into the relative renderer; consumed there.
pub(crate) fn text_to_ansi_lines(t: &Text, width: u16) -> Vec<String> {
    t.lines
        .iter()
        .map(|line| {
            let raw: String = line
                .spans
                .iter()
                .map(|span| {
                    let mut prefix = String::new();
                    if let Some(Color::Indexed(n)) = span.style.fg {
                        prefix.push_str(&format!("\x1b[38;5;{n}m"));
                    }
                    if span.style.add_modifier.contains(Modifier::BOLD) {
                        prefix.push_str("\x1b[1m");
                    }
                    if span.style.add_modifier.contains(Modifier::DIM) {
                        prefix.push_str("\x1b[2m");
                    }
                    if prefix.is_empty() {
                        span.content.to_string()
                    } else {
                        format!("{prefix}{}\x1b[0m", span.content)
                    }
                })
                .collect();
            clip_to_width(&raw, width).0
        })
        .collect()
}

/// Clip a (possibly ANSI-styled) line to `width` display cells, measured via
/// `unicode-width`. ANSI escape sequences contribute zero width and are
/// never split mid-sequence — a sequence is emitted whole or not at all. If
/// a style escape was still open when clipping stopped, a reset (`\x1b[0m`)
/// is appended. `width == 0` yields empty content, never panics.
///
/// Returns the clipped line together with the display width it occupies, so
/// callers that need that width do not have to scan the line a second time.
#[allow(dead_code)] // Task 4 wires this into the relative renderer; consumed there.
pub(crate) fn clip_to_width(line: &str, width: u16) -> (String, u16) {
    if width == 0 {
        return (String::new(), 0);
    }
    let width = width as usize;
    let bytes = line.as_bytes();
    let mut out = String::new();
    let mut used = 0usize;
    let mut style_open = false;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            let start = i;
            let mut j = i + 1;
            if j < bytes.len() && bytes[j] == b'[' {
                j += 1;
                while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
                    j += 1;
                }
                if j < bytes.len() {
                    j += 1; // include the final byte
                }
            }
            let seq = &line[start..j];
            out.push_str(seq);
            style_open = seq != "\x1b[0m";
            i = j;
            continue;
        }
        let ch = line[i..].chars().next().expect("i is a char boundary");
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w > width {
            break;
        }
        out.push(ch);
        used += w;
        i += ch.len_utf8();
    }
    if style_open {
        out.push_str("\x1b[0m");
    }
    (out, used.min(u16::MAX as usize) as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};

    #[test]
    fn plain_text_passes_through() {
        let t = ansi_to_text("merhaba\ndünya");
        assert_eq!(t.lines.len(), 2);
    }

    #[test]
    fn ansi_colors_produce_styled_spans() {
        let t = ansi_to_text("\x1b[38;5;208mturuncu\x1b[0m");
        let joined: String = t.lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(joined, "turuncu");
    }

    #[test]
    fn text_to_ansi_lines_mixes_plain_colored_and_dim_spans() {
        let line = Line::from(vec![
            Span::raw("plain "),
            Span::styled("colored", Style::default().fg(Color::Indexed(114))),
            Span::styled(" dim", Style::default().add_modifier(Modifier::DIM)),
        ]);
        let t = Text::from(vec![line]);
        let out = text_to_ansi_lines(&t, 100);
        assert_eq!(
            out,
            vec!["plain \x1b[38;5;114mcolored\x1b[0m\x1b[2m dim\x1b[0m".to_string()]
        );
    }

    #[test]
    fn text_to_ansi_lines_maps_bold_modifier() {
        let line = Line::from(vec![Span::styled(
            "bold",
            Style::default().add_modifier(Modifier::BOLD),
        )]);
        let t = Text::from(vec![line]);
        let out = text_to_ansi_lines(&t, 100);
        assert_eq!(out, vec!["\x1b[1mbold\x1b[0m".to_string()]);
    }

    #[test]
    fn text_to_ansi_lines_mixes_brand_color_and_bold_modifier() {
        let line = Line::from(vec![Span::styled(
            "brand bold",
            Style::default()
                .fg(Color::Indexed(114))
                .add_modifier(Modifier::BOLD),
        )]);
        let t = Text::from(vec![line]);
        let out = text_to_ansi_lines(&t, 100);
        assert_eq!(
            out,
            vec!["\x1b[38;5;114m\x1b[1mbrand bold\x1b[0m".to_string()]
        );
    }

    #[test]
    fn text_to_ansi_lines_clips_line_wider_than_width() {
        let t = Text::from(vec![Line::from("hello world")]);
        let out = text_to_ansi_lines(&t, 5);
        assert_eq!(out, vec!["hello".to_string()]);
    }

    #[test]
    fn text_to_ansi_lines_empty_text_is_empty_vec() {
        let t = Text::default();
        let out = text_to_ansi_lines(&t, 20);
        assert!(out.is_empty());
    }

    #[test]
    fn clip_to_width_never_splits_an_escape_sequence() {
        // "a" (1 cell, unstyled) + colored "bcdef" (5 cells). Width 3 must
        // stop after "bc", never slicing through the opening escape and
        // must close the still-open style with a reset.
        let raw = "a\x1b[38;5;114mbcdef";
        assert_eq!(
            clip_to_width(raw, 3),
            ("a\x1b[38;5;114mbc\x1b[0m".to_string(), 3)
        );
    }

    #[test]
    fn clip_to_width_zero_width_yields_empty_content() {
        assert_eq!(
            clip_to_width("\x1b[38;5;114mhello\x1b[0m", 0),
            (String::new(), 0)
        );
    }

    #[test]
    fn clip_to_width_unstyled_line_within_width_is_unchanged() {
        assert_eq!(clip_to_width("hi", 10), ("hi".to_string(), 2));
    }

    #[test]
    fn clip_to_width_drops_wide_char_that_would_straddle_the_limit() {
        // "a" (1 cell) leaves exactly 1 cell of budget out of width 2.
        // "世" is a 2-cell CJK character and must not be emitted half-width.
        assert_eq!(clip_to_width("a世", 2), ("a".to_string(), 1));
    }

    #[test]
    fn clip_to_width_reports_display_width_ignoring_escapes_and_counting_wide_chars() {
        // Escape sequences occupy zero cells; "世" occupies two.
        assert_eq!(
            clip_to_width("\x1b[38;5;114mab\x1b[0m", 10),
            ("\x1b[38;5;114mab\x1b[0m".to_string(), 2)
        );
        assert_eq!(clip_to_width("a世", 10), ("a世".to_string(), 3));
    }
}
