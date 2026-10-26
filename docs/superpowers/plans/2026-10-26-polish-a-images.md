# Polish A (images) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Image turns never break the session (history image budget), keep JPEG as JPEG, recall images with Up, refuse images before the session starts, report load failures, and re-send the budgeted images on CLI no-resume turns.

**Architecture:** One pure pruning pass at the single history write site (`Session::push_user_with_images`) bounds every consumer. Editor history entries carry their images (`Arc`). A flag on `InputBox` gates image attach; notices are flushed inside `page::draw` so every loop shows them. The CLI backend chooses images by resume state.

**Tech Stack:** Rust 2021, ratatui/crossterm TUI, `image` 0.25 (png/jpeg/gif/webp), `arboard`, `serde_json`, `base64`.

Spec: `docs/superpowers/specs/2026-10-26-polish-design.md` sections A1-A7.

## Global Constraints

- Production code per file (everything above the first `#[cfg(test)]`) stays at or under 600 lines (SPEC §11). Check with: `awk '/^#\[cfg\(test\)\]/{exit} END{print NR}' FILE` or `grep -n '^#\[cfg(test)\]' FILE | head -1`.
- `cargo fmt` only on the files you edited (`rustfmt --edition 2021 src/x.rs`); `cargo fmt --check` must stay clean crate-wide.
- `cargo clippy --all-targets -- -D warnings` clean.
- `cargo test`: only allowed failure is `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt`.
- NO emoji anywhere in UI text (no U+1F300-1FAFF). Notice texts are exactly as written in this plan.
- Surgical changes: touch only what the task needs; match existing style.
- Budgets: `HISTORY_MAX_IMAGES = 20`, `HISTORY_MAX_IMAGE_B64 = 20 * 1024 * 1024`.
- Commits: add files by name (never `git commit -a`); message ends with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- All paths below are relative to the worktree root `/Users/optimotion/Documents/Work/Projects/headspace/usta-polish-a`.

---

### Task 1: History image budget (A1)

**Files:**
- Modify: `src/anthropic.rs` (add `IMAGE_OMITTED`, `prune_images`; `Message::text` skips the placeholder)
- Modify: `src/session.rs` (constants; `push_user_with_images -> usize`)
- Modify: `src/tui/run.rs:550` (notice)
- Add (commit only): `docs/superpowers/specs/2026-10-26-polish-design.md` (already copied into the worktree, untracked) and this plan file

**Interfaces:**
- Produces: `pub const IMAGE_OMITTED: &str` and `pub fn prune_images(history: &mut [Message], max_count: usize, max_b64: usize) -> usize` in `crate::anthropic`; `pub const HISTORY_MAX_IMAGES: usize`, `pub const HISTORY_MAX_IMAGE_B64: usize` and `Session::push_user_with_images(&mut self, text: &str, images: &[PastedImage]) -> usize` in `crate::session`.

- [ ] **Step 1: Write failing tests in `src/anthropic.rs` tests module**

```rust
    fn img_msg(n: usize, b64_len: usize) -> Message {
        let blocks: Vec<Value> = (0..n)
            .map(|_| json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":"A".repeat(b64_len)}}))
            .chain(std::iter::once(json!({"type":"text","text":"look"})))
            .collect();
        Message { role: "user".into(), content: Value::Array(blocks) }
    }

    fn count_images(h: &[Message]) -> usize {
        h.iter().map(|m| m.image_blocks().len()).sum()
    }

    #[test]
    fn prune_images_caps_the_count_keeping_the_newest() {
        let mut h: Vec<Message> = (0..5).map(|_| img_msg(1, 10)).collect();
        assert_eq!(prune_images(&mut h, 3, usize::MAX), 2);
        assert_eq!(count_images(&h), 3);
        assert!(h[0].image_blocks().is_empty());
        assert!(h[1].image_blocks().is_empty());
        assert_eq!(h[4].image_blocks().len(), 1);
    }

    #[test]
    fn prune_images_caps_the_base64_bytes() {
        let mut h = vec![img_msg(1, 100), img_msg(1, 100), img_msg(1, 100)];
        assert_eq!(prune_images(&mut h, 10, 250), 1);
        assert!(h[0].image_blocks().is_empty());
        assert_eq!(count_images(&h), 2);
    }

    #[test]
    fn prune_images_drops_a_messages_oldest_images_first() {
        let mut h = vec![img_msg(3, 10)];
        assert_eq!(prune_images(&mut h, 2, usize::MAX), 1);
        let blocks = h[0].content.as_array().unwrap();
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], IMAGE_OMITTED);
        assert_eq!(blocks[1]["type"], "image");
        assert_eq!(blocks[2]["type"], "image");
    }

    #[test]
    fn prune_images_is_idempotent() {
        let mut h: Vec<Message> = (0..4).map(|_| img_msg(1, 10)).collect();
        assert_eq!(prune_images(&mut h, 2, usize::MAX), 2);
        assert_eq!(prune_images(&mut h, 2, usize::MAX), 0);
        assert_eq!(count_images(&h), 2);
    }

    #[test]
    fn placeholder_does_not_leak_into_text() {
        let mut h = vec![img_msg(1, 10), img_msg(1, 10)];
        prune_images(&mut h, 1, usize::MAX);
        assert_eq!(h[0].text(), "look");
    }
```

- [ ] **Step 2: Run** `cargo test --lib anthropic::tests::prune` — expected: compile error (`prune_images` not found).

- [ ] **Step 3: Implement in `src/anthropic.rs`** (after `impl Message`):

```rust
/// Stands in for an image block that `prune_images` dropped from history.
pub const IMAGE_OMITTED: &str = "[earlier image omitted to stay under the request size limit]";

/// Keep only the newest images within `max_count` blocks and `max_b64` base64 bytes.
/// Walks messages newest to oldest and, inside a message, image blocks last to first;
/// every image block over budget becomes an `IMAGE_OMITTED` text block in place.
/// Returns how many blocks were newly replaced (placeholders are skipped: idempotent).
pub fn prune_images(history: &mut [Message], max_count: usize, max_b64: usize) -> usize {
    let (mut count, mut bytes, mut dropped) = (0usize, 0usize, 0usize);
    for msg in history.iter_mut().rev() {
        let Some(blocks) = msg.content.as_array_mut() else {
            continue;
        };
        for b in blocks.iter_mut().rev() {
            if b.get("type").and_then(Value::as_str) != Some("image") {
                continue;
            }
            let len = b["source"]["data"].as_str().map_or(0, str::len);
            if count < max_count && bytes + len <= max_b64 {
                count += 1;
                bytes += len;
            } else {
                *b = json!({"type": "text", "text": IMAGE_OMITTED});
                dropped += 1;
            }
        }
    }
    dropped
}
```

And change `Message::text` so placeholders contribute nothing:

```rust
            Value::Array(blocks) => {
                let kept: Vec<Value> = blocks
                    .iter()
                    .filter(|b| b.get("text").and_then(Value::as_str) != Some(IMAGE_OMITTED))
                    .cloned()
                    .collect();
                extract_text(&kept)
            }
```

Update the `text()` doc comment: "(image blocks and omitted-image placeholders contribute nothing ...)".

- [ ] **Step 4: Run** `cargo test --lib anthropic::tests` — expected: PASS.

- [ ] **Step 5: Failing session test in `src/session.rs` tests module**

```rust
    #[test]
    fn push_user_with_images_keeps_the_history_under_budget() {
        let img = crate::image_attach::PastedImage {
            media_type: "image/png",
            data: vec![1, 2, 3],
            width: 1,
            height: 1,
        };
        let mut s = Session::new("rust", "sys");
        let mut dropped = 0;
        for i in 0..25 {
            dropped += s.push_user_with_images(&format!("turn {i} [Image #1]"), &[img.clone()]);
            s.push_assistant("ok".into());
        }
        assert_eq!(dropped, 5);
        let req = crate::anthropic::MessageRequest::new("m".into(), "s".into(), s.history().to_vec());
        let body = serde_json::to_string(&req).unwrap();
        assert_eq!(body.matches("\"type\":\"image\"").count(), HISTORY_MAX_IMAGES);
        assert_eq!(s.history()[0].text(), "turn 0 [Image #1]");
    }
```

- [ ] **Step 6: Run** `cargo test --lib session::tests` — expected: compile error (`HISTORY_MAX_IMAGES`, `()` is not `usize`).

- [ ] **Step 7: Implement in `src/session.rs`**

```rust
/// History image budget (spec A1): far under the API's 100-image cap and the
/// ">20 images => <=2000 px" rule; 20 MB base64 leaves ~12 MB of the 32 MB request
/// for text and the closing prompt.
pub const HISTORY_MAX_IMAGES: usize = 20;
pub const HISTORY_MAX_IMAGE_B64: usize = 20 * 1024 * 1024;
```

```rust
    /// Add a user turn that carries images (`[Image #N]` tokens stay in `text`), then
    /// prune the oldest history images over budget. Returns how many were dropped.
    pub fn push_user_with_images(&mut self, text: &str, images: &[PastedImage]) -> usize {
        self.history.push(Message::user_with_images(text, images));
        crate::anthropic::prune_images(&mut self.history, HISTORY_MAX_IMAGES, HISTORY_MAX_IMAGE_B64)
    }
```

Existing test `push_user_with_images_stores_block_content` ignores the return value; leave it (a `usize` result is not `must_use`).

- [ ] **Step 8: Wire the notice in `src/tui/run.rs`** — replace line 550 `session.push_user_with_images(&outgoing, &images);` with:

```rust
                        let dropped = session.push_user_with_images(&outgoing, &images);
                        if dropped > 0 {
                            crate::tui::page::page_notice(&mut tui, &format!("{dropped} older image(s) dropped from context (size limit)"))?;
                        }
```

- [ ] **Step 9: Run** `cargo test` and `cargo clippy --all-targets -- -D warnings` — expected: PASS (except the allowed materials test). Run `rustfmt --edition 2021 src/anthropic.rs src/session.rs` (run.rs is long-line style already; run rustfmt on it too only if `cargo fmt --check` complains about it).

- [ ] **Step 10: Commit**

```bash
git add docs/superpowers/specs/2026-10-26-polish-design.md docs/superpowers/plans/2026-10-26-polish-a-images.md src/anthropic.rs src/session.rs src/tui/run.rs
git commit -m "feat(image): history image budget (20 images / 20 MB)

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Keep JPEG as JPEG (A2)

**Files:**
- Modify: `src/image_attach.rs` (`shrink(img, src)`, `encode_jpeg`, callers)

**Interfaces:**
- Produces: private `fn shrink(img: DynamicImage, src: ImageFormat) -> Result<PastedImage>`, `fn encode_jpeg(img: &DynamicImage) -> Result<Vec<u8>>`. Public API (`prepare`, `from_rgba`) unchanged.

- [ ] **Step 1: Failing tests in `src/image_attach.rs` tests**

```rust
    /// Deterministic noisy RGB image (compresses badly, like a photo).
    fn noise(w: u32, h: u32) -> DynamicImage {
        let mut s: u32 = 12345;
        let img = image::RgbImage::from_fn(w, h, |_, _| {
            s = s.wrapping_mul(1103515245).wrapping_add(12345);
            let v = (s >> 16) as u8;
            image::Rgb([v, v.wrapping_mul(3), v.wrapping_add(90)])
        });
        DynamicImage::ImageRgb8(img)
    }

    fn encode(img: &DynamicImage, f: ImageFormat) -> Vec<u8> {
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), f).unwrap();
        out
    }

    #[test]
    fn prepare_keeps_an_oversized_jpeg_as_jpeg() {
        let src = noise(3000, 2000);
        let img = prepare(encode(&src, ImageFormat::Jpeg)).unwrap();
        assert_eq!(img.media_type, "image/jpeg");
        assert_eq!((img.width, img.height), (1568, 1045));
        let resized = src.resize_exact(1568, 1045, image::imageops::FilterType::Triangle);
        assert!(img.data.len() < encode(&resized, ImageFormat::Png).len());
    }

    #[test]
    fn q85_noise_jpeg_at_max_edge_fits_the_raw_cap() {
        let jpg = encode_jpeg(&noise(MAX_EDGE, MAX_EDGE)).unwrap();
        assert!(jpg.len() <= MAX_RAW_BYTES);
    }
```

(`downscale_dims(3000, 2000, 1568)` = `(1568, 1045)`.)

- [ ] **Step 2: Run** `cargo test --lib image_attach` — expected: FAIL (`encode_jpeg` missing; JPEG becomes PNG).

- [ ] **Step 3: Implement**

```rust
/// JPEG at quality 85 (RGB; alpha dropped).
fn encode_jpeg(img: &DynamicImage) -> Result<Vec<u8>> {
    let mut jpg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 85)
        .encode_image(&DynamicImage::ImageRgb8(img.to_rgb8()))
        .context("cannot encode image")?;
    Ok(jpg)
}

/// Downscale to `MAX_EDGE` and encode. A JPEG source stays JPEG (q85); anything else
/// becomes PNG, or JPEG q85 when the PNG still exceeds `MAX_RAW_BYTES`.
fn shrink(img: DynamicImage, src: ImageFormat) -> Result<PastedImage> {
    let (w, h) = (img.width(), img.height());
    let (nw, nh) = downscale_dims(w, h, MAX_EDGE);
    let img = if (nw, nh) == (w, h) {
        img
    } else {
        img.resize_exact(nw, nh, image::imageops::FilterType::Triangle)
    };
    let done = |media_type, data| PastedImage { media_type, data, width: nw, height: nh };
    if src != ImageFormat::Jpeg {
        let mut png = Vec::new();
        img.write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .context("cannot encode image")?;
        if png.len() <= MAX_RAW_BYTES {
            return Ok(done("image/png", png));
        }
    }
    let jpg = encode_jpeg(&img)?;
    if jpg.len() > MAX_RAW_BYTES {
        bail!("image is too large even after downscaling");
    }
    Ok(done("image/jpeg", jpg))
}
```

Callers: `prepare` → `shrink(img, format)`; `from_rgba` → `shrink(DynamicImage::ImageRgba8(buf), ImageFormat::Png)`. Update the `prepare` doc comment ("re-encoded: JPEG stays JPEG, others PNG ...").

- [ ] **Step 4: Run** `cargo test --lib image_attach` — expected: PASS (existing PNG tests unchanged).

- [ ] **Step 5: Format + commit**

```bash
rustfmt --edition 2021 src/image_attach.rs
git add src/image_attach.rs
git commit -m "feat(image): keep JPEG as JPEG (q85) when downscaling

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Up-arrow recalls the images too (A3)

**Files:**
- Modify: `src/tui/editor.rs` (struct `Entry`, `history`/`stash`/`images` types, submit, recall, `attach_image`, `referenced_images`, `clear`)

**Interfaces:**
- Consumes: `PastedImage` (Clone).
- Produces: `InputBox` public API unchanged (`attach_image(PastedImage)`, `take_images() -> Vec<PastedImage>`). Internal: `struct Entry { text: String, images: Vec<(usize, Arc<PastedImage>)> }`; `const HISTORY_IMAGE_ENTRIES: usize = 20`.

- [ ] **Step 1: Failing tests (editor tests module, after `clipboard_image_text_and_empty_are_applied`)**

```rust
    #[test]
    fn recalled_line_resends_its_images() {
        let mut b = InputBox::new();
        b.attach_image(pasted(1));
        type_str(&mut b, " what");
        b.handle_key(code(KeyCode::Enter));
        assert_eq!(b.take_images(), vec![pasted(1)]);
        b.handle_key(code(KeyCode::Up));
        assert_eq!(b.value(), "[Image #1] what");
        assert!(matches!(b.handle_key(code(KeyCode::Enter)), Action::Submit(_)));
        assert_eq!(b.take_images(), vec![pasted(1)]);
    }

    #[test]
    fn recall_uses_the_entrys_image_not_the_stashs() {
        let mut b = InputBox::new();
        b.attach_image(pasted(1));
        b.handle_key(code(KeyCode::Enter));
        b.take_images();
        b.attach_image(pasted(9)); // half-typed stash with its own #1
        b.handle_key(code(KeyCode::Up));
        assert_eq!(b.value(), "[Image #1]");
        b.handle_key(code(KeyCode::Enter));
        assert_eq!(b.take_images(), vec![pasted(1)]);
    }

    #[test]
    fn down_restores_the_stash_image_and_numbering() {
        let mut b = InputBox::new();
        type_str(&mut b, "old");
        b.handle_key(code(KeyCode::Enter));
        b.attach_image(pasted(7));
        b.handle_key(code(KeyCode::Up));
        b.handle_key(code(KeyCode::Down));
        assert_eq!(b.value(), "[Image #1]");
        b.attach_image(pasted(8));
        assert_eq!(b.value(), "[Image #1][Image #2]");
        b.handle_key(code(KeyCode::Enter));
        assert_eq!(b.take_images(), vec![pasted(7), pasted(8)]);
    }

    #[test]
    fn only_recent_history_entries_keep_images() {
        let mut b = InputBox::new();
        for i in 0..25u8 {
            b.attach_image(pasted(i));
            b.handle_key(code(KeyCode::Enter));
            b.take_images();
        }
        let with_images = b.history.iter().filter(|e| !e.images.is_empty()).count();
        assert_eq!(with_images, 20);
        assert!(b.history[0].images.is_empty());
    }
```

- [ ] **Step 2: Run** `cargo test --lib tui::editor` — expected: FAIL (compile error on `e.images`, or wrong images).

- [ ] **Step 3: Implement**

Add `use std::sync::Arc;`. Types:

```rust
/// Only the newest this-many history entries keep their images (matches the A1 horizon).
const HISTORY_IMAGE_ENTRIES: usize = 20;

/// A submitted (or stashed) line and the images its `[Image #N]` tokens point at.
#[derive(Default, Clone)]
struct Entry {
    text: String,
    images: Vec<(usize, Arc<PastedImage>)>,
}
```

Struct fields: `history: Vec<Entry>`, `stash: Entry`, `images: Vec<(usize, Arc<PastedImage>)>`. `new()`: `stash: Entry::default()`.

Helper:

```rust
    /// Make `e` the composed text + images (recall / back to stash).
    fn show(&mut self, e: Entry) {
        self.ta = area_with(&e.text);
        self.next_image = e.images.iter().map(|(n, _)| n + 1).max().unwrap_or(1);
        self.images = e.images;
    }
```

`attach_image`: `self.images.push((n, Arc::new(img)));`

`referenced_images` returns the kept pairs:

```rust
    /// Pending images whose `[Image #N]` token survives in `line`, in attach order.
    fn referenced_images(&mut self, line: &str) -> Vec<(usize, Arc<PastedImage>)> {
        std::mem::take(&mut self.images)
            .into_iter()
            .filter(|(n, _)| line.contains(&image_token(*n)))
            .collect()
    }
```

Enter branch:

```rust
                let images = self.referenced_images(&line);
                self.submitted = images.iter().map(|(_, i)| (**i).clone()).collect();
                self.history.push(Entry { text: line.clone(), images });
                if let Some(old) = self.history.len().checked_sub(HISTORY_IMAGE_ENTRIES + 1) {
                    self.history[old].images.clear();
                }
                self.clear();
                Action::Submit(line)
```

`edited_recall`: compare `self.history[i].text != self.value()`.

`recall_prev`:

```rust
        let next = match self.cursor {
            None => {
                self.stash = Entry { text: self.value(), images: std::mem::take(&mut self.images) };
                self.history.len() - 1
            }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.cursor = Some(next);
        self.show(self.history[next].clone());
```

`recall_next`:

```rust
            Some(i) if i + 1 < self.history.len() => {
                self.cursor = Some(i + 1);
                self.show(self.history[i + 1].clone());
            }
            Some(_) => {
                self.cursor = None;
                let stash = std::mem::take(&mut self.stash);
                self.show(stash);
            }
```

(`Entry::clone` clones `Arc`s only, no image bytes.) Check that the existing history tests (`history_up_down_recalls_submitted_lines`, `up_moves_cursor_between_lines_before_history`, `edited_recall_*`) still compile; if any reads `b.history[..]` or `b.stash` as `String`, change to `.text`.

- [ ] **Step 4: Run** `cargo test --lib tui::editor` — expected: PASS. Check production line count of `src/tui/editor.rs` stays ≤ 600.

- [ ] **Step 5: Format + commit**

```bash
rustfmt --edition 2021 src/tui/editor.rs
git add src/tui/editor.rs
git commit -m "feat(image): Up-arrow recall carries the line's images

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Report image load failures + notices flushed in draw (A5)

**Files:**
- Modify: `src/image_paste.rs` (`load_dropped -> Option<Result<PastedImage>>`, `Clip::Failed(String)`)
- Modify: `src/tui/editor.rs` (`paste`, `apply_clip`)
- Modify: `src/tui/page.rs` (`draw` calls `flush_notices`)
- Modify: `src/tui/run.rs` (remove the loop-top `flush_notices` call and its 3-line comment, ~lines 433-436)

**Interfaces:**
- Produces: `pub fn load_dropped(raw: &str) -> Option<anyhow::Result<PastedImage>>`; `pub enum Clip { Image(PastedImage), Text(String), Failed(String), Empty }`.

- [ ] **Step 1: Failing tests**

In `src/image_paste.rs` tests, replace `load_dropped_returns_a_prepared_image` / `load_dropped_is_none_for_a_corrupt_image` with:

```rust
    #[test]
    fn load_dropped_returns_a_prepared_image() {
        let d = scratch("load");
        let p = d.join("x.png");
        write_png(&p);
        let img = load_dropped(p.to_str().unwrap()).unwrap().unwrap();
        assert_eq!((img.media_type, img.width, img.height), ("image/png", 8, 8));
    }

    #[test]
    fn load_dropped_reports_a_corrupt_image() {
        let d = scratch("corrupt");
        let p = d.join("bad.png");
        std::fs::write(&p, b"not a png").unwrap();
        assert!(matches!(load_dropped(p.to_str().unwrap()), Some(Err(_))));
    }

    #[test]
    fn load_dropped_is_none_for_a_text_file() {
        let d = scratch("text");
        let p = d.join("notes.txt");
        std::fs::write(&p, b"hello").unwrap();
        assert!(load_dropped(p.to_str().unwrap()).is_none());
        assert!(load_dropped("just some words").is_none());
    }
```

In `src/tui/editor.rs` tests add (a shared lock serializes tests that read the global notice buffer):

```rust
    static NOTICES: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn notices_after(f: impl FnOnce()) -> Vec<String> {
        let _g = NOTICES.lock().unwrap_or_else(|p| p.into_inner());
        crate::ui::set_tui_active(true);
        crate::ui::drain_tui_notices();
        f();
        crate::ui::drain_tui_notices()
    }

    #[test]
    fn failed_drop_inserts_the_path_and_leaves_a_notice() {
        let dir = std::env::temp_dir().join(format!("usta editor bad {}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("bad.png");
        std::fs::write(&p, b"not a png").unwrap();
        let raw = p.to_str().unwrap().to_string();
        let mut b = InputBox::new();
        let n = notices_after(|| b.paste(&raw));
        assert_eq!(b.value(), raw);
        assert!(n.iter().any(|m| m.starts_with("couldn't attach image: ")), "{n:?}");
    }

    #[test]
    fn failed_clipboard_image_leaves_a_notice() {
        let mut b = InputBox::new();
        let n = notices_after(|| b.apply_clip(Clip::Failed("bad size".into())));
        assert_eq!(b.value(), "");
        assert!(n.iter().any(|m| m == "couldn't attach image: bad size"), "{n:?}");
    }
```

NOTE: `set_allow_images` arrives in Task 5, which adds `b.set_allow_images(true);` to these two tests.

In `src/tui/page.rs` tests extend `draw_paints_through_render_screen`:

```rust
        assert!(draw.contains("flush_notices(tui)"), "draw must flush buffered notices");
```

And add a run.rs pin there:

```rust
    #[test]
    fn run_loop_no_longer_flushes_notices_itself() {
        let src = include_str!("run.rs");
        let production = src.split("#[cfg(test)]").next().unwrap();
        assert!(!production.contains("flush_notices("), "draw owns the notice flush");
    }
```

- [ ] **Step 2: Run** `cargo test --lib image_paste tui::editor tui::page` — expected: compile errors / FAIL.

- [ ] **Step 3: Implement**

`src/image_paste.rs`:

```rust
/// Dropped path -> prepared image. `None`: the paste is not one existing image file (the
/// caller pastes the text as-is). `Some(Err)`: it looked like an image file but failed.
pub fn load_dropped(raw: &str) -> Option<Result<PastedImage>> {
    let path = parse_dropped_path(raw)?;
    Some(
        std::fs::read(&path)
            .context("cannot read file")
            .and_then(image_attach::prepare),
    )
}
```

(add `use anyhow::{Context, Result};`). Enum gains `/// A clipboard image that could not be converted.` `Failed(String),`. In `read_clipboard`:

```rust
    if let Ok(img) = cb.get_image() {
        return match image_attach::from_rgba(img.width as u32, img.height as u32, img.bytes.into_owned()) {
            Ok(p) => Clip::Image(p),
            Err(e) => Clip::Failed(e.to_string()),
        };
    }
```

`src/tui/editor.rs` (add `use crate::ui;` if not present):

```rust
    pub fn paste(&mut self, s: &str) {
        match image_paste::load_dropped(s) {
            Some(Ok(img)) => self.attach_image(img),
            Some(Err(e)) => {
                self.insert_str(s);
                ui::notice(&format!("couldn't attach image: {e}"));
            }
            None => self.insert_str(s),
        }
    }
```

`apply_clip` gains `Clip::Failed(e) => ui::notice(&format!("couldn't attach image: {e}")),`.

`src/tui/page.rs` `draw`: first statement of the body becomes `flush_notices(tui)?;` and add one comment line `// Every loop draws: flushing here shows notices raised in intro/entry/confirm too.`

`src/tui/run.rs`: delete the 3 comment lines and `crate::tui::page::flush_notices(&mut tui)?;` at the top of the main `loop`.

- [ ] **Step 4: Run** `cargo test` — expected: PASS (allowed failure only). `cargo clippy --all-targets -- -D warnings` clean.

- [ ] **Step 5: Format + commit**

```bash
rustfmt --edition 2021 src/image_paste.rs src/tui/editor.rs src/tui/page.rs
git add src/image_paste.rs src/tui/editor.rs src/tui/page.rs src/tui/run.rs
git commit -m "feat(image): tell the user when an image fails to load; flush notices in draw

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: No image attach before the session starts (A4)

**Files:**
- Modify: `src/tui/editor.rs` (`allow_images` flag, `set_allow_images`, `paste`, `apply_clip`, existing image tests)
- Modify: `src/tui/run.rs` (one line before the main `loop`)

**Interfaces:**
- Produces: `pub fn set_allow_images(&mut self, on: bool)` on `InputBox`; flag defaults `false`.

- [ ] **Step 1: Failing tests**

```rust
    #[test]
    fn images_are_off_by_default() {
        let b = InputBox::new();
        assert!(!b.allow_images);
    }

    #[test]
    fn clip_image_before_the_session_inserts_nothing_and_notices() {
        let mut b = InputBox::new();
        let n = notices_after(|| b.apply_clip(Clip::Image(pasted(1))));
        assert_eq!(b.value(), "");
        assert!(n.iter().any(|m| m == "images can be attached once the session has started"), "{n:?}");
    }

    #[test]
    fn dropped_path_before_the_session_stays_text() {
        let dir = std::env::temp_dir().join(format!("usta editor off {}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("ok.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255])).save(&p).unwrap();
        let mut b = InputBox::new();
        b.paste(p.to_str().unwrap());
        assert_eq!(b.value(), p.to_str().unwrap());
    }

    #[test]
    fn run_enables_images_before_the_main_loop() {
        let src = include_str!("run.rs");
        let production = src.split("#[cfg(test)]").next().unwrap();
        let before_loop = production.split("\n    loop {").next().unwrap();
        assert!(before_loop.contains("editor.set_allow_images(true);"));
    }
```

Then update existing image tests that attach via `paste`/`apply_clip` and the two Task 4 failure tests: add `b.set_allow_images(true);` right after `InputBox::new()` in `paste_of_an_image_path_becomes_a_token`, `clipboard_image_text_and_empty_are_applied`, `failed_drop_inserts_the_path_and_leaves_a_notice`, `failed_clipboard_image_leaves_a_notice`. (`attach_image` itself is not gated, so tests calling it directly need no change.)

Note: verify the `"\n    loop {"` needle matches exactly one place in `run.rs` production that is the main loop (the main loop at ~line 432 is indented 4 spaces; topic-resolution loops are indented deeper). If an earlier 4-space `loop {` exists, use `production.split("let mut pending = crate::tui::polite::PendingChanges::new();").nth(1)` and assert the flag is set before the following `loop {` instead.

- [ ] **Step 2: Run** `cargo test --lib tui::editor` — expected: FAIL.

- [ ] **Step 3: Implement**

Field `/// Image attach is live only once the session loop owns the editor (spec A4).` `allow_images: bool,` (default `false` in `new`).

```rust
    /// Allow `[Image #N]` attach (Ctrl+V image, dropped image file). Off until the session starts.
    pub fn set_allow_images(&mut self, on: bool) {
        self.allow_images = on;
    }
```

`paste`: first line `if !self.allow_images { return self.insert_str(s); }`.

`apply_clip`:

```rust
            Clip::Image(img) if self.allow_images => self.attach_image(img),
            Clip::Image(_) => ui::notice("images can be attached once the session has started"),
```

`src/tui/run.rs`: immediately before the main `loop {` (after `let mut pending = ...;`) add `editor.set_allow_images(true);`.

- [ ] **Step 4: Run** `cargo test` — PASS. Clippy clean. Check `run.rs` production lines ≤ 600.

- [ ] **Step 5: Format + commit**

```bash
rustfmt --edition 2021 src/tui/editor.rs
git add src/tui/editor.rs src/tui/run.rs
git commit -m "feat(image): no image attach before the session starts

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: CLI no-resume turns re-send the budgeted images (A6)

**Files:**
- Modify: `src/backend.rs` (`history_images`, `Backend::complete` CLI arm)

**Interfaces:**
- Consumes: `Message::image_blocks()`, `anthropic::IMAGE_OMITTED` (placeholders are text blocks, so `image_blocks` already skips them).
- Produces: `fn history_images(history: &[Message]) -> Vec<serde_json::Value>`.

- [ ] **Step 1: Failing tests (backend tests)**

```rust
    #[test]
    fn history_images_labels_each_user_message_with_images() {
        let mut history = vec![
            Message::user_with_images("one [Image #1]", &[pasted(vec![1])]),
            Message::assistant_raw(serde_json::Value::String("ok".into())),
            Message::user("two"),
            Message::assistant_raw(serde_json::Value::String("ok".into())),
            Message::user_with_images("three", &[pasted(vec![2]), pasted(vec![3])]),
        ];
        let v = history_images(&history);
        let kinds: Vec<&str> = v.iter().map(|b| b["type"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["text", "image", "text", "image", "image"]);
        assert_eq!(v[0]["text"], "images attached to [SEN] message 1:");
        assert_eq!(v[2]["text"], "images attached to [SEN] message 3:");
        // pruned placeholders are not images
        crate::anthropic::prune_images(&mut history, 2, usize::MAX);
        let v = history_images(&history);
        assert_eq!(v.iter().filter(|b| b["type"] == "image").count(), 2);
        assert_eq!(v[0]["text"], "images attached to [SEN] message 3:");
    }

    #[test]
    fn history_images_is_empty_for_text_only_history() {
        let history = vec![Message::user("a"), Message::user("b")];
        assert!(history_images(&history).is_empty());
    }

    #[test]
    fn no_resume_calls_send_history_images() {
        let src = include_str!("backend.rs");
        let production = src.split("#[cfg(test)]").next().unwrap();
        assert!(production.contains("None => (render_transcript(history), history_images(history)),"));
        assert!(production.contains("let images = history_images(history);"), "stale retry re-sends the bounded set");
    }
```

- [ ] **Step 2: Run** `cargo test --lib backend::tests::history_images backend::tests::no_resume` — expected: FAIL.

- [ ] **Step 3: Implement**

```rust
/// Every image still in (budgeted) history, for a no-resume call: per user message k
/// (1-based among user messages) that has images, one label text block then its images.
fn history_images(history: &[Message]) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for (k, m) in history.iter().filter(|m| m.role == "user").enumerate() {
        let imgs = m.image_blocks();
        if imgs.is_empty() {
            continue;
        }
        out.push(serde_json::json!({"type": "text", "text": format!("images attached to [SEN] message {}:", k + 1)}));
        out.extend(imgs);
    }
    out
}
```

CLI arm:

```rust
                let resume = session_id.clone();
                let (input, images) = match &resume {
                    Some(_) => (last_user_text(history), last_user_images(history)),
                    None => (render_transcript(history), history_images(history)),
                };
                let attempt =
                    run_claude_cli(bin, model, system, &input, &images, resume.as_deref()).await;
                let (text, new_sid, tokens) = match attempt {
                    Ok(v) => v,
                    // Stale/deleted session — retry once from scratch with the full transcript
                    // and every budgeted image (also recovers a session overgrown with images).
                    Err(_) if resume.is_some() => {
                        *session_id = None;
                        let transcript = render_transcript(history);
                        let images = history_images(history);
                        run_claude_cli(bin, model, system, &transcript, &images, None).await?
                    }
                    Err(e) => return Err(e),
                };
```

Update the `last_user_images` doc comment: "Image blocks of the LAST user message, for a resumed call (the server-side session already holds earlier images)."

- [ ] **Step 4: Run** `cargo test` — PASS; clippy clean.

- [ ] **Step 5: Format + commit**

```bash
rustfmt --edition 2021 src/backend.rs
git add src/backend.rs
git commit -m "feat(image): CLI no-resume turns re-send the budgeted history images

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: Version v0.35.0, README, SPEC (incl. A7 line)

**Files:**
- Modify: `Cargo.toml` (version `0.35.0`), `Cargo.lock` (via `cargo build`), `README.md`, `SPEC.md`

- [ ] **Step 1:** `Cargo.toml` `version = "0.34.0"` → `"0.35.0"`; run `cargo build` so `Cargo.lock` updates.

- [ ] **Step 2: README.md** — in the `**Images:**` paragraph (after the keybinding table) append: ` Images can be attached once the session has started (not during the intro). Up-arrow recall brings a line's images back with it. Only the 20 newest images (20 MB) stay in the conversation; older ones are dropped with a notice.`

- [ ] **Step 3: SPEC.md** — after the `**v0.34.0 (image paste):**` paragraph add:

```markdown
**v0.35.0 (image polish):** history image budget: `Session::push_user_with_images` prunes after every push (`anthropic::prune_images`), keeping the newest 20 image blocks within 20 MB of base64; older ones become an `[earlier image omitted ...]` text block (invisible to `Message::text`) and the TUI shows `N older image(s) dropped from context (size limit)`. Every consumer (API calls, closing flush, file feedback, CLI) reads the pruned history. Downscaled JPEGs stay JPEG (q85); other formats PNG with a JPEG q85 fallback. Up-arrow recall restores a line's images with its text (only the 20 newest history entries keep images). Images attach only once the session loop owns the editor; before that Ctrl+V on an image shows `images can be attached once the session has started` and a dropped path stays text. A failed image load inserts the path text and shows `couldn't attach image: <reason>`. Notices are flushed inside `page::draw`, so every loop shows them. The `claude` CLI backend sends the last turn's images on `--resume`, and every budgeted history image (labelled per `[SEN]` message) on a no-resume call or stale-session retry. Out of scope by design: on-disk transcripts record the `[Image #N]` token only, never image data. Design: `docs/superpowers/specs/2026-10-26-polish-design.md` (A1-A7).
```

- [ ] **Step 4:** `cargo test` PASS, `cargo clippy --all-targets -- -D warnings` clean, `cargo fmt --check` clean.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock README.md SPEC.md
git commit -m "chore: v0.35.0 (image polish)

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```
