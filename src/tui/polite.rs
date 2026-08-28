//! The watcher's decision layer: which route a flushed debounce batch takes,
//! and the feedback turn itself. Mostly pure logic; `process_batch` is the one
//! impure piece — the single file-feedback cycle `run.rs` starts from the
//! watcher's debounce flush, kept here so `run.rs` stays connective tissue.
//!
//! Timing is uniform: every batch is handled the moment it flushes. `polite`
//! is a prompt-frame switch, not a delay — see `process_batch`.

use std::path::{Path, PathBuf};

use anyhow::Result;
use crossterm::event::EventStream;

use crate::backend::Backend;
use crate::feedback::FileMemory;
use crate::session::Session;
use crate::transcript::Recorder;
use crate::tui::editor::InputBox;
use crate::tui::term::Tui;

/// The three ways a flushed file-change batch can be handled — decided once,
/// up front, so `run.rs` only matches on the outcome.
#[derive(Debug, PartialEq)]
pub(crate) enum Route {
    /// Too many files at once — feedback skipped, baseline still synced.
    Bulk,
    /// Companion off — baseline synced, no LLM feedback.
    ObserveOnly,
    /// Give feedback now.
    Feedback,
}

/// Picks the route for a flushed batch. Order matters, same as the run loop's
/// original if/else chain: a bulk save is skipped before the companion-off
/// gate is consulted. `polite` is deliberately NOT an input — it selects the
/// prompt frame inside `process_batch`, it never delays or withholds a batch.
pub(crate) fn route(batch_len: usize, max_batch: usize, watching: bool) -> Route {
    if batch_len > max_batch {
        Route::Bulk
    } else if !watching {
        Route::ObserveOnly
    } else {
        Route::Feedback
    }
}

/// Accumulated-but-undelivered watcher batches (spec K2): only PATHS are
/// held — the payload is built at delivery time via
/// `file_feedback::deliver_pending`, so intermediate saves collapse into one
/// diff. Order preserved, repeats collapsed. `len` feeds the status line's
/// deterministic counter (spec K3); `take` drains, which is also the counter
/// reset.
#[derive(Default)]
pub(crate) struct PendingChanges {
    paths: Vec<PathBuf>,
}

#[allow(dead_code)] // staged: consumed by the timing-flip task
impl PendingChanges {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Accumulate a flushed batch — order preserved, duplicates collapsed.
    pub(crate) fn hold(&mut self, batch: Vec<PathBuf>) {
        for p in batch {
            if !self.paths.contains(&p) {
                self.paths.push(p);
            }
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.paths.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// Drain for delivery — resets the counter (spec K3).
    pub(crate) fn take(&mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.paths)
    }
}

/// Ride-along delivery at the user's submit (spec K2): with nothing pending
/// the user's text passes through untouched; otherwise the pending paths are
/// drained (counter reset, spec K3), the payload is built at THIS moment, its
/// notices are printed, and the combined turn — file block first, the user's
/// words last — is returned for the normal ask path. Deterministic shell
/// work; no LLM call here.
#[allow(dead_code)] // staged: consumed by the timing-flip task
pub(crate) fn attach_pending(
    tui: &mut Tui,
    pending: &mut PendingChanges,
    files: &mut FileMemory,
    project_root: &Path,
    user_text: String,
) -> Result<String> {
    if pending.is_empty() {
        return Ok(user_text);
    }
    let paths = pending.take();
    let (notices, outgoing) =
        crate::file_feedback::deliver_pending(files, project_root, &paths, user_text);
    for notice in &notices {
        crate::tui::page::page_notice(tui, notice)?;
    }
    Ok(outgoing)
}

/// Whether any line in `text`, trimmed and lowercased, is `watch: live`.
/// Selects the timing axis (spec K4): live = immediate feedback.
pub(crate) fn live_from_approach(text: &str) -> bool {
    text.lines()
        .any(|l| l.trim().eq_ignore_ascii_case("watch: live"))
}

/// The topic's approach file, project override first — same priority as
/// `slash::topic_has_goal` and brain.rs's GOAL probe. An unreadable or missing
/// file is an empty string, which keeps live off (companion default).
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

/// The file-feedback cycle for one flushed debounce batch: `handle_batch_change`
/// merges every changed file into ONE injected turn and makes ONE LLM call,
/// and this function presents the result — the per-file notices it returns
/// first, in `paths` order, then the reply. `polite` picks the prompt frame
/// there: the lesson-flow companion frame when on, plain review when off.
///
/// No batch-size check here: `Route::Feedback` is the only way in, and `route`
/// has already sent anything over `max_feedback_batch` to `bulk_skip`. The
/// silent-skip classes (vanished temp file, binary content) are classified
/// per file inside `handle_batch_change`, so an error out here is the LLM
/// call failing, not a file read.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn process_batch(
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
    paths: &[PathBuf],
    polite: bool,
) -> Result<()> {
    match crate::file_feedback::handle_batch_change(
        backend,
        session,
        files,
        project_root,
        paths,
        recorder,
        polite,
    )
    .await
    {
        Ok((notices, feedback)) => {
            for notice in &notices {
                crate::tui::page::page_notice(tui, notice)?;
            }
            match feedback {
                crate::file_feedback::FileFeedback::Sessiz => {}
                // Never returned by `handle_batch_change` (its notices come
                // back in the tuple's first slot) — handled so the match
                // stays exhaustive over the enum the single-file path shares.
                crate::file_feedback::FileFeedback::Bildirim(m) => {
                    crate::tui::page::page_notice(tui, &m)?
                }
                crate::file_feedback::FileFeedback::Yanit {
                    tokens,
                    reply,
                    show_topic,
                } => {
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
            }
        }
        Err(e) => crate::tui::page::page_error(tui, &format!("file feedback skipped: {e}"))?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn sync_baseline_records_content_so_the_next_save_diffs() {
        // Real file on disk (same temp-dir pattern as
        // approach_text_prefers_project_override_and_tolerates_missing above)
        // so sync_baseline's std::fs::read_to_string actually succeeds and
        // files.observe is exercised, not silently skipped. This is what
        // `Route::ObserveOnly` and `bulk_skip` both rely on.
        let path = std::env::temp_dir().join(format!("usta_polite_sync_{}.rs", std::process::id()));
        let content = "fn main() {}";
        std::fs::write(&path, content).unwrap();

        let mut files = FileMemory::new();
        sync_baseline(&mut files, vec![path.clone()]);

        // Observing the same content again reports "no change", not "first
        // sight" — only true if files.observe ran during the sync.
        assert!(matches!(
            files.observe(&path, content.to_string()),
            crate::feedback::ChangePayload::Skip
        ));

        // A missing path is tolerated, not a panic.
        sync_baseline(&mut files, vec![path.join("does-not-exist.rs")]);

        let _ = std::fs::remove_file(&path);
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
        // bulk wins over everything, watching or not
        assert_eq!(route(11, 10, true), Bulk);
        assert_eq!(route(11, 10, false), Bulk);
        // watching off → observe only
        assert_eq!(route(1, 10, false), ObserveOnly);
        assert_eq!(route(0, 10, false), ObserveOnly);
        // watching on, within the limit → immediate feedback, always
        assert_eq!(route(1, 10, true), Feedback);
        assert_eq!(route(5, 10, true), Feedback);
        // boundary: exactly max is NOT bulk (existing `>` comparison)
        assert_eq!(route(10, 10, true), Feedback);
    }

    #[test]
    fn pending_changes_dedup_preserve_order_and_reset_on_take() {
        let mut p = PendingChanges::new();
        assert!(p.is_empty());
        assert_eq!(p.len(), 0);
        p.hold(vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")]);
        p.hold(vec![PathBuf::from("a.rs"), PathBuf::from("c.rs")]);
        assert_eq!(p.len(), 3);
        // Order preserved, repeats collapsed (spec: Davranış/Akış step 1).
        assert_eq!(
            p.take(),
            vec![
                PathBuf::from("a.rs"),
                PathBuf::from("b.rs"),
                PathBuf::from("c.rs")
            ]
        );
        // take() drains — the status counter resets with it (spec K3).
        assert!(p.is_empty());
        assert!(p.take().is_empty());
    }

    #[test]
    fn run_rs_wiring_call_sites_are_pinned() {
        // Crude source pin, not a unit test: the TUI loop in run.rs is bound
        // to Terminal<TrackedBackend<Stdout>> and can't be driven directly
        // from a test. Two release rounds in a row a reviewer found that
        // deleting the watcher wiring from run.rs left the whole suite green,
        // because everything below the call sites is unit-tested but the
        // call sites themselves were not. This guards against that class of
        // silent deletion by asserting the wiring is still called from
        // run.rs's source text. Every needle names a symbol run.rs itself
        // calls — a needle for a symbol living elsewhere would be a fake pin.
        let src = include_str!("run.rs");
        for needle in [
            "polite::route(",
            "polite::bulk_skip(",
            "polite::sync_baseline(",
            "polite::process_batch(",
        ] {
            assert!(
                src.contains(needle),
                "run.rs lost its watcher wiring: {needle}"
            );
        }
    }
}
