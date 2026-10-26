# Image Paste Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The user can attach screenshots/images to a message (Ctrl+V from the clipboard, or drag-and-drop a file into the terminal) on both the Anthropic API backend and the `claude` CLI backend, per `docs/superpowers/specs/2026-10-26-image-paste-design.md`.

**Architecture:** Two new small modules: `image_attach.rs` (a `PastedImage` value: sniff format, downscale to a 1568 px long edge, base64 content block) and `image_paste.rs` (dropped-path detection + `arboard` clipboard read). `InputBox` (editor) owns the pending images: attaching inserts an `[Image #N]` token; on Enter only images whose token is still in the text are kept and exposed through `take_images()`. `Message` gains array content (`image` blocks first, then one `text` block) plus `Message::text()` so every text consumer (transcript, `/context`) ignores base64. The API path just sends the array. The CLI path switches to `--input-format stream-json --output-format stream-json --verbose` ONLY for turns that carry images (verified in the spike); text-only turns are unchanged.

**Tech Stack:** Rust (rust-version 1.88), `image` 0.25 (png/jpeg/gif/webp), `arboard` 3.7, `base64` 0.22 (already in the lockfile via reqwest), serde_json, ratatui-textarea, tokio. Work in `/Users/optimotion/Documents/Work/Projects/headspace/usta`.

## Global Constraints

- Branch: `image-paste`, cut from `fullscreen` AFTER the fullscreen plan (`2026-10-26-fullscreen.md`) has landed. Line anchors below were read on `multiline-input` (`6bfa45c`); fullscreen renames/moves some code (`page_user_echo` pushes to `tui.chat`, `page` -> `page_fixed`, event loops get a `handle_nav` call). **Match on the quoted code, not line numbers.** None of this plan's edits touch code fullscreen rewrites, except `page.rs` (new function appended at the end) and the quoted lines in `run.rs`/`ask.rs`.
- CLI mechanism (verified, `docs/superpowers/specs/2026-10-26-image-paste-cli-spike.md`): `claude -p --input-format stream-json --output-format stream-json --verbose ...`, stdin = ONE JSON line `{"type":"user","message":{"role":"user","content":[<image block>...,<text block>]}}` then EOF; the answer is the LAST stdout line with `"type":"result"` (fields `result`, `session_id`, `usage`, `is_error`). `--resume <id>` works with it. `--input-format stream-json` requires stream-json output; stream-json output requires `--verbose`.
- Image limits (docs, checked 2026-10-26): supported `image/jpeg|png|gif|webp`; max 10 MB per image base64-encoded on the API; max 8000x8000 px; images above the model tier's long-edge limit are downscaled server-side (standard 1568 px, high-res 2576 px on 4.7+ models); token cost = `ceil(w/28) * ceil(h/28)`; request cap 32 MB; >20 images per request tightens the per-image limit to 2000 px. We downscale locally to a **1568 px long edge** (works on every tier, caps cost) and cap raw bytes at **5 MB** (-> <= ~6.7 MB base64).
- Images ride only on the LAST user message of a CLI call. On the no-resume path (first turn, stale-session fallback, after a cancel) earlier turns are sent as a plain-text transcript, so their images are not re-sent (they appear only as `[Image #N]` tokens). Accepted limitation.
- CLAUDE.md: surgical changes; every changed line traces to this plan. Production-code file budget is 600 lines (SPEC.md §11) - check `wc -l` first. Current sizes: `run.rs` 589 (+1 after fullscreen; this plan adds net +2), `editor.rs` 848 total / 419 production (this plan adds ~70 production lines), `backend.rs` 584 total / ~400 production, `anthropic.rs` 255, `session.rs` 115, `page.rs` 153.
- `cargo fmt --check` is crate-wide clean. Format only files you edited: `rustfmt --edition 2021 <file>...` (never bare `cargo fmt`). Code inside `tokio::select!` in `run.rs`/`ask.rs` is not touched by rustfmt - keep those one-line edits as written.
- Pre-existing failure `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt` fails on the base. Leave it. "All tests pass" below means: everything except that one test.
- English for all user-visible strings and code comments (project rule: technology text is English). Chat echo format is exactly `🖼 image N (W×H)` (W and H are the final, post-downscale size).
- Out of scope (spec): inline image preview, image output, non-image files, persisting images in the on-disk transcript (`Recorder` stays text-only; a resumed-from-disk session has the `[Image #N]` tokens but not the pixels).
- Commit messages: `feat(image): ...` style, end with the Co-Authored-By trailer the session instructs.

---

## File Structure

| File | Action | Responsibility |
|---|---|---|
| `Cargo.toml`, `Cargo.lock` | Modify | add `image`, `arboard`, `base64` |
| `src/image_attach.rs` | Create (~150 prod lines + tests) | `PastedImage`, `prepare` (sniff + downscale), `from_rgba`, `downscale_dims`, `to_block`, `echo_label` |
| `src/image_paste.rs` | Create (~70 prod lines + tests) | `parse_dropped_path`, `load_dropped`, `Clip`, `read_clipboard` |
| `src/main.rs` | Modify (+2 lines) | `mod image_attach; mod image_paste;` |
| `src/anthropic.rs` | Modify (+~30 lines) | `Message::user_with_images`, `Message::text`, `Message::image_blocks` |
| `src/session.rs` | Modify (+5 lines) | `Session::push_user_with_images` |
| `src/context_report.rs` | Modify (3 lines) | `message_text` uses `Message::text` |
| `src/backend.rs` | Modify (+~55 lines) | `last_user_images`, `stream_input`, `parse_stream_output`, `run_claude_cli(.., images, ..)` stream-json branch |
| `src/tui/editor.rs` | Modify (+~70 prod lines) | pending images, `[Image #N]` tokens, `paste`, Ctrl+V, submit reconciliation, `take_images` |
| `src/tui/page.rs` | Modify (+8 lines) | `page_image_echo` |
| `src/tui/run.rs`, `src/tui/ask.rs` | Modify (3 + 1 lines) | route `Event::Paste` through `editor.paste`, take images on submit, echo, push |
| `Cargo.toml`, `Cargo.lock`, `README.md`, `SPEC.md`, `src/tui/welcome_tests.rs` | Modify | v0.34.0 bump (same file set as commit `6004da3`) |

---

## Task 1: `image_attach` - the `PastedImage` value

**Files:**
- Modify: `Cargo.toml` (append 3 dependency lines)
- Create: `src/image_attach.rs`
- Modify: `src/main.rs` (add `mod image_attach;` right after `mod history;`)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces (used by Tasks 2-6):
  - `pub const MAX_EDGE: u32 = 1568;`
  - `pub struct PastedImage { pub media_type: &'static str, pub data: Vec<u8>, pub width: u32, pub height: u32 }` (`Debug, Clone, PartialEq`)
  - `pub fn downscale_dims(w: u32, h: u32, max_edge: u32) -> (u32, u32)`
  - `pub fn prepare(bytes: Vec<u8>) -> anyhow::Result<PastedImage>` (file bytes in; passthrough when small and valid, else decode -> downscale -> re-encode)
  - `pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> anyhow::Result<PastedImage>` (clipboard pixels)
  - `impl PastedImage { pub fn to_block(&self) -> serde_json::Value; pub fn echo_label(&self, n: usize) -> String }`

- [ ] **Step 1: Add dependencies**

Append to `Cargo.toml` (end of `[dependencies]`):

```toml
image = { version = "0.25", default-features = false, features = ["png", "jpeg", "gif", "webp"] }
arboard = "3.7"
base64 = "0.22"
```

Add to `src/main.rs` module list, right after `mod history;`:

```rust
mod image_attach;
```

- [ ] **Step 2: Write the failing tests (and the empty module so it compiles)**

Create `src/image_attach.rs` with ONLY this skeleton + tests first:

```rust
//! Pasted / dropped images: format sniffing, downscaling and the API content block.

use anyhow::{bail, Context, Result};
use base64::Engine;
use image::{DynamicImage, ImageFormat, ImageReader};
use serde_json::{json, Value};
use std::io::Cursor;

/// Long-edge cap. Safe on every model tier (standard tier downscales above 1568 px
/// server-side anyway) and keeps token cost bounded (~1.5k tokens max per image).
pub const MAX_EDGE: u32 = 1568;
/// Raw-byte cap per image: 5 MB raw -> ~6.7 MB base64, under the API's 10 MB limit.
const MAX_RAW_BYTES: usize = 5 * 1024 * 1024;

/// An image ready to ride on a user message.
#[derive(Debug, Clone, PartialEq)]
pub struct PastedImage {
    pub media_type: &'static str,
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub fn downscale_dims(_w: u32, _h: u32, _max_edge: u32) -> (u32, u32) {
    unimplemented!()
}

pub fn prepare(_bytes: Vec<u8>) -> Result<PastedImage> {
    unimplemented!()
}

pub fn from_rgba(_width: u32, _height: u32, _rgba: Vec<u8>) -> Result<PastedImage> {
    unimplemented!()
}

impl PastedImage {
    pub fn to_block(&self) -> Value {
        unimplemented!()
    }
    pub fn echo_label(&self, _n: usize) -> String {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode a solid RGBA image of the given size as PNG bytes.
    pub(crate) fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([220, 20, 20, 255]));
        let mut out = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn downscale_dims_leaves_small_images_alone() {
        assert_eq!(downscale_dims(800, 600, 1568), (800, 600));
        assert_eq!(downscale_dims(1568, 1568, 1568), (1568, 1568));
    }

    #[test]
    fn downscale_dims_caps_the_long_edge_and_keeps_aspect() {
        assert_eq!(downscale_dims(4000, 2000, 1568), (1568, 784));
        assert_eq!(downscale_dims(2000, 4000, 1568), (784, 1568));
        assert_eq!(downscale_dims(10_000, 1, 1568), (1568, 1));
    }

    #[test]
    fn prepare_passes_a_small_png_through_untouched() {
        let bytes = png_bytes(64, 32);
        let img = prepare(bytes.clone()).unwrap();
        assert_eq!(img.media_type, "image/png");
        assert_eq!((img.width, img.height), (64, 32));
        assert_eq!(img.data, bytes);
    }

    #[test]
    fn prepare_downscales_an_oversized_png() {
        let img = prepare(png_bytes(3136, 1000)).unwrap();
        assert_eq!(img.media_type, "image/png");
        assert_eq!((img.width, img.height), (1568, 500));
        let back = image::load_from_memory(&img.data).unwrap();
        assert_eq!((back.width(), back.height()), (1568, 500));
    }

    #[test]
    fn prepare_rejects_non_image_bytes() {
        assert!(prepare(b"definitely not an image".to_vec()).is_err());
    }

    #[test]
    fn from_rgba_encodes_clipboard_pixels_as_png() {
        let img = from_rgba(4, 2, vec![255; 4 * 2 * 4]).unwrap();
        assert_eq!(img.media_type, "image/png");
        assert_eq!((img.width, img.height), (4, 2));
        assert!(img.data.starts_with(b"\x89PNG"));
    }

    #[test]
    fn from_rgba_rejects_a_buffer_of_the_wrong_length() {
        assert!(from_rgba(4, 2, vec![0; 5]).is_err());
    }

    #[test]
    fn to_block_is_a_base64_image_block() {
        let img = PastedImage {
            media_type: "image/png",
            data: vec![1, 2, 3],
            width: 1,
            height: 1,
        };
        let b = img.to_block();
        assert_eq!(b["type"], "image");
        assert_eq!(b["source"]["type"], "base64");
        assert_eq!(b["source"]["media_type"], "image/png");
        assert_eq!(b["source"]["data"], "AQID");
    }

    #[test]
    fn echo_label_shows_index_and_size() {
        let img = PastedImage {
            media_type: "image/png",
            data: vec![],
            width: 1280,
            height: 720,
        };
        assert_eq!(img.echo_label(1), "🖼 image 1 (1280×720)");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test image_attach -- --nocapture 2>&1 | tail -30`
Expected: compiles (first build downloads `image`, `arboard`), tests PANIC with `not implemented` (8 failures). Unused-import warnings are fine at this step.

- [ ] **Step 4: Implement**

Replace the four `unimplemented!()` functions/methods (keep everything above `#[cfg(test)]`) with:

```rust
/// Largest size that fits `max_edge` on the long side, aspect preserved (never upscales).
pub fn downscale_dims(w: u32, h: u32, max_edge: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= max_edge {
        return (w, h);
    }
    let scale = max_edge as f64 / long as f64;
    let scaled = |v: u32| ((v as f64 * scale).round() as u32).max(1);
    (scaled(w), scaled(h))
}

fn media_type(f: ImageFormat) -> Option<&'static str> {
    match f {
        ImageFormat::Png => Some("image/png"),
        ImageFormat::Jpeg => Some("image/jpeg"),
        ImageFormat::Gif => Some("image/gif"),
        ImageFormat::WebP => Some("image/webp"),
        _ => None,
    }
}

/// File bytes -> `PastedImage`. The format is sniffed from the bytes (never the file
/// extension). A valid image within both caps is passed through byte-for-byte; anything
/// larger is decoded, downscaled and re-encoded (PNG, JPEG if the PNG is still too big).
pub fn prepare(bytes: Vec<u8>) -> Result<PastedImage> {
    let reader = ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .context("cannot read image")?;
    let format = reader.format().context("unknown image format")?;
    let Some(mt) = media_type(format) else {
        bail!("unsupported image format {format:?} (png, jpeg, gif, webp only)");
    };
    let (w, h) = reader.into_dimensions().context("cannot read image size")?;
    if w.max(h) <= MAX_EDGE && bytes.len() <= MAX_RAW_BYTES {
        return Ok(PastedImage {
            media_type: mt,
            data: bytes,
            width: w,
            height: h,
        });
    }
    let img = image::load_from_memory_with_format(&bytes, format).context("cannot decode image")?;
    shrink(img)
}

/// Clipboard pixels (RGBA8, row-major) -> `PastedImage`.
pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Result<PastedImage> {
    let buf = image::RgbaImage::from_raw(width, height, rgba)
        .context("clipboard image has an inconsistent size")?;
    shrink(DynamicImage::ImageRgba8(buf))
}

/// Downscale to `MAX_EDGE` and encode: PNG, or JPEG when the PNG still exceeds `MAX_RAW_BYTES`.
fn shrink(img: DynamicImage) -> Result<PastedImage> {
    let (w, h) = (img.width(), img.height());
    let (nw, nh) = downscale_dims(w, h, MAX_EDGE);
    let img = if (nw, nh) == (w, h) {
        img
    } else {
        img.resize_exact(nw, nh, image::imageops::FilterType::Triangle)
    };
    let mut png = Vec::new();
    img.write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .context("cannot encode image")?;
    if png.len() <= MAX_RAW_BYTES {
        return Ok(PastedImage {
            media_type: "image/png",
            data: png,
            width: nw,
            height: nh,
        });
    }
    let mut jpg = Vec::new();
    DynamicImage::ImageRgb8(img.to_rgb8())
        .write_to(&mut Cursor::new(&mut jpg), ImageFormat::Jpeg)
        .context("cannot encode image")?;
    if jpg.len() > MAX_RAW_BYTES {
        bail!("image is too large even after downscaling");
    }
    Ok(PastedImage {
        media_type: "image/jpeg",
        data: jpg,
        width: nw,
        height: nh,
    })
}

impl PastedImage {
    /// The Messages API `image` content block (base64 source).
    pub fn to_block(&self) -> Value {
        json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": self.media_type,
                "data": base64::engine::general_purpose::STANDARD.encode(&self.data),
            }
        })
    }

    /// Chat echo line, `n` is the 1-based position among the message's images.
    pub fn echo_label(&self, n: usize) -> String {
        format!("🖼 image {n} ({}×{})", self.width, self.height)
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test image_attach 2>&1 | tail -15`
Expected: `test result: ok. 8 passed`.

- [ ] **Step 6: Format and commit**

```bash
rustfmt --edition 2021 src/image_attach.rs
cargo build 2>&1 | tail -3
git add Cargo.toml Cargo.lock src/image_attach.rs src/main.rs
git commit -m "feat(image): PastedImage with sniffing, 1568px downscale and API block"
```

---

## Task 2: `image_paste` - dropped paths and the clipboard

**Files:**
- Create: `src/image_paste.rs`
- Modify: `src/main.rs` (add `mod image_paste;` right after `mod image_attach;`)

**Interfaces:**
- Consumes: `crate::image_attach::{prepare, from_rgba, PastedImage}` (Task 1).
- Produces (used by Task 5):
  - `pub fn parse_dropped_path(raw: &str) -> Option<std::path::PathBuf>` - `Some` only for a single-line paste that is an existing file with extension png/jpg/jpeg/gif/webp (any case); handles surrounding `'...'` / `"..."` and backslash-escaped characters (`My\ Shot.png`).
  - `pub fn load_dropped(raw: &str) -> Option<PastedImage>` - `parse_dropped_path` + read + `prepare`; `None` on any failure (the caller then pastes the text as-is).
  - `pub enum Clip { Image(PastedImage), Text(String), Empty }`
  - `pub fn read_clipboard() -> Clip` - system clipboard via `arboard`: image first, else non-empty text, else `Empty` (also `Empty` when no clipboard is reachable, e.g. headless Linux). Not unit-tested (needs a real clipboard) - covered by the manual checklist.

- [ ] **Step 1: Write the failing tests (skeleton + tests)**

Add `mod image_paste;` to `src/main.rs`. Create `src/image_paste.rs`:

```rust
//! Getting an image into the editor: a file path the terminal pasted after a
//! drag-and-drop, or the system clipboard (Ctrl+V).

use crate::image_attach::{self, PastedImage};
use std::path::PathBuf;

const IMAGE_EXTS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];

pub fn parse_dropped_path(_raw: &str) -> Option<PathBuf> {
    unimplemented!()
}

pub fn load_dropped(_raw: &str) -> Option<PastedImage> {
    unimplemented!()
}

/// What the system clipboard held.
pub enum Clip {
    Image(PastedImage),
    Text(String),
    Empty,
}

pub fn read_clipboard() -> Clip {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Fresh scratch dir whose name contains a space (what Finder drags produce).
    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("usta img {tag} {}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_png(path: &Path) {
        image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 255, 255]))
            .save(path)
            .unwrap();
    }

    #[test]
    fn plain_path_to_an_existing_png() {
        let d = scratch("plain");
        let p = d.join("shot.png");
        write_png(&p);
        assert_eq!(parse_dropped_path(p.to_str().unwrap()), Some(p));
    }

    #[test]
    fn quoted_paths_are_unwrapped() {
        let d = scratch("quoted");
        let p = d.join("a b.png");
        write_png(&p);
        let s = p.to_str().unwrap();
        assert_eq!(parse_dropped_path(&format!("'{s}'")), Some(p.clone()));
        assert_eq!(parse_dropped_path(&format!("\"{s}\"")), Some(p));
    }

    #[test]
    fn backslash_escaped_spaces_are_unescaped() {
        let d = scratch("escaped");
        let p = d.join("My Shot.png");
        write_png(&p);
        let escaped = p.to_str().unwrap().replace(' ', "\\ ");
        assert_eq!(parse_dropped_path(&escaped), Some(p));
    }

    #[test]
    fn surrounding_whitespace_and_trailing_newline_are_trimmed() {
        let d = scratch("trim");
        let p = d.join("t.png");
        write_png(&p);
        assert_eq!(
            parse_dropped_path(&format!("  {}\n", p.to_str().unwrap())),
            Some(p)
        );
    }

    #[test]
    fn extension_match_is_case_insensitive() {
        let d = scratch("case");
        let p = d.join("T.PNG");
        write_png(&p);
        assert_eq!(parse_dropped_path(p.to_str().unwrap()), Some(p));
    }

    #[test]
    fn nonexistent_file_is_not_a_drop() {
        assert_eq!(parse_dropped_path("/definitely/not/here.png"), None);
    }

    #[test]
    fn non_image_extension_is_not_a_drop() {
        let d = scratch("txt");
        let p = d.join("notes.txt");
        std::fs::write(&p, "hi").unwrap();
        assert_eq!(parse_dropped_path(p.to_str().unwrap()), None);
    }

    #[test]
    fn multiline_or_prose_paste_is_not_a_drop() {
        assert_eq!(parse_dropped_path("line one\nline two.png"), None);
        assert_eq!(parse_dropped_path("see the screenshot please"), None);
        assert_eq!(parse_dropped_path(""), None);
    }

    #[test]
    fn load_dropped_returns_a_prepared_image() {
        let d = scratch("load");
        let p = d.join("x.png");
        write_png(&p);
        let img = load_dropped(p.to_str().unwrap()).unwrap();
        assert_eq!((img.media_type, img.width, img.height), ("image/png", 8, 8));
    }

    #[test]
    fn load_dropped_is_none_for_a_corrupt_image() {
        let d = scratch("corrupt");
        let p = d.join("bad.png");
        std::fs::write(&p, b"not a png").unwrap();
        assert!(load_dropped(p.to_str().unwrap()).is_none());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test image_paste 2>&1 | tail -20`
Expected: 10 tests FAIL (`not implemented`).

- [ ] **Step 3: Implement**

Replace the three `unimplemented!()` functions:

```rust
/// Backslash-escaped characters (`My\ Shot.png`) -> literal characters.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A terminal drag-and-drop arrives as a bracketed paste of the file path (quoted or
/// backslash-escaped). `Some` only if the whole paste is ONE existing image file.
pub fn parse_dropped_path(raw: &str) -> Option<PathBuf> {
    let t = raw.trim();
    if t.is_empty() || t.contains('\n') {
        return None;
    }
    let quoted = t.len() >= 2
        && ((t.starts_with('\'') && t.ends_with('\'')) || (t.starts_with('"') && t.ends_with('"')));
    let path = PathBuf::from(if quoted {
        t[1..t.len() - 1].to_string()
    } else {
        unescape(t)
    });
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    (IMAGE_EXTS.contains(&ext.as_str()) && path.is_file()).then_some(path)
}

/// Dropped path -> prepared image. `None` on any failure: the caller pastes the text as-is.
pub fn load_dropped(raw: &str) -> Option<PastedImage> {
    let path = parse_dropped_path(raw)?;
    image_attach::prepare(std::fs::read(path).ok()?).ok()
}

/// Read the system clipboard: an image if there is one, else non-empty text.
pub fn read_clipboard() -> Clip {
    let Ok(mut cb) = arboard::Clipboard::new() else {
        return Clip::Empty;
    };
    if let Ok(img) = cb.get_image() {
        if let Ok(p) = image_attach::from_rgba(
            img.width as u32,
            img.height as u32,
            img.bytes.into_owned(),
        ) {
            return Clip::Image(p);
        }
    }
    match cb.get_text() {
        Ok(t) if !t.is_empty() => Clip::Text(t),
        _ => Clip::Empty,
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test image_paste 2>&1 | tail -15`
Expected: `test result: ok. 10 passed`.

- [ ] **Step 5: Format and commit**

```bash
rustfmt --edition 2021 src/image_paste.rs
git add src/image_paste.rs src/main.rs
git commit -m "feat(image): dropped-path detection and clipboard read"
```

---

## Task 3: `Message` with image blocks, `Session::push_user_with_images`

**Files:**
- Modify: `src/anthropic.rs` (add imports + 3 `Message` methods; tests in the existing `mod tests`)
- Modify: `src/session.rs` (one method + one test)
- Modify: `src/context_report.rs` (`message_text` body)
- Modify: `src/backend.rs` (`last_user_text` and `render_transcript` use `Message::text`)

**Interfaces:**
- Consumes: `crate::image_attach::PastedImage::to_block` (Task 1).
- Produces (used by Tasks 4-6):
  - `Message::user_with_images(text: impl Into<String>, images: &[PastedImage]) -> Message` - empty `images` => identical to `Message::user(text)` (string content); otherwise `content = [image blocks..., {"type":"text","text":...}]`.
  - `Message::text(&self) -> String` - string content as is; block arrays => text blocks joined, image blocks contribute nothing.
  - `Message::image_blocks(&self) -> Vec<serde_json::Value>` - the `image` blocks of an array message, else empty.
  - `Session::push_user_with_images(&mut self, text: &str, images: &[PastedImage])`.

- [ ] **Step 1: Write the failing tests**

In `src/anthropic.rs` `mod tests`, add (and `use crate::image_attach::PastedImage;` at the top of the tests module):

```rust
    fn img(bytes: Vec<u8>) -> PastedImage {
        PastedImage {
            media_type: "image/png",
            data: bytes,
            width: 2,
            height: 3,
        }
    }

    #[test]
    fn user_with_no_images_is_a_plain_string_message() {
        let m = Message::user_with_images("hello", &[]);
        assert_eq!(m.content, Value::String("hello".into()));
    }

    #[test]
    fn user_with_images_puts_image_blocks_before_the_text_block() {
        let m = Message::user_with_images("what is this? [Image #1]", &[img(vec![1, 2, 3])]);
        assert_eq!(m.role, "user");
        let blocks = m.content.as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "image");
        assert_eq!(blocks[0]["source"]["media_type"], "image/png");
        assert_eq!(blocks[0]["source"]["data"], "AQID");
        assert_eq!(blocks[1]["type"], "text");
        assert_eq!(blocks[1]["text"], "what is this? [Image #1]");
    }

    #[test]
    fn request_serializes_an_image_message_as_a_content_array() {
        let req = MessageRequest::new(
            DEFAULT_MODEL.into(),
            "sistem".into(),
            vec![Message::user_with_images("hi", &[img(vec![9])])],
        );
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["messages"][0]["content"][0]["type"], "image");
        assert_eq!(v["messages"][0]["content"][1]["text"], "hi");
    }

    #[test]
    fn text_flattens_blocks_and_ignores_base64() {
        let m = Message::user_with_images("only words", &[img(vec![7; 1000])]);
        assert_eq!(m.text(), "only words");
        assert_eq!(Message::user("plain").text(), "plain");
    }

    #[test]
    fn image_blocks_returns_only_image_blocks() {
        let m = Message::user_with_images("t", &[img(vec![1]), img(vec![2])]);
        assert_eq!(m.image_blocks().len(), 2);
        assert!(Message::user("t").image_blocks().is_empty());
    }
```

In `src/session.rs` `mod tests`, add:

```rust
    #[test]
    fn push_user_with_images_stores_block_content() {
        let mut s = Session::new("rust", "sistem");
        let img = crate::image_attach::PastedImage {
            media_type: "image/png",
            data: vec![1],
            width: 1,
            height: 1,
        };
        s.push_user_with_images("look [Image #1]", &[img]);
        s.push_user_with_images("no image", &[]);
        let h = s.history();
        assert!(h[0].content.is_array());
        assert_eq!(h[0].text(), "look [Image #1]");
        assert_eq!(h[1].content, serde_json::Value::String("no image".into()));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test user_with_ text_flattens image_blocks_returns push_user_with_images 2>&1 | tail -20`
Expected: compile error `no function or associated item named user_with_images` / `no method named text`.

- [ ] **Step 3: Implement**

`src/anthropic.rs` - add `use crate::image_attach::PastedImage;` under the existing `use serde_json::{json, Value};`, and extend `impl Message`:

```rust
    /// A user turn with images: image blocks first (the docs' recommended order), then
    /// one text block. No images => the usual plain-string message.
    pub fn user_with_images(text: impl Into<String>, images: &[PastedImage]) -> Self {
        if images.is_empty() {
            return Self::user(text);
        }
        let mut blocks: Vec<Value> = images.iter().map(PastedImage::to_block).collect();
        blocks.push(json!({"type": "text", "text": text.into()}));
        Message {
            role: "user".into(),
            content: Value::Array(blocks),
        }
    }

    /// Plain text of the message: a string as is; a block array = its text blocks joined
    /// (image blocks contribute nothing, so base64 never leaks into transcripts or reports).
    pub fn text(&self) -> String {
        match &self.content {
            Value::String(s) => s.clone(),
            Value::Array(blocks) => extract_text(blocks),
            other => other.to_string(),
        }
    }

    /// The `image` blocks of an array message (empty for string content).
    pub fn image_blocks(&self) -> Vec<Value> {
        self.content
            .as_array()
            .map(|bs| {
                bs.iter()
                    .filter(|b| b.get("type").and_then(Value::as_str) == Some("image"))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
```

`src/session.rs` - add after `push_user`, and `use crate::image_attach::PastedImage;` next to the existing `use crate::anthropic::Message;`:

```rust
    /// Add a user turn that carries images (`[Image #N]` tokens stay in `text`).
    pub fn push_user_with_images(&mut self, text: &str, images: &[PastedImage]) {
        self.history.push(Message::user_with_images(text, images));
    }
```

`src/context_report.rs` - replace the body of `message_text`:

```rust
fn message_text(m: &Message) -> String {
    m.text()
}
```

`src/backend.rs` - in `last_user_text` replace the `.map(|m| match &m.content { ... })` with `.map(Message::text)`; in `render_transcript` replace the `let body = match &msg.content { ... };` block (and its comment line) with `let body = msg.text();`. Concretely `last_user_text` becomes:

```rust
fn last_user_text(history: &[Message]) -> String {
    history
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(Message::text)
        .unwrap_or_default()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test 2>&1 | tail -8`
Expected: everything passes except the pre-existing `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt`. (Existing `last_user_text_*`, `transcript_*`, `/context` tests prove the refactor is behavior-preserving for string content.)

- [ ] **Step 5: Format and commit**

```bash
rustfmt --edition 2021 src/anthropic.rs src/session.rs src/context_report.rs src/backend.rs
git add src/anthropic.rs src/session.rs src/context_report.rs src/backend.rs
git commit -m "feat(image): Message image blocks, Message::text, Session::push_user_with_images"
```

The API backend needs nothing else: `Client::complete` serializes `Message.content` as is, so an array content goes out as image + text blocks.

---

## Task 4: CLI backend - stream-json for turns with images

**Files:**
- Modify: `src/backend.rs` (`complete`, `run_claude_cli`, three new helpers, tests)

**Interfaces:**
- Consumes: `Message::image_blocks`, `Message::text` (Task 3); `parse_cli_output` (existing, unchanged).
- Produces (internal to `backend.rs`):
  - `fn last_user_images(history: &[Message]) -> Vec<serde_json::Value>` - image blocks of the last user message.
  - `fn stream_input(text: &str, images: &[serde_json::Value]) -> String` - the one stdin JSON line (`\n`-terminated).
  - `pub fn parse_stream_output(stdout: &str) -> Result<(String, Option<String>, Option<u64>)>` - last `"type":"result"` line; `Err` if none or `is_error`.
  - `run_claude_cli(bin, model, system, input, images: &[serde_json::Value], resume)` - new 5th parameter `images`; empty => exactly the old behavior.

- [ ] **Step 1: Write the failing tests**

Append inside `backend.rs` `mod tests` (the module already has `use super::*;`; add `use crate::image_attach::PastedImage;`):

```rust
    fn pasted(bytes: Vec<u8>) -> PastedImage {
        PastedImage {
            media_type: "image/png",
            data: bytes,
            width: 1,
            height: 1,
        }
    }

    #[test]
    fn last_user_images_come_from_the_last_user_message_only() {
        let history = vec![
            Message::user_with_images("first [Image #1]", &[pasted(vec![1])]),
            Message::assistant_raw(serde_json::Value::String("ok".into())),
            Message::user_with_images("second [Image #1]", &[pasted(vec![2]), pasted(vec![3])]),
        ];
        assert_eq!(last_user_images(&history).len(), 2);
        let plain_last = vec![
            Message::user_with_images("a", &[pasted(vec![1])]),
            Message::user("b"),
        ];
        assert!(last_user_images(&plain_last).is_empty());
    }

    #[test]
    fn stream_input_is_one_json_line_with_images_before_text() {
        let blocks = vec![pasted(vec![1, 2, 3]).to_block()];
        let line = stream_input("describe [Image #1]", &blocks);
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1);
        let v: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(v["type"], "user");
        assert_eq!(v["message"]["role"], "user");
        assert_eq!(v["message"]["content"][0]["type"], "image");
        assert_eq!(v["message"]["content"][0]["source"]["data"], "AQID");
        assert_eq!(v["message"]["content"][1]["type"], "text");
        assert_eq!(v["message"]["content"][1]["text"], "describe [Image #1]");
    }

    #[test]
    fn parse_stream_output_takes_the_result_line_among_noise() {
        let out = concat!(
            "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"s0\"}\n",
            "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"draft\"}]}}\n",
            "{\"type\":\"rate_limit_event\"}\n",
            "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"Top red, bottom blue.\",\"session_id\":\"abc\",\"usage\":{\"input_tokens\":10,\"cache_read_input_tokens\":5}}\n",
        );
        let (text, sid, tokens) = parse_stream_output(out).unwrap();
        assert_eq!(text, "Top red, bottom blue.");
        assert_eq!(sid.as_deref(), Some("abc"));
        assert_eq!(tokens, Some(15));
    }

    #[test]
    fn parse_stream_output_errors_on_is_error_result() {
        let out = "{\"type\":\"result\",\"subtype\":\"error_during_execution\",\"is_error\":true,\"session_id\":\"x\"}\n";
        let err = parse_stream_output(out).unwrap_err().to_string();
        assert!(err.contains("error_during_execution"), "{err}");
    }

    #[test]
    fn parse_stream_output_errors_without_a_result_line() {
        assert!(parse_stream_output("{\"type\":\"system\"}\nnot json\n").is_err());
        assert!(parse_stream_output("").is_err());
    }

    #[test]
    fn cli_call_uses_stream_json_only_for_image_turns() {
        // Source pin - the flags live inside an async spawn fn.
        let src = include_str!("backend.rs");
        let production = src.split("#[cfg(test)]").next().unwrap();
        for flag in ["--input-format", "stream-json", "--verbose"] {
            assert!(production.contains(flag), "missing {flag}");
        }
        assert!(
            production.contains("let stream = !images.is_empty();"),
            "stream-json must stay opt-in per turn"
        );
    }

    /// Live end-to-end check against the real `claude` CLI (costs ~$0.005, needs login).
    /// Run: cargo test live_cli_describes_an_image -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn live_cli_describes_an_image() {
        let mut img = image::RgbaImage::from_pixel(120, 80, image::Rgba([220, 20, 20, 255]));
        for y in 40..80 {
            for x in 0..120 {
                img.put_pixel(x, y, image::Rgba([20, 20, 220, 255]));
            }
        }
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let block = pasted(png).to_block();
        let (text, sid, _) = run_claude_cli(
            Path::new("claude"),
            "haiku",
            "You are terse.",
            "Name the two colors in [Image #1], top then bottom, two words.",
            &[block],
            None,
        )
        .await
        .unwrap();
        let t = text.to_lowercase();
        assert!(t.contains("red") && t.contains("blue"), "{text}");
        assert!(sid.is_some());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib backend 2>&1 | tail -20`
Expected: compile errors (`cannot find function last_user_images / stream_input / parse_stream_output`, `run_claude_cli` takes 5 arguments).

- [ ] **Step 3: Implement**

In `src/backend.rs`:

(a) Add the three helpers directly after `parse_cli_output`:

```rust
/// Image blocks of the LAST user message. They ride on this call only: a resumed
/// session already holds earlier images, and the no-resume transcript is plain text.
fn last_user_images(history: &[Message]) -> Vec<serde_json::Value> {
    history
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(Message::image_blocks)
        .unwrap_or_default()
}

/// The single stdin line for `--input-format stream-json`: images first, then the text.
fn stream_input(text: &str, images: &[serde_json::Value]) -> String {
    let mut content = images.to_vec();
    content.push(serde_json::json!({"type": "text", "text": text}));
    let mut line = serde_json::json!({
        "type": "user",
        "message": {"role": "user", "content": content}
    })
    .to_string();
    line.push('\n');
    line
}

/// `--output-format stream-json` prints NDJSON; the answer is the last `"type":"result"`
/// line, which carries the same `result` / `session_id` / `usage` as json mode.
pub fn parse_stream_output(stdout: &str) -> Result<(String, Option<String>, Option<u64>)> {
    let v = stdout
        .lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["type"] == "result")
        .context("claude CLI returned no result")?;
    if v["is_error"] == true {
        let why = v["result"].as_str().or(v["subtype"].as_str()).unwrap_or("unknown error");
        bail!("claude CLI error: {why}");
    }
    Ok(parse_cli_output(&v.to_string()))
}
```

(b) In `Backend::complete`, `Backend::Cli` arm: compute images once and pass them to both calls:

```rust
                let images = last_user_images(history);
                let attempt =
                    run_claude_cli(bin, model, system, &input, &images, resume.as_deref()).await;
```
and in the stale-session retry:

```rust
                        run_claude_cli(bin, model, system, &render_transcript(history), &images, None)
                            .await?
```
(the `let images = ...` line goes right after the `let input = match &resume { ... };` statement).

(c) `run_claude_cli`: add the parameter and branch.

Signature:

```rust
async fn run_claude_cli(
    bin: &Path,
    model: &str,
    system: &str,
    input: &str,
    images: &[serde_json::Value],
    resume: Option<&str>,
) -> Result<(String, Option<String>, Option<u64>)> {
```

Replace `cmd.arg("-p").arg("--output-format").arg("json");` with:

```rust
    cmd.arg("-p");
    // Image turns need stream-json in (verified spike: requires stream-json out + --verbose);
    // text-only turns keep the plain json path untouched.
    let stream = !images.is_empty();
    if stream {
        cmd.args([
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
        ]);
    } else {
        cmd.arg("--output-format").arg("json");
    }
```

Replace the stdin write payload (`stdin.write_all(input.as_bytes())`) with:

```rust
        let payload = if stream {
            stream_input(input, images)
        } else {
            input.to_string()
        };
        stdin
            .write_all(payload.as_bytes())
            .await
            .context("failed to write to claude CLI stdin")?;
```
(delete the old `write_all(input.as_bytes())` statement it replaces) and the final line `Ok(parse_cli_output(&String::from_utf8_lossy(&output.stdout)))` with:

```rust
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stream {
        parse_stream_output(&stdout)
    } else {
        Ok(parse_cli_output(&stdout))
    }
```

Update the doc comment of `run_claude_cli` first line to: `/// Run the `claude -p` subprocess: input is written to stdin, JSON output is read back (stream-json when `images` is non-empty).`

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib backend 2>&1 | tail -12`
Expected: all non-ignored backend tests pass (the new 6 included; `live_cli_describes_an_image` shown as `ignored`).

Then the live check (needs the `claude` login; ~$0.005):

Run: `cargo test live_cli_describes_an_image -- --ignored --nocapture 2>&1 | tail -8`
Expected: `test backend::tests::live_cli_describes_an_image ... ok`. If it fails with `claude CLI returned no result`, run the exact command from the spike doc by hand and diff the flags.

- [ ] **Step 5: Format and commit**

```bash
rustfmt --edition 2021 src/backend.rs
git add src/backend.rs
git commit -m "feat(image): CLI backend sends image turns via stream-json"
```

---

## Task 5: Editor - tokens, paste routing, Ctrl+V, submit reconciliation

**Files:**
- Modify: `src/tui/editor.rs` (struct fields, `new`, Enter arm, Ctrl+V arm, `clear`, new methods, tests). Production part is 419 lines; this adds ~70 - stay under 600.

**Interfaces:**
- Consumes: `crate::image_attach::PastedImage` (Task 1), `crate::image_paste::{load_dropped, read_clipboard, Clip}` (Task 2).
- Produces (used by Task 6):
  - `InputBox::attach_image(&mut self, img: PastedImage)` - inserts `[Image #N]` at the cursor, N = 1,2,... per message.
  - `InputBox::paste(&mut self, s: &str)` - bracketed-paste entry point: dropped image path => `attach_image`, else `insert_str`.
  - `InputBox::paste_clipboard(&mut self)` - Ctrl+V.
  - `InputBox::take_images(&mut self) -> Vec<PastedImage>` - images of the LAST submitted line (only those whose token is still in the text), drained.
  - `Action` is unchanged (`Submit(String)` still carries just the text).

- [ ] **Step 1: Write the failing tests**

Add to `editor.rs` `mod tests` (it already has `key`, `code`, `type_str`):

```rust
    use crate::image_attach::PastedImage;
    use crate::image_paste::Clip;

    fn pasted(tag: u8) -> PastedImage {
        PastedImage {
            media_type: "image/png",
            data: vec![tag],
            width: 1,
            height: 1,
        }
    }

    fn backspaces(b: &mut InputBox, n: usize) {
        for _ in 0..n {
            b.handle_key(code(KeyCode::Backspace));
        }
    }

    #[test]
    fn attach_image_inserts_numbered_tokens() {
        let mut b = InputBox::new();
        b.attach_image(pasted(1));
        b.attach_image(pasted(2));
        assert_eq!(b.value(), "[Image #1][Image #2]");
    }

    #[test]
    fn submit_keeps_only_images_whose_token_is_still_in_the_text() {
        let mut b = InputBox::new();
        b.attach_image(pasted(1));
        b.attach_image(pasted(2));
        backspaces(&mut b, "[Image #2]".chars().count()); // delete token 2
        type_str(&mut b, " what is this");
        let Action::Submit(line) = b.handle_key(code(KeyCode::Enter)) else {
            panic!("expected Submit")
        };
        assert_eq!(line, "[Image #1] what is this");
        assert_eq!(b.take_images(), vec![pasted(1)]);
    }

    #[test]
    fn submit_with_every_token_deleted_has_no_images() {
        let mut b = InputBox::new();
        b.attach_image(pasted(1));
        backspaces(&mut b, "[Image #1]".chars().count());
        type_str(&mut b, "hi");
        assert!(matches!(b.handle_key(code(KeyCode::Enter)), Action::Submit(_)));
        assert!(b.take_images().is_empty());
    }

    #[test]
    fn token_ten_does_not_keep_image_one_alive() {
        let mut b = InputBox::new();
        for i in 1..=10 {
            b.attach_image(pasted(i));
        }
        // delete tokens 1..=9 from the front: Home, then Delete the first 9 tokens.
        b.handle_key(code(KeyCode::Home));
        for _ in 0.."[Image #1]".len() * 9 {
            b.handle_key(code(KeyCode::Delete));
        }
        assert_eq!(b.value(), "[Image #10]");
        assert!(matches!(b.handle_key(code(KeyCode::Enter)), Action::Submit(_)));
        assert_eq!(b.take_images(), vec![pasted(10)]);
    }

    #[test]
    fn take_images_drains() {
        let mut b = InputBox::new();
        b.attach_image(pasted(1));
        b.handle_key(code(KeyCode::Enter));
        assert_eq!(b.take_images().len(), 1);
        assert!(b.take_images().is_empty());
    }

    #[test]
    fn clear_resets_images_and_numbering() {
        let mut b = InputBox::new();
        b.attach_image(pasted(1));
        b.clear();
        b.attach_image(pasted(2));
        assert_eq!(b.value(), "[Image #1]");
    }

    #[test]
    fn paste_of_an_image_path_becomes_a_token() {
        let dir = std::env::temp_dir().join(format!("usta editor paste {}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("shot one.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255]))
            .save(&p)
            .unwrap();
        let mut b = InputBox::new();
        b.paste(&p.to_str().unwrap().replace(' ', "\\ "));
        assert_eq!(b.value(), "[Image #1]");
        assert!(matches!(b.handle_key(code(KeyCode::Enter)), Action::Submit(_)));
        let imgs = b.take_images();
        assert_eq!((imgs[0].width, imgs[0].height), (4, 4));
    }

    #[test]
    fn paste_of_ordinary_text_is_inserted_verbatim() {
        let mut b = InputBox::new();
        b.paste("hello\nworld");
        assert_eq!(b.value(), "hello\nworld");
        b.clear();
        b.paste("/no/such/file.png");
        assert_eq!(b.value(), "/no/such/file.png");
    }

    #[test]
    fn clipboard_image_text_and_empty_are_applied() {
        let mut b = InputBox::new();
        b.apply_clip(Clip::Image(pasted(5)));
        b.apply_clip(Clip::Text(" and text".into()));
        b.apply_clip(Clip::Empty);
        assert_eq!(b.value(), "[Image #1] and text");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib tui::editor 2>&1 | tail -20`
Expected: compile errors (`no method named attach_image / paste / apply_clip / take_images`).

- [ ] **Step 3: Implement**

In `src/tui/editor.rs`:

(a) Imports - add next to `use crate::tui::theme;`:

```rust
use crate::image_attach::PastedImage;
use crate::image_paste::{self, Clip};
```

(b) `InputBox` fields (append to the struct, with doc comments):

```rust
    /// Images attached to the text being composed, with their `[Image #N]` number.
    images: Vec<(usize, PastedImage)>,
    /// Next token number (1-based, restarts after every submit / clear).
    next_image: usize,
    /// Images of the last submitted line - drained by `take_images`.
    submitted: Vec<PastedImage>,
```

and in `new()` add `images: Vec::new(), next_image: 1, submitted: Vec::new(),`.

(c) Add a free function next to `area_with`:

```rust
fn image_token(n: usize) -> String {
    format!("[Image #{n}]")
}
```

(d) Ctrl+V - in `handle_key`, directly after the `Action::Exit` early-return block (`if ctrl && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('d')) { return Action::Exit; }`):

```rust
        if ctrl && key.code == KeyCode::Char('v') {
            self.paste_clipboard();
            return Action::None;
        }
```

(e) Enter arm - change the body so the reconciled images are stashed BEFORE `clear()` wipes the pending list:

```rust
            KeyCode::Enter => {
                let line = self.value().trim().to_string();
                if line.is_empty() {
                    return Action::None;
                }
                self.history.push(line.clone());
                self.submitted = self.referenced_images(&line);
                self.clear();
                Action::Submit(line)
            }
```

(f) `clear()` - add two lines:

```rust
        self.images.clear();
        self.next_image = 1;
```

(g) New methods, placed right after `insert_str`:

```rust
    /// Insert an `[Image #N]` token at the cursor and keep the image for submit.
    pub fn attach_image(&mut self, img: PastedImage) {
        let n = self.next_image;
        self.next_image += 1;
        self.ta.insert_str(image_token(n));
        self.images.push((n, img));
    }

    /// Bracketed paste: a dragged-in image file (the terminal pastes its path) becomes a
    /// token; anything else is ordinary text.
    pub fn paste(&mut self, s: &str) {
        match image_paste::load_dropped(s) {
            Some(img) => self.attach_image(img),
            None => self.insert_str(s),
        }
    }

    /// Ctrl+V: clipboard image => token; clipboard text => inserted; nothing => no-op.
    pub fn paste_clipboard(&mut self) {
        self.apply_clip(image_paste::read_clipboard());
    }

    fn apply_clip(&mut self, clip: Clip) {
        match clip {
            Clip::Image(img) => self.attach_image(img),
            Clip::Text(t) => self.insert_str(&t),
            Clip::Empty => {}
        }
    }

    /// Pending images whose `[Image #N]` token survives in `line`, in attach order.
    fn referenced_images(&mut self, line: &str) -> Vec<PastedImage> {
        std::mem::take(&mut self.images)
            .into_iter()
            .filter(|(n, _)| line.contains(&image_token(*n)))
            .map(|(_, img)| img)
            .collect()
    }

    /// Images of the last submitted line (drained). Call right after `Action::Submit`.
    pub fn take_images(&mut self) -> Vec<PastedImage> {
        std::mem::take(&mut self.submitted)
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib tui::editor 2>&1 | tail -8`
Expected: all editor tests pass, including the 9 new ones. (If `token_ten_does_not_keep_image_one_alive` fails because `KeyCode::Delete` is unmapped in `edit()`, replace its deletion loop with `b.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL))`-free alternative: build the text with `b.clear()`-less approach by attaching 10 images, then `Home` + `Delete` is the only intended path - check `rg "KeyCode::Delete" src/tui/editor.rs` first; ratatui-textarea maps Delete to `delete_next_char`.)

- [ ] **Step 5: Check file budget, format, commit**

```bash
awk '/#\[cfg\(test\)\]/{exit} END{print NR}' src/tui/editor.rs
rustfmt --edition 2021 src/tui/editor.rs
git add src/tui/editor.rs
git commit -m "feat(image): editor image tokens, paste routing, Ctrl+V, submit reconciliation"
```
Expected: the awk line count (production lines up to the test module) is <= 600. If not, move the new image methods into a sibling `src/tui/editor_images.rs` as a second `impl InputBox` block (fields stay `pub(super)`-visible via `pub(super)`), then re-run Step 4.

---

## Task 6: Wire into the session loop, echo, version bump, verification

**Files:**
- Modify: `src/tui/page.rs` (append `page_image_echo`)
- Modify: `src/tui/run.rs` (4 quoted edits inside `tokio::select!`)
- Modify: `src/tui/ask.rs` (1 quoted edit)
- Modify: `Cargo.toml`, `Cargo.lock`, `README.md`, `SPEC.md`, `src/tui/welcome_tests.rs` (v0.34.0; mirror `git show 6004da3`)

**Interfaces:**
- Consumes: `InputBox::{paste, take_images}` (Task 5), `PastedImage::echo_label` (Task 1), `Session::push_user_with_images` (Task 3), `page_notice` (existing).
- Produces: `pub(crate) fn page_image_echo(tui: &mut Tui, images: &[PastedImage]) -> Result<()>`.

- [ ] **Step 1: `page_image_echo`**

Append to `src/tui/page.rs` (after `page_user_echo`; add `use crate::image_attach::PastedImage;` to the imports):

```rust
/// One `🖼 image N (W×H)` line per attached image, right under the user's echo.
pub(crate) fn page_image_echo(tui: &mut Tui, images: &[PastedImage]) -> Result<()> {
    for (i, img) in images.iter().enumerate() {
        page_notice(tui, &img.echo_label(i + 1))?;
    }
    Ok(())
}
```

- [ ] **Step 2: `run.rs` edits** (match on the quoted code; all inside `tokio::select!`, so rustfmt leaves them alone)

1. Paste routing. Replace
   `Event::Paste(s) => { editor.insert_str(&s); continue }`
   with
   `Event::Paste(s) => { editor.paste(&s); continue }`

2. Take the images for a real message. Directly ABOVE the line `let outgoing = if crate::slash::is_exam_command(&line) {` insert (one line; `/exam` and `/game` synthesize their own prompt, so a stray image is dropped there):
   ```rust
                        let images = if crate::slash::is_exam_command(&line) || game_cmd.is_some() { Vec::new() } else { editor.take_images() };
   ```

3. Echo. In the final `else` branch of that `let outgoing = ...` chain, directly after
   `crate::tui::page::page_user_echo(&mut tui, &line)?;` (the one under the comment `// Push the submitted line to scrollback as a distinct user block.`) insert:
   ```rust
                            crate::tui::page::page_image_echo(&mut tui, &images)?;
   ```

4. Push. Replace
   `session.push_user(&outgoing);`
   with
   `session.push_user_with_images(&outgoing, &images);`
   (keep `recorder.user(&outgoing);` as is - the transcript stays text with the `[Image #N]` tokens).

Check the budget: `wc -l src/tui/run.rs` must be <= 600 (589 base, +1 fullscreen, +2 here).

- [ ] **Step 3: `ask.rs` edit**

In `ask_live`, replace
```rust
                if let Event::Paste(s) = &ev {
                    editor.insert_str(s);
```
with
```rust
                if let Event::Paste(s) = &ev {
                    editor.paste(s);
```
(`tui_confirm`'s `editor.insert_str(&s)` stays: yes/no prompts never take images.) Ctrl+V during the locked wait already works - the key reaches `editor.handle_key` through `LockedKey::Edit`.

- [ ] **Step 4: Build and run the whole suite**

Run: `cargo build 2>&1 | tail -5 && cargo test 2>&1 | tail -8`
Expected: build clean (no warnings from the new code); tests pass except the pre-existing `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt`.

Run: `rustfmt --edition 2021 --check src/image_attach.rs src/image_paste.rs src/anthropic.rs src/session.rs src/context_report.rs src/backend.rs src/tui/editor.rs src/tui/page.rs`
Expected: no output. (Do not run it on `run.rs`/`ask.rs` edits beyond the select! body.)

- [ ] **Step 5: Version bump, docs**

Run `git show --stat 6004da3` and apply the same kind of change to the same five files with version 0.34.0: `Cargo.toml` `version = "0.34.0"` (and `Cargo.lock` `usta` entry), the version string asserted in `src/tui/welcome_tests.rs`, a README paragraph under the input/keys section ("Ctrl+V pastes a screenshot from the clipboard; dragging an image file into the terminal attaches it; delete the `[Image #N]` token to drop it"), and a SPEC.md line describing: `[Image #N]` tokens, images on the last CLI message via stream-json, 1568 px / 5 MB caps, text-only recorder.

Run: `cargo test 2>&1 | tail -5`
Expected: same single pre-existing failure only.

- [ ] **Step 6: Manual checklist (iTerm2 and Ghostty; both backends - `ANTHROPIC_API_KEY` set for API, unset for CLI)**

1. Screenshot to clipboard (Cmd+Ctrl+Shift+4), focus usta, press **Ctrl+V** -> `[Image #1]` appears; type "what do you see?" -> Enter -> echo shows `🖼 image 1 (W×H)`; the answer describes the screenshot.
2. Press Ctrl+V with TEXT on the clipboard -> text is inserted (no token).
3. Drag a PNG from Finder into the window -> `[Image #1]` (no path text). Drag a `.txt` -> the path is inserted as text. Drag a file whose name contains spaces.
4. Attach two images, delete one token with Backspace, submit -> echo shows ONE image line; the answer references only that image.
5. Follow-up question without a new image: "what color was the top-left?" -> still answered (API: image is in `messages`; CLI: `--resume`).
6. Large screenshot (Retina full screen, > 1568 px): echo shows the downscaled size (long edge 1568).
7. CLI backend: `/context` still prints (no base64 flood); `/quit` flow (progress flush) still works after an image turn.
8. Observation to record: Cmd+V with an image-only clipboard in each terminal (expected: nothing arrives or an empty paste - Ctrl+V is the supported path; note the result in the PR).

- [ ] **Step 7: Commit**

```bash
git add src/tui/page.rs src/tui/run.rs src/tui/ask.rs Cargo.toml Cargo.lock README.md SPEC.md src/tui/welcome_tests.rs
git commit -m "feat(image): wire image paste into the session loop; v0.34.0"
```

---

## Self-Review (against `2026-10-26-image-paste-design.md`)

- Ctrl+V clipboard via `arboard`, image => token, else text paste: Tasks 2 + 5 (`read_clipboard`, `apply_clip`, Ctrl+V arm).
- Drag-and-drop single existing path, quotes / escaped spaces, png/jpg/jpeg/gif/webp, else normal paste: Task 2 (`parse_dropped_path`) + Task 5 (`paste`) + Task 6 (route `Event::Paste`).
- Delete token => image dropped at submit: Task 5 (`referenced_images`, tests for deletion and the `#1` vs `#10` substring trap).
- API backend content blocks: Task 3 (`user_with_images`, serialization test). CLI backend: Task 4 (stream-json, spike-verified, live ignored test).
- Echo line per image `🖼 image N (W×H)`: Tasks 1 + 6.
- Size limit / downscale before encoding, preserve type where possible: Task 1 (passthrough keeps original bytes and type; resize => PNG, JPEG fallback). The "preserving type" requirement is met for everything within limits; oversized JPEG/WebP/GIF become PNG (or JPEG when the PNG is still > 5 MB) - lossless-first is deliberate for screenshots.
- History: API keeps array content in `messages`; CLI relies on `--resume` (Task 4 note: no-resume fallback loses earlier pixels).
- Testing list in the spec: path detection, token reconciliation, API serialization, downscale math - all present; manual checklist in Task 6.
- Type consistency: `PastedImage` fields/`to_block`/`echo_label` (Task 1) are used verbatim in Tasks 2-6; `Message::{user_with_images,text,image_blocks}` (Task 3) in Task 4; `InputBox::{paste,take_images,attach_image}` (Task 5) in Task 6; `run_claude_cli` gains `images` as the 5th parameter in both call sites.
