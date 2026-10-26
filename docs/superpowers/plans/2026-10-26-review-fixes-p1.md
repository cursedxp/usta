# Review Fixes P1 (data loss / broken session) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the six P1 review findings: no closing/compaction write from a cut-off or malformed reply, a `.bak` that survives a whole session, watcher ignore rules judged below the project root, no empty assistant turn, no corrupt image or failed turn left in history. Ship as v0.38.0.

**Architecture:** `backend::Reply` gains `truncated` (API `stop_reason == "max_tokens"`); the closing flush asks for a bigger output budget through `Backend::complete_with_limit` and passes the reply through one pure gate, `progress::parse_closing_reply`, before any write. `Backend::complete_with_limit` also refuses empty replies (`ensure_nonempty`). `write_atomic` backs up each path once per process. The watcher strips the (raw or canonical) root before applying ignore rules. Dropped images are fully decoded; every push→ask site records a history mark and calls `lifecycle::rollback_turn` on error, which truncates history and writes a transcript `rollback` line that `read_history` honors.

**Tech Stack:** Rust 2021 (rust-version 1.88), tokio, reqwest, serde_json, notify v6, image crate, anyhow.

Spec: `docs/superpowers/specs/2026-10-26-review-fixes-design.md`, sections "Global constraints", "P1", "Release".
Verification notes (local, git-ignored): `.superpowers/sdd/review-fixes/verify-A.md`, `verify-B.md`.

Work happens in the worktree `../usta-fix-p1` on branch `fix-p1`. Every path and command below is relative to that worktree's root.

## Global Constraints

Copied from the spec:
- 600-line production budget per file (lines before the first `#[cfg(test)]`, SPEC §11). `setup.rs` is already 611 → it must shrink when touched (P2-3 moves reset code out). (Not touched in P1.)
- `cargo fmt` scoped to edited files; `cargo fmt --check` crate-wide clean; `cargo clippy --all-targets -- -D warnings` clean.
- `cargo test`: the only allowed failure is `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt` (pdftotext installed on the dev machine). P2-11 makes the tool injectable, after which that test must pass too.
- No emoji in any new UI text. Notices are short, lowercase, English (matching existing notices).
- Surgical: no refactors beyond what an item needs. Source-pin tests (closing.rs:98-165, polite.rs:696) must stay green; update a pin only when the pinned line itself is the one being changed.
- No new crates except enabling tokio's `signal` feature (P2-9). (Not needed in P1.)

Plan-level additions:
- `flush_progress`'s signature does not change (closing.rs pins require the literal `flush_progress(` at its call sites).
- The crate is bin-only: use `cargo test <filter>`, never `--lib`.
- Commits: `git add` files by name, never `git commit -a`. Every commit message ends with exactly these two lines:
  ```
  Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
  ```

### The gate (run at the end of every task)

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|panicked" | head -20
cargo clippy --all-targets -- -D warnings
rustfmt --edition 2021 <every .rs file this task edited>
cargo fmt --check
for f in <every .rs file this task edited>; do n=$(grep -n '^#\[cfg(test)\]' "$f" | head -1 | cut -d: -f1); if [ -n "$n" ]; then echo "$f $((n-1))"; else echo "$f $(wc -l < "$f") (no test mod)"; fi; done
```

Baseline at `main` @ b225584: 648 passed, 1 failed (the allowed `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt`), 1 ignored. Each task states the expected pass count after it (one failure, the allowed one, throughout). Every production count must be ≤ 600; the expected counts given per task are approximate (rustfmt decides line breaks).

Run `rustfmt` BEFORE the line count — counts are after formatting.

Baseline production line counts: anthropic.rs 269, backend.rs 497, progress.rs 520 (tests in progress_tests.rs), lifecycle.rs 302, watcher.rs 212, image_attach.rs 135, session.rs 73, transcript.rs 161, tui/run.rs 386 (no test mod), plain.rs 486 (no test mod), file_feedback.rs 522.

---

### Task 1: `Reply.truncated` + per-call output budget plumbing (P1-1, part 1)

**Files:**
- Modify: `src/anthropic.rs` (`MAX_TOKENS` :14, `MessageRequest::new` :146-165, `MessageResponse` :168-173, `Client::complete` :221-268, tests :302-345)
- Modify: `src/backend.rs` (`Reply` :21-27, `Backend::complete` :127-170)
- Modify: `src/file_feedback.rs` (`display_reply` constructions :229-233 and :375-379)
- Modify: `src/session.rs` test at :164 (`MessageRequest::new` call)
- Commit also: `docs/superpowers/plans/2026-10-26-review-fixes-p1.md` (copy it from the main checkout `../usta/docs/superpowers/plans/` if it is not in the worktree)

**Interfaces:**
- Produces:
  - `pub const MAX_TOKENS: u32 = 8000;` in `anthropic.rs` (now `pub`)
  - `MessageRequest::new(model: String, system: String, messages: Vec<Message>, max_tokens: u32) -> MessageRequest`
  - `Client::complete(&self, model: &str, system: &str, history: &[Message], max_tokens: u32) -> Result<(String, bool, Option<u64>, bool)>` — the 4th element is `truncated`
  - `backend::Reply { text: String, web: bool, context_tokens: Option<u64>, truncated: bool }`
  - `Backend::complete_with_limit(&mut self, system: &str, history: &[Message], max_tokens: u32) -> Result<Reply>`; `Backend::complete(&mut self, system, history)` delegates with `anthropic::MAX_TOKENS` (signature unchanged)

No behavior change yet: nothing reads `truncated` except the plumbing; the closing flush is switched over in Task 2.

- [ ] **Step 1: Write the failing tests** — append inside `mod tests` of `src/anthropic.rs` (after `request_serializes_expected_shape`):

```rust
    #[test]
    fn response_with_max_tokens_stop_is_truncated() {
        let cut: MessageResponse = serde_json::from_str(
            r#"{"content":[{"type":"text","text":"===FILE: progress===\n# Ru"}],"stop_reason":"max_tokens"}"#,
        )
        .unwrap();
        assert!(cut.truncated());
        let done: MessageResponse =
            serde_json::from_str(r#"{"content":[],"stop_reason":"end_turn"}"#).unwrap();
        assert!(!done.truncated());
    }

    #[test]
    fn request_carries_the_requested_output_budget() {
        let req = MessageRequest::new(
            DEFAULT_MODEL.into(),
            "s".into(),
            vec![Message::user("x")],
            32000,
        );
        assert_eq!(serde_json::to_value(&req).unwrap()["max_tokens"], 32000);
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test response_with_max_tokens_stop_is_truncated`
Expected: compile error — `no method named truncated` and `this function takes 3 arguments but 4 arguments were supplied`.

- [ ] **Step 3: Implement in `src/anthropic.rs`**

Replace line 14 `const MAX_TOKENS: u32 = 8000;` with:

```rust
/// Output budget for a normal chat turn (adaptive thinking shares it).
pub const MAX_TOKENS: u32 = 8000;
```

`MessageRequest::new` (:146) — add the parameter and use it:

```rust
    pub fn new(model: String, system: String, messages: Vec<Message>, max_tokens: u32) -> Self {
        MessageRequest {
            model,
            max_tokens,
            system,
            // ... rest unchanged
```

After the `MessageResponse` struct (:173) add:

```rust
impl MessageResponse {
    /// The reply hit the output budget — its text is cut off.
    fn truncated(&self) -> bool {
        self.stop_reason.as_deref() == Some("max_tokens")
    }
}
```

`Client::complete` (:221-268): doc line `/// Returns the final (text, used_web_search, context_tokens) triple.` becomes `/// Returns (text, used_web_search, context_tokens, truncated) of the final response.`; the signature becomes:

```rust
    pub async fn complete(
        &self,
        model: &str,
        system: &str,
        history: &[Message],
        max_tokens: u32,
    ) -> Result<(String, bool, Option<u64>, bool)> {
        let mut req = MessageRequest::new(
            model.to_string(),
            system.to_string(),
            history.to_vec(),
            max_tokens,
        );
```

and the final return (:264):

```rust
            return Ok((
                extract_text(&parsed.content),
                web,
                tokens,
                parsed.truncated(),
            ));
```

Existing tests: `request_serializes_an_image_message_as_a_content_array` (:304) and `request_serializes_expected_shape` (:330) get a 4th argument `MAX_TOKENS`; in `request_serializes_expected_shape` add `assert_eq!(v["max_tokens"], 8000);` after the model assert.

`src/session.rs:164` (test): `crate::anthropic::MessageRequest::new("m".into(), "s".into(), s.history().to_vec());` → add `, crate::anthropic::MAX_TOKENS` as the 4th argument.

- [ ] **Step 4: Implement in `src/backend.rs`**

`Reply` (:21-27):

```rust
/// A single completion result — text + web hint + context fullness.
pub struct Reply {
    pub text: String,
    pub web: bool,
    /// Total context tokens of the last call (input + cache) — for the indicator.
    pub context_tokens: Option<u64>,
    /// The model hit the output budget — `text` is cut off (API `stop_reason:
    /// max_tokens`). Always false on the CLI: its output-limit hit exits 1 → Err.
    pub truncated: bool,
}
```

Replace `Backend::complete` (:127-170) with a delegating `complete` plus `complete_with_limit` holding the old body:

```rust
    /// Request a completion from the selected backend, returns `Reply`.
    /// In CLI mode web usage can't be detected from the text → `false`.
    pub async fn complete(&mut self, system: &str, history: &[Message]) -> Result<Reply> {
        self.complete_with_limit(system, history, anthropic::MAX_TOKENS)
            .await
    }

    /// `complete` with an explicit output budget — the closing/compaction flush
    /// needs more room than a chat turn. The CLI backend ignores it.
    pub async fn complete_with_limit(
        &mut self,
        system: &str,
        history: &[Message],
        max_tokens: u32,
    ) -> Result<Reply> {
        match self {
            Backend::Api { client, model } => {
                let (text, web, tokens, truncated) =
                    client.complete(model, system, history, max_tokens).await?;
                Ok(Reply {
                    text,
                    web,
                    context_tokens: tokens,
                    truncated,
                })
            }
            Backend::Cli {
                model,
                session_id,
                bin,
            } => {
                // ... body unchanged from the old `complete` (resume / retry logic,
                // keep the lines pinned by `no_resume_calls_send_history_images`
                // byte-identical) ... except the final value:
                Ok(Reply {
                    text,
                    web: false,
                    context_tokens: tokens,
                    truncated: false,
                })
            }
        }
    }
```

- [ ] **Step 5: Implement in `src/file_feedback.rs`**

Both `display_reply` literals (:229-233 in `handle_file_change`, :375-379 in `handle_batch_change`) become:

```rust
    let display_reply = backend::Reply {
        text: clean,
        web: reply.web,
        context_tokens: reply.context_tokens,
        truncated: reply.truncated,
    };
```

- [ ] **Step 6: Run to verify it passes**

Run: `cargo test anthropic`
Expected: PASS (including the two new tests and the updated `request_serializes_expected_shape`).

- [ ] **Step 7: Gate** (files: `src/anthropic.rs src/backend.rs src/file_feedback.rs src/session.rs`)

Expected: 650 passed, 1 failed (allowed), 1 ignored. Production: anthropic.rs ≈ 280, backend.rs ≈ 512, file_feedback.rs ≈ 524, session.rs 73.

- [ ] **Step 8: Commit**

```bash
git add docs/superpowers/plans/2026-10-26-review-fixes-p1.md src/anthropic.rs src/backend.rs src/file_feedback.rs src/session.rs
git commit -m "$(cat <<'EOF'
fix(api): carry max_tokens truncation and a per-call output budget

Reply.truncated comes from stop_reason == "max_tokens" (API; CLI always
false). Backend::complete_with_limit takes an explicit budget; complete
delegates with MAX_TOKENS. Groundwork for the closing-reply gate (P1-1).

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
EOF
)"
```

---

### Task 2: Gate the closing/compaction reply before any write (P1-1 + P1-2)

**Files:**
- Modify: `src/anthropic.rs` (add `CLOSING_MAX_TOKENS` under `MAX_TOKENS`)
- Modify: `src/progress.rs` (`split_files` :54-72, new `parse_closing_reply` right after it)
- Modify: `src/progress_tests.rs` (rewrite `split_files_without_delimiter_is_progress` :94-100, add tests)
- Modify: `src/lifecycle.rs` (`flush_core` :149-153 and :175-178; new pin test in `mod tests`)

**Interfaces:**
- Consumes (Task 1): `Backend::complete_with_limit(&mut self, system: &str, history: &[Message], max_tokens: u32) -> Result<Reply>`, `Reply.truncated: bool`
- Produces:
  - `pub const CLOSING_MAX_TOKENS: u32 = 32000;` in `anthropic.rs`
  - `pub fn parse_closing_reply(text: &str, truncated: bool) -> anyhow::Result<Vec<(String, String)>>` in `progress.rs`
  - `split_files` returns an empty vec when the reply has no `===FILE:` delimiter

On every Err path the callers already do the right thing (verified, verify-A.md): main.rs warns and leaves the transcript unfinished, recovery retries next start, `maybe_compact` warns and keeps history. This task only makes sure an untrustworthy reply becomes an Err before the first `write_atomic`.

- [ ] **Step 1: Write the failing tests** — in `src/progress_tests.rs`, replace `split_files_without_delimiter_is_progress` (:94-100) with:

```rust
#[test]
fn split_files_without_delimiter_is_empty() {
    // A conversational reply (apology, refusal, error text) is never a progress file.
    assert!(split_files("# Rust — İlerleme\niçerik").is_empty());
}

#[test]
fn parse_closing_reply_rejects_a_cut_off_reply() {
    let err = parse_closing_reply("===FILE: progress===\nP", true).unwrap_err();
    assert!(err.to_string().contains("cut off"), "{err}");
}

#[test]
fn parse_closing_reply_rejects_a_reply_without_sections() {
    let err =
        parse_closing_reply("Sorry, I can't summarize this session.", false).unwrap_err();
    assert!(err.to_string().contains("no file sections"), "{err}");
}

#[test]
fn parse_closing_reply_rejects_sections_without_names() {
    let err = parse_closing_reply("===FILE: ===\nP", false).unwrap_err();
    assert!(err.to_string().contains("no files"), "{err}");
}

#[test]
fn parse_closing_reply_rejects_an_empty_section() {
    let err = parse_closing_reply("===FILE: progress===\nP\n===FILE: approach===\n  \n", false)
        .unwrap_err();
    assert!(err.to_string().contains("approach"), "{err}");
}

#[test]
fn parse_closing_reply_accepts_a_complete_reply() {
    let files =
        parse_closing_reply("===FILE: progress===\nP\n===FILE: approach===\nA\n", false).unwrap();
    assert_eq!(
        files,
        vec![
            ("progress".to_string(), "P".to_string()),
            ("approach".to_string(), "A".to_string()),
        ]
    );
}
```

And in `src/lifecycle.rs` `mod tests` (after the last existing test) add the source pin (flush_core needs a live Backend, no mock exists — same pin class as closing.rs:98-165):

```rust
    /// Source pin: flush_core asks with the closing budget and passes the reply
    /// through the gate BEFORE the first write (review P1-1/P1-2).
    #[test]
    fn flush_core_gates_the_reply_before_any_write() {
        let prod = include_str!("lifecycle.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let start = prod.find("pub(crate) async fn flush_core").unwrap();
        let end = prod.find("pub(crate) async fn flush_progress").unwrap();
        let body = &prod[start..end];
        assert!(body.contains("complete_with_limit(system, &msgs, crate::anthropic::CLOSING_MAX_TOKENS)"));
        let gate = body
            .find("progress::parse_closing_reply(&reply.text, reply.truncated)?")
            .expect("gate");
        let write = body.find("progress::write_atomic(").expect("write");
        assert!(gate < write, "the gate must run before any write");
        assert!(!body.contains("split_files("));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test parse_closing_reply`
Expected: compile error — `cannot find function parse_closing_reply`.

- [ ] **Step 3: Implement**

`src/anthropic.rs`, directly under the `MAX_TOKENS` const:

```rust
/// Output budget for the closing / compaction flush: up to six full files plus
/// thinking — 8000 is what cut them off (review P1-1).
pub const CLOSING_MAX_TOKENS: u32 = 32000;
```

`src/progress.rs` — replace `split_files` (:54-72, doc comment included) with `split_files` + `parse_closing_reply`:

```rust
/// Split the closing reply into (name, content) pairs, one per `===FILE: <name>===`
/// section. No delimiter → no files: a conversational reply is never a progress file.
pub fn split_files(reply: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for chunk in reply.split(FILE_DELIM).skip(1) {
        let Some((header, body)) = chunk.split_once("===") else {
            continue;
        };
        let name = header.trim().to_string();
        if name.is_empty() {
            continue;
        }
        out.push((name, clean_markdown_reply(body)));
    }
    out
}

/// The gate before any closing/compaction write: a cut-off reply, a reply without
/// `===FILE:` sections, or an empty section is rejected — nothing is written and the
/// caller keeps the transcript / history (review P1-1, P1-2).
pub fn parse_closing_reply(text: &str, truncated: bool) -> Result<Vec<(String, String)>> {
    if truncated {
        anyhow::bail!("reply was cut off (max tokens) — nothing written");
    }
    if !text.contains(FILE_DELIM) {
        anyhow::bail!("reply has no file sections — nothing written");
    }
    let files = split_files(text);
    if files.is_empty() {
        anyhow::bail!("reply has no files — nothing written");
    }
    if let Some((name, _)) = files.iter().find(|(_, body)| body.is_empty()) {
        anyhow::bail!("empty file in reply: {name} — nothing written");
    }
    Ok(files)
}
```

`src/lifecycle.rs` `flush_core` — replace :149-153:

```rust
    let reply = ask_usta(backend, system, &msgs).await?;
    let files = progress::split_files(&reply.text);
    if files.is_empty() {
        anyhow::bail!("model produced no files — nothing was written");
    }
```

with:

```rust
    let spinner = ui::Spinner::start("Usta is thinking…");
    let reply = backend
        .complete_with_limit(system, &msgs, crate::anthropic::CLOSING_MAX_TOKENS)
        .await;
    spinner.stop().await;
    let reply = reply?;
    let files = progress::parse_closing_reply(&reply.text, reply.truncated)?;
```

and delete the now-unreachable block inside the write loop (:175-178):

```rust
        if content.is_empty() {
            ui::warn(&format!("empty content skipped: {name}"));
            continue;
        }
```

(`name` is still used by the `other =>` arm's warning? No — that arm binds `other`. After deleting the block, if `name` becomes unused, the loop header `for (name, content) in files` still uses `name` in `match name.as_str()`, so no orphan.)

`ask_usta` stays as is (still used by plain.rs and file_feedback.rs).

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test parse_closing_reply && cargo test split_files && cargo test flush_core_gates`
Expected: PASS.

- [ ] **Step 5: Gate** (files: `src/anthropic.rs src/progress.rs src/progress_tests.rs src/lifecycle.rs`)

Expected: 656 passed, 1 failed (allowed), 1 ignored. Production: anthropic.rs ≈ 283, progress.rs ≈ 537, lifecycle.rs ≈ 299. (progress_tests.rs is a test file — no budget.)

- [ ] **Step 6: Commit**

```bash
git add src/anthropic.rs src/progress.rs src/progress_tests.rs src/lifecycle.rs
git commit -m "$(cat <<'EOF'
fix(progress): validate the closing reply before any write

flush_core asks with a 32000-token budget and runs parse_closing_reply
first: a cut-off reply (max_tokens), one without ===FILE: sections or
with an empty section is an Err, so nothing is written, the transcript
stays for recovery and compaction keeps history. split_files drops the
no-delimiter fallback that turned any reply into progress.md.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
EOF
)"
```

---

### Task 3: Keep the last-known-good `.bak` (P1-3)

**Files:**
- Modify: `src/progress.rs` (imports :6-8, `write_atomic` :503-518 — line numbers shift by Task 2; find `pub fn write_atomic`)
- Modify: `src/progress_tests.rs` (new test after `write_atomic_backs_up_previous_version`)
- Modify: `SPEC.md:108` (the Backup bullet)

**Interfaces:**
- Produces: `write_atomic(path: &Path, content: &str) -> Result<()>` — signature unchanged; `.bak` is written at most once per path per process.

Deviation from spec: `BTreeSet` instead of `HashSet` — `BTreeSet::new` is `const`, so the static needs no lazy init. Same behavior.
Deviation from spec: the path is marked as backed up only when a copy actually happened (the file existed and the copy succeeded). A write that creates a new file has nothing to protect and must not use up the one backup; this also keeps the existing `write_atomic_backs_up_previous_version` test valid unchanged.

- [ ] **Step 1: Write the failing test** — `src/progress_tests.rs`, after `write_atomic_backs_up_previous_version`:

```rust
#[test]
fn write_atomic_keeps_the_pre_session_backup() {
    // Compaction checkpoints + the closing flush write the same file several
    // times per session: two bad writes must not destroy the last good copy.
    let base = std::env::temp_dir().join(format!("usta_progress_bak_once_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let target = base.join("rust.md");
    std::fs::write(&target, "önceki oturum").unwrap();
    write_atomic(&target, "A").unwrap();
    write_atomic(&target, "B").unwrap();
    write_atomic(&target, "C").unwrap();
    assert_eq!(
        std::fs::read_to_string(target.with_extension("md.bak")).unwrap(),
        "önceki oturum"
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "C");
    let _ = std::fs::remove_dir_all(&base);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test write_atomic_keeps_the_pre_session_backup`
Expected: FAIL — `left: "B"`, `right: "önceki oturum"`.

- [ ] **Step 3: Implement** — `src/progress.rs`

Imports (top of file):

```rust
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
```

Directly above `write_atomic`:

```rust
/// Paths already backed up by this process — `.bak` holds the file as the previous
/// session left it, never an earlier write of this session (review P1-3).
static BACKED_UP: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());
```

Inside `write_atomic`, replace:

```rust
    // Back up the previous version — a bad model output can be restored from the single copy.
    if path.exists() {
        let bak = path.with_extension("md.bak");
        let _ = std::fs::copy(path, &bak);
    }
```

with:

```rust
    // Back up once per process: later writes in this session (compaction checkpoints,
    // the closing flush) never overwrite it, so a bad write can always be undone.
    if path.exists() {
        let mut done = BACKED_UP.lock().unwrap_or_else(|e| e.into_inner());
        if !done.contains(path) && std::fs::copy(path, path.with_extension("md.bak")).is_ok() {
            done.insert(path.to_path_buf());
        }
    }
```

`SPEC.md:108` — replace the bullet text `- **Backup:** \`write_atomic\` copies the previous version to \`.bak\` — a bad model output can be rolled back.` with:

```markdown
- **Backup:** `write_atomic` copies a file to `.bak` before its first write in a process; later writes in the same session never overwrite it, so `.bak` holds what the previous session left and a bad model output can be rolled back (v0.38.0).
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test write_atomic`
Expected: PASS (new test plus `write_atomic_creates_parents_and_writes`, `write_atomic_backs_up_previous_version` unchanged).

- [ ] **Step 5: Gate** (files: `src/progress.rs src/progress_tests.rs`)

Expected: 657 passed, 1 failed (allowed), 1 ignored. Production: progress.rs ≈ 546.

- [ ] **Step 6: Commit**

```bash
git add src/progress.rs src/progress_tests.rs SPEC.md
git commit -m "$(cat <<'EOF'
fix(progress): back up each file once per session

write_atomic copies to .bak only before the first write of a path in
this process, so repeated checkpoint writes can no longer overwrite the
last good copy with a bad one.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
EOF
)"
```

---

### Task 4: Watcher ignore rules on the path below the project root (P1-4)

**Files:**
- Modify: `src/watcher.rs` (`spawn` :22-56, `should_forward` :108-119, `StructureTracker::seed`/`walk` :133-151, new `WatchRoot`; tests :213-505)

**Interfaces:**
- Produces:
  - `pub struct WatchRoot` with `pub fn new(root: &Path) -> Self` and `pub fn relative<'a>(&self, path: &'a Path) -> &'a Path`
  - `pub fn should_forward(root: &WatchRoot, path: &Path, kind: &EventKind) -> bool` (new first parameter; only caller is `spawn`)
  - `is_ignored(path: &Path)`, `StructureTracker::seed(root: &Path)` signatures unchanged (run.rs:105 and polite.rs tests untouched)

`is_ignored` stays pure (its tests keep passing). Callers strip the root first: canonical root, then raw root, else the path as given (macOS FSEvents reports `/private/var/...` realpaths).

- [ ] **Step 1: Write the failing tests** — `src/watcher.rs` `mod tests`, at the end:

```rust
    /// A project root under hidden / secret / target ancestors (review P1-4).
    /// Returns (scratch base to delete, root).
    fn awkward_root(tag: &str) -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("usta_watcher_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join(".h/secrets/target/proj");
        std::fs::create_dir_all(root.join("src")).unwrap();
        (base, root)
    }

    #[test]
    fn should_forward_judges_only_the_path_below_the_root() {
        let (base, root) = awkward_root("rel_forward");
        let wr = WatchRoot::new(&root);
        let file = root.join("src/main.rs");
        std::fs::write(&file, b"fn main() {}").unwrap();
        let modify = EventKind::Modify(ModifyKind::Any);
        assert!(should_forward(&wr, &file, &modify));
        // The realpath form FSEvents may report is matched via the canonical root.
        assert!(should_forward(&wr, &file.canonicalize().unwrap(), &modify));
        // Rules still apply below the root.
        assert!(!should_forward(&wr, &root.join(".git/HEAD"), &modify));
        assert!(!should_forward(&wr, &root.join("target/x"), &modify));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn structure_tracker_seeds_under_an_awkward_root() {
        let (base, root) = awkward_root("rel_seed");
        let mut t = StructureTracker::seed(&root);
        assert!(!t.note_new_dir(&root.join("src")), "src must be seeded");
        let _ = std::fs::remove_dir_all(&base);
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test structure_tracker_seeds_under_an_awkward_root`
Expected: compile error (`cannot find type WatchRoot`, `should_forward` takes 2 arguments). That is the failing state for the forward test; for the tracker test, temporarily comment out `should_forward_judges_only_the_path_below_the_root` and re-run to see `FAIL: src must be seeded`, then uncomment.

- [ ] **Step 3: Implement** — `src/watcher.rs`

Add above `should_forward`:

```rust
/// The watched root, raw and canonical. Ignore rules judge only the path BELOW
/// it: a project under `~/.work/`, `~/secrets-lab/` or `/x/target/` must not hide
/// itself (review P1-4). Both forms are kept because macOS FSEvents reports
/// realpaths (`/private/var/...` for `/var/...`).
pub struct WatchRoot {
    raw: PathBuf,
    canon: PathBuf,
}

impl WatchRoot {
    pub fn new(root: &Path) -> Self {
        WatchRoot {
            raw: root.to_path_buf(),
            canon: root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
        }
    }

    /// `path` relative to the root; a path outside it is returned as given.
    pub fn relative<'a>(&self, path: &'a Path) -> &'a Path {
        path.strip_prefix(&self.canon)
            .or_else(|_| path.strip_prefix(&self.raw))
            .unwrap_or(path)
    }
}
```

`should_forward` — new signature and first check (doc comment unchanged except append one sentence `Ignore rules see the path relative to \`root\`.`):

```rust
pub fn should_forward(root: &WatchRoot, path: &Path, kind: &EventKind) -> bool {
    if is_ignored(root.relative(path)) {
        return false;
    }
    // ... directory gate unchanged
```

`spawn` — before `thread::spawn(move || {` add `let watch_root = WatchRoot::new(root);` and change the call inside the loop to `if !should_forward(&watch_root, &path, &event.kind) {`.

`StructureTracker`:

```rust
    pub fn seed(root: &Path) -> Self {
        let mut tracker = StructureTracker {
            dirs: std::collections::BTreeSet::new(),
        };
        tracker.walk(&WatchRoot::new(root), root);
        tracker
    }

    fn walk(&mut self, root: &WatchRoot, dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && !is_ignored(root.relative(&path)) {
                self.dirs.insert(path.clone());
                self.walk(root, &path);
            }
        }
    }
```

Existing tests: 14 `should_forward(` calls in `mod tests` (lines ~350-446: `should_forward_live_directory_only_on_structure_kinds` ×2, `should_forward_vanished_directory_on_remove` ×1, `should_forward_directory_rename_destination` ×4, `should_forward_files_and_vanished_paths_on_every_kind` ×6, `should_forward_pins_ignore_check_before_directory_gate` ×1) gain a first argument `&tmp_root()`. Add the helper at the top of `mod tests`:

```rust
    /// Root for the pre-P1-4 tests: their paths all live under temp_dir().
    fn tmp_root() -> WatchRoot {
        WatchRoot::new(&std::env::temp_dir())
    }
```

Mechanical edit limited to the test module (the new tests already pass `&wr`, so run this BEFORE pasting Step 1's tests, or check they are not doubled):

```bash
perl -0pi -e 'my ($p,$t)=split(/(?=#\[cfg\(test\)\])/,$_,2); $t=~s/should_forward\((?!&wr)/should_forward(&tmp_root(), /g; $_=$p.$t' src/watcher.rs
```

(The pattern `should_forward\(` does not match test names like `should_forward_live_...(`.) Relative-path asserts such as `should_forward(&tmp_root(), Path::new("target/debug/x.rs"), ..)` still hold: the strip fails, the path is judged as given.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test watcher`
Expected: PASS (all old watcher tests + the two new ones).

- [ ] **Step 5: Gate** (files: `src/watcher.rs`)

Expected: 659 passed, 1 failed (allowed), 1 ignored. Production: watcher.rs ≈ 240.

- [ ] **Step 6: Commit**

```bash
git add src/watcher.rs
git commit -m "$(cat <<'EOF'
fix(watcher): apply ignore rules below the project root only

A project under ~/.work, secrets-lab or a target/ ancestor got no file
feedback and an empty structure. should_forward and the structure seed
now strip the (canonical or raw) root before is_ignored.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
EOF
)"
```

---

### Task 5: Never push an empty assistant reply (P1-5)

**Files:**
- Modify: `src/backend.rs` (`complete_with_limit` from Task 1; new `ensure_nonempty`; tests)

**Interfaces:**
- Consumes (Task 1): `Reply { text, web, context_tokens, truncated }`, `Backend::complete_with_limit`
- Produces: `pub(crate) fn ensure_nonempty(reply: Reply) -> Result<Reply>`; `Backend::complete_with_limit` (and so `complete`) returns `Err("empty reply from the model — nothing was added to the conversation")` for a blank reply. Private `async fn complete_raw(&mut self, system: &str, history: &[Message], max_tokens: u32) -> Result<Reply>` holds the old match.

One guard covers all eight `push_assistant` sites (tui/run.rs:186, :348; plain.rs:208, :241, :342; file_feedback.rs:228, :374; intro stitch via tui/ask.rs:36) and both backends, plus the closing flush (Err → transcript kept). No call site changes.

- [ ] **Step 1: Write the failing tests** — `src/backend.rs` `mod tests`, after `cli_call_isolates_usta_from_claude_code_settings`:

```rust
    fn reply(text: &str) -> Reply {
        Reply {
            text: text.into(),
            web: false,
            context_tokens: None,
            truncated: false,
        }
    }

    #[test]
    fn ensure_nonempty_rejects_blank_replies() {
        assert!(ensure_nonempty(reply("")).is_err());
        let err = ensure_nonempty(reply("  \n")).err().expect("blank is an error");
        assert!(err.to_string().contains("empty reply"), "{err}");
        assert_eq!(ensure_nonempty(reply("hi")).unwrap().text, "hi");
    }

    /// Source pin: every completion (both backends, chat and closing) passes the guard.
    #[test]
    fn complete_routes_through_the_empty_reply_guard() {
        let production = include_str!("backend.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(production
            .contains("ensure_nonempty(self.complete_raw(system, history, max_tokens).await?)"));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test ensure_nonempty`
Expected: compile error — `cannot find function ensure_nonempty`.

- [ ] **Step 3: Implement** — `src/backend.rs`

Rename Task 1's `pub async fn complete_with_limit(` to `async fn complete_raw(` (body unchanged, drop its doc comment) and insert above it:

```rust
    /// `complete` with an explicit output budget — the closing/compaction flush
    /// needs more room than a chat turn. The CLI backend ignores it. A blank reply
    /// is an error (see `ensure_nonempty`).
    pub async fn complete_with_limit(
        &mut self,
        system: &str,
        history: &[Message],
        max_tokens: u32,
    ) -> Result<Reply> {
        ensure_nonempty(self.complete_raw(system, history, max_tokens).await?)
    }
```

After `last_user_text` (free fn, before `parse_cli_output`):

```rust
/// A blank reply (thinking-only, cut off before any text, empty CLI `result`) must
/// never reach history: an empty assistant turn makes every later API call fail.
pub(crate) fn ensure_nonempty(reply: Reply) -> Result<Reply> {
    if reply.text.trim().is_empty() {
        bail!("empty reply from the model — nothing was added to the conversation");
    }
    Ok(reply)
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test ensure_nonempty && cargo test complete_routes_through`
Expected: PASS.

- [ ] **Step 5: Gate** (files: `src/backend.rs`)

Expected: 661 passed, 1 failed (allowed), 1 ignored. Production: backend.rs ≈ 527.

- [ ] **Step 6: Commit**

```bash
git add src/backend.rs
git commit -m "$(cat <<'EOF'
fix(backend): treat an empty model reply as an error

A blank reply was pushed as an empty assistant turn, after which every
API call failed. One guard in Backend::complete_with_limit covers all
push sites, both backends and the closing flush.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
EOF
)"
```

---

### Task 6: Corrupt images and failed turns never poison history (P1-6)

**Files:**
- Modify: `src/image_attach.rs` (`prepare` :49-68; test)
- Modify: `src/session.rs` (new `truncate_history`; test)
- Modify: `src/transcript.rs` (`read_history` :69-98, `Recorder::rollback`; test)
- Modify: `src/lifecycle.rs` (new `rollback_turn`; two tests)
- Modify: `src/tui/run.rs` (opening turn :166-208, main submit :333-353)
- Modify: `src/plain.rs` (import :21; sites :196-214, :229-245, :334-347)
- Modify: `src/file_feedback.rs` (import :12; `handle_file_change` :220-222, `handle_batch_change` :366-368)

**Interfaces:**
- Produces:
  - `Session::truncate_history(&mut self, len: usize)` — no-op when `len >= history.len()`
  - `Recorder::rollback(&self)` — appends `{"role":"rollback","text":""}`
  - `transcript::read_history` — a `rollback` line drops the most recent message if it is a user message
  - `pub(crate) fn rollback_turn(session: &mut Session, recorder: &Recorder, mark: usize)` in `lifecycle.rs`
  - `image_attach::prepare` — Err `image could not be read: …` for any undecodable image (also on the pass-through path)

Notes:
- The deliberate `Cancelled` arms in run.rs (spec B2) are unchanged.
- `tui/intro.rs` is not changed: its turns live in a local `Vec<IntroTurn>`, not in `Session`, are text-only (no images), and Task 5 already keeps empty replies out. A failed intro turn leaves two consecutive user turns, which the API accepts.
- `push_user_with_images` may prune older history images before the call; a rollback does not restore them (they were over budget anyway).
- `rollback_turn` writes the transcript marker only when it actually removed something, so a replay never pops a legitimate user line.

- [ ] **Step 1: Write the failing tests**

`src/image_attach.rs` `mod tests`, after `prepare_passes_a_small_png_through_untouched`:

```rust
    #[test]
    fn prepare_rejects_a_truncated_png_with_a_valid_header() {
        let mut bytes = png_bytes(64, 32);
        bytes.truncate(bytes.len() - 20); // IEND + the tail of IDAT gone
        // Precondition: the header-only size read still succeeds (that was the hole).
        // If it ever fails here, cut fewer bytes (e.g. 14).
        let size = ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .unwrap()
            .into_dimensions();
        assert!(size.is_ok());
        let err = prepare(bytes).unwrap_err();
        assert!(err.to_string().starts_with("image could not be read"), "{err}");
    }
```

`src/session.rs` `mod tests`, at the end:

```rust
    #[test]
    fn truncate_history_rolls_back_a_turn_and_ignores_a_longer_mark() {
        let mut s = Session::new("rust", "sistem");
        s.push_user("a");
        s.push_assistant("b".into());
        let mark = s.history().len();
        s.push_user_with_images("c [Image #1]", &[]);
        s.truncate_history(mark);
        assert_eq!(s.history().len(), 2);
        assert_eq!(s.history()[1].role, "assistant");
        s.truncate_history(10);
        assert_eq!(s.history().len(), 2);
    }
```

`src/transcript.rs` `mod tests`, after `read_history_roundtrips_recorder_output`:

```rust
    #[test]
    fn read_history_drops_the_user_turn_a_rollback_undid() {
        let base =
            std::env::temp_dir().join(format!("usta_transcript_rollback_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let p = base.join("rust-1.jsonl");
        let r = Recorder::new(p.clone());
        r.user("merhaba");
        r.assistant("selam");
        r.user("bozuk resim");
        // Raw marker line, so this fails on the old reader (unknown role) rather
        // than on a missing method.
        std::fs::OpenOptions::new()
            .append(true)
            .open(&p)
            .unwrap()
            .write_all(line("rollback", "").as_bytes())
            .unwrap();
        let h = read_history(&p).unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(h[1].role, "assistant");
        let _ = std::fs::remove_dir_all(&base);
    }
```

`src/lifecycle.rs` `mod tests`, at the end:

```rust
    #[test]
    fn rollback_turn_restores_history_and_marks_the_transcript() {
        let dir = std::env::temp_dir().join(format!("usta_rollback_turn_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let rec = Recorder::new(dir.join("rust-20260101-000000.jsonl"));
        let mut s = Session::new("rust", "sys");
        s.push_user("hi");
        rec.user("hi");
        s.push_assistant("hello".into());
        rec.assistant("hello");
        let mark = s.history().len();
        s.push_user_with_images("broken [Image #1]", &[]);
        rec.user("broken [Image #1]");
        rollback_turn(&mut s, &rec, mark);
        assert_eq!(s.history().len(), 2);
        let replay = transcript::read_history(rec.path()).unwrap();
        assert_eq!(replay.len(), 2);
        assert_eq!(replay[1].role, "assistant");
        // Nothing to undo → no marker (it would pop a legitimate user line on replay).
        s.push_user("next");
        rec.user("next");
        let n = s.history().len();
        rollback_turn(&mut s, &rec, n);
        assert_eq!(transcript::read_history(rec.path()).unwrap().len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Source pin: every push→ask site rolls the turn back on error (the TUI/plain
    /// loops need a live terminal + backend, no mock exists).
    #[test]
    fn every_push_ask_site_rolls_back_on_error() {
        for (name, src, n) in [
            ("tui/run.rs", include_str!("tui/run.rs"), 2),
            ("plain.rs", include_str!("plain.rs"), 3),
            ("file_feedback.rs", include_str!("file_feedback.rs"), 2),
        ] {
            let prod = src.split("#[cfg(test)]").next().unwrap();
            assert_eq!(prod.matches("rollback_turn(").count(), n, "{name}");
        }
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test prepare_rejects_a_truncated_png && cargo test read_history_drops`
Expected: `prepare_rejects_a_truncated_png_with_a_valid_header` FAILS (`called Result::unwrap_err() on an Ok value`); `read_history_drops_the_user_turn_a_rollback_undid` FAILS (`unknown role in transcript line: rollback`). The session/lifecycle tests fail to compile (`no method named truncate_history`, `cannot find function rollback_turn`) — comment them out for this run if you want the two runtime failures shown, then restore.

- [ ] **Step 3: Implement the helpers**

`src/image_attach.rs` `prepare` — replace from `let (w, h) = reader.into_dimensions()...` to the end of the fn with:

```rust
    let (w, h) = reader.into_dimensions().context("cannot read image size")?;
    // Full decode even on the pass-through path: a truncated/corrupt body behind a
    // valid header would otherwise reach the API and fail every later turn.
    let img = image::load_from_memory_with_format(&bytes, format)
        .map_err(|e| anyhow::anyhow!("image could not be read: {e}"))?;
    if w.max(h) <= MAX_EDGE && bytes.len() <= MAX_RAW_BYTES {
        return Ok(PastedImage {
            media_type: mt,
            data: bytes,
            width: w,
            height: h,
        });
    }
    shrink(img, format)
```

Also update the fn doc's second sentence to: `A valid image within both caps is decoded (to reject a corrupt body) and passed through byte-for-byte; anything larger is downscaled and re-encoded: JPEG stays JPEG, others PNG (JPEG if the PNG is still too big).` The drop path already surfaces the Err as `couldn't attach image: …` (`failed_drop_inserts_the_path_and_leaves_a_notice`).

`src/session.rs`, after `push_assistant`:

```rust
    /// Roll history back to `len` messages — a failed turn leaves history exactly
    /// as before it (review P1-6). No-op when `len >= history.len()`.
    pub fn truncate_history(&mut self, len: usize) {
        self.history.truncate(len);
    }
```

`src/transcript.rs`:
- `read_history` doc: replace the last sentence with `Any line that fails to parse, or carries a role other than "user"/"assistant"/"rollback", fails the whole read — a salvage flush must not silently drop turns. A "rollback" line drops the user turn the live session rolled back.`
- In the `match role` add, before `other => bail!(...)`:

```rust
            "rollback" => {
                if out.last().is_some_and(|m| m.role == "user") {
                    out.pop();
                }
            }
```

- In `impl Recorder`, after `assistant`:

```rust
    /// Mark the last user turn as rolled back (the call failed) — `read_history`
    /// drops it, so recovery replays the history the live session kept.
    pub fn rollback(&self) {
        self.append("rollback", "");
    }
```

`src/lifecycle.rs`, after `ask_usta`:

```rust
/// Undo a failed turn: history back to `mark` (taken before the user turn was
/// pushed) plus a transcript rollback marker, so a corrupt image, an API error or
/// an empty reply never stays in the conversation (review P1-6).
pub(crate) fn rollback_turn(session: &mut Session, recorder: &Recorder, mark: usize) {
    if session.history().len() > mark {
        session.truncate_history(mark);
        recorder.rollback();
    }
}
```

- [ ] **Step 4: Wire every push→ask site** (each: take `mark` immediately before the push, roll back first thing in the Err arm)

`src/tui/run.rs` — opening turn (:166-208):

```rust
    if let Some(opening) = opening {
        let mark = session.history().len();
        session.push_user(&opening);
        recorder.user(&opening);
```

and its Err arm (:205-207):

```rust
            Err(e) => {
                crate::lifecycle::rollback_turn(&mut session, &recorder, mark);
                crate::tui::page::page_error(&mut tui, &format!("opening turn skipped: {e}"))?
            }
```

`src/tui/run.rs` — main submit (inside the `select!` macro; rustfmt does not touch it, keep the surrounding one-line style): before `let dropped = session.push_user_with_images(&outgoing, &images);` (:333) insert

```rust
                        let mark = session.history().len();
```

and replace the Err arm (:353) `Err(e) => crate::tui::page::page_error(&mut tui, &format!("error: {e}"))?,` with:

```rust
                            Err(e) => {
                                crate::lifecycle::rollback_turn(&mut session, &recorder, mark);
                                crate::tui::page::page_error(&mut tui, &format!("error: {e}"))?;
                            }
```

`src/plain.rs` — import (:21): `use crate::lifecycle::{ask_usta, maybe_compact, rollback_turn, sleep_until_deadline, today};`

Opening drill (:196): before `session.push_user(&opening);` insert `let mark = session.history().len();`; Err arm (:213):

```rust
            Err(e) => {
                rollback_turn(session, ctx.recorder, mark);
                ui::warn(&format!("opening drill skipped: {e}"))
            }
```

(keep the comment line `// Drill failed → don't block the session, fall silently back into normal flow.` above the arm).

Onboarding (:229): before `session.push_user(&onboarding);` insert `let mark = session.history().len();`; Err arm (:244):

```rust
            Err(e) => {
                rollback_turn(session, ctx.recorder, mark);
                ui::warn(&format!("introduction turn skipped: {e}"))
            }
```

Main loop (:334, inside `select!`): before `session.push_user(&line);` insert `let mark = session.history().len();`; Err arm (:346) `Err(e) => ui::warn(&format!("error: {e}")),` becomes:

```rust
                            Err(e) => {
                                rollback_turn(session, ctx.recorder, mark);
                                ui::warn(&format!("error: {e}"));
                            }
```

`src/file_feedback.rs` — import (:12): `use crate::lifecycle::{ask_usta, rollback_turn};`

`handle_file_change` (:220-222) and `handle_batch_change` (:366-368): replace

```rust
    session.push_user(&injected);
    recorder.user(&injected);
    let reply = ask_usta(backend, &session.system, session.history()).await?;
```

with (both places, identical):

```rust
    let mark = session.history().len();
    session.push_user(&injected);
    recorder.user(&injected);
    let reply = match ask_usta(backend, &session.system, session.history()).await {
        Ok(r) => r,
        Err(e) => {
            rollback_turn(session, recorder, mark);
            return Err(e);
        }
    };
```

(The pin `handle_batch_change_selects_feedback_frame_on_live_path` only checks `feedback_frame(` / `flow_frame(` in that body — unaffected.)

- [ ] **Step 5: Run to verify they pass**

Run: `cargo test prepare_ && cargo test truncate_history && cargo test read_history && cargo test rollback_turn && cargo test every_push_ask_site && cargo test failed_drop_inserts`
Expected: PASS.

- [ ] **Step 6: Gate** (files: `src/image_attach.rs src/session.rs src/transcript.rs src/lifecycle.rs src/tui/run.rs src/plain.rs src/file_feedback.rs`)

Expected: 666 passed, 1 failed (allowed), 1 ignored. Production: image_attach.rs ≈ 137, session.rs ≈ 79, transcript.rs ≈ 174, lifecycle.rs ≈ 310, tui/run.rs ≈ 395 (no test mod), plain.rs ≈ 505 (no test mod), file_feedback.rs ≈ 538. Closing/polite/page/editor/intro source pins on run.rs must stay green (they are part of `cargo test`).

- [ ] **Step 7: Commit**

```bash
git add src/image_attach.rs src/session.rs src/transcript.rs src/lifecycle.rs src/tui/run.rs src/plain.rs src/file_feedback.rs
git commit -m "$(cat <<'EOF'
fix(session): reject corrupt images and roll back failed turns

prepare fully decodes images on the pass-through path, so a truncated
file is refused at the door. Every push->ask site rolls the user turn
back on error (Session::truncate_history + a transcript rollback line
that read_history honors), so a failed turn can no longer break every
later call or come back on recovery.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
EOF
)"
```

---

### Task 7: Release v0.38.0

**Files:**
- Modify: `Cargo.toml` (`version = "0.37.1"` → `"0.38.0"`)
- Modify: `Cargo.lock` (the `name = "usta"` package entry, version → `0.38.0`)
- Modify: `src/tui/welcome_tests.rs:8` (version pin)
- Modify: `SPEC.md` (new paragraph after the `**v0.37.1 (quit save inside the TUI):**` paragraph, ~line 258)
- Create (git-ignored, not committed): `.superpowers/sdd/review-fixes-manual-checklist.md`

Deviation from spec: README is not touched — it carries no version string, and P1 changes no user-facing behavior that README describes (cf42c8f edited README only for a feature description).

- [ ] **Step 1: Write the failing test** — `src/tui/welcome_tests.rs:8`:

```rust
    assert_eq!(env!("CARGO_PKG_VERSION"), "0.38.0");
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test version_aligned_with_spec`
Expected: FAIL — `left: "0.37.1"`, `right: "0.38.0"`.

- [ ] **Step 3: Bump**

`Cargo.toml`: `version = "0.38.0"`. Then `cargo build` (updates the `usta` entry in `Cargo.lock` to `0.38.0`; confirm with `git diff Cargo.lock` that only that one line changed).

`SPEC.md` — insert after the v0.37.1 paragraph (with one blank line before and after):

```markdown
**v0.38.0 (review fixes P1 — no silent data loss):** the closing/compaction flush asks for a 32000-token output budget (chat turns stay at 8000) and validates the reply before writing anything (`progress::parse_closing_reply`): a reply cut off at the budget (API `stop_reason: max_tokens`), one without `===FILE:` sections (the old single-file fallback is gone) or one with an empty section is rejected — nothing is written, the transcript stays unfinished for recovery and compaction is postponed. `write_atomic` backs a file up to `.bak` once per process, so `.bak` holds what the previous session left. The watcher applies its ignore rules to the path below the project root (a project under `~/.work/`, `secrets-lab/` or a `target/` ancestor is watched again; macOS realpaths match via the canonical root). An empty model reply is an error, never an empty assistant turn. Dropped images are fully decoded before they are attached (a truncated file is refused with a notice), and a failed turn is rolled back: history returns to its state before the turn and the transcript gets a `rollback` line that recovery honors. Design: `docs/superpowers/specs/2026-10-26-review-fixes-design.md` (P1).
```

`.superpowers/sdd/review-fixes-manual-checklist.md` (git-ignored; create, do not `git add`):

```markdown
# Review fixes — manual checks

## P1 (v0.38.0)
- [ ] Real API truncation: `USTA_BACKEND=api`, temporarily build with `CLOSING_MAX_TOKENS = 200`, end a session → warning names "cut off (max tokens)", progress files unchanged, transcript left unfinished, recovery offered on next start.
- [ ] macOS FSEvents realpath: project under `/tmp/.h/secrets/proj` (realpath `/private/tmp/...`) → saving `src/main.rs` triggers feedback; a new `src/x/` dir gets a structure note.
- [ ] Corrupt image drop: drag a truncated PNG into the TUI → `couldn't attach image: image could not be read: …`, the next turn works.
- [ ] Failed turn: disconnect network, send a message → error notice; reconnect, send again → works (no repeated failure).
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test version_aligned_with_spec`
Expected: PASS.

- [ ] **Step 5: Gate** (files: `src/tui/welcome_tests.rs`)

Expected: 666 passed, 1 failed (allowed), 1 ignored. (welcome_tests.rs is a test file — no budget.)

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/tui/welcome_tests.rs SPEC.md
git commit -m "$(cat <<'EOF'
chore: v0.38.0 (review fixes P1)

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
EOF
)"
```

---

## Notes for the executor

- `CLOSING_MAX_TOKENS = 32000` on the non-streaming reqwest call: the Anthropic SDKs refuse large non-streaming `max_tokens` client-side, but usta uses raw HTTP and reqwest has no default timeout, so the request is accepted. A 32000-token closing reply can take several minutes; that is the spec's accepted trade-off (the flush already runs under a "saving" status). If live testing shows dropped connections, report it rather than lowering the budget silently.
- Test counts assume the tests exactly as written here; if you add or split one, adjust the expected count, but the only failure must remain `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt`.
