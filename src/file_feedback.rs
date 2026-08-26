//! File-watch feedback path: decides whether a changed file is worth sending
//! to the model, and what frame to send it in — extracted from `main.rs`
//! (module split, Task 6). Name note: `feedback.rs` (FileMemory/diffing) and
//! `watcher.rs` (fs events) already exist; this is the layer above both.

use std::path::Path;

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
            "[Exercise submission saved: {path_display}]\n{body}\n\nThis is the user's deliverable for the exercise you assigned. Review it AS AN EXERCISE: compare against the assignment, apply the hint ladder (start high), point at what to reconsider — do NOT rewrite or complete it for them. If no exercise was assigned this session, treat it as spontaneous practice work and review it the same way."
        ),
        (true, true) => format!(
            "[Exercise submission changed: {path_display}]\nChange (unified diff):\n{body}\n\nReview the revision AS AN EXERCISE iteration: did it address your previous feedback? Move one rung down the hint ladder only if they're stuck — never hand over the solution."
        ),
    }
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
            injected.push_str(&format!(
                "\n\n[cargo check result — FOR YOUR EYES ONLY, don't pass this directly to the user; apply the prediction protocol]\n{check_result}"
            ));
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
}
