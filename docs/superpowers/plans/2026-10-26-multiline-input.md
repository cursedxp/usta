# Multiline Input Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace usta's single-string `tui-input` prompt with an opencode-style multiline editor whose box grows 1→8 rows.

**Architecture:** `ratatui-textarea` is used only as the editing engine (lines + `(row, col)` cursor + undo/redo + word ops); usta keeps its own key mapping, history and rendering (`> ` prefix, top rule, word wrap). The inline viewport height becomes dynamic: `page::draw` asks the editor for its row count and `Tui::set_height` rebuilds the inline `Terminal` at the old viewport's top row when the height changes.

**Tech Stack:** Rust 2021, ratatui 0.30, crossterm 0.29, ratatui-textarea 0.9.3 (`default-features = false, features = ["crossterm"]`), unicode-width 0.2.

## Global Constraints

- Branch `multiline-input` in `/Users/optimotion/Documents/Work/Projects/headspace/usta` — no worktree (disk space).
- `InputBox` public API kept: `new()`, `handle_key(KeyEvent) -> Action`, `insert_str(&str)`, `clear()`, `value()`, `render(&self, &mut Frame, Rect)`. `run.rs` / `paint.rs` / `ask.rs` / `entry.rs` / `intro.rs` call sites must not change.
- Ctrl+C / Ctrl+D stay `Action::Exit`.
- Max input rows: 8. Min: 1. Viewport height = rows + 2 (rule + status).
- Files stay under 600 production lines (CLAUDE.md / SPEC §11). `cargo fmt` only on edited files. `cargo clippy --all-targets -- -D warnings` clean.
- Out of scope: paste summary, persistent history, `/` popup, `@` mention, external editor.

---

### Task 1: Editing engine + opencode keymap + history rule

**Files:**
- Modify: `Cargo.toml` (ratatui-textarea already added by `cargo add` on this branch; remove `tui-input` at the end of this task)
- Modify: `src/tui/editor.rs` (struct, `handle_key`, `insert_str`, `clear`, `recall_*`; tests module)

**Interfaces:**
- Produces: `InputBox::value(&self) -> String` (was `&str`; `assert_eq!(b.value(), "x")` keeps compiling), `InputBox::cursor_rc(&self) -> (usize, usize)` (data row, char col — used by Task 2), `InputBox::lines(&self) -> &[String]`.

- [ ] **Step 1: Write failing tests** — append to `mod tests` in `src/tui/editor.rs`:

```rust
    fn mods(k: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(k, m)
    }

    #[test]
    fn shift_enter_and_ctrl_j_insert_real_lines() {
        let mut b = InputBox::new();
        type_str(&mut b, "bir");
        b.handle_key(mods(KeyCode::Enter, KeyModifiers::SHIFT));
        type_str(&mut b, "iki");
        b.handle_key(mods(KeyCode::Char('j'), KeyModifiers::CONTROL));
        type_str(&mut b, "üç");
        assert_eq!(b.value(), "bir\niki\nüç");
        assert_eq!(b.lines().len(), 3);
        assert_eq!(b.cursor_rc(), (2, 2));
    }

    #[test]
    fn up_moves_cursor_between_lines_before_history() {
        let mut b = InputBox::new();
        type_str(&mut b, "eski");
        b.handle_key(code(KeyCode::Enter));
        type_str(&mut b, "a");
        b.handle_key(mods(KeyCode::Enter, KeyModifiers::SHIFT));
        type_str(&mut b, "b");
        b.handle_key(code(KeyCode::Up)); // row 1 -> row 0, no history
        assert_eq!(b.value(), "a\nb");
        assert_eq!(b.cursor_rc().0, 0);
        b.handle_key(code(KeyCode::Up)); // row 0 -> history
        assert_eq!(b.value(), "eski");
        b.handle_key(code(KeyCode::Down)); // single line = last row -> back to stash
        assert_eq!(b.value(), "a\nb");
    }

    #[test]
    fn edited_recall_is_not_overwritten_by_up() {
        let mut b = InputBox::new();
        type_str(&mut b, "bir");
        b.handle_key(code(KeyCode::Enter));
        type_str(&mut b, "iki");
        b.handle_key(code(KeyCode::Enter));
        b.handle_key(code(KeyCode::Up));
        assert_eq!(b.value(), "iki");
        type_str(&mut b, "X");
        b.handle_key(code(KeyCode::Up));
        assert_eq!(b.value(), "ikiX");
    }

    #[test]
    fn emacs_line_and_word_keys() {
        let ctrl = KeyModifiers::CONTROL;
        let alt = KeyModifiers::ALT;
        let mut b = InputBox::new();
        type_str(&mut b, "merhaba güzel dünya");
        b.handle_key(mods(KeyCode::Char('w'), ctrl));
        assert_eq!(b.value(), "merhaba güzel ");
        b.handle_key(mods(KeyCode::Char('a'), ctrl));
        assert_eq!(b.cursor_rc(), (0, 0));
        b.handle_key(mods(KeyCode::Char('f'), alt));
        b.handle_key(mods(KeyCode::Char('k'), ctrl));
        assert_eq!(b.value(), "merhaba");
        b.handle_key(mods(KeyCode::Char('e'), ctrl));
        b.handle_key(mods(KeyCode::Char('u'), ctrl));
        assert_eq!(b.value(), "");
        b.handle_key(mods(KeyCode::Char('-'), ctrl)); // undo
        assert_eq!(b.value(), "merhaba");
        b.handle_key(mods(KeyCode::Char('.'), ctrl)); // redo
        assert_eq!(b.value(), "");
    }

    #[test]
    fn paste_keeps_lines_and_normalizes_crlf() {
        let mut b = InputBox::new();
        b.insert_str("a\r\nb\rc");
        assert_eq!(b.value(), "a\nb\nc");
        assert_eq!(b.cursor_rc(), (2, 1));
    }
```

Note on `alt+f` from "merhaba": `WordForward` lands at the start of the next word (col 8, after the space); `ctrl+k` then deletes "güzel " leaving "merhaba ". If the assertion shows `"merhaba "`, change the expected value to `"merhaba "` — the library's word boundary is the reference behaviour, not this test's guess; do not hand-roll word motion.

- [ ] **Step 2: Run, verify failure**

Run: `cargo test --lib tui::editor`
Expected: compile errors (`lines`, `cursor_rc` not found).

- [ ] **Step 3: Implement** — replace the struct + impl in `src/tui/editor.rs` (keep `Action`, `render`, `wrap_visual` for now; `render` gets rewritten in Task 2, temporarily feed it `&self.value()` and a char index):

```rust
use ratatui_textarea::{CursorMove, TextArea};

pub struct InputBox {
    ta: TextArea<'static>,
    history: Vec<String>,
    /// None = fresh text; Some(i) = history[i] is being shown.
    cursor: Option<usize>,
    /// Fresh text from before entering history — comes back with Down.
    stash: String,
}

/// TextArea holding `s`, cursor at the very end (recall / reset).
fn area_with(s: &str) -> TextArea<'static> {
    let mut ta = TextArea::new(s.split('\n').map(str::to_string).collect());
    ta.move_cursor(CursorMove::Bottom);
    ta.move_cursor(CursorMove::End);
    ta
}

impl InputBox {
    pub fn new() -> Self {
        Self { ta: TextArea::default(), history: Vec::new(), cursor: None, stash: String::new() }
    }

    pub fn value(&self) -> String {
        self.ta.lines().join("\n")
    }
    pub fn lines(&self) -> &[String] {
        self.ta.lines()
    }
    /// (data row, char column) of the cursor.
    pub fn cursor_rc(&self) -> (usize, usize) {
        let c = self.ta.cursor();
        (c.0, c.1)
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        let m = key.modifiers;
        let ctrl = m.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('d')) {
            return Action::Exit;
        }
        // Newline: Shift/Ctrl/Alt+Enter (kitty protocol) or Ctrl+J (universal). Bare Enter submits.
        let newline = (key.code == KeyCode::Enter
            && m.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::CONTROL))
            || (ctrl && key.code == KeyCode::Char('j'));
        if newline {
            self.ta.insert_newline();
            return Action::None;
        }
        let (row, _) = self.cursor_rc();
        let last_row = self.ta.lines().len() - 1;
        let shift = m.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Enter => {
                let line = self.value().trim().to_string();
                if line.is_empty() {
                    return Action::None;
                }
                self.history.push(line.clone());
                self.clear();
                Action::Submit(line)
            }
            // opencode: Up/Down move the cursor inside multiline text; history only from the edge row.
            KeyCode::Up if shift || row > 0 => {
                self.move_to(CursorMove::Up, shift);
                Action::None
            }
            KeyCode::Down if shift || row < last_row => {
                self.move_to(CursorMove::Down, shift);
                Action::None
            }
            KeyCode::Up => {
                self.recall_prev();
                Action::None
            }
            KeyCode::Down => {
                self.recall_next();
                Action::None
            }
            // Edits keep `self.cursor`: `edited_recall()` compares the text instead.
            _ => {
                self.edit(key);
                Action::None
            }
        }
    }

    fn move_to(&mut self, mv: CursorMove, select: bool) {
        if select {
            if !self.ta.is_selecting() {
                self.ta.start_selection();
            }
        } else {
            self.ta.cancel_selection();
        }
        self.ta.move_cursor(mv);
    }

    /// opencode 2.1.4 default input keybinds (Ctrl+C/D handled by the caller).
    fn edit(&mut self, key: KeyEvent) {
        let m = key.modifiers;
        let ctrl = m.contains(KeyModifiers::CONTROL);
        let alt = m.contains(KeyModifiers::ALT);
        let shift = m.contains(KeyModifiers::SHIFT);
        let mv = match key.code {
            KeyCode::Left if ctrl || alt => Some(CursorMove::WordBack),
            KeyCode::Right if ctrl || alt => Some(CursorMove::WordForward),
            KeyCode::Char('b' | 'B') if alt => Some(CursorMove::WordBack),
            KeyCode::Char('f' | 'F') if alt => Some(CursorMove::WordForward),
            KeyCode::Left => Some(CursorMove::Back),
            KeyCode::Right => Some(CursorMove::Forward),
            KeyCode::Char('b') if ctrl => Some(CursorMove::Back),
            KeyCode::Char('f') if ctrl => Some(CursorMove::Forward),
            KeyCode::Char('a' | 'A') if ctrl => Some(CursorMove::Head),
            KeyCode::Char('e' | 'E') if ctrl => Some(CursorMove::End),
            KeyCode::Home if shift => Some(CursorMove::Top),
            KeyCode::End if shift => Some(CursorMove::Bottom),
            KeyCode::Home => Some(CursorMove::Head),
            KeyCode::End => Some(CursorMove::End),
            _ => None,
        };
        if let Some(mv) = mv {
            // Shift+Home/End = select to buffer start/end; Ctrl+Shift+A/E = select to line edge.
            let select = shift && !matches!(key.code, KeyCode::Char('B' | 'F'));
            self.move_to(mv, select);
            return;
        }
        let ta = &mut self.ta;
        match key.code {
            KeyCode::Backspace if ctrl || alt => {
                ta.delete_word();
            }
            KeyCode::Backspace => {
                ta.delete_char();
            }
            KeyCode::Delete if ctrl => {
                ta.delete_next_word();
            }
            KeyCode::Delete => {
                ta.delete_next_char();
            }
            KeyCode::Char('d') if alt => {
                ta.delete_next_word();
            }
            KeyCode::Char('w') if ctrl => {
                ta.delete_word();
            }
            KeyCode::Char('k') if ctrl => {
                ta.delete_line_by_end();
            }
            KeyCode::Char('u') if ctrl => {
                ta.delete_line_by_head();
            }
            KeyCode::Char('-') if ctrl => {
                ta.undo();
            }
            KeyCode::Char('.') if ctrl => {
                ta.redo();
            }
            KeyCode::Esc => ta.cancel_selection(),
            KeyCode::Char(c) if !ctrl && !alt => ta.insert_char(c),
            _ => {}
        }
    }

    /// Drop any half-typed text (confirm cancelled — don't leak it into the next prompt).
    pub fn clear(&mut self) {
        self.ta = TextArea::default();
        self.cursor = None;
    }

    /// Insert pasted text at the cursor (bracketed paste). Line breaks are PRESERVED as
    /// real lines; CRLF / CR → LF.
    pub fn insert_str(&mut self, s: &str) {
        let cleaned = s.replace("\r\n", "\n").replace('\r', "\n");
        self.ta.insert_str(cleaned);
    }

    /// True when the shown text was edited after a recall — opencode won't overwrite it.
    fn edited_recall(&self) -> bool {
        matches!(self.cursor, Some(i) if self.history[i] != self.value())
    }

    fn recall_prev(&mut self) {
        if self.history.is_empty() || self.edited_recall() {
            return;
        }
        let next = match self.cursor {
            None => {
                self.stash = self.value();
                self.history.len() - 1
            }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.cursor = Some(next);
        self.ta = area_with(&self.history[next]);
    }

    fn recall_next(&mut self) {
        if self.edited_recall() {
            return;
        }
        match self.cursor {
            None => {}
            Some(i) if i + 1 < self.history.len() => {
                self.cursor = Some(i + 1);
                self.ta = area_with(&self.history[i + 1]);
            }
            Some(_) => {
                self.cursor = None;
                self.ta = area_with(&std::mem::take(&mut self.stash));
            }
        }
    }
```

`self.cursor` is reset only by `clear()` (Enter, cancel). Typing over a recalled entry keeps it, so `edited_recall()` sees the change and Up/Down won't overwrite the edit.

Temporary render bridge (until Task 2): in `render`, replace `self.input.value()` / `self.input.visual_cursor()` with:

```rust
        let value = self.value();
        let (r, c) = self.cursor_rc();
        let idx = self.ta.lines()[..r].iter().map(|l| l.chars().count() + 1).sum::<usize>() + c;
        let (rows, cur_row, cur_col) = wrap_visual(&value, inner_w, idx);
```

Delete `visual_cursor()` and the `tui_input` imports. Remove `tui-input = "0.15"` from `Cargo.toml`.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib tui::editor`
Expected: all pass (old tests + 5 new). If `emacs_line_and_word_keys` differs only by the word-boundary note above, adjust that expectation once and re-run.

- [ ] **Step 5: Full suite + clippy, commit**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt -- src/tui/editor.rs`
Expected: PASS, no warnings.

```bash
git add Cargo.toml Cargo.lock src/tui/editor.rs
git commit -m "feat(editor): multiline engine on ratatui-textarea, opencode keymap"
```

---

### Task 2: Word-wrapped render + `desired_rows`

**Files:**
- Modify: `src/tui/editor.rs` (`render`, replace `wrap_visual` with `wrap_rows`, add `desired_rows`; tests)

**Interfaces:**
- Consumes: `lines()`, `cursor_rc()` from Task 1.
- Produces: `InputBox::desired_rows(&self, width: u16) -> u16` — visual text rows for a box `width` cells wide, clamped to `1..=MAX_INPUT_ROWS`; `pub const MAX_INPUT_ROWS: u16 = 8;` in `editor.rs`.

- [ ] **Step 1: Failing tests** — replace the existing `wrap_visual` tests with:

```rust
    #[test]
    fn wrap_breaks_at_word_boundary() {
        let lines = vec!["aaa bbb ccc".to_string()];
        let (rows, r, c) = wrap_rows(&lines, 8, (0, 11));
        assert_eq!(rows, vec!["aaa bbb ".to_string(), "ccc".to_string()]);
        assert_eq!((r, c), (1, 3));
    }

    #[test]
    fn wrap_splits_a_word_longer_than_the_row() {
        let lines = vec!["abcdefghij".to_string()];
        let (rows, _, _) = wrap_rows(&lines, 4, (0, 0));
        assert_eq!(rows, vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn wrap_counts_wide_chars_by_cell() {
        let lines = vec!["日本語".to_string()]; // 6 cells
        let (rows, r, c) = wrap_rows(&lines, 4, (0, 3));
        assert_eq!(rows, vec!["日本", "語"]);
        assert_eq!((r, c), (1, 2));
    }

    #[test]
    fn wrap_keeps_logical_lines_and_turkish_cursor() {
        let lines = vec!["şeker".to_string(), "çay".to_string()];
        let (rows, r, c) = wrap_rows(&lines, 20, (1, 2));
        assert_eq!(rows, vec!["şeker", "çay"]);
        assert_eq!((r, c), (1, 2));
    }

    #[test]
    fn desired_rows_grows_and_caps_at_eight() {
        let mut b = InputBox::new();
        assert_eq!(b.desired_rows(40), 1);
        b.insert_str("1\n2\n3");
        assert_eq!(b.desired_rows(40), 3);
        b.insert_str(&"\nx".repeat(20));
        assert_eq!(b.desired_rows(40), MAX_INPUT_ROWS);
    }

    #[test]
    fn render_rule_prefix_and_cursor() {
        use ratatui::{backend::TestBackend, Terminal};
        let mut b = InputBox::new();
        b.insert_str("ab\ncd");
        let mut t = Terminal::new(TestBackend::new(20, 4)).unwrap();
        t.draw(|f| b.render(f, f.area())).unwrap();
        let buf = t.backend().buffer();
        let row = |y: u16| (0..20).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>();
        assert!(row(0).chars().all(|c| c == '─'));
        assert!(row(1).starts_with("> ab"));
        assert!(row(2).starts_with("  cd"));
        t.backend_mut().assert_cursor_position((4, 2));
    }
```

(If the existing `render_rows` helper in this test module already does the `row` closure's job, use it instead of the closure.)

- [ ] **Step 2: Run, verify failure**

Run: `cargo test --lib tui::editor`
Expected: compile error — `wrap_rows`, `desired_rows`, `MAX_INPUT_ROWS` missing.

- [ ] **Step 3: Implement** — delete `wrap_visual`; add:

```rust
/// Most text rows the box shows before it scrolls (opencode-style growth, 1..=8).
pub const MAX_INPUT_ROWS: u16 = 8;

fn cell(ch: char) -> usize {
    use unicode_width::UnicodeWidthChar;
    ch.width().unwrap_or(0).max(1)
}

/// Visual-row start indices for one logical line: break at the last space that fits,
/// or mid-word when a single word is wider than the row. Width is cell-based.
fn row_starts(chars: &[char], width: usize) -> Vec<usize> {
    let mut starts = vec![0];
    let (mut w, mut last_space) = (0usize, None::<usize>);
    let mut i = 0;
    while i < chars.len() {
        let start = *starts.last().expect("starts boş olamaz");
        if w + cell(chars[i]) > width && i > start {
            let s = match last_space {
                Some(sp) if sp + 1 > start && sp < i => sp + 1,
                _ => i,
            };
            starts.push(s);
            w = chars[s..i].iter().map(|&c| cell(c)).sum();
            last_space = None;
            continue;
        }
        if chars[i] == ' ' {
            last_space = Some(i);
        }
        w += cell(chars[i]);
        i += 1;
    }
    starts
}

/// Wrap logical lines into visual rows; also map the data cursor (row, char col)
/// to (visual row, cell column). Pure — render draws from this, the value is unchanged.
fn wrap_rows(lines: &[String], width: usize, cursor: (usize, usize)) -> (Vec<String>, usize, usize) {
    let width = width.max(1);
    let mut rows = Vec::new();
    let (mut cur_row, mut cur_col) = (0, 0);
    for (r, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let starts = row_starts(&chars, width);
        for (k, &s) in starts.iter().enumerate() {
            let e = starts.get(k + 1).copied().unwrap_or(chars.len());
            if r == cursor.0 && cursor.1 >= s && (cursor.1 < e || k + 1 == starts.len()) {
                cur_row = rows.len();
                cur_col = chars[s..cursor.1].iter().map(|&c| cell(c)).sum();
            }
            rows.push(chars[s..e].iter().collect());
        }
    }
    (rows, cur_row, cur_col)
}
```

Add to `impl InputBox`:

```rust
    /// Text rows this box wants at `width` (prefix deducted), clamped to 1..=MAX_INPUT_ROWS.
    pub fn desired_rows(&self, width: u16) -> u16 {
        let inner = width.saturating_sub(2) as usize;
        let (rows, _, _) = wrap_rows(self.ta.lines(), inner, self.cursor_rc());
        (rows.len() as u16).clamp(1, MAX_INPUT_ROWS)
    }
```

In `render`, replace the Task 1 bridge with:

```rust
        let (rows, cur_row, cur_col) = wrap_rows(self.ta.lines(), inner_w, self.cursor_rc());
```

and update the doc comments (`render`: "rows break at word boundaries; logical lines are real lines, no ⏎ marker"). The rest of `render` (window, prefix, rule, cursor clamp) stays.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib tui::editor`
Expected: PASS. Old tests that asserted the `⏎` marker are obsolete — delete them (their behaviour was removed by the spec), and say so in the commit body.

- [ ] **Step 5: Commit**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt -- src/tui/editor.rs`

```bash
git add src/tui/editor.rs
git commit -m "feat(editor): word-wrapped render and desired_rows (1..=8)"
```

---

### Task 3: Growing viewport

**Files:**
- Modify: `src/tui/term.rs` (drop `VIEWPORT_H` const, add `Tui.height`, `viewport_height`, `Tui::set_height`)
- Modify: `src/tui/page.rs:90-110` (`draw`)

**Interfaces:**
- Consumes: `InputBox::desired_rows(width) -> u16`.
- Produces: `term::viewport_height(rows: u16) -> u16` (= rows + 2), `Tui::set_height(&mut self, h: u16) -> Result<()>`, `Tui::height(&self) -> u16`.

- [ ] **Step 1: Failing tests** — in `src/tui/term.rs` add:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_is_rule_plus_rows_plus_status() {
        assert_eq!(viewport_height(1), 3);
        assert_eq!(viewport_height(8), 10);
    }
}
```

and in `src/tui/page.rs` tests module add a source pin (same pattern as `resize_events_are_handled_in_every_event_loop`):

```rust
    #[test]
    fn draw_resizes_the_viewport_before_painting() {
        let src = include_str!("page.rs");
        let draw = src.split("pub(crate) fn draw(").nth(1).expect("draw var");
        let set = draw.find("set_height(").expect("draw set_height çağırmalı");
        let paint = draw.find("terminal.draw(").expect("draw terminal.draw çağırmalı");
        assert!(set < paint, "yükseklik çizimden önce ayarlanmalı");
    }
```

- [ ] **Step 2: Run, verify failure**

Run: `cargo test --lib tui::term tui::page`
Expected: FAIL / compile error (`viewport_height` missing).

- [ ] **Step 3: Implement** — `src/tui/term.rs`:

```rust
use crate::tui::editor::MAX_INPUT_ROWS;

/// Bottom region = top rule + `rows` text rows + status line.
pub fn viewport_height(rows: u16) -> u16 {
    rows.clamp(1, MAX_INPUT_ROWS) + 2
}

pub struct Tui {
    pub terminal: Terminal<TrackedBackend<Stdout>>,
    height: u16,
}
```

`setup()`: build with `Viewport::Inline(viewport_height(1))` and return `Tui { terminal, height: viewport_height(1) }`. Extract the `Terminal::with_options(...)` call into:

```rust
fn inline_terminal(seed: Position, h: u16) -> Result<Terminal<TrackedBackend<Stdout>>> {
    Ok(Terminal::with_options(
        TrackedBackend::new(CrosstermBackend::new(std::io::stdout()), seed),
        TerminalOptions { viewport: Viewport::Inline(h) },
    )?)
}
```

Add:

```rust
impl Tui {
    pub fn height(&self) -> u16 {
        self.height
    }

    /// ratatui 0.30 has no API to change an inline viewport's height. Clear the old
    /// region, then rebuild the inline terminal seeded at the old viewport's top row —
    /// growing near the bottom scrolls the screen up (ratatui's own inline placement),
    /// shrinking leaves the region top-anchored. No CPR query: the seed is known.
    pub fn set_height(&mut self, h: u16) -> Result<()> {
        if h == self.height {
            return Ok(());
        }
        let top = self.terminal.get_frame().area().y;
        self.terminal.clear()?;
        self.terminal = inline_terminal(Position { x: 0, y: top }, h)?;
        self.height = h;
        Ok(())
    }
}
```

`src/tui/page.rs` — import `crate::tui::term::{viewport_height, Tui}` (drop `VIEWPORT_H`), and `draw` becomes:

```rust
    let width = current_width(tui);
    tui.set_height(viewport_height(editor.desired_rows(width)))?;
    let box_h = tui.height() - 1;
    tui.terminal.draw(|f| {
        let [box_area, status_area] =
            Layout::vertical([Constraint::Length(box_h), Constraint::Length(1)]).areas(f.area());
```

(rest unchanged). Update `term.rs` module/const doc comments: "Bottom region: rule + 1..=8 input rows + status line, height follows the text".

- [ ] **Step 4: Run tests**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt -- src/tui/term.rs src/tui/page.rs`
Expected: PASS. `grep -rn VIEWPORT_H src` → no hits.

- [ ] **Step 5: Manual check in a real terminal** (no automated VT emulator exists in this repo)

Run: `cargo run` in Terminal/iTerm. Check: (a) type a long sentence → box grows row by row, words don't split; (b) Shift+Enter / Ctrl+J ×10 → box stops at 8 rows and scrolls; (c) Enter → box back to 1 row, no leftover rows above the status line; (d) while the box is 5 rows, narrow then widen the window → no ghost copies of the box; (e) ↑ on row 2 moves to row 1, ↑ again recalls history.
If (c) or (d) leaves ghost rows: STOP and report — do not patch around it; fallback per spec is a fixed max-height viewport (decision returns to Anil).

- [ ] **Step 6: Commit**

```bash
git add src/tui/term.rs src/tui/page.rs
git commit -m "feat(tui): viewport height follows the input (rule + 1..=8 rows + status)"
```

---

### Task 4: Docs, version, merge, cleanup

**Files:**
- Modify: `Cargo.toml` (`version = "0.32.0"`), `SPEC.md` (new `**v0.32.0 (multiline input):**` paragraph after v0.31.3 at ~line 244), `README.md` (input keys section, if one exists — `grep -n "Shift+Enter\|Ctrl+J" README.md`)

- [ ] **Step 1: SPEC paragraph** — one paragraph: engine = ratatui-textarea 0.9.3 (editing only, usta renders); opencode 2.1.4 keymap table reference to the design doc; Up/Down cursor-first rule + edited-recall rule; word wrap; box grows 1..=8, viewport = rows+2 rebuilt at old top via `Tui::set_height`; `tui-input` removed. Link `docs/superpowers/specs/2026-10-26-multiline-input-design.md`. Also mark the v0.31.3 sentence "box height and `VIEWPORT_H = 6` are unchanged" as superseded by v0.32.0 (append "(superseded in v0.32.0)").
- [ ] **Step 2: README** — replace any "Shift+Enter / Ctrl+J newline" line with the key table from the design doc (Enter, newline keys, Up/Down, Ctrl+A/E/K/U/W, Alt+F/B, Ctrl+-/.).
- [ ] **Step 3: Verify + commit**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`

```bash
git add Cargo.toml Cargo.lock SPEC.md README.md
git commit -m "docs: v0.32.0 multiline input — SPEC, README, version bump"
```

- [ ] **Step 4: Merge + cleanup (after Anil's manual OK)**

```bash
git switch main && git merge --ff-only multiline-input
git branch -d multiline-input
cargo clean            # target/ — Rust build output, GBs
git branch --merged main | grep -v '^\*\|main$'   # list only; delete after Anil confirms
```
