# Polish round: images, in-app selection, fullscreen fixes. Design

Date: 2026-10-26 · Status: draft (decisions made agency-style, see "Needs user input") · Base: `main` @ v0.34.0

## Goal
Close the known gaps left by the fullscreen and image-paste merges. In priority order:
(A) image turns must never break the session, the progress save included, and must behave predictably;
(B) mouse capture took away native drag-select, so bring it back inside the app the way opencode does;
(C) fix the fullscreen rough edges and pay down structure debt (`run.rs` budget, untestable `page*`).

Each decision below gives the chosen option with one line of why, plus the rejected alternative in one line.

---

## A. Images

### A1 (critical): history image budget
**Cause.** Every image turn is stored with its base64 blocks forever: `Session::push_user_with_images` (session.rs:34) → `Message::user_with_images` (anthropic.rs:28). Every API call sends the whole history (`MessageRequest::new(.., history.to_vec())`, anthropic.rs:193), and so does the closing flush (`let mut msgs = history.to_vec()`, lifecycle.rs:139). Once the request passes 32 MB or 100 images, every call fails, including the end-of-session progress save, which means the session's learning is lost.

**Design.** Prune in the session, once, at push time.
- New pure fn in `anthropic.rs`: `pub fn prune_images(history: &mut [Message], max_count: usize, max_b64: usize) -> usize`. It walks messages newest→oldest and, inside each message, image blocks last→first. It keeps a block while the running count stays ≤ `max_count` and the running sum of `source.data.len()` (the base64 length) stays ≤ `max_b64`. Every other image block is replaced in place by `{"type":"text","text":"[earlier image omitted to stay under the request size limit]"}`. Returns the number of blocks newly replaced. Placeholders are skipped, so the fn is idempotent.
- Constants in `session.rs`: `HISTORY_MAX_IMAGES = 20`, `HISTORY_MAX_IMAGE_B64 = 20 * 1024 * 1024`. 20 images stays far under the 100-image limit and under the ">20 images ⇒ ≤2000 px" rule (we cap at 1568 px). 20 MB of base64 leaves about 12 MB under the 32 MB request cap for text plus the closing prompt.
- `Session::push_user_with_images` becomes `-> usize` (images omitted). It pushes, then calls `prune_images`. `compact` needs no change, because its tail is a subset of an already-pruned history.
- `run.rs:550`: when the count is > 0, `page_notice("N older image(s) dropped from context (size limit)")`.
- Every consumer is covered automatically: the API (`ask_live`), the closing flush, `file_feedback`, and the CLI no-resume re-send (A6). All of them read `session.history()`.

**Why:** one monotonic pass at the single write site also bounds memory and covers every caller. **Rejected:** filtering per request in `AnthropicClient::complete`. It repeats the work every call, keeps memory unbounded, and would also have to be wired into flush and the CLI separately.

Edge case: one message whose own images exceed the budget loses its oldest images. The notice covers this, and it is rare after A2 (a typical JPEG is about 0.5 MB).

**Files:** anthropic.rs, session.rs, tui/run.rs (1 line). **Tests:** `prune_images` unit tests: count cap, byte cap, newest-first survival, idempotence, placeholder `text()` stays token-free. A session test pushes 25 small image turns and asserts 20 survive, then serializes a `MessageRequest` and asserts the image count.

### A2: keep JPEG as JPEG
**Cause.** `shrink` (image_attach.rs:79) always encodes PNG first. A downscaled photo JPEG comes out 3–5× bigger, and the JPEG fallback uses the encoder's default quality.

**Design.** `shrink(img, src: ImageFormat)`. `prepare` passes the sniffed format, and `from_rgba` passes `Png` (clipboard screenshots compress well as PNG).
- `src == Jpeg`: encode with `codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 85)` and return `image/jpeg`.
- Anything else (PNG/GIF/WebP): PNG, with a JPEG q85 fallback if it is still over `MAX_RAW_BYTES`. This is the existing ladder, with the quality now explicit.
- A shared helper `encode_jpeg(&DynamicImage) -> Result<Vec<u8>>` is used by both branches.

**Why:** this is the minimal change that keeps the format, and the 5 MB raw cap is unchanged. **Rejected:** WebP output. That encoder is lossless-only in `image` 0.25 and would be bigger.

**Files:** image_attach.rs. **Tests:** a 3000×2000 noisy JPEG becomes `image/jpeg`, 1568 px, and smaller than its PNG encoding. A 1568² q85 noise JPEG stays ≤ `MAX_RAW_BYTES`. The existing PNG tests stay unchanged.

### A3: up-arrow recalls the images too
**Cause.** `InputBox.history: Vec<String>` (editor.rs:28) stores text only. The recall functions `recall_prev`/`recall_next` (editor.rs:293–322) restore the token, and `self.images` still holds the stash's images. So a recalled `[Image #1]` either resolves to nothing, or (a latent bug) resolves to the wrong image from the half-typed stash.

**Design.**
- `struct Entry { text: String, images: Vec<(usize, Arc<PastedImage>)> }`. Both `history` and `stash` become `Entry`, and `images` becomes `Vec<(usize, Arc<PastedImage>)>`.
- Submit (editor.rs:105) pushes `Entry { text, images: referenced }`. `take_images` clones the images out of the Arcs, so the session API is unchanged.
- Recall swaps `images` together with the text and sets `next_image = max(n)+1`. Down to the stash restores the stash's own images.
- Memory: only the last 20 history entries keep images (older entries' `images` are cleared on push). This matches A1's horizon.

**Why:** the image travels with its token, and Arc avoids copying up to 5 MB per recall. **Rejected:** stripping tokens on recall. That would silently send different content than the user sees.

**Files:** tui/editor.rs (production is about 486 lines, which stays under budget). **Tests:** submit image line → Up → Enter → `take_images` returns the same image. Stash with image #1 → Up to an entry with image #1 → the entry's image is sent, not the stash's. Down restores the stash image.

### A4: no image attach before the session starts
**Cause.** Ctrl+V is handled inside `InputBox::handle_key` (editor.rs:84) whichever loop owns the editor. The intro (intro.rs:101), entry (entry.rs:286) and confirm (ask.rs:113) loops never call `take_images` (only run.rs:526 does). Their tokens point at images that are never sent.

**Design.** Disable it there: add an `allow_images: bool` flag on `InputBox`, default `false`, with `pub fn set_allow_images(&mut self, on: bool)`. `run.rs` sets it to `true` once, right before the main `loop` (run.rs:432).
- With the flag off, `paste()` goes straight to `insert_str` (a dropped path stays text).
- With the flag off, a `Clip::Image` from Ctrl+V does nothing except `ui::notice("images can be attached once the session has started")`.

**Why:** the pre-lock turns are stitched into the session as text (`intro::stitch`), and carrying images through `IntroTurn` buys little. **Rejected:** full support, which would mean images in `IntroTurn`, stitch, the slug ask and the confirm.

**Files:** tui/editor.rs, tui/run.rs (1 line). **Tests:** the flag defaults off. A clip image with the flag off inserts no token and pushes a notice. A dropped path with the flag off becomes literal text.

### A5: tell the user when an image fails to load
**Cause.** `load_dropped` (image_paste.rs:59) does `.ok()?`, so an unreadable or corrupt dropped image silently becomes path text. `read_clipboard` (image_paste.rs:73) falls through to text when `from_rgba` fails.

**Design.**
- `load_dropped -> Option<Result<PastedImage>>`. `None` means the paste is not an image drop. `Some(Err(e))` means it looked like an image file but failed.
- `Clip::Failed(String)` is a new variant.
- In the editor, `Some(Err)` inserts the path text (the user sees what arrived) and calls `ui::notice(format!("couldn't attach image: {e}"))`. `Clip::Failed` shows the same notice.
- Notices raised outside the main loop must also show up, so `flush_notices` moves from `run.rs:436` into `page::draw` (draw already runs every iteration of every loop). This also fixes notices getting stuck during intro/entry.

**Files:** image_paste.rs, tui/editor.rs, tui/page.rs, tui/run.rs (remove 1 line). **Tests:** a corrupt `.png` gives `Some(Err)`. A text file gives `None`. Editor test: a failed drop inserts the text and leaves one buffered notice (`ui::drain_tui_notices`, with `set_tui_active(true)` in the test).

### A6: CLI no-resume turns re-send the budgeted images
**Cause.** On the no-resume path, `render_transcript` (backend.rs:261) is text only and `last_user_images` (backend.rs:206) attaches only the last turn's images. That path is taken on the first turn, after a stale-session retry (backend.rs:155), after a cancel (run.rs:415, :570), after stitch (run.rs:367) and after compaction. In all of those cases the model loses every earlier image.

**Design.** In `Backend::complete`'s CLI arm, choose the images by path:
- **Resume:** `last_user_images(history)`. Unchanged: the server-side session already holds the older images.
- **No resume (both the first call and the stale retry):** a new `history_images(history) -> Vec<Value>`. For each user message k (1-based among user messages) that still has image blocks, it emits one text block `"images attached to [SEN] message {k}:"` followed by that message's image blocks in order. `stream_input` already puts the images before the transcript text.

Because A1 already pruned the history, this is at most 20 images and 20 MB. That also makes a server-side session overgrown with images recoverable: the stale-retry path re-sends the bounded set.

**Why:** this reuses the proven stream-json path with no format change to `render_transcript` ([SEN]/[USTA] labels stay). **Rejected:** numbering turns inside the transcript, which changes a format the prompts rely on.

**Files:** backend.rs. **Tests:** `history_images` emits a label then images per user message, skips messages without images, skips placeholders, and gives an empty result for text-only history. A source pin asserts the `None` resume branch uses `history_images`.

### A7: images in the on-disk transcript are out of scope
**Cause.** `recorder.user(&outgoing)` (run.rs:551) writes `{"role","text"}` only (transcript.rs:16).

**Decision: out of scope, documented.** The `.jsonl` exists for the salvage flush (`transcript::read_history`), whose output is text progress files. The assistant replies already carry what the images taught. Storing files of up to 5 MB each under `.usta/sessions/` would bloat project dirs that users may commit. Add one SPEC §4 line saying that transcripts record the `[Image #N]` token only. **Rejected:** saving image files next to the session, because of the disk/git cost for a recovery-only record.

---

## B. In-app selection and copy (opencode-style)

**Cause.** `term.rs:36` enables `EnableMouseCapture` (crossterm sends `?1000h ?1002h ?1003h ?1015h ?1006h`), so the terminal's native drag-select is gone (Shift+drag is the only escape). `page::nav` (page.rs:97) swallows clicks.

**Mechanism.** A selection in flat chat-line coordinates, highlighted at render time and copied on release.
- **State (chat.rs):** `ChatBuffer` gains:
  - `area: Rect`, recorded by `visible()`. Its signature changes to `visible(area: Rect)` so mouse cells can be mapped.
  - `soft: Vec<bool>`, parallel to the flat cache: true when a row continues the previous logical line. `wrap_line` returns it.
  - `sel: Option<Sel { anchor: (usize, u16), head: (usize, u16) }>`, where `(flat line index, display column)`. Absolute indices keep the selection anchored while scrolling.
  - `flash: Option<&'static str>`.
- **Pure module `tui/select.rs`, all unit-tested:**
  - `cell_to_pos(area, start_line, x, y) -> Option<(usize,u16)>`
  - `ordered(sel)`
  - `extract(lines, soft, sel) -> String`: walks display columns with `unicode-width`, so wide chars never split. Each row's trailing spaces are trimmed. Soft-wrapped rows are joined with `""`, because word-wrap (C2) leaves the break space on the upper row. Hard rows are joined with `\n`.
  - `highlight(buf: &mut Buffer, area, start_line, sel)`: sets `Modifier::REVERSED` on the cells.
- **Events (page.rs `nav`):**
  - `Down(Left)` inside the chat area sets `sel = anchor = head`.
  - `Drag(Left)` updates `head`. If y is above the pane, `scroll_up(1)`; if it is below, `scroll_down(1)`. This is auto-scroll. It fires once per drag event, which is enough because terminals repeat drag events while the mouse moves.
  - `Up(Left)`: extract the text. If it is non-empty, call `copy(text)`, set `flash = Some("copied")`, and clear `sel`. If it is empty (a plain click), just clear.
  - The wheel still scrolls.
  - Any key or a new `Down` clears `flash`.
  - `C3` trimming and resize (`handle_resize`) clear `sel`, because flat indices are invalid after them.
- **Copy (`select::copy`):** `arboard::Clipboard` is held in a `OnceLock<Mutex<..>>` (on X11 the clipboard content dies with the handle). If arboard fails or `SSH_TTY` is set, also write OSC 52 (`\x1b]52;c;<base64>\x07`) to stdout. **Why:** native locally, and OSC 52 over SSH. **Rejected:** OSC 52 only, which Terminal.app ignores.
- **Render (page.rs `render_screen`):** after the chat `Paragraph`, call `select::highlight`. `flash` is drawn where the "↓ new message" indicator goes (C5 slot: right end of the editor's rule row).
- **Mouse mode (term.rs):** replace `EnableMouseCapture` with a small `Command` `EnableButtonMouse` that writes `?1000h ?1002h ?1006h` (button-event tracking: motion only while a button is held, SGR coordinates). `DisableMouseCapture` still turns everything off. This also solves C6. On Windows the console API path is unchanged.
- **Not done:** double-click word select and keyboard copy (keep it minimal). Selection in the editor already exists through tui-textarea (`ta.selection_range`).

**Files:** tui/chat.rs, new tui/select.rs, tui/page.rs, tui/term.rs. **Tests:**
- `select.rs`: mapping, reverse drag, wide chars, a soft-wrapped paragraph copied as one line, trailing-space trim.
- `nav` with synthetic `MouseEvent`s: Down/Drag/Up produces the expected `flash` and clears `sel`; drag below the pane scrolls.
- A `TestBackend` render shows reversed cells.
- Update the term.rs source pins (`EnableButtonMouse` in setup, `DisableMouseCapture` in restore).
- Manual: iTerm2/Ghostty drag-copy, plus paste into another app.

---

## C. Fullscreen polish

### C1: welcome panels re-render on resize
**Cause.** `Source::Fixed` (chat.rs:14, :91) is pre-rendered at push width and only clipped afterwards. Callers: run.rs:350–356 (`render_for_entry(.., w)`) and entry.rs:212 (`render_welcome_identity(.., width, ..)`).

**Design.** Replace `Fixed(Text)` with `Panel(Box<dyn Fn(u16) -> Text<'static>>)`. `render()` calls the closure at the current width. `page_fixed(tui, f: impl Fn(u16) -> Text<'static> + 'static)`. The callers move their owned data into the closure: `WelcomeData` is all-owned, and entry.rs clones `name/model/dir/local/other` into owned values. `render_for_entry`'s `None` stays a caller-side check, done once at push time. Clip stays as a safety net inside `render` for widths below the panel minimum. **Rejected:** storing an enum of panel kinds plus args, which needs more types for the same effect.

**Files:** tui/chat.rs, tui/page.rs, tui/run.rs, tui/entry.rs. **Tests:** a panel pushed and rendered at 100 then 60 gives the closure's output at 60 (a border char on the last column). Replace `fixed_entries_are_clipped_not_wrapped`.

### C2: word wrap for notices
**Cause.** `wrap_line` (chat.rs:48) is a greedy char-split, and `Source::Text` uses it (chat.rs:90).

**Design.** Make `wrap_line` word-aware. On overflow, break after the last space of the current row (that space stays on the upper row, which is invisible and keeps B's join exact). Fall back to the char-split only when one word is wider than the row. Return `(Vec<Line>, Vec<bool /*soft*/>)` for B. Styles stay per span, as today. `Reply`/`Echo` are already width-rendered by paint, so for them this is a no-op.

**Tests:** extend `wrap_line_*`: a breaking sentence, a long word char-split, wide chars, style kept across the break, and soft flags.

### C3: cap chat memory
**Cause.** `ChatBuffer.entries` (chat.rs:23) grows forever.

**Design.** `MAX_ENTRIES = 1000`. When pushing past it, drain the oldest 100 entries (amortized), invalidate the cache and clear `sel`. `offset` counts from the bottom, so the view does not jump; `visible` already clamps. **Rejected:** a line-count cap, which needs a re-render to measure.

**Tests:** after 1100 pushes, `entries.len() <= 1000`, the newest line is visible, and a scrolled-up view keeps showing the same text.

### C4: no double render on push
**Cause.** `push` (chat.rs:112) sets `cache = None`. When scrolled up it then calls `flat()` twice (before/after), which is two full re-renders. Even when following, each push forces a full re-render on the next draw.

**Design.** Incremental cache: if `cache` is `Some((w, lines))`, `render(&s, w)` only the new entry and extend `lines`/`soft`. The added row count gives the `offset` bump directly. Full invalidation is kept only for width change and C3 trimming.

**Tests:** a test-only render counter (`#[cfg(test)] renders: usize`) shows a push while scrolled up renders exactly one entry. The offset bump stays the same as today.

### C5: "↓ new message" must not cover the last line
**Cause.** page.rs:152–162 paints the indicator over the chat pane's last row.

**Design.** Paint it right-aligned on the editor's rule row (`box_area.y`), after `editor.render`. The rule is decoration only. The B `flash` shares this slot (flash wins).

**Tests:** in a `TestBackend` 80×24 with an unread flag set, chat row 20 still shows content, and row 21 starts with `─` and ends with "↓ new message".

### C6: no redraw on bare mouse motion
**Cause.** `?1003h` (any-motion) reports every move, `nav` returns `true` for every mouse event (page.rs:99–106), and each loop `continue`s into a full redraw (run.rs:437, ask.rs:41, intro.rs:99, entry.rs:284).

**Design.** This is solved at the source by B's `EnableButtonMouse` (no `?1003h`): `Moved` is never sent, and `Drag` arrives only while a button is held. If C ships before B, C carries the term.rs mouse-mode change on its own. **Rejected:** making every loop's redraw conditional, which means 5 loops restructured for one event type.

**Tests:** a term.rs source pin that `?1003h` is absent.

### C7: capture pdftotext output
**Cause.** `materials.rs:173` uses `Command::new("pdftotext")…status()`, which inherits stdout/stderr, so warnings print over the live TUI.

**Design.** Use `.stdout(Stdio::null()).stderr(Stdio::piped()).output()`. On failure, append the first stderr line to the existing note. **Tests:** a source pin that `.status()` is gone from that function. A behavioral test is skipped because it would need pdftotext.

### C8: stale comments
convert.rs:1 should read "Convert termimad ANSI output to ratatui Text (the chat pane renders it)". In polite.rs:658–660, change "bound to Terminal<TrackedBackend<Stdout>>" to "bound to a live `Tui` (crossterm terminal)". Comments only.

### C9: split `run.rs` (588/600)
**Design.** Move the topic-resolution block (run.rs:52–285, about 230 lines) to a new `tui/topic_flow.rs`: `pub(crate) async fn resolve_topic(tui, editor, events, backend, global, project_root, today, topic_arg) -> Result<Option<TopicPick>>`. `TopicPick { topic, had_topic_arg, resumed, intro: Option<String>, intro_turns: Option<Vec<IntroTurn>>, first_run_completed }`. `run.rs` drops to about 360 lines and keeps the lock, build, welcome, opening and main loop. The polite.rs wiring pins are unaffected (all needles live in the main loop). page.rs's `handle_nav` per-file count is unaffected (no event loop moves). **Why:** this is the largest self-contained block, with no shared mutable state other than its outputs. **Rejected:** extracting the slash dispatch (464–549), which is entangled with `session`/`recorder`/`pending` borrows.

**Tests:** a pure move. The suite stays green. Add `tui/topic_flow.rs` to the module-split list in SPEC §11 if one exists.

### C10: a testable seam for the `page*` wrappers
**Cause.** Every `page_*` (page.rs:21–77) takes `&mut Tui`, and `Tui` owns `Terminal<CrosstermBackend<Stdout>>` (term.rs:25). That makes it unconstructible in tests, so the wrappers have only source pins.

**Design.** The bodies only touch `tui.chat`. Move each body into a `ChatBuffer` method (`chat.notice(msg)`, `warn`, `error`, `user_echo`, `image_echo`, `reply`, `panel`, `flush_notices(msgs)`), each tested on `ChatBuffer::default()`. The `page_*` fns stay as one-line `tui.chat.x(..)` delegations, so the ~100 call sites are unchanged. `current_width` may read `tui.chat` width as a fallback. **Rejected:** a generic `Tui<B: Backend>`, which ripples a type parameter through every TUI signature.

**Tests:** user_echo jumps to the bottom; image_echo adds N lines with labels; a warn-prefixed message from flush lands as warn styling; an error line uses the error color.

---

## Plans, ordering and conflicts

Three plans, three branches, each cut from `main`:

| Plan | Branch | Scope | Main files |
|---|---|---|---|
| **P-A Images** | `image-polish` | A1–A6, A7 SPEC line | anthropic.rs, session.rs, image_attach.rs, image_paste.rs, backend.rs, tui/editor.rs, tui/page.rs (`draw` flush), tui/run.rs (3 lines) |
| **P-C Polish** | `tui-polish` | C1–C10 | tui/chat.rs, tui/page.rs, tui/term.rs (mouse mode), tui/run.rs (split), new tui/topic_flow.rs, tui/entry.rs, materials.rs, convert.rs, polite.rs |
| **P-B Selection** | `chat-select` | B | new tui/select.rs, tui/chat.rs, tui/page.rs |

**Order: P-A → P-C → P-B.**
- P-A goes first: it is the user-facing critical fix (A1) and is mostly outside the TUI files.
- P-C goes second. It does the `run.rs` split (C9) as its **first task**, so later edits land in the smaller files. It also rewrites `ChatBuffer` internals (word-wrap with soft flags, incremental cache, `visible(Rect)`, the C5 slot, the mouse mode), which B builds on.
- P-B goes last. It is the purest addition and consumes P-C's `soft` flags, `area`, rule-row slot and button-mouse mode.

**Conflicts.**
- **`tui/run.rs`:** P-A touches 3 lines (notice at :550, `set_allow_images` before the loop, remove `flush_notices` at :436). P-C moves lines 52–285. They are disjoint hunks, but P-C must rebase on P-A. If they run in parallel, do C9 after merging P-A.
- **`tui/page.rs`:** P-A moves `flush_notices` into `draw`. P-C C10 moves the wrapper bodies into `ChatBuffer`, including `flush_notices(msgs)`. P-C keeps the call inside `draw`. P-B edits `nav` and `render_screen`.
- **`tui/chat.rs`:** P-C and P-B both edit it, so B must wait for C. This is the reason for the order.
- **`tui/term.rs`:** C6 and B's mouse mode are the same change. It is owned by P-C, and P-B only adds `Drag` handling.
- **Versions:** P-A v0.35.0, P-C v0.35.x/0.36.0, P-B v0.36.0. Each plan bumps `Cargo.toml` and its SPEC section.

## Needs user input / suggested drops
- **A7:** out of scope as designed. Confirm that transcripts recording only tokens is acceptable.
- **B "copied" feedback:** the flash sits in the rule-row slot and is cleared by the next event. Confirm this rather than adding a timed status-bar toast.
- **A4:** confirm that "no images before the session starts" is acceptable for the first-run intro.
- **C3 numbers (1000/100) and A1 numbers (20 images / 20 MB):** chosen conservatively. The user can tune them, but these are not blockers.
