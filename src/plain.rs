//! Plain (line-based) front end: the REPL used when stdout is not a terminal
//! or `NO_COLOR` is set — extracted from `main.rs` (module split, Task 8).
//! Topic resolution, the REPL loop, and the `/show` visual-generation flow
//! that only this path needs.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::Result;
use rustyline::DefaultEditor;

use crate::anthropic::Message;
use crate::backend::Backend;
use crate::feedback;
use crate::file_feedback::{
    handle_file_change, is_silent_skip, seed_mentor_baseline, FileFeedback,
};
use crate::help;
use crate::index;
use crate::input;
use crate::lifecycle::{ask_usta, maybe_compact, sleep_until_deadline, today};
use crate::progress;
use crate::session::Session;
use crate::setup::confirm;
use crate::slash::{
    apply_watch, game_on_turn, game_pref, game_streak_line, is_exam_command, parse_game_command,
    parse_watch_command, set_game_pref, topic_has_goal, GameCmd,
};
use crate::tokens;
use crate::topic::{finalize_slug, interpret_topic_input, slug_system, slugify_topic, TopicChoice};
use crate::transcript;
use crate::ui;
use crate::visual;
use crate::watcher;

/// Resolve the topic: explicit argument > TTY prompt > silent "general" default.
/// If stdin is piped (not a TTY), returns "general" directly instead of getting
/// stuck on a prompt that can't be answered. Short input is slugified locally;
/// if a sentence is written, the model infers WHAT the user wants to learn and
/// we let it pick the most sensible slug.
pub(crate) async fn resolve_topic(
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
        println!(
            "saved: {} — Enter = continue with {}",
            local.join(", "),
            local[0]
        );
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
                    || confirm(
                        &format!("Open new topic '{slug}'? [y/N] "),
                        &["e", "evet", "y", "yes"],
                    )?
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

/// Bundles the by-value and shared-reference parameters of [`run_plain_loop`]
/// that don't need `&mut` access — `backend`, `session` and `watch_rx` stay
/// as direct arguments since a struct is the wrong home for `&mut` fields.
/// This keeps `run_plain_loop` at four arguments instead of ten, and — the
/// real point — replaces four positional slots that type-check identically
/// in pairs (`project_root`/`global`: both `&Path`; `has_progress`/
/// `profile_generic`: both `bool`) with named fields a transposition can't
/// silently pass the compiler.
pub(crate) struct PlainLoopCtx<'a> {
    pub(crate) recorder: &'a transcript::Recorder,
    pub(crate) project_root: &'a Path,
    pub(crate) global: &'a Path,
    pub(crate) topic: &'a str,
    pub(crate) has_progress: bool,
    pub(crate) intro: Option<&'a str>,
    pub(crate) profile_generic: bool,
}

/// Plain (line-based) REPL loop: rustyline input thread + watcher + debounce
/// all in one select!. Runs when there's no TTY / in NO_COLOR — behavior identical
/// to the old main loop (banner is printed in main, drill + loop live here).
pub(crate) async fn run_plain_loop(
    backend: &mut Backend,
    session: &mut Session,
    watch_rx: &mut tokio::sync::mpsc::UnboundedReceiver<PathBuf>,
    ctx: PlainLoopCtx<'_>,
) -> Result<()> {
    // Input thread + debounce state — specific to the plain path (rustyline).
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
    let mut input_rx = input::spawn("❯ ", ready_rx);
    let mut debouncer = watcher::Debouncer::new(std::time::Duration::from_millis(1000));
    let mut files = feedback::FileMemory::new();
    // Mentor docs are already in the system prompt — baseline them so an
    // unchanged re-save is a Skip, not a redundant full re-send (FIX: first-sight seed).
    seed_mentor_baseline(&mut files, ctx.project_root);

    // Opening drill: if progress exists from previous sessions, Usta speaks first,
    // warming up with 2-3 recall questions (testing effect — USTA.md rule).
    let project_known = progress::project_md_path(ctx.project_root).exists();
    if ctx.has_progress {
        let td = today();
        let gs = game_streak_line(ctx.global, &td);
        let progress_content =
            std::fs::read_to_string(progress::progress_path(ctx.project_root, ctx.topic))
                .unwrap_or_default();
        let due = crate::tui::welcome_data::due_questions(&progress_content, &td);
        let has_questions = crate::tui::welcome_data::drill_count(&progress_content) > 0;
        let opening = progress::opening_prompt(
            ctx.topic,
            ctx.profile_generic,
            project_known,
            gs.as_deref(),
            &due,
            has_questions,
        );
        session.push_user(&opening);
        ctx.recorder.user(&opening);
        match ask_usta(backend, &session.system, session.history()).await {
            Ok(reply) => {
                let (clean, show_topic) = visual::extract_show_marker(&reply.text);
                print_reply(
                    &clean,
                    reply.web,
                    reply.context_tokens,
                    backend.context_window(),
                );
                ctx.recorder.assistant(&clean);
                session.push_assistant(clean);
                trigger_auto_visual(backend, session, ctx.project_root, ctx.topic, show_topic)
                    .await;
            }
            // Drill failed → don't block the session, fall silently back into normal flow.
            Err(e) => ui::warn(&format!("opening drill skipped: {e}")),
        }
    } else {
        // New topic: no approach/map yet — introduction turn, Usta speaks first.
        for note in crate::materials::convert_pdfs(ctx.project_root) {
            ui::notice(&note);
        }
        let mats = crate::materials::scan(ctx.project_root);
        let material_digest = crate::materials::combined_digests(&mats);
        let onboarding = progress::onboarding_prompt(
            ctx.topic,
            ctx.intro,
            ctx.profile_generic,
            project_known,
            material_digest.as_deref(),
        );
        session.push_user(&onboarding);
        ctx.recorder.user(&onboarding);
        match ask_usta(backend, &session.system, session.history()).await {
            Ok(reply) => {
                let (clean, show_topic) = visual::extract_show_marker(&reply.text);
                print_reply(
                    &clean,
                    reply.web,
                    reply.context_tokens,
                    backend.context_window(),
                );
                ctx.recorder.assistant(&clean);
                session.push_assistant(clean);
                trigger_auto_visual(backend, session, ctx.project_root, ctx.topic, show_topic)
                    .await;
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
                        match crate::visual::show_request(arg, crate::visual::last_assistant_text(session).as_deref()) {
                            None => ui::notice("nothing to visualize yet — explain something first, or use /show [topic]"),
                            Some(req) => run_visual_generation(backend, ctx.project_root, ctx.topic, &concept, &req).await,
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
                                ui::notice(if game_pref(ctx.global) {
                                    "gamification is on"
                                } else {
                                    "gamification is off"
                                });
                                let _ = ready_tx.send(());
                                continue;
                            }
                            GameCmd::On | GameCmd::Off => {
                                let on = matches!(cmd, GameCmd::On);
                                if let Err(e) = set_game_pref(ctx.global, on) {
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
                        if !topic_has_goal(ctx.project_root, ctx.global, ctx.topic) {
                            ui::notice("no goal set for this topic — /exam needs a goal (exam/certificate); set one in the introduction");
                            let _ = ready_tx.send(());
                            continue;
                        }
                        progress::exam_prompt(ctx.topic)
                    } else if let Some(cmd) = game_cmd {
                        match cmd {
                            GameCmd::On => game_on_turn(
                                &std::fs::read_to_string(ctx.global.join("GAMIFICATION.md")).unwrap_or_default(),
                            ),
                            GameCmd::Off => "[GAME MODE OFF] Gamification is now OFF — stop all game narration.".to_string(),
                            GameCmd::Status => line, // unreachable: Status returns above
                        }
                    } else {
                        line
                    };
                    if !line.is_empty() {
                        session.push_user(&line);
                        ctx.recorder.user(&line);
                        match ask_usta(backend, &session.system, session.history()).await {
                            Ok(reply) => {
                                let (clean, show_topic) = visual::extract_show_marker(&reply.text);
                                print_reply(&clean, reply.web, reply.context_tokens, backend.context_window());
                                let tokens = reply.context_tokens;
                                ctx.recorder.assistant(&clean);
                                session.push_assistant(clean);
                                maybe_compact(backend, session, ctx.project_root, tokens).await;
                                trigger_auto_visual(backend, session, ctx.project_root, ctx.topic, show_topic).await;
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
                if batch.len() > crate::MAX_FEEDBACK_BATCH {
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
                        match handle_file_change(backend, session, &mut files, ctx.project_root, &path, ctx.recorder).await {
                            // handle_file_change no longer prints — the plain path applies
                            // its own presentation language (print_reply: web + gauge).
                            Ok(FileFeedback::Sessiz) => {}
                            Ok(FileFeedback::Bildirim(m)) => println!("{m}"),
                            Ok(FileFeedback::Yanit { tokens, reply, show_topic }) => {
                                print_reply(&reply.text, reply.web, reply.context_tokens, backend.context_window());
                                maybe_compact(backend, session, ctx.project_root, tokens).await;
                                trigger_auto_visual(backend, session, ctx.project_root, ctx.topic, show_topic).await;
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
async fn run_visual_generation(
    backend: &mut Backend,
    project_root: &Path,
    topic: &str,
    concept: &str,
    request: &str,
) {
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
                                if opened {
                                    ""
                                } else {
                                    " (open it in your browser)"
                                }
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
    if let Some(req) = crate::visual::show_request(
        Some(t.clone()),
        crate::visual::last_assistant_text(session).as_deref(),
    ) {
        ui::notice(&format!("visualizing: {t}…"));
        run_visual_generation(backend, project_root, topic, &t, &req).await;
    }
}
