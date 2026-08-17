//! Convert termimad ANSI output to ratatui Text — insert_before bridge.

use ratatui::text::Text;

/// ANSI string → ratatui Text. On conversion error, drop styling and print
/// plain text — content is never lost.
pub fn ansi_to_text(s: &str) -> Text<'static> {
    use ansi_to_tui::IntoText;
    s.into_text().unwrap_or_else(|_| Text::raw(s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
