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
mod help;
mod history;
mod index;
mod input;
mod materials;
mod migrate;
mod progress;
mod session;
mod slash;
mod tokens;
mod topic;
mod transcript;
mod tui;
mod ui;
mod visual;
mod watcher;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rustyline::DefaultEditor;

use crate::anthropic::Message;
use crate::backend::Backend;
use crate::cli::{Command, ResetTarget, parse_command};
use crate::session::Session;
use crate::slash::{
    GameCmd, apply_watch, game_on_turn, game_pref, game_streak_line, is_exam_command,
    parse_game_command, parse_watch_command, read_game_pref, restore_game_pref, set_game_pref,
    topic_has_goal,
};
use crate::topic::{finalize_slug, interpret_topic_input, slug_system, slugify_topic, TopicChoice};
use crate::transcript::Recorder;


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
/// Maximum number of files given feedback in a single debounce window — above
/// this it counts as a "bulk change" (git checkout, format-all): no LLM call.
const MAX_FEEDBACK_BATCH: usize = 5;

/// One-shot TR→EN protocol-token migration, run at the top of every command
/// dispatch path (start/topics/stats/reset) right after global root + project
/// `.usta` are known, before any file is read. Silent on `Ok(0)` (nothing to
/// migrate) and on `Err` — a migration failure must never abort the session,
/// it's surfaced as a warning and the command proceeds against whatever state
/// is on disk (pre- or partially-migrated).
fn run_migration(global: &Path, project_usta: Option<&Path>) {
    match migrate::run(global, project_usta) {
        Ok(0) => {}
        Ok(n) => ui::notice(&format!("migrated {n} file(s) to English protocol tokens (backup: .bak)")),
        Err(e) => ui::warn(&format!("token migration skipped: {e}")),
    }
}

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
                ui::warn(&format!("half-finished session record found (may not have been flushed): {}", p.display()));
            }
        } else {
            // List the stale records (dim), then ask ONCE: recover (default) or delete.
            for p in &stale {
                ui::notice(&format!("unflushed session record: {}", p.display()));
            }
            if confirm_recover(&format!("recover {} unflushed session(s)? [Y/n] ", stale.len())) {
                for p in &stale {
                    let Some(topic) = transcript::topic_from_record(p) else {
                        ui::warn(&format!("unrecognized session record name, leaving as-is: {}", p.display()));
                        continue;
                    };
                    match transcript::read_history(p) {
                        Err(e) => ui::warn(&format!("could not read session record ({e}) — leaving as-is: {}", p.display())),
                        Ok(history) if history.iter().filter(|m| m.role == "user").count() == 0 => {
                            // nothing to recover — mark done silently so the noise stops
                            let _ = transcript::mark_done(p);
                        }
                        Ok(history) => {
                            ui::notice(&format!("recovering unflushed session: {} — writing files…", p.display()));
                            let system = brain::load_system_prompt(&global, Some(&project_root), &topic, &today());
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
        ui::warn(&format!("progress could not be updated: {e} — raw record left on disk: {}", recorder.path().display()));
    } else if session.history().is_empty() {
        // Empty session: no file was ever created, nothing to mark.
    } else if let Err(e) = transcript::mark_done(recorder.path()) {
        ui::warn(&format!("session record could not be marked done: {e}"));
    }

    let _ = std::fs::remove_file(&lock);

    ui::notice("See you — keep getting in the water.");
    Ok(())
}

/// Plain (line-based) REPL loop: rustyline input thread + watcher + debounce
/// all in one select!. Runs when there's no TTY / in NO_COLOR — behavior identical
/// to the old main loop (banner is printed in main, drill + loop live here).
async fn run_plain_loop(
    backend: &mut Backend,
    session: &mut Session,
    recorder: &transcript::Recorder,
    project_root: &Path,
    global: &Path,
    topic: &str,
    has_progress: bool,
    intro: Option<&str>,
    profile_generic: bool,
    watch_rx: &mut tokio::sync::mpsc::UnboundedReceiver<PathBuf>,
) -> Result<()> {
    // Input thread + debounce state — specific to the plain path (rustyline).
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
    let mut input_rx = input::spawn("❯ ", ready_rx);
    let mut debouncer = watcher::Debouncer::new(std::time::Duration::from_millis(1000));
    let mut files = feedback::FileMemory::new();
    // Mentor docs are already in the system prompt — baseline them so an
    // unchanged re-save is a Skip, not a redundant full re-send (FIX: first-sight seed).
    seed_mentor_baseline(&mut files, project_root);

    // Opening drill: if progress exists from previous sessions, Usta speaks first,
    // warming up with 2-3 recall questions (testing effect — USTA.md rule).
    let project_known = progress::project_md_path(project_root).exists();
    if has_progress {
        let td = today();
        let gs = game_streak_line(global, &td);
        let progress_content = std::fs::read_to_string(progress::progress_path(project_root, topic)).unwrap_or_default();
        let due = crate::tui::welcome::due_questions(&progress_content, &td);
        let has_questions = crate::tui::welcome::drill_count(&progress_content) > 0;
        let opening = progress::opening_prompt(topic, profile_generic, project_known, gs.as_deref(), &due, has_questions);
        session.push_user(&opening);
        recorder.user(&opening);
        match ask_usta(backend, &session.system, session.history()).await {
            Ok(reply) => {
                let (clean, show_topic) = visual::extract_show_marker(&reply.text);
                print_reply(&clean, reply.web, reply.context_tokens, backend.context_window());
                recorder.assistant(&clean);
                session.push_assistant(clean);
                trigger_auto_visual(backend, session, project_root, topic, show_topic).await;
            }
            // Drill failed → don't block the session, fall silently back into normal flow.
            Err(e) => ui::warn(&format!("opening drill skipped: {e}")),
        }
    } else {
        // New topic: no approach/map yet — introduction turn, Usta speaks first.
        for note in crate::materials::convert_pdfs(project_root) {
            ui::notice(&note);
        }
        let mats = crate::materials::scan(project_root);
        let material_digest = crate::materials::combined_digests(&mats);
        let onboarding = progress::onboarding_prompt(topic, intro, profile_generic, project_known, material_digest.as_deref());
        session.push_user(&onboarding);
        recorder.user(&onboarding);
        match ask_usta(backend, &session.system, session.history()).await {
            Ok(reply) => {
                let (clean, show_topic) = visual::extract_show_marker(&reply.text);
                print_reply(&clean, reply.web, reply.context_tokens, backend.context_window());
                recorder.assistant(&clean);
                session.push_assistant(clean);
                trigger_auto_visual(backend, session, project_root, topic, show_topic).await;
            }
            Err(e) => ui::warn(&format!("introduction turn skipped: {e}")),
        }
    }

    let _ = ready_tx.send(()); // ilk prompt

    let mut watching = true;
    loop {
        tokio::select! {
            maybe_ev = input_rx.recv() => match maybe_ev {
                Some(input::InputEvent::Line(line)) => {
                    let line = line.trim().to_string();
                    if let Some(cmd) = parse_watch_command(&line) {
                        let (next, msg) = apply_watch(cmd, watching);
                        watching = next;
                        ui::notice(msg);
                        let _ = ready_tx.send(());
                        continue;
                    }
                    if help::is_help_command(&line) {
                        println!("{}", help::help_text());
                        let _ = ready_tx.send(());
                        continue;
                    }
                    if let Some(arg) = visual::parse_show_command(&line) {
                        let concept = arg.clone().unwrap_or_else(|| "visual".to_string());
                        match show_request(arg, last_assistant_text(session).as_deref()) {
                            None => ui::notice("nothing to visualize yet — explain something first, or use /show [topic]"),
                            Some(req) => run_visual_generation(backend, project_root, topic, &concept, &req).await,
                        }
                        let _ = ready_tx.send(());
                        continue;
                    }
                    if line.eq_ignore_ascii_case("/quit") {
                        break;
                    }
                    // /game: toggle persists to USER.md (shell-managed). Status is a local
                    // notice that never reaches the LLM. On/Off flip the pref + inject a
                    // mode-switch turn (swapped below) that flows through the normal ask so
                    // Usta applies the TEACHING.md Gamification rules from here on — same
                    // swap-and-fall-through shape as /exam.
                    let game_cmd = parse_game_command(&line);
                    if let Some(cmd) = &game_cmd {
                        match cmd {
                            GameCmd::Status => {
                                ui::notice(if game_pref(global) {
                                    "gamification is on"
                                } else {
                                    "gamification is off"
                                });
                                let _ = ready_tx.send(());
                                continue;
                            }
                            GameCmd::On | GameCmd::Off => {
                                let on = matches!(cmd, GameCmd::On);
                                if let Err(e) = set_game_pref(global, on) {
                                    ui::notice(&format!("could not save game preference: {e}"));
                                    let _ = ready_tx.send(());
                                    continue;
                                }
                                ui::notice(if on {
                                    "gamification on — XP, levels and badges are live"
                                } else {
                                    "gamification off — back to quiet mode"
                                });
                            }
                        }
                    }
                    // /exam and /game on|off: swap the outgoing text and fall through to the
                    // normal ask flow below — the typed command is already echoed by rustyline.
                    let line = if is_exam_command(&line) {
                        if !topic_has_goal(project_root, global, topic) {
                            ui::notice("no goal set for this topic — /exam needs a goal (exam/certificate); set one in the introduction");
                            let _ = ready_tx.send(());
                            continue;
                        }
                        progress::exam_prompt(topic)
                    } else if let Some(cmd) = game_cmd {
                        match cmd {
                            GameCmd::On => game_on_turn(
                                &std::fs::read_to_string(global.join("GAMIFICATION.md")).unwrap_or_default(),
                            ),
                            GameCmd::Off => "[GAME MODE OFF] Gamification is now OFF — stop all game narration.".to_string(),
                            GameCmd::Status => line, // unreachable: Status returns above
                        }
                    } else {
                        line
                    };
                    if !line.is_empty() {
                        session.push_user(&line);
                        recorder.user(&line);
                        match ask_usta(backend, &session.system, session.history()).await {
                            Ok(reply) => {
                                let (clean, show_topic) = visual::extract_show_marker(&reply.text);
                                print_reply(&clean, reply.web, reply.context_tokens, backend.context_window());
                                let tokens = reply.context_tokens;
                                recorder.assistant(&clean);
                                session.push_assistant(clean);
                                maybe_compact(backend, session, project_root, tokens).await;
                                trigger_auto_visual(backend, session, project_root, topic, show_topic).await;
                            }
                            Err(e) => ui::warn(&format!("error: {e}")),
                        }
                    }
                    let _ = ready_tx.send(());
                }
                Some(input::InputEvent::Eof) | None => break,
            },
            Some(path) = watch_rx.recv() => {
                debouncer.push(path, tokio::time::Instant::now());
            },
            _ = sleep_until_deadline(debouncer.deadline()), if debouncer.deadline().is_some() => {
                // Also runs while the user is at the prompt — genuine proactivity.
                println!(); // don't dirty a half-finished prompt line
                let batch = debouncer.flush();
                if batch.len() > MAX_FEEDBACK_BATCH {
                    ui::notice(&format!(
                        "bulk change ({} files) — feedback skipped, still watching",
                        batch.len()
                    ));
                    // Sync FileMemory silently: so the next single save doesn't
                    // produce a huge diff against this batch.
                    for path in batch {
                        if let Ok(c) = std::fs::read_to_string(&path) {
                            let _ = files.observe(&path, c);
                        }
                    }
                } else if !watching {
                    // Companion off: keep the diff baseline current, no LLM feedback.
                    for path in batch {
                        if let Ok(c) = std::fs::read_to_string(&path) {
                            let _ = files.observe(&path, c);
                        }
                    }
                } else {
                    for path in batch {
                        match handle_file_change(backend, session, &mut files, project_root, &path, recorder).await {
                            // handle_file_change no longer prints — the plain path applies
                            // its own presentation language (print_reply: web + gauge).
                            Ok(FileFeedback::Sessiz) => {}
                            Ok(FileFeedback::Bildirim(m)) => println!("{m}"),
                            Ok(FileFeedback::Yanit { tokens, reply, show_topic }) => {
                                print_reply(&reply.text, reply.web, reply.context_tokens, backend.context_window());
                                maybe_compact(backend, session, project_root, tokens).await;
                                trigger_auto_visual(backend, session, project_root, topic, show_topic).await;
                            }
                            // Deleted-before-we-read-it (a tool's transient temp file) or
                            // binary content (an image saved into the project) — not the
                            // user's business, skip silently.
                            Err(e) if is_silent_skip(&e) => {}
                            // Other failures (permission/etc.) — the REPL survives,
                            // but the user still sees the warn.
                            Err(e) => ui::warn(&format!("file feedback skipped: {}: {e}", path.display())),
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Wrap the LLM call in a spinner — don't leave silence while the user waits.
async fn ask_usta(
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
fn build_session(
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

async fn flush_core(
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
            ui::warn(&format!("global root could not be resolved — profile will be skipped this session: {e}"));
            None
        }
    };
    let dummy_global = PathBuf::new();
    let global_for_paths = global.as_deref().unwrap_or(&dummy_global);
    let p_path = flush_target("progress", project_root, global_for_paths, topic).unwrap();
    let a_path = flush_target("approach", project_root, global_for_paths, topic).unwrap();
    let c_path = flush_target("curriculum", project_root, global_for_paths, topic).unwrap();
    let prj_path =
        flush_target("project", project_root, global_for_paths, topic).unwrap();
    let ppg_path =
        flush_target("project-progress", project_root, global_for_paths, topic).unwrap();
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
            Err(e) => ui::warn(&format!("gamification preference could not be restored: {e}")),
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
                let map = cur.as_deref().and_then(crate::tui::welcome::curriculum_percent);
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

async fn flush_progress(
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
        ui::warn(&format!("interim checkpoint failed, compaction postponed: {e}"));
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
fn today() -> String {
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

/// Resolve the topic: explicit argument > TTY prompt > silent "general" default.
/// If stdin is piped (not a TTY), returns "general" directly instead of getting
/// stuck on a prompt that can't be answered. Short input is slugified locally;
/// if a sentence is written, the model infers WHAT the user wants to learn and
/// we let it pick the most sensible slug.
async fn resolve_topic(
    backend: &mut Backend,
    topic_arg: Option<String>,
    project_root: &Path,
    global: &Path,
) -> Result<(String, Option<String>)> {
    // Return: (topic, intro) — intro = the user's raw topic input; for a new topic
    // it's carried into the introduction turn as the "first reply" (None on resume/pipe paths).
    if let Some(raw) = topic_arg {
        let slug = slugify_topic(&raw);
        return Ok((slug, Some(raw)));
    }
    // Empty-stdin / pipe path is UNTOUCHED: falls straight to "general" instead of
    // getting stuck on a prompt that can't be answered.
    if !std::io::stdin().is_terminal() {
        return Ok((tokens::DEFAULT_TOPIC.to_string(), None));
    }
    // Show topics resumable in this project — Enter = continue with the most recent one.
    let index_content =
        std::fs::read_to_string(global.join("learner/index.md")).unwrap_or_default();
    let local = index::local_topics(project_root, &index_content);
    if !local.is_empty() {
        println!("saved: {} — Enter = continue with {}", local.join(", "), local[0]);
    }
    let mut rl = DefaultEditor::new()?;
    // The new-topic confirmation loop lives only here (plain path): a rejection
    // returns to the "What's the topic?" prompt — the equivalent of the TUI's
    // reject-and-ask-again flow. The resume/first-session paths never get stuck in this loop.
    loop {
        let line = match rl.readline("What's the topic? (write it short or as a sentence): ") {
            Ok(l) => l,
            // Ctrl-D / Ctrl-C → fall through to "general" without blocking.
            Err(_) => return Ok((tokens::DEFAULT_TOPIC.to_string(), None)),
        };
        let raw = line.trim();
        // Slash commands at topic entry (TUI parity): /help prints help; session-only
        // commands get a pointer instead of silently being slugged into a topic name.
        if help::is_help_command(raw) {
            println!("{}", help::help_text());
            continue;
        }
        if visual::parse_show_command(raw).is_some() || parse_watch_command(raw).is_some() {
            println!("that command works inside a session — pick a topic first");
            continue;
        }
        // Interpret the topic input: resume or new topic? (spec K1). In the plain path
        // the resume/new distinction only shows up in the slug — the TUI's visual notice
        // difference doesn't apply here.
        match interpret_topic_input(raw, &local, false) {
            None => return Ok((tokens::DEFAULT_TOPIC.to_string(), None)),
            Some(TopicChoice::Resume(t)) => return Ok((t, None)),
            Some(TopicChoice::New(raw)) => {
                // Short input (≤2 words) → local slug, don't waste an LLM call.
                let slug = if raw.split_whitespace().count() <= 2 {
                    slugify_topic(&raw)
                } else {
                    // Sentence → let the model infer what's wanted and pick the slug (for local topics, K2).
                    derive_slug(backend, &raw, &local).await
                };
                if local.contains(&slug) {
                    // The model resolved intent-to-continue to an existing slug — resume without confirmation.
                    return Ok((slug, None));
                }
                // First session (no topics saved yet) → confirmation exempt. Otherwise ask.
                if local.is_empty()
                    || confirm(&format!("Open new topic '{slug}'? [y/N] "), &["e", "evet", "y", "yes"])?
                {
                    return Ok((slug, Some(raw)));
                }
                println!(
                    "cancelled — Enter = continue with {}, or type another topic",
                    local[0]
                );
                // Loop restarts: "What's the topic?" is asked again.
            }
            Some(TopicChoice::Suggest) => unreachable!("plain path passes project_known=false"),
        }
    }
}

/// Last assistant reply in this session — the concept a bare `/show` visualizes.
pub(crate) fn last_assistant_text(session: &Session) -> Option<String> {
    session
        .history()
        .iter()
        .rev()
        .find(|m| m.role == "assistant")
        .and_then(|m| m.content.as_str())
        .map(|s| s.to_string())
}

/// Compose the visual mini-session user turn. `explicit` = `/show <topic>` argument.
pub(crate) fn show_request(explicit: Option<String>, last_reply: Option<&str>) -> Option<String> {
    match (explicit, last_reply) {
        (Some(t), last) => Some(match last {
            Some(l) => format!(
                "Create scenes that visually explain: {t}\n\nRecent explanation for context:\n{l}"
            ),
            None => format!("Create scenes that visually explain: {t}"),
        }),
        (None, Some(l)) => Some(format!(
            "Create scenes that visually explain the following explanation:\n{l}"
        )),
        (None, None) => None, // nothing to visualize yet
    }
}

/// Extract the topic slug from a sentence via the model (plain path). Error → local slug.
/// After the call the CLI session is UNCONDITIONALLY reset — so the slug mini-session
/// doesn't get resumed into the learning session and pollute the context (spec B1).
async fn derive_slug(backend: &mut Backend, raw: &str, known: &[String]) -> String {
    let history = [Message::user(raw)];
    let out = match ask_usta(backend, &slug_system(known), &history).await {
        Ok(reply) => finalize_slug(raw, &reply.text),
        Err(_) => slugify_topic(raw),
    };
    backend.reset_session();
    out
}

/// Lazily sets up the `.usta/` scaffold — lets `start` bootstrap itself, making
/// `usta init` optional. (1) Completes the global brain root (`~/.config/usta`):
/// code-owned files are synced with the embedded ones, user-owned files are preserved.
/// (2) If the project root can't be found by searching upward, sets up a new
/// project `.usta/` in `cwd` and returns `cwd`; if found, returns it as-is.
fn ensure_scaffold(cwd: &Path) -> Result<PathBuf> {
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
fn run_init() -> Result<()> {
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
fn run_topics() -> Result<()> {
    let global = config::global_root()?;
    let project_usta = std::env::current_dir()
        .ok()
        .and_then(|c| config::find_project_root(&c))
        .map(|root| root.join(".usta"));
    run_migration(&global, project_usta.as_deref());
    let content =
        std::fs::read_to_string(global.join("learner/index.md")).unwrap_or_default();
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
    let projects: Vec<String> = entries.iter().map(|e| e.project.display().to_string()).collect();
    let topic_w = entries.iter().map(|e| e.topic.width()).chain(std::iter::once("Topic".width())).max().unwrap_or(5);
    let proj_w = projects.iter().map(|p| p.width()).chain(std::iter::once("Project".width())).max().unwrap_or(7);
    let last = "Last session";
    let mut out = format!("{}  {}  {}\n", col_pad("Topic", topic_w), col_pad("Project", proj_w), last);
    out.push_str(&"─".repeat(topic_w + 2 + proj_w + 2 + last.width()));
    out.push('\n');
    for (e, p) in entries.iter().zip(&projects) {
        out.push_str(&format!("{}  {}  {}\n", col_pad(&e.topic, topic_w), col_pad(p, proj_w), e.date));
    }
    out
}

/// `usta stats` — this week's summary + streaks, read from the global session
/// history. No LLM needed; missing/empty history just renders the empty state.
fn run_stats() -> Result<()> {
    let global = config::global_root()?;
    let project_usta = std::env::current_dir()
        .ok()
        .and_then(|c| config::find_project_root(&c))
        .map(|root| root.join(".usta"));
    run_migration(&global, project_usta.as_deref());
    let content =
        std::fs::read_to_string(global.join("learner/history.md")).unwrap_or_default();
    let es = history::entries(&content);
    println!("{}", render_stats(&es, &today()));
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
    let topic_w = week.per_topic.iter().map(|t| unicode_width::UnicodeWidthStr::width(t.topic.as_str())).max().unwrap_or(0);
    for t in &week.per_topic {
        out.push_str(&format!("  {}   {} session(s)", col_pad(&t.topic, topic_w), t.sessions));
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
fn run_reset_topic(topic: &str) -> Result<()> {
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
        &format!("{} and its visuals will be deleted. Are you sure? [y/N] ", path.display()),
        &["e", "evet", "y", "yes"],
    )? {
        println!("cancelled.");
        return Ok(());
    }
    std::fs::remove_file(&path)
        .with_context(|| format!("could not delete: {}", path.display()))?;
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
const FACTORY_RESET_PROMPT: &str = "Everything will be permanently deleted. Type 'yes' to confirm: ";

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
fn run_reset_factory() -> Result<()> {
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
        std::fs::remove_dir_all(t)
            .with_context(|| format!("could not delete: {}", t.display()))?;
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
fn run_reset_profile() -> Result<()> {
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("no TTY — cannot get confirmation, profile not reset. Run in an interactive terminal.");
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
fn confirm(prompt: &str, yes: &[&str]) -> Result<bool> {
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
    !matches!(input.trim().to_lowercase().as_str(), "n" | "no" | "h" | "hayır" | "hayir")
}

/// Ask the startup recover/delete question. Default-YES variant of `confirm`
/// (which is default-NO). Never returns an error — on any stdin read failure it
/// defaults to RECOVER so a startup hiccup can't cause data loss or block startup.
fn confirm_recover(prompt: &str) -> bool {
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
        std::fs::rename(&old, &new)
            .with_context(|| format!("profile could not be moved: {} → {}", old.display(), new.display()))?;
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
///
/// Everything else (PermissionDenied, …) is a real failure the user should see.
pub(crate) fn is_silent_skip(e: &anyhow::Error) -> bool {
    e.chain()
        .filter_map(|cause| cause.downcast_ref::<std::io::Error>())
        .any(|io_err| matches!(io_err.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidData))
}

/// Build the injected user-turn for a watched-file change. Exercise files get
/// an exercise-review frame (assignment comparison, hint ladder, no solutions);
/// everything else keeps the original project-feedback wording VERBATIM.
pub(crate) fn feedback_frame(is_exercise: bool, path_display: &str, body: &str, is_diff: bool) -> String {
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
    let display_reply = backend::Reply { text: clean, web: reply.web, context_tokens: reply.context_tokens };
    Ok(FileFeedback::Yanit { tokens, reply: display_reply, show_topic })
}

/// Hand off Usta's reply to the presentation layer. Takes the already-CLEAN
/// text (marker stripped by the caller via `visual::extract_show_marker`) —
/// this function never sees a `[[show: ...]]` marker.
fn print_reply(text: &str, web: bool, context_tokens: Option<u64>, window: u64) {
    ui::print_usta_reply(text, web);
    ui::context_gauge(context_tokens, window);
}

/// Runs the `/show` generation flow (mini-session → HTML → browser open).
/// Shared by the explicit `/show <topic>` command and the auto-triggered
/// `[[show: ...]]` marker (Görev 4) — both get the SAME guarantees: isolated
/// mini-session, `backend.reset_session()` on every exit path (success, error,
/// invalid JSON), and the same "try /show again" notice on bad JSON.
async fn run_visual_generation(backend: &mut Backend, project_root: &Path, topic: &str, concept: &str, request: &str) {
    match ask_usta(backend, &visual::visual_system(), &[Message::user(request)]).await {
        Ok(reply) => {
            let json = progress::clean_markdown_reply(&reply.text);
            match visual::build_visual_html(&json) {
                Ok(html) => {
                    let path = visual::visual_path(project_root, topic, concept);
                    let dir = path.parent().map(|d| d.to_path_buf());
                    if let Some(d) = &dir {
                        let _ = std::fs::create_dir_all(d);
                    }
                    match std::fs::write(&path, html) {
                        Ok(()) => {
                            // Görev 5: keep the last 10 visuals per topic — prune AFTER
                            // the write, so `10` is the exact post-write count on disk.
                            if let Some(d) = &dir {
                                visual::prune_visuals(d, 10);
                            }
                            let opened = visual::open_in_browser(&path);
                            ui::notice(&format!(
                                "visual saved: {}{}",
                                path.display(),
                                if opened { "" } else { " (open it in your browser)" }
                            ));
                        }
                        Err(e) => ui::warn(&format!("error: {e}")),
                    }
                }
                Err(e) => ui::warn(&format!("visual generation failed ({e}) — try /show again")),
            }
        }
        Err(e) => ui::warn(&format!("error: {e}")),
    }
    backend.reset_session(); // mini-session must not leak into the CLI session (slug parity)
}

/// After a normal reply has been displayed and recorded, run the visual flow
/// if `[[show: ...]]` was found in it (Görev 4). No-op when `show_topic` is
/// `None`. Reuses `show_request` with the just-pushed clean reply as context —
/// same composition the explicit `/show <topic>` command uses.
async fn trigger_auto_visual(
    backend: &mut Backend,
    session: &Session,
    project_root: &Path,
    topic: &str,
    show_topic: Option<String>,
) {
    let Some(t) = show_topic else { return };
    if let Some(req) = show_request(Some(t.clone()), last_assistant_text(session).as_deref()) {
        ui::notice(&format!("visualizing: {t}…"));
        run_visual_generation(backend, project_root, topic, &t, &req).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_targets_includes_uncatalogued_cwd_project() {
        let idx = "## Records\n- rust | /p/a | 2026-08-01\n";
        // cwd project not in the catalog → still targeted
        let t = factory_targets(idx, Some(Path::new("/p/orphan")));
        assert!(t.contains(&PathBuf::from("/p/a/.usta")));
        assert!(t.contains(&PathBuf::from("/p/orphan/.usta")));
        // cwd project already catalogued → no duplicate
        let t2 = factory_targets(idx, Some(Path::new("/p/a")));
        assert_eq!(t2.iter().filter(|p| p.as_path() == Path::new("/p/a/.usta")).count(), 1);
        // no cwd root → catalog only
        let t3 = factory_targets(idx, None);
        assert_eq!(t3, vec![PathBuf::from("/p/a/.usta")]);
    }

    #[test]
    fn recover_choice_defaults_yes_only_explicit_no_deletes() {
        // default / lossless side → recover (true)
        assert!(recover_choice(""));
        assert!(recover_choice("   \n"));
        assert!(recover_choice("y"));
        assert!(recover_choice("evet"));
        assert!(recover_choice("garbage"));
        // explicit no → delete (false)
        assert!(!recover_choice("n"));
        assert!(!recover_choice("N"));
        assert!(!recover_choice("no"));
        assert!(!recover_choice("h"));
        assert!(!recover_choice("hayır"));
        assert!(!recover_choice("hayir")); // ASCII fallback
        assert!(!recover_choice("HAYIR")); // uppercase Turkish: lowercases to "hayir"
        assert!(!recover_choice("Hayır"));
    }

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
            flush_target("bilinmeyen", Path::new("/proje"), Path::new("/glob"), "rust"),
            None
        );
    }

    #[test]
    fn write_project_scaffold_creates_visible_mentor_dir() {
        let base = std::env::temp_dir().join(format!(
            "usta_main_test_mentor_scaffold_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        write_project_scaffold(&base).unwrap();
        assert!(base.join("mentor").is_dir());
        assert!(base.join("mentor/.gitkeep").is_file());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn write_project_scaffold_creates_visible_exercises_dir() {
        let base = std::env::temp_dir().join(format!(
            "usta_main_test_exercises_scaffold_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        write_project_scaffold(&base).unwrap();
        assert!(base.join("exercises").is_dir());
        assert!(base.join("exercises/.gitkeep").is_file());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn reset_topic_leaves_mentor_dir_untouched() {
        // reset deletes under `.usta/` only — mentor/ is the user's project doc,
        // possibly committed to their repo. Guard that contract with the same
        // path logic run_reset_topic uses (progress_path is under .usta).
        let root = Path::new("/tmp/proj");
        let p = progress::progress_path(root, "rust");
        assert!(p.starts_with(root.join(".usta")));
        assert!(!progress::project_md_path(root).starts_with(root.join(".usta")));
    }

    #[test]
    fn render_stats_full_quiet_and_empty() {
        let mk = |d: &str, t: &str| history::Entry {
            date: d.into(),
            topic: t.into(),
            map: Some(40),
            settled: Some(4),
        };
        let full = render_stats(&[mk("2026-08-14", "rust"), mk("2026-08-15", "rust")], "2026-08-15");
        assert!(full.contains("rust"));
        assert!(full.contains("2 session(s)"));
        assert!(full.contains("current streak: 2 day(s)"));

        // kırık seri: current 0 → yazılmaz, longest pozitif çerçeve
        let broken = render_stats(&[mk("2026-08-01", "rust")], "2026-08-15");
        assert!(!broken.contains("current streak"));
        assert!(broken.contains("longest streak"));
        assert!(broken.contains("quiet week"));

        let empty = render_stats(&[], "2026-08-15");
        assert!(empty.contains("no sessions recorded yet"));
    }

    #[test]
    fn render_stats_omits_missing_settled_segment() {
        // Entry has a map percentage but no settled count (e.g. curriculum exists
        // but has no items in "settled"/"deepened" state yet) — the "settled X → Y"
        // segment must be omitted entirely, not rendered as "None" or a dangling arrow,
        // while the "map X% → Y%" segment still renders normally.
        let entry = history::Entry {
            date: "2026-08-15".into(),
            topic: "rust".into(),
            map: Some(40),
            settled: None,
        };
        let out = render_stats(&[entry], "2026-08-15");
        assert!(out.contains("rust"));
        assert!(out.contains("1 session(s)"));
        assert!(out.contains("map 40% → 40%"));
        assert!(!out.contains("settled"));
        assert!(!out.contains("None"));

        // Both missing (e.g. topic has no curriculum yet at all) — neither segment renders.
        let both_none = history::Entry {
            date: "2026-08-15".into(),
            topic: "gtm".into(),
            map: None,
            settled: None,
        };
        let out2 = render_stats(&[both_none], "2026-08-15");
        assert!(out2.contains("gtm"));
        assert!(!out2.contains("map"));
        assert!(!out2.contains("settled"));
        assert!(!out2.contains("None"));
        assert!(!out2.contains("→"));
    }

    #[test]
    fn render_topics_table_aligns_columns_with_header_rule() {
        use std::path::PathBuf;
        let entries = vec![
            index::IndexEntry { topic: "rust".into(), project: PathBuf::from("~/projects/tokio-lab"), date: "2026-08-14".into() },
            index::IndexEntry { topic: "kaynak-ingest".into(), project: PathBuf::from("~/work/ingest"), date: "2026-08-11".into() },
        ];
        let out = render_topics_table(&entries);
        let lines: Vec<&str> = out.lines().collect();
        // Header, a dim `─` rule, then the rows — content preserved.
        assert!(lines[0].starts_with("Topic"));
        assert!(lines[1].chars().all(|c| c == '─'), "header rule line: {:?}", lines[1]);
        assert!(out.contains("rust"));
        assert!(out.contains("~/projects/tokio-lab"));
        assert!(out.contains("2026-08-11"));
        // Columns align: every data row's project column (starts with ~) begins
        // at the same character offset.
        assert_eq!(lines[2].find('~'), lines[3].find('~'), "project column misaligned: {lines:#?}");
    }

    #[test]
    fn render_stats_aligns_topic_column() {
        let mk = |d: &str, t: &str| history::Entry { date: d.into(), topic: t.into(), map: None, settled: None };
        // Two topics of different widths → the "session(s)" column must line up.
        let out = render_stats(&[mk("2026-08-15", "rust"), mk("2026-08-15", "kaynak-ingest")], "2026-08-15");
        // Only the per-topic rows (2-space indent) — NOT the "total:" footer line.
        let sess_lines: Vec<&str> = out.lines().filter(|l| l.starts_with("  ") && l.contains("session(s)")).collect();
        assert!(sess_lines.len() >= 2);
        use unicode_width::UnicodeWidthStr;
        let offsets: Vec<usize> = sess_lines.iter().map(|l| l.split_once("session(s)").map(|(a, _)| a.width()).unwrap()).collect();
        assert!(offsets.windows(2).all(|w| w[0] == w[1]), "session(s) column misaligned: {sess_lines:#?}");
    }

    #[test]
    fn profile_is_generic_matches_embedded_template_only() {
        let sablon = defaults::global_defaults()
            .into_iter()
            .find(|(rel, _, _)| *rel == "USER.md")
            .map(|(_, c, _)| c)
            .unwrap();
        assert!(profile_is_generic(sablon));
        assert!(profile_is_generic(&format!("{sablon}\n"))); // line-ending tolerance
        assert!(!profile_is_generic("# Öğrenci Profili — Anil\nkişisel"));
    }

    #[test]
    fn reset_profile_files_backs_up_and_writes_generic_template() {
        let base = std::env::temp_dir().join(format!("usta_reset_profile_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("USER.md"), "# Öğrenci Profili — Anil\nkişisel notlar").unwrap();

        reset_profile_files(&base).unwrap();

        let yeni = std::fs::read_to_string(base.join("USER.md")).unwrap();
        let sablon = defaults::global_defaults()
            .into_iter()
            .find(|(rel, _, _)| *rel == "USER.md")
            .map(|(_, c, _)| c)
            .unwrap();
        assert_eq!(yeni, sablon); // equal to the generic template
        assert_eq!(
            std::fs::read_to_string(base.join("USER.md.bak")).unwrap(),
            "# Öğrenci Profili — Anil\nkişisel notlar"
        ); // old content is in the backup
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn reset_profile_files_works_without_existing_profile() {
        let base = std::env::temp_dir().join(format!("usta_reset_profile_yok_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        reset_profile_files(&base).unwrap(); // also works with no existing file: directory is created, template is written, no .bak
        assert!(base.join("USER.md").exists());
        assert!(!base.join("USER.md.bak").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn migrate_moves_old_profile_once() {
        let base = std::env::temp_dir().join(format!("usta_migrate_moves_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("learner")).unwrap();
        std::fs::write(base.join("learner/profile.md"), "KIŞISEL").unwrap();

        let moved = migrate_profile_to_user_md(&base).unwrap();
        assert!(moved);
        assert_eq!(std::fs::read_to_string(base.join("USER.md")).unwrap(), "KIŞISEL");
        assert!(!base.join("learner/profile.md").exists());

        // Second call: the old path no longer exists → no-op.
        let moved_again = migrate_profile_to_user_md(&base).unwrap();
        assert!(!moved_again);
        assert_eq!(std::fs::read_to_string(base.join("USER.md")).unwrap(), "KIŞISEL");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn migrate_never_overwrites_existing_user_md() {
        let base = std::env::temp_dir().join(format!("usta_migrate_no_overwrite_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("learner")).unwrap();
        std::fs::write(base.join("learner/profile.md"), "ESKİ").unwrap();
        std::fs::write(base.join("USER.md"), "YENİ").unwrap();

        let moved = migrate_profile_to_user_md(&base).unwrap();
        assert!(!moved);
        assert_eq!(std::fs::read_to_string(base.join("USER.md")).unwrap(), "YENİ");
        // No risk of data loss is taken — the old file is also left in place.
        assert_eq!(std::fs::read_to_string(base.join("learner/profile.md")).unwrap(), "ESKİ");

        let _ = std::fs::remove_dir_all(&base);
    }

    /// `write_project_scaffold` sets up the `.usta/` scaffold in a temp directory —
    /// without touching `global_root()` at all (doesn't affect the real `~/.config`).
    #[test]
    fn write_global_defaults_syncs_code_owned_preserves_user_owned() {
        let base = std::env::temp_dir().join(format!(
            "usta_main_test_global_sync_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);

        // First write: everything is written.
        let first = write_global_defaults(&base).unwrap();
        assert!(first.iter().all(|(_, wrote)| *wrote));

        // Dirty it: make code-owned USTA.md stale, edit the user-owned profile.
        std::fs::write(base.join("USTA.md"), "eski sürüm").unwrap();
        std::fs::write(base.join("USER.md"), "kullanıcı düzenlemesi").unwrap();

        write_global_defaults(&base).unwrap();

        // Code-owned file was synced — the embedded up-to-date content came back.
        // Note: USTA.md turned into a behavior-free index via Task 1's brain-split
        // ("Hard Rules" now lives in RULES.md) — the assertion was updated to match
        // the current embedded content.
        let usta = std::fs::read_to_string(base.join("USTA.md")).unwrap();
        assert!(usta.contains("Intervention Map"));
        // User-owned file was preserved.
        assert_eq!(
            std::fs::read_to_string(base.join("USER.md")).unwrap(),
            "kullanıcı düzenlemesi"
        );

        // Nothing gets rewritten when there's no change.
        let third = write_global_defaults(&base).unwrap();
        assert!(third.iter().all(|(_, wrote)| !*wrote));

        let _ = std::fs::remove_dir_all(&base);
    }

    /// Regression test for the start-path ordering fix: `run_migration` must
    /// execute BEFORE `ensure_scaffold` (→ `write_global_defaults` →
    /// `config::needs_sync`, which READS `Ownership::Code` files like
    /// `approaches/software.md` and may resync them from the embedded English
    /// template). Those files are also migration-in-scope
    /// (`migrate::run` sweeps global `approaches/*.md`), so a legacy
    /// Turkish-token install needs migration to see the file BEFORE the
    /// scaffold can silently overwrite it — otherwise the original content is
    /// lost with no `.bak`.
    ///
    /// This exercises the two steps in the FIXED order (`migrate::run` then
    /// `write_global_defaults`, mirroring main()'s new ordering) and asserts
    /// the `.bak` captures the ORIGINAL Turkish content — proof migration ran
    /// first. It then asserts the scaffold still resynced the file afterward
    /// (to `Ownership::Code`'s embedded content), showing the overwrite did
    /// happen, just AFTER migration had already captured the legacy state.
    #[test]
    fn migration_before_scaffold_preserves_legacy_approaches_bak() {
        let base = std::env::temp_dir().join(format!(
            "usta_main_test_migration_before_scaffold_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("approaches")).unwrap();

        let legacy = "## Tercihler\n- gamification: on\n";
        let software_md = base.join("approaches/software.md");
        std::fs::write(&software_md, legacy).unwrap();

        // Sanity: the legacy content must differ from the embedded template,
        // otherwise write_global_defaults wouldn't touch it and this test
        // wouldn't prove anything about ordering.
        let embedded = defaults::global_defaults()
            .into_iter()
            .find(|(rel, _, _)| *rel == "approaches/software.md")
            .unwrap()
            .1;
        assert_ne!(legacy, embedded);

        // Fixed order: migration first, THEN scaffold sync — matches main()'s
        // new sequencing.
        migrate::run(&base, None).unwrap();
        write_global_defaults(&base).unwrap();

        // `.bak` sibling path — mirrors migrate::sibling()'s append-not-swap
        // semantics (`software.md` -> `software.md.bak`).
        let mut bak_os = software_md.clone().into_os_string();
        bak_os.push(".bak");
        let bak = PathBuf::from(bak_os);

        assert!(
            bak.exists(),
            ".bak must exist — migration must have run before the scaffold could overwrite the legacy file"
        );
        assert_eq!(
            std::fs::read_to_string(&bak).unwrap(),
            legacy,
            ".bak must hold the ORIGINAL Turkish content, captured before any scaffold resync"
        );

        // The scaffold DID resync the file afterward (Ownership::Code) — this
        // is expected and fine, it just had to happen after migration.
        let after = std::fs::read_to_string(&software_md).unwrap();
        assert_eq!(after, embedded);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn write_project_scaffold_creates_dirs_and_gitkeeps() {
        let base = std::env::temp_dir().join(format!(
            "usta_main_test_project_scaffold_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let results = write_project_scaffold(&base).unwrap();
        assert_eq!(results.len(), 6);
        assert!(results.iter().all(|(_, wrote)| *wrote));
        assert!(base.join(".usta/learner/progress").is_dir());
        assert!(base.join(".usta/approaches").is_dir());
        assert!(base.join(".usta/learner/progress/.gitkeep").is_file());
        assert!(base.join(".usta/approaches/.gitkeep").is_file());

        // Second call: directories already exist → `wrote` should be false, no panic.
        let results2 = write_project_scaffold(&base).unwrap();
        assert!(results2.iter().all(|(_, wrote)| !*wrote));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn write_project_scaffold_creates_visible_materials_dir() {
        let base = std::env::temp_dir().join(format!(
            "usta_main_test_materials_scaffold_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        write_project_scaffold(&base).unwrap();
        assert!(base.join("materials").is_dir());
        assert!(base.join("materials/.gitkeep").is_file());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Görev 5: scaffold writes `.usta/visuals/.gitignore` (`*`) — generated
    /// visual HTML never leaks into the user's git repo, while the files
    /// themselves stay on disk.
    #[test]
    fn write_project_scaffold_writes_visuals_gitignore() {
        let base = std::env::temp_dir().join(format!(
            "usta_main_test_visuals_gitignore_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        write_project_scaffold(&base).unwrap();

        let gitignore = base.join(".usta/visuals/.gitignore");
        assert!(gitignore.is_file());
        assert_eq!(std::fs::read_to_string(&gitignore).unwrap(), "*\n");

        let _ = std::fs::remove_dir_all(&base);
    }

    /// Görev 5: `usta reset <topic>` also removes `.usta/visuals/<topic>/`.
    /// `run_reset_topic` itself reads stdin (confirm) and `cwd`, so it isn't
    /// unit-testable directly — this tests the extracted deletion step,
    /// following the same temp-dir pattern as the scaffold tests above.
    #[test]
    fn remove_topic_visuals_deletes_a_populated_dir() {
        let base = std::env::temp_dir().join(format!(
            "usta_main_test_reset_visuals_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let topic_dir = base.join(".usta/visuals/rust");
        std::fs::create_dir_all(&topic_dir).unwrap();
        std::fs::write(topic_dir.join("2026-01-01-000000-ownership.html"), "x").unwrap();
        let sibling_dir = base.join(".usta/visuals/dns");
        std::fs::create_dir_all(&sibling_dir).unwrap();
        std::fs::write(sibling_dir.join("2026-01-01-000000-records.html"), "x").unwrap();

        remove_topic_visuals(&base, "rust").unwrap();

        assert!(!topic_dir.exists(), "topic visuals dir must be gone after reset");
        // Sibling topics are untouched — reset is scoped to the one topic.
        assert!(sibling_dir.is_dir());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn remove_topic_visuals_missing_dir_is_not_an_error() {
        let base = std::env::temp_dir().join(format!(
            "usta_main_test_reset_visuals_missing_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        // No `.usta/visuals/rust` was ever created (topic never ran `/show`).
        assert!(remove_topic_visuals(&base, "rust").is_ok());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn factory_reset_prompt_advertises_only_english_word() {
        // Display is English-only (Claude Code model: shell text = English base);
        // `evet` stays silently accepted in the `confirm(&["evet", "yes"])` call
        // but is never advertised — the display and the acceptance list are
        // deliberately different surfaces.
        assert!(FACTORY_RESET_PROMPT.contains("yes"));
        assert!(!FACTORY_RESET_PROMPT.contains("evet"));
    }

    #[test]
    fn show_request_composition() {
        assert!(show_request(None, None).is_none());
        let bare = show_request(None, Some("ownership explained")).unwrap();
        assert!(bare.contains("ownership explained"));
        let explicit = show_request(Some("dns".into()), Some("prior")).unwrap();
        assert!(explicit.contains("dns") && explicit.contains("prior"));
        let cold = show_request(Some("dns".into()), None).unwrap();
        assert!(cold.contains("dns"));
    }

    #[test]
    fn is_exercise_path_detects_exercises_dir() {
        let root = Path::new("/tmp/proj");
        assert!(is_exercise_path(root, Path::new("/tmp/proj/exercises/a.md")));
        assert!(is_exercise_path(root, Path::new("/tmp/proj/exercises/gtm/brief.md")));
        assert!(!is_exercise_path(root, Path::new("/tmp/proj/src/exercises.rs")));
        assert!(!is_exercise_path(root, Path::new("/tmp/proj/mentor/PROJECT.md")));
        // watcher may hand a path the root-strip doesn't cover — component scan fallback
        assert!(is_exercise_path(root, Path::new("/other/place/exercises/x.md")));
        assert!(!is_exercise_path(root, Path::new("/other/place/src/lib.rs")));
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
        let io_err = std::io::Error::new(std::io::ErrorKind::InvalidData, "stream did not contain valid UTF-8");
        let e = anyhow::Error::new(io_err).context("reading watched file");
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
