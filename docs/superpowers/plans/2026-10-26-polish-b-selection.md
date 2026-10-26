# Polish B (in-app selection) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring back drag-to-select inside the chat pane (opencode-style): drag with the left button, the cells highlight, and releasing copies the text to the clipboard and shows "copied" on the editor's rule row.

**Architecture:** The selection lives in flat chat-line coordinates `(flat line index, display column)` on `ChatBuffer`, so scrolling keeps it anchored. A pure `tui/select.rs` module maps mouse cells, extracts text (soft-wrapped rows join with "", hard rows with "\n"), paints the highlight and talks to the clipboard. `page::nav` routes Down/Drag/Up; `render_screen` paints the highlight and the flash.

**Tech Stack:** Rust 2021, ratatui (`Buffer`, `Rect`, `Modifier::REVERSED`, `TestBackend`), crossterm mouse events (button-event mode is already on since v0.36.0), `unicode-width`, `arboard`, `base64`.

Spec: `docs/superpowers/specs/2026-10-26-polish-design.md` section B.

## Global Constraints

- Production code per file (everything above the first `#[cfg(test)]` that starts the test module) stays at or under 600 lines (SPEC §11).
- `rustfmt --edition 2021 <edited files>` only; `cargo fmt --check` stays clean.
- `cargo clippy --all-targets -- -D warnings` clean.
- `cargo test`: only allowed failure is `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt`. Bin-only crate: `cargo test <filter>`.
- NO emoji anywhere in UI text. The flash text is exactly `copied`.
- Tests must never touch the real system clipboard or write OSC 52 to the test process's stdout: clipboard access goes through an injected copier in `nav`.
- Not in scope: double-click word select, keyboard copy, selection inside the editor (tui-textarea already has it).
- Known limitation (accepted in spec C2: Reply/Echo wrapping is a no-op): only notice rows (`Source::Text`) carry soft-wrap flags. Assistant replies and user echoes arrive pre-wrapped by their renderers, so copying them keeps a newline at each visual wrap, and echo rows include their 2-column prefix. Document it in SPEC; do not widen scope.
- Surgical changes; commits add files by name; messages end with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- Paths are relative to the worktree root `/Users/optimotion/Documents/Work/Projects/headspace/usta-polish-b`.

---

### Task 1: Pure selection module `tui/select.rs`

**Files:**
- Create: `src/tui/select.rs`
- Modify: `src/tui/mod.rs` (`pub mod select;`, alphabetical)
- Commit also: `docs/superpowers/plans/2026-10-26-polish-b-selection.md` (untracked in the worktree)

**Interfaces:**
- Produces:

```rust
/// A drag selection in flat chat-line coordinates: (flat line index, display column).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sel {
    pub anchor: (usize, u16),
    pub head: (usize, u16),
}

/// Mouse cell -> flat position; `None` outside `area`. `top` = flat index of the pane's first row.
pub fn cell_to_pos(area: Rect, top: usize, x: u16, y: u16) -> Option<(usize, u16)>;
/// Like `cell_to_pos`, but a cell outside `area` is clamped onto its nearest edge row/column.
pub fn clamp_pos(area: Rect, top: usize, x: u16, y: u16) -> (usize, u16);
/// (start, end) in reading order, both inclusive.
pub fn ordered(sel: &Sel) -> ((usize, u16), (usize, u16));
/// Selected text. A char is included when its starting display column is inside the
/// row's selected range (wide chars never split). Hard rows join with '\n', soft
/// (continuation) rows with "" because the wrap keeps the break space on the upper row.
/// Trailing spaces are trimmed only at the end of a logical line (before a hard break
/// and at the very end), so a soft break's space survives.
pub fn extract(lines: &[Line<'_>], soft: &[bool], sel: &Sel) -> String;
/// Add `Modifier::REVERSED` to the selected cells that are visible in `area`.
pub fn highlight(buf: &mut Buffer, area: Rect, top: usize, sel: &Sel);
/// OSC 52 clipboard sequence for `text`.
pub fn osc52(text: &str) -> String;
/// Put `text` on the system clipboard (arboard, handle kept alive in a OnceLock);
/// when that fails or `SSH_TTY` is set, also emit OSC 52 on stdout.
pub fn copy(text: &str);
```

- [ ] **Step 1: Write the module with tests first** (`src/tui/select.rs` test module):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Line;

    const AREA: Rect = Rect { x: 2, y: 1, width: 10, height: 4 };

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
        let s = Sel { anchor: (5, 3), head: (2, 7) };
        assert_eq!(ordered(&s), ((2, 7), (5, 3)));
        let s = Sel { anchor: (4, 8), head: (4, 1) };
        assert_eq!(ordered(&s), ((4, 1), (4, 8)));
    }

    #[test]
    fn extract_joins_soft_rows_without_a_newline() {
        let lines = [Line::raw("the quick "), Line::raw("brown fox")];
        let s = Sel { anchor: (0, 0), head: (1, 8) };
        assert_eq!(extract(&lines, &[false, true], &s), "the quick brown fox");
    }

    #[test]
    fn extract_trims_line_ends_and_joins_hard_rows_with_newlines() {
        let lines = [Line::raw("abc   "), Line::raw("def  ")];
        let s = Sel { anchor: (0, 0), head: (1, 9) };
        assert_eq!(extract(&lines, &[false, false], &s), "abc\ndef");
    }

    #[test]
    fn extract_takes_partial_first_and_last_rows() {
        let lines = [Line::raw("hello world"), Line::raw("second row")];
        let s = Sel { anchor: (1, 5), head: (0, 6) }; // reverse drag
        assert_eq!(extract(&lines, &[false, false], &s), "world\nsecond");
    }

    #[test]
    fn extract_never_splits_wide_chars() {
        let lines = [Line::raw("a日b")];
        // cols: a=0, 日=1..=2, b=3. Selecting 0..=1 includes 日 (starts at col 1).
        assert_eq!(extract(&lines, &[false], &Sel { anchor: (0, 0), head: (0, 1) }), "a日");
        // Starting on 日's right half (col 2) excludes it.
        assert_eq!(extract(&lines, &[false], &Sel { anchor: (0, 2), head: (0, 3) }), "b");
    }

    #[test]
    fn extract_ignores_rows_past_the_end() {
        let lines = [Line::raw("only")];
        assert_eq!(extract(&lines, &[false], &Sel { anchor: (0, 0), head: (5, 3) }), "only");
    }

    #[test]
    fn highlight_reverses_only_the_selected_visible_cells() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 6));
        // pane rows y=1..=4 show flat lines 30..=33; select (31,3)..=(32,1)
        highlight(&mut buf, AREA, 30, &Sel { anchor: (32, 1), head: (31, 3) });
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
```

- [ ] **Step 2: Run** `cargo test tui::select` — compile failure (module missing).

- [ ] **Step 3: Implement**

```rust
//! In-app chat selection (spec B): pure helpers for mapping mouse cells to flat chat
//! positions, extracting the selected text, painting the highlight, plus the clipboard.

use std::sync::{Mutex, OnceLock};

use base64::Engine;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::Line;
use unicode_width::UnicodeWidthChar;

// (Sel struct as in Interfaces)

pub fn cell_to_pos(area: Rect, top: usize, x: u16, y: u16) -> Option<(usize, u16)> {
    let inside = x >= area.x && x < area.x + area.width && y >= area.y && y < area.y + area.height;
    inside.then(|| (top + (y - area.y) as usize, x - area.x))
}

pub fn clamp_pos(area: Rect, top: usize, x: u16, y: u16) -> (usize, u16) {
    let cx = x.clamp(area.x, area.x + area.width.saturating_sub(1));
    let cy = y.clamp(area.y, area.y + area.height.saturating_sub(1));
    (top + (cy - area.y) as usize, cx - area.x)
}

pub fn ordered(sel: &Sel) -> ((usize, u16), (usize, u16)) {
    if sel.anchor <= sel.head { (sel.anchor, sel.head) } else { (sel.head, sel.anchor) }
}

/// Columns of row `row` covered by the selection, inclusive (`u16::MAX` = to the end).
fn row_range(row: usize, start: (usize, u16), end: (usize, u16)) -> (u16, u16) {
    let from = if row == start.0 { start.1 } else { 0 };
    let to = if row == end.0 { end.1 } else { u16::MAX };
    (from, to)
}

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
            let cell = &mut buf[(area.x + dx, area.y + dy)];
            cell.modifier.insert(Modifier::REVERSED);
        }
    }
}

pub fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64::engine::general_purpose::STANDARD.encode(text))
}

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
```

(ratatui 0.30 / ratatui-core 0.1: `Cell::modifier` is a public field, so `cell.modifier.insert(..)` and `buf[(x, y)].modifier.contains(..)` work.) `copy` is not unit-tested (it would touch the real clipboard); only `osc52` is.

- [ ] **Step 4:** `cargo test tui::select` PASS; full `cargo test`; clippy. In a bin crate the dead_code lint fires on items nothing calls yet: put `#[allow(dead_code)] // wired in by page::nav (Task 3)` on exactly the items clippy flags (expected: `copy`, maybe `highlight`/`cell_to_pos`/`clamp_pos`/`extract`). Task 3 removes every one of these allows. Then rustfmt `src/tui/select.rs src/tui/mod.rs`.

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/plans/2026-10-26-polish-b-selection.md src/tui/select.rs src/tui/mod.rs
git commit -m "feat(tui): pure chat-selection helpers (map, extract, highlight, copy)

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Selection state on `ChatBuffer`

**Files:**
- Modify: `src/tui/chat.rs`

**Interfaces:**
- Consumes: `select::{Sel, cell_to_pos, clamp_pos, extract}`; existing `flat(width)`, `cache: Option<(u16, Vec<Line>, Vec<bool>)>`, `trim()`, `scroll_up/scroll_down`, `width`, `pane_h`, `offset`.
- Produces on `ChatBuffer`:

```rust
/// `visible` for a pane at `area` (records it for mouse mapping).
pub fn visible_in(&mut self, area: Rect) -> Vec<Line<'static>>;
/// Flat index of the pane's first visible row at the current offset.
pub fn view_top(&mut self) -> usize;
pub fn mouse_down(&mut self, x: u16, y: u16);
pub fn mouse_drag(&mut self, x: u16, y: u16);
/// Finish the drag: `Some(text)` when something was selected (flash set to "copied").
pub fn mouse_up(&mut self, x: u16, y: u16) -> Option<String>;
pub fn selection(&self) -> Option<Sel>;
pub fn flash(&self) -> Option<&'static str>;
pub fn clear_flash(&mut self);
pub fn clear_selection(&mut self);
```

New fields: `area: Rect` (default `Rect::default()`), `sel: Option<Sel>`, `flash: Option<&'static str>`. `pub const COPIED: &str = "copied";`.

Deviation from the spec text, on purpose: the spec changes `visible(width, height)` to `visible(area: Rect)`. To keep ~30 existing `visible(80, 5)` test call sites untouched, this plan adds `visible_in(area)` (records `area`, then calls `visible(area.width, area.height)`) and `render_screen` switches to it. Same behavior, smaller diff.

- [ ] **Step 1: Failing tests (chat.rs tests)**

```rust
    fn pane(c: &mut ChatBuffer) -> Rect {
        let a = Rect::new(0, 0, 80, 5);
        c.visible_in(a);
        a
    }

    #[test]
    fn drag_selects_and_release_returns_the_text_and_flashes() {
        let mut c = filled(20); // l0..l19, pane shows l15..l19 on rows 0..=4
        pane(&mut c);
        c.mouse_down(0, 1);
        c.mouse_drag(2, 2);
        assert_eq!(c.selection().unwrap().head, (17, 2));
        assert_eq!(c.mouse_up(2, 2).as_deref(), Some("l16\nl17"));
        assert!(c.selection().is_none());
        assert_eq!(c.flash(), Some(COPIED));
    }

    #[test]
    fn a_plain_click_copies_nothing() {
        let mut c = filled(20);
        pane(&mut c);
        c.mouse_down(10, 0); // past the end of "l15"
        assert_eq!(c.mouse_up(10, 0), None);
        assert!(c.selection().is_none() && c.flash().is_none());
    }

    #[test]
    fn mouse_down_outside_the_pane_starts_nothing_and_clears_flash() {
        let mut c = filled(20);
        pane(&mut c);
        c.mouse_down(0, 0);
        c.mouse_drag(2, 0);
        c.mouse_up(2, 0);
        assert!(c.flash().is_some());
        c.mouse_down(0, 9); // below the pane
        assert!(c.selection().is_none() && c.flash().is_none());
    }

    #[test]
    fn dragging_past_the_pane_edges_scrolls() {
        let mut c = filled(40);
        pane(&mut c);
        c.mouse_down(0, 2);
        c.mouse_drag(0, 7); // below: already at the bottom, stays following
        assert!(c.is_following());
        c.mouse_drag(0, 0);
        c.scroll_up(5);
        let off = c.offset;
        c.mouse_drag(0, 9); // below the pane -> scroll down one
        assert_eq!(c.offset, off - 1);
        // above the pane: y is unsigned, so "above" means a pane that doesn't start at 0
        let a = Rect::new(0, 3, 80, 5);
        c.visible_in(a);
        let off = c.offset;
        c.mouse_drag(0, 1);
        assert_eq!(c.offset, off + 1);
    }

    #[test]
    fn selection_survives_scrolling_but_not_trimming() {
        let mut c = filled(20);
        pane(&mut c);
        c.mouse_down(0, 0);
        c.mouse_drag(1, 0);
        c.scroll_up(3);
        assert_eq!(c.selection().unwrap().anchor, (15, 0), "absolute flat index");
        for i in 0..MAX_ENTRIES {
            c.push(Source::Text(Text::raw(format!("x{i}"))));
        }
        assert!(c.selection().is_none(), "trim invalidates flat indices");
    }
```

- [ ] **Step 2: Run** `cargo test tui::chat` — compile failure.

- [ ] **Step 3: Implement**

```rust
    pub fn visible_in(&mut self, area: Rect) -> Vec<Line<'static>> {
        self.area = area;
        self.visible(area.width, area.height)
    }

    pub fn view_top(&mut self) -> usize {
        let w = self.width;
        let total = self.flat(w).len();
        let end = total.saturating_sub(self.offset);
        end.saturating_sub(self.pane_h as usize)
    }

    pub fn mouse_down(&mut self, x: u16, y: u16) {
        self.flash = None;
        let top = self.view_top();
        self.sel = select::cell_to_pos(self.area, top, x, y).map(|p| Sel { anchor: p, head: p });
    }

    pub fn mouse_drag(&mut self, x: u16, y: u16) {
        if self.sel.is_none() {
            return;
        }
        // Auto-scroll: terminals repeat drag events while the mouse moves.
        if y < self.area.y {
            self.scroll_up(1);
        } else if y >= self.area.y + self.area.height {
            self.scroll_down(1);
        }
        let top = self.view_top();
        let head = select::clamp_pos(self.area, top, x, y);
        if let Some(s) = self.sel.as_mut() {
            s.head = head;
        }
    }

    pub fn mouse_up(&mut self, x: u16, y: u16) -> Option<String> {
        self.mouse_drag(x, y);
        let sel = self.sel.take()?;
        let w = self.width;
        self.flat(w);
        let (_, lines, soft) = self.cache.as_ref()?;
        let text = select::extract(lines, soft, &sel);
        if text.is_empty() {
            return None;
        }
        self.flash = Some(COPIED);
        Some(text)
    }
```

Plus the trivial getters/clearers. In `trim()` add `self.sel = None;` with the comment `// flat indices shift: a selection would point at other text`. Mind the `mouse_drag` early return so `mouse_up` without a prior down returns `None`. Keep `mouse_up`'s scroll side effect harmless: `mouse_drag` only scrolls when y is outside the pane.

The "plain click" test: clicking col 10 of "l15" selects (15,10)..(15,10); `extract` yields "" because no char starts at col 10 → `None`, no flash.

- [ ] **Step 4:** `cargo test tui::chat`, full `cargo test`, clippy (if `view_top`/getters are flagged dead until Task 3, add a temporary `#[allow(dead_code)] // wired in by page.rs (next task)` per item; Task 3 removes them), rustfmt.

- [ ] **Step 5: Commit** `git add src/tui/chat.rs` — `feat(tui): chat selection state (anchored flat coordinates, copied flash)`.

---

### Task 3: Wire selection into `page.rs` (events, highlight, flash, resize)

**Files:**
- Modify: `src/tui/page.rs`; remove the temporary `#[allow(dead_code)]` lines added in Tasks 1-2 (`src/tui/select.rs`, `src/tui/chat.rs`)

**Interfaces:**
- Consumes: `ChatBuffer::{visible_in, view_top, mouse_down, mouse_drag, mouse_up, selection, flash, clear_flash, clear_selection}`, `select::{highlight, copy}`, `rule_label(buf, box_area, label, style)` and `NEW_MESSAGE` (existing in page.rs).
- Produces: `fn nav(chat: &mut ChatBuffer, ev: &Event, copy: &mut dyn FnMut(&str)) -> bool` (private); `handle_nav(tui, ev)` passes `&mut crate::tui::select::copy`.

- [ ] **Step 1: Failing tests (page.rs tests).** Update existing `nav(&mut c, &...)` calls in tests to `nav(&mut c, &..., &mut |_| {})`. Change the `mouse(kind)` helper to `mouse_at(kind, column, row)` (keep `mouse(kind)` = `mouse_at(kind, 0, 0)`). Add:

```rust
    #[test]
    fn nav_drag_copies_on_release_and_flashes() {
        let mut c = filled_chat(20);
        c.visible_in(Rect::new(0, 0, 80, 5));
        let mut copied = Vec::new();
        let mut cp = |t: &str| copied.push(t.to_string());
        let left = MouseButton::Left;
        assert!(nav(&mut c, &mouse_at(MouseEventKind::Down(left), 0, 1), &mut cp));
        assert!(nav(&mut c, &mouse_at(MouseEventKind::Drag(left), 2, 2), &mut cp));
        assert!(nav(&mut c, &mouse_at(MouseEventKind::Up(left), 2, 2), &mut cp));
        assert_eq!(copied, vec!["l16\nl17".to_string()]);
        assert_eq!(c.flash(), Some(crate::tui::chat::COPIED));
        assert!(c.selection().is_none());
        // any key clears the flash and still passes non-nav keys through
        let key = Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(!nav(&mut c, &key, &mut cp));
        assert!(c.flash().is_none());
    }

    #[test]
    fn nav_drag_below_the_pane_scrolls_down() {
        let mut c = filled_chat(40);
        c.visible_in(Rect::new(0, 0, 80, 5));
        c.scroll_up(6);
        let before = c.offset();
        let left = MouseButton::Left;
        nav(&mut c, &mouse_at(MouseEventKind::Down(left), 0, 0), &mut |_| {});
        nav(&mut c, &mouse_at(MouseEventKind::Drag(left), 0, 9), &mut |_| {});
        assert_eq!(c.offset(), before - 1);
    }

    #[test]
    fn selection_renders_reversed_and_flash_takes_the_rule_slot() {
        let mut chat = filled_chat(60);
        let editor = InputBox::new();
        screen_rows(80, 24, &mut chat, &editor); // records the 80x21 pane
        chat.mouse_down(0, 19);
        chat.mouse_drag(2, 20);
        let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
        term.draw(|f| render_screen(f, &mut chat, &editor, Line::raw("status"))).unwrap();
        let b = term.backend().buffer();
        assert!(b[(0, 19)].modifier.contains(Modifier::REVERSED));
        assert!(b[(2, 20)].modifier.contains(Modifier::REVERSED));
        assert!(!b[(3, 20)].modifier.contains(Modifier::REVERSED));
        chat.mouse_up(2, 20);
        chat.scroll_up(3);
        chat.push(Source::Text(Text::raw("fresh"))); // unread too: flash wins the slot
        let rows = screen_rows(80, 24, &mut chat, &editor);
        assert!(rows[21].starts_with('─'));
        assert!(rows[21].trim_end().ends_with("copied"), "{:?}", rows[21]);
    }
```

Add `pub fn offset(&self) -> usize` to `ChatBuffer` only if tests can't read the private field from page.rs (they can't; add it as `#[cfg(test)] pub fn offset(&self) -> usize`). Use the same `modifier` access form Task 1 settled on.

- [ ] **Step 2: Run** `cargo test tui::page` — failures.

- [ ] **Step 3: Implement**

```rust
pub(crate) fn handle_nav(tui: &mut Tui, ev: &Event) -> bool {
    nav(&mut tui.chat, ev, &mut crate::tui::select::copy)
}

/// Chat navigation + selection shared by every event loop: wheel, PageUp/PageDown,
/// left-button drag to select (copied on release). Returns true when consumed.
fn nav(chat: &mut ChatBuffer, ev: &Event, copy: &mut dyn FnMut(&str)) -> bool {
    match ev {
        Event::Mouse(m) => {
            match m.kind {
                MouseEventKind::ScrollUp => chat.scroll_up(WHEEL_LINES),
                MouseEventKind::ScrollDown => chat.scroll_down(WHEEL_LINES),
                MouseEventKind::Down(MouseButton::Left) => chat.mouse_down(m.column, m.row),
                MouseEventKind::Drag(MouseButton::Left) => chat.mouse_drag(m.column, m.row),
                MouseEventKind::Up(MouseButton::Left) => {
                    if let Some(text) = chat.mouse_up(m.column, m.row) {
                        copy(&text);
                    }
                }
                _ => {}
            }
            true
        }
        Event::Key(k) => {
            chat.clear_flash();
            match k.code {
                KeyCode::PageUp => { chat.page_up(); true }
                KeyCode::PageDown => { chat.page_down(); true }
                _ => false,
            }
        }
        _ => false,
    }
}
```

Keep the existing doc comment's spirit (other mouse events swallowed). `handle_resize`: add `tui.chat.clear_selection();` before `autoresize` (flat indices change with the width). `render_screen`:

```rust
    let lines = chat.visible_in(chat_area);
    f.render_widget(Paragraph::new(lines), chat_area);
    if let Some(sel) = chat.selection() {
        let top = chat.view_top();
        crate::tui::select::highlight(f.buffer_mut(), chat_area, top, &sel);
    }
    editor.render(f, box_area);
    if let Some(label) = chat.flash() {
        rule_label(f.buffer_mut(), box_area, label, theme::brand());
    } else if chat.unread() {
        rule_label(f.buffer_mut(), box_area, NEW_MESSAGE, theme::brand());
    }
```

(The existing `if chat.unread()` block becomes the `else if`.) Remove the temporary `#[allow(dead_code)]` lines from Tasks 1-2. Check that the `mouse_and_page_keys_are_handled_in_every_event_loop` pin still passes (call counts unchanged).

- [ ] **Step 4:** full `cargo test`, clippy, rustfmt `src/tui/page.rs src/tui/chat.rs src/tui/select.rs`.

- [ ] **Step 5: Commit** `git add src/tui/page.rs src/tui/chat.rs src/tui/select.rs` — `feat(tui): drag-select in the chat pane copies on release`.

---

### Task 4: Version v0.37.0, README, SPEC

**Files:** `Cargo.toml`, `Cargo.lock`, `src/tui/welcome_tests.rs` (`version_aligned_with_spec`), `README.md`, `SPEC.md`

- [ ] **Step 1:** `0.36.0` → `0.37.0` in Cargo.toml and the version pin; `cargo build`.
- [ ] **Step 2: README.md** — line 36: replace `Shift+drag to select and copy` with `drag to select and copy chat text`; line 112: replace `hold Shift while dragging to select and copy text (iTerm2/Ghostty/WezTerm)` with `drag with the mouse to select chat text — it is copied when you release (over SSH via OSC 52); Shift+drag still selects natively`.
- [ ] **Step 3: SPEC.md** — after the `**v0.36.0 (fullscreen polish):**` paragraph:

```markdown
**v0.37.0 (in-app selection):** drag with the left button in the chat pane to select; the cells show reversed, and releasing copies the text (arboard, handle kept for the process; OSC 52 on stdout when arboard fails or over SSH) and shows `copied` on the editor's rule row (it takes the "↓ new message" slot; cleared by the next key or click). A plain click copies nothing. The selection lives in flat chat-line coordinates, so it stays anchored while scrolling; dragging past the pane's top/bottom edge scrolls one line per drag event. Soft-wrapped rows join without a newline (the wrap keeps the break space), hard rows with `\n`; trailing spaces are trimmed at logical line ends; wide chars never split. Trimming old entries or a resize clears the selection. Pure helpers live in `tui/select.rs`. Known limitation: replies and echoes are pre-wrapped by their renderers, so a copy of them keeps a newline at each visual wrap (and the echo's 2-column prefix). Not done: double-click word select, keyboard copy. Design: `docs/superpowers/specs/2026-10-26-polish-design.md` (B).
```

Also update the v0.33.0 paragraph's mention only if it would now be false? No — history paragraphs stay as written.

- [ ] **Step 4:** `cargo test`, clippy, `cargo fmt --check`.
- [ ] **Step 5: Commit** `git add Cargo.toml Cargo.lock src/tui/welcome_tests.rs README.md SPEC.md` — `chore: v0.37.0 (in-app selection)`.
