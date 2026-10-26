# Polish C (fullscreen polish) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the fullscreen rough edges (panels re-render on resize, word-wrapped notices, bounded chat memory, no double render, indicator off the last chat line, no redraw on bare mouse motion, quiet pdftotext, stale comments) and pay down structure debt (`run.rs` split, testable `page*` bodies).

**Architecture:** `run.rs` loses its topic-resolution block to a new `tui/topic_flow.rs`. `ChatBuffer` (tui/chat.rs) gains word-aware wrapping with per-row soft flags, an incremental render cache, an entry cap, closure-backed panels and the bodies of the `page_*` wrappers (which become one-line delegations). The "new message" indicator moves to the editor's rule row. The terminal switches from any-motion mouse tracking to button-event tracking.

**Tech Stack:** Rust 2021, ratatui 0.29-style API (`Frame`, `Buffer`, `TestBackend`), crossterm, `unicode-width`.

Spec: `docs/superpowers/specs/2026-10-26-polish-design.md` sections C1-C10.

## Global Constraints

- Production code per file (everything above the first `#[cfg(test)]`) stays at or under 600 lines (SPEC §11). Count: `grep -n '^#\[cfg(test)\]' FILE | head -1` (no test module: `wc -l`).
- `rustfmt --edition 2021 <edited files>` only; `cargo fmt --check` must stay clean crate-wide.
- `cargo clippy --all-targets -- -D warnings` clean.
- `cargo test`: only allowed failure is `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt`. The crate is bin-only: use `cargo test <filter>`, never `--lib`.
- NO emoji anywhere in UI text (no U+1F300-1FAFF).
- Surgical changes; match existing style; existing source pins are updated only where this plan says so.
- Chat cap: `MAX_ENTRIES = 1000`, trim the oldest `TRIM_ENTRIES = 100` when exceeded.
- Mouse mode: `?1000h ?1002h ?1006h` only; the string `1003h` must not appear in `src/tui/term.rs` production code.
- Commits: add files by name (never `git commit -a`); message ends with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- Paths are relative to the worktree root `/Users/optimotion/Documents/Work/Projects/headspace/usta-polish-c`.

---

### Task 1: Split `run.rs` — topic resolution moves to `tui/topic_flow.rs` (C9)

**Files:**
- Create: `src/tui/topic_flow.rs`
- Modify: `src/tui/mod.rs` (add `pub mod topic_flow;` in alphabetical order)
- Modify: `src/tui/run.rs` (remove the topic-resolution block, call `resolve_topic`)
- Modify: `src/tui/intro.rs` test `run_wires_intro_flow` (needles that moved)
- Modify: `src/slash.rs` test that pins `crate::slash::is_context_command(&raw)` in `tui/run.rs` (now in `tui/topic_flow.rs`)
- Commit also: `docs/superpowers/plans/2026-10-26-polish-c-polish.md` (untracked in the worktree)

**Interfaces:**
- Produces:

```rust
/// Outcome of topic resolution (before the lock / build_session).
pub(crate) struct TopicPick {
    pub topic: String,
    pub had_topic_arg: bool,
    pub resumed: bool,
    pub intro: Option<String>,
    pub intro_turns: Option<Vec<crate::tui::intro::IntroTurn>>,
    pub first_run_completed: bool,
}

/// `Ok(None)` = the user quit without giving a topic.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn resolve_topic(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
    backend: &mut Backend,
    global: &Path,
    project_root: &Path,
    today: &str,
    topic_arg: Option<String>,
) -> Result<Option<TopicPick>>
```

This is a pure move: behavior, comments and text stay byte-identical except for the mechanical `&mut tui` → `tui` / `&mut editor` → `editor` / `&mut events` → `events` re-borrows and the final packaging.

- [ ] **Step 1: Baseline.** Run `cargo test 2>&1 | grep "test result"` and note the pass count.

- [ ] **Step 2: Create `src/tui/topic_flow.rs`.** Module doc: `//! Topic resolution for the TUI session: CLI argument, first-run introduction, conversational suggestion or the manual entry loop. Split out of run.rs (SPEC §11 budget).` Move into `resolve_topic` everything in `run.rs::run` from the comment `// Determine the topic: if an argument was given, slugify it locally` through the end of the `let topic = match topic_arg { ... };` statement (currently `src/tui/run.rs` ~lines 49-285), including the `let had_topic_arg`, `let mut resumed`, `let mut intro`, `let mut intro_turns`, `let mut first_run_completed` declarations with their comments. Also define the local `let read = |p: PathBuf| std::fs::read_to_string(p).ok();` inside `resolve_topic` (it is used by the block). Every `return Ok(None)` inside the block stays `return Ok(None)`. End the fn with:

```rust
    Ok(Some(TopicPick {
        topic,
        had_topic_arg,
        resumed,
        intro,
        intro_turns,
        first_run_completed,
    }))
```

Imports needed (add only what the compiler asks for): `std::path::{Path, PathBuf}`, `anyhow::Result`, `crossterm::event::EventStream`, `crate::anthropic::Message`, `crate::backend::Backend`, `crate::tui::editor::InputBox`, `crate::tui::term::Tui`, `crate::{history, progress}`.

- [ ] **Step 3: Rewire `run.rs`.** Replace the moved block with:

```rust
    let Some(crate::tui::topic_flow::TopicPick {
        topic,
        had_topic_arg,
        resumed,
        intro,
        intro_turns,
        first_run_completed,
    }) = crate::tui::topic_flow::resolve_topic(
        &mut tui,
        &mut editor,
        &mut events,
        backend,
        global,
        project_root,
        today,
        topic_arg,
    )
    .await?
    else {
        return Ok(None); // quit without giving a topic
    };
```

Keep `let read = ...` in `run.rs` (still used by the welcome block). Remove imports that become unused in `run.rs` (`history`, `Message` if unused — let clippy tell you). Update the `run.rs` module doc's sibling list to mention `topic_flow` (topic resolution).

- [ ] **Step 4: Update the source pins that followed the moved code.**

`src/tui/intro.rs` `run_wires_intro_flow`: check the four flow needles in `topic_flow.rs`, keep the stitch and `mark_intro_done` needles on `run.rs`:

```rust
    #[test]
    fn run_wires_intro_flow() {
        let flow = include_str!("topic_flow.rs");
        for needle in ["intro_needed", "run_first_run", "run_suggest", "IntroOutcome"] {
            assert!(flow.contains(needle), "topic_flow.rs lost its intro wiring: {needle}");
        }
        let src = include_str!("run.rs");
        for needle in [
            // (keep the two existing stitch / mark_intro_done needles and their comments verbatim)
        ] {
            assert!(src.contains(needle), "run.rs lost its intro wiring: {needle}");
        }
        for s in [src, flow] {
            assert!(!s.contains("start_suggest_system"));
            assert!(!s.contains("parse_start_suggestion"));
        }
    }
```

(Fill the second array with the two existing long needles, unchanged, and keep the doc comment above the test.)

`src/slash.rs` (~line 266): change `for src in [include_str!("tui/run.rs"), include_str!("tui/intro.rs")]` to `for src in [include_str!("tui/topic_flow.rs"), include_str!("tui/intro.rs")]`.

- [ ] **Step 5: Verify.** `cargo test` — same pass count as Step 1 (allowed failure only); `cargo clippy --all-targets -- -D warnings`; `rustfmt --edition 2021 src/tui/topic_flow.rs src/tui/run.rs src/tui/intro.rs src/slash.rs src/tui/mod.rs`; `cargo fmt --check`. Report production lines: `run.rs` (expect ~360) and `topic_flow.rs` (expect ~250).

- [ ] **Step 6: Commit**

```bash
git add docs/superpowers/plans/2026-10-26-polish-c-polish.md src/tui/topic_flow.rs src/tui/mod.rs src/tui/run.rs src/tui/intro.rs src/slash.rs
git commit -m "refactor(tui): move topic resolution out of run.rs into topic_flow.rs

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Word wrap with soft-row flags (C2)

**Files:**
- Modify: `src/tui/chat.rs` (`wrap_line`, `clip_line`, `wrap_text`, `render`, `flat`, `cache`)

**Interfaces:**
- Produces: `fn wrap_line(line: &Line<'static>, w: usize) -> (Vec<Line<'static>>, Vec<bool>)` — `soft[i]` is true when row i continues the previous logical line (row 0 of every logical line is false). `fn render(src: &Source, w: u16) -> (Vec<Line<'static>>, Vec<bool>)`. `ChatBuffer.cache: Option<(u16, Vec<Line<'static>>, Vec<bool>)>` (lines and soft always the same length). Plan B consumes the soft flags.

Wrapping rule: walk characters with display width (`unicode-width`). When the next char would overflow the row (`used + cw > w && used > 0`):
1. if that char is a space: append it to the current row (it hangs past the edge, invisible) and break after it;
2. else if the current row contains a space: break right after the LAST space of the row (the space stays on the upper row) and carry the rest of the row to the next row;
3. else char-split before this char.
Span styles are preserved per character (consecutive chars with the same style regroup into one span). `line.style` is copied to every row.

- [ ] **Step 1: Failing tests** (replace `wrap_line_splits_by_display_width_and_keeps_style`, keep `plain` helper; `clip_line` callers use `.0`):

```rust
    fn rows(l: &Line<'static>, w: usize) -> (Vec<String>, Vec<bool>) {
        let (lines, soft) = wrap_line(l, w);
        (plain(&lines), soft)
    }

    #[test]
    fn wrap_line_char_splits_a_word_wider_than_the_row() {
        let red = Style::default().fg(Color::Red);
        let (lines, soft) = wrap_line(&Line::from(Span::styled("abcdef", red)), 3);
        assert_eq!(plain(&lines), vec!["abc", "def"]);
        assert_eq!(soft, vec![false, true]);
        assert!(lines.iter().all(|r| r.spans.iter().all(|s| s.style == red)));
    }

    #[test]
    fn wrap_line_breaks_after_the_last_space() {
        let (r, soft) = rows(&Line::raw("the quick brown fox"), 10);
        assert_eq!(r, vec!["the quick ", "brown fox"]);
        assert_eq!(soft, vec![false, true]);
    }

    #[test]
    fn wrap_line_hangs_an_overflowing_space_on_the_upper_row() {
        let (r, _) = rows(&Line::raw("abc def"), 3);
        assert_eq!(r, vec!["abc ", "def"]);
    }

    #[test]
    fn wrap_line_keeps_wide_chars_whole_and_styles_across_breaks() {
        assert_eq!(rows(&Line::raw("日本語"), 4).0, vec!["日本", "語"]);
        let b = Style::default().fg(Color::Blue);
        let l = Line::from(vec![Span::raw("aa "), Span::styled("bbbb", b)]);
        let (lines, _) = wrap_line(&l, 5);
        assert_eq!(plain(&lines), vec!["aa ", "bbbb"]);
        assert_eq!(lines[1].spans[0].style, b);
        assert_eq!(wrap_line(&Line::raw(""), 10).0.len(), 1);
    }

    #[test]
    fn multi_line_text_marks_only_continuations_soft() {
        let mut c = ChatBuffer::default();
        c.push(Source::Text(Text::raw("one two three\nfour")));
        c.visible(8, 10);
        let (_, _, soft) = c.cache.as_ref().unwrap();
        assert_eq!(soft, &vec![false, true, false]);
    }
```

- [ ] **Step 2: Run** `cargo test tui::chat` — expect compile failure.

- [ ] **Step 3: Implement** in `src/tui/chat.rs`:

```rust
/// Word-aware wrap of one line at `w` display columns, keeping span styles.
/// Returns the rows and, per row, whether it continues the previous one (soft).
fn wrap_line(line: &Line<'static>, w: usize) -> (Vec<Line<'static>>, Vec<bool>) {
    let w = w.max(1);
    let cells: Vec<(char, Style)> = line
        .spans
        .iter()
        .flat_map(|s| s.content.chars().map(move |c| (c, s.style)))
        .collect();
    let width = |r: &[(char, Style)]| r.iter().map(|(c, _)| c.width().unwrap_or(0)).sum::<usize>();
    let mut rows: Vec<Vec<(char, Style)>> = Vec::new();
    let mut cur: Vec<(char, Style)> = Vec::new();
    for &(ch, st) in &cells {
        let cw = ch.width().unwrap_or(0);
        if width(&cur) + cw > w && !cur.is_empty() {
            if ch == ' ' {
                cur.push((ch, st));
                rows.push(std::mem::take(&mut cur));
                continue;
            }
            match cur.iter().rposition(|(c, _)| *c == ' ') {
                Some(i) if i + 1 < cur.len() => {
                    let rest = cur.split_off(i + 1);
                    rows.push(std::mem::replace(&mut cur, rest));
                }
                _ => rows.push(std::mem::take(&mut cur)),
            }
        }
        cur.push((ch, st));
    }
    rows.push(cur);
    let soft = (0..rows.len()).map(|i| i > 0).collect();
    let lines = rows.into_iter().map(|r| to_line(r, line.style)).collect();
    (lines, soft)
}

/// Regroup (char, style) cells into spans of equal style.
fn to_line(cells: Vec<(char, Style)>, style: Style) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (c, st) in cells {
        match spans.last_mut() {
            Some(s) if s.style == st => s.content.to_mut().push(c),
            _ => spans.push(Span::styled(c.to_string(), st)),
        }
    }
    Line::from(spans).style(style)
}
```

Note the `Some(i) if i + 1 < cur.len()` guard: when the last char of the row is the space, the row already ends at a break, so it is pushed whole. Import `ratatui::style::Style` (production). `width(&cur)` recomputation is O(row) per char; acceptable at chat sizes — if clippy or a reviewer prefers, keep a running `used` that is recomputed only after a split.

`clip_line` → `wrap_line(line, w).0.swap_remove(0)`. `wrap_text` returns `(Vec<Line>, Vec<bool>)` by extending both vectors per logical line. `render` returns `(Vec<Line>, Vec<bool>)`; the `Fixed` arm returns `soft = vec![false; n]`. `flat` fills `cache = Some((width, lines, soft))` and returns `&self.cache...1`. Update the `Source::Text` doc comment to "Word-wrapped on width change".

- [ ] **Step 4:** `cargo test tui::chat` then full `cargo test`; clippy; rustfmt `src/tui/chat.rs`.

- [ ] **Step 5: Commit** `git add src/tui/chat.rs` — message `feat(tui): word-wrap chat notices and record soft-wrapped rows`.

---

### Task 3: Incremental render cache and entry cap (C4 + C3)

**Files:**
- Modify: `src/tui/chat.rs` (`push`, constants, test-only render counter)

**Interfaces:**
- Consumes: `render(&Source, u16) -> (Vec<Line>, Vec<bool>)`, cache triple from Task 2.
- Produces: `pub const MAX_ENTRIES: usize = 1000;`, `const TRIM_ENTRIES: usize = 100;`. `push` keeps its signature. Plan B will add "clear the selection" at the trim site — put the trim in one private method `fn trim(&mut self)`.

- [ ] **Step 1: Failing tests**

```rust
    #[test]
    fn push_while_scrolled_renders_only_the_new_entry() {
        let mut c = filled(20);
        c.visible(80, 5);
        c.scroll_up(3);
        let before = c.renders;
        let offset = c.offset;
        c.push(Source::Text(Text::raw("a\nb")));
        assert_eq!(c.renders - before, 1);
        assert_eq!(c.offset, offset + 2);
    }

    #[test]
    fn push_while_following_extends_the_cache() {
        let mut c = filled(5);
        c.visible(80, 5);
        let before = c.renders;
        c.push(Source::Text(Text::raw("x")));
        c.visible(80, 5);
        assert_eq!(c.renders - before, 1, "no full re-render after a push");
    }

    #[test]
    fn entries_are_capped_and_the_view_survives() {
        let mut c = filled(1100);
        assert!(c.entries.len() <= MAX_ENTRIES);
        assert_eq!(plain(&c.visible(80, 3)).last().unwrap(), "l1099");
        c.scroll_up(10);
        let before = plain(&c.visible(80, 3));
        for i in 0..150 {
            c.push(Source::Text(Text::raw(format!("n{i}"))));
        }
        assert!(c.entries.len() <= MAX_ENTRIES);
        assert_eq!(plain(&c.visible(80, 3)), before, "scrolled view keeps its text");
    }
```

- [ ] **Step 2: Run** `cargo test tui::chat` — compile failure (`renders`, `MAX_ENTRIES`).

- [ ] **Step 3: Implement**

Field `#[cfg(test)] renders: usize,` (init 0 in `Default`). Count one per `render` call: in `flat` (per entry) and in `push`. Write `flat` as:

```rust
    fn flat(&mut self, width: u16) -> &[Line<'static>] {
        if self.cache.as_ref().map(|c| c.0) != Some(width) {
            let (mut lines, mut soft) = (Vec::new(), Vec::new());
            for e in &self.entries {
                let (l, s) = render(e, width);
                lines.extend(l);
                soft.extend(s);
            }
            #[cfg(test)]
            {
                self.renders += self.entries.len();
            }
            self.cache = Some((width, lines, soft));
        }
        &self.cache.as_ref().expect("cache just filled").1
    }

    pub fn push(&mut self, s: Source) {
        if !self.follow {
            let w = self.width;
            self.flat(w); // cache at the current width, so the new rows can be counted
        }
        let mut added = 0;
        if let Some((w, lines, soft)) = self.cache.as_mut() {
            let (l, sf) = render(&s, *w);
            added = l.len();
            lines.extend(l);
            soft.extend(sf);
            #[cfg(test)]
            {
                self.renders += 1;
            }
        }
        self.entries.push(s);
        if !self.follow {
            // Scrolled up: keep the user's view still and flag the new content.
            self.offset += added;
            self.unread = true;
        }
        self.trim();
    }

    /// Bound memory: past `MAX_ENTRIES`, drop the oldest `TRIM_ENTRIES` (amortized).
    /// `offset` counts from the bottom, so the view does not jump.
    fn trim(&mut self) {
        if self.entries.len() > MAX_ENTRIES {
            self.entries.drain(..TRIM_ENTRIES);
            self.cache = None;
        }
    }
```

(If the borrow checker rejects incrementing `self.renders` inside the `if let` on `self.cache`, increment after the block using `added`-independent flag.)

- [ ] **Step 4:** `cargo test tui::chat`, full `cargo test`, clippy, rustfmt.

- [ ] **Step 5: Commit** `git add src/tui/chat.rs` — `perf(tui): incremental chat cache and a 1000-entry cap`.

---

### Task 4: Welcome panels re-render on resize (C1)

**Files:**
- Modify: `src/tui/chat.rs` (`Source::Fixed` → `Source::Panel`), `src/tui/page.rs` (`page_fixed`, pin `page_pushes_to_the_chat_buffer_not_the_terminal`), `src/tui/run.rs` (welcome push), `src/tui/entry.rs` (`print_identity_welcome`)

**Interfaces:**
- Produces: `Source::Panel(Box<dyn Fn(u16) -> Text<'static>>)`; `pub(crate) fn page_fixed(tui: &mut Tui, f: impl Fn(u16) -> Text<'static> + 'static) -> Result<()>`.

- [ ] **Step 1: Failing test** (replace `fixed_entries_are_clipped_not_wrapped` in chat.rs):

```rust
    #[test]
    fn panels_rerender_at_the_current_width_and_clip_below_it() {
        let mut c = ChatBuffer::default();
        c.push(Source::Panel(Box::new(|w| Text::raw(format!("{}|", "-".repeat(w as usize - 1))))));
        assert_eq!(plain(&c.visible(100, 5)), vec![format!("{}|", "-".repeat(99))]);
        let rows = plain(&c.visible(60, 5));
        assert_eq!(rows, vec![format!("{}|", "-".repeat(59))]);
        assert!(rows[0].ends_with('|'), "border on the last column at the new width");
        // a panel wider than the pane is clipped, never wrapped
        c.push(Source::Panel(Box::new(|_| Text::raw("0123456789"))));
        assert_eq!(plain(&c.visible(4, 5)).last().unwrap(), "0123");
    }
```

Update page.rs pin: `assert!(prod.contains("Source::Panel"));` instead of `Source::Fixed`.

- [ ] **Step 2: Run** `cargo test tui::chat tui::page` — compile failure.

- [ ] **Step 3: Implement**

chat.rs:

```rust
    /// Pre-sized panel (welcome box), rebuilt at the current width on every render and
    /// clipped (never wrapped) when the pane is narrower than the panel's minimum.
    Panel(Box<dyn Fn(u16) -> Text<'static>>),
```

`render` arm: `Source::Panel(f) => { let t = f(w); let n = t.lines.len(); (t.lines.iter().map(|l| clip_line(l, w as usize)).collect(), vec![false; n]) }`.

page.rs:

```rust
/// Like `page`, but a pre-sized panel (welcome box): `f` builds it for the current
/// width on every render, so a resize re-lays it out; narrower than its minimum it is clipped.
pub(crate) fn page_fixed(tui: &mut Tui, f: impl Fn(u16) -> Text<'static> + 'static) -> Result<()> {
    tui.chat.push(Source::Panel(Box::new(f)));
    Ok(())
}
```

run.rs welcome block (the `let w = current_width ...; if let Some(text) = welcome::render_for_entry(...)` part):

```rust
        // `None` only on the resume path with nothing recorded yet (Finding 1,
        // v0.21 review) — the `resuming: <topic>` notice already printed by the
        // topic-entry loop stands on its own; skip the panel rather than print
        // an empty frame. Checked once here; the panel re-renders per width.
        let w = crate::tui::page::current_width(&tui);
        if welcome::render_for_entry(had_topic_arg, &data, w).is_some() {
            crate::tui::page::page_fixed(&mut tui, move |w| {
                welcome::render_for_entry(had_topic_arg, &data, w).unwrap_or_default()
            })?;
        }
```

entry.rs `print_identity_welcome`: build owned copies and push a closure:

```rust
    let name = profile.and_then(welcome_data::extract_name);
    let (model, dir) = (model.to_string(), dir.to_string());
    let (local_o, other_o) = (local.to_vec(), other.to_vec());
    crate::tui::page::page_fixed(tui, move |width| {
        welcome::render_welcome_identity(
            name.as_deref(),
            &model,
            &dir,
            &local_o,
            &other_o,
            project_known,
            width,
            week_sessions,
            streak,
            introduction,
        )
    })?;
```

Remove the now-unused `let width = current_width(tui);` there only if nothing else in the fn uses it. `local`/`other` stay borrowed for the rest of the function (prompt line logic).

- [ ] **Step 4:** full `cargo test`, clippy, rustfmt on the four files.

- [ ] **Step 5: Commit** `git add src/tui/chat.rs src/tui/page.rs src/tui/run.rs src/tui/entry.rs` — `feat(tui): welcome panels re-render at the new width on resize`.

---

### Task 5: Testable seam for the `page*` wrappers (C10)

**Files:**
- Modify: `src/tui/chat.rs` (new methods + tests), `src/tui/page.rs` (wrappers delegate)

**Interfaces:**
- Produces on `ChatBuffer`:

```rust
pub fn text(&mut self, t: Text<'static>)                      // Source::Text
pub fn notice(&mut self, msg: &str)                           // paint::notice_line
pub fn warn(&mut self, msg: &str)                             // paint::warn_line
pub fn error(&mut self, msg: &str)                            // paint::error_line
pub fn user_echo(&mut self, line: &str)                       // jump_bottom + Source::Echo
pub fn image_echo(&mut self, images: &[PastedImage])          // one notice per image, echo_label(i + 1)
pub fn reply(&mut self, md: &str)                             // Source::Reply
pub fn panel(&mut self, f: impl Fn(u16) -> Text<'static> + 'static) // Source::Panel
pub fn flush_notices(&mut self, msgs: Vec<String>)            // "⚠ " prefix (theme::G_WARN + ' ') -> warn, else notice
```

`page.rs`: `page` → `tui.chat.text(text)`, `page_fixed` → `tui.chat.panel(f)`, `page_reply` → `tui.chat.reply(reply)`, `page_notice/warn/error` → the matching method, `flush_notices(tui)` → `tui.chat.flush_notices(ui::drain_tui_notices())`, `page_user_echo` → `tui.chat.user_echo(line)`, `page_image_echo` → `tui.chat.image_echo(images)`. All keep `-> Result<()>` returning `Ok(())`, so no call site changes. `current_width` stays as is.

- [ ] **Step 1: Failing tests (chat.rs)**

```rust
    fn styles(c: &mut ChatBuffer) -> Vec<Vec<Style>> {
        c.visible(80, 50).iter().map(|l| l.spans.iter().map(|s| s.style).collect()).collect()
    }

    #[test]
    fn user_echo_jumps_to_the_bottom() {
        let mut c = filled(20);
        c.visible(80, 5);
        c.scroll_up(5);
        c.user_echo("hello");
        assert!(c.is_following());
        assert!(plain(&c.visible(80, 5)).iter().any(|r| r.contains("hello")));
    }

    #[test]
    fn image_echo_adds_one_labelled_line_per_image() {
        let img = |w, h| crate::image_attach::PastedImage { media_type: "image/png", data: vec![], width: w, height: h };
        let mut c = ChatBuffer::default();
        c.image_echo(&[img(4, 3), img(10, 20)]);
        let rows = plain(&c.visible(80, 10));
        assert_eq!(rows.len(), 2);
        assert!(rows[0].contains("image 1 (4×3)") && rows[1].contains("image 2 (10×20)"));
    }

    #[test]
    fn flush_routes_warn_prefixed_messages_to_the_warn_layer() {
        let mut c = ChatBuffer::default();
        c.flush_notices(vec![format!("{} careful", crate::tui::theme::G_WARN), "fyi".into()]);
        let mut w = ChatBuffer::default();
        w.warn("careful");
        let mut n = ChatBuffer::default();
        n.notice("fyi");
        let s = styles(&mut c);
        assert_eq!(s[0], styles(&mut w)[0]);
        assert_eq!(s[1], styles(&mut n)[0]);
    }

    #[test]
    fn error_line_uses_the_error_style() {
        let mut c = ChatBuffer::default();
        c.error("boom");
        let expect: Vec<Style> = crate::tui::paint::error_line("boom").lines[0].spans.iter().map(|s| s.style).collect();
        assert_eq!(styles(&mut c)[0], expect);
    }
```

- [ ] **Step 2: Run** `cargo test tui::chat` — compile failure.

- [ ] **Step 3: Implement** the methods in `impl ChatBuffer` (each a 1-3 line body built from the existing page.rs bodies; `flush_notices` uses `msg.strip_prefix(&format!("{} ", theme::G_WARN))`). Imports in chat.rs: `crate::image_attach::PastedImage`, `crate::tui::paint::{error_line, notice_line, warn_line}`, `crate::tui::theme`. Then make the page.rs wrappers one-line delegations as listed, removing imports that become unused (`Source` may still be needed by tests only — move such imports into the test module if clippy flags them).

- [ ] **Step 4:** full `cargo test`, clippy, rustfmt `src/tui/chat.rs src/tui/page.rs`. Check chat.rs production ≤ 600.

- [ ] **Step 5: Commit** `git add src/tui/chat.rs src/tui/page.rs` — `refactor(tui): page wrapper bodies move into testable ChatBuffer methods`.

---

### Task 6: Indicator on the rule row (C5) and button-only mouse tracking (C6)

**Files:**
- Modify: `src/tui/page.rs` (`render_screen`, indicator tests), `src/tui/term.rs` (`EnableButtonMouse`, pins)

**Interfaces:**
- Produces: `pub(crate) fn rule_label(buf: &mut ratatui::buffer::Buffer, box_area: Rect, label: &str, style: Style)` in page.rs — paints `label` right-aligned on row `box_area.y`, leaving the rest of the rule intact (plan B reuses it for the "copied" flash). `struct EnableButtonMouse;` implementing `crossterm::Command` in term.rs.

- [ ] **Step 1: Failing tests**

page.rs (replace `indicator_shows_only_while_unread`):

```rust
    #[test]
    fn indicator_sits_on_the_rule_row_not_the_last_chat_line() {
        let mut chat = filled_chat(60);
        let editor = InputBox::new();
        chat.scroll_up(3);
        chat.push(Source::Text(Text::raw("fresh")));
        let rows = screen_rows(80, 24, &mut chat, &editor);
        assert!(!rows[20].trim().is_empty(), "last chat row keeps content: {:?}", rows[20]);
        assert!(!rows[20].contains("new message"));
        assert!(rows[21].starts_with('─'), "{:?}", rows[21]);
        assert!(rows[21].trim_end().ends_with("↓ new message"), "{:?}", rows[21]);
        chat.scroll_down(100);
        let rows = screen_rows(80, 24, &mut chat, &editor);
        assert!(!rows[21].contains("new message"));
    }
```

term.rs tests: rename/replace `setup_enters_the_alternate_screen_with_mouse_capture`:

```rust
    #[test]
    fn setup_enables_button_event_mouse_only() {
        let body = prod().split("pub fn setup(").nth(1).expect("setup");
        let body = body.split("pub fn restore(").next().unwrap();
        assert!(body.contains("EnterAlternateScreen"));
        assert!(body.contains("EnableButtonMouse"));
        assert!(!body.contains("EnableMouseCapture"));
        assert!(!prod().contains("1003h"), "no any-motion tracking (spec C6)");
    }

    #[test]
    fn button_mouse_writes_button_event_and_sgr_modes() {
        let mut s = String::new();
        crossterm::Command::write_ansi(&EnableButtonMouse, &mut s).unwrap();
        assert_eq!(s, "\x1b[?1000h\x1b[?1002h\x1b[?1006h");
    }
```

(Keep whatever else the old setup test asserted, e.g. `EnterAlternateScreen`; the restore test stays — `DisableMouseCapture` still turns everything off.)

- [ ] **Step 2: Run** `cargo test tui::page tui::term` — failures.

- [ ] **Step 3: Implement**

page.rs:

```rust
/// Paint `label` right-aligned on the editor's rule row (decoration only), keeping
/// the rest of the rule. Shared slot for the "new message" indicator.
pub(crate) fn rule_label(buf: &mut Buffer, box_area: Rect, label: &str, style: Style) {
    let w = UnicodeWidthStr::width(label) as u16;
    if box_area.height == 0 || box_area.width < w {
        return;
    }
    buf.set_string(box_area.x + box_area.width - w, box_area.y, label, style);
}
```

In `render_screen`: delete the old `if chat.unread() && chat_area.height > 0 { ... }` block; after `editor.render(f, box_area);` add:

```rust
    if chat.unread() {
        rule_label(f.buffer_mut(), box_area, NEW_MESSAGE, theme::brand());
    }
```

Imports: `ratatui::buffer::Buffer`, `ratatui::style::Style`, `unicode_width::UnicodeWidthStr`; drop now-unused `Span`/`Paragraph` imports only if unused.

term.rs:

```rust
/// Button-event mouse tracking with SGR coordinates (`?1000h ?1002h ?1006h`): presses,
/// releases, wheel and drag-while-held — never bare motion (`?1003h`), so idle mouse
/// movement doesn't wake every loop for a redraw (spec C6). `DisableMouseCapture`
/// turns all of these off again.
struct EnableButtonMouse;

impl crossterm::Command for EnableButtonMouse {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?1000h\x1b[?1002h\x1b[?1006h")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> std::io::Result<()> {
        crossterm::Command::execute_winapi(&EnableMouseCapture)
    }
}
```

`setup`: `crossterm::execute!(std::io::stdout(), EnterAlternateScreen, EnableButtonMouse)`; update the comment above it ("wheel and drag events reach us; Shift+drag still selects natively"). Keep `EnableMouseCapture` imported only under `#[cfg(windows)]` (e.g. `#[cfg(windows)] use crossterm::event::EnableMouseCapture;`) so non-Windows clippy has no unused import. Update the module doc's "mouse capture" wording only if it now lies.

- [ ] **Step 4:** full `cargo test`, clippy, rustfmt `src/tui/page.rs src/tui/term.rs`.

- [ ] **Step 5: Commit** `git add src/tui/page.rs src/tui/term.rs` — `feat(tui): new-message indicator on the rule row; button-event mouse tracking`.

---

### Task 7: Quiet pdftotext (C7) and stale comments (C8)

**Files:**
- Modify: `src/materials.rs` (`convert_pdfs`), `src/tui/convert.rs:1`, `src/tui/polite.rs` (~658-660, comment only)

- [ ] **Step 1: Failing test** (materials.rs tests):

```rust
    #[test]
    fn convert_pdfs_captures_pdftotext_output() {
        let src = include_str!("materials.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap();
        let body = prod.split("pub fn convert_pdfs(").nth(1).expect("convert_pdfs");
        assert!(!body.contains(".status()"), "pdftotext must not inherit the TUI's stdout/stderr");
        assert!(body.contains("Stdio::null()") && body.contains("Stdio::piped()"));
    }
```

- [ ] **Step 2: Run** `cargo test materials::tests::convert_pdfs_captures` — FAIL.

- [ ] **Step 3: Implement** — replace the `let ok = ... .status() ...;` statement and the `notes.push(if ok {...})` with:

```rust
        let out = std::process::Command::new("pdftotext")
            .arg("-layout")
            .arg(pdf)
            .arg(&txt)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output();
        notes.push(match out {
            Ok(o) if o.status.success() => format!("converted: {} → {}", pdf.display(), txt.display()),
            other => {
                let why = other
                    .ok()
                    .and_then(|o| String::from_utf8_lossy(&o.stderr).lines().next().map(str::to_string))
                    .filter(|l| !l.trim().is_empty())
                    .map(|l| format!(" ({})", l.trim()))
                    .unwrap_or_default();
                format!("pdftotext failed on {}{why} — convert it to text yourself", pdf.display())
            }
        });
```

convert.rs line 1 → `//! Convert termimad ANSI output to ratatui Text (the chat pane renders it).`

polite.rs comment in `run_rs_wiring_call_sites_are_pinned`: replace `bound to Terminal<TrackedBackend<Stdout>>` with ``bound to a live `Tui` (crossterm terminal)`` (rewrap the comment lines minimally).

- [ ] **Step 4:** full `cargo test` (the allowed materials failure stays the only failure), clippy, rustfmt the three files.

- [ ] **Step 5: Commit** `git add src/materials.rs src/tui/convert.rs src/tui/polite.rs` — `fix: capture pdftotext output; refresh stale comments`.

---

### Task 8: Version v0.36.0, SPEC, README

**Files:** `Cargo.toml`, `Cargo.lock`, `src/tui/welcome_tests.rs` (`version_aligned_with_spec` pin), `SPEC.md`, `README.md` (only if a user-visible claim changed — the mouse/selection text stays: Shift+drag still selects natively)

- [ ] **Step 1:** version `0.35.0` → `0.36.0` in Cargo.toml and in `version_aligned_with_spec`; `cargo build`.
- [ ] **Step 2:** SPEC.md, after the `**v0.35.0 (image polish):**` paragraph:

```markdown
**v0.36.0 (fullscreen polish):** topic resolution moved out of `tui/run.rs` into `tui/topic_flow.rs` (`resolve_topic` → `TopicPick`), bringing `run.rs` well under the 600-line budget. Chat notices word-wrap (break after the last space; char-split only for a word wider than the row) and every wrapped row records whether it is a soft continuation. Welcome panels are closures rebuilt at the current width (`Source::Panel`), so a resize re-lays them out; narrower than the panel minimum they are clipped. The chat render cache is incremental (a push renders only the new entry) and the buffer keeps at most 1000 entries, dropping the oldest 100 at a time without moving a scrolled-up view. The "↓ new message" indicator sits right-aligned on the editor's rule row instead of covering the last chat line. Mouse tracking is button-event only (`?1000h ?1002h ?1006h`): bare motion is never reported, so idle mouse movement no longer redraws. `pdftotext` runs with captured output (its first stderr line joins the failure note). The `page_*` wrapper bodies live in `ChatBuffer` methods (unit-tested); the wrappers are one-line delegations. Design: `docs/superpowers/specs/2026-10-26-polish-design.md` (C1-C10).
```

- [ ] **Step 3:** `cargo test`, clippy, `cargo fmt --check`.
- [ ] **Step 4: Commit** `git add Cargo.toml Cargo.lock src/tui/welcome_tests.rs SPEC.md` — `chore: v0.36.0 (fullscreen polish)`.
