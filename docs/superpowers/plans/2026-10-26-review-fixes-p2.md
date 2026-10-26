# Review Fixes P2 (clear functional bugs) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the twelve P2 review findings (P2-1 … P2-12): Windows key double-handling, `echo hi | usta` hanging in the TUI, incomplete `usta reset <topic>`, feedback on usta's own writes, torn transcript tails, swallowed CLI errors, blind resume retries, a second usta "recovering" a live session, lost flush/lock on error or signal, lossy slugs, unsafe PDF conversion, and an unkilled `cargo check`. Ship as v0.39.0.

**Architecture:** Small, independent fixes at the point of each bug. Two new single-purpose modules: `src/lock.rs` (pid liveness, `live_owner`, `split_live`, and the `LockGuard` that removes the lock on drop) and `src/shutdown.rs` (one signal listener per process behind a `watch` flag; `requested()` is a `select!` arm in the TUI loop, the live ask and the plain loop). `src/reset.rs` takes the topic reset out of the over-budget `setup.rs` and plans/applies the removal of every per-topic artifact. PDF conversion writes into the hidden `materials/.converted/` with a polled, killable child.

**Tech Stack:** Rust 2021 (rust-version 1.88), tokio 1.53 (adds the `signal` feature), crossterm 0.29, notify, serde_json, anyhow.

Spec: `docs/superpowers/specs/2026-10-26-review-fixes-design.md`, sections "Global constraints", "P2", "Release".
Verification notes (local, git-ignored; line numbers there are from v0.37.1 — this plan's line numbers are re-located against `main` @ 5276da5): `.superpowers/sdd/review-fixes/verify-A.md` (P2-5), `verify-B.md` (P2-4), `verify-C.md` (P2-1,2,3,6,7,8,9), `verify-D.md` (P2-10,11,12).

Work happens in the worktree `../usta-fix-p2` on branch `fix-p2` (create it from `main` @ 5276da5). Every path and command below is relative to that worktree's root.

## Global Constraints

Copied from the spec:
- 600-line production budget per file (lines before the first `#[cfg(test)]`, SPEC §11). `setup.rs` is already 611 → it must shrink when touched (P2-3 moves reset code out).
- `cargo fmt` scoped to edited files; `cargo fmt --check` crate-wide clean; `cargo clippy --all-targets -- -D warnings` clean.
- `cargo test`: the only allowed failure is `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt` (pdftotext installed on the dev machine). P2-11 makes the tool injectable, after which that test must pass too.
- No emoji in any new UI text. Notices are short, lowercase, English (matching existing notices).
- Surgical: no refactors beyond what an item needs. Source-pin tests (closing.rs:98-165, polite.rs:696) must stay green; update a pin only when the pinned line itself is the one being changed.
- No new crates except enabling tokio's `signal` feature (P2-9).

Plan-level additions:
- **Every cargo command is prefixed `CARGO_TARGET_DIR=$PWD/target`** (`~/.cargo/config.toml` sets a shared target dir; run from the worktree root).
- The crate is bin-only: use `cargo test <filter>`, never `--lib`.
- Other source pins that must stay green: `tui/closing.rs` tests (`save_live_*`, `run_saves_after_the_loop_before_returning` — needs the literal `Ok(Some((session, recorder, lock, flushed)))` in `tui/run.rs` and the main loop at 4-space indent `\n    loop {\n`; `main_flushes_only_on_the_plain_path` — splits `main.rs` on the TUI-branch condition, updated once in Task 2), `tui/polite.rs` `dispatch_flush_route_arms_are_pinned`, `tui/page.rs` `mouse_and_page_keys_are_handled_in_every_event_loop` (handle_nav call counts: run.rs 1, ask.rs 2, entry.rs 1, intro.rs 1), `lifecycle.rs` `flush_core_gates_the_reply_before_any_write` and `every_push_ask_site_rolls_back_on_error` (rollback_turn counts: tui/run.rs 2, plain.rs 3, file_feedback.rs 2), `backend.rs` ensure_nonempty pin (`ensure_nonempty(self.complete_raw(system, history, max_tokens).await?)`), `materials.rs` `convert_pdfs_captures_pdftotext_output`.
- `flush_progress`'s signature does not change.
- Commits: `git add` files by name, never `git commit -a`. Every commit message ends with exactly these two lines:
  ```
  Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
  ```

### The gate (run at the end of every task)

```bash
CARGO_TARGET_DIR=$PWD/target cargo test 2>&1 | grep -E "^test result|FAILED|panicked" | head -20
CARGO_TARGET_DIR=$PWD/target cargo clippy --all-targets -- -D warnings
rustfmt --edition 2021 <every .rs file this task edited or created>
CARGO_TARGET_DIR=$PWD/target cargo fmt --check
for f in <every production .rs file this task edited or created>; do n=$(grep -n '^#\[cfg(test)\]' "$f" | head -1 | cut -d: -f1); if [ -n "$n" ]; then echo "$f $((n-1))"; else echo "$f $(wc -l < "$f") (no test mod)"; fi; done
git diff -U0 -- src | perl -CSD -ne 'print if /^\+/ && /[\x{1F000}-\x{1FFFF}\x{2600}-\x{27BF}]/'
```

The last command must print nothing (no emoji in added lines). Every production count must be ≤ 600; the expected counts given per task are approximate (rustfmt decides line breaks). Run `rustfmt` BEFORE the line count.

Baseline at `main` @ 5276da5: **665 passed, 1 failed** (the allowed `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt`), **1 ignored**. Each task states the expected count after it; until Task 12 the one failure is the allowed one, from Task 12 on there are 0 failures.

Baseline production line counts: setup.rs 611 (tests in setup_tests.rs), welcome.rs 618 (not touched), progress.rs 546, file_feedback.rs 538, backend.rs 534, editor.rs 531, plain.rs 498 (no test mod), polite.rs 461, tui/run.rs 396 (no test mod), lifecycle.rs 321, main.rs 282 (no test mod), ui.rs 243, watcher.rs 238, page.rs 213, materials.rs 205, topic.rs 191, transcript.rs 173, check.rs 152, tui/ask.rs 138, cli.rs 54.

Task order: 1 → 13 as written. Hard constraints: Task 6 (P2-6) before Task 7 (P2-7); Task 8 (P2-8, creates `lock.rs`) before Task 9 (P2-9a, adds `LockGuard`); Task 9 before Task 10 (signals).

---

### Task 1: Ignore key-release events (P2-1)

**Files:**
- Modify: `src/tui/page.rs` (import :6, `nav` Key arm :109-122, tests :399-415 neighborhood)
- Modify: `src/tui/editor.rs` (import :6, `handle_key` :94, tests after :560)

**Interfaces:**
- Produces: `page::nav` consumes every `Event::Key` whose `kind == KeyEventKind::Release` (returns `true`, nothing else happens); `InputBox::handle_key` returns `Action::None` for a release.

Every loop that reads keys (run.rs:236, ask.rs:56 and :116, entry.rs:286, intro.rs:101) already calls `page::handle_nav` before its own key match, so the single guard in `nav` covers them; the editor guard is defence in depth. `KeyEventKind::Repeat` stays (held keys type). Unix never sees releases (term.rs pushes only `DISAMBIGUATE_ESCAPE_CODES`), so this changes nothing there.

- [ ] **Step 1: Write the failing tests**

In `src/tui/page.rs` `mod tests`, add `KeyEventKind` to the crossterm import (:218-220):

```rust
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    };
```

and add after `nav_page_keys_scroll_a_page_and_other_keys_pass_through`:

```rust
    #[test]
    fn nav_consumes_key_release_and_changes_nothing() {
        // Windows reports Press + Release for every key; the release must not
        // scroll a second time nor reach the caller's key match.
        let mut c = filled_chat(40);
        let release = |code| {
            Event::Key(KeyEvent::new_with_kind(
                code,
                KeyModifiers::NONE,
                KeyEventKind::Release,
            ))
        };
        assert!(nav(&mut c, &release(KeyCode::PageUp), &mut |_| {}));
        assert!(c.is_following(), "a released PageUp must not scroll");
        assert!(nav(&mut c, &release(KeyCode::Char('a')), &mut |_| {}));
    }
```

In `src/tui/editor.rs` `mod tests`, add after `typing_and_submit_returns_trimmed_line_and_clears`:

```rust
    #[test]
    fn release_events_are_ignored() {
        let rel = |code| {
            KeyEvent::new_with_kind(code, KeyModifiers::NONE, crossterm::event::KeyEventKind::Release)
        };
        let mut b = InputBox::new();
        assert!(matches!(b.handle_key(rel(KeyCode::Char('x'))), Action::None));
        assert_eq!(b.value(), "");
        type_str(&mut b, "hi");
        assert!(matches!(b.handle_key(rel(KeyCode::Enter)), Action::None));
        assert_eq!(b.value(), "hi", "a released Enter must not submit");
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test release`
Expected: FAIL — `nav_consumes_key_release_and_changes_nothing` panics on "a released PageUp must not scroll"; `release_events_are_ignored` fails with `left: "x"`.

- [ ] **Step 3: Implement**

`src/tui/page.rs:6`:

```rust
use crossterm::event::{Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
```

In `fn nav` (:88), insert this arm directly before `Event::Key(k) => {` (:109):

```rust
        // Windows reports every key twice (Press + Release). The release is
        // consumed here — every event loop routes through handle_nav first, so
        // no loop handles a key twice (review P2-1). Repeat stays: held keys type.
        Event::Key(k) if k.kind == KeyEventKind::Release => true,
```

`src/tui/editor.rs:6`:

```rust
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
```

First lines of `handle_key` (:94):

```rust
    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        // Defence in depth for P2-1: a key release is never an edit.
        if key.kind == KeyEventKind::Release {
            return Action::None;
        }
        let m = key.modifiers;
```

- [ ] **Step 4: Run to verify they pass**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test release`
Expected: PASS (both).

- [ ] **Step 5: Gate** (files: `src/tui/page.rs`, `src/tui/editor.rs`)

Expected: 667 passed, 1 failed (allowed), 1 ignored. page.rs ≈ 217, editor.rs ≈ 535.

- [ ] **Step 6: Commit**

```bash
git add src/tui/page.rs src/tui/editor.rs
git commit -m "$(cat <<'EOF'
fix(tui): ignore key-release events (Windows double keys)

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
EOF
)"
```

---

### Task 2: TUI only when stdin is a terminal too (P2-2)

**Files:**
- Modify: `src/ui.rs` (after `is_plain` :73-76, tests :245-261)
- Modify: `src/main.rs:188-190` (comment + TUI gate)
- Modify: `src/tui/closing.rs:161` (source pin that splits main.rs on the TUI-branch condition — the pinned line itself changes)

**Interfaces:**
- Produces: `pub fn ui::use_tui() -> bool`; private `fn use_tui_from(stdin_tty: bool, plain: bool) -> bool`.
- `ui::is_plain()` keeps its meaning (stdout-only output-format gate, used by ui.rs:80-179).

- [ ] **Step 1: Write the failing test** — in `src/ui.rs` `mod tests`:

```rust
    #[test]
    fn use_tui_needs_a_terminal_stdin_and_not_plain() {
        assert!(use_tui_from(true, false));
        assert!(!use_tui_from(false, false), "`echo hi | usta` takes the plain path");
        assert!(!use_tui_from(true, true));
        assert!(!use_tui_from(false, true));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test use_tui_needs`
Expected: compile error — `cannot find function use_tui_from`.

- [ ] **Step 3: Implement**

`src/ui.rs`, after `is_plain` (:76):

```rust
/// Open the fullscreen TUI? Both ends must be a terminal: crossterm reads keys
/// from the tty, so with a piped stdin (`echo hi | usta`) the pipe would never
/// be read and the TUI would hang. The plain path reads the pipe.
pub fn use_tui() -> bool {
    use_tui_from(std::io::stdin().is_terminal(), is_plain())
}

fn use_tui_from(stdin_tty: bool, plain: bool) -> bool {
    stdin_tty && !plain
}
```

`src/main.rs:188-190`:

```rust
    // Plain path (stdin or stdout not a TTY / NO_COLOR): resolve_topic + lock-conflict + build_session
    // + banner + run_plain_loop happen here — behavior preserved exactly.
    let (session, recorder, lock, flushed) = if ui::use_tui() {
```

`src/tui/closing.rs:161` (inside `main_flushes_only_on_the_plain_path`):

```rust
            .split("if ui::use_tui() {")
```

- [ ] **Step 4: Run to verify it passes**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test use_tui_needs` then `CARGO_TARGET_DIR=$PWD/target cargo test main_flushes_only_on_the_plain_path`
Expected: PASS (both).

- [ ] **Step 5: Gate** (files: `src/ui.rs`, `src/main.rs`, `src/tui/closing.rs`)

Expected: 668 passed, 1 failed (allowed), 1 ignored. ui.rs ≈ 254, main.rs 282, closing.rs 74.

- [ ] **Step 6: Commit**

```bash
git add src/ui.rs src/main.rs src/tui/closing.rs
git commit -m "$(cat <<'EOF'
fix: piped stdin takes the plain path instead of a hung TUI

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
EOF
)"
```

---

### Task 3: `usta reset <topic>` removes every topic artifact (P2-3)

**Files:**
- Create: `src/reset.rs`
- Modify: `src/setup.rs` (module doc :1-5; delete `run_reset_topic` + `remove_topic_visuals` with their doc comments :203-251; remove imports this leaves unused)
- Modify: `src/setup_tests.rs` (delete the comment block + `remove_topic_visuals_deletes_a_populated_dir` + `remove_topic_visuals_missing_dir_is_not_an_error`, :476-519)
- Modify: `src/main.rs` (`mod reset;` between `mod progress;` :25 and `mod session;` :26; the `use crate::setup::{…}` list :44-47 drops `run_reset_topic`; dispatch :60)

**Interfaces:**
- Consumes: `progress::{progress_path, approach_path, curriculum_path, write_atomic}`, `transcript::topic_from_record(&Path) -> Option<String>`, `index::{entries, remove}`, `setup::{confirm, run_migration}` (both already `pub(crate)`), `config::{find_project_root, global_root}`.
- Produces: `pub(crate) fn reset::run_reset_topic(topic: &str) -> anyhow::Result<()>` (same signature as the old `setup::run_reset_topic`). Private: `topic_artifacts(root: &Path, topic: &str) -> Vec<PathBuf>`, `record_topic(path: &Path) -> Option<String>`, `remove_artifacts(paths: &[PathBuf]) -> Result<()>`, `struct ResetPlan { artifacts: Vec<PathBuf>, catalog: Option<String> }`, `plan_reset(root, index_path, topic) -> ResetPlan`, `apply_reset(&ResetPlan, index_path) -> Result<()>`.

Deviation from spec: `remove_topic_visuals` is not moved as a function — the visuals dir is one entry of `topic_artifacts` and is removed by the generic, idempotent `remove_artifacts`; its two tests are replaced by `topic_artifacts_lists_exactly_this_topics_files` (populated dir removed, sibling topic untouched, second removal is a no-op). Also deviation (small extension): the `.md.bak` siblings that `write_atomic` leaves next to progress/approach/curriculum are listed too — otherwise reset still leaves per-topic files behind.

Not touched by reset (spec): the lock file, `learner/history.md`, `mentor/`.

- [ ] **Step 1: Write the failing tests** — create `src/reset.rs` with only the test module for now (plus `mod reset;` in main.rs so it compiles into the test binary):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn project(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("usta_reset_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "x").unwrap();
    }

    #[test]
    fn topic_artifacts_lists_exactly_this_topics_files() {
        let root = project("artifacts");
        for t in ["rust", "rust-async"] {
            touch(&progress::progress_path(&root, t));
            touch(&progress::approach_path(&root, t));
            touch(&progress::curriculum_path(&root, t));
            touch(&root.join(".usta/visuals").join(t).join("a.html"));
            touch(&root.join(format!(".usta/sessions/{t}-20261026-101500.jsonl")));
            touch(&root.join(format!(".usta/sessions/{t}-20261025-090000.done.jsonl")));
        }
        touch(&progress::progress_path(&root, "rust").with_extension("md.bak"));
        touch(&crate::lifecycle::lock_path(&root, "rust"));

        let got = topic_artifacts(&root, "rust");
        // progress + its .bak, approach, curriculum, visuals dir, two transcripts
        assert_eq!(got.len(), 7, "{got:?}");
        assert!(got.iter().all(|p| !p.to_string_lossy().contains("rust-async")));
        assert!(!got.contains(&crate::lifecycle::lock_path(&root, "rust")));

        remove_artifacts(&got).unwrap();
        assert!(topic_artifacts(&root, "rust").is_empty());
        assert_eq!(topic_artifacts(&root, "rust-async").len(), 6);
        assert!(remove_artifacts(&got).is_ok(), "removal is idempotent");
        assert!(crate::lifecycle::lock_path(&root, "rust").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn reset_without_progress_still_removes_curriculum_and_catalog_row() {
        let root = project("noprogress");
        let global = root.join("global");
        touch(&progress::curriculum_path(&root, "rust"));
        index::record(&global, "rust", &root, "2026-10-26").unwrap();
        index::record(&global, "dns", &root, "2026-10-26").unwrap();
        let index_path = global.join("learner/index.md");

        let plan = plan_reset(&root, &index_path, "rust");
        assert_eq!(plan.artifacts, vec![progress::curriculum_path(&root, "rust")]);
        assert!(plan.catalog.is_some());
        apply_reset(&plan, &index_path).unwrap();

        assert!(!progress::curriculum_path(&root, "rust").exists());
        let cat = std::fs::read_to_string(&index_path).unwrap();
        let topics: Vec<String> = index::entries(&cat).into_iter().map(|e| e.topic).collect();
        assert_eq!(topics, vec!["dns".to_string()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn nothing_to_reset_without_files_or_catalog_row() {
        let root = project("nothing");
        let plan = plan_reset(&root, &root.join("global/learner/index.md"), "rust");
        assert!(plan.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn record_topic_reads_open_and_done_records() {
        let p = |n: &str| PathBuf::from(format!("/p/.usta/sessions/{n}"));
        assert_eq!(record_topic(&p("rust-20261026-101500.jsonl")).as_deref(), Some("rust"));
        assert_eq!(record_topic(&p("rust-20261026-101500.done.jsonl")).as_deref(), Some("rust"));
        assert_eq!(
            record_topic(&p("rust-async-20261026-101500.done.jsonl")).as_deref(),
            Some("rust-async")
        );
        assert_eq!(record_topic(&p("notes.txt")), None);
    }
}
```

In `src/main.rs` add `mod reset;` between `mod progress;` and `mod session;`.

- [ ] **Step 2: Run to verify they fail**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test reset::`
Expected: compile errors — `cannot find function topic_artifacts` / `plan_reset` / `record_topic` / `remove_artifacts`, unresolved `progress`, `index`.

- [ ] **Step 3: Implement** — put this production code above the test module in `src/reset.rs`:

```rust
//! `usta reset <topic>`: removes every per-topic artifact of the current
//! project — progress, approach, curriculum (and their `.bak`), generated
//! visuals, session transcripts — plus the topic's catalog row. Moved out of
//! `setup.rs` (600-line budget). Not touched: the topic lock (a live session
//! may own it), `learner/history.md` (global stats), `mentor/` (the user's docs).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::setup::{confirm, run_migration};
use crate::{config, index, progress, transcript};

/// `usta reset <topic>` — list what will go, confirm, remove. No LLM needed.
pub(crate) fn run_reset_topic(topic: &str) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let Some(root) = config::find_project_root(&cwd) else {
        anyhow::bail!("no .usta in this directory (or above) — no project found to reset");
    };
    let global = config::global_root()?;
    run_migration(&global, Some(&root.join(".usta")));
    let index_path = global.join("learner/index.md");
    let plan = plan_reset(&root, &index_path, topic);
    if plan.is_empty() {
        println!("nothing to reset for {topic}");
        return Ok(());
    }
    println!("{topic}: will be removed:");
    for p in &plan.artifacts {
        println!("  {}", p.display());
    }
    if plan.catalog.is_some() {
        println!("  catalog row in {}", index_path.display());
    }
    if !confirm("Are you sure? [y/N] ", &["e", "evet", "y", "yes"])? {
        println!("cancelled.");
        return Ok(());
    }
    apply_reset(&plan, &index_path)?;
    println!("reset: {topic}");
    Ok(())
}

/// What a reset would remove: the existing artifacts and, when the catalog has
/// a row for (topic, project), the catalog text without it.
struct ResetPlan {
    artifacts: Vec<PathBuf>,
    catalog: Option<String>,
}

impl ResetPlan {
    fn is_empty(&self) -> bool {
        self.artifacts.is_empty() && self.catalog.is_none()
    }
}

fn plan_reset(root: &Path, index_path: &Path, topic: &str) -> ResetPlan {
    let catalog = std::fs::read_to_string(index_path)
        .ok()
        .filter(|c| {
            index::entries(c)
                .iter()
                .any(|e| e.topic == topic && e.project == root)
        })
        .map(|c| index::remove(&c, topic, root));
    ResetPlan {
        artifacts: topic_artifacts(root, topic),
        catalog,
    }
}

/// Remove the artifacts, then the catalog row — the row goes even when no
/// progress file exists (a cancelled session leaves a row and maybe a curriculum).
fn apply_reset(plan: &ResetPlan, index_path: &Path) -> Result<()> {
    remove_artifacts(&plan.artifacts)?;
    if let Some(updated) = &plan.catalog {
        progress::write_atomic(index_path, updated)?;
    }
    Ok(())
}

/// Every EXISTING per-topic artifact under `root`. Transcripts match the topic
/// exactly (`rust` never takes `rust-async`'s records).
fn topic_artifacts(root: &Path, topic: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for md in [
        progress::progress_path(root, topic),
        progress::approach_path(root, topic),
        progress::curriculum_path(root, topic),
    ] {
        let bak = md.with_extension("md.bak");
        out.push(md);
        out.push(bak);
    }
    out.push(root.join(".usta/visuals").join(topic));
    out.retain(|p| p.exists());
    if let Ok(rd) = std::fs::read_dir(root.join(".usta/sessions")) {
        let mut records: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| record_topic(p).as_deref() == Some(topic))
            .collect();
        records.sort();
        out.extend(records);
    }
    out
}

/// Topic of a transcript, `.done.jsonl` included — `topic_from_record` alone
/// reads the stem `x-<date>-<time>.done` as having no timestamp.
fn record_topic(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let stem = name
        .strip_suffix(".done.jsonl")
        .or_else(|| name.strip_suffix(".jsonl"))?;
    transcript::topic_from_record(Path::new(&format!("{stem}.jsonl")))
}

/// Delete files and directories; one that is already gone is fine (idempotent).
fn remove_artifacts(paths: &[PathBuf]) -> Result<()> {
    for p in paths {
        let res = if p.is_dir() {
            std::fs::remove_dir_all(p)
        } else {
            std::fs::remove_file(p)
        };
        match res {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("could not delete: {}", p.display())),
        }
    }
    Ok(())
}
```

`src/setup.rs`: delete :203-251 (the `/// \`usta reset <topic>\` …` doc comment, `run_reset_topic`, the `/// Removes a topic's generated visuals …` doc comment and `remove_topic_visuals`). Replace the module doc :1-5 with:

```rust
//! Scaffold writing (`usta init`), the `topics`/`stats` reports, the factory
//! and profile reset flows, and the confirm prompts they share — extracted from
//! `main.rs` (module split, Task 7). `usta reset <topic>` lives in `reset.rs`.
//! All of these live behind a CLI subcommand and never enter the session loop.
```

Then remove any import of `setup.rs` :7-12 that clippy reports unused (likely none: `index`, `Context`, `progress` are used by the factory/profile/topics code — let the compiler decide; remove only what it flags).

`src/setup_tests.rs`: delete :476-519 (the `/// Görev 5: …` comment block and both `remove_topic_visuals_*` tests). Keep `reset_topic_leaves_mentor_dir_untouched` (:77) — it pins the path contract reset.rs relies on.

`src/main.rs`: drop `run_reset_topic` from the `use crate::setup::{…}` list (:44-47) and change :60 to:

```rust
        Command::Reset(ResetTarget::Topic(t)) => return reset::run_reset_topic(&t),
```

- [ ] **Step 4: Run to verify they pass**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test reset`
Expected: PASS — the four `reset::tests::*`, plus `setup::tests::reset_topic_leaves_mentor_dir_untouched` and the cli reset parsers.

- [ ] **Step 5: Gate** (files: `src/reset.rs`, `src/setup.rs`, `src/setup_tests.rs`, `src/main.rs`)

Expected: 670 passed, 1 failed (allowed), 1 ignored (−2 moved-away tests, +4 new). setup.rs ≈ 562 (must be ≤ 600 — this is the point of the task), reset.rs ≈ 125, main.rs ≈ 283.

- [ ] **Step 6: Commit**

```bash
git add src/reset.rs src/setup.rs src/setup_tests.rs src/main.rs
git commit -m "$(cat <<'EOF'
fix(reset): remove every artifact of the topic, catalog row always

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
EOF
)"
```

---

### Task 4: No feedback on usta's own writes (P2-4)

**Files:**
- Modify: `src/watcher.rs` (`is_ignored` :84-89, tests near :343)
- Modify: `src/lifecycle.rs` (`maybe_compact` :257-293, tests)
- Modify: `src/tui/run.rs:352`, `src/plain.rs:352` and `:400`, `src/tui/polite.rs:380` (re-seed after compaction)
- Modify: `src/file_feedback.rs` (test only)

**Interfaces:**
- Produces: `pub(crate) async fn lifecycle::maybe_compact(backend: &mut Backend, session: &mut Session, project_root: &Path, tokens: Option<u64>) -> bool` — `true` only when the interim flush succeeded and history was compacted (mentor/PROJECT.md and PROGRESS.md were rewritten).
- Consumes: `file_feedback::seed_mentor_baseline(files: &mut feedback::FileMemory, project_root: &Path)` (:172, unchanged).

Interim compaction writes `mentor/PROJECT.md`, `mentor/PROGRESS.md` and their `.md.bak` inside the watched project. (a) `.bak` names are ignored by the watcher; (b) every compaction call site re-seeds the mentor baseline, so the queued watcher events for those two files observe as `Skip`. PROJECT.md / PROGRESS.md themselves stay watched (users edit them).

- [ ] **Step 1: Write the failing tests**

`src/watcher.rs` `mod tests`, after `is_ignored_flags_tmp_suffix`:

```rust
    #[test]
    fn is_ignored_flags_backup_copies() {
        assert!(is_ignored(Path::new("mentor/PROJECT.md.bak")));
        assert!(is_ignored(Path::new("notes.BAK")));
        assert!(!is_ignored(Path::new("mentor/PROJECT.md")));
    }
```

`src/lifecycle.rs` `mod tests`, after `every_push_ask_site_rolls_back_on_error`:

```rust
    #[tokio::test]
    async fn maybe_compact_reports_whether_it_compacted() {
        let mut backend = Backend::Cli {
            model: "opus".into(),
            session_id: None,
            bin: PathBuf::from("usta-no-such-claude"),
        };
        let mut session = Session::new("rust", "system");
        let root = Path::new("/nonexistent-usta-project");
        assert!(!maybe_compact(&mut backend, &mut session, root, None).await);
        assert!(!maybe_compact(&mut backend, &mut session, root, Some(10)).await);
    }

    /// Source pin: a compaction rewrites mentor/PROJECT.md + PROGRESS.md inside the
    /// watched project — every call site re-seeds the watcher baseline when it
    /// compacted, so usta's own write is not fed back as a user change (P2-4).
    #[test]
    fn every_compaction_site_reseeds_the_mentor_baseline() {
        for (name, src, initial_seeds) in [
            ("tui/run.rs", include_str!("tui/run.rs"), 1),
            ("plain.rs", include_str!("plain.rs"), 1),
            ("tui/polite.rs", include_str!("tui/polite.rs"), 0),
        ] {
            let prod = src.split("#[cfg(test)]").next().unwrap();
            let calls = prod.matches("maybe_compact(").count();
            assert!(calls > 0, "{name}");
            let checked = prod.matches("if crate::lifecycle::maybe_compact(").count()
                + prod.matches("if maybe_compact(").count();
            assert_eq!(checked, calls, "{name}: compaction result ignored");
            assert_eq!(
                prod.matches("seed_mentor_baseline(").count(),
                calls + initial_seeds,
                "{name}"
            );
        }
    }
```

`src/file_feedback.rs` `mod tests`, after `scratch_dir`:

```rust
    #[test]
    fn reseeding_after_usta_rewrites_mentor_docs_makes_queued_events_skip() {
        let dir = scratch_dir("reseed-mentor");
        let md = progress::project_md_path(&dir);
        std::fs::create_dir_all(md.parent().unwrap()).unwrap();
        std::fs::write(&md, "# Project\nold\n").unwrap();
        let mut files = feedback::FileMemory::new();
        seed_mentor_baseline(&mut files, &dir);
        // The interim compaction rewrites the doc, the caller re-seeds…
        std::fs::write(&md, "# Project\nnew\n").unwrap();
        seed_mentor_baseline(&mut files, &dir);
        // …so the watcher event that follows observes no change.
        assert!(matches!(
            files.observe(&md, "# Project\nnew\n".to_string()),
            feedback::ChangePayload::Skip
        ));
        std::fs::remove_dir_all(&dir).ok();
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test compact` then `CARGO_TARGET_DIR=$PWD/target cargo test is_ignored_flags_backup`
Expected: compile error in `maybe_compact_reports_whether_it_compacted` — "cannot apply unary operator `!` to type `()`". (Temporarily comment it out to see the others: `every_compaction_site_reseeds_the_mentor_baseline` FAILS "compaction result ignored", `is_ignored_flags_backup_copies` FAILS, `reseeding_…_skip` PASSES — it guards the seed semantics the fix relies on. Uncomment before Step 3.)

- [ ] **Step 3: Implement**

`src/watcher.rs` `is_ignored`, after `|| s.ends_with(".tmp")` (:88):

```rust
                // Backup copies (usta's own `write_atomic` leaves `*.md.bak`).
                || s.ends_with(".bak")
```

`src/lifecycle.rs` `maybe_compact` — doc + signature + every return:

```rust
/// If the threshold is exceeded: interim flush → reload the system prompt with
/// fresh files → trim history → reset the CLI session. If the flush fails,
/// compaction is CANCELED — history is never discarded before the data lands on disk.
/// Returns true when it compacted: the flush rewrote mentor/PROJECT.md and
/// PROGRESS.md inside the watched project, so the caller re-seeds the watcher
/// baseline (`file_feedback::seed_mentor_baseline`, review P2-4).
pub(crate) async fn maybe_compact(
    backend: &mut Backend,
    session: &mut Session,
    project_root: &Path,
    tokens: Option<u64>,
) -> bool {
    let Some(t) = tokens else { return false };
    if !session.compaction_allowed() {
        return false;
    }
    if (t as f64) < COMPACT_THRESHOLD * backend.context_window() as f64 {
        return false;
    }
    if session.history().len() <= COMPACT_KEEP_LAST {
        return false;
    }
    ui::notice("context filling up — taking an interim checkpoint…");
    if let Err(e) = flush_progress(backend, session, project_root, false).await {
        ui::warn(&format!(
            "interim checkpoint failed, compaction postponed: {e}"
        ));
        session.compaction_failed();
        return false;
    }
    // … unchanged: system prompt reload, session.compact, backend.reset_session …
    ui::notice("context compacted — pick up where you left off");
    true
}
```

`src/tui/run.rs:352` (inside `select!`, not reformatted by rustfmt — keep it on one line like its neighbors):

```rust
                                if crate::lifecycle::maybe_compact(backend, &mut session, project_root, last_tokens).await { crate::file_feedback::seed_mentor_baseline(&mut files, project_root); }
```

`src/plain.rs:352`:

```rust
                                if maybe_compact(backend, session, ctx.project_root, tokens).await { seed_mentor_baseline(&mut files, ctx.project_root); }
```

`src/plain.rs:400` (same text):

```rust
                                if maybe_compact(backend, session, ctx.project_root, tokens).await { seed_mentor_baseline(&mut files, ctx.project_root); }
```

`src/tui/polite.rs:380` (`files: &mut FileMemory` is `process_batch`'s parameter):

```rust
                    if crate::lifecycle::maybe_compact(backend, session, project_root, tokens).await {
                        crate::file_feedback::seed_mentor_baseline(files, project_root);
                    }
```

- [ ] **Step 4: Run to verify they pass**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test compact` then `CARGO_TARGET_DIR=$PWD/target cargo test is_ignored` then `CARGO_TARGET_DIR=$PWD/target cargo test reseeding`
Expected: PASS.

- [ ] **Step 5: Gate** (files: `src/watcher.rs`, `src/lifecycle.rs`, `src/tui/run.rs`, `src/plain.rs`, `src/tui/polite.rs`, `src/file_feedback.rs`)

Expected: 674 passed, 1 failed (allowed), 1 ignored. lifecycle.rs ≈ 324, watcher.rs ≈ 240, polite.rs ≈ 463, run.rs 396, plain.rs 498, file_feedback.rs 538. `dispatch_flush_route_arms_are_pinned` and `every_push_ask_site_rolls_back_on_error` stay green.

- [ ] **Step 6: Commit**

```bash
git add src/watcher.rs src/lifecycle.rs src/tui/run.rs src/plain.rs src/tui/polite.rs src/file_feedback.rs
git commit -m "$(cat <<'EOF'
fix(watcher): ignore .bak, re-seed mentor baseline after compaction

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
EOF
)"
```

---

### Task 5: A torn last transcript line no longer blocks recovery (P2-5)

**Files:**
- Modify: `src/transcript.rs` (`read_history` :68-108, tests)

**Interfaces:**
- `pub fn read_history(path: &Path) -> Result<Vec<Message>>` — signature unchanged. Reads bytes (`String::from_utf8_lossy`); an unparseable LAST non-empty line is skipped with `ui::warn("last session line was incomplete — skipped")`; an unparseable middle line is still an `Err`. Only caller: main.rs:139 (recovery).

- [ ] **Step 1: Write the failing tests** — `src/transcript.rs` `mod tests`, after `read_history_drops_the_user_turn_a_rollback_undid`:

```rust
    #[test]
    fn read_history_skips_a_torn_last_line() {
        let base =
            std::env::temp_dir().join(format!("usta_transcript_torn_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let p = base.join("rust-1.jsonl");
        let r = Recorder::new(p.clone());
        r.user("merhaba");
        r.assistant("selam");
        // Crash mid-append: half a line, cut inside the multibyte 'ş' (0xC5 0x9F).
        let mut torn = br#"{"role":"user","text":"ba"#.to_vec();
        torn.push(0xC5);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&p)
            .unwrap()
            .write_all(&torn)
            .unwrap();
        let h = read_history(&p).unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(h[1].role, "assistant");
        // An ASCII-only torn tail is skipped the same way.
        let q = base.join("rust-2.jsonl");
        std::fs::write(&q, format!("{}{}", line("user", "a"), r#"{"role":"assis"#)).unwrap();
        assert_eq!(read_history(&q).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn read_history_rejects_a_bad_middle_line() {
        let base =
            std::env::temp_dir().join(format!("usta_transcript_badmid_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let p = base.join("rust-1.jsonl");
        std::fs::write(
            &p,
            format!("{}not json\n{}", line("user", "a"), line("assistant", "b")),
        )
        .unwrap();
        assert!(read_history(&p).is_err(), "a salvage flush must not drop turns");
        let _ = std::fs::remove_dir_all(&base);
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test read_history`
Expected: `read_history_skips_a_torn_last_line` FAILS (`stream did not contain valid UTF-8` unwrap); `read_history_rejects_a_bad_middle_line` PASSES (guard — the fix must keep it green).

- [ ] **Step 3: Implement** — replace the head of `read_history` up to the `let role = …` line, and update its doc:

```rust
/// Read a transcript file written by `Recorder` (`{"role","text"}` per line)
/// and reconstruct it as `Message` history, preserving order. A line that fails
/// to parse, or carries a role other than "user"/"assistant"/"rollback", fails
/// the whole read — a salvage flush must not silently drop turns — except the
/// LAST line: a crash mid-append leaves it torn, and only that one is skipped
/// (with a warning). Bytes are read lossily, so a cut multibyte char can't fail
/// the read. A "rollback" line drops the user turn the live session rolled back.
pub fn read_history(path: &Path) -> Result<Vec<Message>> {
    let bytes = std::fs::read(path)?;
    let content = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let v: serde_json::Value = match serde_json::from_str(l) {
            Ok(v) => v,
            Err(_) if i + 1 == lines.len() => {
                crate::ui::warn("last session line was incomplete — skipped");
                break;
            }
            Err(e) => return Err(e.into()),
        };
        let role = v
```

The rest of the loop body (role/text extraction, the `match role` with `"rollback"`) is unchanged.

- [ ] **Step 4: Run to verify they pass**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test read_history`
Expected: PASS (all `read_history_*`).

- [ ] **Step 5: Gate** (files: `src/transcript.rs`)

Expected: 676 passed, 1 failed (allowed), 1 ignored. transcript.rs ≈ 182.

- [ ] **Step 6: Commit**

```bash
git add src/transcript.rs
git commit -m "$(cat <<'EOF'
fix(transcript): skip a torn last line instead of failing recovery

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
EOF
)"
```

---

### Task 6: CLI json-mode errors are errors (P2-6)

**Files:**
- Modify: `src/backend.rs` (`parse_cli_output` :224-238, `parse_stream_output` :281-298, `stream_failure` :300-310 → `cli_failure`, `run_claude_cli` exit handling :431-448, tests :637-664 and :848-857)

**Interfaces:**
- Produces: `pub fn parse_cli_output(stdout: &str) -> Result<(String, Option<String>, Option<u64>)>` — `Err(anyhow!("claude CLI error: {result}"))` when the json has `is_error: true` (falls back to `subtype`, then `unknown error`); raw-text fallback for non-json stdout kept.
- `pub fn parse_stream_output(stdout: &str) -> Result<(String, Option<String>, Option<u64>)>` — unchanged contract; its own `is_error` branch is replaced by delegating to `parse_cli_output` (same message format).
- `fn cli_failure(stream: bool, stdout: &str) -> Option<String>` (renamed from `stream_failure`): on a non-zero exit, the `claude CLI error: …` text from stdout in BOTH modes; `None` → the stderr-based bail.
- Task 7 relies on the message text `claude CLI error: No conversation found with session ID: …` arriving as an `Err` on both exit paths.

- [ ] **Step 1: Write the failing tests** — `src/backend.rs` `mod tests`. First add `.unwrap()` to the four existing tuple tests (`parse_cli_output_reads_json_result_and_session` :640, `parse_cli_output_falls_back_to_plain_text` :647, `parse_cli_output_reads_usage_tokens` :655, `parse_cli_output_tokens_none_when_usage_missing` :662), e.g.:

```rust
        let (text, sid, _) = parse_cli_output(out).unwrap();
```

Add after `parse_cli_output_tokens_none_when_usage_missing`:

```rust
    #[test]
    fn parse_cli_output_errors_on_is_error_json() {
        let out = r#"{"type":"result","subtype":"success","is_error":true,"result":"Credit balance is too low","session_id":"s"}"#;
        let err = parse_cli_output(out).unwrap_err().to_string();
        assert_eq!(err, "claude CLI error: Credit balance is too low");
    }
```

Replace `stream_failure_surfaces_the_result_line_error` (:848-857) with:

```rust
    #[test]
    fn cli_failure_surfaces_the_result_error_in_both_modes() {
        let stream_out = "{\"type\":\"system\"}\n{\"type\":\"result\",\"is_error\":true,\"subtype\":\"error_during_execution\"}\n";
        let msg = cli_failure(true, stream_out).unwrap();
        assert!(msg.contains("error_during_execution"), "{msg}");
        // json mode: the exit-1 result text is no longer dropped (stderr is empty).
        let json_out = r#"{"type":"result","is_error":true,"result":"API Error: Claude's response exceeded the 32000 output token maximum"}"#;
        let msg = cli_failure(false, json_out).unwrap();
        assert!(msg.contains("exceeded the 32000 output token maximum"), "{msg}");
        // stdout without an is_error result keeps the stderr bail.
        assert!(cli_failure(true, "").is_none());
        assert!(cli_failure(true, "{\"type\":\"system\"}\n").is_none());
        assert!(cli_failure(false, "plain text").is_none());
        assert!(cli_failure(false, r#"{"result":"ok","is_error":false}"#).is_none());
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test parse_cli_output`
Expected: compile errors — `no method named unwrap found for tuple`, `cannot find function cli_failure`.

- [ ] **Step 3: Implement**

`parse_cli_output` (:222-238):

```rust
/// Parse `claude -p --output-format json` output. An `is_error` result (credit,
/// API error, missing session — exit 0 or not) is an `Err` carrying its text,
/// never a mentor reply. Not JSON (old version / unexpected output) → raw text,
/// continuing without a session id.
pub fn parse_cli_output(stdout: &str) -> Result<(String, Option<String>, Option<u64>)> {
    #[derive(serde::Deserialize)]
    struct CliJson {
        result: Option<String>,
        session_id: Option<String>,
        usage: Option<serde_json::Value>,
        is_error: Option<bool>,
        subtype: Option<String>,
    }
    match serde_json::from_str::<CliJson>(stdout) {
        Ok(j) if j.is_error == Some(true) => {
            let why = j.result.or(j.subtype);
            bail!("claude CLI error: {}", why.as_deref().unwrap_or("unknown error"))
        }
        Ok(j) => {
            let tokens = j.usage.as_ref().and_then(anthropic::sum_context_tokens);
            Ok((j.result.unwrap_or_default(), j.session_id, tokens))
        }
        Err(_) => Ok((stdout.trim().to_string(), None, None)),
    }
}
```

`parse_stream_output` (:281-298) — drop its own `is_error` branch:

```rust
pub fn parse_stream_output(stdout: &str) -> Result<(String, Option<String>, Option<u64>)> {
    let v = stdout
        .lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["type"] == "result")
        .context("claude CLI returned no result")?;
    parse_cli_output(&v.to_string())
}
```

Replace `stream_failure` (:300-310) with:

```rust
/// On a non-zero exit the useful message is the `is_error` result on stdout (stderr
/// is empty) — json mode prints one object, stream mode a result line among NDJSON.
/// `None` → fall back to the stderr-based exit-status error.
fn cli_failure(stream: bool, stdout: &str) -> Option<String> {
    let err = if stream {
        parse_stream_output(stdout).err()
    } else {
        parse_cli_output(stdout).err()
    };
    err.map(|e| e.to_string())
        .filter(|m| m.starts_with("claude CLI error:"))
}
```

`run_claude_cli` (:431-448):

```rust
    if !output.status.success() {
        if let Some(msg) = cli_failure(stream, &String::from_utf8_lossy(&output.stdout)) {
            bail!("{msg}");
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "claude CLI returned an error ({}): {}",
            output.status,
            stderr.trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stream {
        parse_stream_output(&stdout)
    } else {
        parse_cli_output(&stdout)
    }
```

- [ ] **Step 4: Run to verify they pass**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test parse_` then `CARGO_TARGET_DIR=$PWD/target cargo test cli_failure`
Expected: PASS — including `parse_stream_output_errors_on_is_error_result` (message still contains the subtype) and `parse_stream_output_errors_without_a_result_line`.

- [ ] **Step 5: Gate** (files: `src/backend.rs`)

Expected: 677 passed, 1 failed (allowed), 1 ignored. backend.rs ≈ 540.

- [ ] **Step 6: Commit**

```bash
git add src/backend.rs
git commit -m "$(cat <<'EOF'
fix(backend): CLI is_error json results are errors, not replies

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
EOF
)"
```

---

### Task 7: Resume retry only for a missing session (P2-7)

**Files:**
- Modify: `src/backend.rs` (`complete_raw` Cli arm :175-186, new fn after `ensure_nonempty` :197-202, tests)

**Interfaces:**
- Consumes: Task 6 — a missing session arrives as `Err("claude CLI error: No conversation found with session ID: …")` (or in the stderr bail).
- Produces: private `fn is_missing_session(e: &anyhow::Error) -> bool` — case-insensitive `no conversation found` anywhere in the error chain.

Any other error (rate limit, auth, network, credit) is returned as-is and `session_id` is kept. If the full retry also fails, its message carries the first error. Note: the old comment's "also recovers a session overgrown with images" case no longer retries automatically — that is the spec's decision (the error now surfaces instead of silently re-sending everything).

**Controller amendment (overrides the code below where they differ):** keep the overgrown-session recovery. The pure predicate is named `should_retry_fresh(e: &anyhow::Error) -> bool` and is true (case-insensitive, over the error chain) when the text contains `no conversation found`, `prompt is too long`, `too many images`, or contains `image` together with one of `exceed`, `too large`, `limit`. Everything else (rate limit, 429/529, overloaded, auth, network, credit) returns the original error with `session_id` kept. Tests: true for `No conversation found with session ID: x`, `Prompt is too long`, `too many images in request`, `image exceeds 5 MB maximum`; false for `rate limit exceeded`, `529 overloaded`, `authentication failed`. Keep the code comment accurate (mention both cases).

- [ ] **Step 1: Write the failing test** — `src/backend.rs` `mod tests`:

```rust
    #[test]
    fn is_missing_session_matches_only_the_gone_session_error() {
        assert!(is_missing_session(&anyhow::anyhow!(
            "claude CLI error: No conversation found with session ID: 1234"
        )));
        assert!(is_missing_session(
            &anyhow::anyhow!("no conversation found").context("claude CLI returned an error")
        ));
        assert!(!is_missing_session(&anyhow::anyhow!("claude CLI error: rate limit reached")));
        assert!(!is_missing_session(&anyhow::anyhow!(
            "claude CLI returned an error (exit status: 1): "
        )));
        // Source pin: the retry arm is gated on the predicate, not on any error.
        let prod = include_str!("backend.rs").split("#[cfg(test)]").next().unwrap();
        assert!(prod.contains("Err(first) if resume.is_some() && is_missing_session(&first)"));
        assert!(!prod.contains("Err(_) if resume.is_some()"));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test is_missing_session`
Expected: compile error — `cannot find function is_missing_session`.

- [ ] **Step 3: Implement**

After `ensure_nonempty` (:202):

```rust
/// The resumed CLI session no longer exists (`claude --resume` → "No conversation
/// found with session ID: …") — the only error worth a full-transcript retry.
fn is_missing_session(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        c.to_string()
            .to_ascii_lowercase()
            .contains("no conversation found")
    })
}
```

`complete_raw`, the `match attempt` (:175-186):

```rust
                let (text, new_sid, tokens) = match attempt {
                    Ok(v) => v,
                    // The resumed session is gone — retry once from scratch with the full
                    // transcript and every budgeted image. Any other error (rate limit,
                    // auth, network) is returned as-is and the session id kept.
                    Err(first) if resume.is_some() && is_missing_session(&first) => {
                        *session_id = None;
                        let transcript = render_transcript(history);
                        let images = history_images(history);
                        run_claude_cli(bin, model, system, &transcript, &images, None)
                            .await
                            .map_err(|e| anyhow::anyhow!("{e} (after: {first})"))?
                    }
                    Err(e) => return Err(e),
                };
```

- [ ] **Step 4: Run to verify it passes**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test is_missing_session`
Expected: PASS.

- [ ] **Step 5: Gate** (files: `src/backend.rs`)

Expected: 678 passed, 1 failed (allowed), 1 ignored. backend.rs ≈ 551. The `ensure_nonempty` pin stays green.

- [ ] **Step 6: Commit**

```bash
git add src/backend.rs
git commit -m "$(cat <<'EOF'
fix(backend): resume retry only when the CLI session is gone

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
EOF
)"
```

---

### Task 8: Recovery skips a session a running usta owns (P2-8)

**Files:**
- Create: `src/lock.rs`
- Modify: `src/main.rs` (`mod lock;` between `mod lifecycle;` :21 and `mod materials;` :22; recovery list :111)

**Interfaces:**
- Consumes: `lifecycle::lock_path(project_root: &Path, topic: &str) -> PathBuf` (:304, lock content = pid), `transcript::topic_from_record`.
- Produces (Task 9 extends this file):
  - `pub(crate) fn lock::pid_alive(pid: u32) -> bool` — Unix: `kill -0 <pid>` via `std::process::Command`, exit success = alive; Windows: `tasklist /FI "PID eq <pid>" /NH` stdout contains the pid as a token. Spawn failure → `false`.
  - `pub(crate) fn lock::live_owner(root: &Path, topic: &str) -> Option<u32>` — lock exists, pid parses, pid ≠ 0, pid ≠ own pid, alive.
  - `pub(crate) fn lock::split_live(root: &Path, records: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<String>)` — (recoverable records, deduplicated topics owned by a running usta). A record whose name doesn't parse stays recoverable (the recovery loop already reports it).

`kill -0` on a pid owned by another user returns EPERM → non-zero → treated as dead. Acceptable: two users never share one project's `.usta`, and a false "dead" only means recovery is offered (today's behavior). The live-owned records are filtered out BEFORE the prompt, so they are neither recovered (flushed + renamed under the live writer) nor deleted by a "no" answer, nor listed in pipe mode. A lock whose pid is dead does not block recovery. The lock-conflict prompts (main.rs:219, tui/run.rs:73) are unchanged.

- [ ] **Step 1: Write the failing tests** — create `src/lock.rs` with the test module (and `mod lock;` in main.rs):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn project(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("usta_lock_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join(".usta/sessions")).unwrap();
        base
    }

    #[cfg(unix)]
    fn reaped_pid() -> u32 {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    #[test]
    fn pid_alive_true_for_self() {
        assert!(pid_alive(std::process::id()));
    }

    #[cfg(unix)]
    #[test]
    fn pid_alive_false_for_a_reaped_child() {
        assert!(!pid_alive(reaped_pid()));
    }

    #[cfg(unix)]
    #[test]
    fn live_owner_ignores_missing_own_and_dead_pids() {
        let root = project("owner");
        let lock = crate::lifecycle::lock_path(&root, "rust");
        assert_eq!(live_owner(&root, "rust"), None, "no lock");
        std::fs::write(&lock, std::process::id().to_string()).unwrap();
        assert_eq!(live_owner(&root, "rust"), None, "own pid");
        std::fs::write(&lock, reaped_pid().to_string()).unwrap();
        assert_eq!(live_owner(&root, "rust"), None, "dead pid = crashed run");
        std::fs::write(&lock, "garbage").unwrap();
        assert_eq!(live_owner(&root, "rust"), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn split_live_skips_records_a_running_usta_owns() {
        let root = project("split");
        let mut other = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        std::fs::write(crate::lifecycle::lock_path(&root, "rust"), other.id().to_string()).unwrap();
        std::fs::write(crate::lifecycle::lock_path(&root, "dns"), reaped_pid().to_string()).unwrap();
        let rec = |n: &str| root.join(".usta/sessions").join(n);
        let records = vec![
            rec("dns-20261026-101500.jsonl"),
            rec("odd.jsonl"),
            rec("rust-20261026-101500.jsonl"),
            rec("rust-20261026-111500.jsonl"),
        ];
        let (free, live) = split_live(&root, records);
        assert_eq!(
            free,
            vec![rec("dns-20261026-101500.jsonl"), rec("odd.jsonl")]
        );
        assert_eq!(live, vec!["rust".to_string()]);
        let _ = other.kill();
        let _ = other.wait();
        let _ = std::fs::remove_dir_all(&root);
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test lock::`
Expected: compile errors — `cannot find function pid_alive` / `live_owner` / `split_live`.

- [ ] **Step 3: Implement** — production code above the tests in `src/lock.rs`:

```rust
//! Topic lock (`.usta/.lock-<topic>`, content: the owner's pid): is the owner
//! still running? Liveness is probed with the platform's own tool — no new
//! crates (an OS advisory lock would need one, or Rust 1.89). Review P2-8.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Is a process with this pid running? Unix: `kill -0 <pid>` exits 0. A pid
/// owned by another user answers EPERM → non-zero → treated as dead (a false
/// "dead" only means recovery is offered). Windows: `tasklist` lists the pid.
/// A tool that can't be spawned → dead.
pub(crate) fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
    #[cfg(windows)]
    {
        let want = pid.to_string();
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .is_ok_and(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .split_whitespace()
                    .any(|w| w == want)
            })
    }
}

/// The pid of ANOTHER running usta holding this topic's lock, if any. A lock
/// left by a crashed run (dead pid) or by this process reads as no owner.
pub(crate) fn live_owner(root: &Path, topic: &str) -> Option<u32> {
    let pid: u32 = std::fs::read_to_string(crate::lifecycle::lock_path(root, topic))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    (pid != 0 && pid != std::process::id() && pid_alive(pid)).then_some(pid)
}

/// Split unfinished transcripts into (recoverable, topics a running usta owns).
/// A live session's transcript must not be flushed, renamed or deleted under it.
pub(crate) fn split_live(root: &Path, records: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<String>) {
    let mut free = Vec::new();
    let mut live: Vec<String> = Vec::new();
    for p in records {
        match crate::transcript::topic_from_record(&p) {
            Some(t) if live_owner(root, &t).is_some() => {
                if !live.contains(&t) {
                    live.push(t);
                }
            }
            _ => free.push(p),
        }
    }
    (free, live)
}
```

`src/main.rs:111` — replace `let stale = transcript::find_unfinished(&project_root);` with:

```rust
    // A record whose topic lock belongs to another RUNNING usta is that session's
    // live transcript — never recover (flush + rename) or delete it (review P2-8).
    let (stale, live) = lock::split_live(&project_root, transcript::find_unfinished(&project_root));
    for topic in live {
        ui::notice(&format!("skipping {topic}: a running usta owns this session"));
    }
```

- [ ] **Step 4: Run to verify they pass**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test lock::`
Expected: PASS (4).

- [ ] **Step 5: Gate** (files: `src/lock.rs`, `src/main.rs`)

Expected: 682 passed, 1 failed (allowed), 1 ignored. lock.rs ≈ 70, main.rs ≈ 289.

- [ ] **Step 6: Commit**

```bash
git add src/lock.rs src/main.rs
git commit -m "$(cat <<'EOF'
fix: startup recovery skips a session a running usta owns

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
EOF
)"
```

---

### Task 9: Lock guard + plain-path flush on loop error (P2-9, part 1)

**Files:**
- Modify: `src/lock.rs` (add `LockGuard` + test)
- Modify: `src/lifecycle.rs` (`build_session` :54-86: doc, return type, lock write :67-71)
- Modify: `src/tui/run.rs` (doc :28-33, return type :42; the build_session binding :87-89 keeps the name `lock`)
- Modify: `src/main.rs` (TUI/plain branch tuple :190-265, closing tail :267-281)
- Modify: `src/tui/closing.rs` (new source-pin test)

**Interfaces:**
- Consumes: Task 8's `lock.rs`.
- Produces:
  - `pub(crate) struct lock::LockGuard(PathBuf)` with `pub(crate) fn acquire(path: PathBuf) -> LockGuard` (writes the own pid; a write failure only warns `topic lock could not be written: {e}`, as today) and `impl Drop` (removes the file, errors ignored).
  - `lifecycle::build_session(global, project_root, topic, today) -> Result<(Session, Recorder, crate::lock::LockGuard, bool)>`.
  - `tui::run::run(…) -> Result<Option<(Session, Recorder, crate::lock::LockGuard, Result<()>)>>`.

Every exit after `build_session` — normal, `?` error, panic with unwinding (no `panic = "abort"` in Cargo.toml; the TUI panic hook restores the terminal and chains, it does not exit) — now drops the guard and removes the lock. The plain path keeps the loop's result, always flushes, runs the shared closing (mark_done, lock drop) and only then propagates the error, so a successful flush is not followed by a second recovery flush on the next start.

- [ ] **Step 1: Write the failing tests**

`src/lock.rs` `mod tests`:

```rust
    #[test]
    fn lock_guard_writes_own_pid_and_removes_the_file_on_drop() {
        let root = project("guard");
        let path = crate::lifecycle::lock_path(&root, "rust");
        let guard = LockGuard::acquire(path.clone());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            std::process::id().to_string()
        );
        assert_eq!(live_owner(&root, "rust"), None, "own lock is not another owner");
        drop(guard);
        assert!(!path.exists(), "every exit path drops the guard");
        let _ = std::fs::remove_dir_all(&root);
    }
```

`src/tui/closing.rs` `mod tests`, after `main_flushes_only_on_the_plain_path`:

```rust
    /// Source pin: a plain-loop error no longer skips the closing flush — the
    /// loop result is kept, the flush runs, and the error propagates last (P2-9).
    #[test]
    fn main_flushes_the_plain_session_even_when_the_loop_fails() {
        let src = prod(include_str!("../main.rs"));
        let after_loop = src.split("run_plain_loop(").nth(1).expect("plain loop call");
        let until_flush = after_loop.split("flush_progress(").next().unwrap();
        assert!(
            !until_flush.contains(".await?"),
            "a loop error must not skip the flush"
        );
        assert!(src.contains("drop(lock);"), "the lock goes through its guard");
        assert!(src.contains("looped?;"));
        assert!(!src.contains("remove_file(&lock)"));
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test lock_guard` then `CARGO_TARGET_DIR=$PWD/target cargo test main_flushes_the_plain`
Expected: compile error `cannot find type LockGuard`; the closing pin FAILS "a loop error must not skip the flush".

- [ ] **Step 3: Implement**

`src/lock.rs` — module doc gains one line (`//! \`LockGuard\` removes the lock on every exit path (review P2-9).`) and, after `split_live`:

```rust
/// The topic lock this process holds. Dropping it removes the file — on the
/// normal close, on any `?` error and on a panic that unwinds.
pub(crate) struct LockGuard(PathBuf);

impl LockGuard {
    /// Write the own pid to `path`. A write failure only warns: the lock is a
    /// courtesy check, it must never block the session.
    pub(crate) fn acquire(path: PathBuf) -> LockGuard {
        if let Err(e) = std::fs::write(&path, std::process::id().to_string()) {
            crate::ui::warn(&format!("topic lock could not be written: {e}"));
        }
        LockGuard(path)
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
```

`src/lifecycle.rs` `build_session` — doc line ":`(session, recorder, lock_path, has_progress)`" becomes "`(session, recorder, lock guard, has_progress)` — dropping the guard removes the lock."; signature:

```rust
) -> Result<(Session, Recorder, crate::lock::LockGuard, bool)> {
```

and replace :67-71 (`let lock = lock_path(…); if let Err(e) = std::fs::write(…) { … }`) with:

```rust
    let lock = crate::lock::LockGuard::acquire(lock_path(project_root, topic));
```

`src/tui/run.rs:42`:

```rust
) -> Result<Option<(Session, Recorder, crate::lock::LockGuard, Result<()>)>> {
```

and in the doc (:30) `Some((session, recorder, lock, flushed))` stays; append to that sentence "(`lock` is the guard — dropping it removes the lock file)". The comment at :87 becomes `// build_session writes its own lock and returns its guard.`

`src/main.rs` :190 and the TUI match (:206-213):

```rust
    let (session, recorder, lock, flushed, looped) = if ui::use_tui() {
```

```rust
        match r? {
            Some((session, recorder, lock, flushed)) => (session, recorder, lock, flushed, Ok(())),
            None => {
```

Plain branch :248-264:

```rust
        let looped = run_plain_loop(
            &mut backend,
            &mut session,
            &mut watch_rx,
            PlainLoopCtx {
                recorder: &recorder,
                project_root: &project_root,
                global: &global,
                topic: &topic,
                has_progress,
                intro: intro.as_deref(),
                profile_generic,
            },
        )
        .await;
        // Flush even when the loop failed — the turns are in memory; the error
        // propagates after the closing steps below.
        let flushed = flush_progress(&mut backend, &session, &project_root, true).await;
        (session, recorder, lock, flushed, looped)
    };
```

Closing tail (:278-281):

```rust
    drop(lock); // LockGuard: removes the topic lock
    looped?;

    ui::notice("See you — keep getting in the water.");
    Ok(())
```

- [ ] **Step 4: Run to verify they pass**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test lock_guard` then `CARGO_TARGET_DIR=$PWD/target cargo test main_flushes`
Expected: PASS — both `main_flushes_*` pins and `run_saves_after_the_loop_before_returning`.

- [ ] **Step 5: Gate** (files: `src/lock.rs`, `src/lifecycle.rs`, `src/tui/run.rs`, `src/main.rs`, `src/tui/closing.rs`)

Expected: 684 passed, 1 failed (allowed), 1 ignored. lock.rs ≈ 92, lifecycle.rs ≈ 321, run.rs ≈ 396, main.rs ≈ 291.

- [ ] **Step 6: Commit**

```bash
git add src/lock.rs src/lifecycle.rs src/tui/run.rs src/main.rs src/tui/closing.rs
git commit -m "$(cat <<'EOF'
fix: lock guard on every exit path, plain flush survives loop errors

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
EOF
)"
```

---

### Task 10: SIGTERM / SIGHUP / console close save the session (P2-9, part 2)

**Files:**
- Modify: `Cargo.toml:9` (tokio features + `"signal"`) — `Cargo.lock` may not change (tokio's `signal-hook-registry` is already locked); commit it if it does
- Create: `src/shutdown.rs`
- Modify: `src/main.rs` (`mod shutdown;` between `mod setup;` :27 and `mod slash;` :28)
- Modify: `src/tui/run.rs` (after build_session :89; main loop `select!` :231)
- Modify: `src/tui/ask.rs` (`ask_live` `select!` :52-53)
- Modify: `src/plain.rs` (top of `run_plain_loop` :168; loop `select!` :261)

**Interfaces:**
- Produces:
  - `pub(crate) fn shutdown::listen()` — idempotent (`OnceLock`); registers the handlers once and spawns a task that flips a `tokio::sync::watch` flag to `true` when one fires. Registration failure → the flag never flips.
  - `pub(crate) async fn shutdown::requested()` — resolves once the flag is `true` (immediately if it already is); pends forever before `listen()` or when registration failed. Safe to re-create every `select!` iteration: the flag is state, so a signal that arrived while an arm body was running is not lost.
  - private `fn signals() -> std::io::Result<impl Future<Output = ()>>` — Unix: `tokio::signal::unix::signal(SignalKind::terminate())` + `signal(SignalKind::hangup())`; Windows: `tokio::signal::windows::ctrl_close()` + `ctrl_shutdown()`; the future resolves on the first of the two.

`listen()` is called only once a session exists (after `build_session` in the TUI, at the top of `run_plain_loop`): tokio's handler replaces the default terminate action for the rest of the process, so registering earlier would make `kill` ignored during topic entry. Arms: the TUI main loop breaks into the existing `save_live` path; `ask_live` returns `AskOutcome::Cancelled` (the callers' Cancelled handling runs, then the main loop sees the flag and breaks); the plain loop breaks to main's flush. `save_live` gets no arm (the save is deliberately not cancellable). SIGINT stays as today (raw-mode Ctrl+C is a key). A signal arriving during a non-selectable await (a plain-path `ask_usta`, `attach_pending`'s cargo check) is noticed after it — accepted by the spec. Windows gives a console-close handler only a few seconds before the process is terminated, so a long closing flush may be cut there; noted in the manual checklist.

- [ ] **Step 1: Write the failing tests** — create `src/shutdown.rs` with only its tests (plus `mod shutdown;` in main.rs):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Handler registered first, then SIGHUP to ourselves: the future resolves.
    /// (Once tokio owns SIGHUP the default "terminate" action is gone for the
    /// whole test process, so the signal can't kill the test run.)
    #[cfg(unix)]
    #[tokio::test]
    async fn sighup_to_self_resolves_the_signal_future() {
        let wait = signals().expect("handlers register");
        let sent = std::process::Command::new("kill")
            .args(["-HUP", &std::process::id().to_string()])
            .status()
            .unwrap();
        assert!(sent.success());
        tokio::time::timeout(std::time::Duration::from_secs(5), wait)
            .await
            .expect("SIGHUP reaches the future");
    }

    /// Source pin: every session select! loop watches the flag, and listening
    /// starts once the session exists.
    #[test]
    fn session_loops_watch_for_shutdown() {
        for (name, src) in [
            ("tui/run.rs", include_str!("tui/run.rs")),
            ("tui/ask.rs", include_str!("tui/ask.rs")),
            ("plain.rs", include_str!("plain.rs")),
        ] {
            let prod = src.split("#[cfg(test)]").next().unwrap();
            assert_eq!(prod.matches("crate::shutdown::requested()").count(), 1, "{name}");
        }
        assert!(include_str!("tui/run.rs").contains("crate::shutdown::listen();"));
        assert!(include_str!("plain.rs").contains("crate::shutdown::listen();"));
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test shutdown::`
Expected: compile error — `cannot find function signals`.

- [ ] **Step 3: Implement**

`Cargo.toml:9`:

```toml
tokio = { version = "1", features = ["macros", "rt-multi-thread", "process", "io-util", "sync", "time", "signal"] }
```

`src/shutdown.rs` production code above the tests:

```rust
//! Terminal close / kill → the normal save path (review P2-9). `listen()` runs
//! once a session exists; from then on `requested()` resolves as soon as
//! SIGTERM or SIGHUP (Unix) / a console close or shutdown event (Windows) has
//! arrived — immediately if it already did. SIGINT is untouched: in the TUI
//! Ctrl+C is a key.

use std::future::Future;
use std::sync::OnceLock;

use tokio::sync::watch;

static STOP: OnceLock<watch::Receiver<bool>> = OnceLock::new();

/// Register the handlers (once per process) and flip the flag when one fires.
/// If registration fails the flag never flips — the session runs as before.
pub(crate) fn listen() {
    STOP.get_or_init(|| {
        let (tx, rx) = watch::channel(false);
        if let Ok(wait) = signals() {
            tokio::spawn(async move {
                wait.await;
                let _ = tx.send(true);
            });
        }
        rx
    });
}

/// Resolves once a shutdown signal was seen; pends forever before `listen()`
/// or when registration failed. Re-created on every `select!` turn on purpose:
/// the flag is state, so a signal that arrived meanwhile is not lost.
pub(crate) async fn requested() {
    if let Some(rx) = STOP.get() {
        let mut rx = rx.clone();
        if rx.wait_for(|stop| *stop).await.is_ok() {
            return;
        }
    }
    std::future::pending::<()>().await
}

#[cfg(unix)]
fn signals() -> std::io::Result<impl Future<Output = ()>> {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = signal(SignalKind::terminate())?;
    let mut hup = signal(SignalKind::hangup())?;
    Ok(async move {
        tokio::select! {
            _ = term.recv() => {}
            _ = hup.recv() => {}
        }
    })
}

#[cfg(windows)]
fn signals() -> std::io::Result<impl Future<Output = ()>> {
    use tokio::signal::windows::{ctrl_close, ctrl_shutdown};
    let mut close = ctrl_close()?;
    let mut shutdown = ctrl_shutdown()?;
    Ok(async move {
        tokio::select! {
            _ = close.recv() => {}
            _ = shutdown.recv() => {}
        }
    })
}
```

`src/tui/run.rs` — right after the `build_session` call (:88-89):

```rust
    // From here on SIGTERM / SIGHUP end the session through the normal save.
    crate::shutdown::listen();
```

and in the main loop (:231), first arm of the `select!`:

```rust
        tokio::select! {
            _ = crate::shutdown::requested() => break, // terminal closed / killed → save_live below
            maybe_ev = events.next() => {
```

`src/tui/ask.rs` (:52-53), after the reply arm:

```rust
        tokio::select! {
            r = &mut fut => return Ok(AskOutcome::Reply(r?)),
            _ = crate::shutdown::requested() => return Ok(AskOutcome::Cancelled),
            Some(Ok(ev)) = events.next() => {
```

`src/plain.rs` — first statement of `run_plain_loop` (:168):

```rust
    // From here on SIGTERM / SIGHUP end the loop; main then flushes.
    crate::shutdown::listen();
```

and the loop (:261):

```rust
        tokio::select! {
            _ = crate::shutdown::requested() => break,
            maybe_ev = input_rx.recv() => match maybe_ev {
```

- [ ] **Step 4: Run to verify they pass**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test shutdown::` then `CARGO_TARGET_DIR=$PWD/target cargo test closing::` then `CARGO_TARGET_DIR=$PWD/target cargo test mouse_and_page_keys`
Expected: PASS — the closing pins (`run_saves_after_the_loop_before_returning`) and the handle_nav counts are unchanged.

Also confirm the Windows branch type-checks if the target is installed (skip if `rustup target list --installed` doesn't show it — do not install it): `CARGO_TARGET_DIR=$PWD/target cargo check --target x86_64-pc-windows-msvc`.

- [ ] **Step 5: Gate** (files: `src/shutdown.rs`, `src/main.rs`, `src/tui/run.rs`, `src/tui/ask.rs`, `src/plain.rs`)

Expected: 686 passed, 1 failed (allowed), 1 ignored. shutdown.rs ≈ 70, run.rs ≈ 399, ask.rs ≈ 139, plain.rs ≈ 501, main.rs ≈ 292.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml src/shutdown.rs src/main.rs src/tui/run.rs src/tui/ask.rs src/plain.rs
git status --short Cargo.lock   # if modified: git add Cargo.lock
git commit -m "$(cat <<'EOF'
fix: SIGTERM/SIGHUP and console close save the session

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
EOF
)"
```

---

### Task 11: Slugs keep Latin letters, `+`/`#`, and non-Latin topics (P2-10)

**Files:**
- Modify: `src/topic.rs` (after `deasciify` :127-137, `slugify_topic` :139-191, tests :211-213 + new)
- Modify: `src/cli.rs` (reset arm :46, tests :106-112 + new)

**Interfaces:**
- `pub fn slugify_topic(input: &str) -> String` — signature unchanged. `deasciify(char) -> char` is unchanged (also used by the resume-word check at :115).
- New private: `latin_fold(c: char) -> Option<&'static str>`, `slug_word(word: &str) -> String`, `hash_slug(input: &str) -> String` (`t-` + FNV-1a 32-bit, 8 lowercase hex, of `input.trim().to_lowercase()`).

Callers unchanged: topic.rs:34/:60/:107, visual.rs:111, plain.rs:50/:99/:135, cli.rs:46, tui/topic_flow.rs:59/:215/:232. Every input that previously produced a non-empty slug without `+`/`#`/non-Turkish diacritics produces the same slug (Turkish mapping runs first, untouched).

Deviation from spec: the `t-<hash>` fallback applies only when the input contains a NON-ASCII alphanumeric char (`c.is_alphanumeric() && !c.is_ascii()`), not "any alphanumeric char" — otherwise stopword-only ASCII input (`"learn"`, `"the a"`) would move from `general` to a hash, breaking the "existing slugs unchanged" rule. Also: `+`/`#` are spelled out only once the word has a kept char (`c++` → `cpp`, `#rust` → `rust`, a lone `+` → nothing) — the simplest reading of "inside a word".

- [ ] **Step 1: Write the failing tests**

`src/topic.rs` `mod tests` — replace `slugify_drops_non_alnum_chars` (:211-213) with:

```rust
    #[test]
    fn slugify_spells_out_plus_and_sharp() {
        assert_eq!(slugify_topic("C++"), "cpp");
        assert_eq!(slugify_topic("C#"), "csharp");
        assert_eq!(slugify_topic("F#"), "fsharp");
        assert_eq!(slugify_topic("C"), "c");
        assert_eq!(slugify_topic("#rust"), "rust");
    }

    #[test]
    fn slugify_folds_latin_diacritics() {
        assert_eq!(slugify_topic("straße"), "strasse");
        assert_eq!(slugify_topic("café"), "cafe");
        assert_eq!(slugify_topic("Ñandú"), "nandu");
        assert_eq!(slugify_topic("Œuvre Æsir"), "oeuvre-aesir");
        // Turkish results unchanged.
        assert_eq!(slugify_topic("temel Linux güvenliği"), "temel-linux-guvenligi");
        assert_eq!(slugify_topic("İstanbul Işık"), "istanbul-isik");
    }

    #[test]
    fn slugify_non_latin_topics_get_a_stable_hash_slug() {
        // FNV-1a 32 of the trimmed, lowercased input — pinned values, so a slug
        // naming files on disk never changes between builds.
        assert_eq!(slugify_topic("日本語"), "t-805f5ce7");
        assert_eq!(slugify_topic("Привет мир"), "t-8113ba70");
    }

    #[test]
    fn slugify_symbols_and_ascii_stopwords_only_stay_general() {
        assert_eq!(slugify_topic("!!!"), "general");
        assert_eq!(slugify_topic("the learn"), "general");
    }
```

`src/cli.rs` `mod tests` — `parse_reset_topic_is_slugified` (:106-112) expects `"cpp"`:

```rust
            Command::Reset(ResetTarget::Topic("cpp".to_string()))
```

and add:

```rust
    #[test]
    fn parse_reset_blank_topic_errors() {
        let err = parse_command(&["usta".into(), "reset".into(), "  ".into()]).unwrap_err();
        assert_eq!(err.to_string(), "topic name required");
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test slugify` then `CARGO_TARGET_DIR=$PWD/target cargo test parse_reset`
Expected: FAIL — `slugify_spells_out_plus_and_sharp` (`left: "c"`), `slugify_folds_latin_diacritics` (`left: "strae"`), `slugify_non_latin_topics_get_a_stable_hash_slug` (`left: "general"`), `parse_reset_topic_is_slugified` (`"c"`), `parse_reset_blank_topic_errors` (Ok returned); `slugify_symbols_and_ascii_stopwords_only_stay_general` PASSES (guard).

- [ ] **Step 3: Implement**

`src/topic.rs`, after `deasciify` (:137):

```rust
/// Latin letters beyond Turkish, folded to ascii (review P2-10). `c` is already
/// lowercase.
fn latin_fold(c: char) -> Option<&'static str> {
    Some(match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ą' => "a",
        'æ' => "ae",
        'ć' | 'č' => "c",
        'ď' | 'đ' | 'ð' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' | 'ě' => "e",
        'ì' | 'í' | 'î' | 'ï' | 'ī' => "i",
        'ł' => "l",
        'ñ' | 'ń' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ø' | 'ō' | 'ő' => "o",
        'œ' => "oe",
        'ř' => "r",
        'ś' | 'š' => "s",
        'ß' => "ss",
        'ť' => "t",
        'þ' => "th",
        'ù' | 'ú' | 'û' | 'ū' | 'ů' | 'ű' => "u",
        'ý' | 'ÿ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        _ => return None,
    })
}

/// One word's slug part: the Turkish mapping first (unchanged), then Latin
/// diacritics; `+` → `p` and `#` → `sharp` once the word has started (`c++` →
/// `cpp`, `c#` → `csharp`); anything else is dropped.
fn slug_word(word: &str) -> String {
    let mut out = String::new();
    for c in word.chars().map(deasciify) {
        match c {
            '+' if !out.is_empty() => out.push('p'),
            '#' if !out.is_empty() => out.push_str("sharp"),
            c if c.is_ascii_alphanumeric() => out.push(c),
            c => {
                for l in c.to_lowercase() {
                    if let Some(s) = latin_fold(l) {
                        out.push_str(s);
                    }
                }
            }
        }
    }
    out
}

/// Stable slug for a topic written without Latin letters (日本語, Привет):
/// `t-` + FNV-1a 32-bit of the trimmed, lowercased input. Inline on purpose —
/// `DefaultHasher` may change between Rust versions and the slug names files.
fn hash_slug(input: &str) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for b in input.trim().to_lowercase().bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("t-{h:08x}")
}
```

`slugify_topic` — doc rule line becomes "Rule: fold Turkish and Latin letters to ascii, spell out `+`/`#` inside a word, lowercase, take at most the FIRST 3 content words, join with hyphens. No ascii left but non-Latin letters present → `t-<hash>`; otherwise empty → `tokens::DEFAULT_TOPIC`."; the word mapping and the tail become:

```rust
    let words: Vec<String> = input
        .split_whitespace()
        .map(slug_word)
        .filter(|w| !w.is_empty() && !STOPWORDS.contains(&w.as_str()))
        .take(3)
        .collect();
    if !words.is_empty() {
        return words.join("-");
    }
    // Only non-Latin letters: a stable per-topic slug, so two such topics never
    // share `general`'s progress file. Symbol-only / ascii-stopword-only input
    // stays `general`.
    if input.chars().any(|c| c.is_alphanumeric() && !c.is_ascii()) {
        hash_slug(input)
    } else {
        tokens::DEFAULT_TOPIC.to_string()
    }
}
```

`src/cli.rs:46`:

```rust
            Some(topic) if topic.trim().is_empty() => anyhow::bail!("topic name required"),
            Some(topic) => Ok(Command::Reset(ResetTarget::Topic(slugify_topic(topic)))),
```

- [ ] **Step 4: Run to verify they pass**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test slugify` then `CARGO_TARGET_DIR=$PWD/target cargo test parse_` then `CARGO_TARGET_DIR=$PWD/target cargo test topic::`
Expected: PASS (all — including `finalize_slug_*`, `interpret_topic_input_*`, `slugify_strips_stopwords`, `blank_input_falls_back_to_general`).

- [ ] **Step 5: Gate** (files: `src/topic.rs`, `src/cli.rs`)

Expected: 690 passed, 1 failed (allowed), 1 ignored. topic.rs ≈ 255, cli.rs ≈ 55.

- [ ] **Step 6: Commit**

```bash
git add src/topic.rs src/cli.rs
git commit -m "$(cat <<'EOF'
fix(topic): slugs keep Latin letters, c++/c#, non-Latin topics

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
EOF
)"
```

---

### Task 12: Safe PDF conversion into `materials/.converted/` (P2-11)

**Files:**
- Modify: `src/materials.rs` (imports :6; `scan` :70-91; `convert_pdfs` :139-203 → `convert_pdfs` + `convert_pdfs_with` + `convert_one` + helpers; tests :329-345 + new)

**Interfaces:**
- `pub fn convert_pdfs(project_root: &Path) -> Vec<String>` — signature unchanged; calls `convert_pdfs_with(project_root, "pdftotext", PDF_TIMEOUT)`. Call sites unchanged: tui/entry.rs:175, tui/intro.rs:245, plain.rs:217.
- New private: `const PDF_TIMEOUT: Duration = 30 s`, `fn convert_pdfs_with(project_root: &Path, tool: &str, timeout: Duration) -> Vec<String>`, `fn convert_one(tool: &str, pdf: &Path, txt: &Path, timeout: Duration) -> String`, `fn converted_path(materials: &Path, pdf: &Path) -> Option<PathBuf>`, `fn is_pdf(p: &Path) -> bool`, `fn failed_note(pdf: &Path, why: &str) -> String`.
- `pub fn scan(project_root: &Path) -> Vec<Material>` — also reads `materials/.converted/**.txt`, named after the PDF (`alt/x.pdf`); blank conversions and ones shadowed by a sibling `<stem>.txt` are skipped; result sorted by name.

Deviation from spec: one injection point instead of two — `convert_pdfs_with(root, tool, timeout)` (the spec's `convert_pdfs_with(root, tool)` + `convert_pdfs_inner(root, tool, timeout)` collapse into one private fn; tests call it directly). Output mirrors the PDF's path below `materials/` (`materials/alt/x.pdf` → `materials/.converted/alt/x.txt`) so two PDFs with the same stem in different folders can't collide.

Behavior: a sibling `<stem>.txt` (hand-written, or written by older usta) → the PDF is skipped, never overwritten. Fresh conversion (converted mtime ≥ pdf mtime) → skipped. Tool missing → existing note. Per file: `spawn` + `try_wait` poll every 50 ms; after the timeout `kill` + `wait`, note `pdftotext timed out on <pdf>`. Failure or timeout → partial output removed. Success with blank output → note `no text found in <pdf> (scanned PDF?)`, the blank file stays as the freshness marker (reported once). Extension match is case-insensitive. The hidden `.converted/` dir is skipped by the watcher (`is_ignored`: leading `.`) and by `collect_files`, so usta's own output is never fed back. Bounded blocking (≤ 30 s per PDF) stays on the calling thread — `block_in_place` would panic under current-thread test runtimes (spec).

- [ ] **Step 1: Write the failing tests** — `src/materials.rs` `mod tests`. Replace `convert_pdfs_missing_tool_reports_notice_and_no_txt` (:328-345) with:

```rust
    #[test]
    fn convert_pdfs_missing_tool_reports_notice_and_no_txt() {
        let base = std::env::temp_dir().join(format!(
            "usta_materials_pdftotext_missing_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("materials");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("kitap.pdf"), "%PDF").unwrap();

        let notes = convert_pdfs_with(&base, "usta-no-such-pdftotext", PDF_TIMEOUT);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("pdftotext missing"));
        assert!(!dir.join("kitap.txt").exists());
        assert!(!dir.join(".converted").exists());

        let _ = std::fs::remove_dir_all(&base);
    }
```

Add after it:

```rust
    fn pdf_project(tag: &str) -> PathBuf {
        let base =
            std::env::temp_dir().join(format!("usta_materials_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("materials")).unwrap();
        base
    }

    /// A stand-in for pdftotext: answers `-v`, otherwise runs `body` with
    /// `$1=-layout $2=<pdf> $3=<out>`. Lives outside materials/.
    #[cfg(unix)]
    fn fake_tool(base: &Path, body: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let p = base.join("fake-pdftotext");
        std::fs::write(&p, format!("#!/bin/sh\n[ \"$1\" = \"-v\" ] && exit 0\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn convert_pdfs_never_touches_a_handwritten_sibling_txt() {
        let base = pdf_project("pdf_sibling");
        let dir = base.join("materials");
        std::fs::write(dir.join("kitap.txt"), "my own notes").unwrap();
        std::fs::write(dir.join("kitap.pdf"), "%PDF").unwrap();
        let notes = convert_pdfs_with(&base, "usta-no-such-pdftotext", PDF_TIMEOUT);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(std::fs::read_to_string(dir.join("kitap.txt")).unwrap(), "my own notes");
        assert!(!dir.join(".converted").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn convert_pdfs_times_out_and_leaves_no_partial_output() {
        let base = pdf_project("pdf_timeout");
        let dir = base.join("materials");
        std::fs::write(dir.join("kitap.pdf"), "%PDF").unwrap();
        let tool = fake_tool(&base, "printf partial > \"$3\"\nsleep 5");
        let notes = convert_pdfs_with(&base, &tool, Duration::from_secs(1));
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("timed out"), "{notes:?}");
        assert!(!dir.join(".converted/kitap.txt").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn convert_pdfs_notes_blank_output_once_as_scanned() {
        let base = pdf_project("pdf_blank");
        let dir = base.join("materials");
        std::fs::write(dir.join("kitap.pdf"), "%PDF").unwrap();
        let tool = fake_tool(&base, "printf '\\f\\n' > \"$3\"");
        let notes = convert_pdfs_with(&base, &tool, PDF_TIMEOUT);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("no text found") && notes[0].contains("scanned PDF?"), "{notes:?}");
        assert!(dir.join(".converted/kitap.txt").exists(), "freshness marker stays");
        assert!(convert_pdfs_with(&base, &tool, PDF_TIMEOUT).is_empty(), "reported once");
        assert!(scan(&base).is_empty(), "a blank conversion adds no material");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn convert_pdfs_matches_extension_case_insensitively_and_scan_reads_it() {
        let base = pdf_project("pdf_case");
        let dir = base.join("materials");
        std::fs::write(dir.join("KITAP.PDF"), "%PDF").unwrap();
        let tool = fake_tool(&base, "printf 'hello world\\n' > \"$3\"");
        let notes = convert_pdfs_with(&base, &tool, PDF_TIMEOUT);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].starts_with("converted:"), "{notes:?}");
        assert!(dir.join(".converted/KITAP.txt").is_file());
        assert!(!dir.join("KITAP.txt").exists(), "nothing written next to the PDF");
        let mats = scan(&base);
        assert_eq!(mats.len(), 1);
        assert_eq!(mats[0].name, "KITAP.pdf");
        assert!(mats[0].digest.contains("hello world"));
        assert!(convert_pdfs_with(&base, &tool, PDF_TIMEOUT).is_empty(), "fresh → skipped");
        let _ = std::fs::remove_dir_all(&base);
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test convert_pdfs`
Expected: compile errors — `cannot find function convert_pdfs_with`, `cannot find value PDF_TIMEOUT`, `cannot find type Duration`.

- [ ] **Step 3: Implement**

Imports (:6):

```rust
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
```

`scan` (:67-91) becomes:

```rust
/// Recursively collect .md/.txt under `materials/`, digest each, plus the PDF
/// conversions in `materials/.converted/` (named after their PDF). A .pdf with a
/// sibling .txt is represented by the .txt alone (no double counting); a blank
/// conversion (scanned PDF) adds nothing. Deterministic: sorted by name.
pub fn scan(project_root: &Path) -> Vec<Material> {
    let root = project_root.join("materials");
    let mut files: Vec<PathBuf> = Vec::new();
    collect_files(&root, &mut files);
    let mut mats: Vec<Material> = files
        .iter()
        .filter_map(|p| {
            let name = p.strip_prefix(&root).ok()?.to_string_lossy().to_string();
            let ext = p.extension()?.to_str()?;
            let content = std::fs::read_to_string(p).ok()?;
            let digest = match ext {
                "md" => digest_md(&content, PER_FILE_CAP),
                "txt" => digest_txt(&content, PER_FILE_CAP),
                _ => return None,
            };
            Some(Material { name, digest })
        })
        .collect();
    let conv = root.join(".converted");
    let mut converted: Vec<PathBuf> = Vec::new();
    collect_files(&conv, &mut converted);
    for p in converted {
        let Ok(rel) = p.strip_prefix(&conv) else { continue };
        if root.join(rel).exists() {
            continue; // a sibling .txt now speaks for this PDF
        }
        let Ok(content) = std::fs::read_to_string(&p) else { continue };
        if content.trim().is_empty() {
            continue;
        }
        mats.push(Material {
            name: rel.with_extension("pdf").to_string_lossy().to_string(),
            digest: digest_txt(&content, PER_FILE_CAP),
        });
    }
    mats.sort_by(|a, b| a.name.cmp(&b.name));
    mats
}
```

Replace `convert_pdfs` (:137-203) with:

```rust
/// Per-PDF time cap: a huge or hung PDF can't freeze the session.
const PDF_TIMEOUT: Duration = Duration::from_secs(30);

/// Convert each materials/**/*.pdf (any case) to text in `materials/.converted/`
/// via pdftotext when available. Returns user-facing notice lines; never fails
/// the session.
pub fn convert_pdfs(project_root: &Path) -> Vec<String> {
    convert_pdfs_with(project_root, "pdftotext", PDF_TIMEOUT)
}

/// `convert_pdfs` with the tool and time cap injectable (tests: a missing or fake tool).
fn convert_pdfs_with(project_root: &Path, tool: &str, timeout: Duration) -> Vec<String> {
    let root = project_root.join("materials");
    let mut files: Vec<PathBuf> = Vec::new();
    collect_files(&root, &mut files);
    let pdfs: Vec<&PathBuf> = files.iter().filter(|p| is_pdf(p)).collect();
    if pdfs.is_empty() {
        return Vec::new();
    }
    let have_tool = std::process::Command::new(tool).arg("-v").output().is_ok();
    let mut notes = Vec::new();
    for pdf in pdfs {
        if pdf.with_extension("txt").exists() {
            continue; // a hand-written (or older usta's) text wins — never overwritten
        }
        let Some(txt) = converted_path(&root, pdf) else {
            continue;
        };
        let fresh = match (std::fs::metadata(&txt), std::fs::metadata(pdf)) {
            (Ok(t), Ok(p)) => t.modified().ok() >= p.modified().ok(),
            _ => false,
        };
        if fresh {
            continue; // cached conversion is current
        }
        if !have_tool {
            notes.push(format!(
                "PDF found but pdftotext missing — convert {} to text yourself, or `brew install poppler`",
                pdf.display()
            ));
            continue;
        }
        notes.push(convert_one(tool, pdf, &txt, timeout));
    }
    notes
}

/// Run the tool on one PDF under a time cap. On failure or timeout no partial
/// output is left (it would read as a fresh conversion next time).
fn convert_one(tool: &str, pdf: &Path, txt: &Path, timeout: Duration) -> String {
    if let Some(dir) = txt.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let spawned = std::process::Command::new(tool)
        .arg("-layout")
        .arg(pdf)
        .arg(txt)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => return failed_note(pdf, &format!(" ({e})")),
    };
    let start = Instant::now();
    let exited = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(50)),
            _ => break None,
        }
    };
    let Some(exit) = exited else {
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_file(txt);
        return format!("pdftotext timed out on {}", pdf.display());
    };
    if !exit.success() {
        let mut err = String::new();
        if let Some(mut s) = child.stderr.take() {
            let _ = s.read_to_string(&mut err);
        }
        let _ = std::fs::remove_file(txt);
        let why = err
            .lines()
            .next()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| format!(" ({l})"))
            .unwrap_or_default();
        return failed_note(pdf, &why);
    }
    match std::fs::read_to_string(txt) {
        Ok(t) if t.trim().is_empty() => {
            format!("no text found in {} (scanned PDF?)", pdf.display())
        }
        _ => format!("converted: {} → {}", pdf.display(), txt.display()),
    }
}

fn failed_note(pdf: &Path, why: &str) -> String {
    format!(
        "pdftotext failed on {}{why} — convert it to text yourself",
        pdf.display()
    )
}

/// `materials/<rel>.pdf` → `materials/.converted/<rel>.txt` (hidden: the watcher
/// and `collect_files` skip it, so usta's own output is never fed back).
fn converted_path(materials: &Path, pdf: &Path) -> Option<PathBuf> {
    let rel = pdf.strip_prefix(materials).ok()?;
    Some(materials.join(".converted").join(rel).with_extension("txt"))
}

fn is_pdf(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}
```

The pin `convert_pdfs_captures_pdftotext_output` still holds: everything after `pub fn convert_pdfs(` contains `Stdio::null()` and `Stdio::piped()` and no `.status()` call.

- [ ] **Step 4: Run to verify they pass**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test materials::`
Expected: PASS — all materials tests, including the previously failing `convert_pdfs_missing_tool_reports_notice_and_no_txt`, `scan_skips_pdf_when_sibling_txt_exists`, `scan_finds_md_txt_skips_hidden_sorts`.

- [ ] **Step 5: Gate** (files: `src/materials.rs`)

Expected: **695 passed, 0 failed**, 1 ignored (+4 new; the allowed failure now passes). materials.rs ≈ 290.

- [ ] **Step 6: Commit**

```bash
git add src/materials.rs
git commit -m "$(cat <<'EOF'
fix(materials): PDF text into .converted/, timeout, scanned-PDF note

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
EOF
)"
```

---

### Task 13: Kill `cargo check` on timeout (P2-12)

**Files:**
- Modify: `src/check.rs` (imports :7-10, `run_check` :38-53, tests)

**Interfaces:**
- `pub async fn run_check(root: &Path) -> Option<String>` — unchanged signature; callers file_feedback.rs (three `.await` sites) unchanged.

`kill_on_drop(true)` kills `cargo` when the timed-out `output()` future is dropped (rustc children exit when cargo's jobserver pipes close); `stdin(Stdio::null())` keeps it off the TUI's tty. A behavioural test would need a slow cargo, so this is a source pin (spec).

- [ ] **Step 1: Write the failing test** — `src/check.rs` `mod tests`:

```rust
    /// Source pin: on timeout the `output()` future is dropped — cargo must die
    /// with it, or it keeps running and holds the build lock.
    #[test]
    fn run_check_kills_cargo_when_the_timeout_drops_it() {
        let prod = include_str!("check.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let body = prod
            .split("pub async fn run_check(")
            .nth(1)
            .expect("run_check")
            .split("\n}\n")
            .next()
            .unwrap();
        assert!(body.contains(".kill_on_drop(true)"));
        assert!(body.contains(".stdin(Stdio::null())"));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test run_check_kills`
Expected: FAIL at `assert!(body.contains(".kill_on_drop(true)"))`.

- [ ] **Step 3: Implement**

Imports: add `use std::process::Stdio;` after `use std::path::Path;`. In `run_check`:

```rust
    let fut = Command::new("cargo")
        .arg("check")
        .arg("--message-format=short")
        .current_dir(root)
        .stdin(Stdio::null())
        // On timeout this future is dropped: kill cargo with it, or it keeps
        // running and holds the build lock for every later check.
        .kill_on_drop(true)
        .output();
```

- [ ] **Step 4: Run to verify it passes**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test run_check_kills`
Expected: PASS.

- [ ] **Step 5: Gate** (files: `src/check.rs`)

Expected: 696 passed, 0 failed, 1 ignored. check.rs ≈ 157.

- [ ] **Step 6: Commit**

```bash
git add src/check.rs
git commit -m "$(cat <<'EOF'
fix(check): kill cargo check when its timeout fires

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
EOF
)"
```

---

### Task 14: Release v0.39.0

**Files:**
- Modify: `Cargo.toml` (`version = "0.38.0"` → `"0.39.0"`)
- Modify: `Cargo.lock` (the `name = "usta"` package entry, version → `0.39.0`)
- Modify: `src/tui/welcome_tests.rs:8` (version pin)
- Modify: `SPEC.md` (new paragraph after the `**v0.38.0 (review fixes P1 — no silent data loss):**` paragraph, line 260; plus the now-stale lines 74 (`usta reset <topic>`), 107 (topic lock) and 156 (pdftotext))
- Append (git-ignored, not committed): `.superpowers/sdd/review-fixes-manual-checklist.md` — add a `## P2 (v0.39.0)` section at the END; do not overwrite the existing P1 content

Deviation from spec: README is not touched — it carries no version string, and its statements stay true (line 35 "PDFs auto-convert if `pdftotext` is installed", line 69 `usta reset rust`, line 134 "No TTY … falls back to the plain line mode"). SPEC gets three one-line body corrections besides the version paragraph, because P2 makes those lines wrong.

- [ ] **Step 1: Write the failing test** — `src/tui/welcome_tests.rs:8`:

```rust
    assert_eq!(env!("CARGO_PKG_VERSION"), "0.39.0");
```

- [ ] **Step 2: Run to verify it fails**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test version_aligned_with_spec`
Expected: FAIL — `left: "0.38.0"`, `right: "0.39.0"`.

- [ ] **Step 3: Bump**

`Cargo.toml`: `version = "0.39.0"`. Then `CARGO_TARGET_DIR=$PWD/target cargo build` (updates the `usta` entry in `Cargo.lock`; confirm with `git diff Cargo.lock` that only that one line changed).

`SPEC.md` line 74 — replace with:

```markdown
- **`usta reset <topic>`** — every per-topic artifact of the current project is listed and, after a `[y/N]` confirmation (`e/evet` silently accepted), removed: progress, approach, curriculum (and their `.bak`), generated visuals, session transcripts (exact topic match), plus the topic's catalog row — the row goes even when no progress file exists. Not touched: the topic lock, `learner/history.md`, `mentor/`. `usta reset ""` is an error (`topic name required`). The "let me relearn this topic" scenario.
```

`SPEC.md` line 107 — replace with:

```markdown
- **Topic lock:** `.usta/.lock-<topic>` (content: pid) — a concurrent second session is opened with confirmation, progress isn't silently overwritten. In pipe mode: warn + continue. The lock is held by a guard and removed on every exit path; startup recovery skips a transcript whose topic lock belongs to another running usta (pid alive), while a lock left by a crashed run does not block recovery.
```

`SPEC.md` line 156 — replace with:

```markdown
- **pdftotext optional:** if `pdftotext` is on PATH, PDF files (any extension case) are auto-converted to text in the hidden `materials/.converted/` (never next to the PDF; a sibling `<stem>.txt` wins and is never overwritten), 30 s per file, partial output removed on failure; a PDF without a text layer gets a one-time `no text found … (scanned PDF?)` note. Otherwise the PDF is skipped, a single info line is printed (with a `brew install poppler` suggestion) — no hard error, the flow continues.
```

`SPEC.md` — insert after the v0.38.0 paragraph (line 260, one blank line before and after):

```markdown
**v0.39.0 (review fixes P2 — functional bugs):** key-release events are ignored (Windows no longer handles every key twice); the TUI opens only when stdin is a terminal too (`echo hi | usta` takes the plain path, which reads the pipe). `usta reset <topic>` lives in `reset.rs` and removes every per-topic artifact (progress, approach, curriculum and their `.bak`, visuals, transcripts) plus the catalog row, even when no progress file exists; a blank topic is an error. The watcher ignores `*.bak`, and every interim compaction re-seeds the mentor-doc baseline, so usta's own `mentor/` writes are not fed back as user changes. A torn last transcript line is skipped with a warning instead of blocking recovery. A CLI json result with `is_error: true` is an error carrying its text (exit 0 or not), and the full-transcript resume retry runs only when the CLI session is gone (`No conversation found`). Recovery skips a transcript a running usta owns (lock pid alive via `kill -0` / `tasklist`); the topic lock is a guard removed on every exit path; the plain path flushes even when its loop fails; SIGTERM/SIGHUP (Windows: console close/shutdown) end the session through the normal save (tokio `signal`). Slugs fold Latin diacritics (`straße` → `strasse`), spell out `c++`/`c#` (`cpp`/`csharp`), and give non-Latin topics a stable `t-<fnv1a>` slug instead of `general`. PDFs convert into `materials/.converted/` with a 30 s timeout and a scanned-PDF note; `cargo check` is killed when its timeout fires. Design: `docs/superpowers/specs/2026-10-26-review-fixes-design.md` (P2).
```

Append to `.superpowers/sdd/review-fixes-manual-checklist.md` (git-ignored; append, do not `git add`, do not overwrite):

```bash
cat >> .superpowers/sdd/review-fixes-manual-checklist.md <<'EOF'

## P2 (v0.39.0)
- [ ] Windows key handling: in Windows Terminal, type `hello`, press PageUp once, Enter once → each key acts exactly once (no doubled letters, one page of scroll, one submit). Also inside the yes/no lock confirm and while a reply is pending (Esc cancels once).
- [ ] SIGTERM save: `HOME=$(mktemp -d) usta start rust` in a scratch project, exchange one turn, then from another shell `kill -TERM <pid>` → the TUI shows the saving status, exits, `.usta/learner/progress/rust.md` is written, `.usta/.lock-rust` is gone, the transcript is `.done.jsonl`.
- [ ] SIGHUP save: same setup, close the terminal window/tab instead → progress written, lock gone (check from another terminal).
- [ ] Windows console close: close the console window during a session → the save runs within Windows' close grace period (a long closing flush may be cut — note the result).
- [ ] `echo hi | usta` in a scratch project: plain mode, topic `general`, the pipe's `hi` is the first turn, exits on EOF after the closing flush; no TUI, no hang.
- [ ] PDF timeout with a real slow file: a very large or pathological PDF in `materials/` (or temporarily set `PDF_TIMEOUT` to 1 s) → `pdftotext timed out on …`, no `materials/.converted/<name>.txt` left, the session continues; a scanned (image-only) PDF → `no text found … (scanned PDF?)` once.
- [ ] Second usta on the same project: start a session on `rust`, then start `usta` in a second terminal → `skipping rust: a running usta owns this session`, the first session's transcript is untouched.
EOF
```

- [ ] **Step 4: Run to verify it passes**

Run: `CARGO_TARGET_DIR=$PWD/target cargo test version_aligned_with_spec`
Expected: PASS.

- [ ] **Step 5: Gate** (files: `src/tui/welcome_tests.rs` — test file, no budget)

Expected: 696 passed, 0 failed, 1 ignored.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/tui/welcome_tests.rs SPEC.md
git commit -m "$(cat <<'EOF'
chore: v0.39.0 (review fixes P2)

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01U5GYfPrZwhasqfjqDeegZr
EOF
)"
```

---

## Notes for the executor

- Test counts assume the tests exactly as written here; if you add or split one, adjust the expected count. Until Task 12 the only failure must be `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt`; from Task 12 on there are none.
- `cfg(unix)` tests (lock, shutdown, materials fake tool) all run on the macOS dev machine. On Linux a freshly written shell script can hit `ETXTBSY` ("Text file busy") if another test thread forks while it is open; macOS has no such race. If it ever shows up, retry the spawn once rather than weakening the test.
- `kill -0` / `tasklist` are spawned processes: `pid_alive` is called only at startup recovery (once per unfinished record) and in tests — never in a loop.
- `tui/run.rs` and `plain.rs` lines inside `tokio::select!` are not reformatted by rustfmt; keep edited arm lines on one line like their neighbors so the source pins (which search for exact substrings such as `if crate::lifecycle::maybe_compact(`) match.
- The Windows `signals()` and `pid_alive` branches can't be tested here; if `rustup target list --installed` shows a Windows target, run `cargo check --target <it>` once after Task 10 (with the `CARGO_TARGET_DIR` prefix). Don't install a target just for this.
