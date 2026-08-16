//! The single source of truth for the TUI's visual language (design system —
//! Claude Design project `f8cc2dc7`). Every semantic color is a `Color::Indexed`
//! so it degrades identically on 256-color terminals; truecolor terminals render
//! the same index correctly. Every status color is paired with a glyph so meaning
//! survives colorblindness and monochrome terminals — the glyph carries the
//! meaning, the color only reinforces it.
//!
//! ALL tui modules pull color+glyph from here — scattered `Color::` literals are
//! banned. Orange (BRAND) is IDENTITY, never a status: logo, `●` bullet, `❯`
//! prompt caret, panel titles. Status is carried by the four hues below.

// This module is the design system's single source of truth. Its palette + glyph
// API is intentionally COMPLETE (mandated by the design plan): the shell renders
// BRAND / WARN / ERROR / DIM directly, while SUCCESS / GAME and the ✓ / ▸ glyphs
// define the tokens that model-drawn surfaces (exam scorecard, game feed) emit —
// per GOAL.md / TEACHING.md format rules — plus the helper fns are the public
// styling API. So a few tokens have no direct shell call site by design.
#![allow(dead_code)]

use ratatui::style::{Color, Modifier, Style};

// ── Raw 256-color indices — the numeric source of truth. Ratatui consumers use
//    the `Color` constants below; the ANSI / termimad plain paths (ui.rs) reach
//    for the index directly so a single number defines each hue. ────────────────
pub const BRAND_IDX: u8 = 208;
pub const CODE_IDX: u8 = 114;
pub const SUCCESS_IDX: u8 = 149;
pub const WARN_IDX: u8 = 179;
pub const ERROR_IDX: u8 = 210;

// ── Semantic palette (256-color index — see design tokens page 01) ───────────
/// Identity & structure: logo, Usta bullet, prompt caret, panel titles. Orange.
pub const BRAND: Color = Color::Indexed(BRAND_IDX);
/// Positive result: exam pass, gap closed, file converted, review recalled. Green.
pub const SUCCESS: Color = Color::Indexed(SUCCESS_IDX);
/// Caution: context filling, missing tool, needs-confirm, truncation. Amber —
/// this REPLACES the old raw `Color::Yellow`, which read as a harsh warning.
pub const WARN: Color = Color::Indexed(WARN_IDX);
/// Failure / hard stop: backend missing, generation failed, dangerous action. Red.
pub const ERROR: Color = Color::Indexed(ERROR_IDX);
/// Gamification ONLY: XP, level-up, badge. Violet — a cool hue that never
/// masquerades as Usta's orange voice.
pub const GAME: Color = Color::Indexed(141);
/// The quiet default: system notices, hints, secondary meta, box borders.
/// (Design tokens 01: dim = 256:244.)
pub const DIM: Color = Color::Indexed(244);

// ── Glyph vocabulary (colorblind/monochrome-safe — design tokens page 02/03) ──
pub const G_INFO: &str = "·"; // system notice / hint (dim)
pub const G_OK: &str = "✓"; // success result (green)
pub const G_WARN: &str = "⚠"; // warning result (amber)
pub const G_ERR: &str = "✗"; // error / hard stop (red)
pub const G_GAME: &str = "▸"; // game event (violet, dosed)
pub const G_BRAND: &str = "●"; // Usta is speaking (orange bullet)
pub const G_PROMPT: &str = "❯"; // your input, echoed into scrollback (orange caret)

/// Thinking spinner frames — Braille, ~120 ms cadence. Kept as the existing
/// four-frame set (matches the shipped `ui::Spinner` and status line).
pub const SPINNER: [&str; 4] = ["⠋", "⠙", "⢸", "⢴"];

// ── Ready-made styles — helpers so no module hand-rolls `.fg(...)` ────────────
/// Info/ambient: dim, no explicit color (survives dark & light themes).
pub fn info() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}
pub fn success() -> Style {
    Style::default().fg(SUCCESS)
}
pub fn warn() -> Style {
    Style::default().fg(WARN)
}
pub fn error() -> Style {
    Style::default().fg(ERROR)
}
pub fn game() -> Style {
    Style::default().fg(GAME)
}
pub fn brand() -> Style {
    Style::default().fg(BRAND)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_semantics_locked() {
        // Amber warning — NOT the raw Color::Yellow it replaces.
        assert_eq!(WARN, Color::Indexed(179));
        assert_ne!(WARN, Color::Yellow);
        // Game violet and brand orange are the exact design-token indices.
        assert_eq!(GAME, Color::Indexed(141));
        assert_eq!(BRAND, Color::Indexed(208));
        assert_eq!(SUCCESS, Color::Indexed(149));
        assert_eq!(ERROR, Color::Indexed(210));
        // Every status glyph is non-empty AND its style actually sets a color
        // (not a vacuous default) — glyph+color pairing is real.
        for (g, s) in [(G_OK, success()), (G_WARN, warn()), (G_ERR, error()), (G_GAME, game()), (G_BRAND, brand())] {
            assert!(!g.is_empty());
            assert!(s.fg.is_some());
        }
    }

    #[test]
    fn info_is_dim_not_a_hardcoded_color() {
        // Ambient text must degrade with the terminal theme — dim modifier, no fg.
        assert!(info().add_modifier.contains(Modifier::DIM));
        assert!(info().fg.is_none());
    }

    #[test]
    fn glyph_set_is_complete_and_distinct() {
        let glyphs = [G_INFO, G_OK, G_WARN, G_ERR, G_GAME, G_BRAND, G_PROMPT];
        for g in glyphs {
            assert_eq!(g.chars().count(), 1, "glyph must be a single char: {g:?}");
        }
        // No two glyphs collide.
        for i in 0..glyphs.len() {
            for j in (i + 1)..glyphs.len() {
                assert_ne!(glyphs[i], glyphs[j], "glyphs must be distinct");
            }
        }
    }

    #[test]
    fn spinner_has_four_frames() {
        assert_eq!(SPINNER.len(), 4);
        assert!(SPINNER.iter().all(|f| !f.is_empty()));
        // Claude Design mockup 01 legend frames (Anil onaylı).
        assert_eq!(SPINNER, ["⠋", "⠙", "⢸", "⢴"]);
    }
}
