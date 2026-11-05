//! The steps of a `/scenario` command that more than one verb shares: which
//! tasks get paused, waking a run, the ledger repair, the error log.

use anyhow::{anyhow, Result};

use crate::progress;
use crate::scenario::command::{failure, Ctx, Reply, Seams};
use crate::scenario::container::ContainerEnv;
use crate::scenario::docker::{ensure_ready, START_POLL, START_TIMEOUT};
use crate::scenario::engine::{self, Tick};
use crate::scenario::env::{CONTAINER, FOLDER};
use crate::scenario::folder::{gone_error, DONE_DIR, TASKS_DIR};
use crate::scenario::ledger;
use crate::scenario::package::Package;
use crate::scenario::route::Router;
use crate::scenario::store::{RunRecord, RunState, Store};
use crate::scenario::sweep::{owner_alive, park};
use crate::scenario::terminal::{window_notice, WindowReq};
use crate::scenario::turns::resume_turn;

pub(crate) const NO_RUNTIME: &str = "Usta cannot start a practice server on this computer right now, so it picked a task that needs none.";

const MODEL_DOWN: &str = "Usta could not reach the model just now, so no task was started. Try again in a moment; details are in .usta/scenarios/last-error.log";

pub(crate) fn log_error(ctx: &Ctx, text: &str) {
    let p = ctx.project_root.join(".usta/scenarios/last-error.log");
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(p, format!("{text}\n"));
}

pub(crate) fn model_failed(ctx: &Ctx, e: &anyhow::Error) -> Reply {
    log_error(ctx, &format!("{e:#}"));
    Reply::note(MODEL_DOWN)
}

/// A paused task whose server no longer exists cannot be woken. Say so in one
/// plain sentence instead of letting the raw runtime error through.
pub(crate) fn server_gone_error(env: &ContainerEnv, rec: &RunRecord) -> Option<anyhow::Error> {
    let gone = rec.reference.is_empty() || env.is_gone(&rec.reference);
    gone.then(|| {
        anyhow!(
            "the server for this task is gone, so it cannot be resumed; give it up with /scenario drop {} confirm and start it again",
            rec.run_id
        )
    })
}

pub(crate) fn finished_ids(store: &Store) -> Vec<String> {
    store
        .all()
        .into_iter()
        .filter(|r| r.state == RunState::Finished)
        .map(|r| r.package)
        .collect()
}

pub(crate) fn map_text(ctx: &Ctx) -> String {
    std::fs::read_to_string(progress::curriculum_path(&ctx.project_root, &ctx.topic))
        .unwrap_or_default()
}

/// Apply finished runs the ledger has not seen (repairs a crash between the
/// finished record and the ledger write). Never fatal.
pub(crate) fn sync_ledger(ctx: &Ctx, store: &Store, pkgs: &[Package], today: &str) {
    let _ = ledger::reconcile(&ctx.project_root, &ctx.topic, &store.all(), pkgs, today);
}

/// What a step needs around the run it acts on.
pub(crate) struct Step<'a> {
    pub ctx: &'a Ctx,
    pub seams: &'a Seams<'a>,
    pub store: &'a Store,
    pub tick: &'a Tick<'a>,
    pub pkgs: &'a [Package],
}

pub(crate) fn ready(s: &Step) -> Result<()> {
    ready_unless(s, &|| false)
}

/// `stop` ends the wait early, with an error.
pub(crate) fn ready_unless(s: &Step, stop: &dyn Fn() -> bool) -> Result<()> {
    let m = s.seams;
    ensure_ready(
        m.docker,
        m.starter,
        START_TIMEOUT,
        START_POLL,
        m.sleep,
        stop,
    )
}

/// Starting or waking any task pauses a running server task, and only the runtime can
/// stop that server, whatever the new task needs. A runtime that will not come up is
/// no reason to stop here: the record is parked, as `sweep_records` does, and a later
/// sweep stops a server that survived. A task that needs no server goes on, and one
/// that does says so itself when it asks for the runtime. Returns what the learner is
/// owed for a parked task. Another live Usta's run is never touched.
pub(crate) fn ready_to_release(s: &Step, except: Option<&str>) -> Vec<String> {
    let running: Vec<RunRecord> = s
        .store
        .all()
        .into_iter()
        .filter(|r| {
            r.state == RunState::Active
                && r.env == CONTAINER
                && Some(r.run_id.as_str()) != except
                && (r.owner_pid == s.tick.pid || !owner_alive(r, s.tick.alive))
        })
        .collect();
    if running.is_empty() || ready(s).is_ok() {
        return Vec::new();
    }
    for r in &running {
        let _ = park(s.store, &mut r.clone(), s.tick.today);
    }
    pause_notices(s, &running)
}

pub(crate) fn title_of(pkgs: &[Package], package: &str) -> String {
    pkgs.iter()
        .find(|p| p.id == package)
        .map(|p| p.title.clone())
        .unwrap_or_else(|| package.to_string())
}

/// What runs now. Starting or waking a task pauses it (`engine::release_others`).
pub(crate) fn running_now(s: &Step) -> Vec<RunRecord> {
    s.store
        .all()
        .into_iter()
        .filter(|r| r.state == RunState::Active)
        .collect()
}

/// One line for each of `before` that is really paused now.
pub(crate) fn pause_notices(s: &Step, before: &[RunRecord]) -> Vec<String> {
    before
        .iter()
        .filter(|r| {
            s.store
                .load(&r.run_id)
                .is_ok_and(|n| n.state == RunState::Paused)
        })
        .map(|r| {
            format!(
                "'{}' was running, so it is paused now. Back to it: /scenario resume {}",
                title_of(s.pkgs, &r.package),
                r.run_id
            )
        })
        .collect()
}

pub(crate) fn with_notices(mut r: Reply, mut notices: Vec<String>) -> Reply {
    notices.append(&mut r.notices);
    r.notices = notices;
    r
}

/// A step failed after the learner was owed `notices`: an `Err` would lose them.
pub(crate) fn failed_with(s: &Step, e: anyhow::Error, notices: Vec<String>) -> Result<Reply> {
    if notices.is_empty() {
        return Err(e);
    }
    Ok(with_notices(failure(s.ctx, e), notices))
}

/// How the learner reaches the task: a window, a folder or this conversation.
pub(crate) fn open_notices(s: &Step, pkg: &Package, rec: &RunRecord) -> Vec<String> {
    match rec.env.as_str() {
        CONTAINER => {
            let scratch = s.ctx.project_root.join(".usta/scenarios");
            let req = WindowReq {
                command_line: &rec.access,
                run_id: &rec.run_id,
                scratch: &scratch,
            };
            let mut notices = vec![window_notice(s.seams.launcher, &req)];
            if pkg.environment.workspace.is_some() {
                notices.push(folder_notice(s, pkg, rec));
            }
            notices
        }
        FOLDER => vec![folder_notice(s, pkg, rec)],
        _ => vec!["Answer here, in this conversation.".to_string()],
    }
}

fn folder_notice(s: &Step, pkg: &Package, rec: &RunRecord) -> String {
    format!(
        "Your task folder is {}. Work there in your own editor; when the task ends, your files move to {TASKS_DIR}/{DONE_DIR}/.{}",
        s.store.tasks_dir().join(&rec.run_id).display(),
        deliverables_note(pkg)
    )
}

/// A folder task, or a server task whose files the learner edits on their own computer.
pub(crate) fn keeps_folder(pkgs: &[Package], rec: &RunRecord) -> bool {
    rec.env == FOLDER
        || pkgs
            .iter()
            .any(|p| p.id == rec.package && p.environment.workspace.is_some())
}

fn deliverables_note(pkg: &Package) -> String {
    if pkg.check.deliverables.is_empty() {
        String::new()
    } else {
        format!(" Write: {}.", pkg.check.deliverables.join(", "))
    }
}

/// Where a finished or dropped folder task's files are now. A failed move leaves them in place.
pub(crate) fn kept_notice(s: &Step, rec: &RunRecord, warning: Option<&str>) -> String {
    match warning {
        None => format!("Your files are kept in {TASKS_DIR}/{DONE_DIR}/."),
        Some(raw) => {
            log_error(s.ctx, raw);
            format!(
                "Your files could not be moved, so they are still in {}.",
                s.store.tasks_dir().join(&rec.run_id).display()
            )
        }
    }
}

/// Bring `rec` back and show how to reach it. A closed window leaves an own active
/// run with a healthy server: that one only gets a new window.
pub(crate) fn resume_run(
    s: &Step,
    env: &mut Router,
    pkg: &Package,
    mut rec: RunRecord,
) -> Result<Reply> {
    // Only a container task has a server, so container logic must not judge the others.
    let woke = match rec.env.as_str() {
        CONTAINER => {
            if pkg.environment.workspace.is_some() {
                env.container.workspace_present(&rec.handle())?;
            }
            if let Some(e) = server_gone_error(&env.container, &rec) {
                return Err(e);
            }
            !already_running(&env.container, &rec)
        }
        FOLDER => {
            if !env.folder.dir(&rec.handle())?.is_dir() {
                return Err(gone_error(&rec.run_id));
            }
            rec.state != RunState::Active
        }
        // A conversation has nothing to wake: resuming is a fresh mentor turn.
        _ => true,
    };
    let mut restarted = true;
    let mut paused = Vec::new();
    if woke {
        paused = ready_to_release(s, Some(&rec.run_id));
        let before = running_now(s);
        let woken = engine::resume(env, s.store, pkg, &mut rec, s.tick);
        paused.extend(pause_notices(s, &before));
        match woken {
            Ok(None) => {}
            Ok(Some(w)) => {
                log_error(s.ctx, &w);
                restarted = false;
            }
            Err(e) => return failed_with(s, e, paused),
        }
    }
    let mut notices = vec![if rec.env == CONTAINER {
        resume_notice(&pkg.title, woke, restarted)
    } else if woke {
        format!("Resumed: {}.", pkg.title)
    } else {
        format!("Back in: {}.", pkg.title)
    }];
    notices.extend(paused);
    notices.extend(open_notices(s, pkg, &rec));
    // The mentor turn says the services restarted: only true when we woke them.
    let turn = if woke {
        Some(resume_turn(
            pkg,
            &pkg.instruction()?,
            rec.last_check.as_deref(),
            restarted,
        ))
    } else {
        None
    };
    Ok(Reply { notices, turn })
}

/// An own active run whose server is still running needs no wake-up.
pub(crate) fn already_running(env: &ContainerEnv, rec: &RunRecord) -> bool {
    rec.state == RunState::Active && env.is_running(&rec.reference)
}

pub(crate) fn resume_notice(title: &str, woke: bool, restarted: bool) -> String {
    if !restarted {
        format!("Resumed: {title}. Your files are as you left them, but its programs did not come back up; you are in, so you can look into it. Details are in .usta/scenarios/last-error.log")
    } else if woke {
        format!("Resumed: {title}. Your files are as you left them; its programs were restarted.")
    } else {
        format!("Back in: {title}. Your task is still running; opened a new terminal window.")
    }
}
