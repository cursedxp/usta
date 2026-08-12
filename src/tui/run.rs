//! TUI session loop: keys + watcher + LLM all in one select!. Persistent content
//! flows into scrollback via insert_before; the bottom region is redrawn live. Spec §3.
//!
//! In plain mode (ui::is_plain) this module is never used — main's branching
//! routes the plain path to run_plain_loop. There is NO alt-screen here: only
//! an inline viewport + insert_before, scrollback is preserved.

use std::path::{Path, PathBuf};

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent};
use futures_util::StreamExt;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Widget};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::anthropic::Message;
use crate::backend::Backend;
use crate::session::Session;
use crate::transcript::Recorder;
use crate::tui::convert::ansi_to_text;
use crate::tui::editor::{Action, InputBox};
use crate::tui::status::{render_status, Status};
use crate::tui::term::{Tui, VIEWPORT_H};
use crate::tui::welcome;
use crate::{feedback, progress, ui, watcher};

/// Push persistent content above the viewport (into scrollback).
fn page(tui: &mut Tui, text: Text<'static>) -> Result<()> {
    let h = text.height() as u16;
    tui.terminal.insert_before(h, |buf| {
        Paragraph::new(text).render(buf.area, buf);
    })?;
    Ok(())
}

/// Print Usta's reply in the visual language: orange ● line + markdown + blank line.
fn page_reply(tui: &mut Tui, reply: &str, width: u16) -> Result<()> {
    let ansi = ui::render_markdown(reply, width as usize);
    let mut t = ansi_to_text(&format!("\x1b[38;5;208m●\x1b[0m\n{ansi}\n"));
    t.lines.push(Line::raw(""));
    page(tui, t)
}

/// Dim system notice (the TUI counterpart of ui::notice).
fn page_notice(tui: &mut Tui, msg: &str) -> Result<()> {
    page(tui, ansi_to_text(&format!("\x1b[2m· {msg}\x1b[0m")))
}

/// User block: blank separator line + orange `❯ ` prefix + NORMAL-colored text.
/// DO NOT USE DIM — it blended into the background and became invisible on dark
/// themes (spec S1). In multi-line submissions, continuation lines are indented
/// 2 spaces — the pasted structure is preserved.
fn user_echo_text(line: &str, width: u16) -> Text<'static> {
    // Prefix is 2 columns ("❯ " / "  "); the text wraps to the width minus this
    // allowance so a long message isn't cut off on one line (page_reply already
    // wraps markdown — echo was getting truncated when it didn't wrap). The first
    // VISUAL line gets ❯, the rest get 2 spaces — both multi-line paste and
    // single-line wrap read aligned this way.
    let inner = (width as usize).saturating_sub(2).max(1);
    let mut lines: Vec<Line> = vec![Line::raw("")];
    let mut first_visual = true;
    for logical in line.split('\n') {
        for chunk in wrap_cells(logical, inner) {
            let prefix = if first_visual {
                Span::styled("❯ ", ratatui::style::Style::default().fg(ratatui::style::Color::Indexed(208)))
            } else {
                Span::raw("  ")
            };
            lines.push(Line::from(vec![prefix, Span::raw(chunk)]));
            first_visual = false;
        }
    }
    Text::from(lines)
}

/// Split text to CELL width (unicode-width) — character-based, not word-based,
/// consistent with the input box's `wrap_visual`. Empty input → single blank line.
fn wrap_cells(s: &str, width: usize) -> Vec<String> {
    use unicode_width::UnicodeWidthChar;
    let width = width.max(1);
    let mut rows: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut col = 0usize;
    for ch in s.chars() {
        let w = ch.width().unwrap_or(0).max(1);
        if col + w > width && !cur.is_empty() {
            rows.push(std::mem::take(&mut cur));
            col = 0;
        }
        cur.push(ch);
        col += w;
    }
    rows.push(cur);
    rows
}

/// Push the user's submitted line to scrollback — wrapped to the current width.
fn page_user_echo(tui: &mut Tui, line: &str) -> Result<()> {
    let w = current_width(tui);
    page(tui, user_echo_text(line, w))
}

/// Current terminal width — keeps wrapping correct after a resize (spec B3).
/// Falls back to 80 if measurement fails (wrapping doesn't break, just gets narrow).
fn current_width(tui: &Tui) -> u16 {
    tui.terminal.size().map(|s| s.width).unwrap_or(80)
}

/// Draw the bottom region: input box (top) + status line (bottom).
fn draw(
    tui: &mut Tui,
    editor: &InputBox,
    status: &Status,
    tokens: Option<u64>,
    window: u64,
    watching: Option<bool>,
) -> Result<()> {
    tui.terminal.draw(|f| {
        let [box_area, status_area] =
            Layout::vertical([Constraint::Length(VIEWPORT_H - 1), Constraint::Length(1)])
                .areas(f.area());
        editor.render(f, box_area);
        f.render_widget(render_status(status, tokens, window, watching), status_area);
    })?;
    Ok(())
}

/// Result of ask_live: either a reply arrived or the user cancelled with double Ctrl-C.
pub enum AskOutcome {
    Reply(crate::backend::Reply),
    Cancelled,
}

/// Meaning of a keypress in locked mode — pure, testable (spec B2).
enum LockedKey {
    /// Key to be processed by the editor (including Enter — Enter is swallowed but counts as an edit).
    Edit,
    /// Ctrl-C / Ctrl-D — cancel-request step.
    CancelRequest,
}

fn classify_locked_key(k: KeyEvent) -> LockedKey {
    use crossterm::event::KeyModifiers;
    if k.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(k.code, KeyCode::Char('c') | KeyCode::Char('d'))
    {
        LockedKey::CancelRequest
    } else {
        LockedKey::Edit
    }
}

/// Wait for the LLM call with a live interface: spinner spins, keys are processed
/// by the editor but Submit/Exit are LOCKED (single-turn principle) — Enter is
/// swallowed. Can be cancelled with double Ctrl-C (or Ctrl-D): the first press
/// lights up a hint on the status line, the second drops the future
/// (kill_on_drop kills the child).
async fn ask_live(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
    backend: &mut Backend,
    system: &str,
    history: &[Message],
    tokens: Option<u64>,
) -> Result<AskOutcome> {
    let window = backend.context_window();
    let fut = backend.complete(system, history);
    tokio::pin!(fut);
    let mut frame = 0usize;
    let mut cancel_armed = false; // true after the first Ctrl-C — the counter doesn't reset (spec B2)
    loop {
        draw(tui, editor, &Status::Thinking { frame, cancel_hint: cancel_armed }, tokens, window, None)?;
        tokio::select! {
            r = &mut fut => return Ok(AskOutcome::Reply(r?)),
            Some(Ok(ev)) = events.next() => {
                // Paste is still processed by the editor even while locked (doesn't submit).
                if let Event::Paste(s) = &ev {
                    editor.insert_str(s);
                } else if let Event::Key(k) = ev {
                    // Single Esc = instant cancel (drops fut → kill_on_drop kills the child).
                    if matches!(k.code, KeyCode::Esc) {
                        return Ok(AskOutcome::Cancelled);
                    }
                    match classify_locked_key(k) {
                        LockedKey::CancelRequest if cancel_armed => {
                            // fut drops → kill_on_drop kills the child (backend.rs).
                            return Ok(AskOutcome::Cancelled);
                        }
                        LockedKey::CancelRequest => { cancel_armed = true; }
                        LockedKey::Edit => {
                            if !matches!(k.code, KeyCode::Enter) {
                                let _ = match editor.handle_key(k) {
                                    Action::Exit => Action::None, // never reached here (CancelRequest catches it) — safety net
                                    other => other,
                                };
                            }
                        }
                    }
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(120)) => { frame += 1; }
        }
    }
}

/// TUI sibling of main.rs's `run_visual_generation` — same guarantees: isolated
/// mini-session, `backend.reset_session()` on every exit path (success, cancel,
/// error, invalid JSON), Esc-cancellable via `ask_live`, same "try /show again"
/// notice on bad JSON. Shared by the explicit `/show <topic>` command and the
/// auto-triggered `[[show: ...]]` marker (Görev 4).
#[allow(clippy::too_many_arguments)]
async fn run_visual_generation(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
    backend: &mut Backend,
    project_root: &Path,
    topic: &str,
    concept: &str,
    request: &str,
    last_tokens: Option<u64>,
) -> Result<()> {
    match ask_live(
        tui,
        editor,
        events,
        backend,
        &crate::visual::visual_system(),
        &[Message::user(request)],
        last_tokens,
    )
    .await
    {
        Ok(AskOutcome::Reply(reply)) => {
            let json = crate::progress::clean_markdown_reply(&reply.text);
            match crate::visual::build_visual_html(&json) {
                Ok(html) => {
                    let path = crate::visual::visual_path(project_root, topic, concept);
                    if let Some(dir) = path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    match std::fs::write(&path, html) {
                        Ok(()) => {
                            let opened = crate::visual::open_in_browser(&path);
                            page_notice(
                                tui,
                                &format!(
                                    "visual saved: {}{}",
                                    path.display(),
                                    if opened { "" } else { " (open it in your browser)" }
                                ),
                            )?;
                        }
                        Err(e) => page_notice(tui, &format!("error: {e}"))?,
                    }
                }
                Err(e) => page_notice(tui, &format!("visual generation failed ({e}) — try /show again"))?,
            }
        }
        Ok(AskOutcome::Cancelled) => page_notice(tui, "visual generation cancelled")?,
        Err(e) => page_notice(tui, &format!("error: {e}"))?,
    }
    backend.reset_session(); // all paths — slug parity
    Ok(())
}

/// After a normal reply has been displayed and recorded, run the visual flow
/// if `[[show: ...]]` was found in it (Görev 4). No-op when `show_topic` is
/// `None`. Reuses `show_request` with the just-pushed clean reply as context —
/// same composition the explicit `/show <topic>` command uses.
#[allow(clippy::too_many_arguments)]
async fn trigger_auto_visual(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
    backend: &mut Backend,
    session: &Session,
    project_root: &Path,
    topic: &str,
    show_topic: Option<String>,
    last_tokens: Option<u64>,
) -> Result<()> {
    let Some(t) = show_topic else { return Ok(()) };
    if let Some(req) = crate::show_request(Some(t.clone()), crate::last_assistant_text(session).as_deref()) {
        page_notice(tui, &format!("visualizing: {t}…"))?;
        run_visual_generation(tui, editor, events, backend, project_root, topic, &t, &req, last_tokens).await?;
    }
    Ok(())
}

/// Prints the identity welcome and reads the topic from the input box. `None` =
/// the user quit without giving a topic (Ctrl-C/D). Slug resolution is left to
/// the caller. Watcher events are NOT consumed here during topic entry — only
/// keys are listened for; events accumulated in the channel are quietly
/// absorbed after the session is set up (see `run`).
#[allow(clippy::too_many_arguments)]
async fn ask_topic(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
    profile: Option<&str>,
    model: &str,
    dir: &str,
    local: &[String],
    other: &[String],
    show_welcome: bool,
) -> Result<Option<String>> {
    // Topic lists (project-local + other projects) are computed by the caller
    // and passed in here — the global catalog is not read here (see `run`).
    // `show_welcome=false`: when the new-topic confirmation is rejected and we
    // go back to the entry question, the identity welcome + initial notice are
    // NOT printed again.
    if show_welcome {
        let name = profile.and_then(welcome::extract_name);
        let width = current_width(tui);
        page(tui, welcome::render_welcome_identity(name.as_deref(), model, dir, local, other, width))?;
        page_notice(tui, "What do you want to learn? (a word, or describe it in a sentence)")?;
    }

    loop {
        draw(tui, editor, &Status::Idle, None, 0, None)?;
        match events.next().await {
            Some(Ok(Event::Key(k))) => {
                // Empty Enter = resume sentinel (only when there's a topic to resume) —
                // we catch it before the editor swallows the empty line (spec K1 rule 1).
                if matches!(k.code, KeyCode::Enter)
                    && editor.value().trim().is_empty()
                    && !local.is_empty()
                {
                    return Ok(Some(String::new()));
                }
                match editor.handle_key(k) {
                    Action::Submit(line) => return Ok(Some(line)),
                    Action::Exit => return Ok(None),
                    Action::None => {}
                }
            }
            Some(Ok(Event::Paste(s))) => editor.insert_str(&s),
            Some(Ok(_)) | Some(Err(_)) => {} // resize etc. — ignore
            None => return Ok(None), // stream ended — don't spin in a hot loop (spec B4)
        }
    }
}

/// Single-key confirmation in the TUI: print the message, wait for one key. `y`/`Y`/`e`/`E` → true, other → false.
async fn tui_confirm(
    tui: &mut Tui,
    editor: &InputBox,
    events: &mut EventStream,
    msg: &str,
) -> Result<bool> {
    page_notice(tui, msg)?;
    loop {
        draw(tui, editor, &Status::Idle, None, 0, None)?;
        match events.next().await {
            Some(Ok(Event::Key(k))) => match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Char('e') | KeyCode::Char('E') => return Ok(true),
                _ => return Ok(false),
            },
            Some(Ok(_)) | Some(Err(_)) => {} // resize etc. — ignore
            None => return Ok(false), // stream ended — don't spin in a hot loop (spec B4)
        }
    }
}

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
    let mut resumed = false; // whether the resume flow was chosen — triggers the full-mode welcome
    // The RAW text from the user's topic entry — carried into the new-topic intro
    // turn as the "first answer"; reducing it to a slug and discarding it would
    // force the model to re-ask what was already said. Not used in the resume flow.
    let mut intro: Option<String> = None;
    let topic = match topic_arg {
        Some(t) => {
            intro = Some(t.clone());
            crate::slugify_topic(&t)
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
            // The identity welcome is only printed on the first turn — it's not
            // printed again if the new-topic confirmation is rejected and we go back
            // to the entry question.
            let mut welcome_shown = false;
            loop {
                let raw = match ask_topic(
                    &mut tui,
                    &mut editor,
                    &mut events,
                    read(global.join("USER.md")).as_deref(),
                    &backend.label(),
                    &short_dir(project_root),
                    &local,
                    &other,
                    !welcome_shown,
                )
                .await?
                {
                    Some(line) => line,
                    None => return Ok(None), // quit without giving a topic
                };
                welcome_shown = true;
                if !raw.trim().is_empty() {
                    page_user_echo(&mut tui, raw.trim())?;
                }
                // Slash commands at topic entry: the hint above the prompt promises
                // /help, so honor it here too; session-only commands get a pointer
                // instead of silently being slugged into a topic name.
                if crate::help::is_help_command(&raw) {
                    page_notice(&mut tui, crate::help::help_text())?;
                    continue;
                }
                if raw.trim().eq_ignore_ascii_case("/quit") {
                    return Ok(None); // quit without giving a topic
                }
                if crate::visual::parse_show_command(&raw).is_some()
                    || crate::parse_watch_command(&raw).is_some()
                {
                    page_notice(&mut tui, "that command works inside a session — pick a topic first")?;
                    continue;
                }
                match crate::interpret_topic_input(&raw, &local) {
                    // SAFE FALLBACK: interpret only returns None when (input is empty +
                    // local is empty); ask_topic only produces the empty-Enter sentinel
                    // when local is non-empty, so this normally isn't reached. Its natural
                    // counterpart in the loop is "swallow, ask again" — this is the safe fallback.
                    None => {}
                    Some(crate::TopicChoice::Resume(t)) => {
                        page_notice(&mut tui, &format!("resuming: {t}"))?;
                        resumed = true; // for the full-mode welcome below
                        break t;
                    }
                    Some(crate::TopicChoice::New(raw)) => {
                        // New-topic flow: ≤2 words → local slug; a sentence → LLM slug (spinner).
                        let slug = if raw.split_whitespace().count() <= 2 {
                            crate::slugify_topic(&raw)
                        } else {
                            let slug = match ask_live(
                                &mut tui,
                                &mut editor,
                                &mut events,
                                backend,
                                &crate::slug_system(&local),
                                &[Message::user(raw.as_str())],
                                None,
                            )
                            .await
                            {
                                Ok(AskOutcome::Reply(reply)) => crate::finalize_slug(&raw, &reply.text),
                                Ok(AskOutcome::Cancelled) | Err(_) => crate::slugify_topic(&raw),
                            };
                            // The slug mini-session must not carry over into the learning session (spec B1).
                            backend.reset_session();
                            slug
                        };
                        // If the LLM/short slug happens to match a local topic, this also
                        // counts as RESUME (spec K2): the notice becomes "resuming", the
                        // full-mode welcome is printed, NO confirmation.
                        if local.contains(&slug) {
                            page_notice(&mut tui, &format!("resuming: {slug}"))?;
                            resumed = true;
                            break slug;
                        }
                        // New-topic confirmation: only asked when there's a topic that could
                        // be resumed (spec §2) — on first-run/empty-local it opens without confirmation.
                        if local.is_empty()
                            || tui_confirm(
                                &mut tui,
                                &editor,
                                &mut events,
                                &crate::new_topic_confirm_msg(&slug),
                            )
                            .await?
                        {
                            page_notice(&mut tui, &format!("topic: {slug} — tell me the details in chat"))?;
                            intro = Some(raw);
                            break slug;
                        }
                        // Rejected → go back to the entry question (welcome is not printed again).
                        page_notice(&mut tui, "cancelled — Enter = resume, or type another topic")?;
                    }
                }
            }
        }
    };

    // Lock-conflict confirmation (TUI single-key) — BEFORE build_session, without
    // writing its own lock. If rejected, no session/lock → Tui drop restores.
    let lock = crate::lock_path(project_root, &topic);
    if lock.exists()
        && !tui_confirm(
            &mut tui,
            &editor,
            &mut events,
            "Another session may be open for this topic — progress could clash. Continue? [y/N]",
        )
        .await?
    {
        page_notice(&mut tui, "cancelled")?;
        return Ok(None);
    }

    // build_session writes its own lock; the returned lock = same path.
    let (mut session, recorder, lock, has_progress) =
        crate::build_session(global, project_root, &topic, today)?;

    let mut debouncer = watcher::Debouncer::new(std::time::Duration::from_millis(1000));
    let mut files = feedback::FileMemory::new();
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

    // Welcome: if the topic was known upfront (an arg was given) OR resume was
    // chosen, the full-mode learning-status box is printed. On resume, the
    // identity welcome was already printed inside ask_topic; the learning-status
    // box is added on top of it (two boxes stacked — similar to Claude Code's
    // flow). On a purely new topic, only the identity welcome remains.
    if had_topic_arg || resumed {
        let data = welcome::gather(
            read(global.join("USER.md")).as_deref(),
            read(progress::progress_path(project_root, &topic)).as_deref(),
            read(progress::curriculum_path(project_root, &topic)).as_deref(),
            &topic,
            &backend.label(),
            &short_dir(project_root),
        );
        let w = current_width(&tui);
        page(&mut tui, welcome::render_welcome(&data, w))?;
    }

    // Opening drill / intro (the TUI counterpart of main.rs's plain path). If the
    // profile is still the embedded generic template (or doesn't exist at all),
    // Usta doesn't know the user yet — a short introduction instruction is added
    // to the opening turn (spec Ç3a).
    let profile_generic = read(global.join("USER.md"))
        .as_deref()
        .map(crate::profile_is_generic)
        .unwrap_or(true);
    let opening = if has_progress {
        progress::opening_prompt(&topic, profile_generic)
    } else {
        progress::onboarding_prompt(&topic, intro.as_deref(), profile_generic)
    };
    session.push_user(&opening);
    recorder.user(&opening);
    match ask_live(
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
        Ok(AskOutcome::Reply(reply)) => {
            last_tokens = reply.context_tokens;
            let (clean, show_topic) = crate::visual::extract_show_marker(&reply.text);
            let w = current_width(&tui);
            page_reply(&mut tui, &clean, w)?;
            recorder.assistant(&clean);
            session.push_assistant(clean);
            trigger_auto_visual(&mut tui, &mut editor, &mut events, backend, &session, project_root, &topic, show_topic, last_tokens).await?;
        }
        Ok(AskOutcome::Cancelled) => {
            backend.reset_session();
            page_notice(&mut tui, "opening turn cancelled")?;
        }
        Err(e) => page_notice(&mut tui, &format!("opening turn skipped: {e}"))?,
    }

    let mut watching = true;
    loop {
        // Drain the buffer at the start of every iteration — notices that
        // accumulate outside maybe_compact, like a transcript write error,
        // should never be lost either.
        for m in ui::drain_tui_notices() {
            page_notice(&mut tui, &m)?;
        }
        draw(&mut tui, &editor, &Status::Idle, last_tokens, window, Some(watching))?;
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
                        if let Some(cmd) = crate::parse_watch_command(&line) {
                            page_user_echo(&mut tui, &line)?;
                            let (next, msg) = crate::apply_watch(cmd, watching);
                            watching = next;
                            page_notice(&mut tui, msg)?;
                            continue;
                        }
                        if crate::help::is_help_command(&line) {
                            page_user_echo(&mut tui, &line)?;
                            page_notice(&mut tui, crate::help::help_text())?;
                            continue;
                        }
                        if let Some(arg) = crate::visual::parse_show_command(&line) {
                            page_user_echo(&mut tui, &line)?;
                            let concept = arg.clone().unwrap_or_else(|| "visual".to_string());
                            // Borrow care: read the last reply BEFORE any &mut session borrow.
                            let last = crate::last_assistant_text(&session);
                            match crate::show_request(arg, last.as_deref()) {
                                None => page_notice(&mut tui, "nothing to visualize yet — explain something first, or use /show [topic]")?,
                                Some(req) => {
                                    run_visual_generation(&mut tui, &mut editor, &mut events, backend, project_root, &topic, &concept, &req, last_tokens).await?;
                                }
                            }
                            continue;
                        }
                        if line.eq_ignore_ascii_case("/quit") { break; }
                        // Push the submitted line to scrollback as a distinct user block.
                        page_user_echo(&mut tui, &line)?;
                        session.push_user(&line);
                        recorder.user(&line);
                        match ask_live(
                            &mut tui, &mut editor, &mut events, backend,
                            &session.system, session.history(), last_tokens,
                        ).await {
                            Ok(AskOutcome::Reply(reply)) => {
                                last_tokens = reply.context_tokens;
                                let (clean, show_topic) = crate::visual::extract_show_marker(&reply.text);
                                let w = current_width(&tui);
                                page_reply(&mut tui, &clean, w)?;
                                recorder.assistant(&clean);
                                session.push_assistant(clean);
                                crate::maybe_compact(backend, &mut session, project_root, last_tokens).await;
                                trigger_auto_visual(&mut tui, &mut editor, &mut events, backend, &session, project_root, &topic, show_topic, last_tokens).await?;
                            }
                            Ok(AskOutcome::Cancelled) => {
                                // The user turn stays in history (intentional — spec B2); the CLI
                                // session is half-done — don't resume it, the next call should go
                                // with the full transcript.
                                backend.reset_session();
                                page_notice(&mut tui, "response cancelled — your message is kept, continue if you like")?;
                            }
                            Err(e) => page_notice(&mut tui, &format!("error: {e}"))?,
                        }
                    }
                }
            }
            Some(path) = watch_rx.recv() => {
                debouncer.push(path, tokio::time::Instant::now());
            }
            _ = crate::sleep_until_deadline(debouncer.deadline()), if debouncer.deadline().is_some() => {
                let batch = debouncer.flush();
                if batch.len() > max_feedback_batch {
                    page_notice(&mut tui, &format!(
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
                        match crate::handle_file_change(backend, &mut session, &mut files, project_root, &path, &recorder).await {
                            Ok(crate::FileFeedback::Sessiz) => {}
                            Ok(crate::FileFeedback::Bildirim(m)) => page_notice(&mut tui, &m)?,
                            Ok(crate::FileFeedback::Yanit { tokens, reply, show_topic }) => {
                                if let Some(t) = tokens { last_tokens = Some(t); }
                                let w = current_width(&tui);
                                page_reply(&mut tui, &reply.text, w)?;
                                crate::maybe_compact(backend, &mut session, project_root, tokens).await;
                                trigger_auto_visual(&mut tui, &mut editor, &mut events, backend, &session, project_root, &topic, show_topic, last_tokens).await?;
                            }
                            Err(e) => page_notice(&mut tui, &format!("file feedback skipped: {}: {e}", path.display()))?,
                        }
                    }
                }
            }
        }
    }
    // Drain the last iteration's notices right before exit — so a transcript
    // warning that lands in the buffer on the /quit or Exit path is still seen
    // while the TUI is up.
    for m in ui::drain_tui_notices() {
        page_notice(&mut tui, &m)?;
    }
    Ok(Some((session, recorder, lock))) // Tui drop → restore
}

/// Project directory with `$HOME` → `~` abbreviation.
fn short_dir(p: &Path) -> String {
    let s = p.display().to_string();
    match dirs::home_dir() {
        Some(h) => s.replace(&h.display().to_string(), "~"),
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::Modifier;

    fn line_text(l: &ratatui::text::Line) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn user_echo_prefixes_first_line_and_indents_rest() {
        // Wide width → no wrapping, only \n splitting.
        let t = user_echo_text("satır1\nsatır2", 80);
        let lines: Vec<String> = t.lines.iter().map(line_text).collect();
        // [0] blank separator line, [1] ❯ + text, [2] indented continuation.
        assert_eq!(lines[0], "");
        assert_eq!(lines[1], "❯ satır1");
        assert_eq!(lines[2], "  satır2");
    }

    #[test]
    fn user_echo_wraps_long_line_to_width() {
        // 50 'a's, width 20 → inner width 18 → 18+18+14 = 3 content lines.
        // A long single line is NOT cut off, it wraps (bug: page_reply wraps, echo didn't).
        let t = user_echo_text(&"a".repeat(50), 20);
        let lines: Vec<String> = t.lines.iter().map(line_text).collect();
        assert_eq!(lines[0], "");
        assert!(lines[1].starts_with("❯ "), "ilk görsel satır ❯: {:?}", lines[1]);
        assert_eq!(lines[1].chars().filter(|c| *c == 'a').count(), 18, "ilk satır iç genişlik kadar");
        assert!(lines[2].starts_with("  "), "devam satırı girintili: {:?}", lines[2]);
        let total: usize = lines.iter().map(|l| l.chars().filter(|c| *c == 'a').count()).sum();
        assert_eq!(total, 50, "hiçbir karakter kaybolmaz");
        assert!(t.lines.len() >= 4, "birden çok görsel satıra bölündü: {}", t.lines.len());
    }

    #[test]
    fn user_echo_text_is_not_dim() {
        let t = user_echo_text("merhaba", 80);
        // No span carries DIM — that was the root of the visibility issue (spec S1).
        for l in &t.lines {
            for s in &l.spans {
                assert!(!s.style.add_modifier.contains(Modifier::DIM), "DIM span: {:?}", s.content);
            }
        }
    }

    #[test]
    fn user_echo_prefix_is_orange() {
        let t = user_echo_text("x", 80);
        let first = &t.lines[1].spans[0];
        assert_eq!(first.content.as_ref(), "❯ ");
        assert_eq!(first.style.fg, Some(ratatui::style::Color::Indexed(208)));
    }

    #[test]
    fn classify_locked_key_ctrl_c_and_d_are_cancel_requests() {
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert!(matches!(classify_locked_key(ctrl_c), LockedKey::CancelRequest));
        assert!(matches!(classify_locked_key(ctrl_d), LockedKey::CancelRequest));
    }

    #[test]
    fn classify_locked_key_enter_and_chars_are_edits() {
        assert!(matches!(
            classify_locked_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            LockedKey::Edit
        ));
        assert!(matches!(
            classify_locked_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
            LockedKey::Edit
        ));
    }
}
