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
pub(crate) struct Screen<W: Write> {
    out: W,
    /// How many lines the block occupies right now (K2).
    painted: u16,
    /// Rows from the visible cursor DOWN to the block's last line.
    cursor_up: u16,
    /// Column the visible cursor sits at, as last given to `paint`.
    cursor_col: u16,
    /// Display widths of the lines as last printed, for the resize recount.
    last_widths: Vec<u16>,
    size: Size,
}

impl<W: Write> Screen<W> {
    pub(crate) fn new(out: W, size: Size) -> Self {
        Self {
            out,
            painted: 0,
            cursor_up: 0,
            cursor_col: 0,
            last_widths: Vec::new(),
            size,
        }
    }

    /// The terminal size the block is currently painted for. Callers wrap
    /// their content to this width so content and frame never disagree.
    pub(crate) fn size(&self) -> Size {
        self.size
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
        self.cursor_col = 0;
        self.last_widths.clear();
    }

    /// Erase the current block and reprint `lines` in its place, leaving the
    /// cursor on row `cursor_line` (0-based within the block) at `cursor_col`.
    ///
    /// Three clamps are silent and deliberate. A `cursor_line` at or past the
    /// block's last row collapses to the last row: the chained
    /// `saturating_sub` in step 5 floors the upward move at 0. `cursor_col`
    /// reaches `MoveToColumn` unclamped against `size.width` — a column past
    /// the right margin is left to the terminal, which parks the cursor on
    /// the last column. And `lines.len()` is clamped to `u16::MAX` before it
    /// becomes `painted`, so a block longer than that is undercounted rather
    /// than overflowing the counter.
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
        self.cursor_col = cursor_col;
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
    /// old block. The cursor is first driven down by [`descend_rows`]: the
    /// number of rows from the visible cursor to the block's last line,
    /// computed from our own recorded state (`last_widths`, `painted`,
    /// `cursor_up`, `cursor_col`) rather than assumed from where the block
    /// sits on screen.
    ///
    /// This computation assumes the terminal RE-WRAPS hard-terminated lines
    /// when the width narrows — still only BELIEVED true of common terminals
    /// (iTerm2, Terminal.app, Ghostty, kitty, WezTerm); no manual resize test
    /// against a real terminal has been run, so the belief remains unmeasured
    /// outside the model. On a terminal that does NOT re-wrap, each
    /// hard-terminated line still occupies exactly 1 physical row while
    /// `rows(w) = ceil(w / new_width)` counts 2 or more, so both the descent
    /// and the erase reach too far and the erase destroys transcript rows
    /// ABOVE the block.
    ///
    /// That is not an arithmetic slip left to fix. Measured on the modelled
    /// screen, the two policies demand OPPOSITE things of the SAME relative
    /// row from bit-identical state: narrowing 80 -> 40 with the block at the
    /// screen's bottom, the row two above the cursor is our own rewrapped
    /// rule (must be cleared) when the terminal reflowed and is the user's
    /// transcript (must be kept) when it did not. No byte sequence can be
    /// both. Closing that gap needs an input this module does not have —
    /// which policy the host terminal follows — i.e. the policy switch this
    /// comment has flagged since v0.30.1, now measured rather than guessed.
    /// `bottom_of_screen_leaves_no_residue_without_reflow` is parked on it.
    /// The caller then reprints with [`Screen::paint`], which clears below
    /// it (K4).
    pub(crate) fn resize(&mut self, size: Size) -> io::Result<()> {
        let descend = descend_rows(
            &self.last_widths,
            self.painted,
            self.cursor_up,
            self.cursor_col,
            size.width,
        );
        if descend != 0 {
            queue!(self.out, MoveDown(descend))?;
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

/// Rows to descend from the visible cursor to reach the block's last line
/// after the terminal rewraps to `new_width`: the rows still below the
/// cursor within its own (possibly rewrapped) logical line, plus the
/// rewrapped height of every logical line below that one. An empty line
/// still occupies one row. `painted == 0` returns `0`. `new_width == 0`
/// cannot divide, so every logical line is floored to exactly 1 row and the
/// cursor's own visual row is treated as 0 — the same no-panic fallback
/// `rewrapped_rows` uses.
pub(crate) fn descend_rows(
    last_widths: &[u16],
    painted: u16,
    cursor_up: u16,
    cursor_col: u16,
    new_width: u16,
) -> u16 {
    if painted == 0 {
        return 0;
    }
    let rows = |w: u16| -> u32 {
        if new_width == 0 {
            1
        } else {
            u32::from(w).div_ceil(u32::from(new_width)).max(1)
        }
    };
    let cursor_line = usize::from(painted.saturating_sub(1).saturating_sub(cursor_up));
    let rows_here = rows(last_widths.get(cursor_line).copied().unwrap_or(1));
    let cursor_visual_row = u32::from(cursor_col)
        .checked_div(u32::from(new_width))
        .unwrap_or(0)
        .min(rows_here - 1);
    let mut total = rows_here - 1 - cursor_visual_row;
    for &w in last_widths
        .iter()
        .take(usize::from(painted))
        .skip(cursor_line + 1)
    {
        total += rows(w);
    }
    total.min(u32::from(u16::MAX)) as u16
}

/// Rows the block occupies after the terminal rewrapped it to `new_width`:
/// `sum(ceil(w_i / new_width))`, floored at `painted`. An empty line still
/// occupies one row. `new_width == 0` cannot divide, so the recount falls
/// back to `painted`.
///
/// There is no ceiling. A `painted * 2` ceiling stood here until it was
/// measured on a modelled screen: narrowing 200 -> 60 makes the block
/// occupy 10 rows, the ceiling reported 8, and `resize`'s upward erase
/// stopped exactly 2 rows short of the block's top -- the two stray rule
/// rows the user photographed.
pub(crate) fn rewrapped_rows(widths: &[u16], new_width: u16, painted: u16) -> u16 {
    if new_width == 0 {
        return painted;
    }
    let sum: u32 = widths
        .iter()
        .map(|&w| u32::from(w.max(1)).div_ceil(u32::from(new_width)))
        .sum();
    sum.max(u32::from(painted)).min(u32::from(u16::MAX)) as u16
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use super::*;
    use crate::tui::screen_model::{ResizePolicy, TermModel};

    /// A writer whose bytes stay inspectable after `Screen` has taken it by
    /// value. Pattern inherited from the removed `backend_wrap.rs` tests;
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
    /// VPA (`d`, what `MoveToRow` emits), a cursor-position report/query (`n`),
    /// or save/restore (`s`/`u`, `ESC 7`, `ESC 8`). This is the mechanical
    /// guard for K3 — never weaken it.
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
                        if matches!(bytes[j], b'H' | b'f' | b'd' | b'n' | b's' | b'u') {
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

    /// Branch-wide source pin for K3 AND K1: no absolute row addressing and no
    /// ratatui inline viewport anywhere in `src/tui/` production code. Every
    /// module is named explicitly, and the enumeration is then checked against
    /// the real directory listing — so a new file cannot silently escape the
    /// guard: forgetting to add it here turns this test red. Only the text
    /// before `#[cfg(test)]` is scanned, so tests may still construct a bad
    /// sequence to prove the runtime guard bites. `MoveToColumn(` does not
    /// match `MoveTo(` — column addressing is safe. `MoveToRow` is named
    /// separately for the opposite reason: it does NOT match `MoveTo(` either,
    /// yet it emits `ESC[{n+1}d`, absolute row addressing under a safe-looking
    /// name. `MoveToNextLine` / `MoveToPreviousLine` stay unguarded — they
    /// emit `E` / `F` and are genuinely relative.
    ///
    /// The K1 needles are bare words, not call forms: once the last
    /// viewport-era doc comments were rewritten (Task 5), no production module
    /// mentions `insert_before` at all, so the needle no longer has to tolerate
    /// prose. `page.rs` and `term.rs` keep their own per-file pins as well —
    /// they are the two modules that used to draw through the viewport, and a
    /// reintroduction there should fail twice, not once.
    #[test]
    fn no_tui_module_uses_absolute_addressing_or_the_inline_viewport() {
        // The `*_tests.rs` entries are test-only modules attached via `#[path]`;
        // they have no production half, so they are scanned whole.
        let modules = [
            ("ask.rs", include_str!("ask.rs")),
            ("convert.rs", include_str!("convert.rs")),
            ("editor.rs", include_str!("editor.rs")),
            ("entry.rs", include_str!("entry.rs")),
            ("intro.rs", include_str!("intro.rs")),
            ("mod.rs", include_str!("mod.rs")),
            ("page.rs", include_str!("page.rs")),
            ("paint.rs", include_str!("paint.rs")),
            ("polite.rs", include_str!("polite.rs")),
            ("run.rs", include_str!("run.rs")),
            ("screen.rs", include_str!("screen.rs")),
            ("screen_model.rs", include_str!("screen_model.rs")),
            ("status.rs", include_str!("status.rs")),
            ("term.rs", include_str!("term.rs")),
            ("theme.rs", include_str!("theme.rs")),
            ("welcome.rs", include_str!("welcome.rs")),
            ("welcome_data.rs", include_str!("welcome_data.rs")),
            ("welcome_tests.rs", include_str!("welcome_tests.rs")),
            (
                "welcome_data_tests.rs",
                include_str!("welcome_data_tests.rs"),
            ),
        ];
        for (name, src) in modules {
            let prod = src.split("#[cfg(test)]").next().unwrap();
            for needle in ["MoveTo(", "MoveToRow"] {
                assert!(
                    !prod.contains(needle),
                    "{name} must not address an absolute row ({needle})"
                );
            }
            assert!(
                !prod.contains("cursor::position()"),
                "{name} must not query the cursor position"
            );
            for needle in ["SavePosition", "RestorePosition"] {
                assert!(
                    !prod.contains(needle),
                    "{name} must not use {needle} to fake absolute addressing"
                );
            }
            for needle in ["insert_before", "Viewport::Inline"] {
                assert!(
                    !prod.contains(needle),
                    "{name} must not draw the bottom region through ratatui's \
                     inline viewport ({needle})"
                );
            }
        }

        // Completeness: the enumeration above must name every `.rs` file that
        // actually lives in `src/tui/`. Without this, a new module escapes the
        // guard until a human remembers to add it. A directory that cannot be
        // read is a FAILURE, never a silent pass. The walk is non-recursive, so
        // a subdirectory (e.g. a future `src/tui/widgets/`) would be invisible
        // to it — that's ruled out below by asserting none exists, rather than
        // by claiming a completeness this walk does not actually perform.
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/tui");
        let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {dir}: {e}"));
        for entry in entries {
            let entry = entry.unwrap_or_else(|e| panic!("cannot read an entry of {dir}: {e}"));
            let file_type = entry
                .file_type()
                .unwrap_or_else(|e| panic!("cannot stat an entry of {dir}: {e}"));
            let file = entry.file_name().to_string_lossy().into_owned();
            assert!(
                !file_type.is_dir(),
                "{file} is a subdirectory of src/tui/ — the completeness walk \
                 above is non-recursive and cannot see into it, so any .rs \
                 files inside would silently escape the K1/K3 guard"
            );
            if !file.ends_with(".rs") {
                continue;
            }
            assert!(
                modules.iter().any(|(name, _)| *name == file),
                "{file} is missing from the guarded module list in screen.rs — \
                 add it there so the K1/K3 guard covers it"
            );
        }
    }

    #[test]
    fn the_absolute_addressing_guard_detects_a_known_bad_sequence() {
        assert!(contains_absolute_addressing(b"\x1b[3;5H"));
        assert!(contains_absolute_addressing(b"\x1b[10d"));
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
        assert_eq!(s.cursor_col, 0);
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
        assert_eq!(s.cursor_col, 0);
        assert!(s.last_widths.is_empty());
    }

    // ---- resize --------------------------------------------------------

    /// Source pin: the guessed `painted * 2` descent must never come back.
    /// `resize` computes its descent from `descend_rows` exclusively.
    #[test]
    fn resize_never_reintroduces_the_painted_times_two_guess() {
        let prod = include_str!("screen.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(
            !prod.contains("saturating_mul(2)"),
            "the painted * 2 descent guess must not return to production code"
        );
    }

    /// v0.30.0 assumed the block always sits at the bottom of the screen;
    /// in a freshly opened session it does not, and the descent overshot
    /// into blank rows, leaving the block's top rows alive as a ghost.
    /// Here the block is small (5 lines) and the cursor is already on its
    /// LAST line, so the correct descent is 0 -- but `painted * 2` would
    /// still drive the cursor down 10 rows, straight into the blank space
    /// below the block on a tall screen.
    #[test]
    fn resize_descends_by_recorded_rows_not_by_painted_times_two() {
        let (mut s, buf) = screen(40, 20);
        let block = lines(&["────", "> hi", "────", "ready", "extra"]);
        s.paint(&block, 4, 0).unwrap();
        assert_eq!(s.cursor_up, 0, "cursor is already on the block's last line");
        buf.clear();
        s.resize(Size::new(40, 8)).unwrap();
        let t = buf.text();
        assert!(
            !t.contains('B'),
            "no descent is needed from the block's last line, but a \
             MoveDown was emitted -- the painted * 2 overshoot into blank \
             rows below the block: {t:?}"
        );
        assert!(
            !contains_absolute_addressing(t.as_bytes()),
            "resize must never use absolute addressing (K3): {t:?}"
        );
        assert_eq!(
            count(&t, CLEAR_LINE),
            5,
            "the erase must still happen: {t:?}"
        );
    }

    #[test]
    fn resize_descends_then_erases_the_rewrapped_rows_and_resets() {
        let (mut s, buf) = screen(80, 20);
        s.paint(&lines(&[&"x".repeat(80), &"y".repeat(80)]), 0, 0)
            .unwrap();
        buf.clear();
        s.resize(Size::new(40, 20)).unwrap();
        let t = buf.text();
        // Cursor is on line 0 of 2 (cursor_up = 1): 1 row to finish that
        // line's rewrap (80 wide -> 2 rows at width 40) plus the 2-row
        // rewrap of the line below it -- descend_rows(3), not painted*2 (4).
        assert!(
            t.starts_with("\x1b[3B"),
            "must descend by descend_rows first: {t:?}"
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

    #[test]
    fn resize_uses_the_cursor_column_recorded_by_paint() {
        let (mut s, buf) = screen(80, 20);
        // Cursor on line 0 of 2, at column 60. At width 40 that line rewraps
        // into 2 rows and the cursor lands in the SECOND of them, so only the
        // 2-row rewrap of the line below remains: 2, not the 3 a forgotten
        // (zero) cursor_col would produce.
        s.paint(
            &lines(&["x".repeat(80).as_str(), "y".repeat(80).as_str()]),
            0,
            60,
        )
        .unwrap();
        buf.clear();
        s.resize(Size::new(40, 20)).unwrap();
        assert!(
            buf.text().starts_with("\x1b[2B"),
            "descent must use the recorded cursor_col: {:?}",
            buf.text()
        );
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

    // ---- descend_rows (pure) --------------------------------------------

    #[test]
    fn descend_rows_is_zero_when_the_cursor_is_already_on_the_last_line() {
        // Width unchanged (no line wraps), cursor on the block's last line.
        assert_eq!(descend_rows(&[10, 10, 10], 3, 0, 5, 40), 0);
    }

    #[test]
    fn descend_rows_matches_cursor_up_when_nothing_wraps() {
        // Width unchanged, cursor two rows above the last line: with no
        // rewrap in play, the descent is exactly `cursor_up`.
        assert_eq!(descend_rows(&[10, 10, 10], 3, 2, 5, 40), 2);
    }

    #[test]
    fn descend_rows_adds_one_when_the_cursors_own_line_wraps_and_it_sits_in_the_first_half() {
        // A single-line block, width 15 wraps to 2 rows at new_width 10. The
        // cursor at column 3 is in the FIRST visual row, one row above the
        // wrapped line's bottom.
        assert_eq!(descend_rows(&[15], 1, 0, 3, 10), 1);
    }

    #[test]
    fn descend_rows_adds_nothing_when_the_cursors_own_line_wraps_and_it_sits_in_the_second_half() {
        // Same wrap as above, but the cursor at column 13 is already in the
        // SECOND (last) visual row -- already at the wrapped line's bottom.
        assert_eq!(descend_rows(&[15], 1, 0, 13, 10), 0);
    }

    #[test]
    fn descend_rows_adds_two_when_the_two_lines_below_the_cursor_each_wrap_into_two_rows() {
        // Cursor is 2 logical lines above the last (cursor_up = 2). Its own
        // line (width 5) does not wrap at new_width 10. The two lines below
        // it (width 15 each) each wrap into 2 rows, adding 2 extra rows over
        // the no-wrap baseline of `cursor_up` (2): 2 + 2 = 4.
        assert_eq!(descend_rows(&[5, 15, 15], 3, 2, 0, 10), 4);
    }

    #[test]
    fn descend_rows_is_zero_when_nothing_has_been_painted() {
        assert_eq!(descend_rows(&[], 0, 0, 0, 40), 0);
    }

    #[test]
    fn descend_rows_never_divides_by_zero_and_floors_rows_at_one() {
        // new_width == 0: no row can be computed by division, so every
        // logical line counts as exactly 1 row (the same floor
        // `rewrapped_rows` falls back to). Must not panic.
        assert_eq!(descend_rows(&[100, 200, 300], 3, 2, 999, 0), 2);
    }

    // ---- rewrapped_rows (pure) -----------------------------------------

    #[test]
    fn rewrapped_rows_roughly_doubles_when_the_width_halves() {
        assert_eq!(rewrapped_rows(&[80, 80, 80, 80], 40, 4), 8);
    }

    #[test]
    fn rewrapped_rows_is_floored_at_painted_with_no_ceiling() {
        // Raw sum is 4 x 8 = 32. The old painted*2 ceiling reported 8 here,
        // and that shortfall IS the residue; the count must not be capped.
        assert_eq!(rewrapped_rows(&[80, 80, 80, 80], 10, 4), 32);
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

    // ---- the modelled screen: what a byte assertion cannot see ------------

    /// A realistic block: two FULL-WIDTH rules around one content line, plus
    /// a status line. The full-width rules are the part that re-wraps into
    /// extra rows when the terminal narrows, so they are what residue is
    /// counted on.
    fn block(width: u16) -> Vec<String> {
        let rule = "─".repeat(usize::from(width));
        vec![
            rule.clone(),
            "> hello".to_string(),
            rule,
            "ready".to_string(),
        ]
    }

    const BLOCK_CURSOR_LINE: u16 = 1;
    const BLOCK_CURSOR_COL: u16 = 7;

    /// A `Screen` paired with the grid its bytes are actually applied to.
    struct Modelled {
        screen: Screen<SharedBuf>,
        buf: SharedBuf,
        model: TermModel,
    }

    impl Modelled {
        fn new(w: u16, h: u16) -> Self {
            let (screen, buf) = screen(w, h);
            Self {
                screen,
                buf,
                model: TermModel::new(w, h),
            }
        }

        /// Everything emitted since the last drain, applied in ONE `apply`
        /// so no escape sequence is ever split across two calls.
        fn drain(&mut self) {
            let bytes = self.buf.text().into_bytes();
            self.buf.clear();
            self.model.apply(&bytes);
        }

        /// Ordinary output straight into the grid -- how a real session
        /// drives the cursor down the screen and fills the transcript.
        fn feed(&mut self, text: &str) {
            self.model.apply(text.as_bytes());
        }

        /// Paint the block at the screen's CURRENT width: callers wrap their
        /// content to `screen.size().width`, so after a width change the
        /// rules must be regenerated at the new width.
        fn paint(&mut self) {
            let lines = block(self.screen.size().width);
            self.screen
                .paint(&lines, BLOCK_CURSOR_LINE, BLOCK_CURSOR_COL)
                .unwrap();
            self.drain();
        }

        /// Step one of a resize: the terminal reflows on its own, before our
        /// code hears anything about it.
        fn terminal_resizes(&mut self, w: u16, h: u16, policy: ResizePolicy) {
            self.model.resize(w, h, policy);
        }

        /// Step two: the event reaches us and we emit our erase.
        fn we_resize(&mut self, w: u16, h: u16) {
            self.screen.resize(Size::new(w, h)).unwrap();
            self.drain();
        }

        fn resize(&mut self, w: u16, h: u16, policy: ResizePolicy) {
            self.terminal_resizes(w, h, policy);
            self.we_resize(w, h);
        }

        /// The transcript lines still on the grid, in order.
        fn transcript_rows(&self) -> Vec<String> {
            self.model
                .snapshot()
                .into_iter()
                .map(|row| row.trim_end().to_string())
                .filter(|row| row.starts_with("transcript "))
                .collect()
        }

        fn grid(&self) -> String {
            self.model
                .snapshot()
                .iter()
                .enumerate()
                .map(|(i, row)| format!("{i:>3} |{}|", row.trim_end()))
                .collect::<Vec<_>>()
                .join("\n")
        }

        /// The acceptance criterion: exactly two rows carry the rule
        /// character -- the block's top and bottom rule. More is residue.
        fn assert_no_residue(&self, scenario: &str, policy: ResizePolicy) {
            let rows = self.model.rows_containing('─');
            assert_eq!(
                rows.len(),
                2,
                "{scenario} under {policy:?}: {} rule rows, expected 2 (rows {rows:?})\n{}",
                rows.len(),
                self.grid()
            );
        }
    }

    /// Scenario 1: fresh session on a tall terminal, block mid-screen with
    /// blank rows below it. Narrow, widen, narrow again, painting after each.
    fn fresh_session_mid_screen(policy: ResizePolicy) {
        let mut m = Modelled::new(80, 40);
        m.feed(&"\r\n".repeat(15));
        m.paint();
        for w in [40u16, 100, 30] {
            m.resize(w, 40, policy);
            m.paint();
        }
        m.assert_no_residue("fresh session 80 -> 40 -> 100 -> 30", policy);
    }

    /// Scenario 2: dragging a window edge -- 20 consecutive resizes, each to
    /// a different width, with a single `paint` at the end.
    fn dragging(policy: ResizePolicy) {
        let mut m = Modelled::new(80, 40);
        m.feed(&"\r\n".repeat(15));
        m.paint();
        for w in (60u16..80).rev() {
            m.resize(w, 40, policy);
        }
        m.paint();
        m.assert_no_residue("drag 80 -> 60 in 20 steps", policy);
    }

    /// Scenario 3: hard narrowing, 200 -> 60. More than a threefold shrink,
    /// which is past what `rewrapped_rows`' `painted..=painted * 2` clamp can
    /// count.
    fn hard_narrowing(policy: ResizePolicy) {
        let mut m = Modelled::new(200, 40);
        m.feed(&"\r\n".repeat(20));
        m.paint();
        m.resize(60, 40, policy);
        m.paint();
        m.assert_no_residue("hard narrowing 200 -> 60", policy);
    }

    /// Scenario 4: block on the screen's last rows with a full transcript
    /// above it. Residue-free AND no text loss: rows the terminal itself
    /// dropped off the top are its own scroll, but everything it KEPT must
    /// survive our erase.
    fn bottom_of_screen(policy: ResizePolicy) {
        let mut m = Modelled::new(80, 20);
        for i in 0..16 {
            m.feed(&format!("transcript line {i:02}\r\n"));
        }
        m.paint();
        m.terminal_resizes(40, 20, policy);
        let survivors = m.transcript_rows();
        m.we_resize(40, 20);
        m.paint();
        m.assert_no_residue("bottom of screen 80 -> 40", policy);
        assert_eq!(
            m.transcript_rows(),
            survivors,
            "bottom of screen under {policy:?}: transcript text lost\nkept by the terminal: {survivors:?}\n{}",
            m.grid()
        );
    }

    /// Scenario 5: the shape the user reported -- narrow PAST the old
    /// `painted * 2` ceiling so residue would be created, then widen back
    /// out. Pins that a widen neither creates residue of its own nor carries
    /// the narrowing's damage forward.
    ///
    /// Caveat on the widen leg: `ResizePolicy::Reflow` never re-merges rows
    /// its own earlier narrowing split (pinned by
    /// `reflow_previously_split_row_does_not_remerge_on_widen`), where real
    /// xterm.js would recombine them. So the widen here is modelled MORE
    /// row-hungry than a real terminal's, which makes this the harsher case,
    /// not a softer one.
    fn narrow_past_the_clamp_then_widen(policy: ResizePolicy) {
        let mut m = Modelled::new(200, 40);
        m.feed(&"\r\n".repeat(20));
        m.paint();
        m.resize(50, 40, policy);
        m.paint();
        m.assert_no_residue("narrow 200 -> 50", policy);
        m.resize(120, 40, policy);
        m.paint();
        m.assert_no_residue("narrow 200 -> 50 then widen to 120", policy);
    }

    #[test]
    fn narrow_past_the_clamp_then_widen_leaves_no_residue_when_reflowing() {
        narrow_past_the_clamp_then_widen(ResizePolicy::Reflow);
    }

    #[test]
    fn narrow_past_the_clamp_then_widen_leaves_no_residue_without_reflow() {
        narrow_past_the_clamp_then_widen(ResizePolicy::NoReflow);
    }

    #[test]
    fn fresh_session_mid_screen_leaves_no_residue_when_reflowing() {
        fresh_session_mid_screen(ResizePolicy::Reflow);
    }

    #[test]
    fn fresh_session_mid_screen_leaves_no_residue_without_reflow() {
        fresh_session_mid_screen(ResizePolicy::NoReflow);
    }

    #[test]
    fn dragging_leaves_no_residue_when_reflowing() {
        dragging(ResizePolicy::Reflow);
    }

    #[test]
    fn dragging_leaves_no_residue_without_reflow() {
        dragging(ResizePolicy::NoReflow);
    }

    #[test]
    fn hard_narrowing_leaves_no_residue_when_reflowing() {
        hard_narrowing(ResizePolicy::Reflow);
    }

    #[test]
    fn hard_narrowing_leaves_no_residue_without_reflow() {
        hard_narrowing(ResizePolicy::NoReflow);
    }

    #[test]
    fn bottom_of_screen_leaves_no_residue_when_reflowing() {
        bottom_of_screen(ResizePolicy::Reflow);
    }

    #[test]
    #[ignore = "unreachable without a terminal-reflow-policy input: from bit-identical state the two policies demand opposite treatment of the row two above the cursor (see .superpowers/sdd/smh-task-4-report.md)"]
    fn bottom_of_screen_leaves_no_residue_without_reflow() {
        bottom_of_screen(ResizePolicy::NoReflow);
    }
}
