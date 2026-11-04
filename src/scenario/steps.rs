//! The steps of a `/scenario` command that more than one verb shares: which
//! tasks get paused, waking a run, the ledger repair, the error log.

use anyhow::{anyhow, Result};

use crate::progress;
use crate::scenario::command::{Ctx, Reply, Seams};
use crate::scenario::container::ContainerEnv;
use crate::scenario::engine::{self, Tick};
use crate::scenario::env::FOLDER;
use crate::scenario::ledger;
use crate::scenario::package::Package;
use crate::scenario::route::Router;
use crate::scenario::store::{RunRecord, RunState, Store};
use crate::scenario::terminal::{window_notice, WindowReq};
use crate::scenario::turns::{resume_files_turn, resume_turn};

pub(crate) fn log_error(ctx: &Ctx, text: &str) {
    let p = ctx.project_root.join(".usta/scenarios/last-error.log");
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(p, format!("{text}\n"));
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

pub(crate) fn title_of(pkgs: &[Package], package: &str) -> String {
    pkgs.iter()
        .find(|p| p.id == package)
        .map(|p| p.title.clone())
        .unwrap_or_else(|| package.to_string())
}

/// Starting or waking a task pauses whatever else runs (`engine::release_others`).
/// Asked before that happens, so the learner can be told.
pub(crate) fn pause_notices(s: &Step, except: Option<&str>) -> Vec<String> {
    s.store
        .all()
        .into_iter()
        .filter(|r| r.state == RunState::Active && Some(r.run_id.as_str()) != except)
        .map(|r| {
            format!(
                "'{}' was running, so it is paused now. Back to it: /scenario resume {}",
                title_of(s.pkgs, &r.package),
                r.run_id
            )
        })
        .collect()
}

/// Bring `rec` back and open its window. A closed window leaves an own active
/// run with a healthy server: that one only gets a new window.
pub(crate) fn resume_run(
    s: &Step,
    env: &mut Router,
    pkg: &Package,
    mut rec: RunRecord,
) -> Result<Reply> {
    let folder = rec.env == FOLDER;
    // A folder task has no server: its files are the state, so container logic must not judge it.
    if folder {
        if !env.folder.dir(&rec.handle())?.is_dir() {
            return Err(anyhow!(
                "the files for this task are gone, so it cannot be resumed; give it up with /scenario drop {} confirm and start it again",
                rec.run_id
            ));
        }
    } else if let Some(e) = server_gone_error(&env.container, &rec) {
        return Err(e);
    }
    let woke = if folder {
        rec.state != RunState::Active
    } else {
        !already_running(&env.container, &rec)
    };
    let mut restarted = true;
    let mut paused = Vec::new();
    if woke {
        paused = pause_notices(s, Some(&rec.run_id));
        if let Some(w) = engine::resume(env, s.store, pkg, &mut rec, s.tick)? {
            log_error(s.ctx, &w);
            restarted = false;
        }
    }
    let scratch = s.ctx.project_root.join(".usta/scenarios");
    let req = WindowReq {
        command_line: &rec.access,
        run_id: &rec.run_id,
        scratch: &scratch,
    };
    let mut notices = vec![if folder {
        folder_resume_notice(&pkg.title, woke)
    } else {
        resume_notice(&pkg.title, woke, restarted)
    }];
    notices.extend(paused);
    notices.push(window_notice(s.seams.launcher, &req));
    // The mentor turn says the services restarted: only true when we woke them.
    let turn = if woke {
        Some(if folder {
            resume_files_turn(pkg, &pkg.instruction()?, rec.last_check.as_deref())
        } else {
            resume_turn(
                pkg,
                &pkg.instruction()?,
                rec.last_check.as_deref(),
                restarted,
            )
        })
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

pub(crate) fn folder_resume_notice(title: &str, woke: bool) -> String {
    if woke {
        format!("Resumed: {title}. Your files are as you left them.")
    } else {
        format!("Back in: {title}. Your task is still open; opened a new terminal window.")
    }
}
