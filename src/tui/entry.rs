//! Topic entry (`ask_topic`) and the visual-generation trigger pair
//! (`run_visual_generation`, `trigger_auto_visual`) — extracted from `run.rs`
//! (cleanup round, Task 6).

use std::path::Path;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode};
use futures_util::StreamExt;

use crate::anthropic::Message;
use crate::backend::Backend;
use crate::session::Session;
use crate::tui::editor::{Action, InputBox};
use crate::tui::status::Status;
use crate::tui::term::Tui;
use crate::tui::welcome;
use crate::tui::welcome_data;

/// TUI sibling of plain.rs's `run_visual_generation` — same guarantees: isolated
/// mini-session, `backend.reset_session()` on every exit path (success, cancel,
/// error, invalid JSON), Esc-cancellable via `ask_live`, same "try /show again"
/// notice on bad JSON. Shared by the explicit `/show <topic>` command and the
/// auto-triggered `[[show: ...]]` marker (Görev 4).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_visual_generation(
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
    // `page_notice(...)?` must NOT be allowed to early-return here — every arm's
    // notice Result is captured instead, so `backend.reset_session()` below always
    // runs before any terminal-IO error is propagated (Görev 6 carry-forward fix).
    let notice_result: Result<()> = match crate::tui::ask::ask_live(
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
        Ok(crate::tui::ask::AskOutcome::Reply(reply)) => {
            let json = crate::progress::clean_markdown_reply(&reply.text);
            match crate::visual::build_visual_html(&json) {
                Ok(html) => {
                    let path = crate::visual::visual_path(project_root, topic, concept);
                    let dir = path.parent().map(|d| d.to_path_buf());
                    if let Some(d) = &dir {
                        let _ = std::fs::create_dir_all(d);
                    }
                    match std::fs::write(&path, html) {
                        Ok(()) => {
                            // Görev 5: keep the last 10 visuals per topic — prune AFTER
                            // the write, so `10` is the exact post-write count on disk.
                            if let Some(d) = &dir {
                                crate::visual::prune_visuals(d, 10);
                            }
                            let opened = crate::visual::open_in_browser(&path);
                            crate::tui::page::page_notice(
                                tui,
                                &format!(
                                    "visual saved: {}{}",
                                    path.display(),
                                    if opened {
                                        ""
                                    } else {
                                        " (open it in your browser)"
                                    }
                                ),
                            )
                        }
                        Err(e) => crate::tui::page::page_error(tui, &format!("error: {e}")),
                    }
                }
                Err(e) => crate::tui::page::page_error(
                    tui,
                    &format!("visual generation failed ({e}) — try /show again"),
                ),
            }
        }
        Ok(crate::tui::ask::AskOutcome::Cancelled) => {
            crate::tui::page::page_notice(tui, "visual generation cancelled")
        }
        Err(e) => crate::tui::page::page_error(tui, &format!("error: {e}")),
    };
    backend.reset_session(); // all paths — slug parity, ALWAYS runs (see notice_result above)
    notice_result
}

/// After a normal reply has been displayed and recorded, run the visual flow
/// if `[[show: ...]]` was found in it (Görev 4). No-op when `show_topic` is
/// `None`. Reuses `show_request` with the just-pushed clean reply as context —
/// same composition the explicit `/show <topic>` command uses.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn trigger_auto_visual(
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
    if let Some(req) = crate::visual::show_request(
        Some(t.clone()),
        crate::visual::last_assistant_text(session).as_deref(),
    ) {
        crate::tui::page::page_notice(tui, &format!("visualizing: {t}…"))?;
        run_visual_generation(
            tui,
            editor,
            events,
            backend,
            project_root,
            topic,
            &t,
            &req,
            last_tokens,
        )
        .await?;
    }
    Ok(())
}

/// Prints the identity welcome and reads the topic from the input box. `None` =
/// the user quit without giving a topic (Ctrl-C/D). Slug resolution is left to
/// the caller. Watcher events are NOT consumed here during topic entry — only
/// keys are listened for; events accumulated in the channel are quietly
/// absorbed after the session is set up (see `run`).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn ask_topic(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
    profile: Option<&str>,
    model: &str,
    dir: &str,
    local: &[String],
    other: &[String],
    project_known: bool,
    show_welcome: bool,
    week_sessions: u32,
    streak: u32,
) -> Result<Option<String>> {
    // Topic lists (project-local + other projects) are computed by the caller
    // and passed in here — the global catalog is not read here (see `run`).
    // `show_welcome=false`: when the new-topic confirmation is rejected and we
    // go back to the entry question, the identity welcome + initial notice are
    // NOT printed again.
    if show_welcome {
        let name = profile.and_then(welcome_data::extract_name);
        let width = crate::tui::page::current_width(tui);
        crate::tui::page::page(
            tui,
            welcome::render_welcome_identity(
                name.as_deref(),
                model,
                dir,
                local,
                other,
                project_known,
                width,
                week_sessions,
                streak,
            ),
        )?;
        // The "Enter = suggests" hint is only truthful on a first session with no
        // resumable topics — when `local` is non-empty, empty Enter resumes
        // instead (see welcome box above), so the suggest wording must not show.
        let prompt_line = if project_known && local.is_empty() {
            "What do you want to learn? (Enter = Usta suggests from PROJECT.md; or type a topic)"
        } else {
            "What do you want to learn? (a word, or describe it in a sentence)"
        };
        crate::tui::page::page_notice(tui, prompt_line)?;
    }

    loop {
        crate::tui::page::draw(tui, editor, &Status::Idle, None, 0, None)?;
        match events.next().await {
            Some(Ok(Event::Key(k))) => {
                // Empty Enter = resume sentinel (only when there's a topic to resume) —
                // we catch it before the editor swallows the empty line (spec K1 rule 1).
                // `editor.handle_key` returns `Action::None` on an empty Enter, so without
                // this early return the key would be silently swallowed. Empty Enter also
                // returns the sentinel when a project is known, so the Suggest flow can
                // trigger even with no resumable topics.
                if matches!(k.code, KeyCode::Enter)
                    && editor.value().trim().is_empty()
                    && (!local.is_empty() || project_known)
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
            Some(Ok(Event::Resize(_, _))) => crate::tui::page::handle_resize(tui)?,
            Some(Ok(_)) | Some(Err(_)) => {} // other events — ignore
            None => return Ok(None),         // stream ended — don't spin in a hot loop (spec B4)
        }
    }
}
