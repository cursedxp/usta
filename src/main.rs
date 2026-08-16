//! Usta — terminal Socratic learning mentor. Thin shell: CLI + LLM client +
//! file watcher + markdown brain loader. The intelligence lives in markdown.

mod anthropic;
mod backend;
mod brain;
mod check;
mod config;
mod defaults;
mod feedback;
mod help;
mod history;
mod index;
mod input;
mod materials;
mod progress;
mod session;
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
use crate::session::Session;
use crate::transcript::Recorder;


/// Once this ratio is reached, an interim checkpoint + compaction is triggered.
const COMPACT_THRESHOLD: f64 = 0.70;
/// Number of most recent messages kept in history after compaction.
const COMPACT_KEEP_LAST: usize = 4;
/// Note prepended to history after compaction — tells the model the context
/// was compacted and the essence now lives in the files.
const COMPACT_NOTE: &str = "[ARA KAYIT] Bağlam sıkıştırıldı. Önceki konuşmanın özü \
system prompt'taki progress/curriculum/approach dosyalarına yazıldı — güncel durum \
orada. Kaldığımız yerden devam et; kullanıcıya kompaksiyonu anlatma.";
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

    // Set up `.usta/` silently if missing — `usta init` is no longer a mandatory
    // pre-step, `start` bootstraps itself (see ensure_scaffold).
    let cwd = std::env::current_dir()?;
    let had_project_root = config::find_project_root(&cwd).is_some();
    let project_root = ensure_scaffold(&cwd)?;
    if !had_project_root {
        ui::notice(".usta/ set up");
    }

    // Global brain + project root are merged to produce the system prompt (hybrid
    // model — see brain.rs). build_session uses this.
    let global = config::global_root()?;

    // File watcher — spawned ONCE (starts a thread), then passed by (&mut) into
    // the running path. Input thread + debounce state are path-specific:
    // the plain path uses rustyline, the TUI path uses crossterm EventStream.
    let mut watch_rx = watcher::spawn(&project_root)?;

    let stale = transcript::find_unfinished(&project_root);
    if !stale.is_empty() {
        let tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
        if !tty {
            // pipe/script: EXACT existing behavior — warn only, no LLM calls
            for p in &stale {
                ui::warn(&format!("half-finished session record found (may not have been flushed): {}", p.display()));
            }
        } else {
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
                        // Salvage is a standalone conversation — don't let its CLI session_id
                        // leak into the next salvage or the real session that follows.
                        backend.reset_session();
                    }
                }
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

    // Opening drill: if progress exists from previous sessions, Usta speaks first,
    // warming up with 2-3 recall questions (testing effect — USTA.md rule).
    let project_known = progress::project_md_path(project_root).exists();
    if has_progress {
        let gs = game_streak_line(global, &today());
        let opening = progress::opening_prompt(topic, profile_generic, project_known, gs.as_deref());
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
                            GameCmd::On => "[GAME MODE ON] Gamification is now ON — apply the Gamification rules from TEACHING.md from this point on.".to_string(),
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
                            // Binary/deleted file etc. — pass silently, the REPL survives.
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
    session.compact(COMPACT_KEEP_LAST, COMPACT_NOTE);
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

/// Reset scope.
#[derive(Debug, PartialEq)]
pub enum ResetTarget {
    /// Just one topic's progress in the current project.
    Topic(String),
    /// All known project `.usta/`s + global brain — zero point.
    Factory,
    /// Global user profile — reverts to the embedded generic template (with backup).
    Profile,
}

/// Command-line command — argument parsing in one place, pure and testable.
#[derive(Debug, PartialEq)]
pub enum Command {
    /// `usta init` — set up the scaffold, print per-file status.
    Init,
    /// `usta topics` — topic list from the global catalog.
    Topics,
    /// `usta stats` — this week's summary + streaks (ADHD-safe framing).
    Stats,
    /// `usta reset <topic>` — delete progress (with confirmation) + drop from the catalog.
    Reset(ResetTarget),
    /// `usta` / `usta start [topic]` — learning session.
    Start(Option<String>),
}

/// Turn arguments into a command. Unknown command → clear error, no silent surprises.
pub fn parse_command(args: &[String]) -> Result<Command> {
    let mut rest = args.iter().skip(1);
    match rest.next().map(String::as_str) {
        None => Ok(Command::Start(None)),
        Some("start") => Ok(Command::Start(rest.next().cloned())),
        Some("init") => Ok(Command::Init),
        Some("topics") => Ok(Command::Topics),
        Some("stats") => Ok(Command::Stats),
        Some("reset") => match rest.next().map(String::as_str) {
            Some("--factory") => Ok(Command::Reset(ResetTarget::Factory)),
            Some("--profile") | Some("--profil") => Ok(Command::Reset(ResetTarget::Profile)),
            Some(topic) => Ok(Command::Reset(ResetTarget::Topic(slugify_topic(topic)))),
            None => anyhow::bail!("usage: usta reset <topic>  |  --factory  |  --profile"),
        },
        Some(other) => anyhow::bail!(
            "unknown command: '{other}'. Commands: start [topic], init, topics, stats, reset <topic>|--factory|--profile"
        ),
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
        return Ok(("genel".to_string(), None));
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
            Err(_) => return Ok(("genel".to_string(), None)),
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
            None => return Ok(("genel".to_string(), None)),
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

/// Companion (file-watch feedback) slash command. Slash lines never reach the LLM.
#[derive(Debug, PartialEq)]
pub(crate) enum WatchCmd { On, Off, Toggle }

pub(crate) fn parse_watch_command(line: &str) -> Option<WatchCmd> {
    // Case-insensitive: /WATCH OFF, /Watch also work (forgiving slash commands).
    match line.trim().to_ascii_lowercase().as_str() {
        "/watch" => Some(WatchCmd::Toggle),
        "/watch on" => Some(WatchCmd::On),
        "/watch off" => Some(WatchCmd::Off),
        _ => None,
    }
}

pub(crate) fn apply_watch(cmd: WatchCmd, cur: bool) -> (bool, &'static str) {
    let next = match cmd {
        WatchCmd::On => true,
        WatchCmd::Off => false,
        WatchCmd::Toggle => !cur,
    };
    let msg = if next {
        "companion on — watching your files"
    } else {
        "companion paused — file feedback off"
    };
    (next, msg)
}

/// Gamification slash command (`/game`). Slash lines never reach the LLM.
#[derive(Debug)]
pub(crate) enum GameCmd { On, Off, Status }

pub(crate) fn parse_game_command(line: &str) -> Option<GameCmd> {
    let t = line.trim();
    if t == "/game" {
        return Some(GameCmd::Status);
    }
    let rest = t.strip_prefix("/game ")?;
    match rest.trim().to_ascii_lowercase().as_str() {
        "on" => Some(GameCmd::On),
        "off" => Some(GameCmd::Off),
        _ => None,
    }
}

/// Shell-managed preference line in USER.md (`## Tercihler` section).
/// The closing flush is told to keep this section as-is.
pub(crate) fn game_pref(global: &Path) -> bool {
    std::fs::read_to_string(global.join("USER.md"))
        .map(|c| c.lines().any(|l| l.trim() == "- gamification: on"))
        .unwrap_or(false)
}

/// Opening `[GAME]` streak line, shell-computed from the global history log — only
/// when gamification is on. ADHD-safe: a broken streak (current 0) shows the LONGEST,
/// never `streak: 0`; no history at all yields None (no game line).
pub(crate) fn game_streak_line(global: &Path, today: &str) -> Option<String> {
    if !game_pref(global) {
        return None;
    }
    let content = std::fs::read_to_string(global.join("learner/history.md")).ok()?;
    let es = history::entries(&content);
    if es.is_empty() {
        return None;
    }
    let cur = history::current_streak(&es, today);
    let longest = history::longest_streak(&es);
    if cur > 0 {
        Some(format!("streak: {cur} day(s) (longest {longest})"))
    } else if longest > 0 {
        Some(format!("longest streak: {longest} day(s)"))
    } else {
        None
    }
}

pub(crate) fn set_game_pref(global: &Path, on: bool) -> Result<()> {
    let path = global.join("USER.md");
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let value = if on { "- gamification: on" } else { "- gamification: off" };
    let new = if content.lines().any(|l| l.trim().starts_with("- gamification:")) {
        let had_trailing_newline = content.ends_with('\n');
        let mut rebuilt = content
            .lines()
            .map(|l| if l.trim().starts_with("- gamification:") { value } else { l })
            .collect::<Vec<_>>()
            .join("\n");
        if had_trailing_newline && !rebuilt.ends_with('\n') {
            rebuilt.push('\n');
        }
        rebuilt
    } else if content.contains("## Tercihler") {
        content.replace("## Tercihler", &format!("## Tercihler\n{value}"))
    } else {
        format!("{}\n\n## Tercihler\n{value}\n", content.trim_end())
    };
    progress::write_atomic(&path, &new)
}

/// Raw on-disk state of the shell-managed `- gamification:` line in USER.md.
/// `None` = no such line (user never toggled `/game`); `Some(true/false)` = line
/// present with value. Unlike `game_pref`, this distinguishes "line absent" from
/// "line present and off" — needed to know whether a restore should touch the file.
pub(crate) fn read_game_pref(global: &Path) -> Option<bool> {
    let content = std::fs::read_to_string(global.join("USER.md")).ok()?;
    content.lines().find_map(|l| {
        let v = l.trim().strip_prefix("- gamification:")?;
        Some(v.trim() == "on")
    })
}

/// Shell restore guarantee for the `/game` preference. `before` is the raw state
/// captured (via `read_game_pref`) BEFORE the closing flush rewrote USER.md. If the
/// model dropped the line or flipped its value, the captured value is written back.
/// `before == None` (user never toggled) is left strictly untouched — no line is added.
/// Returns whether a restore was performed. Pure: does exactly one conditional write.
pub(crate) fn restore_game_pref(global: &Path, before: Option<bool>) -> Result<bool> {
    let Some(want) = before else {
        return Ok(false);
    };
    if read_game_pref(global) == Some(want) {
        return Ok(false); // model honored the KEEP rule — nothing to do
    }
    set_game_pref(global, want)?;
    Ok(true)
}

/// Mock-exam slash command (`/exam`) — same exact-match pattern as `help::is_help_command`.
pub(crate) fn is_exam_command(line: &str) -> bool {
    line.trim() == "/exam"
}

/// Does this topic have a goal (## Hedef)? Same approach-file priority as
/// brain.rs GOAL loading: project override wins over global — keep in sync.
pub(crate) fn topic_has_goal(project_root: &Path, global: &Path, topic: &str) -> bool {
    let override_path = progress::approach_path(project_root, topic);
    let path = if override_path.exists() {
        override_path
    } else {
        global.join("approaches").join(format!("{topic}.md"))
    };
    std::fs::read_to_string(path)
        .map(|c| c.contains("## Hedef"))
        .unwrap_or(false)
}

/// System prompt that extracts a topic slug from a sentence — used by both the plain
/// path (`derive_slug`) and the TUI topic entry.
pub(crate) const SLUG_SYSTEM: &str = "Reduce what the user wants to learn/do to A SINGLE short \
    file-name slug. Rules: lowercase only, ascii (no accented characters), words separated \
    by hyphens, AT MOST 3 words, filler words (i/a/with/make/want) are dropped. \
    RETURN ONLY the slug — no explanation, no quotes, no punctuation. \
    Example: 'i want to build a todo app with rust' -> rust-todo";

/// Slug system prompt — if there are saved topics, resume-awareness is added
/// (spec K2): the model converts intent-to-continue into the existing slug, and the flow counts it as Resume.
pub(crate) fn slug_system(known: &[String]) -> String {
    if known.is_empty() {
        return SLUG_SYSTEM.to_string();
    }
    format!(
        "{SLUG_SYSTEM}\n\nExisting topics: {list}. If what the user wrote is a request \
         to CONTINUE one of these topics (picking up the same work, 'where we left \
         off', referencing prior work), return ONLY that topic's slug VERBATIM. If \
         it's a new topic, generate a new slug.",
        list = known.join(", ")
    )
}

/// Convert the model's slug reply into the final slug — turn hyphens into spaces
/// and guarantee it via `slugify_topic`; if it falls back to "general", derive a
/// local slug from the raw input instead. Pure.
pub(crate) fn finalize_slug(raw: &str, model_reply: &str) -> String {
    let s = slugify_topic(&model_reply.trim().replace(['-', '_'], " "));
    if s == "general" || s == "genel" {
        slugify_topic(raw)
    } else {
        s
    }
}

/// System prompt for the one-shot start suggestion (spec: project-aware start).
/// Mirrors the slug mini-session: single call, session reset afterwards.
pub(crate) fn start_suggest_system() -> String {
    "You are Usta, a Socratic engineering mentor. The user has a project \
     definition (given in the user message) but does NOT know where to start \
     learning. Propose the single best starting topic. Reply in the language \
     of the project file. FIRST line must be exactly `KONU: <topic-slug>` \
     (lowercase, hyphenated, 1-3 words). Then 2-4 sentences: why this topic \
     first, and ONE concrete first step small enough to start today. No \
     greeting, no markdown headings, nothing after the suggestion."
        .to_string()
}

/// Parse the suggestion reply: first `KONU:` line → slug (normalized through
/// slugify_topic), remaining lines → suggestion text shown to the user.
/// No `KONU:` marker or empty slug → None (caller falls back to manual entry).
pub(crate) fn parse_start_suggestion(reply: &str) -> Option<(String, String)> {
    let mut lines = reply.trim().lines();
    let first = lines.next()?.trim();
    let rest_raw = first.strip_prefix("KONU:")?;
    // `slugify_topic` never returns an empty string — it falls back to
    // "genel" for empty/whitespace input. So the emptiness check MUST happen
    // here, before slugify_topic runs, or a blank `KONU:` line would wrongly
    // parse to Some(("genel", ...)) instead of None.
    if rest_raw.trim().is_empty() {
        return None;
    }
    // `slugify_topic` splits on whitespace only, so a hyphen already inside
    // the KONU value (e.g. "rust-temelleri") would otherwise be stripped and
    // the words glued together ("rusttemelleri"). Turn hyphens/underscores
    // into spaces first, same trick `finalize_slug` uses for model replies.
    let slug = slugify_topic(&rest_raw.replace(['-', '_'], " "));
    let text = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    Some((slug, text))
}

/// New-topic confirmation text (for TUI tui_confirm). The plain path uses its own
/// `[e/H]` rustyline format — the wording is deliberately different, the two surfaces are separate.
pub(crate) fn new_topic_confirm_msg(slug: &str) -> String {
    format!("new topic: {slug} — open it? [e = yes / any other key = go back]")
}

/// Interpret the topic input: resume or new topic? (spec K1)
#[derive(Debug)]
pub(crate) enum TopicChoice {
    /// Resume an existing project-local topic.
    Resume(String),
    /// New-topic flow — raw input (the caller slugifies it).
    New(String),
    /// Empty Enter with no resumable topic but a filled mentor/PROJECT.md —
    /// Usta proposes where to start (spec: project-aware start).
    Suggest,
}

/// Deterministic selection rules — order follows spec §3/K1's table. `None` =
/// swallow the input (empty + no topic to resume). No LLM; sentences return
/// `New`, K2 (slug_system) kicks in there.
pub(crate) fn interpret_topic_input(raw: &str, local: &[String], project_known: bool) -> Option<TopicChoice> {
    let raw = raw.trim();
    // 1-2: empty Enter.
    if raw.is_empty() {
        return match local.first() {
            Some(t) => Some(TopicChoice::Resume(t.clone())), // resume wins over suggest
            None if project_known => Some(TopicChoice::Suggest),
            None => None,
        };
    }
    // 3: numeric selection.
    if let Ok(n) = raw.parse::<usize>() {
        if n >= 1 && n <= local.len() {
            return Some(TopicChoice::Resume(local[n - 1].clone()));
        }
    }
    // 4: slug match.
    let slug = slugify_topic(raw);
    if let Some(t) = local.iter().find(|t| **t == slug) {
        return Some(TopicChoice::Resume(t.clone()));
    }
    // 5: short resume pattern (substring after deasciify).
    if !local.is_empty() && raw.split_whitespace().count() <= 4 {
        let d: String = raw.chars().map(deasciify).collect::<String>().to_lowercase();
        const RESUME_WORDS: &[&str] = &["devam", "kaldigimiz", "kaldigim", "continue", "resume"];
        if RESUME_WORDS.iter().any(|w| d.contains(w)) {
            return Some(TopicChoice::Resume(local[0].clone()));
        }
    }
    // 6: new topic.
    Some(TopicChoice::New(raw.to_string()))
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

/// Reduce a Turkish letter to ascii + lowercase; lowercase everything else.
fn deasciify(c: char) -> char {
    match c {
        'ç' | 'Ç' => 'c',
        'ğ' | 'Ğ' => 'g',
        'ı' | 'İ' | 'I' => 'i',
        'ö' | 'Ö' => 'o',
        'ş' | 'Ş' => 's',
        'ü' | 'Ü' => 'u',
        other => other.to_ascii_lowercase(),
    }
}

/// Turn free text into a topic slug — pure function, testable.
/// Rule: simplify Turkish characters, lowercase, take at most the FIRST 3
/// words, keep only ascii alphanumeric characters in each word, join words
/// with hyphens. Empty result → `"genel"`.
/// "temel Linux güvenliği" → `temel-linux-guvenligi`.
pub fn slugify_topic(input: &str) -> String {
    // Filler words, compared against their deasciified (ç→c…) form — kept out
    // of the slug, so "ben rust ile bir todo yapmak istiyorum" → "rust-todo".
    const STOPWORDS: &[&str] = &[
        "ben", "bir", "ile", "ve", "icin", "bu", "su", "yapmak", "yapmayi",
        "istiyorum", "ogrenmek", "ogreniyorum", "istiyor", "bana", "de", "da",
        "the", "a", "an", "to", "learn", "want", "make", "build",
    ];
    let words: Vec<String> = input
        .split_whitespace()
        .map(|w| {
            w.chars()
                .map(deasciify)
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
        })
        .filter(|w| !w.is_empty() && !STOPWORDS.contains(&w.as_str()))
        .take(3)
        .collect();
    if words.is_empty() {
        "genel".to_string()
    } else {
        words.join("-")
    }
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
    let global = config::global_root()?;
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
const FACTORY_RESET_PROMPT: &str = "Everything will be permanently deleted. Type 'yes' (or 'evet') to confirm: ";

/// `usta reset --factory` — deletes the `.usta/` of every project in the catalog +
/// the global brain. The next `usta` run sets everything back up from defaults
/// (bootstrap) — Usta starts as if it never knew the user.
fn run_reset_factory() -> Result<()> {
    let global = config::global_root()?;
    let index_content =
        std::fs::read_to_string(global.join("learner/index.md")).unwrap_or_default();
    let mut targets: Vec<PathBuf> = index::entries(&index_content)
        .into_iter()
        .map(|e| e.project.join(".usta"))
        .filter(|p| p.is_dir())
        .collect();
    targets.sort();
    targets.dedup();

    println!("FACTORY RESET — will be deleted:");
    for t in &targets {
        println!("  {}", t.display());
    }
    println!("  {} (global brain)", global.display());
    println!("Note: old projects not in the catalog are NOT in this list.");
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
    fn is_exam_command_exact_only() {
        assert!(is_exam_command("/exam"));
        assert!(is_exam_command("  /exam  "));
        assert!(!is_exam_command("/exam now"));
        assert!(!is_exam_command("exam"));
        assert!(!is_exam_command("/examx"));
    }

    #[test]
    fn topic_has_goal_override_priority() {
        let base = std::env::temp_dir().join(format!("usta_exam_goal_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let project = base.join("proj");
        let global = base.join("global");
        std::fs::create_dir_all(project.join(".usta/approaches")).unwrap();
        std::fs::create_dir_all(global.join("approaches")).unwrap();

        // yalnız global hedefli → true
        std::fs::write(global.join("approaches/rust.md"), "yaklaşım\n## Hedef\nsınav").unwrap();
        assert!(topic_has_goal(&project, &global, "rust"));

        // override VAR ama hedefsiz → override kazanır → false
        std::fs::write(project.join(".usta/approaches/rust.md"), "yaklaşım hedefsiz").unwrap();
        assert!(!topic_has_goal(&project, &global, "rust"));

        // override hedefli → true
        std::fs::write(project.join(".usta/approaches/rust.md"), "## Hedef\nCEFR B2").unwrap();
        assert!(topic_has_goal(&project, &global, "rust"));

        // hiç dosya yok → false
        assert!(!topic_has_goal(&project, &global, "linux"));

        let _ = std::fs::remove_dir_all(&base);
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
    fn slugify_lowercases_simple_word() {
        assert_eq!(slugify_topic("JavaScript"), "javascript");
    }

    #[test]
    fn slugify_hyphenates_short_phrase_and_deasciifies() {
        assert_eq!(slugify_topic("temel Linux güvenliği"), "temel-linux-guvenligi");
        assert_eq!(slugify_topic("todo app"), "todo-app");
    }

    #[test]
    fn slugify_drops_non_alnum_chars() {
        assert_eq!(slugify_topic("C++"), "c");
    }

    #[test]
    fn slugify_caps_at_three_content_words() {
        assert_eq!(slugify_topic("alfa beta gama delta"), "alfa-beta-gama");
    }

    #[test]
    fn slugify_strips_stopwords_from_sentence() {
        // "ben ... ile bir ... yapmak istiyorum" — filler words are dropped.
        assert_eq!(
            slugify_topic("ben rust ile bir todo uygulaması yapmak istiyorum"),
            "rust-todo-uygulamasi"
        );
        assert_eq!(slugify_topic("Rust öğreniyorum"), "rust");
    }

    #[test]
    fn slugify_blank_input_falls_back_to_genel() {
        assert_eq!(slugify_topic("   "), "genel");
        assert_eq!(slugify_topic(""), "genel");
    }

    #[test]
    fn parse_bare_is_start_without_topic() {
        let args = vec!["usta".to_string()];
        assert_eq!(parse_command(&args).unwrap(), Command::Start(None));
    }

    #[test]
    fn parse_start_keeps_topic_arg() {
        let args = vec!["usta".into(), "start".into(), "javascript".into()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Start(Some("javascript".to_string()))
        );
    }

    #[test]
    fn parse_start_without_arg_is_start_none() {
        let args = vec!["usta".into(), "start".into()];
        assert_eq!(parse_command(&args).unwrap(), Command::Start(None));
    }

    #[test]
    fn parse_init_and_topics() {
        assert_eq!(
            parse_command(&["usta".into(), "init".into()]).unwrap(),
            Command::Init
        );
        assert_eq!(
            parse_command(&["usta".into(), "topics".into()]).unwrap(),
            Command::Topics
        );
    }

    #[test]
    fn parse_stats() {
        assert_eq!(
            parse_command(&["usta".into(), "stats".into()]).unwrap(),
            Command::Stats
        );
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
        // but has no items in "oturdu"/"derinleşildi" state yet) — the "settled X → Y"
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
    fn parse_unknown_command_errors() {
        assert!(parse_command(&["usta".into(), "rust".into()]).is_err());
    }

    #[test]
    fn parse_reset_topic_is_slugified() {
        let args = vec!["usta".into(), "reset".into(), "C++".into()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Reset(ResetTarget::Topic("c".to_string()))
        );
    }

    #[test]
    fn parse_reset_without_arg_errors() {
        assert!(parse_command(&["usta".into(), "reset".into()]).is_err());
    }

    #[test]
    fn parse_reset_factory_flag() {
        let args = vec!["usta".into(), "reset".into(), "--factory".into()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Reset(ResetTarget::Factory)
        );
    }

    #[test]
    fn parse_reset_profile_flag_both_spellings() {
        let args = |s: &str| vec!["usta".to_string(), "reset".to_string(), s.to_string()];
        assert_eq!(parse_command(&args("--profile")).unwrap(), Command::Reset(ResetTarget::Profile));
        assert_eq!(parse_command(&args("--profil")).unwrap(), Command::Reset(ResetTarget::Profile));
        // Regression: topic and factory unchanged.
        assert_eq!(parse_command(&args("--factory")).unwrap(), Command::Reset(ResetTarget::Factory));
        assert!(matches!(parse_command(&args("rust")).unwrap(), Command::Reset(ResetTarget::Topic(t)) if t == "rust"));
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
    fn finalize_slug_uses_model_reply_then_slugifies() {
        // Model returns a hyphenated slug → hyphens are preserved, slugify guarantees it.
        assert_eq!(finalize_slug("ben golang öğrenmek istiyorum", "golang-web"), "golang-web");
        // If the model returns noisy output it still gets slugified.
        assert_eq!(finalize_slug("x", "Rust Todo"), "rust-todo");
    }

    #[test]
    fn finalize_slug_falls_back_to_raw_when_model_gives_genel() {
        // If the model says "general", derive a local slug from the raw input instead.
        assert_eq!(finalize_slug("temel linux güvenliği", "general"), "temel-linux-guvenligi");
    }

    #[test]
    fn slug_system_injects_known_topics() {
        let s = slug_system(&["linux-guvenlik".to_string(), "rust".to_string()]);
        assert!(s.contains("linux-guvenlik, rust"));
        assert!(s.contains("CONTINUE"));
    }

    #[test]
    fn slug_system_without_topics_is_base_only() {
        let s = slug_system(&[]);
        assert!(s.contains("slug"));
        assert!(!s.contains("Existing topics"));
    }

    #[test]
    fn start_suggest_system_defines_konu_contract() {
        let s = start_suggest_system();
        assert!(s.contains("KONU:"));
        assert!(s.contains("first step"));
    }

    #[test]
    fn parse_start_suggestion_splits_slug_and_text() {
        let reply = "KONU: rust-temelleri\nStart with Rust because the backend is Rust.\nFirst step: cargo new.";
        let (slug, text) = parse_start_suggestion(reply).unwrap();
        assert_eq!(slug, "rust-temelleri");
        assert!(text.contains("First step"));
        assert!(!text.contains("KONU:"));
    }

    #[test]
    fn parse_start_suggestion_normalizes_messy_slug_line() {
        let (slug, _) = parse_start_suggestion("KONU: Rust Temelleri!\ngerekçe").unwrap();
        assert_eq!(slug, "rust-temelleri");
    }

    #[test]
    fn parse_start_suggestion_tolerates_missing_text_rejects_missing_konu() {
        let (slug, text) = parse_start_suggestion("KONU: rust").unwrap();
        assert_eq!(slug, "rust");
        assert_eq!(text, "");
        assert!(parse_start_suggestion("just prose, no marker").is_none());
        assert!(parse_start_suggestion("KONU:   \ntext").is_none());
    }

    #[test]
    fn factory_reset_prompt_names_both_confirmation_words() {
        // The prompt must mention both accepted words so it matches the SPEC and
        // the `confirm(&["evet", "yes"])` call — neither drifts from the other.
        assert!(FACTORY_RESET_PROMPT.contains("yes"));
        assert!(FACTORY_RESET_PROMPT.contains("evet"));
    }

    #[test]
    fn interpret_empty_resumes_latest_or_swallows() {
        let local = vec!["son-konu".to_string(), "eski".to_string()];
        assert!(matches!(interpret_topic_input("", &local, false), Some(TopicChoice::Resume(t)) if t == "son-konu"));
        assert!(interpret_topic_input("  ", &[], false).is_none()); // no topic → swallow
    }

    #[test]
    fn interpret_digit_selects_from_list_out_of_range_is_new() {
        let local = vec!["a".to_string(), "b".to_string()];
        assert!(matches!(interpret_topic_input("2", &local, false), Some(TopicChoice::Resume(t)) if t == "b"));
        assert!(matches!(interpret_topic_input("5", &local, false), Some(TopicChoice::New(r)) if r == "5"));
    }

    #[test]
    fn interpret_existing_slug_match_resumes() {
        let local = vec!["linux-guvenlik".to_string()];
        // Slugify match: Turkish spelling is caught too.
        assert!(matches!(
            interpret_topic_input("Linux Güvenlik", &local, false),
            Some(TopicChoice::Resume(t)) if t == "linux-guvenlik"
        ));
    }

    #[test]
    fn interpret_resume_phrases_short_input_only() {
        let local = vec!["son-konu".to_string()];
        for s in ["devam", "devam edelim", "kaldığımız yerden devam", "continue", "resume"] {
            assert!(matches!(interpret_topic_input(s, &local, false), Some(TopicChoice::Resume(t)) if t == "son-konu"), "{s}");
        }
        // >4 words → goes to the LLM/new-topic flow (K2 catches it).
        assert!(matches!(
            interpret_topic_input("devam edelim ama bu sefer docker öğrenelim", &local, false),
            Some(TopicChoice::New(_))
        ));
        // Resume pattern but no topic exists → new topic.
        assert!(matches!(interpret_topic_input("devam", &[], false), Some(TopicChoice::New(_))));
    }

    #[test]
    fn interpret_other_input_is_new() {
        let local = vec!["son-konu".to_string()];
        assert!(matches!(interpret_topic_input("docker compose", &local, false), Some(TopicChoice::New(r)) if r == "docker compose"));
    }

    #[test]
    fn empty_enter_suggests_when_no_local_topics_and_project_known() {
        assert!(matches!(
            interpret_topic_input("", &[], true),
            Some(TopicChoice::Suggest)
        ));
        assert!(matches!(interpret_topic_input("  ", &[], true), Some(TopicChoice::Suggest)));
    }

    #[test]
    fn empty_enter_resume_beats_suggest_when_local_exists() {
        let local = vec!["rust".to_string()];
        assert!(matches!(
            interpret_topic_input("", &local, true),
            Some(TopicChoice::Resume(t)) if t == "rust"
        ));
    }

    #[test]
    fn empty_enter_without_project_stays_none() {
        assert!(interpret_topic_input("", &[], false).is_none());
    }

    #[test]
    fn new_topic_confirm_msg_names_slug_and_keys() {
        let m = new_topic_confirm_msg("rust-cli");
        assert!(m.contains("rust-cli"));
        assert!(m.contains("[e"));
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
    fn parse_watch_command_variants() {
        assert_eq!(parse_watch_command("/watch"), Some(WatchCmd::Toggle));
        assert_eq!(parse_watch_command("/watch on"), Some(WatchCmd::On));
        assert_eq!(parse_watch_command("/watch off"), Some(WatchCmd::Off));
        assert_eq!(parse_watch_command("  /watch off  "), Some(WatchCmd::Off));
        assert_eq!(parse_watch_command("/WATCH OFF"), Some(WatchCmd::Off));
        assert_eq!(parse_watch_command("/Watch"), Some(WatchCmd::Toggle));
        assert_eq!(parse_watch_command("hello"), None);
        assert_eq!(parse_watch_command("/quit"), None);
    }

    #[test]
    fn apply_watch_transitions() {
        assert_eq!(apply_watch(WatchCmd::Off, true).0, false);
        assert_eq!(apply_watch(WatchCmd::On, false).0, true);
        assert_eq!(apply_watch(WatchCmd::Toggle, true).0, false);
        assert_eq!(apply_watch(WatchCmd::Toggle, false).0, true);
        assert!(apply_watch(WatchCmd::On, false).1.contains("on"));
        assert!(apply_watch(WatchCmd::Off, true).1.contains("off"));
    }

    #[test]
    fn parse_game_command_variants() {
        assert!(matches!(parse_game_command("/game on"), Some(GameCmd::On)));
        assert!(matches!(parse_game_command(" /game OFF "), Some(GameCmd::Off)));
        assert!(matches!(parse_game_command("/game"), Some(GameCmd::Status)));
        assert!(parse_game_command("/game x").is_none());
        assert!(parse_game_command("/gamer").is_none());
        assert!(parse_game_command("game on").is_none());
    }

    #[test]
    fn game_pref_roundtrip_idempotent_preserves_user_md() {
        let base = std::env::temp_dir().join(format!("usta_game_pref_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("USER.md"), "# Öğrenci Profili\n\n## Kim\n- Anil\n").unwrap();

        assert!(!game_pref(&base)); // default off
        set_game_pref(&base, true).unwrap();
        assert!(game_pref(&base));
        set_game_pref(&base, true).unwrap(); // idempotent
        let c = std::fs::read_to_string(base.join("USER.md")).unwrap();
        assert_eq!(c.matches("- gamification:").count(), 1);
        assert!(c.contains("## Kim")); // diğer içerik korunur
        assert!(c.contains("## Tercihler"));
        assert!(c.ends_with('\n')); // trailing newline korunur (line-replace path)
        set_game_pref(&base, false).unwrap();
        assert!(!game_pref(&base));
        let c2 = std::fs::read_to_string(base.join("USER.md")).unwrap();
        assert!(c2.ends_with('\n')); // ikinci toggle'da da korunur

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn restore_game_pref_readds_dropped_line_and_keeps_other_content() {
        let base = std::env::temp_dir().join(format!("usta_restore_drop_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        // Before flush: preference is on.
        std::fs::write(
            base.join("USER.md"),
            "# Öğrenci Profili\n\n## Kim\n- Anil\n\n## Tercihler\n- gamification: on\n",
        )
        .unwrap();
        let before = read_game_pref(&base);
        assert_eq!(before, Some(true));

        // Simulate the model rewriting the profile and DROPPING the preference line.
        std::fs::write(
            base.join("USER.md"),
            "# Öğrenci Profili\n\n## Kim\n- Anil (güncel)\n",
        )
        .unwrap();

        let restored = restore_game_pref(&base, before).unwrap();
        assert!(restored); // restore happened
        let c = std::fs::read_to_string(base.join("USER.md")).unwrap();
        assert!(c.contains("- gamification: on")); // line is back
        assert!(c.contains("## Kim")); // other (rewritten) profile content preserved
        assert!(c.contains("Anil (güncel)"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn restore_game_pref_reverts_flipped_value() {
        let base = std::env::temp_dir().join(format!("usta_restore_flip_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(
            base.join("USER.md"),
            "# Öğrenci Profili\n\n## Tercihler\n- gamification: on\n",
        )
        .unwrap();
        let before = read_game_pref(&base);
        assert_eq!(before, Some(true));

        // Model flipped the value to off.
        std::fs::write(
            base.join("USER.md"),
            "# Öğrenci Profili\n\n## Tercihler\n- gamification: off\n",
        )
        .unwrap();

        let restored = restore_game_pref(&base, before).unwrap();
        assert!(restored);
        assert_eq!(read_game_pref(&base), Some(true)); // back to old value
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn restore_game_pref_none_before_is_untouched() {
        let base = std::env::temp_dir().join(format!("usta_restore_none_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        // User never toggled — no preference line at all.
        let body = "# Öğrenci Profili\n\n## Kim\n- Anil\n";
        std::fs::write(base.join("USER.md"), body).unwrap();
        let before = read_game_pref(&base);
        assert_eq!(before, None);

        let restored = restore_game_pref(&base, before).unwrap();
        assert!(!restored); // nothing done
        let c = std::fs::read_to_string(base.join("USER.md")).unwrap();
        assert_eq!(c, body); // byte-for-byte untouched, no line added
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn game_streak_line_never_shows_streak_zero() {
        let base = std::env::temp_dir().join(format!("usta_game_streak_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("learner")).unwrap();

        // game OFF (no USER.md at all) → None regardless of history.
        let hist = format!(
            "{}\n{}\n{}\n",
            history::record_line("2026-08-14", "rust", None, None),
            history::record_line("2026-08-15", "rust", None, None),
            history::record_line("2026-08-16", "rust", None, None),
        );
        std::fs::write(base.join("learner/history.md"), &hist).unwrap();
        assert_eq!(game_streak_line(&base, "2026-08-16"), None);

        // game ON, but no history file (or empty) → None.
        std::fs::write(base.join("USER.md"), "# Öğrenci Profili\n\n## Tercihler\n- gamification: on\n").unwrap();
        std::fs::remove_file(base.join("learner/history.md")).unwrap();
        assert_eq!(game_streak_line(&base, "2026-08-16"), None);

        // game ON + current streak > 0 → Some("streak: N day(s) (longest M)"), never "streak: 0".
        std::fs::write(base.join("learner/history.md"), &hist).unwrap();
        let s = game_streak_line(&base, "2026-08-16").unwrap();
        assert_eq!(s, "streak: 3 day(s) (longest 3)");
        assert!(!s.contains("streak: 0"));

        // game ON + broken streak (gap before today, longest > 0) → Some("longest streak: M day(s)").
        let broken = game_streak_line(&base, "2026-08-19").unwrap();
        assert_eq!(broken, "longest streak: 3 day(s)");
        assert!(!broken.contains("streak: 0"));
        assert!(broken.starts_with("longest"));

        let _ = std::fs::remove_dir_all(&base);
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
