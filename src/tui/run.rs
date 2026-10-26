//! TUI session loop: keys + watcher + LLM all in one select!. Persistent content
//! flows into the chat pane via `tui/page.rs`'s `page*` wrappers; the whole
//! screen is redrawn live. Spec §3. Delegates to five sibling modules split out
//! of this one in the cleanup round: `paint` (pure text/style builders), `page`
//! (the paging layer), `ask` (question-asking), `entry` (topic entry +
//! visual-generation trigger), and `topic_flow` (topic resolution).
//!
//! In plain mode (ui::is_plain) this module is never used — main's branching
//! routes the plain path to run_plain_loop. The UI owns the fullscreen
//! alternate screen; the chat lives in usta's own pane, not in scrollback.

use std::path::{Path, PathBuf};

use anyhow::Result;
use crossterm::event::{Event, EventStream};
use futures_util::StreamExt;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::backend::Backend;
use crate::session::Session;
use crate::transcript::Recorder;
use crate::tui::editor::{Action, InputBox};
use crate::tui::status::Status;
use crate::tui::welcome;
use crate::tui::welcome_data;
use crate::{feedback, progress, watcher};

/// TUI session: topic entry (if no arg) + welcome box + drill/intro + main
/// loop. Session/recorder are set up inside via `build_session`. Return:
/// `Some((session, recorder, lock, flushed))` — the closing flush already ran
/// inside the TUI (`closing::save_live`); the rest of closing is shared with the
/// plain path in main; `None` — the user quit without giving a topic (no session, no lock). On
/// return, Tui drops → terminal restore.
pub async fn run(
    backend: &mut Backend,
    global: &Path,
    project_root: &Path,
    today: &str,
    topic_arg: Option<String>,
    max_feedback_batch: usize,
    watch_rx: &mut UnboundedReceiver<PathBuf>,
) -> Result<Option<(Session, Recorder, PathBuf, Result<()>)>> {
    // setup() installs the panic hook → called EXACTLY ONCE PER PROCESS (not in a loop).
    let mut tui = crate::tui::term::setup()?;
    let mut editor = InputBox::new();
    let mut events = EventStream::new();
    let read = |p: PathBuf| std::fs::read_to_string(p).ok();

    let Some(crate::tui::topic_flow::TopicPick {
        topic,
        had_topic_arg,
        resumed,
        intro,
        intro_turns,
        first_run_completed,
    }) = crate::tui::topic_flow::resolve_topic(
        &mut tui,
        &mut editor,
        &mut events,
        backend,
        global,
        project_root,
        today,
        topic_arg,
    )
    .await?
    else {
        return Ok(None); // quit without giving a topic
    };

    // Lock-conflict confirmation (TUI single-key) — BEFORE build_session, without
    // writing its own lock. If rejected, no session/lock → Tui drop restores.
    let lock = crate::lifecycle::lock_path(project_root, &topic);
    if lock.exists()
        && !crate::tui::ask::tui_confirm(
            &mut tui,
            &mut editor,
            &mut events,
            "Another session may be open for this topic — progress could clash. Continue? (yes/no)",
        )
        .await?
    {
        crate::ui::notice("cancelled");
        return Ok(None);
    }

    // build_session writes its own lock; the returned lock = same path.
    let (mut session, recorder, lock, has_progress) =
        crate::lifecycle::build_session(global, project_root, &topic, today)?;

    // The first-run introduction only counts as completed once a real session
    // exists — writing this any earlier (e.g. the moment the model returns a
    // topic) would strand the marker on disk if the lock-conflict confirmation
    // above is declined, and the user would never be introduced (SPEC §4.22
    // review, Fix 1).
    if first_run_completed {
        crate::setup::mark_intro_done(global, "completed");
    }

    let mut debouncer = watcher::Debouncer::new(std::time::Duration::from_millis(1000));
    let mut files = feedback::FileMemory::new();
    // Mentor docs are already in the system prompt — baseline them so an
    // unchanged re-save is a Skip, not a redundant full re-send (FIX: first-sight seed).
    crate::file_feedback::seed_mentor_baseline(&mut files, project_root);
    let mut tracker = watcher::StructureTracker::seed(project_root);
    let mut monitor = crate::check::VerifyMonitor::new(project_root);
    let mut last_tokens: Option<u64> = None;
    let window = backend.context_window();

    // Quietly absorb watcher events accumulated during topic entry — files saved
    // while the user was typing the topic shouldn't produce surprise feedback the
    // moment the session starts (FileMemory is synced, the next real change is
    // diffed against it).
    while let Ok(path) = watch_rx.try_recv() {
        if let Ok(c) = std::fs::read_to_string(&path) {
            let _ = files.observe(&path, c);
        }
    }

    // Welcome: dispatch between the full-mode box and the identity-free resume
    // panel — see `welcome::render_for_entry`'s doc comment for why the two
    // entry paths render differently.
    if had_topic_arg || resumed {
        let data = welcome_data::gather(
            read(global.join("USER.md")).as_deref(),
            read(progress::progress_path(project_root, &topic)).as_deref(),
            read(progress::curriculum_path(project_root, &topic)).as_deref(),
            &topic,
            &backend.label(),
            &crate::tui::paint::short_dir(project_root),
            today,
            read(global.join("learner/history.md")).as_deref(),
        );
        let w = crate::tui::page::current_width(&tui);
        // `None` only on the resume path with nothing recorded yet (Finding 1,
        // v0.21 review) — the `resuming: <topic>` notice already printed by the
        // topic-entry loop stands on its own; skip the panel rather than print
        // an empty frame. Checked once here; the panel re-renders per width.
        if welcome::render_for_entry(had_topic_arg, &data, w).is_some() {
            crate::tui::page::page_fixed(&mut tui, move |w| {
                welcome::render_for_entry(had_topic_arg, &data, w).unwrap_or_default()
            })?;
        }
    }

    // 13a: the pre-lock conversation becomes the head of the real session —
    // replayed into history + transcript. `build_session` rebuilt the system
    // prompt with the real topic and the CLI backend's `--resume` would keep
    // the pre-lock one, so reset (maybe_compact precedent).
    let opening = match &intro_turns {
        Some(turns) => {
            crate::tui::intro::stitch(turns, &mut session, &recorder);
            backend.reset_session();
            None // already flowing — the opening turn is not injected
        }
        None => Some(crate::tui::entry::opening_turn(
            &mut tui,
            global,
            project_root,
            &topic,
            today,
            has_progress,
            intro.as_deref(),
        )?),
    };
    if let Some(opening) = opening {
        session.push_user(&opening);
        recorder.user(&opening);
        match crate::tui::ask::ask_live(
            &mut tui,
            &mut editor,
            &mut events,
            backend,
            &session.system,
            session.history(),
            last_tokens,
        )
        .await
        {
            Ok(crate::tui::ask::AskOutcome::Reply(reply)) => {
                last_tokens = reply.context_tokens;
                let (clean, show_topic) = crate::visual::extract_show_marker(&reply.text);
                let w = crate::tui::page::current_width(&tui);
                crate::tui::page::page_reply(&mut tui, &clean, w)?;
                recorder.assistant(&clean);
                session.push_assistant(clean);
                crate::tui::entry::trigger_auto_visual(
                    &mut tui,
                    &mut editor,
                    &mut events,
                    backend,
                    &session,
                    project_root,
                    &topic,
                    show_topic,
                    last_tokens,
                )
                .await?;
            }
            Ok(crate::tui::ask::AskOutcome::Cancelled) => {
                backend.reset_session();
                crate::tui::page::page_notice(&mut tui, "opening turn cancelled")?;
            }
            Err(e) => {
                crate::tui::page::page_error(&mut tui, &format!("opening turn skipped: {e}"))?
            }
        }
    }

    let mut watching = true;
    // One honest axis (spec K4): `live` = immediate plain-review turn at
    // every flush, only by the user's explicit choice (`/watch live` or a
    // `watch: live` approach line). Off (default) = companion: accumulate,
    // deliver on the user's next submit with the lesson-flow frame (K1/K2).
    let approach = crate::tui::polite::approach_text(project_root, global, &topic);
    let mut live = crate::tui::polite::live_from_approach(&approach);
    let mut pending = crate::tui::polite::PendingChanges::new();
    editor.set_allow_images(true);
    loop {
        crate::tui::page::draw(
            &mut tui,
            &editor,
            &Status::Idle,
            last_tokens,
            window,
            Some((watching, live, pending.len())),
            watching && monitor.is_failing(),
        )?;
        tokio::select! {
            maybe_ev = events.next() => {
                let Some(Ok(ev)) = maybe_ev else {
                    if maybe_ev.is_none() { break; } // stream ended = Eof (spec B4)
                    continue; // single event error — ignore
                };
                if crate::tui::page::handle_nav(&mut tui, &ev) { continue; }
                let k = match ev {
                    Event::Key(k) => k,
                    // Bracketed paste: single event, Enter isn't triggered, structure is preserved.
                    Event::Paste(s) => { editor.paste(&s); continue }
                    Event::Resize(_, _) => { crate::tui::page::handle_resize(&mut tui)?; continue }
                    _ => continue,
                };
                match editor.handle_key(k) {
                    Action::None => {}
                    Action::Exit => break,
                    Action::Submit(line) => {
                        if let Some(cmd) = crate::slash::parse_watch_command(&line) {
                            crate::tui::page::page_user_echo(&mut tui, &line)?;
                            // Toggle handling + queue drop live in polite::handle_watch_command (600-line budget displacement).
                            for n in crate::tui::polite::handle_watch_command(cmd, &mut watching, &mut live, &mut pending) { crate::tui::page::page_notice(&mut tui, &n)?; }
                            continue;
                        }
                        if crate::help::is_help_command(&line) {
                            crate::tui::page::page_user_echo(&mut tui, &line)?;
                            crate::tui::page::page_notice(&mut tui, crate::help::help_text())?;
                            continue;
                        }
                        if crate::slash::is_context_command(&line) {
                            crate::tui::page::page_user_echo(&mut tui, &line)?;
                            // Deterministic breakdown — shell-only, no LLM call (spec F1).
                            crate::tui::page::page_notice(&mut tui, &crate::context_report::build(&session.system, session.history(), last_tokens, window))?;
                            continue;
                        }
                        if let Some(arg) = crate::visual::parse_show_command(&line) {
                            crate::tui::page::page_user_echo(&mut tui, &line)?;
                            let concept = arg.clone().unwrap_or_else(|| "visual".to_string());
                            // Borrow care: read the last reply BEFORE any &mut session borrow.
                            let last = crate::visual::last_assistant_text(&session);
                            match crate::visual::show_request(arg, last.as_deref()) {
                                None => crate::tui::page::page_notice(&mut tui, "nothing to visualize yet — explain something first, or use /show [topic]")?,
                                Some(req) => {
                                    crate::tui::entry::run_visual_generation(&mut tui, &mut editor, &mut events, backend, project_root, &topic, &concept, &req, last_tokens).await?;
                                }
                            }
                            continue;
                        }
                        if line.eq_ignore_ascii_case("/quit") { break; }
                        // /game: toggle persists to USER.md. Status is a local notice (never
                        // reaches the LLM). On/Off flip the pref + inject a mode-switch turn
                        // (swapped below) that flows through the normal ask — same shape as /exam.
                        let game_cmd = crate::slash::parse_game_command(&line);
                        if let Some(cmd) = &game_cmd {
                            crate::tui::page::page_user_echo(&mut tui, &line)?;
                            match cmd {
                                crate::slash::GameCmd::Status => {
                                    crate::tui::page::page_notice(&mut tui, if crate::slash::game_pref(global) {
                                        "gamification is on"
                                    } else {
                                        "gamification is off"
                                    })?;
                                    continue;
                                }
                                crate::slash::GameCmd::On | crate::slash::GameCmd::Off => {
                                    let on = matches!(cmd, crate::slash::GameCmd::On);
                                    if let Err(e) = crate::slash::set_game_pref(global, on) {
                                        crate::tui::page::page_notice(&mut tui, &format!("could not save game preference: {e}"))?;
                                        continue;
                                    }
                                    crate::tui::page::page_notice(&mut tui, if on {
                                        "gamification on — XP, levels and badges are live"
                                    } else {
                                        "gamification off — back to quiet mode"
                                    })?;
                                }
                            }
                        }
                        // /exam: echo the literal command, gate on goal presence, then swap the
                        // outgoing text and fall through to the normal ask flow below.
                        let images = if crate::slash::is_exam_command(&line) || game_cmd.is_some() { Vec::new() } else { editor.take_images() };
                        let outgoing = if crate::slash::is_exam_command(&line) {
                            crate::tui::page::page_user_echo(&mut tui, "/exam")?;
                            if !crate::slash::topic_has_goal(project_root, global, &topic) {
                                crate::tui::page::page_error(&mut tui, "no goal set for this topic — /exam needs a goal (exam/certificate); set one in the introduction")?;
                                continue;
                            }
                            progress::exam_prompt(&topic)
                        } else if let Some(cmd) = game_cmd {
                            // literal command already echoed in the /game block above
                            match cmd {
                                crate::slash::GameCmd::On => crate::slash::game_on_turn(
                                    &std::fs::read_to_string(global.join("GAMIFICATION.md")).unwrap_or_default(),
                                ),
                                crate::slash::GameCmd::Off => "[GAME MODE OFF] Gamification is now OFF — stop all game narration.".to_string(),
                                crate::slash::GameCmd::Status => line.clone(), // unreachable: Status returns above
                            }
                        } else {
                            // Push the submitted line to scrollback as a distinct user block.
                            crate::tui::page::page_user_echo(&mut tui, &line)?;
                            crate::tui::page::page_image_echo(&mut tui, &images)?;
                            // Ride-along (spec K2): pending changes join THIS turn — payload first, the user's words last. Only genuine user text carries them; /exam and /game synthesize operational directives, so those branches leave the queue untouched for the next real message.
                            crate::tui::polite::attach_pending(&mut tui, watching, &mut pending, &mut files, project_root, &mut monitor, line.clone()).await?
                        };
                        let dropped = session.push_user_with_images(&outgoing, &images);
                        if dropped > 0 {
                            crate::tui::page::page_notice(&mut tui, &format!("{dropped} older image(s) dropped from context (size limit)"))?;
                        }
                        recorder.user(&outgoing);
                        match crate::tui::ask::ask_live(
                            &mut tui, &mut editor, &mut events, backend,
                            &session.system, session.history(), last_tokens,
                        ).await {
                            Ok(crate::tui::ask::AskOutcome::Reply(reply)) => {
                                last_tokens = reply.context_tokens;
                                let (clean, show_topic) = crate::visual::extract_show_marker(&reply.text);
                                let w = crate::tui::page::current_width(&tui);
                                crate::tui::page::page_reply(&mut tui, &clean, w)?;
                                recorder.assistant(&clean);
                                session.push_assistant(clean);
                                crate::lifecycle::maybe_compact(backend, &mut session, project_root, last_tokens).await;
                                crate::tui::entry::trigger_auto_visual(&mut tui, &mut editor, &mut events, backend, &session, project_root, &topic, show_topic, last_tokens).await?;
                            }
                            Ok(crate::tui::ask::AskOutcome::Cancelled) => {
                                // The user turn stays in history (intentional — spec B2); the CLI
                                // session is half-done — don't resume it, the next call should go
                                // with the full transcript.
                                backend.reset_session();
                                crate::tui::page::page_warn(&mut tui, "response cancelled — your message is kept, continue if you like")?;
                            }
                            Err(e) => crate::tui::page::page_error(&mut tui, &format!("error: {e}"))?,
                        }
                    }
                }
            }
            Some(path) = watch_rx.recv() => {
                debouncer.push(path, tokio::time::Instant::now());
            }
            _ = crate::lifecycle::sleep_until_deadline(debouncer.deadline()), if debouncer.deadline().is_some() => {
                // Watcher flush: routing + action live in polite::dispatch_flush (spec K1/K2) — run.rs keeps the call site only.
                crate::tui::polite::dispatch_flush(&mut tui, &mut editor, &mut events, backend, &mut session, &mut files, &recorder, project_root, &topic, &mut last_tokens, debouncer.flush(), max_feedback_batch, watching, live, &mut pending, &mut tracker, &mut monitor).await?;
            }
        }
    }
    // Closing save while the TUI is still up (keys dropped, status line busy);
    // main does the remaining closing steps with the result.
    let flushed = crate::tui::closing::save_live(
        &mut tui,
        &editor,
        &mut events,
        backend,
        &session,
        project_root,
        last_tokens,
    )
    .await;
    Ok(Some((session, recorder, lock, flushed))) // Tui drop → restore
}
