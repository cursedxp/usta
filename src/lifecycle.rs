//! Session lifecycle: setup (`build_session`), the closing flush (`flush_core`),
//! interim compaction (`maybe_compact`), and the time/lock helpers — extracted
//! from `main.rs` (module split, Task 5). The most load-bearing cluster in the
//! crate: every session passes through these.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::anthropic::Message;
use crate::backend::{self, Backend};
use crate::session::Session;
use crate::slash::{read_game_pref, restore_game_pref};
use crate::transcript::{self, Recorder};
use crate::{brain, config, history, index, progress, tokens, ui};

/// Once this ratio is reached, an interim checkpoint + compaction is triggered.
const COMPACT_THRESHOLD: f64 = 0.70;
/// Number of most recent messages kept in history after compaction.
const COMPACT_KEEP_LAST: usize = 4;
/// Note prepended to history after compaction — tells the model the context
/// was compacted and the essence now lives in the files.
fn compact_note() -> String {
    format!(
        "{} Bağlam sıkıştırıldı. Önceki konuşmanın özü \
system prompt'taki progress/curriculum/approach dosyalarına yazıldı — güncel durum \
orada. Kaldığımız yerden devam et; kullanıcıya kompaksiyonu anlatma.",
        tokens::CHECKPOINT
    )
}

/// Wrap the LLM call in a spinner — don't leave silence while the user waits.
pub(crate) async fn ask_usta(
    backend: &mut Backend,
    system: &str,
    history: &[Message],
) -> Result<backend::Reply> {
    let spinner = ui::Spinner::start("Usta is thinking…");
    let result = backend.complete(system, history).await;
    spinner.stop().await;
    result
}

/// Session setup once the topic is known — system prompt + Session + write its
/// own lock + recorder + has_progress. The lock-CONFLICT confirmation is NOT here
/// (handled by the caller depending on the path: plain stdin, TUI single-key). Returns:
/// `(session, recorder, lock_path, has_progress)`.
pub(crate) fn build_session(
    global: &Path,
    project_root: &Path,
    topic: &str,
    today: &str,
) -> Result<(Session, Recorder, PathBuf, bool)> {
    let system = brain::load_system_prompt(global, Some(project_root), topic, today);
    let session = Session::new(topic.to_string(), system);

    let lock = lock_path(project_root, topic);
    if let Err(e) = std::fs::write(&lock, std::process::id().to_string()) {
        ui::warn(&format!("topic lock could not be written: {e}"));
    }

    let recorder = Recorder::new(transcript::session_path(project_root, topic, &now_stamp()));

    // Catalog upsert at OPEN (not only at close): a project opened even once must be
    // in the catalog so factory reset can find and clean it — even if this session is
    // cancelled before the closing flush ever runs. Same row format as the close-time
    // upsert (index::record in flush_core); date = today (the open day). Non-fatal —
    // a catalog miss must never block the session.
    if let Err(e) = index::record(global, topic, project_root, today) {
        ui::warn(&format!("catalog could not be updated at open: {e}"));
    }

    let has_progress = std::fs::read_to_string(progress::progress_path(project_root, topic))
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);

    Ok((session, recorder, lock, has_progress))
}

/// Generates the progress/approach/curriculum files via the LLM at session close.
/// Doesn't touch anything for an empty session; an unknown file name is skipped
/// with a warning (never written to an arbitrary path).
/// Resolves the closing file name to its write target — PURE: no I/O, just
/// path computation. `profile` is written to the GLOBAL root (`global`) (about-the-
/// person, shared across all topics); `progress`/`approach`/`curriculum` go to the
/// PROJECT root (`project_root`). Unknown name → `None` — this lets the "unknown
/// file skipped" safety in `flush_progress` be tested in isolation.
/// `project`/`project-progress` go to the visible `mentor/` dir under the project
/// root (user-facing, spec: mentor layer).
fn flush_target(name: &str, project_root: &Path, global: &Path, topic: &str) -> Option<PathBuf> {
    match name {
        "progress" => Some(progress::progress_path(project_root, topic)),
        "approach" => Some(progress::approach_path(project_root, topic)),
        "curriculum" => Some(progress::curriculum_path(project_root, topic)),
        "project" => Some(progress::project_md_path(project_root)),
        "project-progress" => Some(progress::project_progress_path(project_root)),
        "profile" => Some(global.join("USER.md")),
        _ => None,
    }
}

pub(crate) async fn flush_core(
    backend: &mut Backend,
    topic: &str,
    system: &str,
    history: &[Message],
    project_root: &Path,
    record_history: bool,
) -> Result<()> {
    if history.is_empty() {
        return Ok(());
    }
    ui::notice("summarizing session — writing files…");
    // The global root is resolved once: used both to embed the current profile
    // into the prompt and to write the profile at closing. If it can't be resolved,
    // the profile is skipped for this session — progress/approach/curriculum
    // (project-local) don't depend on it, their writes are unaffected.
    let global = match config::global_root() {
        Ok(g) => Some(g),
        Err(e) => {
            ui::warn(&format!(
                "global root could not be resolved — profile will be skipped this session: {e}"
            ));
            None
        }
    };
    let dummy_global = PathBuf::new();
    let global_for_paths = global.as_deref().unwrap_or(&dummy_global);
    let p_path = flush_target("progress", project_root, global_for_paths, topic).unwrap();
    let a_path = flush_target("approach", project_root, global_for_paths, topic).unwrap();
    let c_path = flush_target("curriculum", project_root, global_for_paths, topic).unwrap();
    let prj_path = flush_target("project", project_root, global_for_paths, topic).unwrap();
    let ppg_path = flush_target("project-progress", project_root, global_for_paths, topic).unwrap();
    let pr_path = global
        .as_ref()
        .map(|g| flush_target("profile", project_root, g, topic).unwrap());

    let read = |p: &Path| std::fs::read_to_string(p).ok();
    let mut msgs = history.to_vec();
    msgs.push(Message::user(progress::closing_prompt(
        topic,
        read(&p_path).as_deref(),
        read(&a_path).as_deref(),
        read(&c_path).as_deref(),
        pr_path.as_deref().and_then(read).as_deref(),
        read(&prj_path).as_deref(),
        read(&ppg_path).as_deref(),
    )));
    let reply = ask_usta(backend, system, &msgs).await?;
    let files = progress::split_files(&reply.text);
    if files.is_empty() {
        anyhow::bail!("model produced no files — nothing was written");
    }
    // Shell guarantee for the `/game` preference: capture the on-disk state of the
    // `- gamification:` line BEFORE the model rewrites USER.md. If the closing prompt's
    // KEEP rule is ignored (line dropped or value flipped), we restore it after the write.
    let game_pref_before = global.as_deref().and_then(read_game_pref);
    for (name, content) in files {
        let path = match name.as_str() {
            "progress" => p_path.clone(),
            "approach" => a_path.clone(),
            "curriculum" => c_path.clone(),
            "project" => prj_path.clone(),
            "project-progress" => ppg_path.clone(),
            "profile" => match &pr_path {
                Some(p) => p.clone(),
                // no global root — the warning was already given above.
                None => continue,
            },
            other => {
                ui::warn(&format!("unknown closing file skipped: {other}"));
                continue;
            }
        };
        if content.is_empty() {
            ui::warn(&format!("empty content skipped: {name}"));
            continue;
        }
        progress::write_atomic(&path, &content)?;
        ui::notice(&format!("updated: {}", path.display()));
    }

    // Restore the `/game` preference if the model's profile rewrite dropped or flipped
    // it (game_pref_before == None → user never toggled → left untouched).
    if let Some(g) = &global {
        match restore_game_pref(g, game_pref_before) {
            Ok(true) => ui::notice("gamification preference restored"),
            Ok(false) => {}
            Err(e) => ui::warn(&format!(
                "gamification preference could not be restored: {e}"
            )),
        }
    }

    // Update the global catalog — a failure here doesn't roll back the progress
    // write, it's just logged as a warning (the catalog is a comfort layer, not the memory itself).
    match &global {
        Some(g) => {
            if let Err(e) = index::record(g, topic, project_root, &today()) {
                ui::warn(&format!("catalog could not be updated: {e}"));
            }

            // Session history line — powers streaks/weekly stats (spec: progress stats).
            // Gated on record_history: maybe_compact's interim checkpoints are NOT
            // session closes — they must not add a history line, or a session that
            // compacts K times would count as K+1 sessions (spec: one line per closing flush).
            if record_history {
                let cur = std::fs::read_to_string(&c_path).ok();
                let map = cur
                    .as_deref()
                    .and_then(crate::tui::welcome::curriculum_percent);
                let settled = cur.as_deref().and_then(history::settled_count);
                let line = history::record_line(&today(), topic, map, settled);
                if let Err(e) = history::append(g, &line) {
                    ui::warn(&format!("history could not be updated: {e}"));
                }
            }
        }
        None => ui::warn("catalog could not be updated: no global root"),
    }

    Ok(())
}

pub(crate) async fn flush_progress(
    backend: &mut Backend,
    session: &Session,
    project_root: &Path,
    record_history: bool,
) -> Result<()> {
    flush_core(
        backend,
        &session.topic,
        &session.system,
        session.history(),
        project_root,
        record_history,
    )
    .await
}

/// If the threshold is exceeded: interim flush → reload the system prompt with
/// fresh files → trim history → reset the CLI session. If the flush fails,
/// compaction is CANCELED — history is never discarded before the data lands on disk.
pub(crate) async fn maybe_compact(
    backend: &mut Backend,
    session: &mut Session,
    project_root: &Path,
    tokens: Option<u64>,
) {
    let Some(t) = tokens else { return };
    if (t as f64) < COMPACT_THRESHOLD * backend.context_window() as f64 {
        return;
    }
    if session.history().len() <= COMPACT_KEEP_LAST {
        return;
    }
    ui::notice("context filling up — taking an interim checkpoint…");
    if let Err(e) = flush_progress(backend, session, project_root, false).await {
        ui::warn(&format!(
            "interim checkpoint failed, compaction postponed: {e}"
        ));
        return;
    }
    match config::global_root() {
        Ok(global) => {
            session.system =
                brain::load_system_prompt(&global, Some(project_root), &session.topic, &today());
        }
        Err(e) => ui::warn(&format!("system prompt could not be refreshed: {e}")),
    }
    session.compact(COMPACT_KEEP_LAST, &compact_note());
    backend.reset_session();
    ui::notice("context compacted — pick up where you left off");
}

/// Today's local date — the date field for catalog rows.
pub(crate) fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Session filename stamp — local time.
fn now_stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

/// Topic lock: `.usta/.lock-<topic>` — prevents two concurrent sessions from
/// silently overwriting the same progress. Content: pid (for diagnostics).
pub(crate) fn lock_path(project_root: &Path, topic: &str) -> PathBuf {
    project_root.join(".usta").join(format!(".lock-{topic}"))
}

/// Sleep until the deadline if there is one; otherwise a future that never
/// returns (the select guard never polls this arm without a deadline anyway —
/// this is just for type safety).
pub(crate) async fn sleep_until_deadline(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(d) => tokio::time::sleep_until(d).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_target_maps_profile_to_global_other_three_to_project() {
        let project = Path::new("/proje");
        let global = Path::new("/glob");
        assert_eq!(
            flush_target("profile", project, global, "rust"),
            Some(PathBuf::from("/glob/USER.md"))
        );
        assert_eq!(
            flush_target("progress", project, global, "rust"),
            Some(PathBuf::from("/proje/.usta/learner/progress/rust.md"))
        );
        assert_eq!(
            flush_target("approach", project, global, "rust"),
            Some(PathBuf::from("/proje/.usta/approaches/rust.md"))
        );
        assert_eq!(
            flush_target("curriculum", project, global, "rust"),
            Some(PathBuf::from("/proje/.usta/learner/curriculum/rust.md"))
        );
    }

    #[test]
    fn flush_target_routes_mentor_files_to_project_root() {
        let project = Path::new("/tmp/proj");
        let global = Path::new("/tmp/global");
        assert_eq!(
            flush_target("project", project, global, "rust"),
            Some(PathBuf::from("/tmp/proj/mentor/PROJECT.md"))
        );
        assert_eq!(
            flush_target("project-progress", project, global, "rust"),
            Some(PathBuf::from("/tmp/proj/mentor/PROGRESS.md"))
        );
    }

    #[test]
    fn flush_target_rejects_unknown_name() {
        assert_eq!(
            flush_target(
                "bilinmeyen",
                Path::new("/proje"),
                Path::new("/glob"),
                "rust"
            ),
            None
        );
    }
}
