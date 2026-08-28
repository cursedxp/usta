//! Scaffold writing (`usta init`), the `topics`/`stats` reports, all three
//! reset flows (topic/factory/profile), and the confirm prompts they share —
//! extracted from `main.rs` (module split, Task 7). All three sub-clusters
//! live behind a CLI subcommand and never enter the session loop; `confirm`
//! is the thread that ties them together, so they stay in one module.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::{config, defaults, history, index, migrate, progress, ui};

/// One-shot TR→EN protocol-token migration, run at the top of every command
/// dispatch path (start/topics/stats/reset) right after global root + project
/// `.usta` are known, before any file is read. Silent on `Ok(0)` (nothing to
/// migrate) and on `Err` — a migration failure must never abort the session,
/// it's surfaced as a warning and the command proceeds against whatever state
/// is on disk (pre- or partially-migrated).
pub(crate) fn run_migration(global: &Path, project_usta: Option<&Path>) {
    match migrate::run(global, project_usta) {
        Ok(0) => {}
        Ok(n) => ui::notice(&format!(
            "migrated {n} file(s) to English protocol tokens (backup: .bak)"
        )),
        Err(e) => ui::warn(&format!("token migration skipped: {e}")),
    }
}

/// Lazily sets up the `.usta/` scaffold — lets `start` bootstrap itself, making
/// `usta init` optional. (1) Completes the global brain root (`~/.config/usta`):
/// code-owned files are synced with the embedded ones, user-owned files are preserved.
/// (2) If the project root can't be found by searching upward, sets up a new
/// project `.usta/` in `cwd` and returns `cwd`; if found, returns it as-is.
pub(crate) fn ensure_scaffold(cwd: &Path) -> Result<PathBuf> {
    let global = config::global_root()?;
    write_global_defaults(&global)?;

    match config::find_project_root(cwd) {
        Some(root) => Ok(root),
        None => {
            write_project_scaffold(cwd)?;
            Ok(cwd.to_path_buf())
        }
    }
}

/// `usta init` — fills the global brain (`~/.config/usta`) with defaults
/// (code-owned files are synced, user-owned files are NEVER overwritten) and
/// sets up the project `.usta/` scaffold in CWD. The global brain is "set up once,
/// shared across all projects"; the project `.usta/` is separate per project, for
/// overrides + progress tracking.
/// The write logic is shared with `ensure_scaffold` (`write_global_defaults` /
/// `write_project_scaffold`) — the only difference here is per-file status printing.
pub(crate) fn run_init() -> Result<()> {
    // Migration intentionally NOT wired here — init only scaffolds (writes
    // defaults) and parses no migration-scoped tokens; the next real command
    // (e.g. `usta topics`, `usta start`) runs the migration.
    let global = config::global_root()?;
    for (path, wrote) in write_global_defaults(&global)? {
        print_scaffold_status(&path, wrote);
    }

    let cwd = std::env::current_dir()?;
    for (path, wrote) in write_project_scaffold(&cwd)? {
        print_scaffold_status(&path, wrote);
    }

    println!("Ready. Start with 'usta start <topic>'.");
    Ok(())
}

/// `usta topics` — list the entries in the global catalog. No LLM needed.
pub(crate) fn run_topics() -> Result<()> {
    let global = config::global_root()?;
    let project_usta = std::env::current_dir()
        .ok()
        .and_then(|c| config::find_project_root(&c))
        .map(|root| root.join(".usta"));
    run_migration(&global, project_usta.as_deref());
    let content = std::fs::read_to_string(global.join("learner/index.md")).unwrap_or_default();
    let list = index::entries(&content);
    if list.is_empty() {
        println!("No saved topics — start with 'usta start <topic>'.");
        return Ok(());
    }
    print!("{}", render_topics_table(&list));
    Ok(())
}

/// Pad `s` to visible width `w` (unicode-width — byte counting misaligns Turkish
/// and path characters). Shared by the `stats`/`topics` column layouts.
fn col_pad(s: &str, w: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    format!("{s}{}", " ".repeat(w.saturating_sub(s.width())))
}

/// Render the `usta topics` catalog as space-aligned columns with a single dim
/// rule under the header (design tokens 06 / mockup 05). Pure — no I/O. The
/// topic/project/date values are unchanged; only the layout is aligned.
fn render_topics_table(entries: &[index::IndexEntry]) -> String {
    use unicode_width::UnicodeWidthStr;
    let projects: Vec<String> = entries
        .iter()
        .map(|e| e.project.display().to_string())
        .collect();
    let topic_w = entries
        .iter()
        .map(|e| e.topic.width())
        .chain(std::iter::once("Topic".width()))
        .max()
        .unwrap_or(5);
    let proj_w = projects
        .iter()
        .map(|p| p.width())
        .chain(std::iter::once("Project".width()))
        .max()
        .unwrap_or(7);
    let last = "Last session";
    let mut out = format!(
        "{}  {}  {}\n",
        col_pad("Topic", topic_w),
        col_pad("Project", proj_w),
        last
    );
    out.push_str(&"─".repeat(topic_w + 2 + proj_w + 2 + last.width()));
    out.push('\n');
    for (e, p) in entries.iter().zip(&projects) {
        out.push_str(&format!(
            "{}  {}  {}\n",
            col_pad(&e.topic, topic_w),
            col_pad(p, proj_w),
            e.date
        ));
    }
    out
}

/// `usta stats` — this week's summary + streaks, read from the global session
/// history. No LLM needed; missing/empty history just renders the empty state.
pub(crate) fn run_stats() -> Result<()> {
    let global = config::global_root()?;
    let project_usta = std::env::current_dir()
        .ok()
        .and_then(|c| config::find_project_root(&c))
        .map(|root| root.join(".usta"));
    run_migration(&global, project_usta.as_deref());
    let content = std::fs::read_to_string(global.join("learner/history.md")).unwrap_or_default();
    let es = history::entries(&content);
    println!("{}", render_stats(&es, &crate::lifecycle::today()));
    Ok(())
}

/// Render the `usta stats` report from history entries — pure function, no I/O.
/// ADHD-safe tone: never prints "current streak: 0" anywhere — a broken streak
/// falls back to the longest-streak framing instead of shaming a zero.
fn render_stats(entries: &[history::Entry], today: &str) -> String {
    if entries.is_empty() {
        return "no sessions recorded yet — streaks start with the first one.".to_string();
    }
    let longest = history::longest_streak(entries);
    let week = history::week_summary(entries, today);
    if week.sessions == 0 {
        return format!("quiet week — your longest streak is still {longest} day(s).");
    }
    let current = history::current_streak(entries, today);
    let mut out = String::from("This week (last 7 days)\n\n");
    let topic_w = week
        .per_topic
        .iter()
        .map(|t| unicode_width::UnicodeWidthStr::width(t.topic.as_str()))
        .max()
        .unwrap_or(0);
    for t in &week.per_topic {
        out.push_str(&format!(
            "  {}   {} session(s)",
            col_pad(&t.topic, topic_w),
            t.sessions
        ));
        if let (Some(from), Some(to)) = (t.map_from, t.map_to) {
            out.push_str(&format!("   map {from}% → {to}%"));
        }
        if let (Some(from), Some(to)) = (t.settled_from, t.settled_to) {
            out.push_str(&format!("   settled {from} → {to}"));
        }
        out.push('\n');
    }
    out.push('\n');
    if current > 0 {
        out.push_str(&format!(
            "total: {} session(s) · current streak: {current} day(s) · longest: {longest} day(s)",
            week.sessions
        ));
    } else {
        out.push_str(&format!(
            "total: {} session(s) · longest streak: {longest} day(s)",
            week.sessions
        ));
    }
    out
}

/// `usta reset <topic>` — delete the progress for that topic in the current
/// project (with confirmation), remove its generated visuals (Görev 5), and
/// drop it from the global catalog. No LLM needed.
pub(crate) fn run_reset_topic(topic: &str) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let Some(root) = config::find_project_root(&cwd) else {
        anyhow::bail!("no .usta in this directory (or above) — no project found to reset");
    };
    let global = config::global_root()?;
    run_migration(&global, Some(&root.join(".usta")));
    let path = progress::progress_path(&root, topic);
    if !path.is_file() {
        println!("no record: {}", path.display());
        return Ok(());
    }
    if !confirm(
        &format!(
            "{} and its visuals will be deleted. Are you sure? [y/N] ",
            path.display()
        ),
        &["e", "evet", "y", "yes"],
    )? {
        println!("cancelled.");
        return Ok(());
    }
    std::fs::remove_file(&path).with_context(|| format!("could not delete: {}", path.display()))?;
    println!("deleted: {}", path.display());

    remove_topic_visuals(&root, topic)?;

    // Drop it from the catalog too — pass silently if the catalog doesn't exist / can't be read.
    let index_path = global.join("learner/index.md");
    if let Ok(current) = std::fs::read_to_string(&index_path) {
        let updated = index::remove(&current, topic, &root);
        progress::write_atomic(&index_path, &updated)?;
    }
    Ok(())
}

/// Removes a topic's generated visuals (`.usta/visuals/<topic>/`, Görev 5).
/// Idempotent: a topic that never ran `/show` has no such directory — that's
/// `NotFound`, not an error, so reset still succeeds cleanly.
fn remove_topic_visuals(root: &Path, topic: &str) -> Result<()> {
    let dir = root.join(".usta/visuals").join(topic);
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("could not delete: {}", dir.display())),
    }
}

/// Factory-reset confirmation prompt. Both `yes` and `evet` are accepted (see the
/// `confirm` call below); the wording names both so the prompt matches the SPEC.
const FACTORY_RESET_PROMPT: &str =
    "Everything will be permanently deleted. Type 'yes' to confirm: ";

/// Factory-reset deletion targets: every catalogued project's `.usta` — plus the
/// project the user is standing in (`cwd_root`), even if it was never catalogued
/// (a cancelled session from before open-time cataloguing leaves such orphans).
/// The user already word-confirms the reset; the next start must not come back
/// asking about leftovers in the very project the reset was run from.
fn factory_targets(index_content: &str, cwd_root: Option<&Path>) -> Vec<PathBuf> {
    let mut targets: Vec<PathBuf> = index::entries(index_content)
        .into_iter()
        .map(|e| e.project.join(".usta"))
        .collect();
    if let Some(root) = cwd_root {
        targets.push(root.join(".usta"));
    }
    targets.sort();
    targets.dedup();
    targets
}

/// `usta reset --factory` — deletes the `.usta/` of every project in the catalog +
/// the global brain. The next `usta` run sets everything back up from defaults
/// (bootstrap) — Usta starts as if it never knew the user.
pub(crate) fn run_reset_factory() -> Result<()> {
    let global = config::global_root()?;
    let cwd_root = std::env::current_dir()
        .ok()
        .and_then(|c| config::find_project_root(&c));
    let cwd_usta = cwd_root.as_deref().map(|r| r.join(".usta"));
    run_migration(&global, cwd_usta.as_deref());
    let index_content =
        std::fs::read_to_string(global.join("learner/index.md")).unwrap_or_default();
    let mut targets = factory_targets(&index_content, cwd_root.as_deref());
    targets.retain(|p| p.is_dir());

    println!("FACTORY RESET — will be deleted:");
    for t in &targets {
        println!("  {}", t.display());
    }
    println!("  {} (global brain)", global.display());
    println!("Note: other old projects not in the catalog are NOT in this list (the current directory's project is).");
    println!("Check: find ~ -maxdepth 5 -name .usta -type d");

    if !confirm(FACTORY_RESET_PROMPT, &["evet", "yes"])? {
        println!("cancelled.");
        return Ok(());
    }
    for t in &targets {
        std::fs::remove_dir_all(t).with_context(|| format!("could not delete: {}", t.display()))?;
        println!("deleted: {}", t.display());
    }
    if global.is_dir() {
        std::fs::remove_dir_all(&global)
            .with_context(|| format!("could not delete: {}", global.display()))?;
        println!("deleted: {}", global.display());
    }
    println!("Zero point. The next 'usta' run will set everything up from scratch.");
    Ok(())
}

/// Is the profile still the embedded generic template? (= Usta doesn't know the user yet.)
/// Trimmed comparison — line-ending/whitespace differences shouldn't produce a false negative.
pub(crate) fn profile_is_generic(disk: &str) -> bool {
    defaults::global_defaults()
        .into_iter()
        .find(|(rel, _, _)| *rel == "USER.md")
        .map(|(_, c, _)| c.trim() == disk.trim())
        .unwrap_or(false)
}

/// First-run marker: `<global>/learner/.introduced`. Its EXISTENCE is the only
/// thing the shell reads (deterministic); the content is diagnostics.
/// `profile_is_generic` can't carry first-run detection — one hand edit to
/// USER.md would skip the introduction forever, and a profile reset would
/// re-trigger it for a veteran (SPEC §4.22, blocker H3).
pub(crate) fn intro_marker_path(global: &Path) -> PathBuf {
    global.join("learner/.introduced")
}

/// Write the first-run marker (best-effort — a marker write failure must never
/// block the session; worst case the introduction re-runs next launch).
/// `how`: "completed" (introduction finished) or "seeded" (grandfathered).
pub(crate) fn mark_intro_done(global: &Path, how: &str) {
    let path = intro_marker_path(global);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, format!("{} | {how}\n", crate::lifecycle::today()));
}

/// Does the first-run introduction still need to run? Self-seeding: when the
/// marker is absent but there is evidence of prior use (a filled profile, or
/// any catalog record), the marker is written as "seeded" and the introduction
/// is skipped — existing users are grandfathered on their first post-upgrade
/// launch. An empty/missing USER.md is NOT evidence (profile_is_generic("")
/// returns false because it matches nothing — guard it explicitly).
pub(crate) fn intro_needed(global: &Path, index_content: &str) -> bool {
    if intro_marker_path(global).exists() {
        return false;
    }
    let profile = std::fs::read_to_string(global.join("USER.md")).unwrap_or_default();
    let filled_profile = !profile.trim().is_empty() && !profile_is_generic(&profile);
    let has_records = !crate::index::entries(index_content).is_empty();
    if filled_profile || has_records {
        mark_intro_done(global, "seeded");
        return false;
    }
    true
}

/// Profile reset core — PURE (no confirmation, no global_root): backs up the
/// current profile to `.bak`, writes the embedded generic template. Does NOT
/// touch topic progress (spec Ç2).
fn reset_profile_files(global: &Path) -> Result<()> {
    let sablon = defaults::global_defaults()
        .into_iter()
        .find(|(rel, _, _)| *rel == "USER.md")
        .map(|(_, c, _)| c)
        .context("embedded profile template not found")?;
    let path = global.join("USER.md");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create directory: {}", parent.display()))?;
    }
    if path.exists() {
        std::fs::copy(&path, path.with_extension("md.bak"))
            .with_context(|| format!("could not back up: {}", path.display()))?;
    }
    std::fs::write(&path, sablon)
        .with_context(|| format!("could not write: {}", path.display()))?;
    Ok(())
}

/// `usta reset --profile` — with confirmation; Usta starts "not knowing" the user.
/// Destructive operation: if there's no TTY (confirmation can't be obtained), exits
/// with an error instead of running silently — even though `confirm()` falls back to
/// "no" on empty stdin, this behavior is made explicit here so it doesn't stay
/// dependent on the pipe's content.
pub(crate) fn run_reset_profile() -> Result<()> {
    if !std::io::stdin().is_terminal() {
        anyhow::bail!(
            "no TTY — cannot get confirmation, profile not reset. Run in an interactive terminal."
        );
    }
    let global = config::global_root()?;
    let project_usta = std::env::current_dir()
        .ok()
        .and_then(|c| config::find_project_root(&c))
        .map(|root| root.join(".usta"));
    run_migration(&global, project_usta.as_deref());
    let path = global.join("USER.md");
    if !confirm(
        &format!(
            "Profile will be reset — Usta will start not knowing you (backup: {}.bak). Continue? [y/N] ",
            path.display()
        ),
        &["e", "evet", "y", "yes"],
    )? {
        println!("cancelled — profile unchanged.");
        return Ok(());
    }
    reset_profile_files(&global)?;
    println!("profile reset: {} (old version in .bak)", path.display());
    Ok(())
}

/// Ask for confirmation: read one line from stdin, compare with the accepted list
/// (lowercase). stdin closed/empty = no — safe default.
pub(crate) fn confirm(prompt: &str, yes: &[&str]) -> Result<bool> {
    use std::io::Write;
    print!("{prompt}");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(yes.contains(&line.trim().to_lowercase().as_str()))
}

/// Startup recover(true) / delete(false) decision from a raw confirm line.
/// Default is RECOVER — the lossless side: only an explicit no deletes; empty
/// Enter or anything unrecognized recovers. The no-set carries both "hayır" and
/// ASCII "hayir": Rust lowercases 'I' to 'i' (not Turkish dotless 'ı'), so
/// "HAYIR" arrives as "hayir" — without the ASCII form uppercase Turkish no
/// would silently recover instead of delete.
fn recover_choice(input: &str) -> bool {
    !matches!(
        input.trim().to_lowercase().as_str(),
        "n" | "no" | "h" | "hayır" | "hayir"
    )
}

/// Ask the startup recover/delete question. Default-YES variant of `confirm`
/// (which is default-NO). Never returns an error — on any stdin read failure it
/// defaults to RECOVER so a startup hiccup can't cause data loss or block startup.
pub(crate) fn confirm_recover(prompt: &str) -> bool {
    use std::io::Write;
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return true; // lossless side
    }
    recover_choice(&line)
}

/// Per-file/directory status line for `usta init`.
fn print_scaffold_status(path: &Path, wrote: bool) {
    if wrote {
        println!("written: {}", path.display());
    } else {
        println!("already exists, skipped: {}", path.display());
    }
}

/// One-time migration from the old profile location (previous `learner/` subpath)
/// to the new root (`USER.md`). Moves it if the old file exists and the new one
/// doesn't (`true`); otherwise a no-op (`false`) — an existing `USER.md` is never
/// overwritten, no risk of data loss.
fn migrate_profile_to_user_md(global: &Path) -> Result<bool> {
    let old = global.join("learner/profile.md");
    let new = global.join("USER.md");
    if old.exists() && !new.exists() {
        std::fs::rename(&old, &new).with_context(|| {
            format!(
                "profile could not be moved: {} → {}",
                old.display(),
                new.display()
            )
        })?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Creates the global brain root and writes the default files. Code-owned files
/// (USTA.md, approaches/*) are synced with the embedded content — overwritten if
/// stale; user-owned files (learner/*, USER.md) are written only the first time
/// (`defaults::Ownership`). Returns `(path, was-written)` for each file —
/// `run_init` prints this, `ensure_scaffold` swallows it silently.
/// The migration from the old profile location to `USER.md` runs BEFORE the
/// write loop (spec §5 ordering requirement) — this way both `ensure_scaffold`
/// and `run_init` (both call this function) preserve existing user data, and
/// `USER.md`'s `Ownership::User` write-once rule doesn't overwrite the moved file.
fn write_global_defaults(global: &Path) -> Result<Vec<(PathBuf, bool)>> {
    std::fs::create_dir_all(global)
        .with_context(|| format!("could not create global root: {}", global.display()))?;
    migrate_profile_to_user_md(global)?;

    let mut results = Vec::new();
    for (rel, content, ownership) in defaults::global_defaults() {
        let path = global.join(rel);
        let write_needed = match ownership {
            defaults::Ownership::Code => config::needs_sync(&path, content),
            defaults::Ownership::User => config::should_write(&path),
        };
        let wrote = if write_needed {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("could not create directory: {}", parent.display()))?;
            }
            std::fs::write(&path, content)
                .with_context(|| format!("could not write: {}", path.display()))?;
            true
        } else {
            false
        };
        results.push((path, wrote));
    }
    Ok(results)
}

/// Sets up the project scaffold under `cwd/.usta/` (`learner/progress`,
/// `approaches` + `.gitkeep`s — so an empty directory can still be committed).
/// `.gitkeep` writes are silent (identical to the original `run_init` behavior);
/// the returned list only contains the directories' `(path, was-written)` status.
///
/// `visuals/` (Görev 5) gets a `.gitignore` (`*`) instead of a `.gitkeep` — the
/// directory holds generated `/show` HTML that should stay on disk but never
/// enter the user's git repo.
fn write_project_scaffold(cwd: &Path) -> Result<Vec<(PathBuf, bool)>> {
    let usta_dir = cwd.join(".usta");
    let mut results = Vec::new();

    for sub in ["learner/progress", "approaches"] {
        let dir = usta_dir.join(sub);
        let dir_existed = dir.is_dir();
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("could not create directory: {}", dir.display()))?;
        results.push((dir.clone(), !dir_existed));

        // .gitkeep — so an empty directory can still be committed.
        let gitkeep = dir.join(".gitkeep");
        if config::should_write(&gitkeep) {
            std::fs::write(&gitkeep, "")
                .with_context(|| format!("could not write: {}", gitkeep.display()))?;
        }
    }

    // Visible, user-facing project docs (mentor/PROJECT.md + PROGRESS.md are
    // written by the closing flush; the dir is scaffolded so it's visible from
    // day one). Deliberately OUTSIDE `.usta/` — reset must never touch it.
    let mentor_dir = cwd.join("mentor");
    let mentor_existed = mentor_dir.is_dir();
    std::fs::create_dir_all(&mentor_dir)
        .with_context(|| format!("could not create directory: {}", mentor_dir.display()))?;
    results.push((mentor_dir.clone(), !mentor_existed));
    let mentor_gitkeep = mentor_dir.join(".gitkeep");
    if config::should_write(&mentor_gitkeep) {
        std::fs::write(&mentor_gitkeep, "")
            .with_context(|| format!("could not write: {}", mentor_gitkeep.display()))?;
    }

    // Visible exercise deliverables dir (spec: exercise loop). The watcher is
    // extension-agnostic, so anything saved here is already watched — this dir
    // only gives assignments a conventional, visible home.
    let exercises_dir = cwd.join("exercises");
    let exercises_existed = exercises_dir.is_dir();
    std::fs::create_dir_all(&exercises_dir)
        .with_context(|| format!("could not create directory: {}", exercises_dir.display()))?;
    results.push((exercises_dir.clone(), !exercises_existed));
    let ex_gitkeep = exercises_dir.join(".gitkeep");
    if config::should_write(&ex_gitkeep) {
        std::fs::write(&ex_gitkeep, "")
            .with_context(|| format!("could not write: {}", ex_gitkeep.display()))?;
    }

    // Visible course-material dir (spec: material ingest). The user drops
    // book/course notes here; a new-topic introduction scans it and anchors
    // the curriculum to its chapters.
    let materials_dir = cwd.join("materials");
    let materials_existed = materials_dir.is_dir();
    std::fs::create_dir_all(&materials_dir)
        .with_context(|| format!("could not create directory: {}", materials_dir.display()))?;
    results.push((materials_dir.clone(), !materials_existed));
    let materials_gitkeep = materials_dir.join(".gitkeep");
    if config::should_write(&materials_gitkeep) {
        std::fs::write(&materials_gitkeep, "")
            .with_context(|| format!("could not write: {}", materials_gitkeep.display()))?;
    }

    let visuals_dir = usta_dir.join("visuals");
    let visuals_existed = visuals_dir.is_dir();
    std::fs::create_dir_all(&visuals_dir)
        .with_context(|| format!("could not create directory: {}", visuals_dir.display()))?;
    results.push((visuals_dir.clone(), !visuals_existed));

    let gitignore = visuals_dir.join(".gitignore");
    if config::should_write(&gitignore) {
        std::fs::write(&gitignore, "*\n")
            .with_context(|| format!("could not write: {}", gitignore.display()))?;
    }

    Ok(results)
}

#[cfg(test)]
#[path = "setup_tests.rs"]
mod tests;
