//! The live bottom region, drawn by us and nobody else.
//!
//! `Screen` owns the only number that matters: how many lines the block
//! currently occupies. It erases by that number and reprints — it never asks
//! the terminal or a library where the cursor is. Only relative moves
//! (`MoveUp` / `MoveDown`) and column addressing (`MoveToColumn`) are emitted;
//! absolute row addressing is the bug class this module exists to remove.
//!
//! Invariant: when [`Screen::paint`] returns, the cursor sits where the user's
//! input cursor is displayed, and `cursor_up` is the number of rows to move
//! DOWN from there to reach the block's LAST line.

use std::io::{self, Write};

use crossterm::cursor::{MoveDown, MoveToColumn, MoveUp};
use crossterm::queue;
use crossterm::terminal::{Clear, ClearType};
use ratatui::layout::Size;

use crate::tui::convert;

/// The live bottom region: input frame plus status line.
#[allow(dead_code)] // Task 4 wires this into run.rs; consumed there.
pub(crate) struct Screen<W: Write> {
    out: W,
    /// How many lines the block occupies right now (K2).
    painted: u16,
    /// Rows from the visible cursor DOWN to the block's last line.
    cursor_up: u16,
    /// Display widths of the lines as last printed, for the resize recount.
    last_widths: Vec<u16>,
    size: Size,
}

#[allow(dead_code)] // Task 4 wires these into run.rs; consumed there.
impl<W: Write> Screen<W> {
    pub(crate) fn new(out: W, size: Size) -> Self {
        Self {
            out,
            painted: 0,
            cursor_up: 0,
            last_widths: Vec::new(),
            size,
        }
    }

    /// Erase the block: drop to its last line, then clear upward `painted`
    /// times. Leaves the cursor at column 0 of the block's first line.
    /// Spec steps 1-2 of `paint`, shared with `page` and `clear_block`.
    fn erase_block(&mut self) -> io::Result<()> {
        if self.cursor_up != 0 {
            queue!(self.out, MoveDown(self.cursor_up))?;
        }
        queue!(self.out, MoveToColumn(0))?;
        for i in 0..self.painted {
            queue!(self.out, Clear(ClearType::CurrentLine))?;
            if i + 1 != self.painted {
                queue!(self.out, MoveUp(1))?;
            }
        }
        queue!(self.out, MoveToColumn(0))?;
        Ok(())
    }

    fn forget_block(&mut self) {
        self.painted = 0;
        self.cursor_up = 0;
        self.last_widths.clear();
    }

    /// Erase the current block and reprint `lines` in its place, leaving the
    /// cursor on row `cursor_line` (0-based within the block) at `cursor_col`.
    ///
    /// Two clamps are silent and deliberate. A `cursor_line` at or past the
    /// block's last row collapses to the last row: the chained
    /// `saturating_sub` in step 5 floors the upward move at 0. And
    /// `cursor_col` reaches `MoveToColumn` unclamped against `size.width` —
    /// a column past the right margin is left to the terminal, which parks
    /// the cursor on the last column.
    pub(crate) fn paint(
        &mut self,
        lines: &[String],
        cursor_line: u16,
        cursor_col: u16,
    ) -> io::Result<()> {
        self.erase_block()?;

        // Step 3: lines separated by CRLF -- in raw mode `\n` does not return
        // the carriage. Each line is clipped so the terminal never auto-wraps,
        // which is what would desynchronize `painted`.
        let mut widths = Vec::with_capacity(lines.len());
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                self.out.write_all(b"\r\n")?;
            }
            let (clipped, width) = convert::clip_to_width(line, self.size.width);
            self.out.write_all(clipped.as_bytes())?;
            widths.push(width);
        }

        queue!(self.out, Clear(ClearType::FromCursorDown))?; // step 4 (K4)

        // Step 5: back up from the last line to the input cursor's row.
        let painted = lines.len().min(u16::MAX as usize) as u16;
        let up = painted.saturating_sub(1).saturating_sub(cursor_line);
        if up != 0 {
            queue!(self.out, MoveUp(up))?;
        }
        queue!(self.out, MoveToColumn(cursor_col))?;

        self.cursor_up = up; // step 6
        self.painted = painted;
        self.last_widths = widths;
        self.out.flush()
    }

    /// Push `content` into the scrollback above the block. The block is erased
    /// first; the caller reprints it with [`Screen::paint`].
    pub(crate) fn page(&mut self, content: &[String]) -> io::Result<()> {
        self.erase_block()?;
        self.forget_block();
        for line in content {
            self.out.write_all(line.as_bytes())?;
            self.out.write_all(b"\r\n")?;
        }
        self.out.flush()
    }

    /// Adopt a new terminal size, erasing whatever the terminal made of the
    /// old block. The cursor is first driven to the bottom-most screen row --
    /// terminals stop at the last row, so `painted * 2` down is a known place
    /// and the block is always at the bottom. The caller then reprints with
    /// [`Screen::paint`], which clears below it (K4).
    pub(crate) fn resize(&mut self, size: Size) -> io::Result<()> {
        let down = self.painted.saturating_mul(2);
        if down != 0 {
            queue!(self.out, MoveDown(down))?;
        }
        let rewrapped = rewrapped_rows(&self.last_widths, size.width, self.painted);
        queue!(self.out, MoveToColumn(0))?;
        for i in 0..rewrapped {
            queue!(self.out, Clear(ClearType::CurrentLine))?;
            if i + 1 != rewrapped {
                queue!(self.out, MoveUp(1))?;
            }
        }
        queue!(self.out, MoveToColumn(0))?;
        self.size = size;
        self.forget_block();
        self.out.flush()
    }

    /// Erase the block and everything below it -- used on shutdown.
    pub(crate) fn clear_block(&mut self) -> io::Result<()> {
        self.erase_block()?;
        queue!(self.out, Clear(ClearType::FromCursorDown))?;
        self.forget_block();
        self.out.flush()
    }
}

/// Rows the block occupies after the terminal rewrapped it to `new_width`:
/// `sum(ceil(w_i / new_width))`, clamped into `painted..=painted * 2`. An empty
/// line still occupies one row. `new_width == 0` cannot divide, so the recount
/// falls back to `painted`.
#[allow(dead_code)] // Task 4 wires this into run.rs; consumed there.
pub(crate) fn rewrapped_rows(widths: &[u16], new_width: u16, painted: u16) -> u16 {
    if new_width == 0 {
        return painted;
    }
    let sum: u32 = widths
        .iter()
        .map(|&w| u32::from(w.max(1)).div_ceil(u32::from(new_width)))
        .sum();
    let floor = u32::from(painted);
    sum.clamp(floor, floor * 2).min(u32::from(u16::MAX)) as u16
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use super::*;

    /// A writer whose bytes stay inspectable after `Screen` has taken it by
    /// value. Pattern copied from the (soon deleted) `backend_wrap.rs` tests;
    /// a flush counter is added because flushing is a requirement here.
    #[derive(Clone, Default)]
    struct SharedBuf {
        bytes: Rc<RefCell<Vec<u8>>>,
        flushes: Rc<Cell<usize>>,
    }

    impl SharedBuf {
        fn text(&self) -> String {
            String::from_utf8(self.bytes.borrow().clone()).expect("output is utf-8")
        }
        fn clear(&self) {
            self.bytes.borrow_mut().clear();
        }
    }

    impl Write for SharedBuf {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.bytes.borrow_mut().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            self.flushes.set(self.flushes.get() + 1);
            Ok(())
        }
    }

    fn screen(width: u16, height: u16) -> (Screen<SharedBuf>, SharedBuf) {
        let buf = SharedBuf::default();
        (Screen::new(buf.clone(), Size::new(width, height)), buf)
    }

    fn count(hay: &str, needle: &str) -> usize {
        hay.matches(needle).count()
    }

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// True if the bytes contain any absolute cursor addressing: CUP (`H`/`f`),
    /// a cursor-position report/query (`n`), or save/restore (`s`/`u`, `ESC 7`,
    /// `ESC 8`). This is the mechanical guard for K3 — never weaken it.
    fn contains_absolute_addressing(bytes: &[u8]) -> bool {
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == 0x1b {
                if i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                    let mut j = i + 2;
                    while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
                        j += 1;
                    }
                    if j < bytes.len() {
                        if matches!(bytes[j], b'H' | b'f' | b'n' | b's' | b'u') {
                            return true;
                        }
                        i = j + 1;
                        continue;
                    }
                } else if i + 1 < bytes.len() && matches!(bytes[i + 1], b'7' | b'8') {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// Drop every CSI sequence, leaving only the printable payload.
    fn strip_csi(s: &str) -> String {
        let bytes = s.as_bytes();
        let mut out = String::new();
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == 0x1b {
                let mut j = i + 1;
                if j < bytes.len() && bytes[j] == b'[' {
                    j += 1;
                    while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
                        j += 1;
                    }
                    if j < bytes.len() {
                        j += 1;
                    }
                }
                i = j;
                continue;
            }
            let ch = s[i..].chars().next().expect("char boundary");
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    /// Every escape this module is allowed to emit, and nothing else.
    /// `crossterm 0.29` writes `Clear(FromCursorDown)` in its default-parameter
    /// form `ESC[J`, which is identical in meaning to `ESC[0J`.
    const CLEAR_LINE: &str = "\x1b[2K";
    const CLEAR_BELOW: &str = "\x1b[J";

    /// A full lifecycle, used by the guard tests.
    fn lifecycle() -> SharedBuf {
        let (mut s, buf) = screen(40, 20);
        let block = lines(&["────", "> hi", "────", "ready"]);
        s.paint(&block, 1, 4).unwrap();
        s.paint(&block, 1, 5).unwrap();
        s.page(&lines(&["transcript line"])).unwrap();
        s.paint(&block, 1, 5).unwrap();
        s.resize(Size::new(20, 20)).unwrap();
        s.paint(&lines(&["──", "> h", "──", "rdy"]), 1, 3).unwrap();
        // A one-line block: step 5's upward move is 0, and the erase that
        // follows descends by 0 -- both zero-count guards on the up axis.
        s.paint(&lines(&["only"]), 0, 0).unwrap();
        s.clear_block().unwrap();
        // Resizing a screen that has never painted: the descend count is 0.
        let mut fresh = Screen::new(buf.clone(), Size::new(20, 20));
        fresh.resize(Size::new(40, 20)).unwrap();
        buf
    }

    // ---- K3: the guard -------------------------------------------------

    #[test]
    fn the_absolute_addressing_guard_detects_a_known_bad_sequence() {
        assert!(contains_absolute_addressing(b"\x1b[3;5H"));
        assert!(contains_absolute_addressing(b"\x1b[6n"));
        assert!(contains_absolute_addressing(b"\x1b[s"));
        assert!(!contains_absolute_addressing(
            b"\x1b[2A\x1b[1G\x1b[2K\x1b[J"
        ));
    }

    #[test]
    fn no_absolute_cursor_addressing_ever_reaches_the_writer() {
        let buf = lifecycle();
        let bytes = buf.bytes.borrow().clone();
        assert!(
            !contains_absolute_addressing(&bytes),
            "absolute addressing leaked into the output: {:?}",
            buf.text()
        );
    }

    #[test]
    fn relative_moves_are_never_emitted_with_a_zero_count() {
        let t = lifecycle().text();
        assert!(!t.contains("\x1b[0A"), "MoveUp(0) emitted: {t:?}");
        assert!(!t.contains("\x1b[0B"), "MoveDown(0) emitted: {t:?}");
    }

    // ---- paint ---------------------------------------------------------

    #[test]
    fn first_paint_emits_no_line_erase() {
        let (mut s, buf) = screen(40, 20);
        s.paint(&lines(&["────", "> hi", "────", "ready"]), 1, 4)
            .unwrap();
        let t = buf.text();
        assert_eq!(
            count(&t, CLEAR_LINE),
            0,
            "first paint must erase nothing: {t:?}"
        );
        assert_eq!(s.painted, 4);
    }

    #[test]
    fn second_paint_erases_exactly_painted_lines() {
        let (mut s, buf) = screen(40, 20);
        s.paint(&lines(&["a", "b", "c", "d"]), 1, 2).unwrap();
        buf.clear();
        // The second block is SHORTER on purpose. Erasing by the previous
        // count (4) and erasing by the new count (2) only differ when the two
        // counts differ -- with equal blocks the bug is invisible.
        s.paint(&lines(&["a", "b"]), 1, 2).unwrap();
        let t = buf.text();
        assert_eq!(
            count(&t, CLEAR_LINE),
            4,
            "erase count must equal the PREVIOUS painted count, not the new one: {t:?}"
        );
        // previous painted - 1 single-row hops between the erases, and nothing
        // else emits MoveUp(1) here (step 5's `up` is 0 for this block).
        assert_eq!(
            count(&t, "\x1b[1A"),
            3,
            "one MoveUp per erase but the last: {t:?}"
        );
    }

    #[test]
    fn second_paint_descends_to_the_blocks_last_line_before_erasing() {
        let (mut s, buf) = screen(40, 20);
        s.paint(&lines(&["a", "b", "c", "d"]), 1, 2).unwrap();
        assert_eq!(s.cursor_up, 2, "row 1 of 4 is 2 rows above the last");
        buf.clear();
        s.paint(&lines(&["a", "b", "c", "d"]), 1, 2).unwrap();
        let t = buf.text();
        // MoveDown(cursor_up) then MoveToColumn(0): the erase walks upward from
        // the block's LAST line, so it has to get there first.
        assert!(
            t.starts_with("\x1b[2B\x1b[1G"),
            "paint must descend cursor_up rows before erasing: {t:?}"
        );
    }

    #[test]
    fn every_paint_clears_from_cursor_down_after_the_last_content() {
        let (mut s, buf) = screen(40, 20);
        let block = lines(&["a", "b", "c"]);
        for _ in 0..2 {
            buf.clear();
            s.paint(&block, 0, 1).unwrap();
            let t = buf.text();
            let at = t
                .rfind(CLEAR_BELOW)
                .unwrap_or_else(|| panic!("no K4 clear: {t:?}"));
            let tail = &t[at + CLEAR_BELOW.len()..];
            assert!(!tail.contains(CLEAR_LINE), "erase after K4 clear: {t:?}");
            assert_eq!(strip_csi(tail), "", "content printed after K4 clear: {t:?}");
        }
    }

    #[test]
    fn paint_writes_lines_separated_by_crlf_with_no_trailing_newline() {
        let (mut s, buf) = screen(40, 20);
        s.paint(&lines(&["one", "two", "three"]), 0, 0).unwrap();
        assert_eq!(strip_csi(&buf.text()), "one\r\ntwo\r\nthree");
    }

    #[test]
    fn cursor_up_is_the_distance_down_to_the_blocks_last_line() {
        let (mut s, buf) = screen(40, 20);
        s.paint(&lines(&["a", "b", "c", "d"]), 1, 3).unwrap();
        assert_eq!(s.painted, 4);
        assert_eq!(s.cursor_up, 2, "row 1 of 4 is 2 rows above the last");
        assert!(
            buf.text().ends_with("\x1b[2A\x1b[4G"),
            "paint must end on the input cursor: {:?}",
            buf.text()
        );
    }

    #[test]
    fn paint_clips_every_line_to_the_screen_width() {
        let (mut s, buf) = screen(5, 20);
        s.paint(&lines(&["hello world"]), 0, 0).unwrap();
        assert_eq!(strip_csi(&buf.text()), "hello");
        assert_eq!(s.last_widths, vec![5]);
    }

    #[test]
    fn paint_flushes_once() {
        let (mut s, buf) = screen(40, 20);
        s.paint(&lines(&["a"]), 0, 0).unwrap();
        assert_eq!(buf.flushes.get(), 1);
    }

    #[test]
    fn painting_an_empty_block_zeroes_the_counters() {
        let (mut s, _buf) = screen(40, 20);
        s.paint(&lines(&["a", "b"]), 0, 0).unwrap();
        s.paint(&[], 0, 0).unwrap();
        assert_eq!(s.painted, 0);
        assert_eq!(s.cursor_up, 0);
        assert!(s.last_widths.is_empty());
    }

    // ---- page ----------------------------------------------------------

    #[test]
    fn page_erases_the_block_prints_content_with_crlf_and_zeroes_painted() {
        let (mut s, buf) = screen(40, 20);
        s.paint(&lines(&["a", "b"]), 0, 0).unwrap();
        buf.clear();
        s.page(&lines(&["hello", "world"])).unwrap();
        let t = buf.text();
        assert_eq!(
            count(&t, CLEAR_LINE),
            2,
            "page must erase the whole block: {t:?}"
        );
        assert!(
            t.ends_with("hello\r\nworld\r\n"),
            "content must end with CRLF: {t:?}"
        );
        assert_eq!(s.painted, 0);
        assert_eq!(s.cursor_up, 0);
        assert!(s.last_widths.is_empty());
    }

    // ---- resize --------------------------------------------------------

    #[test]
    fn resize_drops_to_the_bottom_erases_the_rewrapped_rows_and_resets() {
        let (mut s, buf) = screen(80, 20);
        s.paint(&lines(&[&"x".repeat(80), &"y".repeat(80)]), 0, 0)
            .unwrap();
        buf.clear();
        s.resize(Size::new(40, 20)).unwrap();
        let t = buf.text();
        assert!(
            t.starts_with("\x1b[4B"),
            "must drop painted*2 rows first: {t:?}"
        );
        assert_eq!(
            count(&t, CLEAR_LINE),
            4,
            "two 80-wide lines rewrap to four rows: {t:?}"
        );
        assert_eq!(s.size.width, 40);
        assert_eq!(s.painted, 0);
        assert_eq!(s.cursor_up, 0);
    }

    // ---- clear_block ---------------------------------------------------

    #[test]
    fn clear_block_erases_the_block_and_clears_everything_below() {
        let (mut s, buf) = screen(40, 20);
        s.paint(&lines(&["a", "b", "c"]), 0, 0).unwrap();
        buf.clear();
        s.clear_block().unwrap();
        let t = buf.text();
        assert_eq!(count(&t, CLEAR_LINE), 3);
        assert!(
            t.ends_with(CLEAR_BELOW),
            "must end by clearing below: {t:?}"
        );
        assert_eq!(s.painted, 0);
    }

    // ---- rewrapped_rows (pure) -----------------------------------------

    #[test]
    fn rewrapped_rows_roughly_doubles_when_the_width_halves() {
        assert_eq!(rewrapped_rows(&[80, 80, 80, 80], 40, 4), 8);
    }

    #[test]
    fn rewrapped_rows_is_clamped_into_the_painted_range() {
        // Raw sum is 4 x 8 = 32, far above the painted*2 ceiling of 8.
        assert_eq!(rewrapped_rows(&[80, 80, 80, 80], 10, 4), 8);
        // Raw sum is 2, below the painted floor of 6.
        assert_eq!(rewrapped_rows(&[5, 5], 80, 6), 6);
    }

    #[test]
    fn rewrapped_rows_never_divides_by_zero() {
        assert_eq!(rewrapped_rows(&[10, 20], 0, 3), 3);
    }

    #[test]
    fn rewrapped_rows_counts_an_empty_line_as_one_row() {
        // Two empty lines plus one 80-wide line that rewraps to two rows.
        // Counting the empty lines as zero would give 2, which the floor would
        // silently round up to 3 -- so 4 is what proves the rule.
        assert_eq!(rewrapped_rows(&[0, 0, 80], 40, 3), 4);
    }
}
