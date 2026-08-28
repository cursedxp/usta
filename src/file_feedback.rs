//! File-watch feedback path: decides whether a changed file is worth sending
//! to the model, and what frame to send it in — extracted from `main.rs`
//! (module split, Task 6). Name note: `feedback.rs` (FileMemory/diffing) and
//! `watcher.rs` (fs events) already exist; this is the layer above both.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::backend::{self, Backend};
use crate::feedback;
use crate::lifecycle::ask_usta;
use crate::session::Session;
use crate::transcript;
use crate::{check, progress, visual};

/// File-change feedback result — the caller (plain/TUI) prints it themselves.
/// `handle_file_change` never println!s anything (so stdout doesn't get corrupted
/// in raw-mode); the full `Reply` is carried so the `web` flag and context gauge
/// can be reproduced on the caller's side with the original behavior (print_reply).
pub(crate) enum FileFeedback {
    /// Skip — no output.
    Sessiz,
    /// Large-file notice — the caller shows it in its own way
    /// (plain: `println!`, TUI: `page_notice`).
    Bildirim(String),
    /// Actual reply — context token count + full `Reply` (the web flag is preserved).
    /// `reply.text` is already CLEAN (marker stripped, see `visual::extract_show_marker`
    /// in this function) — the caller prints it as-is. `show_topic` carries the
    /// marker's topic, if any (Görev 4): the caller runs the visual flow AFTER printing.
    Yanit {
        tokens: Option<u64>,
        reply: backend::Reply,
        show_topic: Option<String>,
    },
}

/// Is this saved file an exercise deliverable? True when a path component is
/// the `exercises/` dir (project-root-relative when possible; the watcher
/// hands absolute paths, so we fall back to scanning the path as-is).
///
/// The fallback assumes `project_root` and the watched path share canonical
/// form (both are absolute in normal operation, so `strip_prefix` succeeds).
/// If a mismatch makes `strip_prefix` fail, the raw absolute path is scanned —
/// so a project living UNDER an `exercises/` ancestor could misclassify a
/// normal file. Narrow and accepted (design chose the fallback to keep
/// watcher-absolute paths detectable); canonicalize the root if it ever bites.
pub(crate) fn is_exercise_path(project_root: &Path, path: &Path) -> bool {
    let rel = path.strip_prefix(project_root).unwrap_or(path);
    rel.components().any(|c| c.as_os_str() == "exercises")
}

/// True if `e`'s cause chain contains an io error the user shouldn't be
/// bothered with — a read failure that means "there is no text content here",
/// not "something is wrong":
/// - `NotFound`: the file died between the watcher event and the feedback
///   read (e.g. a tool's transient temp file).
/// - `InvalidData`: non-UTF-8 content — a binary (image, archive, …) dropped
///   into the project. Content-based, so the domain-agnostic principle holds:
///   any TEXT file of any extension is still watched (a GTM brief is a valid
///   deliverable), binary never is.
/// - `IsADirectory`: the watcher's own `is_dir()` source filter should have
///   caught this, but create/rename can race it (e.g. `cargo new` creating
///   `src/` mid-event) — treat the same as the other silent cases rather
///   than surfacing "Is a directory (os error 21)" to the user.
///
/// Everything else (PermissionDenied, …) is a real failure the user should see.
pub(crate) fn is_silent_skip(e: &anyhow::Error) -> bool {
    e.chain()
        .filter_map(|cause| cause.downcast_ref::<std::io::Error>())
        .any(|io_err| {
            matches!(
                io_err.kind(),
                std::io::ErrorKind::NotFound
                    | std::io::ErrorKind::InvalidData
                    | std::io::ErrorKind::IsADirectory
            )
        })
}

/// Shared exercise-review instruction (review-as-exercise, hint ladder, no
/// solutions handed over) — the first-sight wording from `feedback_frame`,
/// extracted so `flow_frame`'s exercise addendum doesn't duplicate it.
/// Both callers must keep producing the exact same sentence.
const EXERCISE_REVIEW_RULE: &str = "Review it AS AN EXERCISE: compare against the assignment, apply the hint ladder (start high), point at what to reconsider — do NOT rewrite or complete it for them. If no exercise was assigned this session, treat it as spontaneous practice work and review it the same way.";

/// Build the injected user-turn for a watched-file change. Exercise files get
/// an exercise-review frame (assignment comparison, hint ladder, no solutions);
/// everything else keeps the original project-feedback wording VERBATIM.
pub(crate) fn feedback_frame(
    is_exercise: bool,
    path_display: &str,
    body: &str,
    is_diff: bool,
) -> String {
    match (is_exercise, is_diff) {
        (false, false) => format!(
            "[File saved: {path_display}]\n{body}\n\nGive project-grounded, Socratic feedback on this change."
        ),
        (false, true) => format!(
            "[File changed: {path_display}]\nChange (unified diff):\n{body}\n\nGive project-grounded, Socratic feedback on this change — focus on what changed."
        ),
        (true, false) => format!(
            "[Exercise submission saved: {path_display}]\n{body}\n\nThis is the user's deliverable for the exercise you assigned. {EXERCISE_REVIEW_RULE}"
        ),
        (true, true) => format!(
            "[Exercise submission changed: {path_display}]\nChange (unified diff):\n{body}\n\nReview the revision AS AN EXERCISE iteration: did it address your previous feedback? Move one rung down the hint ladder only if they're stuck — never hand over the solution."
        ),
    }
}

/// Build the injected user-turn for a companion-mode (lesson-flow) batch. Unlike
/// `feedback_frame`'s plain review wording, this frame casts the model as the
/// mentor mid-lesson, not a fresh reviewer — the five rules from the design
/// spec (`docs/superpowers/specs/2026-08-27-flow-companion-design.md`,
/// "Behavior" section, companion default): (1) check a requested
/// step and advance on success, (2) nudge any unanswered question of yours
/// in one short sentence, never a full repeat, (3) wave off tool-generated
/// scaffold in one sentence and focus on the user's hand-written change,
/// (4) answer an interruption then recall the task, (5) treat an assigned
/// artifact's content as eyes-only until the user reports on it. `files_payload`
/// is Task 2's pre-merged multi-file block (one or more `FILE: <path>`
/// sections); this function does not build it.
///
/// `any_exercise` appends the shared exercise-review rule (see
/// `EXERCISE_REVIEW_RULE`) when at least one file in the batch is an exercise
/// submission.
///
/// Rules 2 and 5 are the K5 backup layer for ride-along payloads — K1 removes
/// the leak opportunity, these guard the payload itself.
///
/// Called from `ride_along_turn` (below), the companion default's delivery
/// path: the frame ships with the batch that rides along on the user's next
/// submit (spec K2). `handle_batch_change` no longer chooses between frames —
/// the only path left through it is live, which is plain review by definition.
pub(crate) fn flow_frame(files_payload: &str, any_exercise: bool) -> String {
    let mut frame = format!(
        "[Files changed]\n{files_payload}\n\n\
This change is part of the ongoing lesson — respond as the mentor guiding it, not as a reviewer opening a fresh audit. Apply these rules:\n\
1. If your last message asked for a step and this change satisfies it: confirm briefly, flag any errors, move to the next step — unless rule 5 restricts it, in which case only acknowledge that the step happened and say nothing about the artifact's content.\n\
2. If there's an unanswered question from you still pending: nudge it in ONE short sentence — never repeat the full question text.\n\
3. First-sight full-content files may be tool-generated scaffold (e.g. a `cargo new` template) — acknowledge scaffold in one sentence, don't review it line by line; focus on the user's hand-written change.\n\
4. If the user asks a question in the middle of this, answer it, then recall the task.\n\
5. If your assignment asked the user to read, run, or describe an artifact, that artifact is OFF-LIMITS to discuss until the user reports on it: do not quote, summarize, or explain it. When their report arrives, verify it against what you saw."
    );
    if any_exercise {
        frame.push_str(&format!(
            "\n\nThis batch includes an exercise submission. {EXERCISE_REVIEW_RULE}"
        ));
    }
    frame
}

/// The `cargo check` block appended to a file-feedback turn — the prediction
/// protocol's raw input (spec §4.6). One definition, three call sites
/// (single-file plain path, live batch, ride-along delivery), so the framing
/// the model sees can never drift between paths.
fn check_result_block(check_result: &str) -> String {
    format!(
        "\n\n[cargo check result — FOR YOUR EYES ONLY, don't pass this directly to the user; apply the prediction protocol]\n{check_result}"
    )
}

/// Seed FileMemory with the mentor docs (`mentor/PROJECT.md`, `mentor/PROGRESS.md`)
/// that are ALREADY embedded in the system prompt at session start. Without this,
/// an unchanged re-save of one of these files is a "first sight" for the watcher —
/// it would re-send the whole file to the LLM, wasting a turn and doubling context.
/// After seeding, an unchanged save is a `Skip`, and an edit is a `Diff` (fark),
/// not full content. Missing files are silently skipped.
pub(crate) fn seed_mentor_baseline(files: &mut feedback::FileMemory, project_root: &Path) {
    for path in [
        progress::project_md_path(project_root),
        progress::project_progress_path(project_root),
    ] {
        if let Ok(content) = std::fs::read_to_string(&path) {
            files.seed(&path, content);
        }
    }
}

/// Runs a saved file through FileMemory; full content on first sight, a diff
/// afterward, turned into a synthetic user turn → Socratic feedback. For a cargo
/// project, the check result is appended in an "Usta's eyes only" block (prediction protocol).
/// Exercise files (see `is_exercise_path`) get the exercise-review frame and skip
/// `cargo check` entirely — the check doesn't apply to review-only submissions.
/// Doesn't PRINT anything — both the plain and TUI paths print the returned
/// `FileFeedback` in their own presentation language (so stdout doesn't get corrupted in raw-mode).
pub(crate) async fn handle_file_change(
    backend: &mut Backend,
    session: &mut Session,
    files: &mut feedback::FileMemory,
    project_root: &Path,
    path: &Path,
    recorder: &transcript::Recorder,
) -> Result<FileFeedback> {
    let contents = std::fs::read_to_string(path)?;
    let exercise = is_exercise_path(project_root, path);
    let mut injected = match files.observe(path, contents) {
        feedback::ChangePayload::Skip => return Ok(FileFeedback::Sessiz),
        feedback::ChangePayload::TooLarge(len) => {
            return Ok(FileFeedback::Bildirim(format!(
                "(large file — not watched: {} — {len} bytes)",
                path.display()
            )));
        }
        feedback::ChangePayload::FirstSight(full) => {
            feedback_frame(exercise, &path.display().to_string(), &full, false)
        }
        feedback::ChangePayload::Diff(diff) => {
            feedback_frame(exercise, &path.display().to_string(), &diff, true)
        }
    };
    if !exercise {
        if let Some(check_result) = check::run_check(project_root).await {
            injected.push_str(&check_result_block(&check_result));
        }
    }
    session.push_user(&injected);
    recorder.user(&injected);
    let reply = ask_usta(backend, &session.system, session.history()).await?;
    let tokens = reply.context_tokens;
    // Marker (Görev 4) is stripped BEFORE recording/pushing — history never
    // carries `[[show: ...]]`, only the clean text does.
    let (clean, show_topic) = visual::extract_show_marker(&reply.text);
    recorder.assistant(&clean);
    session.push_assistant(clean.clone());
    let display_reply = backend::Reply {
        text: clean,
        web: reply.web,
        context_tokens: reply.context_tokens,
    };
    Ok(FileFeedback::Yanit {
        tokens,
        reply: display_reply,
        show_topic,
    })
}

/// Metadata `build_batch_payload` derives alongside the merged payload —
/// everything `handle_batch_change` needs to pick a frame, decide whether to
/// run `cargo check`, and report what happened, without re-walking `paths`.
pub(crate) struct BatchMeta {
    /// How many files made it into the payload (0 means no LLM call at all).
    pub(crate) total_included: usize,
    /// At least one included file is an exercise submission.
    pub(crate) any_exercise: bool,
    /// At least one included file is NOT an exercise submission (gates
    /// `cargo check`, same rule `handle_file_change` applies per-file).
    pub(crate) any_non_exercise: bool,
    /// Notices for files that were dropped but still worth telling the user
    /// about (read errors that aren't silent-skip, oversized files). In
    /// `paths` order.
    pub(crate) notices: Vec<String>,
    /// Path display strings of the INCLUDED files, in `paths` order.
    pub(crate) displays: Vec<String>,
}

/// Merge a debounce batch of changed files into one payload for a single LLM
/// turn. Pure aside from the `FileMemory` state machine and the file reads
/// themselves — no LLM call, so this is what the batch tests exercise
/// directly. Per-file classification mirrors `handle_file_change` exactly
/// (same silent-skip rule, same oversized-file notice text); the only new
/// behavior is merging N files into one `FILE: <path> (<kind>)` block per
/// included file, joined with blank lines, in `paths` order.
fn build_batch_payload(
    files: &mut feedback::FileMemory,
    project_root: &Path,
    paths: &[PathBuf],
) -> (String, BatchMeta) {
    let mut blocks = Vec::new();
    let mut meta = BatchMeta {
        total_included: 0,
        any_exercise: false,
        any_non_exercise: false,
        notices: Vec::new(),
        displays: Vec::new(),
    };
    for path in paths {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                let e = anyhow::Error::new(e);
                if !is_silent_skip(&e) {
                    meta.notices
                        .push(format!("file feedback skipped: {}: {e}", path.display()));
                }
                continue;
            }
        };
        let exercise = is_exercise_path(project_root, path);
        let (body, is_diff) = match files.observe(path, content) {
            feedback::ChangePayload::Skip => continue,
            feedback::ChangePayload::TooLarge(len) => {
                meta.notices.push(format!(
                    "(large file — not watched: {} — {len} bytes)",
                    path.display()
                ));
                continue;
            }
            feedback::ChangePayload::FirstSight(full) => (full, false),
            feedback::ChangePayload::Diff(diff) => (diff, true),
        };
        let kind = match (exercise, is_diff) {
            (false, false) => "full contents",
            (false, true) => "unified diff",
            (true, false) => "exercise submission, full contents",
            (true, true) => "exercise submission, unified diff",
        };
        blocks.push(format!("FILE: {} ({kind})\n{body}", path.display()));
        meta.displays.push(path.display().to_string());
        meta.total_included += 1;
        if exercise {
            meta.any_exercise = true;
        } else {
            meta.any_non_exercise = true;
        }
    }
    (blocks.join("\n\n"), meta)
}

/// Runs a whole debounce batch of saved files through `FileMemory` and turns
/// it into ONE injected user turn → ONE LLM call, instead of one call per
/// file (`handle_file_change`'s old per-file loop, see `polite::process_batch`).
/// `.0` is notices the caller should print, in `paths` order, BEFORE the
/// reply (dropped files that are still worth telling the user about);
/// `.1` is `FileFeedback::Sessiz` when nothing made it into the payload (no
/// LLM call at all), otherwise `FileFeedback::Yanit` in its usual shape.
/// `FileFeedback::Bildirim` is never returned here — a batch can carry both a
/// large-file notice AND a reply, which a single `FileFeedback` can't express;
/// that variant stays in the enum for `handle_file_change`'s single-file path.
///
/// Frame: always the plain review frame (feedback_frame) — only the live path
/// calls this, and live is plain review by definition (spec K4); the companion
/// frame ships with ride-along delivery (deliver_pending). The payload goes in
/// as one block, exercise-flagged when any file in the batch is an exercise
/// submission. `cargo check` runs at most once per
/// batch — only when at least one included file is not an exercise
/// submission — instead of once per file.
pub(crate) async fn handle_batch_change(
    backend: &mut Backend,
    session: &mut Session,
    files: &mut feedback::FileMemory,
    project_root: &Path,
    paths: &[PathBuf],
    recorder: &transcript::Recorder,
) -> Result<(Vec<String>, FileFeedback)> {
    let (payload, meta) = build_batch_payload(files, project_root, paths);
    if meta.total_included == 0 {
        return Ok((meta.notices, FileFeedback::Sessiz));
    }
    let mut injected = feedback_frame(
        meta.any_exercise,
        &meta.displays.join(", "),
        &payload,
        false,
    );
    if meta.any_non_exercise {
        if let Some(check_result) = check::run_check(project_root).await {
            injected.push_str(&check_result_block(&check_result));
        }
    }
    session.push_user(&injected);
    recorder.user(&injected);
    let reply = ask_usta(backend, &session.system, session.history()).await?;
    let tokens = reply.context_tokens;
    // Marker (Görev 4) is stripped BEFORE recording/pushing — same rule as
    // handle_file_change: history never carries `[[show: ...]]`.
    let (clean, show_topic) = visual::extract_show_marker(&reply.text);
    recorder.assistant(&clean);
    session.push_assistant(clean.clone());
    let display_reply = backend::Reply {
        text: clean,
        web: reply.web,
        context_tokens: reply.context_tokens,
    };
    Ok((
        meta.notices,
        FileFeedback::Yanit {
            tokens,
            reply: display_reply,
            show_topic,
        },
    ))
}

/// One-line frame at the head of a ride-along delivery (spec name:
/// `pending_preamble`). Purely descriptive — the behavioral rules live in
/// `flow_frame`, which follows it.
const PENDING_PREAMBLE: &str = "[The user changed the files below while working; they are delivered together with the user's message, which follows after the file block — the user's own words are the message to answer.]";

/// Compose the combined outgoing turn for a ride-along delivery (spec K2):
/// the one-line pending preamble, the lesson-flow-framed file block
/// (companion frame axis, spec K4), the optional eyes-only `cargo check`
/// block, then the user's own words LAST — their message is the one to answer
/// (spec: Sıralama ve içerik). The check block sits with the files it
/// describes, ahead of the user's text, so the user still gets the last word.
fn ride_along_turn(
    files_payload: &str,
    any_exercise: bool,
    check_block: Option<&str>,
    user_text: &str,
) -> String {
    let mut turn = format!(
        "{PENDING_PREAMBLE}\n{}",
        flow_frame(files_payload, any_exercise)
    );
    if let Some(block) = check_block {
        turn.push_str(block);
    }
    turn.push_str("\n\n");
    turn.push_str(user_text);
    turn
}

/// Deterministic ride-along delivery (spec K2): build the payload NOW — at
/// delivery time, not at flush time — so intermediate saves collapse into one
/// diff and meanwhile-deleted files drop out as silent skips. `.0` is the
/// notice channel (large/binary files), printed by the caller at delivery;
/// `.1` is the combined outgoing turn, or `user_text` UNCHANGED when nothing
/// made it into the payload. No LLM call happens here — the caller sends the
/// returned string through the normal ask path (prompt diet: only payload and
/// frame ever reach the model).
///
/// `cargo check` runs here too, under exactly the live path's condition (at
/// least one included file is NOT an exercise submission), and its result is
/// appended in the same eyes-only block — without this the prediction protocol
/// (spec §4.6) would never fire in the default companion mode, since delivery
/// is the only path a companion-mode save takes. Running it at DELIVERY rather
/// than at every flush also keeps the silent accumulation phase free of
/// compiles.
pub(crate) async fn deliver_pending(
    files: &mut feedback::FileMemory,
    project_root: &Path,
    paths: &[PathBuf],
    user_text: String,
) -> (Vec<String>, String) {
    let (payload, meta) = build_batch_payload(files, project_root, paths);
    if meta.total_included == 0 {
        return (meta.notices, user_text);
    }
    let check = if meta.any_non_exercise {
        check::run_check(project_root)
            .await
            .map(|r| check_result_block(&r))
    } else {
        None
    };
    let turn = ride_along_turn(&payload, meta.any_exercise, check.as_deref(), &user_text);
    (meta.notices, turn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_exercise_path_detects_exercises_dir() {
        let root = Path::new("/tmp/proj");
        assert!(is_exercise_path(
            root,
            Path::new("/tmp/proj/exercises/a.md")
        ));
        assert!(is_exercise_path(
            root,
            Path::new("/tmp/proj/exercises/gtm/brief.md")
        ));
        assert!(!is_exercise_path(
            root,
            Path::new("/tmp/proj/src/exercises.rs")
        ));
        assert!(!is_exercise_path(
            root,
            Path::new("/tmp/proj/mentor/PROJECT.md")
        ));
        // watcher may hand a path the root-strip doesn't cover — component scan fallback
        assert!(is_exercise_path(
            root,
            Path::new("/other/place/exercises/x.md")
        ));
        assert!(!is_exercise_path(
            root,
            Path::new("/other/place/src/lib.rs")
        ));
    }

    #[test]
    fn is_silent_skip_true_for_wrapped_not_found() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
        let e = anyhow::Error::new(io_err).context("reading watched file");
        assert!(is_silent_skip(&e));
    }

    #[test]
    fn is_silent_skip_true_for_wrapped_invalid_data() {
        // Binary content (an image dropped into the project) → read_to_string
        // fails with InvalidData. Not the user's business — silent, like NotFound.
        let io_err = std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "stream did not contain valid UTF-8",
        );
        let e = anyhow::Error::new(io_err).context("reading watched file");
        assert!(is_silent_skip(&e));
    }

    #[test]
    fn is_silent_skip_true_for_wrapped_is_a_directory() {
        // A directory path that slipped past the watcher's own filter (race
        // between create and the source filter) must still be silent, not a
        // user-visible "Is a directory (os error 21)".
        let io = std::io::Error::new(std::io::ErrorKind::IsADirectory, "is a directory");
        let e = anyhow::Error::from(io).context("reading changed file");
        assert!(is_silent_skip(&e));
    }

    #[test]
    fn is_silent_skip_false_for_wrapped_permission_denied() {
        // A real read failure the user SHOULD see — must keep warning.
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let e = anyhow::Error::new(io_err).context("reading watched file");
        assert!(!is_silent_skip(&e));
    }

    #[test]
    fn binary_file_read_error_classifies_as_silent_skip() {
        // Integration-style: a real non-UTF-8 byte file through the same read
        // call handle_file_change uses must classify as silent.
        let dir = std::env::temp_dir().join(format!("usta-binary-skip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("image.png");
        std::fs::write(&p, [0x89u8, 0x50, 0x4E, 0x47, 0xFF, 0xFE, 0x00, 0x9C]).unwrap();
        let err = std::fs::read_to_string(&p).expect_err("non-UTF-8 must fail read_to_string");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(is_silent_skip(&anyhow::Error::new(err)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn feedback_frame_regular_paths_keep_existing_wording() {
        let s = feedback_frame(false, "src/main.rs", "fn main() {}", false);
        assert!(s.contains("[File saved: src/main.rs]"));
        assert!(s.contains("Give project-grounded, Socratic feedback on this change."));
        let d = feedback_frame(false, "src/main.rs", "-a\n+b", true);
        assert!(d.contains("[File changed: src/main.rs]"));
        assert!(d.contains("focus on what changed"));
    }

    #[test]
    fn feedback_frame_exercise_paths_review_as_exercise() {
        let s = feedback_frame(true, "exercises/gtm/brief.md", "draft", false);
        assert!(s.contains("[Exercise submission saved: exercises/gtm/brief.md]"));
        assert!(s.contains("AS AN EXERCISE"));
        assert!(s.contains("hint ladder"));
        assert!(s.contains("do NOT rewrite"));
        let d = feedback_frame(true, "exercises/gtm/brief.md", "-a\n+b", true);
        assert!(d.contains("[Exercise submission changed: exercises/gtm/brief.md]"));
        assert!(d.contains("previous feedback"));
        assert!(d.contains("never hand over the solution"));
    }

    #[test]
    fn flow_frame_pins_the_five_lesson_rules() {
        let s = flow_frame("FILE: src/main.rs\n...", false);
        // (1) step check + advance, (2) one-sentence nudge — never a full
        // repeat (spec K5.2), (3) scaffold in one sentence, (4) answer then
        // recall the task, (5) eyes-only until the user reports (spec K5.1)
        assert!(s.contains("part of the ongoing lesson"));
        assert!(s.contains("next step"));
        assert!(s.contains("unanswered question"));
        assert!(s.contains("ONE short sentence"));
        assert!(s.contains("never repeat the full question"));
        assert!(s.contains("scaffold"));
        assert!(s.contains("hand-written"));
        assert!(s.contains("OFF-LIMITS"));
        assert!(s.contains("verify it against"));
        // Rule 1's rule-5 escape hatch must license ACKNOWLEDGEMENT ONLY.
        // The old wording ("describe only the change") licensed exactly the
        // leak rule 5 forbids: for a first-sight file the change IS the whole
        // content, so a restricted artifact could be reproduced verbatim.
        assert!(s.contains("only acknowledge that the step happened"));
        assert!(s.contains("say nothing about the artifact's content"));
        assert!(!s.contains("describe only the change"));
        assert!(!s.to_lowercase().contains("standalone code review"));
    }

    #[test]
    fn flow_frame_carries_exercise_rule_when_flagged() {
        assert!(flow_frame("x", true).contains("AS AN EXERCISE"));
        assert!(!flow_frame("x", false).contains("AS AN EXERCISE"));
    }

    #[test]
    fn batch_change_selects_feedback_frame_not_flow_frame() {
        // `handle_batch_change` needs a live Backend, so its frame choice
        // can't be driven from a unit test: switching the call to
        // `flow_frame` would leave every test green while the live path
        // silently adopted the companion frame. Pin the call site in this
        // file's own production source (same crude-pin pattern as the old,
        // deleted `polite_branch_selecting_flow_frame_is_pinned` — restored
        // here because its replacement,
        // `ride_along_turn_selects_flow_frame_and_keeps_user_words_last`,
        // only covers the OTHER branch, `ride_along_turn`). Split at the
        // test module so this assert's own text can't satisfy it, then
        // scope to `handle_batch_change`'s own body so the legitimate
        // `flow_frame(` call in `ride_along_turn` doesn't false-positive.
        let production_src = include_str!("file_feedback.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let body_start = production_src
            .find("pub(crate) async fn handle_batch_change")
            .expect("handle_batch_change definition not found");
        let body_end = production_src[body_start..]
            .find("fn ride_along_turn(")
            .map(|offset| body_start + offset)
            .expect("handle_batch_change body end marker not found");
        let body = &production_src[body_start..body_end];
        assert!(
            body.contains("feedback_frame("),
            "handle_batch_change must select feedback_frame (plain review) on the live path — it is plain review by definition (spec K4)"
        );
        assert!(
            !body.contains("flow_frame("),
            "handle_batch_change must never call flow_frame — the companion frame ships with ride-along delivery instead"
        );
    }

    /// Unique scratch dir per test so parallel `cargo test` runs don't collide
    /// (same pattern as `binary_file_read_error_classifies_as_silent_skip`).
    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("usta-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn batch_payload_merges_files_and_drops_skips() {
        let dir = scratch_dir("batch-merge");
        let unchanged = dir.join("unchanged.rs");
        let changed = dir.join("changed.rs");
        let binary = dir.join("image.png");
        std::fs::write(&binary, [0x89u8, 0x50, 0x4E, 0x47, 0xFF, 0xFE, 0x00, 0x9C]).unwrap();

        let mut files = feedback::FileMemory::new();
        // Baseline both text files, then only change one — so `unchanged`
        // observes as Skip and `changed` observes as Diff.
        files.seed(&unchanged, "same\n".to_string());
        files.seed(&changed, "old\n".to_string());
        std::fs::write(&unchanged, "same\n").unwrap();
        std::fs::write(&changed, "new\n").unwrap();

        let paths = vec![unchanged.clone(), changed.clone(), binary.clone()];
        let (payload, meta) = build_batch_payload(&mut files, &dir, &paths);

        assert_eq!(meta.total_included, 1);
        assert_eq!(meta.displays, vec![changed.display().to_string()]);
        assert!(payload.contains(&format!("FILE: {} (unified diff)", changed.display())));
        assert!(payload.contains("-old"));
        assert!(payload.contains("+new"));
        assert!(!payload.contains(&format!("FILE: {}", unchanged.display())));
        assert!(!payload.contains(&format!("FILE: {}", binary.display())));
        // Skip (unchanged) is silent, and the binary read error is a silent-skip
        // class (InvalidData) — neither produces a notice.
        assert!(meta.notices.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn batch_payload_orders_files_deterministically() {
        let dir = scratch_dir("batch-order");
        let a = dir.join("a.rs");
        let b = dir.join("b.rs");
        std::fs::write(&a, "fn a() {}\n").unwrap();
        std::fs::write(&b, "fn b() {}\n").unwrap();
        // Deliberately not alphabetical — order must follow `paths`, not sorting.
        let paths = vec![b.clone(), a.clone()];

        let mut files1 = feedback::FileMemory::new();
        let (payload1, meta1) = build_batch_payload(&mut files1, &dir, &paths);
        let mut files2 = feedback::FileMemory::new();
        let (payload2, meta2) = build_batch_payload(&mut files2, &dir, &paths);

        assert_eq!(payload1, payload2);
        assert_eq!(meta1.displays, meta2.displays);
        assert_eq!(
            meta1.displays,
            vec![b.display().to_string(), a.display().to_string()]
        );
        let b_pos = payload1.find(&b.display().to_string()).unwrap();
        let a_pos = payload1.find(&a.display().to_string()).unwrap();
        assert!(b_pos < a_pos);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn batch_payload_flags_exercise_and_non_exercise_mix() {
        let dir = scratch_dir("batch-mix");
        let exercises_dir = dir.join("exercises");
        std::fs::create_dir_all(&exercises_dir).unwrap();
        let exercise_file = exercises_dir.join("brief.md");
        let regular_file = dir.join("src_main.rs");
        std::fs::write(&exercise_file, "my answer\n").unwrap();
        std::fs::write(&regular_file, "fn main() {}\n").unwrap();

        let mut files = feedback::FileMemory::new();
        let paths = vec![exercise_file.clone(), regular_file.clone()];
        let (payload, meta) = build_batch_payload(&mut files, &dir, &paths);

        assert_eq!(meta.total_included, 2);
        assert!(meta.any_exercise);
        assert!(meta.any_non_exercise);
        assert!(payload.contains(&format!(
            "FILE: {} (exercise submission, full contents)",
            exercise_file.display()
        )));
        assert!(payload.contains(&format!("FILE: {} (full contents)", regular_file.display())));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn batch_payload_exercise_only_batch_has_no_non_exercise_flag() {
        // Gates whether `handle_batch_change` runs `cargo check` at all.
        let dir = scratch_dir("batch-exercise-only");
        let exercises_dir = dir.join("exercises");
        std::fs::create_dir_all(&exercises_dir).unwrap();
        let exercise_file = exercises_dir.join("brief.md");
        std::fs::write(&exercise_file, "my answer\n").unwrap();

        let mut files = feedback::FileMemory::new();
        let paths = vec![exercise_file.clone()];
        let (_payload, meta) = build_batch_payload(&mut files, &dir, &paths);

        assert!(meta.any_exercise);
        assert!(!meta.any_non_exercise);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn batch_payload_too_large_file_drops_with_existing_notice_text() {
        let dir = scratch_dir("batch-too-large");
        let big_path = dir.join("big.rs");
        let big = "x".repeat(feedback::MAX_FILE_BYTES + 1);
        std::fs::write(&big_path, &big).unwrap();

        let mut files = feedback::FileMemory::new();
        let paths = vec![big_path.clone()];
        let (payload, meta) = build_batch_payload(&mut files, &dir, &paths);

        assert_eq!(meta.total_included, 0);
        assert!(payload.is_empty());
        assert_eq!(
            meta.notices,
            vec![format!(
                "(large file — not watched: {} — {} bytes)",
                big_path.display(),
                big.len()
            )]
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn batch_payload_everything_dropped_yields_zero_included() {
        // Proves the precondition `handle_batch_change` uses to skip the LLM
        // call entirely: when every file in the batch drops (Skip or
        // silent-skip), `total_included` is 0 and the payload is empty.
        let dir = scratch_dir("batch-all-dropped");
        let unchanged = dir.join("unchanged.rs");
        let binary = dir.join("image.png");
        std::fs::write(&binary, [0x89u8, 0x50, 0x4E, 0x47, 0xFF, 0xFE, 0x00, 0x9C]).unwrap();

        let mut files = feedback::FileMemory::new();
        files.seed(&unchanged, "same\n".to_string());
        std::fs::write(&unchanged, "same\n").unwrap();

        let paths = vec![unchanged.clone(), binary.clone()];
        let (payload, meta) = build_batch_payload(&mut files, &dir, &paths);

        assert_eq!(meta.total_included, 0);
        assert!(payload.is_empty());
        assert!(meta.notices.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ride_along_turn_selects_flow_frame_and_keeps_user_words_last() {
        // Companion frame axis (spec K4): ride-along wraps the payload in
        // flow_frame — the plain-review wording must NOT appear. This is the
        // direct-test replacement for the old polite_branch source pin (the
        // frame choice is now pure and testable, so no crude pin is needed).
        let t = ride_along_turn(
            "FILE: src/main.rs (full contents)\nfn main() {}",
            false,
            None,
            "here is my report",
        );
        assert!(t.starts_with(PENDING_PREAMBLE));
        assert!(t.contains("part of the ongoing lesson"));
        assert!(!t.contains("Give project-grounded"));
        // The user's words are the LAST word (spec: Sıralama ve içerik).
        assert!(t.trim_end().ends_with("here is my report"));
    }

    #[tokio::test]
    async fn deliver_pending_rides_payload_before_user_text() {
        let dir = scratch_dir("deliver-pending");
        let file = dir.join("main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let mut files = feedback::FileMemory::new();
        let (notices, outgoing) =
            deliver_pending(&mut files, &dir, &[file], "done, take a look".to_string()).await;
        assert!(notices.is_empty());
        assert!(outgoing.starts_with(PENDING_PREAMBLE));
        let pos_payload = outgoing.find("FILE:").unwrap();
        let pos_user = outgoing.rfind("done, take a look").unwrap();
        assert!(
            pos_payload < pos_user,
            "payload must precede the user's text"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn deliver_pending_everything_dropped_returns_user_text_unchanged() {
        // A vanished file (deleted between hold and delivery) is a silent
        // skip; with nothing left, NO payload is attached (spec:
        // total_included == 0 → payload eklenmez).
        let dir = scratch_dir("deliver-pending-empty");
        let gone = dir.join("gone.rs");
        let mut files = feedback::FileMemory::new();
        let (notices, outgoing) =
            deliver_pending(&mut files, &dir, &[gone], "just a question".to_string()).await;
        assert!(notices.is_empty());
        assert_eq!(outgoing, "just a question");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn deliver_pending_flags_exercise_batches_and_keeps_notices() {
        let dir = scratch_dir("deliver-pending-exercise");
        let ex = dir.join("exercises").join("rust").join("notes.md");
        std::fs::create_dir_all(ex.parent().unwrap()).unwrap();
        std::fs::write(&ex, "my answer\n").unwrap();
        // An oversized companion file exercises the notice channel at delivery
        // (spec: build_batch_payload'ın mevcut notis kanalı korunur).
        let big = dir.join("big.rs");
        std::fs::write(&big, "x".repeat(feedback::MAX_FILE_BYTES + 1)).unwrap();
        let mut files = feedback::FileMemory::new();
        let (notices, outgoing) =
            deliver_pending(&mut files, &dir, &[ex, big], "done".to_string()).await;
        assert_eq!(notices.len(), 1);
        assert!(notices[0].contains("large file"));
        assert!(outgoing.contains("AS AN EXERCISE"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn deliver_pending_collapses_repeated_saves_into_one_diff() {
        // Key property: deliver_pending builds the payload at DELIVERY time,
        // not at flush time. Because of this, multiple saves of the same file
        // between deliveries collapse into ONE diff (from the FileMemory
        // baseline to the final content). Intermediate states never reach the
        // model — they don't appear in the payload.
        let dir = scratch_dir("deliver-pending-collapse");
        let file = dir.join("script.rs");

        // Three distinct states
        let original = "fn greet() {\n    println!(\"original\");\n}\n";
        let intermediate = "fn greet() {\n    println!(\"intermediate\");\n}\n";
        let final_state = "fn greet() {\n    println!(\"final\");\n}\n";

        // Seed FileMemory baseline with the original content
        let mut files = feedback::FileMemory::new();
        files.seed(&file, original.to_string());

        // User saves file twice before delivering: intermediate, then final.
        // Neither save is observed by FileMemory yet — delivery will do it.
        std::fs::write(&file, intermediate).unwrap();
        std::fs::write(&file, final_state).unwrap();

        // Deliver pending; payload is built at delivery time from the final state
        let (notices, outgoing) =
            deliver_pending(&mut files, &dir, &[file], "ready for review".to_string()).await;

        // Sanity checks
        assert!(notices.is_empty());
        assert!(outgoing.contains("FILE:"));
        assert!(outgoing.contains("unified diff"));

        // The critical assertion: diff goes from original to final
        assert!(outgoing.contains("-    println!(\"original\");"));
        assert!(outgoing.contains("+    println!(\"final\");"));

        // The collapse property: intermediate MUST NOT appear in the payload.
        // If deliver_pending built the payload eagerly (after the first save),
        // the intermediate content would be visible here.
        assert!(
            !outgoing.contains("intermediate"),
            "intermediate content must not appear in collapsed diff"
        );

        // User's message is at the end (spec: Sıralama ve içerik)
        assert!(outgoing.trim_end().ends_with("ready for review"));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A minimal, compiling cargo project in a scratch dir — so `run_check`
    /// actually runs instead of short-circuiting on `is_cargo_project`.
    fn scratch_cargo_project(tag: &str) -> std::path::PathBuf {
        let dir = scratch_dir(tag);
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"scratch\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        dir
    }

    #[tokio::test]
    async fn deliver_pending_runs_the_check_for_non_exercise_files() {
        // Prediction protocol (spec §4.6): in the DEFAULT companion mode
        // delivery is the only path a save takes, so if the check doesn't run
        // here it never runs at all — which is what SPEC/README promise.
        let dir = scratch_cargo_project("deliver-pending-check");
        let file = dir.join("src").join("main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let mut files = feedback::FileMemory::new();
        let (_, outgoing) =
            deliver_pending(&mut files, &dir, &[file], "have a look".to_string()).await;
        assert!(
            outgoing.contains("[cargo check result — FOR YOUR EYES ONLY"),
            "a non-exercise delivery must carry the eyes-only check block"
        );
        assert!(outgoing.contains("apply the prediction protocol"));
        // The user still gets the last word — the check block rides in front.
        let pos_check = outgoing.find("[cargo check result").unwrap();
        let pos_user = outgoing.rfind("have a look").unwrap();
        assert!(
            pos_check < pos_user,
            "the check block must precede the user's text"
        );
        assert!(outgoing.trim_end().ends_with("have a look"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn deliver_pending_skips_the_check_for_exercise_only_batches() {
        // Same gate the live path uses: no non-exercise file in the batch,
        // no check — the check doesn't apply to review-only submissions.
        let dir = scratch_cargo_project("deliver-pending-check-exercise");
        let ex = dir.join("exercises").join("answer.md");
        std::fs::create_dir_all(ex.parent().unwrap()).unwrap();
        std::fs::write(&ex, "my answer\n").unwrap();
        let mut files = feedback::FileMemory::new();
        let (_, outgoing) = deliver_pending(&mut files, &dir, &[ex], "done".to_string()).await;
        assert!(outgoing.contains("AS AN EXERCISE"));
        assert!(
            !outgoing.contains("cargo check result"),
            "an exercise-only delivery must not run the check"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn batch_change_skips_llm_call_when_everything_drops() {
        // Everything in the batch drops (unchanged file → Skip) — proves
        // `handle_batch_change` returns `Sessiz` BEFORE touching the backend
        // or session: no injected turn is pushed, no recorder entry is
        // written, so no LLM call happened.
        let dir = scratch_dir("batch-change-no-llm");
        let unchanged = dir.join("unchanged.rs");
        std::fs::write(&unchanged, "same\n").unwrap();
        let mut files = feedback::FileMemory::new();
        files.seed(&unchanged, "same\n".to_string());

        let mut backend = Backend::Cli {
            model: "opus".to_string(),
            session_id: None,
        };
        let mut session = Session::new("rust", "system prompt");
        let recorder_path = dir.join("transcript.jsonl");
        let recorder = transcript::Recorder::new(recorder_path.clone());
        let paths = vec![unchanged.clone()];

        let (notices, result) = handle_batch_change(
            &mut backend,
            &mut session,
            &mut files,
            &dir,
            &paths,
            &recorder,
        )
        .await
        .unwrap();

        assert!(notices.is_empty());
        assert!(matches!(result, FileFeedback::Sessiz));
        assert!(session.history().is_empty());
        assert!(!recorder_path.exists());

        std::fs::remove_dir_all(&dir).ok();
    }
}
