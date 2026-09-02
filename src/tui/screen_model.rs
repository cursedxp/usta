//! Test-only terminal grid model.
//!
//! Every existing `screen.rs` test inspects the raw bytes we queue into a
//! fake `Write` — none of them applies those bytes to anything resembling a
//! real screen. `TermModel` closes that gap: it is a minimal terminal
//! emulator that actually APPLIES the narrow set of ANSI escapes `screen.rs`
//! is allowed to emit, so a test can assert on the resulting grid instead of
//! the byte soup.
//!
//! The recognised escape set mirrors exactly what `screen.rs` emits via
//! crossterm: `MoveUp`/`MoveDown` (`ESC[nA`/`ESC[nB`), `MoveToColumn`
//! (`ESC[nG`), `Clear(CurrentLine)` (`ESC[2K`), `Clear(FromCursorDown)`
//! (`ESC[0J` or `ESC[J`), SGR (`ESC[...m`, swallowed), `\r`, `\n`, and plain
//! text. Anything else — most importantly absolute row addressing (`H`,
//! `f`, `d`, ...) — panics. That panic is a second, mechanical guard against
//! the same bug class `contains_absolute_addressing` guards against in
//! `screen.rs`: absolute addressing once slipped past review, so here it
//! can't slip past a test run either.

use std::iter::Peekable;
use std::str::Chars;

/// A terminal grid that `apply` writes into, cell by cell.
pub(crate) struct TermModel {
    rows: Vec<String>,
    /// `(column, row)`, both 0-based — matches crossterm's own cursor
    /// position convention, which is also what `MoveToColumn`'s 1-based
    /// escape parameter gets converted to/from.
    cursor: (u16, u16),
    w: u16,
    h: u16,
}

impl TermModel {
    pub(crate) fn new(w: u16, h: u16) -> Self {
        Self {
            rows: vec![" ".repeat(w as usize); h as usize],
            cursor: (0, 0),
            w,
            h,
        }
    }

    /// Apply a chunk of emitted bytes to the grid. Panics on any escape
    /// outside the narrow recognised set — see module docs.
    pub(crate) fn apply(&mut self, bytes: &[u8]) {
        let s = std::str::from_utf8(bytes).expect("emitted bytes are valid UTF-8");
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\x1b' => self.apply_escape(&mut chars),
                '\r' => self.cursor.0 = 0,
                '\n' => self.line_feed(),
                _ => self.put_char(c),
            }
        }
    }

    /// Row indices (0-based) whose text contains `needle`.
    pub(crate) fn rows_containing(&self, needle: char) -> Vec<u16> {
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.contains(needle))
            .map(|(i, _)| i as u16)
            .collect()
    }

    pub(crate) fn snapshot(&self) -> Vec<String> {
        self.rows.clone()
    }

    fn apply_escape(&mut self, chars: &mut Peekable<Chars<'_>>) {
        if chars.next() != Some('[') {
            panic!("unrecognized escape sequence: ESC not followed by CSI '['");
        }
        let mut param = String::new();
        let final_byte = loop {
            match chars.next() {
                Some(c) if c.is_ascii_digit() || c == ';' => param.push(c),
                Some(c) => break c,
                None => panic!("unrecognized escape sequence: unterminated ESC[{param}"),
            }
        };
        match final_byte {
            'A' => self.move_up(parse_param(&param)),
            'B' => self.move_down(parse_param(&param)),
            'G' => self.move_to_column(parse_param(&param)),
            'K' if param == "2" => self.clear_current_line(),
            'J' if param.is_empty() || param == "0" => self.clear_from_cursor_down(),
            // SGR: swallowed silently, regardless of its parameters. Does
            // not touch the grid or the cursor.
            'm' => {}
            _ => panic!("unrecognized escape sequence: ESC[{param}{final_byte}"),
        }
    }

    /// Real terminal behaviour: MoveUp stops at the top row, it does not
    /// wrap or go negative.
    fn move_up(&mut self, n: u16) {
        self.cursor.1 = self.cursor.1.saturating_sub(n);
    }

    /// Real terminal behaviour: MoveDown stops at the bottom row.
    fn move_down(&mut self, n: u16) {
        let bottom = self.h.saturating_sub(1);
        self.cursor.1 = self.cursor.1.saturating_add(n).min(bottom);
    }

    /// `n` is the 1-based CSI parameter (`ESC[nG`); the model's cursor
    /// column is 0-based, so this is where the conversion happens.
    fn move_to_column(&mut self, n: u16) {
        let rightmost = self.w.saturating_sub(1);
        self.cursor.0 = n.saturating_sub(1).min(rightmost);
    }

    /// `ESC[2K`: clears the whole current row, cursor unchanged.
    fn clear_current_line(&mut self) {
        self.rows[self.cursor.1 as usize] = " ".repeat(self.w as usize);
    }

    /// `ESC[0J` / `ESC[J`: clears from the cursor (inclusive) to the end of
    /// the current row, and every row below it entirely. Cursor unchanged.
    fn clear_from_cursor_down(&mut self) {
        let row = self.cursor.1 as usize;
        let col = self.cursor.0 as usize;
        let mut chars: Vec<char> = self.rows[row].chars().collect();
        for c in chars.iter_mut().skip(col) {
            *c = ' ';
        }
        self.rows[row] = chars.into_iter().collect();
        for r in &mut self.rows[row + 1..] {
            *r = " ".repeat(self.w as usize);
        }
    }

    /// LF moves the cursor down one row, column unchanged (raw mode has no
    /// implied CR — that is why `screen.rs` always emits `\r\n`, never bare
    /// `\n`, when it wants a fresh line). On the last row there is nothing
    /// to move down into, so the whole grid scrolls up by one and the
    /// topmost row is dropped — there is no scrollback here, it is gone.
    fn line_feed(&mut self) {
        if self.cursor.1 == self.h.saturating_sub(1) {
            self.rows.remove(0);
            self.rows.push(" ".repeat(self.w as usize));
        } else {
            self.cursor.1 += 1;
        }
    }

    /// Writes `c` at the cursor and advances by one column. Design choice:
    /// the production code (`convert::clip_to_width`) clips every line to
    /// the terminal width before it ever reaches the terminal, specifically
    /// so the terminal never has to auto-wrap. The model mirrors that
    /// contract rather than emulating auto-wrap: once the cursor reaches
    /// the right margin (`w`), further characters in the same write are
    /// simply dropped and the cursor stays pinned at `w` — it does not wrap
    /// to the next row.
    fn put_char(&mut self, c: char) {
        let (col, row) = (self.cursor.0, self.cursor.1);
        if col >= self.w {
            return;
        }
        let mut chars: Vec<char> = self.rows[row as usize].chars().collect();
        chars[col as usize] = c;
        self.rows[row as usize] = chars.into_iter().collect();
        self.cursor.0 += 1;
    }
}

/// Parses a CSI numeric parameter, defaulting to 1 when omitted (ANSI
/// default for `A`/`B`/`G`).
fn parse_param(s: &str) -> u16 {
    if s.is_empty() {
        1
    } else {
        s.parse().unwrap_or_else(|_| {
            panic!("unrecognized escape sequence: non-numeric CSI parameter {s:?}")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_model_is_blank_grid_of_given_dimensions() {
        let m = TermModel::new(4, 2);
        assert_eq!(m.snapshot(), vec!["    ".to_string(), "    ".to_string()]);
    }

    #[test]
    fn plain_text_writes_at_cursor_and_advances() {
        let mut m = TermModel::new(10, 3);
        m.apply(b"abc");
        assert_eq!(m.snapshot()[0], "abc       ");
        assert_eq!(m.cursor, (3, 0));
    }

    #[test]
    fn carriage_return_then_line_feed_starts_next_row_at_column_zero() {
        let mut m = TermModel::new(5, 3);
        m.apply(b"ab\r\ncd");
        assert_eq!(m.snapshot()[0], "ab   ");
        assert_eq!(m.snapshot()[1], "cd   ");
        assert_eq!(m.cursor, (2, 1));
    }

    #[test]
    fn line_feed_alone_moves_down_without_resetting_column() {
        let mut m = TermModel::new(5, 3);
        m.apply(b"ab\ncd");
        assert_eq!(m.snapshot()[0], "ab   ");
        assert_eq!(m.snapshot()[1], "  cd ");
        assert_eq!(m.cursor, (4, 1));
    }

    #[test]
    fn clear_current_line_only_clears_that_row() {
        let mut m = TermModel::new(5, 2);
        m.apply(b"abc\r\nxyz");
        m.apply(b"\x1b[2K");
        assert_eq!(m.snapshot()[0], "abc  ");
        assert_eq!(m.snapshot()[1], "     ");
    }

    #[test]
    fn clear_from_cursor_down_clears_tail_of_row_and_rows_below() {
        let mut m = TermModel::new(5, 3);
        m.apply(b"abcde\r\nfghij\r\nklmno");
        m.apply(b"\x1b[1A"); // up to row 1
        m.apply(b"\x1b[3G"); // column 3 (1-based) -> cursor col index 2
        m.apply(b"\x1b[0J");
        assert_eq!(m.snapshot()[0], "abcde");
        assert_eq!(m.snapshot()[1], "fg   ");
        assert_eq!(m.snapshot()[2], "     ");
    }

    #[test]
    fn clear_from_cursor_down_recognizes_bare_j_same_as_0j() {
        let mut m = TermModel::new(4, 2);
        m.apply(b"ab\r\ncd");
        m.apply(b"\x1b[1G"); // back to column 0, still row 1
        m.apply(b"\x1b[J");
        assert_eq!(m.snapshot()[1], "    ");
    }

    #[test]
    fn move_up_stops_at_top_row() {
        let mut m = TermModel::new(5, 3);
        m.apply(b"\x1b[5A");
        assert_eq!(m.cursor.1, 0);
    }

    #[test]
    fn move_down_stops_at_bottom_row() {
        let mut m = TermModel::new(5, 3);
        m.apply(b"\x1b[10B");
        assert_eq!(m.cursor.1, 2);
    }

    #[test]
    fn line_feed_on_last_row_scrolls_grid_up_and_drops_top_row() {
        let mut m = TermModel::new(4, 2);
        m.apply(b"ab\r\ncd");
        m.apply(b"\r\nef");
        assert_eq!(m.snapshot(), vec!["cd  ".to_string(), "ef  ".to_string()]);
    }

    #[test]
    fn move_to_column_converts_1_based_param_to_0_based_cursor() {
        let mut m = TermModel::new(10, 1);
        m.apply(b"\x1b[1G");
        assert_eq!(m.cursor.0, 0, "ESC[1G is column 1, i.e. cursor index 0");
        m.apply(b"\x1b[5G");
        assert_eq!(m.cursor.0, 4, "ESC[5G is column 5, i.e. cursor index 4");
    }

    #[test]
    fn sgr_sequences_are_swallowed_without_affecting_grid_or_cursor() {
        let mut m = TermModel::new(10, 1);
        m.apply(b"\x1b[1mbold\x1b[0m");
        assert_eq!(m.snapshot()[0], "bold      ");
        assert_eq!(m.cursor.0, 4);
    }

    #[test]
    fn text_clips_at_right_margin_instead_of_wrapping() {
        let mut m = TermModel::new(3, 2);
        m.apply(b"abcdef");
        assert_eq!(m.snapshot()[0], "abc");
        assert_eq!(m.snapshot()[1], "   ");
        assert_eq!(m.cursor, (3, 0));
    }

    #[test]
    fn rows_containing_finds_matching_rows() {
        let mut m = TermModel::new(5, 3);
        m.apply(b"ab\r\ncXc\r\nde");
        assert_eq!(m.rows_containing('X'), vec![1]);
        assert_eq!(m.rows_containing('z'), Vec::<u16>::new());
    }

    #[test]
    #[should_panic(expected = "unrecognized escape sequence")]
    fn unrecognized_absolute_addressing_escape_panics() {
        let mut m = TermModel::new(10, 3);
        m.apply(b"\x1b[3;5H");
    }

    #[test]
    #[should_panic(expected = "unrecognized escape sequence")]
    fn unrecognized_vpa_escape_panics() {
        // ESC[5d is VPA -- what crossterm's MoveToRow emits. Absolute row
        // addressing must reach the panic arm, not a silent skip.
        let mut m = TermModel::new(10, 5);
        m.apply(b"\x1b[5d");
    }

    #[test]
    fn move_to_column_clamps_out_of_range_column_to_rightmost() {
        let mut m = TermModel::new(10, 1);
        m.apply(b"\x1b[100G");
        assert_eq!(
            m.cursor.0, 9,
            "ESC[100G on width-10 grid clamps to rightmost column 9"
        );
    }

    #[test]
    #[should_panic(expected = "unrecognized escape sequence")]
    fn clear_current_line_with_wrong_param_panics() {
        let mut m = TermModel::new(5, 2);
        m.apply(b"\x1b[1K");
    }

    #[test]
    #[should_panic(expected = "unrecognized escape sequence")]
    fn clear_screen_variant_other_than_from_cursor_down_panics() {
        let mut m = TermModel::new(5, 2);
        m.apply(b"\x1b[2J");
    }
}
