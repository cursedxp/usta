//! Usta — terminal Socratic learning mentor. Thin shell: CLI + LLM client +
//! file watcher + markdown brain loader. The intelligence lives in markdown.

mod anthropic;
mod backend;
mod brain;
mod check;
mod cli;
mod config;
mod defaults;
mod feedback;
mod file_feedback;
mod help;
mod history;
mod index;
mod input;
mod lifecycle;
mod materials;
mod migrate;
mod plain;
mod progress;
mod session;
mod setup;
mod slash;
mod tokens;
mod topic;
mod transcript;
mod tui;
mod ui;
mod visual;
mod watcher;

use std::io::IsTerminal;

use anyhow::Result;

use crate::cli::{parse_command, Command, ResetTarget};
use crate::lifecycle::{build_session, flush_core, flush_progress, lock_path, today};
use crate::plain::{resolve_topic, run_plain_loop};
use crate::setup::{
    confirm, confirm_recover, ensure_scaffold, profile_is_generic, run_init, run_migration,
    run_reset_factory, run_reset_profile, run_reset_topic, run_stats, run_topics,
};

/// Maximum number of files given feedback in a single debounce window — above
/// this it counts as a "bulk change" (git checkout, format-all): no LLM call.
const MAX_FEEDBACK_BATCH: usize = 5;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let topic_arg = match parse_command(&args)? {
        Command::Init => return run_init(),
        Command::Topics => return run_topics(),
        Command::Stats => return run_stats(),
        Command::Reset(ResetTarget::Topic(t)) => return run_reset_topic(&t),
        Command::Reset(ResetTarget::Factory) => return run_reset_factory(),
        Command::Reset(ResetTarget::Profile) => return run_reset_profile(),
        Command::Start(t) => t,
    };

    // Backend selection (CLI default, API optional) — with a clear error message.
    let mut backend = match backend::select() {
        Ok(b) => b,
        // Config error (bad USTA_BACKEND value) is not "no backend" — surface it.
        Err(e) if std::env::var_os("USTA_BACKEND").is_some() => return Err(e),
        Err(e) => {
            if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
                backend::run_backend_wizard()?
            } else {
                return Err(e);
            }
        }
    };

    // Migration MUST run before `ensure_scaffold` — `ensure_scaffold` →
    // `write_global_defaults` → `config::needs_sync` READS Ownership::Code
    // files (approaches/software.md, approaches/_default.md) and may resync
    // them with the embedded English templates. Those files are also
    // migration-in-scope (migrate::run sweeps global `approaches/*.md`), so
    // migration has to see them first or a legacy Turkish-token install loses
    // its `.bak` — the scaffold would silently overwrite before migration
    // ever runs. `global_root()` / `find_project_root()` are pure path
    // resolution (no reads of migration-scoped file content), so it's safe to
    // resolve both paths up front and migrate before touching the scaffold.
    let cwd = std::env::current_dir()?;
    let global = config::global_root()?;
    let existing_project_root = config::find_project_root(&cwd);
    let had_project_root = existing_project_root.is_some();
    let project_usta = existing_project_root.as_deref().map(|r| r.join(".usta"));
    run_migration(&global, project_usta.as_deref());

    // Set up `.usta/` silently if missing — `usta init` is no longer a mandatory
    // pre-step, `start` bootstraps itself (see ensure_scaffold). Global brain +
    // project root are merged to produce the system prompt (hybrid model — see
    // brain.rs). build_session uses this.
    let project_root = ensure_scaffold(&cwd)?;
    if !had_project_root {
        ui::notice(".usta/ set up");
    }

    // File watcher — spawned ONCE (starts a thread), then passed by (&mut) into
    // the running path. Input thread + debounce state are path-specific:
    // the plain path uses rustyline, the TUI path uses crossterm EventStream.
    let mut watch_rx = watcher::spawn(&project_root)?;

    let stale = transcript::find_unfinished(&project_root);
    if !stale.is_empty() {
        let tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
        if !tty {
            // pipe/script: EXACT existing behavior — warn only, no LLM calls, no delete
            for p in &stale {
                ui::warn(&format!(
                    "half-finished session record found (may not have been flushed): {}",
                    p.display()
                ));
            }
        } else {
            // List the stale records (dim), then ask ONCE: recover (default) or delete.
            for p in &stale {
                ui::notice(&format!("unflushed session record: {}", p.display()));
            }
            if confirm_recover(&format!(
                "recover {} unflushed session(s)? [Y/n] ",
                stale.len()
            )) {
                for p in &stale {
                    let Some(topic) = transcript::topic_from_record(p) else {
                        ui::warn(&format!(
                            "unrecognized session record name, leaving as-is: {}",
                            p.display()
                        ));
                        continue;
                    };
                    match transcript::read_history(p) {
                        Err(e) => ui::warn(&format!(
                            "could not read session record ({e}) — leaving as-is: {}",
                            p.display()
                        )),
                        Ok(history) if history.iter().filter(|m| m.role == "user").count() == 0 => {
                            // nothing to recover — mark done silently so the noise stops
                            let _ = transcript::mark_done(p);
                        }
                        Ok(history) => {
                            ui::notice(&format!(
                                "recovering unflushed session: {} — writing files…",
                                p.display()
                            ));
                            let system = brain::load_system_prompt(
                                &global,
                                Some(&project_root),
                                &topic,
                                &today(),
                            );
                            match flush_core(&mut backend, &topic, &system, &history, &project_root, true).await {
                                Ok(()) => {
                                    let _ = transcript::mark_done(p);
                                    ui::notice(&format!("recovered: {topic}"));
                                }
                                Err(e) => ui::warn(&format!("recovery failed ({e}) — record kept, will retry next start: {}", p.display())),
                            }
                            // Standalone conversation — don't let its CLI session_id leak
                            // into the next salvage or the real session that follows.
                            backend.reset_session();
                        }
                    }
                }
            } else {
                // User declined recovery → the conversation is dead; clean the records.
                let (deleted, errors) = transcript::delete_unflushed(&stale);
                for e in errors {
                    ui::warn(&format!("could not delete: {e}"));
                }
                ui::notice(&format!("cleaned {deleted} stale session record(s)"));
            }
        }
    }

    // Both paths produce `(Session, Recorder, PathBuf)`; closing is shared.
    // TUI path: topic entry + slug/confirmation + build_session all happen inside run() —
    // topic_arg is passed raw, a `None` return means the user exited without giving a topic.
    // Plain path (no TTY / NO_COLOR): resolve_topic + lock-conflict + build_session
    // + banner + run_plain_loop happen here — behavior preserved exactly.
    let (session, recorder, lock) = if !ui::is_plain() {
        // While TUI is active, don't let notice/warn/Spinner print raw ANSI —
        // turn the flag on, and always turn it off when run() returns (even on error), then raise the error.
        ui::set_tui_active(true);
        let r = tui::run::run(
            &mut backend,
            &global,
            &project_root,
            &today(),
            topic_arg,
            MAX_FEEDBACK_BATCH,
            &mut watch_rx,
        )
        .await;
        ui::set_tui_active(false);
        match r? {
            Some(artifacts) => artifacts,
            None => {
                // Exited without giving a topic — no session/lock, nothing to close.
                ui::notice("See you — keep getting in the water.");
                return Ok(());
            }
        }
    } else {
        let (topic, intro) = resolve_topic(&mut backend, topic_arg, &project_root, &global).await?;

        // Lock-conflict confirmation (plain/pipe) — BEFORE build_session, without
        // writing its own lock yet. (In the TUI path this check happens inside run() via tui_confirm.)
        let lock = lock_path(&project_root, &topic);
        if lock.exists() {
            let pid = std::fs::read_to_string(&lock).unwrap_or_default();
            if std::io::stdin().is_terminal() {
                let msg = format!(
                    "Another session may be open for this topic (pid {}) — progress could clash \
                     if both sessions close at the same time. Continue anyway? [y/N] ",
                    pid.trim()
                );
                if !confirm(&msg, &["e", "evet", "y", "yes"])? {
                    println!("cancelled — close the other session first (or delete the lock if it's stale: {})", lock.display());
                    return Ok(());
                }
            } else {
                ui::warn("stale topic lock found — continuing in pipe mode");
            }
        }

        let (mut session, recorder, lock, has_progress) =
            build_session(&global, &project_root, &topic, &today())?;
        ui::banner(&topic, &backend.label());
        // If the profile is still the embedded generic template (or doesn't exist at all),
        // Usta doesn't know the user yet — a short introduction instruction is added to
        // the opening turn (spec Ç3a).
        let profile_generic = std::fs::read_to_string(global.join("USER.md"))
            .ok()
            .as_deref()
            .map(profile_is_generic)
            .unwrap_or(true);
        run_plain_loop(
            &mut backend,
            &mut session,
            &recorder,
            &project_root,
            &global,
            &topic,
            has_progress,
            intro.as_deref(),
            profile_generic,
            &mut watch_rx,
        )
        .await?;
        (session, recorder, lock)
    };

    if let Err(e) = flush_progress(&mut backend, &session, &project_root, true).await {
        ui::warn(&format!(
            "progress could not be updated: {e} — raw record left on disk: {}",
            recorder.path().display()
        ));
    } else if session.history().is_empty() {
        // Empty session: no file was ever created, nothing to mark.
    } else if let Err(e) = transcript::mark_done(recorder.path()) {
        ui::warn(&format!("session record could not be marked done: {e}"));
    }

    let _ = std::fs::remove_file(&lock);

    ui::notice("See you — keep getting in the water.");
    Ok(())
}
