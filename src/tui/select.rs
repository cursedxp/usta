//! In-app chat selection (spec B): pure helpers for mapping mouse cells to flat chat
//! positions, extracting the selected text, painting the highlight, plus the clipboard.

use std::sync::{Mutex, OnceLock};

use base64::Engine;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::Line;
use unicode_width::UnicodeWidthChar;

/// A drag selection in flat chat-line coordinates: (flat line index, display column).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sel {
    pub anchor: (usize, u16),
    pub head: (usize, u16),
}

/// Mouse cell -> flat position; `None` outside `area`. `top` = flat index of the pane's first row.
pub fn cell_to_pos(area: Rect, top: usize, x: u16, y: u16) -> Option<(usize, u16)> {
    let inside = x >= area.x && x < area.x + area.width && y >= area.y && y < area.y + area.height;
    inside.then(|| (top + (y - area.y) as usize, x - area.x))
}

/// Like `cell_to_pos`, but a cell outside `area` is clamped onto its nearest edge row/column.
pub fn clamp_pos(area: Rect, top: usize, x: u16, y: u16) -> (usize, u16) {
    let cx = x.clamp(area.x, area.x + area.width.saturating_sub(1));
    let cy = y.clamp(area.y, area.y + area.height.saturating_sub(1));
    (top + (cy - area.y) as usize, cx - area.x)
}

/// (start, end) in reading order, both inclusive.
pub fn ordered(sel: &Sel) -> ((usize, u16), (usize, u16)) {
    if sel.anchor <= sel.head {
        (sel.anchor, sel.head)
    } else {
        (sel.head, sel.anchor)
    }
}

/// Columns of row `row` covered by the selection, inclusive (`u16::MAX` = to the end).
fn row_range(row: usize, start: (usize, u16), end: (usize, u16)) -> (u16, u16) {
    let from = if row == start.0 { start.1 } else { 0 };
    let to = if row == end.0 { end.1 } else { u16::MAX };
    (from, to)
}

/// Selected text. A char is included when its starting display column is inside the
/// row's selected range (wide chars never split). Hard rows join with '\n', soft
/// (continuation) rows with "" because the wrap keeps the break space on the upper row.
/// Trailing spaces are trimmed only at the end of a logical line (before a hard break
/// and at the very end), so a soft break's space survives.
pub fn extract(lines: &[Line<'_>], soft: &[bool], sel: &Sel) -> String {
    let (start, end) = ordered(sel);
    let last = end.0.min(lines.len().saturating_sub(1));
    let mut out = String::new();
    for row in start.0..=last {
        if row >= lines.len() {
            break;
        }
        if row > start.0 && !soft.get(row).copied().unwrap_or(false) {
            trim_end_spaces(&mut out);
            out.push('\n');
        }
        let (from, to) = row_range(row, start, end);
        let mut col: u16 = 0;
        for span in &lines[row].spans {
            for ch in span.content.chars() {
                if col >= from && col <= to {
                    out.push(ch);
                }
                col = col.saturating_add(ch.width().unwrap_or(0) as u16);
            }
        }
    }
    trim_end_spaces(&mut out);
    out
}

fn trim_end_spaces(s: &mut String) {
    let keep = s.trim_end_matches(' ').len();
    s.truncate(keep);
}

/// Add `Modifier::REVERSED` to the selected cells that are visible in `area`.
pub fn highlight(buf: &mut Buffer, area: Rect, top: usize, sel: &Sel) {
    let (start, end) = ordered(sel);
    for dy in 0..area.height {
        let row = top + dy as usize;
        if row < start.0 || row > end.0 {
            continue;
        }
        let (from, to) = row_range(row, start, end);
        let to = to.min(area.width.saturating_sub(1));
        for dx in from..=to {
            if dx >= area.width {
                break;
            }
            if let Some(cell) = buf.cell_mut((area.x + dx, area.y + dy)) {
                cell.modifier.insert(Modifier::REVERSED);
            }
        }
    }
}

/// OSC 52 clipboard sequence for `text`.
pub fn osc52(text: &str) -> String {
    format!(
        "\x1b]52;c;{}\x07",
        base64::engine::general_purpose::STANDARD.encode(text)
    )
}

/// Put `text` on the system clipboard (arboard, handle kept alive in a OnceLock);
/// when that fails or `SSH_TTY` is set, also emit OSC 52 on stdout.
pub fn copy(text: &str) {
    // Kept alive for the whole process: on X11 the clipboard content dies with the handle.
    static CLIPBOARD: OnceLock<Mutex<Option<arboard::Clipboard>>> = OnceLock::new();
    let native = CLIPBOARD
        .get_or_init(|| Mutex::new(arboard::Clipboard::new().ok()))
        .lock()
        .ok()
        .and_then(|mut cb| cb.as_mut().map(|c| c.set_text(text.to_string()).is_ok()))
        .unwrap_or(false);
    if !native || std::env::var_os("SSH_TTY").is_some() {
        use std::io::Write;
        let mut out = std::io::stdout();
        let _ = out.write_all(osc52(text).as_bytes());
        let _ = out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Line;

    const AREA: Rect = Rect {
        x: 2,
        y: 1,
        width: 10,
        height: 4,
    };

    #[test]
    fn cells_map_to_flat_positions_inside_the_area_only() {
        assert_eq!(cell_to_pos(AREA, 30, 2, 1), Some((30, 0)));
        assert_eq!(cell_to_pos(AREA, 30, 11, 4), Some((33, 9)));
        assert_eq!(cell_to_pos(AREA, 30, 1, 1), None);
        assert_eq!(cell_to_pos(AREA, 30, 2, 5), None);
    }

    #[test]
    fn clamp_pos_pins_outside_cells_to_the_edges() {
        assert_eq!(clamp_pos(AREA, 30, 0, 0), (30, 0));
        assert_eq!(clamp_pos(AREA, 30, 50, 9), (33, 9));
    }

    #[test]
    fn ordered_handles_a_reverse_drag() {
        let s = Sel {
            anchor: (5, 3),
            head: (2, 7),
        };
        assert_eq!(ordered(&s), ((2, 7), (5, 3)));
        let s = Sel {
            anchor: (4, 8),
            head: (4, 1),
        };
        assert_eq!(ordered(&s), ((4, 1), (4, 8)));
    }

    #[test]
    fn extract_joins_soft_rows_without_a_newline() {
        let lines = [Line::raw("the quick "), Line::raw("brown fox")];
        let s = Sel {
            anchor: (0, 0),
            head: (1, 8),
        };
        assert_eq!(extract(&lines, &[false, true], &s), "the quick brown fox");
    }

    #[test]
    fn extract_trims_line_ends_and_joins_hard_rows_with_newlines() {
        let lines = [Line::raw("abc   "), Line::raw("def  ")];
        let s = Sel {
            anchor: (0, 0),
            head: (1, 9),
        };
        assert_eq!(extract(&lines, &[false, false], &s), "abc\ndef");
    }

    #[test]
    fn extract_takes_partial_first_and_last_rows() {
        let lines = [Line::raw("hello world"), Line::raw("second row")];
        let s = Sel {
            anchor: (1, 5),
            head: (0, 6),
        }; // reverse drag
        assert_eq!(extract(&lines, &[false, false], &s), "world\nsecond");
    }

    #[test]
    fn extract_never_splits_wide_chars() {
        let lines = [Line::raw("a日b")];
        // cols: a=0, 日=1..=2, b=3. Selecting 0..=1 includes 日 (starts at col 1).
        assert_eq!(
            extract(
                &lines,
                &[false],
                &Sel {
                    anchor: (0, 0),
                    head: (0, 1)
                }
            ),
            "a日"
        );
        // Starting on 日's right half (col 2) excludes it.
        assert_eq!(
            extract(
                &lines,
                &[false],
                &Sel {
                    anchor: (0, 2),
                    head: (0, 3)
                }
            ),
            "b"
        );
    }

    #[test]
    fn extract_ignores_rows_past_the_end() {
        let lines = [Line::raw("only")];
        assert_eq!(
            extract(
                &lines,
                &[false],
                &Sel {
                    anchor: (0, 0),
                    head: (5, 3)
                }
            ),
            "only"
        );
    }

    #[test]
    fn highlight_reverses_only_the_selected_visible_cells() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 6));
        // pane rows y=1..=4 show flat lines 30..=33; select (31,3)..=(32,1)
        highlight(
            &mut buf,
            AREA,
            30,
            &Sel {
                anchor: (32, 1),
                head: (31, 3),
            },
        );
        let rev = |x, y| buf[(x, y)].modifier.contains(Modifier::REVERSED);
        assert!(!rev(AREA.x + 2, 2));
        assert!(rev(AREA.x + 3, 2) && rev(AREA.x + 9, 2)); // rest of row 31
        assert!(rev(AREA.x, 3) && rev(AREA.x + 1, 3)); // row 32 up to col 1
        assert!(!rev(AREA.x + 2, 3));
        assert!(!rev(AREA.x, 1) && !rev(AREA.x, 4));
        assert!(!rev(0, 2), "outside the area untouched");
    }

    #[test]
    fn osc52_wraps_base64_text() {
        assert_eq!(osc52("hi"), "\x1b]52;c;aGk=\x07");
    }
}
