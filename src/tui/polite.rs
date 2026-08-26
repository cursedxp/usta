//! The polite watcher: queues file-change feedback while a mentor question is
//! open, and decides when to flush it. Mostly pure logic; `process_paths` is
//! the one impure piece — the single file-feedback cycle `run.rs` starts from
//! its three flush points, kept here so `run.rs` stays connective tissue.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::EventStream;

use crate::backend::Backend;
use crate::feedback::FileMemory;
use crate::session::Session;
use crate::transcript::Recorder;
use crate::tui::editor::InputBox;
use crate::tui::term::Tui;

/// How long to wait for a keystroke before flushing a queued file-change
/// notice even without an answer (inactivity backstop).
pub(crate) const POLITE_BACKSTOP: Duration = Duration::from_secs(180);

/// Order-preserving, dedup'd queue of file-change paths withheld while a
/// mentor question is open.
pub(crate) struct PoliteQueue {
    paths: Vec<PathBuf>,
    armed_at: Option<tokio::time::Instant>,
}

impl PoliteQueue {
    pub(crate) fn new() -> Self {
        Self {
            paths: Vec::new(),
            armed_at: None,
        }
    }

    /// Pushes `path` if not already queued. Returns `true` when this push
    /// is the first into an empty queue (i.e. it's time to announce).
    pub(crate) fn push(&mut self, path: PathBuf) -> bool {
        let was_empty = self.paths.is_empty();
        let added = !self.paths.contains(&path);
        if added {
            self.paths.push(path);
        }
        if was_empty && added {
            self.armed_at = Some(tokio::time::Instant::now());
        }
        was_empty && added
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// When the queue was armed (first path pushed into an empty queue), or
    /// `None` while it's empty.
    pub(crate) fn armed_at(&self) -> Option<tokio::time::Instant> {
        self.armed_at
    }

    /// Drains all queued paths in order, resetting the queue to empty.
    pub(crate) fn drain(&mut self) -> Vec<PathBuf> {
        self.armed_at = None;
        std::mem::take(&mut self.paths)
    }
}

/// Whether `text` contains an open question (a `?`).
pub(crate) fn question_open(text: &str) -> bool {
    text.contains('?')
}

/// The backstop flush deadline: `None` while the queue is empty, otherwise
/// anchored to whichever is later, the queue's arm time or the last
/// keystroke, plus `POLITE_BACKSTOP` — so the window is never shorter than
/// the time the queue has actually been armed.
pub(crate) fn backstop_deadline(
    armed_at: Option<tokio::time::Instant>,
    last_key: tokio::time::Instant,
) -> Option<tokio::time::Instant> {
    armed_at.map(|a| a.max(last_key) + POLITE_BACKSTOP)
}

/// Whether any line in `text`, trimmed and lowercased, is `watch: live`.
pub(crate) fn live_from_approach(text: &str) -> bool {
    text.lines()
        .any(|l| l.trim().eq_ignore_ascii_case("watch: live"))
}

/// The topic's approach file, project override first — same priority as
/// `slash::topic_has_goal` and brain.rs's GOAL probe. An unreadable or missing
/// file is an empty string, which keeps polite mode on.
pub(crate) fn approach_text(project_root: &Path, global: &Path, topic: &str) -> String {
    let override_path = crate::progress::approach_path(project_root, topic);
    let path = if override_path.exists() {
        override_path
    } else {
        global.join("approaches").join(format!("{topic}.md"))
    };
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Re-baselines `files` for `paths` without asking the mentor — used wherever a
/// batch is skipped, so the next single save still diffs against fresh content.
pub(crate) fn sync_baseline(files: &mut FileMemory, paths: Vec<PathBuf>) {
    for path in paths {
        if let Ok(c) = std::fs::read_to_string(&path) {
            let _ = files.observe(&path, c);
        }
    }
}

/// A question is open — withhold `paths` instead of interrupting. Exactly one
/// notice per queue fill: `push` only reports the first path into an empty queue.
pub(crate) fn queue_batch(tui: &mut Tui, pq: &mut PoliteQueue, paths: Vec<PathBuf>) -> Result<()> {
    for path in paths {
        if pq.push(path) {
            crate::tui::page::page_notice(tui, "change noticed — feedback after your answer")?;
        }
    }
    Ok(())
}

/// Too many files at once: say so, skip the LLM feedback, sync the baseline.
pub(crate) fn bulk_skip(tui: &mut Tui, files: &mut FileMemory, paths: Vec<PathBuf>) -> Result<()> {
    crate::tui::page::page_notice(
        tui,
        &format!(
            "bulk change ({} files) — feedback skipped, still watching",
            paths.len()
        ),
    )?;
    sync_baseline(files, paths);
    Ok(())
}

/// The file-feedback cycle, shared by the three points that can start one: the
/// watcher's debounce flush, the flush after the user's answer, and the
/// inactivity backstop. Over `max_feedback_batch` paths it degrades to
/// `bulk_skip` — the same rule the live path applies, different source of paths.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn process_paths(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
    backend: &mut Backend,
    session: &mut Session,
    files: &mut FileMemory,
    recorder: &Recorder,
    project_root: &Path,
    topic: &str,
    last_tokens: &mut Option<u64>,
    question_open: &mut bool,
    paths: Vec<PathBuf>,
    max_feedback_batch: usize,
) -> Result<()> {
    if paths.len() > max_feedback_batch {
        return bulk_skip(tui, files, paths);
    }
    for path in paths {
        match crate::file_feedback::handle_file_change(
            backend,
            session,
            files,
            project_root,
            &path,
            recorder,
        )
        .await
        {
            Ok(crate::file_feedback::FileFeedback::Sessiz) => {}
            Ok(crate::file_feedback::FileFeedback::Bildirim(m)) => {
                crate::tui::page::page_notice(tui, &m)?
            }
            Ok(crate::file_feedback::FileFeedback::Yanit {
                tokens,
                reply,
                show_topic,
            }) => {
                // A feedback reply can end with a question too — keep the gate honest.
                *question_open = self::question_open(&reply.text);
                if let Some(t) = tokens {
                    *last_tokens = Some(t);
                }
                let w = crate::tui::page::current_width(tui);
                crate::tui::page::page_reply(tui, &reply.text, w)?;
                crate::lifecycle::maybe_compact(backend, session, project_root, tokens).await;
                crate::tui::entry::trigger_auto_visual(
                    tui,
                    editor,
                    events,
                    backend,
                    session,
                    project_root,
                    topic,
                    show_topic,
                    *last_tokens,
                )
                .await?;
            }
            // Same silent-skip classes as the plain path (plain.rs) /
            // is_silent_skip (file_feedback.rs): vanished temp file
            // (NotFound) or binary content (InvalidData) — no noise for either.
            Err(e) if crate::file_feedback::is_silent_skip(&e) => {}
            Err(e) => crate::tui::page::page_error(
                tui,
                &format!("file feedback skipped: {}: {e}", path.display()),
            )?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn queue_push_dedups_and_preserves_order() {
        let mut q = PoliteQueue::new();
        assert!(q.push(PathBuf::from("a.rs"))); // first push into empty queue → announce
        assert!(!q.push(PathBuf::from("b.rs"))); // queue already non-empty → silent
        assert!(!q.push(PathBuf::from("a.rs"))); // duplicate → silent, not re-added
        assert_eq!(
            q.drain(),
            vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")]
        );
        assert!(q.is_empty());
        assert!(q.push(PathBuf::from("a.rs"))); // empty again → announce again
    }

    #[test]
    fn question_open_detects_question_mark() {
        assert!(question_open("What does ownership mean?"));
        assert!(question_open("Try it in parallel. What happens?"));
        assert!(!question_open("Good. Keep going."));
        assert!(!question_open(""));
    }

    #[test]
    fn backstop_deadline_only_when_queue_nonempty() {
        let now = tokio::time::Instant::now();
        assert_eq!(backstop_deadline(None, now), None);
        assert_eq!(
            backstop_deadline(Some(now), now),
            Some(now + POLITE_BACKSTOP)
        );
    }

    #[test]
    fn queue_stamps_armed_at_on_first_push_and_clears_on_drain() {
        let mut q = PoliteQueue::new();
        assert_eq!(q.armed_at(), None);
        let before = tokio::time::Instant::now();
        q.push(std::path::PathBuf::from("a.rs"));
        let armed = q.armed_at().expect("armed after first push");
        assert!(armed >= before);
        q.push(std::path::PathBuf::from("b.rs"));
        assert_eq!(q.armed_at(), Some(armed)); // second push does not re-stamp
        q.drain();
        assert_eq!(q.armed_at(), None);
    }

    #[test]
    fn backstop_window_never_shorter_than_arm_time() {
        let last_key = tokio::time::Instant::now();
        let armed = last_key + POLITE_BACKSTOP; // user idle 180s, THEN saves a file
                                                // Old bug: deadline = last_key + 180 = already past → fired immediately.
        assert_eq!(
            backstop_deadline(Some(armed), last_key),
            Some(armed + POLITE_BACKSTOP)
        );
        // Typing after the queue armed still extends the window:
        let late_key = armed + std::time::Duration::from_secs(10);
        assert_eq!(
            backstop_deadline(Some(armed), late_key),
            Some(late_key + POLITE_BACKSTOP)
        );
        assert_eq!(backstop_deadline(None, last_key), None);
    }

    #[test]
    fn approach_text_prefers_project_override_and_tolerates_missing() {
        let base =
            std::env::temp_dir().join(format!("usta_polite_approach_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let project = base.join("proj");
        let global = base.join("global");
        std::fs::create_dir_all(project.join(".usta/approaches")).unwrap();
        std::fs::create_dir_all(global.join("approaches")).unwrap();

        // no file at all → empty (so live_from_approach is false → polite stays on)
        assert_eq!(approach_text(&project, &global, "rust"), "");

        // only global → global content
        std::fs::write(global.join("approaches/rust.md"), "watch: live\n").unwrap();
        assert_eq!(approach_text(&project, &global, "rust"), "watch: live\n");

        // project override wins, even when it drops the line
        std::fs::write(project.join(".usta/approaches/rust.md"), "# local\n").unwrap();
        assert_eq!(approach_text(&project, &global, "rust"), "# local\n");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn live_from_approach_matches_watch_live_line() {
        assert!(live_from_approach("# JS\n\nwatch: live\n"));
        assert!(live_from_approach("  WATCH: LIVE  \n")); // forgiving case/whitespace
        assert!(!live_from_approach("watch: polite\n")); // unknown value → default
        assert!(!live_from_approach("# nothing here\n"));
        assert!(!live_from_approach(""));
    }
}
