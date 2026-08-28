//! The watcher's decision layer: which route a flushed debounce batch takes,
//! and what happens on each route. The watcher NEVER initiates an LLM turn on
//! its own (spec K1; TUI path only — the plain/pipe path in `plain.rs` still
//! opens an immediate turn per save, a deliberate exception): the companion
//! default HOLDS flushed
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

/// Flush-time split of a debounced batch into CONTENT paths (existing
/// files — the payload pipeline as before) and STRUCTURE notes (spec D1):
/// new directories, deleted known files, removed known directories. Paths
/// that vanished without ever being known stay silent — the transient-temp
/// class is_silent_skip swallows today. Deterministic; existence is probed
/// HERE, once, so classification can't rot between event and delivery. The
/// tracker updates even while watching is off, so re-enabling never
/// misreports old directories as new.
pub(crate) fn classify_flush(
    batch: Vec<PathBuf>,
    tracker: &mut crate::watcher::StructureTracker,
    files: &FileMemory,
    project_root: &Path,
) -> (Vec<PathBuf>, Vec<String>) {
    let mut content = Vec::new();
    let mut notes = Vec::new();
    for path in batch {
        let rel = path
            .strip_prefix(project_root)
            .unwrap_or(&path)
            .display()
            .to_string();
        if path.is_dir() {
            if tracker.note_new_dir(&path) {
                notes.push(format!("+ {rel}/ (new directory)"));
            }
        } else if path.is_file() {
            content.push(path);
        } else if tracker.note_removed(&path) {
            notes.push(format!("- {rel}/ (directory removed)"));
        } else if files.knows(&path) {
            notes.push(format!("- {rel} (deleted)"));
        }
        // else: a vanished path nobody ever knew — transient noise, silent.
    }
    (content, notes)
}

/// Cap on held structure notes — a branch switch can delete hundreds of
/// files; past this the rest collapses into one overflow line at take().
pub(crate) const MAX_STRUCTURE_NOTES: usize = 20;

/// Accumulated-but-undelivered watcher batches (spec K2) plus structure
/// notes (spec D2): only PATHS and one-line notes are held — file payloads
/// are built at delivery time via `file_feedback::deliver_pending`, so
/// intermediate saves collapse into one diff, and directory CONTENTS are
/// never sent at all. Order preserved, repeats collapsed. `len` feeds the
/// status line's deterministic counter (spec K3) and counts paths, notes
/// and suppressed-overflow alike; `take` drains everything, which is also
/// the counter reset.
#[derive(Default)]
pub(crate) struct PendingChanges {
    paths: Vec<PathBuf>,
    notes: Vec<String>,
    suppressed: usize,
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

    /// Accumulate structure notes — exact repeats collapse; overflow past
    /// the cap is counted and rendered as one honest line at take().
    pub(crate) fn hold_notes(&mut self, notes: Vec<String>) {
        for n in notes {
            if self.notes.contains(&n) {
                continue;
            }
            if self.notes.len() >= MAX_STRUCTURE_NOTES {
                self.suppressed += 1;
            } else {
                self.notes.push(n);
            }
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.paths.len() + self.notes.len() + self.suppressed
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.paths.is_empty() && self.notes.is_empty() && self.suppressed == 0
    }

    /// Drain for delivery — resets the counter (spec K3). The overflow
    /// count collapses into its one line here.
    pub(crate) fn take(&mut self) -> (Vec<PathBuf>, Vec<String>) {
        let mut notes = std::mem::take(&mut self.notes);
        if self.suppressed > 0 {
            notes.push(format!("… and {} more structural changes", self.suppressed));
            self.suppressed = 0;
        }
        (std::mem::take(&mut self.paths), notes)
    }
}

/// Ride-along delivery at the user's submit (spec K2): with watching off or
/// nothing pending the user's text passes through untouched; otherwise the
/// pending paths are drained (counter reset, spec K3), the payload is built at
/// THIS moment, and the combined turn — file block first, the user's words
/// last — is returned for the normal ask path. Deterministic shell work; no
/// LLM call here.
///
/// Split in two: this is the `Tui` shell (it only prints the notices the core
/// returns), and `drain_and_deliver` below is the testable core. A `Tui` needs
/// a real terminal in raw mode, so without the split the drain itself — the
/// whole point of the function — could only be source-pinned.
pub(crate) async fn attach_pending(
    tui: &mut Tui,
    watching: bool,
    pending: &mut PendingChanges,
    files: &mut FileMemory,
    project_root: &Path,
    user_text: String,
) -> Result<String> {
    let (notices, outgoing) =
        drain_and_deliver(watching, pending, files, project_root, user_text).await;
    for notice in &notices {
        crate::tui::page::page_notice(tui, notice)?;
    }
    Ok(outgoing)
}

/// `attach_pending` without the terminal: decide, drain, deliver. `.0` is the
/// notice channel the caller prints, `.1` the outgoing turn.
///
/// Watching off is a hard gate, not just an optimization: `/watch off` already
/// drops the queue (`drop_pending_on_watch_off`), and this makes sure no queue
/// filled by some other route can survive the toggle either — "stop watching
/// my files" must hold for what is already queued too.
pub(crate) async fn drain_and_deliver(
    watching: bool,
    pending: &mut PendingChanges,
    files: &mut FileMemory,
    project_root: &Path,
    user_text: String,
) -> (Vec<String>, String) {
    if !watching || pending.is_empty() {
        return (Vec::new(), user_text);
    }
    let (paths, notes) = pending.take();
    crate::file_feedback::deliver_pending(files, project_root, &paths, &notes, user_text).await
}

/// Turning watching OFF discards whatever the watcher already queued. Without
/// this, a save made just before `/watch off` still shipped its file contents
/// with the user's next message — the explicit "stop watching my files"
/// silently disregarded, and invisible on the way in because the status line
/// hides the counter as soon as watching is off. Returns the one-line notice
/// to print when something was actually dropped; `None` when there was
/// nothing to drop or watching is still on.
pub(crate) fn drop_pending_on_watch_off(
    watching: bool,
    pending: &mut PendingChanges,
) -> Option<String> {
    if watching || pending.is_empty() {
        return None;
    }
    let dropped = pending.len();
    let _ = pending.take();
    Some(format!(
        "{dropped} noted change(s) dropped — they will not be sent"
    ))
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
/// (spec: Edge Cases section), so the cap keeps meaning.
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
    tracker: &mut crate::watcher::StructureTracker,
) -> Result<()> {
    // Flush-time classification (spec D1): existence is probed NOW; the
    // tracker updates even with watching off, so re-enabling stays honest.
    let (content, notes) = classify_flush(batch, tracker, files, project_root);
    if watching && !notes.is_empty() {
        // Structure notes never open a turn in ANY mode — they ride the
        // user's next submit (spec D2), so K1 holds for mkdir too.
        pending.hold_notes(notes);
    }
    let picked = route(content.len(), max_batch, watching, live);
    match picked {
        Route::Bulk => bulk_skip(tui, files, content)?,
        Route::ObserveOnly => sync_baseline(files, content),
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
                &content,
            )
            .await?
        }
        // Companion default: accumulate; delivery rides the next submit (K2).
        Route::Hold => pending.hold(content),
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

        // no file at all → empty (so live_from_approach is false → companion default stays on)
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
            p.take().0,
            vec![
                PathBuf::from("a.rs"),
                PathBuf::from("b.rs"),
                PathBuf::from("c.rs")
            ]
        );
        // take() drains — the status counter resets with it (spec K3).
        assert!(p.is_empty());
        assert!(p.take().0.is_empty());
    }

    #[test]
    fn pending_notes_dedup_cap_and_overflow_line() {
        let mut p = PendingChanges::new();
        p.hold_notes(vec![
            "+ a/ (new directory)".to_string(),
            "+ a/ (new directory)".to_string(),
        ]);
        assert_eq!(p.len(), 1, "exact repeats collapse");
        // A branch switch can delete hundreds of files — past the cap the
        // rest is counted and collapses into ONE overflow line at take().
        let many: Vec<String> = (0..30).map(|i| format!("- f{i}.rs (deleted)")).collect();
        p.hold_notes(many);
        assert_eq!(
            p.len(),
            31,
            "the counter stays honest about suppressed notes"
        );
        let (paths, notes) = p.take();
        assert!(paths.is_empty());
        assert_eq!(notes.len(), MAX_STRUCTURE_NOTES + 1);
        assert!(notes.last().unwrap().contains("11 more structural changes"));
        assert!(
            p.is_empty(),
            "take() drains notes and the overflow count too"
        );
    }

    #[test]
    fn classify_flush_five_way_table() {
        let base = scratch("classify");
        std::fs::create_dir_all(base.join("known")).unwrap();
        let file = base.join("main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let mut tracker = crate::watcher::StructureTracker::seed(&base);
        let mut files = FileMemory::new();
        let deleted_known = base.join("old.rs");
        files.seed(&deleted_known, "gone\n".to_string());

        let new_dir = base.join("brands");
        std::fs::create_dir_all(&new_dir).unwrap();
        let vanished_unknown = base.join("_transient_tmp.rs");

        let (content, notes) = classify_flush(
            vec![
                file.clone(),
                new_dir.clone(),
                base.join("known"),
                deleted_known.clone(),
                vanished_unknown,
            ],
            &mut tracker,
            &files,
            &base,
        );
        // Existing file → content path (unchanged pipeline).
        assert_eq!(content, vec![file]);
        // New dir noted, pre-existing dir silent, deleted KNOWN file noted,
        // vanished unknown path silent (transient noise, as today).
        assert_eq!(
            notes,
            vec![
                "+ brands/ (new directory)".to_string(),
                "- old.rs (deleted)".to_string()
            ]
        );
        // Second sighting of the same dir is silent — the tracker learned it.
        let (_, notes2) = classify_flush(vec![new_dir], &mut tracker, &files, &base);
        assert!(notes2.is_empty());
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn classify_flush_reports_removed_known_directory() {
        let base = scratch("classify-rmdir");
        let dir = base.join("assets");
        std::fs::create_dir_all(&dir).unwrap();
        let mut tracker = crate::watcher::StructureTracker::seed(&base);
        std::fs::remove_dir_all(&dir).unwrap();
        let files = FileMemory::new();
        let (content, notes) = classify_flush(vec![dir], &mut tracker, &files, &base);
        assert!(content.is_empty());
        assert_eq!(notes, vec!["- assets/ (directory removed)".to_string()]);
        std::fs::remove_dir_all(&base).ok();
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
            "polite::drop_pending_on_watch_off(",
            // The visible half of the feature: the status line must be fed
            // the REAL pending count. A reviewer's mutation replacing this
            // argument with a literal `0` killed the counter with the whole
            // suite still green — this needle is the tripwire for that.
            "Some((watching, live, pending.len()))",
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
        // v0.29.0: the batch is classified first — the route sees CONTENT
        // only, structure notes are held before routing.
        let production_src = include_str!("polite.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        for needle in [
            "let picked = route(content.len()",
            "pending.hold_notes(notes)",
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
        // directive they would feed file feedback into the exam prompt or into
        // a game toggle, and neither is a report on the learner's work. The
        // /exam and /game branches must therefore leave the queue intact, so
        // the changes deliver on the next real message. Note what this does
        // NOT do: /exam is only a prompt injection — the loop, the watcher and
        // the debouncer keep running through an exam, so a save made during
        // one still accumulates and rides the learner's next exam ANSWER,
        // wrapped in the lesson-flow frame. Known limitation, recorded in
        // SPEC.md §4.21; exam-state tracking is out of scope.
        // Crude source pin, same class as
        // run_rs_wiring_call_sites_are_pinned: the branch structure is inside
        // a TUI select! loop that can't be driven from a unit test.
        let src = include_str!("run.rs");
        assert!(
            src.contains(
                "attach_pending(&mut tui, watching, &mut pending, &mut files, project_root, line.clone())"
            ),
            "ride-along must take the user's own line, not the shared `outgoing` binding"
        );
        assert!(
            !src.contains("project_root, outgoing)"),
            "ride-along must not wrap `outgoing`: that attaches pending changes to /exam and /game directives"
        );
        // The positive needle above only forbids one exact spelling — it says
        // nothing about a SECOND call site being added elsewhere in the file
        // (e.g. inside the /exam branch). An occurrence count catches that:
        // exactly one `polite::attach_pending(` call may exist in run.rs.
        assert_eq!(
            src.matches("polite::attach_pending(").count(),
            1,
            "run.rs must call attach_pending exactly once — a second call site would let pending file changes ride into an operational directive (e.g. /exam) instead of only the user's own words"
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

    /// Scratch project dir for the delivery tests — unique per test tag.
    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("usta_polite_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn drain_and_deliver_empties_the_queue_after_one_delivery() {
        // The drain IS the feature: without it the same file block re-rides
        // on every later message and the status counter never resets. A
        // reviewer's mutation (take() -> a clone leaving the queue intact)
        // kept the whole suite green, so this test drives the real thing.
        let dir = scratch("drain");
        let file = dir.join("main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let mut files = FileMemory::new();
        let mut pending = PendingChanges::new();
        pending.hold(vec![file.clone()]);
        assert_eq!(pending.len(), 1);

        let (_, first) =
            drain_and_deliver(true, &mut pending, &mut files, &dir, "look".to_string()).await;
        assert!(
            first.contains("FILE:"),
            "first delivery carries the payload"
        );
        // Drained: the counter is back to zero...
        assert_eq!(pending.len(), 0, "delivery must drain the pending queue");
        assert!(pending.is_empty());

        // ...and the next message rides alone, with no file block at all.
        let (_, second) =
            drain_and_deliver(true, &mut pending, &mut files, &dir, "and now?".to_string()).await;
        assert_eq!(
            second, "and now?",
            "a drained queue must not re-attach the same files to the next message"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn drain_and_deliver_passes_text_through_when_nothing_is_queued() {
        let dir = scratch("drain-empty");
        let mut files = FileMemory::new();
        let mut pending = PendingChanges::new();
        let (notices, out) = drain_and_deliver(
            true,
            &mut pending,
            &mut files,
            &dir,
            "just asking".to_string(),
        )
        .await;
        assert!(notices.is_empty());
        assert_eq!(out, "just asking");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn drain_and_deliver_is_a_no_op_while_watching_is_off() {
        // Belt to drop_pending_on_watch_off's braces: even if a queue somehow
        // survives the toggle, nothing it holds reaches the model.
        let dir = scratch("drain-watch-off");
        let file = dir.join("main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let mut files = FileMemory::new();
        let mut pending = PendingChanges::new();
        pending.hold(vec![file]);
        let (notices, out) =
            drain_and_deliver(false, &mut pending, &mut files, &dir, "hello".to_string()).await;
        assert!(notices.is_empty());
        assert_eq!(out, "hello", "watching off must attach nothing");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn watch_off_drops_the_queued_changes_and_says_so() {
        // `/watch off` is an explicit "stop watching my files" — what is
        // already queued must go with it, and the user must be told, because
        // the status counter disappears the moment watching goes off.
        let mut pending = PendingChanges::new();
        pending.hold(vec![PathBuf::from("/tmp/a.rs"), PathBuf::from("/tmp/b.rs")]);
        pending.hold_notes(vec!["+ x/ (new directory)".into()]);
        let notice = drop_pending_on_watch_off(false, &mut pending).expect("a notice is due");
        assert!(
            notice.contains('3'),
            "the notice names how many were dropped: {notice}"
        );
        assert!(pending.is_empty(), "the queue must be dropped");
        // Nothing left to drop -> no second notice.
        assert!(drop_pending_on_watch_off(false, &mut pending).is_none());
    }

    #[test]
    fn watch_on_never_drops_the_queue() {
        let mut pending = PendingChanges::new();
        pending.hold(vec![PathBuf::from("/tmp/a.rs")]);
        assert!(drop_pending_on_watch_off(true, &mut pending).is_none());
        assert_eq!(pending.len(), 1, "turning watching ON must keep the queue");
    }
}
