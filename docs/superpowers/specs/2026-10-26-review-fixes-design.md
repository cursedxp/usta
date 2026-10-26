# Review fixes (P1 + P2). Design

Date: 2026-10-26 · Status: approved scope (user: fix all P1, then all P2; P3 out of scope) · Base: `main` @ v0.37.1
Source: `.superpowers/sdd/review-2026-10-26.md`. Verification notes (local, git-ignored): `.superpowers/sdd/review-fixes/verify-{A,B,C,D}.md`.

Every item below was re-verified against v0.37.1 by reading the code; all 18 are confirmed (one sub-claim of P2-11 is refuted, noted there). None is dropped. Each item: root cause, chosen fix with one line of why, test.

Delivery: two plans. P1 ships as v0.38.0, P2 as v0.39.0 (P2 plan written against main after P1 merged).

## Global constraints
- 600-line production budget per file (lines before the first `#[cfg(test)]`, SPEC §11). `setup.rs` is already 611 → it must shrink when touched (P2-3 moves reset code out).
- `cargo fmt` scoped to edited files; `cargo fmt --check` crate-wide clean; `cargo clippy --all-targets -- -D warnings` clean.
- `cargo test`: the only allowed failure is `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt` (pdftotext installed on the dev machine). P2-11 makes the tool injectable, after which that test must pass too.
- No emoji in any new UI text. Notices are short, lowercase, English (matching existing notices).
- Surgical: no refactors beyond what an item needs. Source-pin tests (closing.rs:98-165, polite.rs:696) must stay green; update a pin only when the pinned line itself is the one being changed.
- No new crates except enabling tokio's `signal` feature (P2-9).

---

# P1 — data loss / session permanently broken

## P1-1 + P1-2: validate the closing / compaction reply before any write

**Cause.** `flush_core` (lifecycle.rs:102-223) does `ask_usta` → `progress::split_files(&reply.text)` (progress.rs:56) → bails only if the list is empty (:151) → `write_atomic` per file (:179). Three holes:
1. `anthropic::Client::complete` (anthropic.rs:225-264) checks only `pause_turn` (:255); `stop_reason == "max_tokens"` is dropped. `backend::Reply { text, web, context_tokens }` (backend.rs:22) has no truncation field. `MAX_TOKENS = 8000` (anthropic.rs:14) is shared by normal turns, closing and compaction, and adaptive thinking eats into it.
2. `split_files` accepts a last file cut off mid-body.
3. With no `===FILE:` delimiter, `split_files` returns `("progress", clean_markdown_reply(reply))` (progress.rs:57-59): any conversational reply/apology overwrites progress.md. Empty-bodied files are silently skipped (:158-181).
Same path runs for `maybe_compact` (lifecycle.rs:245-272), which on any Ok drops history permanently.

On Err, all callers already do the right thing: main.rs:265-270 / closing.rs warn and leave the transcript unfinished (recovery offered next start); `maybe_compact` aborts and keeps history. So the fix is purely: **return Err before the first write** when the reply is not trustworthy.

**Design.**
- `backend::Reply` gains `pub truncated: bool`. API backend: `Client::complete` returns it as `stop_reason.as_deref() == Some("max_tokens")` (extend its return tuple or return a small struct; `backend.rs:133` is the only consumer). CLI backend: always `false` here — a CLI output-limit hit already surfaces as exit 1 → Err (verified with `CLAUDE_CODE_MAX_OUTPUT_TOKENS=30`: `is_error:true`, exit 1); P2-6 additionally turns exit-0 `is_error` into Err.
- New pure fn in progress.rs: `pub fn parse_closing_reply(text: &str, truncated: bool) -> anyhow::Result<Vec<(String, String)>>`. Rejects (Err with a short reason) when: `truncated`; no `===FILE:` delimiter at all; zero files; any delimited file whose body is empty/whitespace after cleaning. Otherwise returns the same pairs `split_files` would.
- `split_files` drops the no-delimiter fallback (returns an empty vec). Test progress_tests.rs:95 (`split_files_without_delimiter_is_progress`) is rewritten to assert the new behaviour.
- `flush_core` calls `parse_closing_reply(&reply.text, reply.truncated)?` instead of `split_files` + the empty check. The error message names the reason, e.g. `progress not saved: reply was cut off (max tokens) — session kept for recovery`.
- Closing/compaction get a larger output budget: `CLOSING_MAX_TOKENS = 32000` in anthropic.rs. Plumb a `max_tokens: u32` through `MessageRequest::new` / `Client::complete`; `Backend` gets a way to request it (e.g. `Backend::complete_with_limit(system, history, max_tokens)`, with `complete` delegating with `MAX_TOKENS`). `ask_usta` gains nothing; `flush_core` uses the limit-taking variant (CLI ignores the limit). Why: the files are long and thinking shares the budget; 8000 is what truncates today.
- "Obviously cut" heuristics beyond the above (sentinels, length ratios) are **not** added: a sentinel the model forgets would block every save. Truncation is caught by the authoritative `stop_reason`.

**Why:** one validation gate before writes covers closing, recovery and compaction, and reuses the existing keep-transcript-on-Err behaviour.

**Tests.** `parse_closing_reply` unit tests: truncated → Err; no delimiter → Err; empty body → Err; valid two-file reply → Ok with both. `split_files` no-delimiter → empty. A `Client` response-parse test that `stop_reason: "max_tokens"` yields `truncated == true` (pure helper on `MessageResponse`). `MessageRequest::new(..., 32000)` serializes `max_tokens: 32000`.

## P1-3: keep the last-known-good backup

**Cause.** `progress::write_atomic` (progress.rs:505-518) copies the current file to `<name>.md.bak` before every write, overwriting the previous `.bak`. Compaction checkpoints plus the closing flush write the same files several times per session, so two bad writes lose the last good copy.

**Design.** Back up each path at most once per process: a `static BACKED_UP: Mutex<HashSet<PathBuf>>` in progress.rs; `write_atomic` copies to `.bak` only if the path is not yet in the set (then inserts it). The `.bak` therefore holds the file as it was before this session's first write — the last state a previous session left. Applies to every `write_atomic` caller (slash, history, index, setup), which is fine: same rule, same meaning.

**Why:** simplest rule that guarantees a bad write later in the same session can never destroy the pre-session copy. **Rejected:** `.bak/.bak2` rotation — three bad writes still lose it.

**Tests.** progress_tests.rs:401 updated: write A (original exists) → write B → write C: `.bak` still holds the original. Unique temp paths per test keep the static set from interfering.

## P1-4: watcher ignore rules on the path relative to the project root

**Cause.** `watcher::is_ignored(path)` (watcher.rs:68) checks every component of the absolute path. Callers pass absolute paths: `should_forward` (watcher.rs:46 → :109, notify events) and `StructureTracker::walk` (:147, used by `seed`). A project under `~/.work/`, `~/secrets-lab/`, `.../target/...` gets no feedback and an empty structure.

**Design.** Keep `is_ignored` pure (existing tests stay). Callers strip the root first: `spawn` canonicalizes `root` once (fallback: the raw root) and passes it to `should_forward(root, path, kind)`; `walk`/`seed` likewise. Relative path = `path.strip_prefix(canon_root).or_else(|_| path.strip_prefix(raw_root)).unwrap_or(path)` (macOS FSEvents may report `/private/var/...` realpaths).

**Tests.** Root `<tmp>/.h/secrets/target/proj`: `src/main.rs` forwarded, `proj/.git/HEAD` and `proj/target/x` still ignored; `StructureTracker::seed` on such a root records `src`.

## P1-5: never push an empty assistant reply

**Cause.** API `extract_text` (anthropic.rs:176, returned at :264) yields `""` for thinking-only/max-token replies; CLI `parse_cli_output` (backend.rs:196) `unwrap_or_default()`. Eight `push_assistant` call sites (tui/run.rs:186,:348; plain.rs:208,:241,:342; file_feedback.rs:228,:374; intro stitch) push it unchecked; an empty assistant message makes every later API call 400.

**Design.** Single choke point: `Backend::complete` returns `Err(anyhow!("empty reply from the model — nothing was added to the conversation"))` when `reply.text.trim().is_empty()`, via a small pure helper `ensure_nonempty(reply) -> Result<Reply>` so it is unit-testable. Every caller already shows Err as a notice; with P1-6's rollback the user turn is removed too, so the next turn works. Closing flush also gets Err (transcript kept), which is correct.

**Why:** one guard at the backend covers all eight call sites and both backends. **Rejected:** a placeholder assistant text — it would put a fake mentor reply into history.

**Tests.** `ensure_nonempty` with `""`, `"  \n"` → Err; `"hi"` → Ok.

## P1-6: corrupt images and failed turns never poison history

**Cause.** (a) `image_attach::prepare` (image_attach.rs:49-68) only reads the header (`into_dimensions`); an image within limits passes through undecoded, so a truncated/corrupt file reaches the API. (b) On any API error the user turn (with images) stays in history: `Session` has no rollback; push → Err sites: tui/run.rs:333→:353 and the opening turn ~:167; plain.rs:196/:229/:334→:346; file_feedback.rs:220→:222, :366→:368. Every later turn re-sends the bad turn and fails. CLI backend resume retry re-sends it too.

**Design.**
- `prepare` fully decodes on the pass-through path (`image::load_from_memory_with_format(bytes, fmt)`), still returning the original bytes on success; decode failure → Err (`image could not be read: …`), which the existing drop path already shows as a notice (`failed_drop_inserts_the_path_and_leaves_a_notice`).
- `Session::truncate_history(len: usize)` (no-op if `len >= history.len()`). Each push→ask site records `let mark = session.history().len();` before pushing and calls `session.truncate_history(mark)` in its Err arm, before showing the notice. The deliberate Cancelled arm in run.rs (spec B2) is unchanged.
- Transcript: the recorder already appended the user text. Add `Recorder::rollback()` that appends a rollback marker line; `transcript::read_history` drops the most recent user message when it meets the marker. So recovery sees the same history the live session had. (Recorder records text only; images are never in transcripts.)

**Why:** reject bad input at the door and make every failed turn leave history exactly as before the turn — covers images, network errors, 400s and empty replies alike.

**Tests.** Truncated PNG (`png_bytes()` with its tail cut) → `prepare` Err. `truncate_history` unit test. A run-path test is not feasible without a backend mock; instead a focused test per helper plus a source-level check is acceptable only where the codebase already uses source pins — prefer extracting the push/ask/rollback pattern into a tiny tested helper if it stays simple. `read_history` with user, assistant, user, rollback → two messages.

---

# P2 — clear functional bugs

## P2-1: Windows key events handled twice
**Cause.** No `KeyEventKind` filter anywhere; Windows reports Press and Release. Key matches: run.rs:237, ask.rs:62 and :114, entry.rs:287, intro.rs:102, page.rs:109 (`nav`). All loops call `page::handle_nav` first.
**Design.** In `page::nav`, a first arm `Event::Key(k) if k.kind == KeyEventKind::Release => true` (consumed, ignored). Repeat stays (held keys). Verify every loop (including the ask.rs:114 confirm loop) routes through `handle_nav` before its own key match; any that does not gets the same one-line guard. Defence in depth: `InputBox::handle_key` (editor.rs:94) returns `Action::None` on Release.
**Why:** one guard where all loops already pass. **Test.** `nav` consumes a Release key and changes nothing; `handle_key` ignores a Release char.

## P2-2: `echo hi | usta` enters the TUI and hangs
**Cause.** main.rs:190 gates the TUI on `!ui::is_plain()`, which checks stdout only (ui.rs:74). `is_plain()` is also an output-format gate (ui.rs:80-179), so its meaning stays.
**Design.** `ui::use_tui() -> bool` = stdin is a tty && `!is_plain()`. main.rs:190 uses it. Update the closing.rs:161 source pin that splits main.rs on `"if !ui::is_plain() {"`.
**Test.** Pure helper `use_tui_from(stdin_tty, plain)` truth table; pin updated.

## P2-3: `usta reset <topic>` leaves artifacts
**Cause.** setup.rs:206-232 deletes progress only (+visuals), returning early when progress is missing, so catalog cleanup never runs; approach (`.usta/approaches/<t>.md`), curriculum (`progress::curriculum_path`), transcripts (`.usta/sessions/<t>-<date>-<time>[.done].jsonl`) remain.
**Design.** New `src/reset.rs`: move `run_reset_topic` and `remove_topic_visuals` there (setup.rs drops under 600). `topic_artifacts(root, topic) -> Vec<PathBuf>` lists existing progress, approach, curriculum, visuals dir, and transcripts whose `transcript::topic_from_record` (after stripping a `.done` suffix from the stem) equals the topic exactly (no prefix match: `rust` ≠ `rust-async`). The confirm prompt lists what will be removed; removal is idempotent; catalog row (`index::remove`) always removed. Nothing found and no catalog row → `nothing to reset for <topic>`. Not touched: the lock file (a live session may own it), `learner/history.md` (global stats), `mentor/` (pinned by setup_tests.rs:77).
**Test.** Temp project with all artifacts for `rust` and `rust-async` → reset `rust` removes exactly rust's; progress missing still removes curriculum + catalog row. Moved tests move with the code.

## P2-4: usta's own writes trigger feedback
**Cause.** Interim compaction (`maybe_compact`, called at run.rs:349, plain.rs:343) writes `mentor/PROJECT.md`, `mentor/PROGRESS.md` and their `.md.bak` inside the watched project; the watcher baseline (`seed_mentor_baseline`, run.rs:104) is never refreshed and `.bak` is not ignored → mentor feedback on usta's own output.
**Design.** (a) `watcher::is_ignored` also ignores names ending in `.bak`. (b) `maybe_compact` returns `bool` (compacted); run.rs and plain.rs re-run `seed_mentor_baseline` when it is true, so queued events for those files observe as unchanged. PROJECT.md / PROGRESS.md themselves stay watched (users edit them).
**Test.** `is_ignored("mentor/PROJECT.md.bak")`; after re-seeding, observing the same content yields `Skip`.

## P2-5: torn last transcript line kills recovery
**Cause.** `transcript::read_history` (transcript.rs:73-98) uses `read_to_string` (invalid UTF-8 from a torn multibyte char fails all) and `?` per line.
**Design.** Read bytes, `String::from_utf8_lossy`; an unparseable **last** non-empty line is skipped with a `ui::warn` (`last session line was incomplete — skipped`); an unparseable middle line is still an Err.
**Test.** Torn tail (incl. a cut multibyte char) → earlier messages returned; bad middle line → Err.

## P2-6: CLI json-mode error text discarded
**Cause.** `parse_cli_output` (backend.rs:189-201) ignores `is_error`; an exit-0 error result is shown as the mentor's reply. On non-zero exit in json mode the stdout `result` is dropped (`stream_failure` :265 is stream-only).
**Design.** `parse_cli_output` returns `Result`: when the json has `is_error: true`, `Err(anyhow!("claude CLI error: {result}"))`. The non-zero-exit path includes the json `result` text when stdout parses (json or stream), not only stderr. Raw-text fallback kept (tests :571-597).
**Test.** json with `is_error:true` → Err containing the result text; normal json unchanged; non-zero exit with json stdout → error contains result.

## P2-7: resume retry on any error
**Cause.** backend.rs:150-161 `Err(_) if resume.is_some()` re-sends the full transcript + images on any error (rate limit, auth, network); first error discarded.
**Design.** Retry only when a pure `is_missing_session(&anyhow::Error)` matches `no conversation found` (case-insensitive, over the error chain). Otherwise return the original error and keep `session_id`. If the retry also fails, its error carries the first as context. Lands after P2-6.
**Test.** `is_missing_session` true for `No conversation found with session ID: x`, false for `rate limit`.

## P2-8: a second usta can "recover" the live session
**Cause.** Startup recovery (main.rs:111-185) offers every unfinished transcript from `transcript::find_unfinished`, including one a running usta is still writing; accepting flushes and renames it, declining deletes it. The lock (`.usta/.lock-<topic>`, pid inside, lifecycle.rs:57/:289) is only checked by existence, never for liveness.
**Design.** New `src/lock.rs`: `pid_alive(pid: u32) -> bool` (Unix: `kill -0 <pid>` via `std::process::Command`, success = alive; Windows: `tasklist /FI "PID eq <pid>" /NH` output contains the pid) and `live_owner(root, topic) -> Option<u32>` (lock exists, pid parses, pid != own pid, alive). Recovery skips a record whose topic has a live owner, with a one-line note (`skipping <topic>: a running usta owns this session`). A lock whose pid is dead does not block recovery. No new crates.
**Why:** liveness is the real question; the crash case (dead pid) must still recover. **Rejected:** OS advisory lock — needs a crate or Rust 1.89.
**Test.** `pid_alive(std::process::id())` true; a spawned-and-reaped child's pid false; `live_owner` with own pid → None, with a dead pid → None.

## P2-9: flush skipped and lock left on error/signal
**Cause.** Plain path: `run_plain_loop(...).await?` (main.rs) skips `flush_progress` and lock removal on Err. TUI: any `?` inside `tui::run::run` bypasses `closing::save_live`. Lock removed only at the end of main.rs. No signal handling (tokio features lack `signal`).
**Design.**
- `LockGuard` (in lock.rs) whose `Drop` removes the lock file; created where the lock is written; removes the lock on every exit path including errors and panics-with-unwind. Main's explicit removal goes through the guard.
- Plain path: capture the loop result, always run the flush, then propagate.
- Signals: enable tokio `signal`. `src/shutdown.rs`: `async fn shutdown_signal()` resolving on SIGTERM or SIGHUP (Unix) / `ctrl_close` or `ctrl_shutdown` (Windows); if registration fails it pends forever. Added as a `select!` arm in the TUI main loop (run.rs, breaks into the existing `save_live` path), in ask.rs's wait (returns Cancelled so the loop can exit), and in the plain loop (breaks to the flush). SIGINT stays as today (raw-mode Ctrl+C is a key).
- Where a signal arrives during a blocking non-selectable call it is noticed after that call; acceptable.
**Why:** the guard handles every error path in one place; signals reuse the normal save path.
**Test.** `LockGuard` drop removes the file; plain-path helper flushes on Err (if extractable); `shutdown_signal` arm presence via a minimal unit test that sends SIGHUP to self under `#[cfg(unix)]` with the handler registered first (if flaky, a source pin). Manual: `kill -TERM` a running TUI in a temp HOME → progress saved, lock gone (manual checklist).

## P2-10: slug loses letters / collides
**Cause.** `topic::slugify_topic` (topic.rs:145) keeps only ASCII alphanumerics after a Turkish-only `deasciify` (topic.rs:128): `straße→strae`, `café→caf`, `C++`/`C#`/`C`→`c`, non-Latin → `general`; `reset ""` → `general` (cli.rs:46).
**Design.**
- Extend `deasciify` with a Latin table (applied per char after the existing Turkish mapping, existing Turkish results unchanged): à á â ã ä å ā ą→a, æ→ae, ć č→c, ď đ ð→d, è é ê ë ē ę ě→e, ì í î ï ī→i, ł→l, ñ ń ň→n, ò ó ô õ ø ō ő→o, œ→oe, ř→r, ś š→s, ß→ss, ť→t, þ→th, ù ú û ū ů ű→u, ý ÿ→y, ź ż ž→z (and their uppercase forms via lowercasing first, keeping the existing `İ`/`I` handling).
- Inside a word, `+`→`p` and `#`→`sharp`: `c++`→`cpp`, `c#`→`csharp`, `f#`→`fsharp`.
- If the result is empty but the input contained any alphanumeric char (`char::is_alphanumeric`, e.g. 日本語, Привет), return `t-<8 hex>` = FNV-1a 32-bit of the trimmed, lowercased input (implemented inline; `DefaultHasher` is not stable across Rust versions). Input with no alphanumeric char stays `general`.
- `usta reset ""` (blank after trim) → error `topic name required` before slugifying.
- Existing slugs: every input that previously produced a non-empty slug without `+`/`#`/non-Turkish diacritics produces the same slug. Tests pinning `C++→c` (topic.rs `slugify_drops_non_alnum_chars`, cli.rs `parse_reset_topic_is_slugified`) are updated.
**Test.** straße→strasse, café→cafe, Ñandú→nandu, c++→cpp, c#→csharp, 日本語→`t-…` stable and ≠ general and ≠ Привет's, "!!!"→general, Turkish cases unchanged, `reset ""` → Err.

## P2-11: PDF conversion
**Cause.** `materials::convert_pdfs` (materials.rs:141-203) runs `pdftotext` with blocking `output()` and no timeout, from sync code on the async runtime (tui/entry.rs:175, tui/intro.rs:245, plain.rs:217); writes `<stem>.txt` next to the PDF, overwriting a hand-written file; the new .txt is then reported by the watcher as fresh work; a scanned PDF (no text layer) yields a blank txt silently; `.pdf` match is case-sensitive. Refuted sub-claim: a failed conversion does not count as fresh forever (no txt is written, so it is retried) — except a partial txt left by a failing run.
**Design.**
- Output goes to `materials/.converted/<stem>.txt` (hidden dir: watcher and `collect_files` already skip dot-dirs, so no feedback). `scan()` reads `.converted/` too. If a sibling `<stem>.txt` exists (hand-written, or converted by older usta), the PDF is skipped — never overwritten, no duplicate.
- Per-file timeout 30 s: `spawn` + `try_wait` poll loop, `kill` + `wait` on timeout, note `pdftotext timed out on <name>`. Bounded blocking is accepted (moving to the async runtime would need `block_in_place`, which panics under current-thread test runtimes).
- On failure or timeout, remove any partial output. A conversion whose output is blank: note `no text found in <name> (scanned PDF?)`; the blank file stays as the freshness marker so it is reported once.
- Extension match case-insensitive.
- Tool injectable for tests: `convert_pdfs_with(root, tool: &str)`; `convert_pdfs(root)` calls it with `"pdftotext"`. The missing-tool test uses a nonexistent tool name, so the pre-existing failure is fixed.
**Test.** Missing tool → `pdftotext missing` note; hand-written sibling txt untouched and PDF skipped; fake tool (shell script) that sleeps → timeout note, no output (timeout made a parameter internally, e.g. `convert_pdfs_inner(root, tool, timeout)`, test uses ~1 s); fake tool writing blank → scanned note; `.PDF` picked up. Shell-script tools under `#[cfg(unix)]`.

## P2-12: `cargo check` not killed on timeout
**Cause.** check.rs:42-47 `tokio::process::Command::new("cargo")…output()` inside `timeout(60s)`; on timeout the child keeps running (`kill_on_drop` defaults to false) and holds the build lock.
**Design.** Add `.kill_on_drop(true)` and `.stdin(Stdio::null())`.
**Test.** Source-level pin in check.rs tests that the command builder contains `kill_on_drop(true)` (a behavioural test would need a slow cargo).

---

## Release
- P1 → v0.38.0, P2 → v0.39.0: Cargo.toml + Cargo.lock version, `welcome_tests.rs` version pin, SPEC.md version entry (one paragraph like v0.37.1's), README version mention as in `cf42c8f`.
- Manual-only checks go to `.superpowers/sdd/review-fixes-manual-checklist.md` (Windows key handling, SIGTERM/SIGHUP save, real API truncation behaviour, macOS FSEvents realpath).
