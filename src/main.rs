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
use std::path::{Path, PathBuf};

use anyhow::Result;
use rustyline::DefaultEditor;

use crate::anthropic::Message;
use crate::backend::Backend;
use crate::cli::{Command, ResetTarget, parse_command};
use crate::file_feedback::{FileFeedback, handle_file_change, is_silent_skip, seed_mentor_baseline};
use crate::lifecycle::{
    ask_usta, build_session, flush_core, flush_progress, lock_path, maybe_compact,
    sleep_until_deadline, today,
};
use crate::session::Session;
use crate::setup::{
    confirm, confirm_recover, ensure_scaffold, profile_is_generic, run_init, run_migration,
    run_reset_factory, run_reset_profile, run_reset_topic, run_stats, run_topics,
};
use crate::slash::{
    GameCmd, apply_watch, game_on_turn, game_pref, game_streak_line, is_exam_command,
    parse_game_command, parse_watch_command, set_game_pref, topic_has_goal,
};
use crate::topic::{finalize_slug, interpret_topic_input, slug_system, slugify_topic, TopicChoice};

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
    fn show_request_composition() {
        assert!(show_request(None, None).is_none());
        let bare = show_request(None, Some("ownership explained")).unwrap();
        assert!(bare.contains("ownership explained"));
        let explicit = show_request(Some("dns".into()), Some("prior")).unwrap();
        assert!(explicit.contains("dns") && explicit.contains("prior"));
        let cold = show_request(Some("dns".into()), None).unwrap();
        assert!(cold.contains("dns"));
    }
}
