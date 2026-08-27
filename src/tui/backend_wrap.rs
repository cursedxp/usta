//! Cursor-tracking wrapper around `CrosstermBackend`.
//!
//! ratatui's inline viewport asks the backend where the cursor is
//! (`Terminal::clear()` and `compute_inline_size()`), and `CrosstermBackend`
//! answers by writing a CPR query (`ESC[6n`) to the terminal and reading the
//! reply from stdin. While a crossterm `EventStream` is alive it holds the
//! global stdin reader lock, the reply is never read in time, and the query
//! fails fatally with "The cursor position could not be read within a normal
//! duration".
//!
//! `TrackedBackend` tracks the cursor itself and answers those queries from
//! tracked state, so no CPR query is ever issued while the app is running.

use std::io::{self, Write};

// usta owns a `pub enum Backend` (`src/backend.rs`) — the ratatui trait comes in
// under an alias so the two never collide.
use ratatui::backend::{Backend as RatatuiBackend, ClearType, CrosstermBackend, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};

/// A `CrosstermBackend` that remembers where the cursor is instead of asking.
pub(crate) struct TrackedBackend<W: Write> {
    inner: CrosstermBackend<W>,
    pos: Position,
}

impl<W: Write> TrackedBackend<W> {
    /// `seed` is where the cursor is believed to be right now — it must be
    /// obtained before any `EventStream` exists, because afterwards it cannot
    /// be asked for at all.
    pub(crate) fn new(inner: CrosstermBackend<W>, seed: Position) -> Self {
        Self { inner, pos: seed }
    }
}

/// Where `append_lines(n)` leaves the cursor. `CrosstermBackend` emits `n` bare
/// LF bytes: the row advances and stops at the last row (the terminal scrolls
/// instead of moving past the bottom), and with `OPOST` cleared by raw mode
/// there is no NL→CRNL translation, so the column does not move.
/// Deliberately *not* `TestBackend`'s `x + 1` model, which describes a different
/// backend.
fn advanced_by_lines(pos: Position, n: u16, term_height: u16) -> Position {
    Position {
        x: pos.x,
        y: pos.y.saturating_add(n).min(term_height.saturating_sub(1)),
    }
}

/// The seed to use when the real cursor position cannot be read. Bottom-left:
/// a CLI normally starts at the bottom of a filled screen, and `compute_inline_size`
/// then reproduces exactly the scroll a bottom-anchored terminal performs. Guessing
/// the top instead would paint the viewport over live scrollback.
pub(crate) fn fallback_seed(term_height: u16) -> Position {
    Position {
        x: 0,
        y: term_height.saturating_sub(1),
    }
}

impl<W: Write> RatatuiBackend for TrackedBackend<W> {
    type Error = io::Error;

    /// Ends on the last painted cell — the same cursor model ratatui-core keeps
    /// internally. An empty diff paints nothing and moves nothing.
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let mut last = None;
        self.inner.draw(content.inspect(|(x, y, _)| {
            last = Some(Position { x: *x, y: *y });
        }))?;
        if let Some(p) = last {
            self.pos = p;
        }
        Ok(())
    }

    fn append_lines(&mut self, n: u16) -> io::Result<()> {
        self.inner.append_lines(n)?;
        let height = self.inner.size()?.height;
        self.pos = advanced_by_lines(self.pos, n, height);
        Ok(())
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.inner.show_cursor()
    }

    /// The whole point: answered from tracked state, never from the terminal.
    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(self.pos)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        let position = position.into();
        self.inner.set_cursor_position(position)?;
        self.pos = position;
        Ok(())
    }

    fn clear(&mut self) -> io::Result<()> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> io::Result<Size> {
        self.inner.size()
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> io::Result<()> {
        // `CrosstermBackend` also implements `Write`; name the trait explicitly.
        RatatuiBackend::flush(&mut self.inner)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    /// A writer whose bytes stay inspectable after the backend has consumed it.
    /// `CrosstermBackend::new` takes the writer by value and this crate cannot
    /// borrow it back (`CrosstermBackend::writer()` is behind an unstable
    /// feature), so the buffer is shared instead.
    #[derive(Clone, Default)]
    struct SharedBuf(Rc<RefCell<Vec<u8>>>);

    impl SharedBuf {
        fn bytes(&self) -> Vec<u8> {
            self.0.borrow().clone()
        }
    }

    impl Write for SharedBuf {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// A backend over a fake writer — no TTY, no global terminal state.
    fn backend(seed: Position) -> (TrackedBackend<SharedBuf>, SharedBuf) {
        let buf = SharedBuf::default();
        let tb = TrackedBackend::new(CrosstermBackend::new(buf.clone()), seed);
        (tb, buf)
    }

    /// The byte sequence `CrosstermBackend::get_cursor_position` would emit.
    const CPR_QUERY: &[u8] = b"\x1b[6n";

    fn contains_cpr(bytes: &[u8]) -> bool {
        bytes.windows(CPR_QUERY.len()).any(|w| w == CPR_QUERY)
    }

    #[test]
    fn seed_is_returned_by_get_cursor_position() {
        let (mut tb, _buf) = backend(Position { x: 3, y: 7 });
        assert_eq!(tb.get_cursor_position().unwrap(), Position { x: 3, y: 7 });
    }

    #[test]
    fn get_cursor_position_never_writes_a_cpr_query() {
        let (mut tb, buf) = backend(Position { x: 3, y: 7 });
        for _ in 0..10 {
            assert_eq!(tb.get_cursor_position().unwrap(), Position { x: 3, y: 7 });
        }
        assert!(
            buf.bytes().is_empty(),
            "the query must not reach the writer"
        );
    }

    #[test]
    fn set_cursor_position_is_tracked_exactly() {
        let (mut tb, buf) = backend(Position { x: 0, y: 0 });
        tb.set_cursor_position(Position { x: 11, y: 4 }).unwrap();
        assert_eq!(tb.get_cursor_position().unwrap(), Position { x: 11, y: 4 });
        // The move is really forwarded: CSI 5;12 H (1-based row;col).
        assert!(
            String::from_utf8_lossy(&buf.bytes()).contains("[5;12H"),
            "expected a MoveTo on the wire, got {:?}",
            String::from_utf8_lossy(&buf.bytes())
        );
    }

    #[test]
    fn draw_tracks_the_last_yielded_cell() {
        let (mut tb, _buf) = backend(Position { x: 0, y: 0 });
        let cells = [Cell::new("a"), Cell::new("b"), Cell::new("c")];
        let content = vec![
            (1u16, 2u16, &cells[0]),
            (5u16, 2u16, &cells[1]),
            (7u16, 9u16, &cells[2]),
        ];
        tb.draw(content.into_iter()).unwrap();
        assert_eq!(tb.get_cursor_position().unwrap(), Position { x: 7, y: 9 });
    }

    #[test]
    fn draw_with_empty_content_leaves_the_position_unchanged() {
        let (mut tb, _buf) = backend(Position { x: 4, y: 6 });
        tb.draw(std::iter::empty::<(u16, u16, &Cell)>()).unwrap();
        assert_eq!(tb.get_cursor_position().unwrap(), Position { x: 4, y: 6 });
    }

    #[test]
    fn append_lines_advances_the_row_and_keeps_the_column() {
        assert_eq!(
            advanced_by_lines(Position { x: 5, y: 2 }, 3, 24),
            Position { x: 5, y: 5 }
        );
    }

    #[test]
    fn append_lines_clamps_at_the_last_row() {
        assert_eq!(
            advanced_by_lines(Position { x: 5, y: 20 }, 10, 24),
            Position { x: 5, y: 23 }
        );
        assert_eq!(
            advanced_by_lines(Position { x: 0, y: 23 }, 5, 24),
            Position { x: 0, y: 23 }
        );
        // u16 overflow and a zero-height terminal must not panic.
        assert_eq!(
            advanced_by_lines(Position { x: 1, y: u16::MAX }, 9, 24),
            Position { x: 1, y: 23 }
        );
        assert_eq!(
            advanced_by_lines(Position { x: 1, y: 5 }, 9, 0),
            Position { x: 1, y: 0 }
        );
    }

    #[test]
    fn append_lines_with_zero_is_a_noop() {
        assert_eq!(
            advanced_by_lines(Position { x: 5, y: 2 }, 0, 24),
            Position { x: 5, y: 2 }
        );
    }

    #[test]
    fn append_lines_through_the_backend_never_moves_the_column_backwards() {
        // The clamp height comes from `size()`, a real ioctl, so off a TTY it may
        // fail outright — but `append_lines` now forwards to the writer *before*
        // calling `size()`, so the newlines below must reach the wire regardless
        // of whether the overall call ends up `Ok` or `Err`. That write is the
        // part this test must not let pass vacuously: if the `append_lines`
        // override were deleted, the trait's provided default (`Ok(())`, a
        // silent no-op) would leave `buf` empty and fail the assertion below.
        let (mut tb, buf) = backend(Position { x: 2, y: 1 });
        let _ = tb.append_lines(3);
        assert_eq!(
            buf.bytes(),
            b"\n\n\n".to_vec(),
            "append_lines(3) must emit exactly three bare LF bytes to the writer"
        );
        let after = tb.get_cursor_position().unwrap();
        assert_eq!(after.x, 2, "append_lines emits bare LFs, never a CR");
        assert!(after.y >= 1, "the row only ever advances or clamps");
    }

    #[test]
    fn clear_and_clear_region_preserve_the_tracked_position() {
        let (mut tb, _buf) = backend(Position { x: 9, y: 9 });
        tb.clear().unwrap();
        assert_eq!(tb.get_cursor_position().unwrap(), Position { x: 9, y: 9 });
        tb.clear_region(ClearType::AfterCursor).unwrap();
        assert_eq!(tb.get_cursor_position().unwrap(), Position { x: 9, y: 9 });
    }

    #[test]
    fn hide_show_and_flush_preserve_the_tracked_position() {
        let (mut tb, _buf) = backend(Position { x: 9, y: 9 });
        tb.hide_cursor().unwrap();
        tb.show_cursor().unwrap();
        tb.flush().unwrap();
        assert_eq!(tb.get_cursor_position().unwrap(), Position { x: 9, y: 9 });
    }

    #[test]
    fn seed_fallback_anchors_to_the_bottom_row() {
        assert_eq!(fallback_seed(24), Position { x: 0, y: 23 });
        assert_eq!(fallback_seed(0), Position { x: 0, y: 0 });
    }

    #[test]
    fn no_backend_operation_ever_emits_a_cpr_query() {
        let (mut tb, buf) = backend(Position { x: 1, y: 1 });
        let cells = [Cell::new("x")];
        tb.draw(vec![(0u16, 0u16, &cells[0])].into_iter()).unwrap();
        tb.set_cursor_position(Position { x: 2, y: 3 }).unwrap();
        tb.hide_cursor().unwrap();
        tb.show_cursor().unwrap();
        tb.clear().unwrap();
        tb.clear_region(ClearType::UntilNewLine).unwrap();
        let _ = tb.append_lines(2);
        tb.flush().unwrap();
        tb.get_cursor_position().unwrap();
        assert!(
            !contains_cpr(&buf.bytes()),
            "a cursor-position query reached the terminal"
        );
    }
}
