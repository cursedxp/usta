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

/// The four ways a flushed file-change batch can be handled — decided once,
/// up front, so `run.rs` only matches on the outcome.
#[derive(Debug, PartialEq)]
pub(crate) enum Route {
    /// Too many files at once — feedback skipped, baseline still synced.
    Bulk,
    /// Companion off — baseline synced, no LLM feedback.
    ObserveOnly,
    /// Polite mode with a question open — withhold until the user answers.
    Queue,
    /// Give feedback now.
    Feedback,
}

/// Picks the route for a flushed batch. Order matters: bulk and
/// companion-off gates come before the polite queue, same as the run loop's
/// original if/else chain — a bulk save is skipped, never queued.
pub(crate) fn route(
    batch_len: usize,
    max_batch: usize,
    watching: bool,
    polite: bool,
    question_open: bool,
) -> Route {
    if batch_len > max_batch {
        Route::Bulk
    } else if !watching {
        Route::ObserveOnly
    } else if polite && question_open {
        Route::Queue
    } else {
        Route::Feedback
    }
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

/// `/watch off` must silence pending feedback too, not just future feedback:
/// drain the queue (which also disarms the backstop) and sync the diff
/// baseline for whatever was withheld, same treatment as any other skipped
/// batch. No-op while still watching, or while the queue is empty.
pub(crate) fn silence_queue_on_watch_off(
    watching: bool,
    pq: &mut PoliteQueue,
    files: &mut FileMemory,
) {
    if !watching && !pq.is_empty() {
        sync_baseline(files, pq.drain());
    }
}

/// Same drop as `silence_queue_on_watch_off`, plus a user-facing notice —
/// unlike `/watch polite off`, which delivers the queue instead of dropping
/// it (`deliver_queue_on_polite_off`), `/watch off` discards it, so the user
/// needs to be told. No-op notice when there was nothing pending to drop.
pub(crate) fn silence_queue_on_watch_off_with_notice(
    tui: &mut Tui,
    watching: bool,
    pq: &mut PoliteQueue,
    files: &mut FileMemory,
) -> Result<()> {
    let had_pending = !watching && !pq.is_empty();
    silence_queue_on_watch_off(watching, pq, files);
    if had_pending {
        crate::tui::page::page_notice(tui, "(pending feedback dropped)")?;
    }
    Ok(())
}

/// Whether turning polite off should also flush the withheld queue right now:
/// only when polite just turned off and something is actually queued. Split
/// out from `deliver_queue_on_polite_off` so the gate is unit-testable on its
/// own, same shape as `silence_queue_on_watch_off`'s off-check.
pub(crate) fn should_deliver_queue_on_polite_off(polite: bool, pq: &PoliteQueue) -> bool {
    !polite && !pq.is_empty()
}

/// Whether turning polite off should print the "delivering queued feedback"
/// notice: only when `process_paths` will actually deliver the queue rather
/// than degrade to `bulk_skip`, which prints its own, truthful notice instead.
pub(crate) fn polite_off_delivery_notice(
    queue_len: usize,
    max_batch: usize,
) -> Option<&'static str> {
    if queue_len == 0 || queue_len > max_batch {
        None
    } else {
        Some("polite mode off — delivering queued feedback")
    }
}

/// Turning polite off means "give me instant feedback now" (spec v0.24.2 G1):
/// deliver the withheld queue immediately instead of stranding it. No-op
/// unless `should_deliver_queue_on_polite_off` says so. Returns whether it
/// delivered, so the caller can skip its normal mode-change notice on that
/// path — this path prints its own.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn deliver_queue_on_polite_off(
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
    polite: bool,
    pq: &mut PoliteQueue,
    max_feedback_batch: usize,
) -> Result<bool> {
    if !should_deliver_queue_on_polite_off(polite, pq) {
        return Ok(false);
    }
    let paths = pq.drain();
    if let Some(notice) = polite_off_delivery_notice(paths.len(), max_feedback_batch) {
        crate::tui::page::page_notice(tui, notice)?;
    }
    process_paths(
        tui,
        editor,
        events,
        backend,
        session,
        files,
        recorder,
        project_root,
        topic,
        last_tokens,
        question_open,
        paths,
        max_feedback_batch,
    )
    .await?;
    Ok(true)
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

/// Drains `pq`'s withheld paths into `paths` so a bulk sync absorbs them too
/// — merging first means the queue empties and `armed_at` clears in the same
/// step, so nothing is left stranded mid-queue past the bulk baseline sync.
pub(crate) fn absorb_queue_into_batch(
    pq: &mut PoliteQueue,
    mut paths: Vec<PathBuf>,
) -> Vec<PathBuf> {
    paths.extend(pq.drain());
    paths
}

/// `Route::Bulk`, but folds in whatever `pq` was withholding first (H1): a
/// path queued behind an open question must not have its baseline synced
/// while it's still sitting in the queue, or the eventual drain would find
/// `ChangePayload::Skip` and the promised feedback would silently never arrive.
pub(crate) fn bulk_skip_absorbing_queue(
    tui: &mut Tui,
    files: &mut FileMemory,
    pq: &mut PoliteQueue,
    paths: Vec<PathBuf>,
) -> Result<()> {
    bulk_skip(tui, files, absorb_queue_into_batch(pq, paths))
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
    fn silence_queue_on_watch_off_drains_only_when_off_and_nonempty() {
        // Real file on disk (same temp-dir pattern as
        // approach_text_prefers_project_override_and_tolerates_missing above)
        // so sync_baseline's std::fs::read_to_string actually succeeds and
        // files.observe is exercised, not silently skipped.
        let path =
            std::env::temp_dir().join(format!("usta_polite_silence_{}.rs", std::process::id()));
        let content = "fn main() {}";
        std::fs::write(&path, content).unwrap();

        let mut files = FileMemory::new();
        let mut pq = PoliteQueue::new();
        pq.push(path.clone());

        // Still watching — queue is left alone.
        silence_queue_on_watch_off(true, &mut pq, &mut files);
        assert!(!pq.is_empty());
        assert!(pq.armed_at().is_some());

        // Watch off — the queue drains, which also disarms the backstop.
        silence_queue_on_watch_off(false, &mut pq, &mut files);
        assert!(pq.is_empty());
        assert!(pq.armed_at().is_none());

        // Baseline was actually recorded during the drain: observing the same
        // content again reports "no change", not "first sight". This only
        // holds if sync_baseline's files.observe call ran on the drained path.
        assert!(matches!(
            files.observe(&path, content.to_string()),
            crate::feedback::ChangePayload::Skip
        ));

        // Already empty + off — no-op, doesn't panic or re-arm.
        silence_queue_on_watch_off(false, &mut pq, &mut files);
        assert!(pq.is_empty());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn bulk_route_absorbs_pending_queue() {
        // Real temp files (same pattern as
        // silence_queue_on_watch_off_drains_only_when_off_and_nonempty above)
        // so sync_baseline's std::fs::read_to_string actually succeeds and
        // files.observe is exercised, not silently skipped.
        let queued_path = std::env::temp_dir().join(format!(
            "usta_polite_bulk_absorb_queued_{}.rs",
            std::process::id()
        ));
        let queued_content = "fn queued() {}";
        std::fs::write(&queued_path, queued_content).unwrap();

        let bulk_path = std::env::temp_dir().join(format!(
            "usta_polite_bulk_absorb_bulk_{}.rs",
            std::process::id()
        ));
        let bulk_content = "fn bulk() {}";
        std::fs::write(&bulk_path, bulk_content).unwrap();

        let mut files = FileMemory::new();
        let mut pq = PoliteQueue::new();
        pq.push(queued_path.clone());
        assert!(pq.armed_at().is_some());

        // A same-tick bulk batch arrives, independent of the pending queue.
        let batch = vec![bulk_path.clone()];
        let merged = absorb_queue_into_batch(&mut pq, batch);

        // The queue is absorbed, not stranded: it's empty and disarmed.
        assert!(pq.is_empty());
        assert!(pq.armed_at().is_none());
        assert!(merged.contains(&queued_path));
        assert!(merged.contains(&bulk_path));

        // What bulk_skip does with the merged paths: sync the baseline for
        // all of them, queued path included.
        sync_baseline(&mut files, merged);

        // Baseline was actually recorded for the queued path during the bulk
        // sync: observing the same content again reports "no change", not a
        // stale diff — the promised feedback isn't stranded behind a synced
        // baseline that the queue never got credit for.
        assert!(matches!(
            files.observe(&queued_path, queued_content.to_string()),
            crate::feedback::ChangePayload::Skip
        ));

        let _ = std::fs::remove_file(&queued_path);
        let _ = std::fs::remove_file(&bulk_path);
    }

    #[test]
    fn should_deliver_queue_on_polite_off_only_when_off_and_nonempty() {
        let mut pq = PoliteQueue::new();
        // Empty queue — nothing to deliver, even when turning off.
        assert!(!should_deliver_queue_on_polite_off(false, &pq));
        pq.push(PathBuf::from("a.rs"));
        // Still polite (on) — no delivery even though something is queued.
        assert!(!should_deliver_queue_on_polite_off(true, &pq));
        // Off + non-empty — deliver now instead of stranding the queue.
        assert!(should_deliver_queue_on_polite_off(false, &pq));
    }

    #[test]
    fn polite_off_notice_only_when_queue_actually_deliverable() {
        assert_eq!(polite_off_delivery_notice(0, 10), None);
        assert_eq!(
            polite_off_delivery_notice(3, 10),
            Some("polite mode off — delivering queued feedback")
        );
        assert_eq!(polite_off_delivery_notice(11, 10), None); // bulk skip will tell the truth
        assert!(polite_off_delivery_notice(10, 10).is_some()); // boundary: exactly max delivers
    }

    #[test]
    fn live_from_approach_matches_watch_live_line() {
        assert!(live_from_approach("# JS\n\nwatch: live\n"));
        assert!(live_from_approach("  WATCH: LIVE  \n")); // forgiving case/whitespace
        assert!(!live_from_approach("watch: polite\n")); // unknown value → default
        assert!(!live_from_approach("# nothing here\n"));
        assert!(!live_from_approach(""));
    }

    #[test]
    fn route_truth_table() {
        use Route::*;
        // bulk wins over everything
        assert_eq!(route(11, 10, true, true, true), Bulk);
        assert_eq!(route(11, 10, false, false, false), Bulk);
        // watching off → observe only, regardless of polite/question
        assert_eq!(route(1, 10, false, true, true), ObserveOnly);
        assert_eq!(route(1, 10, false, false, false), ObserveOnly);
        // polite + open question → queue
        assert_eq!(route(1, 10, true, true, true), Queue);
        // polite but no open question → instant feedback
        assert_eq!(route(1, 10, true, true, false), Feedback);
        // live mode ignores question state
        assert_eq!(route(1, 10, true, false, true), Feedback);
        assert_eq!(route(1, 10, true, false, false), Feedback);
        // boundary: exactly max is NOT bulk (existing `>` comparison)
        assert_eq!(route(10, 10, true, false, false), Feedback);
    }
}
