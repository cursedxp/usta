//! The watcher's decision layer: which route a flushed debounce batch takes,
//! and what happens on each route. The watcher NEVER initiates an LLM turn on
//! its own (spec K1, no exceptions): the companion default HOLDS flushed
//! batches in `PendingChanges` and delivers them with the user's next submit
//! (`attach_pending` — ride along, spec K2). An immediate turn at flush exists
//! only as the user's explicit choice (`live`: `/watch live` or a
//! `watch: live` approach line), framed as plain review. Mostly pure logic;
//! `dispatch_flush` and `process_batch` are the impure pieces, kept here so
//! `run.rs` stays connective tissue.

use std::path::{Path, PathBuf};

use anyhow::Result;
use crossterm::event::EventStream;

use crate::backend::Backend;
use crate::feedback::FileMemory;
use crate::session::Session;
use crate::transcript::Recorder;
use crate::tui::editor::InputBox;
use crate::tui::term::Tui;

/// The four ways a flushed file-change batch can be handled — decided once,
/// up front, so the dispatcher only matches on the outcome.
#[derive(Debug, PartialEq)]
pub(crate) enum Route {
    /// Too many files at once — feedback skipped, baseline still synced.
    Bulk,
    /// Companion off — baseline synced, no LLM feedback, nothing accumulates.
    ObserveOnly,
    /// Live mode (explicit user choice): give feedback now, plain review frame.
    Feedback,
    /// Companion default: hold — paths accumulate in `PendingChanges` and
    /// ride along with the user's next submit. No turn (spec K1).
    Hold,
}

/// Picks the route for a flushed batch. Order matters, same as the original
/// if/else chain: a bulk save is skipped before the watching gate, and the
/// watching gate before the timing axis. `live` selects timing (spec K4):
/// an immediate turn only on the user's explicit say-so — the default is
/// Hold, because the watcher never initiates (spec K1).
pub(crate) fn route(batch_len: usize, max_batch: usize, watching: bool, live: bool) -> Route {
    if batch_len > max_batch {
        Route::Bulk
    } else if !watching {
        Route::ObserveOnly
    } else if live {
        Route::Feedback
    } else {
        Route::Hold
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
/// first, in `paths` order, then the reply. Only the live path reaches here,
/// and live is plain review by definition (spec K4) — the companion frame
/// travels with ride-along delivery instead (attach_pending).
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
) -> Result<()> {
    match crate::file_feedback::handle_batch_change(
        backend,
        session,
        files,
        project_root,
        paths,
        recorder,
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

/// The single flush entry point `run.rs` calls from the debounce deadline arm:
/// route the batch, then act — so the whole watcher policy lives here and
/// run.rs keeps one thin call site (its 600-line budget is why). Bulk and
/// observe-only are unchanged; a bulk batch never enters `PendingChanges`
/// (spec: Kenar durumlar), so the cap keeps meaning.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn dispatch_flush(
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
    batch: Vec<PathBuf>,
    max_batch: usize,
    watching: bool,
    live: bool,
    pending: &mut PendingChanges,
) -> Result<()> {
    match route(batch.len(), max_batch, watching, live) {
        Route::Bulk => bulk_skip(tui, files, batch)?,
        Route::ObserveOnly => sync_baseline(files, batch),
        // Live mode — the user's explicit timing choice: immediate turn.
        Route::Feedback => {
            process_batch(
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
                &batch,
            )
            .await?
        }
        // Companion default: accumulate; delivery rides the next submit (K2).
        Route::Hold => pending.hold(batch),
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
        // bulk wins over everything, watching/live or not
        assert_eq!(route(11, 10, true, false), Bulk);
        assert_eq!(route(11, 10, true, true), Bulk);
        assert_eq!(route(11, 10, false, false), Bulk);
        // watching off → observe only, live or not
        assert_eq!(route(1, 10, false, false), ObserveOnly);
        assert_eq!(route(1, 10, false, true), ObserveOnly);
        // watching on, within the limit: live → immediate feedback,
        // companion default → hold, NEVER a turn (spec K1)
        assert_eq!(route(1, 10, true, true), Feedback);
        assert_eq!(route(1, 10, true, false), Hold);
        assert_eq!(route(5, 10, true, false), Hold);
        // boundary: exactly max is NOT bulk (existing `>` comparison)
        assert_eq!(route(10, 10, true, false), Hold);
        assert_eq!(route(10, 10, true, true), Feedback);
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
        // v0.28.0: the four route arms moved into polite::dispatch_flush, so
        // the old per-arm needles went vacuous — replaced by needles for the
        // two new call sites plus the state wiring that feeds them. The arms
        // themselves are pinned by dispatch_flush_route_arms_are_pinned.
        let src = include_str!("run.rs");
        for needle in [
            "polite::approach_text(",
            "polite::live_from_approach(",
            "polite::PendingChanges::new(",
            "polite::dispatch_flush(",
            "polite::attach_pending(",
        ] {
            assert!(
                src.contains(needle),
                "run.rs lost its watcher wiring: {needle}"
            );
        }
    }

    #[test]
    fn dispatch_flush_route_arms_are_pinned() {
        // dispatch_flush needs a live Backend + Tui, so its arms can't be
        // driven from a unit test (same class as
        // run_rs_wiring_call_sites_are_pinned): pin this file's own
        // production source, split at the test module so this assert's own
        // text can't match itself. Deleting an arm's body would otherwise
        // leave the suite green while a whole route silently died — the
        // exact failure class these pins exist for.
        let production_src = include_str!("polite.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        for needle in [
            "match route(batch.len()",
            "Route::Bulk => bulk_skip(",
            "Route::ObserveOnly => sync_baseline(",
            "Route::Hold => pending.hold(",
        ] {
            assert!(
                production_src.contains(needle),
                "dispatch_flush lost an arm: {needle}"
            );
        }
        // process_batch appears once as its own definition; the second
        // occurrence is dispatch_flush's live-arm call.
        assert!(
            production_src.matches("process_batch(").count() >= 2,
            "dispatch_flush no longer calls process_batch on the live route"
        );
    }

    #[test]
    fn ride_along_attaches_only_to_genuine_user_text() {
        // run.rs's submit arm builds one `outgoing` binding from three
        // branches: the user's own typed line, /exam's synthesized exam
        // prompt, and /game's mode directives. Pending file changes may ride
        // along ONLY with the user's own words — in front of an operational
        // directive they would feed file feedback into an exam (where it is
        // suspended by design) or into a game toggle, and neither is a report
        // on the learner's work. The /exam and /game branches must therefore
        // leave the queue intact, so the changes deliver on the next real
        // message. Crude source pin, same class as
        // run_rs_wiring_call_sites_are_pinned: the branch structure is inside
        // a TUI select! loop that can't be driven from a unit test.
        let src = include_str!("run.rs");
        assert!(
            src.contains(
                "attach_pending(&mut tui, &mut pending, &mut files, project_root, line.clone())"
            ),
            "ride-along must take the user's own line, not the shared `outgoing` binding"
        );
        assert!(
            !src.contains("attach_pending(&mut tui, &mut pending, &mut files, project_root, outgoing)"),
            "ride-along must not wrap `outgoing`: that attaches pending changes to /exam and /game directives"
        );
        // The two synthesized-directive branches still build their text
        // directly — nothing drains `pending` on the way.
        assert!(
            src.contains("progress::exam_prompt(&topic)"),
            "/exam must build its directive directly, leaving pending changes queued"
        );
        assert!(
            src.contains("crate::slash::game_on_turn("),
            "/game must build its directive directly, leaving pending changes queued"
        );
    }
}
