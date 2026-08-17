//! TUI session loop: keys + watcher + LLM all in one select!. Persistent content
//! flows into scrollback via insert_before; the bottom region is redrawn live. Spec §3.
//!
//! In plain mode (ui::is_plain) this module is never used — main's branching
//! routes the plain path to run_plain_loop. There is NO alt-screen here: only
//! an inline viewport + insert_before, scrollback is preserved.

use std::path::{Path, PathBuf};

use anyhow::Result;
use crossterm::event::{Event, EventStream};
use futures_util::StreamExt;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::anthropic::Message;
use crate::backend::Backend;
use crate::session::Session;
use crate::transcript::Recorder;
use crate::tui::editor::{Action, InputBox};
use crate::tui::status::Status;
use crate::tui::welcome;
use crate::tui::welcome_data;
use crate::{feedback, history, progress, watcher};

/// TUI session: topic entry (if no arg) + welcome box + drill/intro + main
/// loop. Session/recorder are set up inside via `build_session`. Return:
/// `Some((session, recorder, lock))` — closing is shared with the plain path in
/// main; `None` — the user quit without giving a topic (no session, no lock). On
/// return, Tui drops → terminal restore.
pub async fn run(
    backend: &mut Backend,
    global: &Path,
    project_root: &Path,
    today: &str,
    topic_arg: Option<String>,
    max_feedback_batch: usize,
    watch_rx: &mut UnboundedReceiver<PathBuf>,
) -> Result<Option<(Session, Recorder, PathBuf)>> {
    // setup() installs the panic hook → called EXACTLY ONCE PER PROCESS (not in a loop).
    let mut tui = crate::tui::term::setup()?;
    let mut editor = InputBox::new();
    let mut events = EventStream::new();
    let read = |p: PathBuf| std::fs::read_to_string(p).ok();

    // Determine the topic: if an argument was given, slugify it locally (so
    // `usta start "JavaScript Basics"` works). If no argument, show the identity
    // welcome + ask via the input box.
    let had_topic_arg = topic_arg.is_some();
    let mut resumed = false; // whether the resume flow was chosen — triggers the identity-free continuation panel
                             // The RAW text from the user's topic entry — carried into the new-topic intro
                             // turn as the "first answer"; reducing it to a slug and discarding it would
                             // force the model to re-ask what was already said. Not used in the resume flow.
    let mut intro: Option<String> = None;
    let topic = match topic_arg {
        Some(t) => {
            intro = Some(t.clone());
            crate::topic::slugify_topic(&t)
        }
        None => {
            // Topic lists are computed here and passed to ask_topic:
            //  - `local`: topics resumable in this project (newest → oldest, [0]=latest)
            //  - `other`: topics in other projects (informational only, at most 4)
            let index_content =
                std::fs::read_to_string(global.join("learner/index.md")).unwrap_or_default();
            let local = crate::index::local_topics(project_root, &index_content);
            let other: Vec<String> = {
                let mut o: Vec<String> = crate::index::entries(&index_content)
                    .into_iter()
                    .filter(|e| e.project != project_root)
                    .map(|e| e.topic)
                    .collect();
                o.dedup();
                o.truncate(4);
                o
            };
            // Whether PROJECT.md has content — gates the empty-Enter "suggest" flow.
            let project_known = std::fs::read_to_string(progress::project_md_path(project_root))
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
            // Weekly session count + current streak, from the global (not
            // topic-scoped) history log — shown on the identity welcome's "This
            // week" line. Computed once; doesn't change across loop iterations.
            let (week_sessions, streak) = match read(global.join("learner/history.md")) {
                Some(h) => {
                    let es = history::entries(&h);
                    (
                        history::week_summary(&es, today).sessions,
                        history::current_streak(&es, today),
                    )
                }
                None => (0, 0),
            };
            // The identity welcome is only printed on the first turn — it's not
            // printed again if the new-topic confirmation is rejected and we go back
            // to the entry question.
            let mut welcome_shown = false;
            loop {
                let raw = match crate::tui::entry::ask_topic(
                    &mut tui,
                    &mut editor,
                    &mut events,
                    read(global.join("USER.md")).as_deref(),
                    &backend.label(),
                    &crate::tui::paint::short_dir(project_root),
                    &local,
                    &other,
                    project_known,
                    !welcome_shown,
                    week_sessions,
                    streak,
                )
                .await?
                {
                    Some(line) => line,
                    None => return Ok(None), // quit without giving a topic
                };
                welcome_shown = true;
                if !raw.trim().is_empty() {
                    crate::tui::page::page_user_echo(&mut tui, raw.trim())?;
                }
                // Slash commands at topic entry: the hint above the prompt promises
                // /help, so honor it here too; session-only commands get a pointer
                // instead of silently being slugged into a topic name.
                if crate::help::is_help_command(&raw) {
                    crate::tui::page::page_notice(&mut tui, crate::help::help_text())?;
                    continue;
                }
                if raw.trim().eq_ignore_ascii_case("/quit") {
                    return Ok(None); // quit without giving a topic
                }
                if crate::visual::parse_show_command(&raw).is_some()
                    || crate::slash::parse_watch_command(&raw).is_some()
                {
                    crate::tui::page::page_notice(
                        &mut tui,
                        "that command works inside a session — pick a topic first",
                    )?;
                    continue;
                }
                match crate::topic::interpret_topic_input(&raw, &local, project_known) {
                    // SAFE FALLBACK: interpret only returns None when (input is empty +
                    // local is empty); ask_topic only produces the empty-Enter sentinel
                    // when local is non-empty, so this normally isn't reached. Its natural
                    // counterpart in the loop is "swallow, ask again" — this is the safe fallback.
                    None => {}
                    Some(crate::topic::TopicChoice::Suggest) => {
                        // One-shot suggestion from mentor/PROJECT.md (spec: project-aware
                        // start). Same mechanics as the slug mini-session: single call,
                        // then ALWAYS reset.
                        let project_md =
                            read(progress::project_md_path(project_root)).unwrap_or_default();
                        let outcome = crate::tui::ask::ask_live(
                            &mut tui,
                            &mut editor,
                            &mut events,
                            backend,
                            &crate::topic::start_suggest_system(),
                            &[Message::user(project_md.as_str())],
                            None,
                        )
                        .await;
                        backend.reset_session(); // suggestion chat must not leak into the session
                        let parsed = match outcome {
                            Ok(crate::tui::ask::AskOutcome::Reply(reply)) => {
                                crate::topic::parse_start_suggestion(&reply.text)
                            }
                            Ok(crate::tui::ask::AskOutcome::Cancelled) | Err(_) => None,
                        };
                        let Some((slug, text)) = parsed else {
                            crate::tui::page::page_error(
                                &mut tui,
                                "suggestion failed — type a topic",
                            )?;
                            continue;
                        };
                        if !text.is_empty() {
                            crate::tui::page::page_notice(&mut tui, &text)?;
                        }
                        if crate::tui::ask::tui_confirm(
                            &mut tui,
                            &editor,
                            &mut events,
                            &format!("start with '{slug}'? [y/N]"),
                        )
                        .await?
                        {
                            crate::tui::page::page_notice(&mut tui, &format!("topic: {slug}"))?;
                            intro = Some(format!(
                                "Usta's own opening suggestion (already shown to the user, \
                                 they accepted it — continue from its first step, don't repeat it):\n{text}"
                            ));
                            break slug;
                        }
                        crate::tui::page::page_notice(&mut tui, "cancelled — type a topic")?;
                    }
                    Some(crate::topic::TopicChoice::Resume(t)) => {
                        crate::tui::page::page_notice(&mut tui, &format!("resuming: {t}"))?;
                        resumed = true; // for the continuation panel below
                        break t;
                    }
                    Some(crate::topic::TopicChoice::New(raw)) => {
                        // New-topic flow: ≤2 words → local slug; a sentence → LLM slug (spinner).
                        let slug = if raw.split_whitespace().count() <= 2 {
                            crate::topic::slugify_topic(&raw)
                        } else {
                            let slug = match crate::tui::ask::ask_live(
                                &mut tui,
                                &mut editor,
                                &mut events,
                                backend,
                                &crate::topic::slug_system(&local),
                                &[Message::user(raw.as_str())],
                                None,
                            )
                            .await
                            {
                                Ok(crate::tui::ask::AskOutcome::Reply(reply)) => {
                                    crate::topic::finalize_slug(&raw, &reply.text)
                                }
                                Ok(crate::tui::ask::AskOutcome::Cancelled) | Err(_) => {
                                    crate::topic::slugify_topic(&raw)
                                }
                            };
                            // The slug mini-session must not carry over into the learning session (spec B1).
                            backend.reset_session();
                            slug
                        };
                        // If the LLM/short slug happens to match a local topic, this also
                        // counts as RESUME (spec K2): the notice becomes "resuming", the
                        // identity-free continuation panel is printed, NO confirmation.
                        if local.contains(&slug) {
                            crate::tui::page::page_notice(&mut tui, &format!("resuming: {slug}"))?;
                            resumed = true;
                            break slug;
                        }
                        // New-topic confirmation: only asked when there's a topic that could
                        // be resumed (spec §2) — on first-run/empty-local it opens without confirmation.
                        if local.is_empty()
                            || crate::tui::ask::tui_confirm(
                                &mut tui,
                                &editor,
                                &mut events,
                                &crate::topic::new_topic_confirm_msg(&slug),
                            )
                            .await?
                        {
                            crate::tui::page::page_notice(
                                &mut tui,
                                &format!("topic: {slug} — tell me the details in chat"),
                            )?;
                            intro = Some(raw);
                            break slug;
                        }
                        // Rejected → go back to the entry question (welcome is not printed again).
                        crate::tui::page::page_notice(
                            &mut tui,
                            "cancelled — Enter = resume, or type another topic",
                        )?;
                    }
                }
            }
        }
    };

    // Lock-conflict confirmation (TUI single-key) — BEFORE build_session, without
    // writing its own lock. If rejected, no session/lock → Tui drop restores.
    let lock = crate::lifecycle::lock_path(project_root, &topic);
    if lock.exists()
        && !crate::tui::ask::tui_confirm(
            &mut tui,
            &editor,
            &mut events,
            "Another session may be open for this topic — progress could clash. Continue? [y/N]",
        )
        .await?
    {
        crate::tui::page::page_notice(&mut tui, "cancelled")?;
        return Ok(None);
    }

    // build_session writes its own lock; the returned lock = same path.
    let (mut session, recorder, lock, has_progress) =
        crate::lifecycle::build_session(global, project_root, &topic, today)?;

    let mut debouncer = watcher::Debouncer::new(std::time::Duration::from_millis(1000));
    let mut files = feedback::FileMemory::new();
    // Mentor docs are already in the system prompt — baseline them so an
    // unchanged re-save is a Skip, not a redundant full re-send (FIX: first-sight seed).
    crate::file_feedback::seed_mentor_baseline(&mut files, project_root);
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
        // an empty frame.
        if let Some(text) = welcome::render_for_entry(had_topic_arg, &data, w) {
            crate::tui::page::page(&mut tui, text)?;
        }
    }

    // Opening drill / intro (the TUI counterpart of plain.rs's plain path). If the
    // profile is still the embedded generic template (or doesn't exist at all),
    // Usta doesn't know the user yet — a short introduction instruction is added
    // to the opening turn (spec Ç3a).
    let profile_generic = read(global.join("USER.md"))
        .as_deref()
        .map(crate::setup::profile_is_generic)
        .unwrap_or(true);
    let project_known = progress::project_md_path(project_root).exists();
    let opening = if has_progress {
        let gs = crate::slash::game_streak_line(global, today);
        let progress_content =
            read(progress::progress_path(project_root, &topic)).unwrap_or_default();
        let due = welcome_data::due_questions(&progress_content, today);
        let has_questions = welcome_data::drill_count(&progress_content) > 0;
        progress::opening_prompt(
            &topic,
            profile_generic,
            project_known,
            gs.as_deref(),
            &due,
            has_questions,
        )
    } else {
        for note in crate::materials::convert_pdfs(project_root) {
            crate::tui::page::page_notice(&mut tui, &note)?;
        }
        let mats = crate::materials::scan(project_root);
        let material_digest = crate::materials::combined_digests(&mats);
        progress::onboarding_prompt(
            &topic,
            intro.as_deref(),
            profile_generic,
            project_known,
            material_digest.as_deref(),
        )
    };
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
        Err(e) => crate::tui::page::page_error(&mut tui, &format!("opening turn skipped: {e}"))?,
    }

    let mut watching = true;
    loop {
        // Drain the buffer at the start of every iteration — notices that
        // accumulate outside maybe_compact, like a transcript write error,
        // should never be lost either.
        crate::tui::page::flush_notices(&mut tui)?;
        crate::tui::page::draw(
            &mut tui,
            &editor,
            &Status::Idle,
            last_tokens,
            window,
            Some(watching),
        )?;
        tokio::select! {
            maybe_ev = events.next() => {
                let Some(Ok(ev)) = maybe_ev else {
                    if maybe_ev.is_none() { break; } // stream ended = Eof (spec B4)
                    continue; // single event error — ignore
                };
                let k = match ev {
                    Event::Key(k) => k,
                    // Bracketed paste: single event, Enter isn't triggered, structure is preserved.
                    Event::Paste(s) => { editor.insert_str(&s); continue }
                    _ => continue,
                };
                match editor.handle_key(k) {
                    Action::None => {}
                    Action::Exit => break,
                    Action::Submit(line) => {
                        if let Some(cmd) = crate::slash::parse_watch_command(&line) {
                            crate::tui::page::page_user_echo(&mut tui, &line)?;
                            let (next, msg) = crate::slash::apply_watch(cmd, watching);
                            watching = next;
                            crate::tui::page::page_notice(&mut tui, msg)?;
                            continue;
                        }
                        if crate::help::is_help_command(&line) {
                            crate::tui::page::page_user_echo(&mut tui, &line)?;
                            crate::tui::page::page_notice(&mut tui, crate::help::help_text())?;
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
                            line.clone()
                        };
                        session.push_user(&outgoing);
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
                let batch = debouncer.flush();
                if batch.len() > max_feedback_batch {
                    crate::tui::page::page_notice(&mut tui, &format!(
                        "bulk change ({} files) — feedback skipped, still watching",
                        batch.len()
                    ))?;
                    // Silently sync FileMemory: the next single save shouldn't produce a giant diff.
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
                        match crate::file_feedback::handle_file_change(backend, &mut session, &mut files, project_root, &path, &recorder).await {
                            Ok(crate::file_feedback::FileFeedback::Sessiz) => {}
                            Ok(crate::file_feedback::FileFeedback::Bildirim(m)) => crate::tui::page::page_notice(&mut tui, &m)?,
                            Ok(crate::file_feedback::FileFeedback::Yanit { tokens, reply, show_topic }) => {
                                if let Some(t) = tokens { last_tokens = Some(t); }
                                let w = crate::tui::page::current_width(&tui);
                                crate::tui::page::page_reply(&mut tui, &reply.text, w)?;
                                crate::lifecycle::maybe_compact(backend, &mut session, project_root, tokens).await;
                                crate::tui::entry::trigger_auto_visual(&mut tui, &mut editor, &mut events, backend, &session, project_root, &topic, show_topic, last_tokens).await?;
                            }
                            // Same silent-skip classes as the plain path (plain.rs) /
                            // is_silent_skip (file_feedback.rs): vanished temp file
                            // (NotFound) or binary content (InvalidData) — no noise for either.
                            Err(e) if crate::file_feedback::is_silent_skip(&e) => {}
                            Err(e) => crate::tui::page::page_error(&mut tui, &format!("file feedback skipped: {}: {e}", path.display()))?,
                        }
                    }
                }
            }
        }
    }
    // Drain the last iteration's notices right before exit — so a transcript
    // warning that lands in the buffer on the /quit or Exit path is still seen
    // while the TUI is up.
    crate::tui::page::flush_notices(&mut tui)?;
    Ok(Some((session, recorder, lock))) // Tui drop → restore
}
