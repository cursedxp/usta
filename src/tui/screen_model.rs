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

    /// Resize the grid to `w`x`h`, re-deriving both the row contents and the
    /// cursor position under `policy`. See `ResizePolicy` for what each
    /// policy models.
    pub(crate) fn resize(&mut self, w: u16, h: u16, policy: ResizePolicy) {
        let (rows, cursor) = match policy {
            ResizePolicy::NoReflow => self.resize_no_reflow(w),
            ResizePolicy::Reflow => self.resize_reflow(w),
        };
        let (rows, cursor) = Self::adjust_height(rows, cursor, h, w);
        self.rows = rows;
        self.cursor = cursor;
        self.w = w;
        self.h = h;
    }

    /// `NoReflow`: every grid row stays exactly one physical row. Content
    /// past the new width is clipped (or the row is padded, if widening).
    /// The cursor's row is untouched here; its column clamps into the new
    /// width — `w` itself is a valid column, meaning "one past the last
    /// cell", matching the convention `put_char` already leaves the cursor
    /// in once a row is filled (see `text_clips_at_right_margin_instead_of_wrapping`).
    fn resize_no_reflow(&self, w: u16) -> (Vec<String>, (u16, u16)) {
        let rows = self.rows.iter().map(|row| clip_or_pad(row, w)).collect();
        let cursor = (self.cursor.0.min(w), self.cursor.1);
        (rows, cursor)
    }

    /// `Reflow`: each grid row is its own logical line, re-wrapped to the
    /// new width independently of every other row — hard-terminated lines
    /// never merge, on narrowing OR widening, so a row previously split by a
    /// narrowing does not re-merge when widened back out.
    ///
    /// Re-wrapping is computed on the row's CONTENT length with trailing
    /// blanks stripped, not on its padded grid width. Every row in `self.rows`
    /// is always padded to exactly `self.w` chars (see `TermModel::new` and
    /// `put_char`'s row-write), so reflowing on the padded length would make
    /// every row exactly `self.w` wide and split on ANY narrowing, real
    /// content or not. A real terminal only knows how far a line was
    /// actually written, which is what the trailing-blank strip recovers.
    /// An all-blank row still strips down to a single blank logical line
    /// (never zero rows) — a blank row is still a row.
    fn resize_reflow(&self, w: u16) -> (Vec<String>, (u16, u16)) {
        let width = w as usize;
        let mut rows = Vec::new();
        let mut cursor = (0u16, 0u16);
        for (i, row) in self.rows.iter().enumerate() {
            let content: Vec<char> = row.trim_end_matches(' ').chars().collect();
            let chunk_count = if content.is_empty() {
                1
            } else {
                content.len().div_ceil(width)
            };
            let base = rows.len() as u16;
            for chunk_index in 0..chunk_count {
                let start = chunk_index * width;
                let end = (start + width).min(content.len());
                let mut chunk = content[start..end].to_vec();
                chunk.resize(width, ' ');
                rows.push(chunk.into_iter().collect());
            }
            if i as u16 == self.cursor.1 {
                // The cursor travels with the content it sits on: locate
                // which chunk its column offset now falls in. An offset at
                // or beyond the end of content (cursor sitting in blank
                // padding never captured by the content-based split above,
                // e.g. moved there without writing) has no chunk of its own
                // — it is pinned to the last chunk's rightmost column,
                // mirroring the NoReflow clamp-into-new-width rule.
                let offset = self.cursor.0 as usize;
                let last_chunk_index = chunk_count - 1;
                let chunk_index = (offset / width).min(last_chunk_index);
                let col = (offset - chunk_index * width).min(width);
                cursor = (col as u16, base + chunk_index as u16);
            }
        }
        (rows, cursor)
    }

    /// Shared height handling for both policies, applied after the width
    /// pass. Mirrors the model's existing no-scrollback contract
    /// (`line_feed` on the last row): if reflowing/re-clipping produced more
    /// rows than the new height, the TOPMOST rows are dropped, same as a
    /// scroll — there is no scrollback to hold them. If it produced fewer,
    /// blank rows are padded onto the BOTTOM. The cursor's row shifts by
    /// however many rows were dropped off the top (saturating at 0, so a
    /// cursor whose row got dropped entirely pins to the new top row —
    /// there is nothing left of its original content to pin to).
    fn adjust_height(
        mut rows: Vec<String>,
        cursor: (u16, u16),
        h: u16,
        w: u16,
    ) -> (Vec<String>, (u16, u16)) {
        let h = h as usize;
        let (col, row) = cursor;
        if rows.len() > h {
            let excess = (rows.len() - h) as u16;
            rows.drain(0..excess as usize);
            (rows, (col, row.saturating_sub(excess)))
        } else {
            while rows.len() < h {
                rows.push(" ".repeat(w as usize));
            }
            (rows, (col, row))
        }
    }
}

/// Clips `row` to `new_w` if it is narrower than the row's current content,
/// or pads it with trailing spaces if wider. `Vec::resize` does both:
/// truncating on a shorter length, padding on a longer one.
fn clip_or_pad(row: &str, new_w: u16) -> String {
    let mut chars: Vec<char> = row.chars().collect();
    chars.resize(new_w as usize, ' ');
    chars.into_iter().collect()
}

/// How `TermModel::resize` re-derives row contents and the cursor position
/// when the grid dimensions change. Which real terminals implement which
/// policy has never been measured (iTerm2/Ghostty/kitty/WezTerm are believed
/// to reflow; some terminals and some tmux configs are believed not to) — so
/// neither is treated as "the real one". A fix that must hold under both
/// stops needing that unmeasured answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResizePolicy {
    /// Re-wrap logical lines to the new width; see `TermModel::resize_reflow`.
    Reflow,
    /// Keep one grid row per physical row, clipping or padding; see
    /// `TermModel::resize_no_reflow`.
    NoReflow,
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

    // --- resize: Reflow vs NoReflow ---

    #[test]
    fn reflow_narrow_splits_full_row_into_two_rows() {
        let mut m = TermModel::new(80, 1);
        m.apply("a".repeat(80).as_bytes());
        m.resize(40, 2, ResizePolicy::Reflow);
        let snap = m.snapshot();
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0], "a".repeat(40));
        assert_eq!(snap[1], "a".repeat(40));
    }

    #[test]
    fn no_reflow_narrow_keeps_one_row_and_clips() {
        let mut m = TermModel::new(80, 1);
        m.apply("a".repeat(80).as_bytes());
        m.resize(40, 1, ResizePolicy::NoReflow);
        let snap = m.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0], "a".repeat(40));
    }

    #[test]
    fn reflow_widening_does_not_merge_hard_terminated_lines() {
        let mut m = TermModel::new(4, 2);
        m.apply(b"ab\r\ncd");
        m.resize(8, 2, ResizePolicy::Reflow);
        let snap = m.snapshot();
        assert_eq!(
            snap.len(),
            2,
            "two hard-terminated lines must stay two rows"
        );
        assert_eq!(snap[0], format!("ab{}", " ".repeat(6)));
        assert_eq!(snap[1], format!("cd{}", " ".repeat(6)));
    }

    #[test]
    fn reflow_previously_split_row_does_not_remerge_on_widen() {
        let mut m = TermModel::new(80, 1);
        m.apply("a".repeat(80).as_bytes());
        m.resize(40, 2, ResizePolicy::Reflow); // splits into two 40-wide rows
        m.resize(80, 2, ResizePolicy::Reflow); // widen back out
        let snap = m.snapshot();
        assert_eq!(
            snap.len(),
            2,
            "a row split by narrowing must not re-merge into one row on widening"
        );
        assert_eq!(snap[0], format!("{}{}", "a".repeat(40), " ".repeat(40)));
        assert_eq!(snap[1], format!("{}{}", "a".repeat(40), " ".repeat(40)));
    }

    #[test]
    fn reflow_mostly_blank_row_does_not_split_when_width_halves() {
        // Naive-bug guard: if reflow were computed on the PADDED row length
        // (always == the old width) rather than what was actually written,
        // this 2-char row would wrongly look 80 "wide" and split into two
        // rows when narrowed from 80 to 40.
        let mut m = TermModel::new(80, 1);
        m.apply(b"hi");
        m.resize(40, 1, ResizePolicy::Reflow);
        let snap = m.snapshot();
        assert_eq!(
            snap.len(),
            1,
            "a mostly-blank row must not split when width halves"
        );
        assert_eq!(snap[0], format!("hi{}", " ".repeat(38)));
    }

    #[test]
    fn reflow_cursor_at_end_of_full_row_travels_to_second_chunk() {
        let mut m = TermModel::new(80, 1);
        m.apply("a".repeat(80).as_bytes()); // cursor now (80, 0)
        m.resize(40, 2, ResizePolicy::Reflow);
        assert_eq!(
            m.cursor,
            (40, 1),
            "cursor right after the last char of a full 80-char row lands \
             one-past the last column of the second 40-wide chunk"
        );
    }

    #[test]
    fn reflow_cursor_mid_content_travels_into_correct_chunk_and_column() {
        let mut m = TermModel::new(80, 1);
        m.apply("b".repeat(60).as_bytes());
        m.apply(b"\x1b[46G"); // move cursor to column 46 (1-based) -> index 45
        m.resize(40, 2, ResizePolicy::Reflow);
        assert_eq!(m.cursor, (5, 1));
    }

    #[test]
    fn no_reflow_cursor_row_unchanged_column_clamps_into_new_width() {
        let mut m = TermModel::new(80, 2);
        m.apply(b"ab\r\n");
        m.apply("c".repeat(80).as_bytes()); // fills row 1 fully, cursor (80, 1)
        m.resize(40, 2, ResizePolicy::NoReflow);
        assert_eq!(
            m.cursor,
            (40, 1),
            "NoReflow: cursor row does not change, column clamps into new width"
        );
        let snap = m.snapshot();
        assert_eq!(snap[1], "c".repeat(40));
    }

    #[test]
    fn no_reflow_widen_pads_row_with_spaces() {
        let mut m = TermModel::new(4, 1);
        m.apply(b"ab");
        m.resize(8, 1, ResizePolicy::NoReflow);
        assert_eq!(m.snapshot()[0], "ab      ");
    }

    #[test]
    fn resize_height_growth_pads_blank_rows_at_bottom() {
        let mut m = TermModel::new(5, 2);
        m.apply(b"ab\r\ncd");
        m.resize(5, 4, ResizePolicy::NoReflow);
        let snap = m.snapshot();
        assert_eq!(snap.len(), 4);
        assert_eq!(snap[0], "ab   ");
        assert_eq!(snap[1], "cd   ");
        assert_eq!(snap[2], "     ");
        assert_eq!(snap[3], "     ");
        assert_eq!(
            m.cursor,
            (2, 1),
            "growth pads the bottom, cursor is untouched"
        );
    }

    #[test]
    fn resize_height_shrink_drops_topmost_rows_and_shifts_cursor() {
        let mut m = TermModel::new(5, 4);
        m.apply(b"aa\r\nbb\r\ncc\r\ndd");
        m.resize(5, 2, ResizePolicy::NoReflow);
        let snap = m.snapshot();
        assert_eq!(snap, vec!["cc   ".to_string(), "dd   ".to_string()]);
        assert_eq!(
            m.cursor,
            (2, 1),
            "two rows dropped from the top, cursor row shifts down by the same amount"
        );
    }

    #[test]
    fn resize_height_shrink_pins_cursor_to_top_when_its_row_is_dropped() {
        let mut m = TermModel::new(5, 4);
        m.apply(b"aa\r\nbb"); // cursor lands on row 1; rows 2 and 3 are blank
        m.resize(5, 1, ResizePolicy::NoReflow); // excess 3 drops rows 0..3
        assert_eq!(
            m.cursor.1, 0,
            "cursor's row was entirely dropped, so it pins to the new top row"
        );
    }
}
