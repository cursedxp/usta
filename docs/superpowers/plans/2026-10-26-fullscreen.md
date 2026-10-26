# Fullscreen TUI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** usta owns the whole terminal window (alternate screen + mouse capture) and renders chat history itself in a scrollable pane above the multiline editor and status bar (opencode-style), per `docs/superpowers/specs/2026-10-26-fullscreen-design.md`.

**Architecture:** A new `tui/chat.rs` holds un-wrapped source entries and the scroll state (`offset` = lines from bottom, `follow`, `unread`); it re-wraps from source on width change. `page()`/`page_*` push into `Tui.chat` instead of `insert_before`. A pure `render_screen(&mut Frame, ...)` in `page.rs` splits the frame into chat pane / editor / status bar and is unit-tested with `TestBackend`; `draw()` calls it. `term.rs` switches to `Viewport::Fullscreen` + `EnterAlternateScreen` + `EnableMouseCapture`; all inline-only machinery (CPR seed, `set_height` rebuild, `TrackedBackend`, trailing `println!`) is deleted. One shared `handle_nav(&mut Tui, &Event) -> bool` is called from every event loop for wheel / PageUp / PageDown.

**Tech Stack:** Rust (rust-version 1.88), ratatui 0.30.2, crossterm 0.29, ratatui-textarea 0.9.3, unicode-width 0.2, tokio. Work in `/Users/optimotion/Documents/Work/Projects/headspace/usta`.

**Global Constraints:**
- Branch: cut `fullscreen` from `main` AFTER `multiline-input` merges. Every line anchor below was read on `multiline-input` (the base); if line numbers drift, match on the quoted code, not the number.
- CLAUDE.md: surgical changes; every changed line traces to this plan. Production-code file budget is 600 lines (SPEC.md §11): check `wc -l` before adding to a file. `run.rs` is 589 lines — the only change allowed there is the 1-line `handle_nav` call, 1 doc-comment edit and the `page` -> `page_fixed` rename (net +1 line).
- `cargo fmt --check` is crate-wide clean. Format only the files you edited: `rustfmt --edition 2021 <file>...` (never bare `cargo fmt`).
- Pre-existing failure `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt` fails on the base. Leave it. "All tests pass" below means: everything except that one test.
- Remove only the orphans this change creates (`backend_wrap.rs`, `Tui::set_height`, imports). Comments elsewhere that still say "insert_before"/"TrackedBackend" (`convert.rs` doc line, `polite.rs` ~L658 comment) are pre-existing prose — do not edit them (mention in the final report).
- Plain (non-TTY) mode in `plain.rs`/`ui.rs` is untouched.
- Commit messages: `feat(tui): ...` style, end with the Co-Authored-By trailer the session instructs.
- Tasks 2 and 3 must land back-to-back on the branch: after Task 2 `page()` no longer writes to the screen and `draw()` is still the old inline code, so the app is not usable between them (tests stay green). Do not release/tag between them.

---

## File Structure

| File | Action | Responsibility |
|---|---|---|
| `src/tui/chat.rs` | Create (~170 prod lines + tests) | `Source`, `ChatBuffer` (entries, wrap, offset/follow/unread math), `wrap_line`, `clip_line` |
| `src/tui/mod.rs` | Modify | `pub mod chat;`, drop `backend_wrap`, fix doc line |
| `src/tui/paint.rs` | Modify (+~12 lines) | `reply_text(reply, width)` pure builder (moved out of `page_reply`) |
| `src/tui/page.rs` | Modify | `page`/`page_fixed`/`page_reply`/`page_user_echo` push to chat; `render_screen`; `draw` rewrite; `handle_nav`; guards |
| `src/tui/term.rs` | Rewrite | Fullscreen terminal, alt screen, mouse capture, `restore()` |
| `src/tui/backend_wrap.rs` | Delete | Orphaned once `term.rs` stops using `TrackedBackend` |
| `src/tui/run.rs`, `entry.rs` | Modify | `page` -> `page_fixed` for welcome panels; `handle_nav` hook |
| `src/tui/ask.rs`, `intro.rs` | Modify | `handle_nav` hook in each event loop |
| `Cargo.toml`, `Cargo.lock`, `src/tui/welcome_tests.rs`, `README.md`, `SPEC.md` | Modify | v0.33.0 bump (same file set as the v0.32.0 commit `6004da3`) |

---

## Task 1: Chat buffer module (wrap / scroll / follow / unread math)

**Files:**
- Create: `src/tui/chat.rs`
- Modify: `src/tui/mod.rs`
- Modify: `src/tui/paint.rs` (add `reply_text`)

**Interfaces:**
- Consumes: `crate::tui::paint::user_echo_text(line: &str, width: u16) -> Text<'static>` (existing, `pub(crate)`), `crate::tui::convert::ansi_to_text`, `crate::ui::render_markdown(md: &str, width: usize) -> String`, `crate::tui::theme::{BRAND_IDX, G_BRAND}`.
- Produces (all `pub` so `Tui` can expose the field):
  - `pub enum Source { Text(Text<'static>), Fixed(Text<'static>), Reply(String), Echo(String) }` — `Text` = soft-wrapped on width change (notices); `Fixed` = clipped, never re-wrapped (welcome panels, which have box borders); `Reply` = markdown re-rendered for the width; `Echo` = user line re-wrapped for the width.
  - `pub struct ChatBuffer` with `impl Default`.
  - `pub fn push(&mut self, s: Source)`
  - `pub fn visible(&mut self, width: u16, height: u16) -> Vec<Line<'static>>`
  - `pub fn scroll_up(&mut self, n: usize)`, `pub fn scroll_down(&mut self, n: usize)`, `pub fn page_up(&mut self)`, `pub fn page_down(&mut self)`, `pub fn jump_bottom(&mut self)`
  - `pub fn unread(&self) -> bool`, `pub fn is_following(&self) -> bool`, `pub fn line_count(&mut self, width: u16) -> usize`
  - `pub(crate) fn reply_text(reply: &str, width: u16) -> Text<'static>` in `paint.rs`

Semantics: `offset` = number of lines hidden below the pane (0 = bottom). `follow == (offset == 0)` except it is set false only when a scroll actually moved. New content while `!follow` keeps the view stable (`offset += added_lines`) and sets `unread`. Short content (fewer lines than the pane) is top-anchored.

- [ ] **Step 1: Write the failing tests** — create `src/tui/chat.rs` containing ONLY this test module, and add `pub mod chat;` to `src/tui/mod.rs` (between `backend_wrap` and `convert`, keeping alphabetical order).

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Style};

    fn plain(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    /// `n` single-line entries "l0".."l{n-1}".
    fn filled(n: usize) -> ChatBuffer {
        let mut c = ChatBuffer::default();
        for i in 0..n {
            c.push(Source::Text(Text::raw(format!("l{i}"))));
        }
        c
    }

    #[test]
    fn wrap_line_splits_by_display_width_and_keeps_style() {
        let red = Style::default().fg(Color::Red);
        let l = Line::from(Span::styled("abcdef", red));
        let w = wrap_line(&l, 3);
        assert_eq!(plain(&w), vec!["abc", "def"]);
        assert!(w.iter().all(|r| r.spans[0].style == red));
        // wide (2-column) chars are never split in half
        let w = wrap_line(&Line::raw("日本語"), 4);
        assert_eq!(plain(&w), vec!["日本", "語"]);
        // empty line stays one empty row
        assert_eq!(wrap_line(&Line::raw(""), 10).len(), 1);
    }

    #[test]
    fn clip_line_truncates_to_width() {
        assert_eq!(plain(&[clip_line(&Line::raw("abcdef"), 4)]), vec!["abcd"]);
    }

    #[test]
    fn short_content_is_top_anchored() {
        let mut c = filled(3);
        assert_eq!(plain(&c.visible(80, 10)), vec!["l0", "l1", "l2"]);
    }

    #[test]
    fn follows_the_bottom_by_default() {
        let mut c = filled(20);
        assert_eq!(plain(&c.visible(80, 5)), vec!["l15", "l16", "l17", "l18", "l19"]);
        assert!(c.is_following());
        c.push(Source::Text(Text::raw("new")));
        assert_eq!(plain(&c.visible(80, 5)).last().unwrap(), "new");
        assert!(!c.unread());
    }

    #[test]
    fn scroll_up_moves_the_window_and_clamps_at_the_top() {
        let mut c = filled(20);
        c.visible(80, 5);
        c.scroll_up(3);
        assert_eq!(plain(&c.visible(80, 5)), vec!["l12", "l13", "l14", "l15", "l16"]);
        assert!(!c.is_following());
        c.scroll_up(1000);
        assert_eq!(plain(&c.visible(80, 5)), vec!["l0", "l1", "l2", "l3", "l4"]);
    }

    #[test]
    fn scroll_up_is_a_noop_when_everything_fits() {
        let mut c = filled(3);
        c.visible(80, 10);
        c.scroll_up(3);
        assert!(c.is_following());
    }

    #[test]
    fn scrolling_back_to_the_bottom_refollows_and_clears_unread() {
        let mut c = filled(20);
        c.visible(80, 5);
        c.scroll_up(3);
        c.push(Source::Text(Text::raw("new")));
        assert!(c.unread());
        c.scroll_down(2);
        assert!(!c.is_following() && c.unread());
        c.scroll_down(100);
        assert!(c.is_following() && !c.unread());
        assert_eq!(plain(&c.visible(80, 5)).last().unwrap(), "new");
    }

    #[test]
    fn new_content_while_scrolled_keeps_the_view_stable() {
        let mut c = filled(20);
        c.visible(80, 5);
        c.scroll_up(3);
        let before = plain(&c.visible(80, 5));
        c.push(Source::Text(Text::raw("a\nb")));
        assert_eq!(plain(&c.visible(80, 5)), before);
        assert!(c.unread());
    }

    #[test]
    fn page_up_and_down_move_pane_height_minus_one() {
        let mut c = filled(40);
        c.visible(80, 10);
        c.page_up();
        assert_eq!(c.offset, 9);
        assert_eq!(plain(&c.visible(80, 10))[0], "l21");
        c.page_down();
        assert_eq!(c.offset, 0);
        assert!(c.is_following());
    }

    #[test]
    fn jump_bottom_resets_everything() {
        let mut c = filled(20);
        c.visible(80, 5);
        c.scroll_up(3);
        c.push(Source::Text(Text::raw("new")));
        c.jump_bottom();
        assert!(c.is_following() && !c.unread() && c.offset == 0);
    }

    #[test]
    fn echo_and_reply_rewrap_when_the_width_changes() {
        let mut c = ChatBuffer::default();
        c.push(Source::Echo("x ".repeat(30)));
        let wide = c.line_count(80);
        let narrow = c.line_count(20);
        assert!(narrow > wide, "narrow {narrow} should exceed wide {wide}");
        assert_eq!(c.line_count(80), wide, "width round-trip is stable");
    }

    #[test]
    fn offset_is_clamped_after_the_pane_grows() {
        let mut c = filled(20);
        c.visible(80, 5);
        c.scroll_up(8);
        assert_eq!(c.offset, 8);
        c.visible(80, 18); // taller pane: max offset is now 2
        assert_eq!(c.offset, 2);
        c.visible(80, 25); // everything fits
        assert_eq!(c.offset, 0);
        assert!(c.is_following());
    }

    #[test]
    fn fixed_entries_are_clipped_not_wrapped() {
        let mut c = ChatBuffer::default();
        c.push(Source::Fixed(Text::raw("0123456789")));
        assert_eq!(plain(&c.visible(4, 5)), vec!["0123"]);
        c.push(Source::Text(Text::raw("0123456789")));
        assert_eq!(c.line_count(4), 1 + 3);
    }

    #[test]
    fn zero_sized_pane_returns_nothing() {
        let mut c = filled(3);
        assert!(c.visible(0, 5).is_empty());
        assert!(c.visible(80, 0).is_empty());
    }
}
```

- [ ] **Step 2: Run the tests, expect compile failure**

Run: `cargo test chat::`
Expected: FAIL to compile — `cannot find type 'ChatBuffer' / 'Source' / function 'wrap_line' / 'clip_line' in this scope`.

- [ ] **Step 3: Implement.** Put this ABOVE the test module in `src/tui/chat.rs`:

```rust
//! Scrollable chat history for the fullscreen TUI. Entries keep their
//! un-wrapped source so every width change re-wraps from scratch; the scroll
//! position is `offset` = lines hidden BELOW the pane (0 = at the bottom).

use ratatui::text::{Line, Span, Text};
use unicode_width::UnicodeWidthChar;

use crate::tui::paint::{reply_text, user_echo_text};

/// What was pushed, kept in a form that can be re-rendered at any width.
pub enum Source {
    /// Soft-wrapped (char-split) on width change — notices, warnings, errors.
    Text(Text<'static>),
    /// Clipped, never re-wrapped — welcome panels (box borders break if wrapped).
    Fixed(Text<'static>),
    /// Assistant markdown, re-rendered for the width.
    Reply(String),
    /// The user's submitted line, re-wrapped for the width.
    Echo(String),
}

pub struct ChatBuffer {
    entries: Vec<Source>,
    /// Lines hidden below the pane. 0 = showing the newest line.
    offset: usize,
    follow: bool,
    unread: bool,
    /// Width / pane height seen by the last `visible()` call (used by scroll math).
    width: u16,
    pane_h: u16,
    cache: Option<(u16, Vec<Line<'static>>)>,
}

impl Default for ChatBuffer {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            offset: 0,
            follow: true,
            unread: false,
            width: 80,
            pane_h: 1,
            cache: None,
        }
    }
}

/// Greedy char-split of one line at `w` display columns, keeping span styles.
fn wrap_line(line: &Line<'static>, w: usize) -> Vec<Line<'static>> {
    let w = w.max(1);
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut cur: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    for span in &line.spans {
        let mut buf = String::new();
        for ch in span.content.chars() {
            let cw = ch.width().unwrap_or(0);
            if used + cw > w && used > 0 {
                if !buf.is_empty() {
                    cur.push(Span::styled(std::mem::take(&mut buf), span.style));
                }
                out.push(Line::from(std::mem::take(&mut cur)).style(line.style));
                used = 0;
            }
            buf.push(ch);
            used += cw;
        }
        if !buf.is_empty() {
            cur.push(Span::styled(buf, span.style));
        }
    }
    out.push(Line::from(cur).style(line.style));
    out
}

/// First visual row of `wrap_line` — a line cut at the pane edge.
fn clip_line(line: &Line<'static>, w: usize) -> Line<'static> {
    wrap_line(line, w).swap_remove(0)
}

fn wrap_text(t: &Text<'static>, w: u16) -> Vec<Line<'static>> {
    t.lines.iter().flat_map(|l| wrap_line(l, w as usize)).collect()
}

fn render(src: &Source, w: u16) -> Vec<Line<'static>> {
    match src {
        Source::Text(t) => wrap_text(t, w),
        Source::Fixed(t) => t.lines.iter().map(|l| clip_line(l, w as usize)).collect(),
        Source::Reply(s) => wrap_text(&reply_text(s, w), w),
        Source::Echo(s) => wrap_text(&user_echo_text(s, w), w),
    }
}

impl ChatBuffer {
    fn flat(&mut self, width: u16) -> &[Line<'static>] {
        if self.cache.as_ref().map(|c| c.0) != Some(width) {
            let lines = self.entries.iter().flat_map(|e| render(e, width)).collect();
            self.cache = Some((width, lines));
        }
        &self.cache.as_ref().expect("cache just filled").1
    }

    /// Total wrapped lines at `width`.
    pub fn line_count(&mut self, width: u16) -> usize {
        self.flat(width).len()
    }

    pub fn push(&mut self, s: Source) {
        if self.follow {
            self.entries.push(s);
            self.cache = None;
            return;
        }
        // Scrolled up: keep the user's view still and flag the new content.
        let w = self.width;
        let before = self.flat(w).len();
        self.entries.push(s);
        self.cache = None;
        let after = self.flat(w).len();
        self.offset += after.saturating_sub(before);
        self.unread = true;
    }

    /// The lines to paint into a `width` x `height` pane (top-anchored when short).
    /// Also records the pane size for the scroll helpers and clamps `offset`.
    pub fn visible(&mut self, width: u16, height: u16) -> Vec<Line<'static>> {
        if width == 0 || height == 0 {
            return Vec::new();
        }
        self.width = width;
        self.pane_h = height;
        let h = height as usize;
        let total = self.flat(width).len();
        self.offset = self.offset.min(total.saturating_sub(h));
        if self.offset == 0 {
            self.follow = true;
            self.unread = false;
        }
        let end = total - self.offset;
        let start = end.saturating_sub(h);
        self.flat(width)[start..end].to_vec()
    }

    pub fn scroll_up(&mut self, n: usize) {
        let w = self.width;
        let max = self.flat(w).len().saturating_sub(self.pane_h as usize);
        if max == 0 {
            return;
        }
        self.offset = (self.offset + n).min(max);
        self.follow = false;
    }

    pub fn scroll_down(&mut self, n: usize) {
        self.offset = self.offset.saturating_sub(n);
        if self.offset == 0 {
            self.follow = true;
            self.unread = false;
        }
    }

    fn page_step(&self) -> usize {
        (self.pane_h as usize).saturating_sub(1).max(1)
    }

    pub fn page_up(&mut self) {
        self.scroll_up(self.page_step());
    }

    pub fn page_down(&mut self) {
        self.scroll_down(self.page_step());
    }

    pub fn jump_bottom(&mut self) {
        self.offset = 0;
        self.follow = true;
        self.unread = false;
    }

    pub fn unread(&self) -> bool {
        self.unread
    }

    pub fn is_following(&self) -> bool {
        self.follow
    }
}
```

Add to `src/tui/paint.rs`, directly after `user_echo_text`'s function (before the next item), the markdown builder that currently lives inside `page_reply`:

```rust
/// Assistant reply block: orange `●` line + termimad markdown + blank line,
/// rendered for `width`. Pure — `ChatBuffer` re-runs it on every width change.
pub(crate) fn reply_text(reply: &str, width: u16) -> Text<'static> {
    let ansi = crate::ui::render_markdown(reply, width as usize);
    let mut t = ansi_to_text(&format!(
        "\x1b[38;5;{}m{}\x1b[0m\n{ansi}\n",
        theme::BRAND_IDX,
        theme::G_BRAND
    ));
    t.lines.push(Line::raw(""));
    t
}
```
(Uses the fully-qualified `crate::ui::render_markdown`; no new import needed.) Add a test to `paint.rs`'s existing test module:

```rust
    #[test]
    fn reply_text_starts_with_the_brand_glyph_and_ends_blank() {
        let t = reply_text("**hi**", 60);
        let first: String = t.lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(first.contains(theme::G_BRAND));
        assert!(t.lines.last().unwrap().spans.iter().all(|s| s.content.is_empty()));
        assert!(t.lines.len() >= 3);
    }
```
(If `paint.rs`'s test module lacks `use super::*;`, add it. `page_reply` still has its own copy of the body until Task 2 — that is intended; Task 2 deletes it.)

- [ ] **Step 4: Run, expect pass**

Run: `cargo test chat:: && cargo test paint::`
Expected: all `chat::tests::*` and `paint::tests::reply_text_*` PASS. Then `cargo build` must have no new warnings except possibly `dead_code` for unused `ChatBuffer` items (they get used in Task 2; if the build is `-D warnings` locally, add nothing — just proceed).

- [ ] **Step 5: Format and commit**

```bash
rustfmt --edition 2021 src/tui/chat.rs src/tui/paint.rs src/tui/mod.rs
cargo test chat:: paint::
git add src/tui/chat.rs src/tui/mod.rs src/tui/paint.rs
git commit -m "feat(tui): chat buffer with re-wrap, scroll, follow and unread math"
```
(`cargo test` takes ONE filter; run the two separately if the combined form errors.)

---

## Task 2: `page()` pushes to the buffer; `render_screen` layout with TestBackend test

**Files:**
- Modify: `src/tui/term.rs` (add `chat` field only)
- Modify: `src/tui/page.rs`
- Modify: `src/tui/run.rs` (L356 `page(` -> `page_fixed(`), `src/tui/entry.rs` (L213 `page(` -> `page_fixed(`)

**Interfaces:**
- Consumes: Task 1's `ChatBuffer`, `Source`, `visible`; `InputBox::desired_rows(&self, width: u16) -> u16`, `InputBox::render(&self, f: &mut Frame, area: Rect)`; `term::viewport_height(rows: u16) -> u16` (= rows.clamp(1, 8) + 2); `status::render_status(...) -> Line<'static>`.
- Produces:
  - `Tui.chat: ChatBuffer` (`pub` field)
  - `pub(crate) fn page(tui: &mut Tui, text: Text<'static>) -> Result<()>` (same signature, now `Source::Text`)
  - `pub(crate) fn page_fixed(tui: &mut Tui, text: Text<'static>) -> Result<()>` (new, `Source::Fixed`)
  - `page_reply(tui, reply, _width)` and `page_user_echo(tui, line)` keep their signatures
  - `pub(crate) fn render_screen(f: &mut Frame, chat: &mut ChatBuffer, editor: &InputBox, status: Line<'static>)`

- [ ] **Step 1: Write the failing tests.** In `src/tui/page.rs`'s existing `mod tests`, add at the top `use super::*; use ratatui::backend::TestBackend; use ratatui::Terminal;` and these tests:

```rust
    fn screen_rows(w: u16, h: u16, chat: &mut ChatBuffer, editor: &InputBox) -> Vec<String> {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| render_screen(f, chat, editor, Line::raw("status")))
            .unwrap();
        let b = term.backend().buffer();
        (0..h)
            .map(|y| (0..w).map(|x| b[(x, y)].symbol()).collect::<String>())
            .collect()
    }

    #[test]
    fn layout_80x24_is_chat_then_editor_then_status() {
        let mut chat = ChatBuffer::default();
        chat.push(Source::Echo("hello".into()));
        chat.push(Source::Reply("**hi there**".into()));
        let editor = InputBox::new();
        let rows = screen_rows(80, 24, &mut chat, &editor);
        // chat pane = rows 0..21, rule row 21, one input row 22, status row 23
        assert!(rows[..21].iter().any(|r| r.contains("hello")));
        assert!(rows[..21].iter().any(|r| r.contains("hi there")));
        assert!(rows[21].starts_with('─'), "rule: {:?}", rows[21]);
        assert!(rows[22].starts_with("> "), "input: {:?}", rows[22]);
        assert!(rows[23].starts_with("status"));
    }

    #[test]
    fn layout_narrow_and_tiny_do_not_panic() {
        let mut chat = ChatBuffer::default();
        chat.push(Source::Echo("a long line that must wrap in a narrow pane".into()));
        let editor = InputBox::new();
        let rows = screen_rows(20, 8, &mut chat, &editor);
        assert!(rows[5].starts_with('─'));
        assert!(rows[6].starts_with("> "));
        assert!(rows[7].starts_with("status"));
        assert!(rows[..5].iter().filter(|r| r.contains("pane") || r.contains("long")).count() >= 1);
        // no room for a chat pane at all: status still lands on the last row
        let rows = screen_rows(20, 3, &mut chat, &editor);
        assert!(rows[2].starts_with("status"));
    }

    #[test]
    fn page_pushes_to_the_chat_buffer_not_the_terminal() {
        let prod = include_str!("page.rs").split("#[cfg(test)]").next().unwrap();
        assert!(!prod.contains("insert_before"), "page.rs must not use insert_before");
        assert!(prod.contains("Source::Reply"));
        assert!(prod.contains("Source::Echo"));
        assert!(prod.contains("Source::Fixed"));
    }
```
(In the last test the needle strings appear in the prod slice only; this test body sits after `#[cfg(test)]` so it cannot satisfy itself.)

- [ ] **Step 2: Run, expect failure**

Run: `cargo test page::tests`
Expected: FAIL to compile — `cannot find function 'render_screen'`, `cannot find type 'ChatBuffer'/'Source' in this scope`.

- [ ] **Step 3: Implement.**

`src/tui/term.rs` (still inline in this task) — add the field and initialise it:

```rust
use crate::tui::chat::ChatBuffer;
// ...
pub struct Tui {
    pub terminal: Terminal<TrackedBackend<Stdout>>,
    height: u16,
    pub chat: ChatBuffer,
}
```
and in `setup()` the last line becomes `Ok(Tui { terminal, height, chat: ChatBuffer::default() })`. (`set_height` only reassigns `self.terminal`/`self.height`, so `chat` survives rebuilds.)

`src/tui/page.rs` production code — replace the import block and the three functions; leave `page_notice/page_warn/page_error`, `flush_notices`, `current_width`, `handle_resize` and the OLD `draw` untouched:

```rust
use anyhow::Result;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Text};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::chat::{ChatBuffer, Source};
use crate::tui::editor::InputBox;
use crate::tui::status::{render_status, Status};
use crate::tui::term::{viewport_height, Tui};
use crate::tui::theme;
use crate::ui;

/// Push persistent content into the chat pane (soft-wrapped on resize).
pub(crate) fn page(tui: &mut Tui, text: Text<'static>) -> Result<()> {
    tui.chat.push(Source::Text(text));
    Ok(())
}

/// Like `page`, but the lines are pre-sized panels (welcome box): clipped on a
/// narrower terminal instead of re-wrapped, so box borders stay intact.
pub(crate) fn page_fixed(tui: &mut Tui, text: Text<'static>) -> Result<()> {
    tui.chat.push(Source::Fixed(text));
    Ok(())
}

/// Usta's reply: orange ● line + markdown + blank line. The markdown is
/// rendered by the chat pane for the CURRENT width, so `_width` is unused
/// (kept so callers stay unchanged).
pub(crate) fn page_reply(tui: &mut Tui, reply: &str, _width: u16) -> Result<()> {
    tui.chat.push(Source::Reply(reply.to_string()));
    Ok(())
}
```
and replace `page_user_echo`:

```rust
/// Push the user's submitted line to the chat pane (wrapped at render time).
pub(crate) fn page_user_echo(tui: &mut Tui, line: &str) -> Result<()> {
    tui.chat.push(Source::Echo(line.to_string()));
    Ok(())
}
```
Add `render_screen` directly above `draw`:

```rust
/// One frame: chat pane (fills the rest) / editor (rule + 1..=8 rows) / status bar.
/// Pure over its inputs so it can be driven by a `TestBackend`.
pub(crate) fn render_screen(
    f: &mut Frame,
    chat: &mut ChatBuffer,
    editor: &InputBox,
    status: Line<'static>,
) {
    let area = f.area();
    let box_h = viewport_height(editor.desired_rows(area.width)) - 1;
    let [chat_area, box_area, status_area] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(box_h),
        Constraint::Length(1),
    ])
    .areas(area);
    let lines = chat.visible(chat_area.width, chat_area.height);
    f.render_widget(Paragraph::new(lines), chat_area);
    editor.render(f, box_area);
    f.render_widget(status, status_area);
}
```
Delete from `page.rs` the now-unused `use crate::tui::convert::ansi_to_text;` and `Widget` import (already dropped above). `draw()` body stays as-is for this task (it still uses `Layout`, `Constraint`, `render_status`, `viewport_height`).

`run.rs` L356 and `entry.rs` L213: change `crate::tui::page::page(` to `crate::tui::page::page_fixed(` (welcome panels only — these two are the only direct `page(` callers).

- [ ] **Step 4: Run, expect pass**

Run: `cargo test page::tests && cargo test chat:: && cargo build`
Expected: new `page::tests::*` PASS (including the pre-existing `resize_events_are_handled_in_every_event_loop` and `draw_resizes_the_viewport_before_painting`, which are still valid in this task). `cargo build` warns nothing new.

- [ ] **Step 5: Format and commit**

```bash
rustfmt --edition 2021 src/tui/page.rs src/tui/term.rs src/tui/run.rs src/tui/entry.rs
git add src/tui/page.rs src/tui/term.rs src/tui/run.rs src/tui/entry.rs
git commit -m "feat(tui): page() feeds the chat buffer; render_screen lays out chat/editor/status"
```

---

## Task 3: Alternate screen + mouse capture; delete inline-only code

**Files:**
- Rewrite: `src/tui/term.rs`
- Modify: `src/tui/page.rs` (`draw`, `handle_resize` doc, guard test)
- Modify: `src/tui/mod.rs` (drop `backend_wrap`, fix doc line)
- Modify: `src/tui/run.rs` (L1-2 doc comment only)
- Delete: `src/tui/backend_wrap.rs`

**Interfaces:**
- Consumes: Task 2's `render_screen`, `Tui.chat`.
- Produces:
  - `pub struct Tui { pub terminal: Terminal<CrosstermBackend<Stdout>>, pub chat: ChatBuffer }`
  - `pub fn setup() -> Result<Tui>` (same signature; enters alt screen + enables mouse capture)
  - `pub fn restore()` (same signature; additionally disables mouse capture, leaves alt screen, shows cursor)
  - `pub fn viewport_height(rows: u16) -> u16` unchanged (now documented as the bottom region height)
  - REMOVED: `Tui::height`, `Tui::set_height`, `inline_terminal`, `TrackedBackend`, `fallback_seed`, CPR seed, trailing `println!`
  - `draw(...)` keeps its exact signature (callers in run/ask/entry/intro untouched)

- [ ] **Step 1: Write the failing tests.** Replace the ENTIRE `mod tests` in `src/tui/term.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn prod() -> &'static str {
        include_str!("term.rs").split("#[cfg(test)]").next().unwrap()
    }

    #[test]
    fn viewport_is_rule_plus_rows_plus_status() {
        assert_eq!(viewport_height(1), 3);
        assert_eq!(viewport_height(8), 10);
    }

    /// Source pin: setup takes the whole window.
    #[test]
    fn setup_enters_the_alternate_screen_with_mouse_capture() {
        let body = prod().split("pub fn setup(").nth(1).expect("setup");
        let body = body.split("pub fn restore(").next().unwrap();
        assert!(body.contains("EnterAlternateScreen"));
        assert!(body.contains("EnableMouseCapture"));
        assert!(body.contains("Viewport::Fullscreen"));
    }

    /// Source pin: restore() undoes both, mouse capture first, and the panic hook calls it.
    #[test]
    fn restore_leaves_the_alternate_screen_and_disables_mouse_capture() {
        let body = prod().split("pub fn restore(").nth(1).expect("restore");
        let mouse = body.find("DisableMouseCapture").expect("mouse capture off");
        let leave = body.find("LeaveAlternateScreen").expect("alt screen left");
        assert!(mouse < leave);
        assert!(body.contains("disable_raw_mode"));
        assert!(prod().contains("restore();\n        prev(info);"), "panic hook must restore");
    }

    /// Source pin: the inline-viewport machinery is gone for good.
    #[test]
    fn no_inline_viewport_remnants() {
        for needle in [
            "Viewport::Inline",
            "TrackedBackend",
            "cursor::position()",
            "set_height",
            "inline_terminal",
            "println!",
        ] {
            assert!(!prod().contains(needle), "term.rs still contains {needle}");
        }
    }

    #[test]
    fn drop_only_restores() {
        let body = prod().split("impl Drop for Tui").nth(1).expect("Drop impl");
        assert!(body.contains("restore();"));
        assert!(!body.contains("clear()"));
    }
}
```

In `src/tui/page.rs` tests, replace `draw_resizes_the_viewport_before_painting` with:

```rust
    #[test]
    fn draw_paints_through_render_screen() {
        let src = include_str!("page.rs");
        let draw = src.split("pub(crate) fn draw(").nth(1).expect("draw var");
        let draw = draw.split("#[cfg(test)]").next().unwrap();
        assert!(draw.contains("render_screen("), "draw must use render_screen");
        assert!(!draw.contains("set_height("), "no viewport rebuild in fullscreen");
    }
```

- [ ] **Step 2: Run, expect failure**

Run: `cargo test term::tests && cargo test page::tests::draw_paints`
Expected: FAIL — `setup_enters_the_alternate_screen_with_mouse_capture` (no `EnterAlternateScreen`), `restore_leaves...`, `no_inline_viewport_remnants` (`Viewport::Inline` still present), `draw_paints_through_render_screen` (draw has no `render_screen(` and still calls `set_height(`).

- [ ] **Step 3: Implement.**

Replace the whole production part of `src/tui/term.rs` (everything above `#[cfg(test)]`) with:

```rust
//! Terminal lifecycle: take over the whole window (alternate screen + mouse
//! capture) and give it back NO MATTER WHAT. A shell left in raw mode or on the
//! alternate screen = the worst user experience; Drop + panic hook double
//! safety net.

use std::io::Stdout;

use anyhow::Result;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::{Terminal, TerminalOptions, Viewport};

use crate::tui::chat::ChatBuffer;
use crate::tui::editor::MAX_INPUT_ROWS;

/// Bottom region under the chat pane: rule + 1..=8 input rows + status line.
pub fn viewport_height(rows: u16) -> u16 {
    rows.clamp(1, MAX_INPUT_ROWS) + 2
}

pub struct Tui {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
    pub chat: ChatBuffer,
}

/// Raw mode + alternate screen + mouse capture. Restore is chained onto the
/// panic hook — the previous hook is preserved (the test harness's hook isn't
/// overwritten).
pub fn setup() -> Result<Tui> {
    crossterm::terminal::enable_raw_mode()?;
    // Own the whole window; wheel events reach us (Shift+drag selects natively).
    if let Err(e) = crossterm::execute!(
        std::io::stdout(),
        EnterAlternateScreen,
        EnableMouseCapture
    ) {
        restore();
        return Err(e.into());
    }
    // Bracketed paste: the paste arrives as a single Event::Paste — line breaks
    // aren't counted as Enter and don't split the message. Silently skipped on
    // unsupported terminals (falls back to old behavior).
    let _ = crossterm::execute!(std::io::stdout(), EnableBracketedPaste);
    // Kitty keyboard protocol: lets modern terminals disambiguate Shift+Enter / Alt+Enter
    // from bare Enter. Unsupported terminals are skipped silently (Ctrl+J still works).
    if matches!(
        crossterm::terminal::supports_keyboard_enhancement(),
        Ok(true)
    ) {
        let _ = crossterm::execute!(
            std::io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
    }
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        prev(info);
    }));
    let terminal = Terminal::with_options(
        CrosstermBackend::new(std::io::stdout()),
        TerminalOptions {
            viewport: Viewport::Fullscreen,
        },
    )?;
    Ok(Tui {
        terminal,
        chat: ChatBuffer::default(),
    })
}

/// Give the terminal back — idempotent, swallows errors (no panics on the shutdown path).
pub fn restore() {
    let _ = crossterm::execute!(std::io::stdout(), PopKeyboardEnhancementFlags);
    let _ = crossterm::execute!(std::io::stdout(), DisableBracketedPaste);
    let _ = crossterm::execute!(
        std::io::stdout(),
        DisableMouseCapture,
        LeaveAlternateScreen,
        crossterm::cursor::Show
    );
    let _ = crossterm::terminal::disable_raw_mode();
}

impl Drop for Tui {
    fn drop(&mut self) {
        // Leaving the alternate screen restores the user's previous content;
        // anything printed afterwards lands on the normal screen.
        restore();
    }
}
```

`src/tui/page.rs`: replace `draw` and the `handle_resize` doc/body comment:

```rust
/// Refresh ratatui's idea of the screen size after a terminal resize. The next
/// `draw()` (the caller's loop redraws every iteration) repaints the whole
/// screen and `ChatBuffer` re-wraps from source at the new width. An unchanged
/// size is a no-op (`autoresize()` only resizes when the area differs).
pub(crate) fn handle_resize(tui: &mut Tui) -> Result<()> {
    tui.terminal.autoresize()?;
    Ok(())
}

/// Paint the whole screen: chat pane + input box + status line.
pub(crate) fn draw(
    tui: &mut Tui,
    editor: &InputBox,
    status: &Status,
    tokens: Option<u64>,
    window: u64,
    watch: Option<(bool, bool, usize)>,
    verify_failing: bool,
) -> Result<()> {
    let line = render_status(status, tokens, window, watch, verify_failing);
    let chat = &mut tui.chat;
    tui.terminal
        .draw(|f| render_screen(f, chat, editor, line))?;
    Ok(())
}
```
(`line` is moved into the closure; `draw`'s closure is `FnOnce`-compatible because `Terminal::draw` takes `FnOnce(&mut Frame)`. If the compiler asks for it, add `move` is NOT needed since `line` is moved by value use.) Remove any now-unused imports in `page.rs` (`viewport_height` is still used by `render_screen`).

Delete the file and module: `git rm src/tui/backend_wrap.rs`; in `src/tui/mod.rs` remove `pub(crate) mod backend_wrap;` and change line 1 to `//! Claude Code-style TUI: fullscreen alternate screen + scrollable chat pane.`

`src/tui/run.rs` lines 1-2 doc comment: change "flows into scrollback via `tui/page.rs`'s `insert_before` wrapper; the bottom region is redrawn live." to "flows into the chat pane via `tui/page.rs`'s `page*` wrappers; the whole screen is redrawn live." (Keep line count identical: re-wrap within the same 2 lines.)

- [ ] **Step 4: Run, expect pass**

Run: `cargo build 2>&1 | tail -20 && cargo test`
Expected: build clean (no unused-import warnings); full suite PASS except the pre-existing `materials::...convert_pdfs_missing_tool...` failure. The deleted `backend_wrap` tests no longer run (expected count drop).

- [ ] **Step 5: Format and commit**

```bash
rustfmt --edition 2021 src/tui/term.rs src/tui/page.rs src/tui/mod.rs src/tui/run.rs
git add -A src/tui
git commit -m "feat(tui): fullscreen alternate screen with mouse capture; drop inline viewport machinery"
```

---

## Task 4: Wheel / PageUp / PageDown in every loop, "↓ new message" indicator

**Files:**
- Modify: `src/tui/page.rs` (`handle_nav`, indicator in `render_screen`, jump-to-bottom on user echo, guards)
- Modify: `src/tui/run.rs`, `src/tui/ask.rs` (2 loops), `src/tui/entry.rs`, `src/tui/intro.rs` (1 hook each)

**Interfaces:**
- Consumes: Task 1 `ChatBuffer::{scroll_up, scroll_down, page_up, page_down, jump_bottom, unread}`.
- Produces:
  - `pub(crate) fn handle_nav(tui: &mut Tui, ev: &Event) -> bool` — true when the event was a mouse event or PageUp/PageDown (caller skips its own handling)
  - private `fn nav(chat: &mut ChatBuffer, ev: &Event) -> bool` (the testable core)
  - indicator text `"↓ new message"`, right-aligned on the last chat-pane row while `chat.unread()`

Event loops covered (the only five places that read `EventStream`): `run.rs` main select, `ask.rs` `ask_blocking`-style locked loop (L~56) and yes/no confirm (L~108), `entry.rs` topic entry (L~284), `intro.rs` intro prompt (L~99). `polite.rs` only forwards `events` into `ask.rs` functions, so it needs no hook.

- [ ] **Step 1: Write the failing tests** (in `page.rs` `mod tests`; add `use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};`):

```rust
    fn mouse(kind: MouseEventKind) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        })
    }

    fn filled_chat(n: usize) -> ChatBuffer {
        let mut c = ChatBuffer::default();
        for i in 0..n {
            c.push(Source::Text(Text::raw(format!("l{i}"))));
        }
        c.visible(80, 5);
        c
    }

    #[test]
    fn nav_wheel_scrolls_three_lines_and_consumes_mouse_events() {
        let mut c = filled_chat(20);
        assert!(nav(&mut c, &mouse(MouseEventKind::ScrollUp)));
        assert_eq!(c.visible(80, 5)[0].spans[0].content, "l12");
        assert!(nav(&mut c, &mouse(MouseEventKind::ScrollDown)));
        assert!(c.is_following());
        // motion / clicks are swallowed but change nothing
        assert!(nav(&mut c, &mouse(MouseEventKind::Moved)));
        assert!(c.is_following());
    }

    #[test]
    fn nav_page_keys_scroll_a_page_and_other_keys_pass_through() {
        let mut c = filled_chat(40);
        let key = |code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
        assert!(nav(&mut c, &key(KeyCode::PageUp)));
        assert!(!c.is_following());
        assert!(nav(&mut c, &key(KeyCode::PageDown)));
        assert!(c.is_following());
        assert!(!nav(&mut c, &key(KeyCode::Char('a'))));
        assert!(!nav(&mut c, &Event::Paste("x".into())));
    }

    #[test]
    fn indicator_shows_only_while_unread() {
        let mut chat = filled_chat(30);
        let editor = InputBox::new();
        let rows = screen_rows(40, 10, &mut chat, &editor); // pane = 7 rows
        assert!(!rows[6].contains("new message"));
        chat.scroll_up(3);
        chat.push(Source::Text(Text::raw("fresh")));
        let rows = screen_rows(40, 10, &mut chat, &editor);
        assert!(rows[6].trim_end().ends_with("↓ new message"), "{:?}", rows[6]);
        chat.scroll_down(100);
        let rows = screen_rows(40, 10, &mut chat, &editor);
        assert!(!rows[6].contains("new message"));
    }

    /// Source pin: every loop that reads the EventStream routes through handle_nav
    /// (otherwise the wheel is silently ignored in that prompt, like Resize once was).
    #[test]
    fn mouse_and_page_keys_are_handled_in_every_event_loop() {
        for (name, src, loops) in [
            ("run.rs", include_str!("run.rs"), 1),
            ("ask.rs", include_str!("ask.rs"), 2),
            ("entry.rs", include_str!("entry.rs"), 1),
            ("intro.rs", include_str!("intro.rs"), 1),
        ] {
            let prod = src.split("#[cfg(test)]").next().unwrap();
            assert_eq!(
                prod.matches("page::handle_nav(").count(),
                loops,
                "{name}: expected {loops} handle_nav call(s)"
            );
        }
        let own = include_str!("page.rs").split("#[cfg(test)]").next().unwrap();
        assert!(own.contains("Event::Mouse"));
        assert!(own.contains("KeyCode::PageUp") && own.contains("KeyCode::PageDown"));
    }
```
(`screen_rows` is the helper added in Task 2; it already takes `&mut ChatBuffer`.)

- [ ] **Step 2: Run, expect failure**

Run: `cargo test page::tests`
Expected: FAIL to compile — `cannot find function 'nav' in this scope` (indicator and loop-hook tests would fail next).

- [ ] **Step 3: Implement.**

`page.rs` — add imports `use crossterm::event::{Event, KeyCode, KeyEvent, MouseEventKind};` and `use ratatui::layout::Rect;` (extend the existing layout import to `{Constraint, Layout, Rect}`), `use ratatui::text::Span;` (extend to `{Line, Span, Text}`). Add above `handle_resize`:

```rust
/// Wheel notch = this many chat lines.
const WHEEL_LINES: usize = 3;
const NEW_MESSAGE: &str = "↓ new message";

/// Chat navigation shared by every event loop: wheel + PageUp/PageDown.
/// Returns true when the event was navigation (consumed) — the caller then
/// skips its own handling. Other mouse events (motion, clicks) are swallowed
/// too: mouse capture reports them and no prompt wants them.
pub(crate) fn handle_nav(tui: &mut Tui, ev: &Event) -> bool {
    nav(&mut tui.chat, ev)
}

fn nav(chat: &mut ChatBuffer, ev: &Event) -> bool {
    match ev {
        Event::Mouse(m) => {
            match m.kind {
                MouseEventKind::ScrollUp => chat.scroll_up(WHEEL_LINES),
                MouseEventKind::ScrollDown => chat.scroll_down(WHEEL_LINES),
                _ => {}
            }
            true
        }
        Event::Key(KeyEvent {
            code: KeyCode::PageUp,
            ..
        }) => {
            chat.page_up();
            true
        }
        Event::Key(KeyEvent {
            code: KeyCode::PageDown,
            ..
        }) => {
            chat.page_down();
            true
        }
        _ => false,
    }
}
```
In `render_screen`, after `f.render_widget(Paragraph::new(lines), chat_area);` insert:

```rust
    if chat.unread() && chat_area.height > 0 {
        let row = Rect {
            y: chat_area.y + chat_area.height - 1,
            height: 1,
            ..chat_area
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(NEW_MESSAGE, theme::brand())).right_aligned()),
            row,
        );
    }
```
In `page_user_echo` add `tui.chat.jump_bottom();` before the push (submitting a message returns you to the live bottom — otherwise a scrolled-up user never sees their own message or the reply).

Loop hooks (exact anchors):
- `run.rs`: in the `maybe_ev = events.next() =>` arm, immediately before `let k = match ev {` insert `if crate::tui::page::handle_nav(&mut tui, &ev) { continue; }`.
- `ask.rs` locked loop: first statement inside `Some(Ok(ev)) = events.next() => {` (before the `// Paste is still processed...` comment) insert `if crate::tui::page::handle_nav(tui, &ev) { continue; }`.
- `ask.rs` confirm loop (`match events.next().await {` at ~L111), `entry.rs` (~L285) and `intro.rs` (~L100): insert as the FIRST arm of the `match events.next().await {`:
  `Some(Ok(ev)) if crate::tui::page::handle_nav(tui, &ev) => {}`
  (guard consumes the event; when it returns false the value is unmoved and the existing `Some(Ok(Event::Key(k)))` arm still matches.)

- [ ] **Step 4: Run, expect pass**

Run: `cargo test page::tests && cargo test`
Expected: new tests PASS; full suite PASS except the pre-existing `materials` test.

- [ ] **Step 5: Format and commit**

```bash
rustfmt --edition 2021 src/tui/page.rs src/tui/run.rs src/tui/ask.rs src/tui/entry.rs src/tui/intro.rs
wc -l src/tui/run.rs   # must stay <= 600
git add src/tui
git commit -m "feat(tui): wheel and PageUp/PageDown scroll the chat pane; new-message indicator"
```

---

## Task 5: Final verification, v0.33.0 bump, docs, manual checklist

**Files:**
- Modify: `Cargo.toml` (`version = "0.33.0"`), `Cargo.lock` (via `cargo build`), `src/tui/welcome_tests.rs` L8 (`"0.33.0"`), `README.md`, `SPEC.md`

Convention (from `6004da3`, "docs: v0.32.0 multiline input — SPEC, README, version bump"): a feature minor bump touches exactly those five files, in one docs commit after the feature commits. This is a user-visible behavior change (alternate screen, no scrollback), so minor bump 0.32.0 -> 0.33.0.

- [ ] **Step 1: Failing test first.** Edit `src/tui/welcome_tests.rs` `version_aligned_with_spec` to assert `"0.33.0"`. Run `cargo test version_aligned_with_spec`. Expected: FAIL (`left: "0.32.0"`, `right: "0.33.0"`).

- [ ] **Step 2: Bump and docs.**
  - `Cargo.toml`: `version = "0.33.0"`; then `cargo build` to refresh `Cargo.lock` (only the `usta` package version line changes — verify with `git diff Cargo.lock`).
  - `README.md` L36 (Terminal UI row): replace "a borderless live input area (a single rule above, fixed height)" with "a borderless multiline input (a single rule above, grows to 8 rows)" and replace the sentence "The bottom region is a small inline viewport pinned under the conversation — the flow stays in normal scrollback, so scroll up and copy." with "Fullscreen: usta owns the window and keeps the conversation in its own scrollable pane (mouse wheel / PageUp / PageDown; Shift+drag to select and copy)."
  - `README.md` L112: replace the paragraph with: "In an interactive terminal Usta opens a **fullscreen ratatui TUI** on the alternate screen: the conversation fills a scrollable pane, with the input box and status line pinned below it. Scroll with the mouse wheel or PageUp/PageDown; hold Shift while dragging to select and copy text (iTerm2/Ghostty/WezTerm). Leaving usta restores your previous terminal content — the conversation is not printed to scrollback, it lives in usta's own history."
  - `README.md` input-keys table: add rows `| Mouse wheel / PageUp / PageDown | scroll the chat pane |`.
  - `SPEC.md`: after the v0.32.0 paragraph add `**v0.33.0 (fullscreen):** ...` covering: alternate screen + mouse capture (`term::setup`/`restore`), exit clears (no transcript), `ChatBuffer` (`tui/chat.rs`) storing un-wrapped sources and re-wrapping on resize, `page*` push to the buffer, `render_screen` layout (chat / rule+1..=8 rows / status), scroll (wheel 3 lines, PageUp/PageDown = pane height - 1, follow + "↓ new message"), submitting jumps to the bottom, removed: CPR seed, `Tui::set_height`, `TrackedBackend`, trailing newline. Design: `docs/superpowers/specs/2026-10-26-fullscreen-design.md`. In the v0.32.0 paragraph append "(inline viewport and `Tui::set_height` superseded in v0.33.0)". Run `grep -n "inline" SPEC.md` and annotate only the lines that describe CURRENT behavior as inline; historical version paragraphs stay as they are.

- [ ] **Step 3: Full verification.**
  - `cargo test version_aligned_with_spec` -> PASS
  - `cargo test` -> PASS except `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt` (pre-existing; confirm it is the ONLY failure)
  - `cargo clippy --all-targets 2>&1 | tail -30` -> no new warnings in touched files (compare with `git stash`-free baseline: run it once on `main` first if unsure what is pre-existing; fix only warnings located in files this branch changed)
  - `cargo fmt --check` -> clean crate-wide
  - `wc -l src/tui/*.rs` -> every production file <= 600 lines (`run.rs` is the tight one)
  - `grep -rn "TrackedBackend\|set_height\|insert_before" src --include=*.rs` -> only the two pre-existing prose comments (`convert.rs` doc line, `polite.rs` ~L658) remain

- [ ] **Step 4: Manual check list (iTerm2 and Ghostty; report results, do not skip).**
  1. `cargo run` -> window clears, welcome panel at top of the pane, editor + status at the bottom.
  2. Have a chat long enough to overflow; mouse wheel scrolls 3 lines per notch; PageUp/PageDown move a page.
  3. While scrolled up, trigger a reply (or `/help`): "↓ new message" appears bottom-right of the pane; scrolling to the bottom clears it; the view did not jump.
  4. Submit a message while scrolled up -> pane jumps to the bottom.
  5. Shift+drag selects text and Cmd+C copies it.
  6. Resize the window (wider, narrower, very short): chat re-wraps, no ghost lines, welcome panel is clipped not garbled.
  7. Type 8+ lines in the editor: the box grows, the chat pane shrinks, nothing overlaps.
  8. Ctrl+C / Ctrl+D exit: previous terminal content is back, cursor visible, no stray blank line, shell usable (type `echo ok`).
  9. Force a panic (e.g. temporary `panic!()` behind an env var, then remove it) -> terminal restored, panic message readable on the normal screen.
  10. Pipe mode: `echo hi | cargo run` still falls back to plain mode (no escape codes).

- [ ] **Step 5: Commit.**

```bash
rustfmt --edition 2021 src/tui/welcome_tests.rs
git add Cargo.toml Cargo.lock README.md SPEC.md src/tui/welcome_tests.rs
git commit -m "docs: v0.33.0 fullscreen TUI — SPEC, README, version bump"
```

---

## Self-Review (against `2026-10-26-fullscreen-design.md`)

| Spec item | Where |
|---|---|
| Decision 1: mouse capture on, wheel scrolls, Shift+drag selects | Task 3 `EnableMouseCapture`; Task 4 `handle_nav`; Task 5 manual #5 |
| Decision 2: exit clears, no transcript | Task 3 `restore()`/`Drop` (no `println!`, guard `no_inline_viewport_remnants`) |
| Layout chat / editor / status + "↓ new message" | Task 2 `render_screen`; Task 4 indicator |
| `term.rs` Fullscreen, Enter/Leave alt screen, Enable/DisableMouseCapture, panic hook | Task 3 |
| Remove CPR seed, `set_height`, `TrackedBackend` seed; editor height = layout constraint | Task 3 (+ `backend_wrap.rs` deleted); Task 2 `box_h` constraint |
| `Drop` no trailing newline | Task 3 `drop_only_restores` |
| Chat buffer storing un-wrapped source, `scroll_offset`, `follow` | Task 1 |
| `page()`/`page_*` push to buffer, callers unchanged | Task 2 (only 2 welcome call sites renamed to `page_fixed`) |
| `draw()` splits frame | Tasks 2-3 |
| Events: wheel 3 lines, PageUp/PageDown = pane height - 1, bottom re-enables follow, unread flag | Tasks 1, 4 |
| Resize re-wrap, bottom-anchored when following, clamp otherwise | Task 1 (`visible` clamp, `offset_is_clamped_after_the_pane_grows`) |
| Unchanged: editor, slash, backend, plain mode | untouched; `plain.rs`/`ui.rs` not edited |
| Tests: buffer math, TestBackend 80x24 + narrow, source guards (Resize + Mouse in every loop; restore leaves alt screen + disables mouse) | Tasks 1, 2, 3, 4 |

Placeholder scan: none ("similar to" / TODO not used). Type consistency: `Source`/`ChatBuffer` signatures identical across tasks; `render_screen` signature identical in Tasks 2-4; `screen_rows` helper defined in Task 2 and reused in Task 4.

**Spec ambiguities / risks surfaced (decisions made in this plan, confirm or override):**
1. Spec says "re-wrap from source" but welcome panels are pre-sized boxes: they are stored as `Source::Fixed` (clipped on narrow terminals, not re-wrapped). Notices are char-split (not word-wrapped); replies/echo re-render via termimad / `wrap_cells` so they wrap at word boundaries.
2. Not in spec: submitting a message calls `jump_bottom()` (otherwise a scrolled-up user never sees their own echo). One line in `page_user_echo`.
3. Not in spec: `EnableMouseCapture` also reports mouse motion/clicks; `handle_nav` swallows them, but each one still triggers a redraw iteration (cheap with ratatui's diff).
4. `Tui` can no longer be built in unit tests (needs real stdout), so `page*` push wrappers are covered by source pins + `ChatBuffer` unit tests, not behavior tests.
5. Chat history is unbounded in memory and every push while scrolled up re-renders all entries twice (markdown re-render per width change). Fine for a session; add a cap/per-entry cache only if profiling shows it.
6. Tasks 2 and 3 must land together (app shows no history in between). The plan keeps tests green at each commit but the binary is not usable after Task 2 alone.
7. `run.rs` is 589 lines against the 600 budget; the plan adds net +1 line there.
