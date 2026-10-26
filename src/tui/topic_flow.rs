//! Topic resolution for the TUI session: CLI argument, first-run introduction, conversational suggestion or the manual entry loop. Split out of run.rs (SPEC §11 budget).

use std::path::{Path, PathBuf};

use anyhow::Result;
use crossterm::event::EventStream;

use crate::anthropic::Message;
use crate::backend::Backend;
use crate::tui::editor::InputBox;
use crate::tui::term::Tui;
use crate::{history, progress};

/// Outcome of topic resolution (before the lock / build_session).
pub(crate) struct TopicPick {
    pub topic: String,
    pub had_topic_arg: bool,
    pub resumed: bool,
    pub intro: Option<String>,
    pub intro_turns: Option<Vec<crate::tui::intro::IntroTurn>>,
    pub first_run_completed: bool,
}

/// `Ok(None)` = the user quit without giving a topic.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn resolve_topic(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
    backend: &mut Backend,
    global: &Path,
    project_root: &Path,
    today: &str,
    topic_arg: Option<String>,
) -> Result<Option<TopicPick>> {
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
    // 13a: turns of a pre-lock conversation (first-run introduction or
    // conversational start suggestion) — stitched into the session after
    // build_session; when set, the opening turn is NOT injected.
    let mut intro_turns: Option<Vec<crate::tui::intro::IntroTurn>> = None;
    // Set only when `run_first_run` (not `run_suggest`, not `usta start
    // <topic>`) reaches IntroOutcome::Topic — gates the "introduction
    // completed" marker write below, which must fire once a real session
    // exists (after the lock), not the moment the model returns a topic.
    let mut first_run_completed = false;
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
            // First run ever (no marker, no evidence): the introduction replaces
            // the topic prompt — the model's `TOPIC:` line locks the slug
            // (SPEC §4.22). Fallback keeps the manual entry loop below.
            let mut preset_topic: Option<String> = None;
            if crate::setup::intro_needed(global, &index_content) {
                welcome_shown = true;
                match crate::tui::intro::run_first_run(
                    tui,
                    editor,
                    events,
                    backend,
                    global,
                    project_root,
                    today,
                    &local,
                    &other,
                    project_known,
                    week_sessions,
                    streak,
                )
                .await?
                {
                    crate::tui::intro::IntroOutcome::Topic { slug, turns } => {
                        intro_turns = Some(turns);
                        preset_topic = Some(slug);
                        first_run_completed = true;
                    }
                    crate::tui::intro::IntroOutcome::Quit => return Ok(None),
                    crate::tui::intro::IntroOutcome::Fallback => {}
                }
            }
            loop {
                // The introduction already agreed a topic — skip the prompt.
                if let Some(t) = preset_topic.take() {
                    break t;
                }
                let raw = match crate::tui::entry::ask_topic(
                    tui,
                    editor,
                    events,
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
                    crate::tui::page::page_user_echo(tui, raw.trim())?;
                }
                // Slash commands at topic entry: the hint above the prompt promises
                // /help, so honor it here too; session-only commands get a pointer
                // instead of silently being slugged into a topic name.
                if crate::help::is_help_command(&raw) {
                    crate::tui::page::page_notice(tui, crate::help::help_text())?;
                    continue;
                }
                if raw.trim().eq_ignore_ascii_case("/quit") {
                    return Ok(None); // quit without giving a topic
                }
                if crate::visual::parse_show_command(&raw).is_some()
                    || crate::slash::parse_watch_command(&raw).is_some()
                    || crate::slash::is_context_command(&raw)
                {
                    crate::tui::page::page_notice(
                        tui,
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
                        // Conversational start suggestion (SPEC §4.22): the whole
                        // brain rides along and agreement happens in conversation
                        // instead of a yes/no gate ("no, something easier" works).
                        match crate::tui::intro::run_suggest(
                            tui,
                            editor,
                            events,
                            backend,
                            global,
                            project_root,
                            today,
                        )
                        .await?
                        {
                            crate::tui::intro::IntroOutcome::Topic { slug, turns } => {
                                intro_turns = Some(turns);
                                break slug;
                            }
                            crate::tui::intro::IntroOutcome::Quit => return Ok(None),
                            crate::tui::intro::IntroOutcome::Fallback => {}
                        }
                    }
                    Some(crate::topic::TopicChoice::Resume(t)) => {
                        crate::tui::page::page_notice(tui, &format!("resuming: {t}"))?;
                        resumed = true; // for the continuation panel below
                        break t;
                    }
                    Some(crate::topic::TopicChoice::New(raw)) => {
                        // New-topic flow: ≤2 words → local slug; a sentence → LLM slug (spinner).
                        let slug = if raw.split_whitespace().count() <= 2 {
                            crate::topic::slugify_topic(&raw)
                        } else {
                            let slug = match crate::tui::ask::ask_live(
                                tui,
                                editor,
                                events,
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
                            crate::tui::page::page_notice(tui, &format!("resuming: {slug}"))?;
                            resumed = true;
                            break slug;
                        }
                        // New-topic confirmation: only asked when there's a topic that could
                        // be resumed (spec §2) — on first-run/empty-local it opens without confirmation.
                        if local.is_empty()
                            || crate::tui::ask::tui_confirm(
                                tui,
                                editor,
                                events,
                                &crate::topic::new_topic_confirm_msg(&slug),
                            )
                            .await?
                        {
                            crate::tui::page::page_notice(
                                tui,
                                &format!("topic: {slug} — tell me the details in chat"),
                            )?;
                            intro = Some(raw);
                            break slug;
                        }
                        // Rejected → go back to the entry question (welcome is not printed again).
                        crate::tui::page::page_notice(
                            tui,
                            "cancelled — Enter = resume, or type another topic",
                        )?;
                    }
                }
            }
        }
    };

    Ok(Some(TopicPick {
        topic,
        had_topic_arg,
        resumed,
        intro,
        intro_turns,
        first_run_completed,
    }))
}
