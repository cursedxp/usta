//! `/scenario start` and `/scenario talk`: a package from the library, or a new one.

use anyhow::{anyhow, Result};

use crate::scenario::command::{failure, Ctx, Reply};
use crate::scenario::docker::{probe, runc_warning, Runtime};
use crate::scenario::engine;
use crate::scenario::env::Internal;
use crate::scenario::generate::{self, Job, Outcome};
use crate::scenario::ledger::Ledger;
use crate::scenario::library;
use crate::scenario::model::ModelFailed;
use crate::scenario::package::Package;
use crate::scenario::pick::open_runs;
use crate::scenario::prompt::Brief;
use crate::scenario::route::Router;
use crate::scenario::select::{self, Pick};
use crate::scenario::steps::{
    failed_with, finished_ids, log_error, map_text, model_failed, open_notices, pause_notices,
    ready, ready_to_release, ready_unless, resume_run, running_now, sync_ledger, with_notices,
    Step, NO_RUNTIME,
};
use crate::scenario::store::{project_tag, RunState};
use crate::scenario::sweep;
use crate::scenario::turns::start_turn;
use crate::scenario::validate::RealProver;

/// A generation that could not be tested, or could not fit, ends here.
const AS_TALK: &str =
    "Usta could not build a tested task for this here, so you get it as a conversation instead.";

const LEVEL_LIMIT: usize = 4_000;
const MAP_LIMIT: usize = 12_000;

fn clip(text: String, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn brief(
    s: &Step,
    ledger: &Ledger,
    request: Option<String>,
    runtime: bool,
    talk_only: bool,
) -> Brief {
    let ctx = s.ctx;
    let level = std::fs::read_to_string(ctx.global.join("USER.md")).unwrap_or_default();
    let finished = finished_ids(s.store);
    let titles = s
        .pkgs
        .iter()
        .filter(|p| finished.contains(&p.id))
        .map(|p| p.title.clone())
        .collect();
    Brief {
        topic: ctx.topic.clone(),
        request,
        map: clip(map_text(ctx), MAP_LIMIT),
        level: clip(level, LEVEL_LIMIT),
        world: ledger.for_generation(),
        finished: titles,
        runtime,
        talk_only,
    }
}

/// Why this start cannot use a server.
enum NoServer {
    /// The runtime is missing or will not come up.
    Absent(anyhow::Error),
    /// Another project's task holds the one server allowed.
    Refused(anyhow::Error),
}

impl NoServer {
    fn into_notice(self) -> String {
        match self {
            NoServer::Absent(e) | NoServer::Refused(e) => e.to_string(),
        }
    }
}

/// The refusal of `guard_single` is a sentence for the learner; a failed `docker ps` is
/// tool output, which goes to the log and blocks nothing.
fn refusal(ctx: &Ctx, e: anyhow::Error) -> Option<NoServer> {
    if e.downcast_ref::<Internal>().is_some() {
        log_error(ctx, &format!("{e:#}"));
        return None;
    }
    Some(NoServer::Refused(e))
}

/// Brings the runtime up: only for a task known to need it.
fn server_blocked(s: &Step, env: &Router) -> Option<NoServer> {
    if let Err(e) = ready(s) {
        return Some(NoServer::Absent(e));
    }
    env.container
        .guard_single()
        .err()
        .and_then(|e| refusal(s.ctx, e))
}

pub(crate) fn start(
    s: &Step,
    env: &mut Router,
    sel: Option<String>,
    mut notices: Vec<String>,
) -> Result<Reply> {
    let ctx = s.ctx;
    engine::refuse_other_owner(s.store, s.tick, None)?;
    notices.extend(ready_to_release(s, None));
    sync_ledger(ctx, s.store, s.pkgs, s.tick.today);
    let ledger = Ledger::load(&ctx.project_root, &ctx.topic);
    let offered: Vec<Package> = s
        .pkgs
        .iter()
        .filter(|p| library::fits_world(p, &ledger))
        .cloned()
        .collect();
    let (map, finished) = (map_text(ctx), finished_ids(s.store));
    let open = ledger.open_ids();
    let resolve = |runtime: bool| {
        select::resolve(&offered, sel.as_deref(), &map, &finished, &open, runtime)
            .map_err(|m| anyhow!(m))
    };
    let installed = !matches!(probe(s.seams.docker), Runtime::NotInstalled);
    let mut pick = resolve(installed)?;
    // Only a learner who named a task hears why it cannot run; a bare start moves on.
    let mut blocked = None;
    if sel.is_none() && matches!(pick, Pick::Package(p) if p.needs_runtime()) {
        blocked = server_blocked(s, env);
        if blocked.is_some() {
            pick = resolve(false)?;
        }
    }
    match pick {
        Pick::Package(pkg) if pkg.needs_runtime() => {
            if let Err(e) = ready(s) {
                notices.push(e.to_string());
                return talk(s, env, Some(pkg.title.clone()), notices);
            }
            sweep::sweep_docker(
                &ctx.project_root,
                &ctx.global,
                s.tick.today,
                s.seams.docker,
                s.seams.alive,
            );
            begin(s, env, pkg, &ledger, notices)
        }
        Pick::Package(pkg) => {
            let absent = !installed || matches!(blocked, Some(NoServer::Absent(_)));
            if sel.is_none() && absent && s.store.note_once("no-runtime") {
                notices.push(NO_RUNTIME.into());
            }
            begin(s, env, pkg, &ledger, notices)
        }
        Pick::Generate { request } => {
            generate_and_begin(s, env, &ledger, request, blocked, notices)
        }
    }
}

/// Nothing suitable is in the library. The runtime is only looked at here: starting it
/// waits for a proof that needs it, because most tasks never do.
fn generate_and_begin(
    s: &Step,
    env: &mut Router,
    ledger: &Ledger,
    request: Option<String>,
    mut blocked: Option<NoServer>,
    mut notices: Vec<String>,
) -> Result<Reply> {
    let ctx = s.ctx;
    let docker = s.seams.docker;
    let sweep = || {
        let alive = s.seams.alive;
        sweep::sweep_docker(&ctx.project_root, &ctx.global, s.tick.today, docker, alive)
    };
    let mut up = false;
    if blocked.is_none() {
        match probe(docker) {
            Runtime::Ready => {
                sweep();
                // Said now, not after paying for a generation that cannot be proven.
                blocked = env
                    .container
                    .guard_single()
                    .err()
                    .and_then(|e| refusal(ctx, e));
                up = true;
            }
            Runtime::NotInstalled => blocked = ready(s).err().map(NoServer::Absent),
            Runtime::Stopped => {}
        }
    }
    let b = brief(s, ledger, request, blocked.is_none(), false);
    let mut late = None;
    let outcome = {
        let cancel = &s.seams.live.cancel;
        let paused = &mut notices;
        let late = &mut late;
        let mut free = || {
            if !up {
                let came_up = ready_unless(s, &|| cancel.requested()).and_then(|_| {
                    sweep();
                    env.container.guard_single()
                });
                if let Err(e) = came_up {
                    let told = anyhow!("no server for the proof: {e:#}");
                    if e.downcast_ref::<Internal>().is_none() {
                        *late = Some(NoServer::Absent(e));
                    }
                    return Err(told);
                }
                up = true;
            }
            let before = running_now(s);
            let released = engine::release_others(&mut *env, s.store, s.tick, None);
            paused.extend(pause_notices(s, &before));
            released
        };
        produce(s, &b, ledger, &mut free)
    };
    let skill = b.request.as_deref().unwrap_or(&b.topic);
    let log = match outcome {
        Ok(Outcome::Cancelled) => return Ok(with_notices(Reply::default(), notices)),
        Ok(Outcome::Ready(p)) => {
            let absent = matches!(blocked, Some(NoServer::Absent(_)));
            if absent && s.store.note_once("no-runtime") {
                notices.push(NO_RUNTIME.into());
            }
            return begin(s, env, &p, ledger, notices);
        }
        // Only an unreachable model ends here; every other error (a dead runtime, a disk
        // error) is logged and the learner still gets a conversation. Esc and a shutdown
        // were handled above and in `fall_back`.
        Err(e) if e.downcast_ref::<ModelFailed>().is_some() => {
            return Ok(not_generated(s, e, notices))
        }
        Err(e) => vec![format!("{e:#}")],
        Ok(Outcome::NeedsSystem(why)) => vec![why],
        Ok(Outcome::Failed(log)) => log,
    };
    let why_not = blocked.or(late).map(NoServer::into_notice);
    fall_back(s, env, ledger, skill, &log, Some(AS_TALK), why_not, notices)
}

/// The tail of every failed generation: the learner still gets a task, a conversation
/// from fixed text. `said` and `why_not` are told first; a shutdown installs nothing.
#[allow(clippy::too_many_arguments)] // one call per verb; a struct would only rename them
fn fall_back(
    s: &Step,
    env: &mut Router,
    ledger: &Ledger,
    skill: &str,
    log: &[String],
    said: Option<&str>,
    why_not: Option<String>,
    mut notices: Vec<String>,
) -> Result<Reply> {
    let ctx = s.ctx;
    log_error(ctx, &log.join("\n"));
    if (s.seams.stopping)() {
        return Ok(with_notices(Reply::default(), notices));
    }
    notices.extend(why_not);
    notices.extend(said.map(String::from));
    match generate::fallback_talk(&ctx.global, s.tick.stamp, &ctx.topic, skill) {
        Ok(p) => begin(s, env, &p, ledger, notices),
        Err(e) => failed_with(s, e, notices),
    }
}

/// A model outage gets the one plain sentence. Anything else (a dead runtime, a disk
/// error) is not an outage and takes the normal path. What the learner was already
/// owed, such as a task paused for a proof, is still said.
fn not_generated(s: &Step, e: anyhow::Error, notices: Vec<String>) -> Reply {
    let reply = if e.downcast_ref::<ModelFailed>().is_some() {
        model_failed(s.ctx, &e)
    } else {
        failure(s.ctx, e)
    };
    with_notices(reply, notices)
}

fn produce(
    s: &Step,
    b: &Brief,
    ledger: &Ledger,
    free_server: &mut dyn FnMut() -> Result<()>,
) -> Result<Outcome> {
    let live = s.seams.live;
    let say = |m: &str| (live.progress)(m);
    let job = Job {
        brief: b,
        global: &s.ctx.global,
        stamp: s.tick.stamp,
        model: live.model.as_ref(),
        say: &say,
        ledger,
        cancel: &live.cancel,
    };
    let mut prover = RealProver {
        bases: Some(crate::scenario::bases::file(&s.ctx.global)),
        docker: s.seams.docker,
        store: s.store,
        tag: project_tag(&s.ctx.project_root),
        model: live.model.as_ref(),
        tick: s.tick,
        say: &say,
        free_server,
    };
    generate::run(&job, &mut prover)
}

/// A conversation scenario on `skill`. Needs no runtime, and when generation fails it
/// falls back to fixed text: never after Esc, an outage or a shutdown.
pub(crate) fn talk(
    s: &Step,
    env: &mut Router,
    skill: Option<String>,
    mut notices: Vec<String>,
) -> Result<Reply> {
    let ctx = s.ctx;
    engine::refuse_other_owner(s.store, s.tick, None)?;
    notices.extend(ready_to_release(s, None));
    sync_ledger(ctx, s.store, s.pkgs, s.tick.today);
    let ledger = Ledger::load(&ctx.project_root, &ctx.topic);
    let b = brief(s, &ledger, skill.clone(), false, true);
    let log = match produce(s, &b, &ledger, &mut || Ok(())) {
        Err(e) => return Ok(not_generated(s, e, notices)),
        Ok(Outcome::Cancelled) => return Ok(with_notices(Reply::default(), notices)),
        Ok(Outcome::Ready(p)) => return begin(s, env, &p, &ledger, notices),
        Ok(Outcome::Failed(log)) => log,
        // The prompt rules this reply out, so here it is one more failed attempt.
        Ok(Outcome::NeedsSystem(why)) => vec![why],
    };
    let line = skill.as_deref().unwrap_or(&ctx.topic);
    fall_back(s, env, &ledger, line, &log, None, None, notices)
}

/// Open `pkg`: continue an unfinished run of it, or start one.
fn begin(
    s: &Step,
    env: &mut Router,
    pkg: &Package,
    ledger: &Ledger,
    mut notices: Vec<String>,
) -> Result<Reply> {
    // A proof can pass after the signal: nothing new starts on the learner's behalf.
    if (s.seams.stopping)() {
        return Ok(with_notices(Reply::default(), notices));
    }
    let unfinished = open_runs(s.store, &s.ctx.topic, s.ctx.pid)
        .into_iter()
        .filter(|r| r.package == pkg.id)
        .max_by_key(|r| r.state == RunState::Active);
    if let Some(rec) = unfinished {
        notices.push(format!(
            "You already have '{}' unfinished, so Usta continued it instead of starting over. To start fresh: /scenario drop {} confirm",
            pkg.title, rec.run_id
        ));
        return Ok(with_notices(resume_run(s, env, pkg, rec)?, notices));
    }
    // Generation takes minutes, and the runtime may have died since `start` looked.
    notices.extend(ready_to_release(s, None));
    let tone = s.store.tone();
    if pkg.needs_runtime() {
        notices.extend(runc_warning(s.seams.docker));
    }
    let before = running_now(s);
    let started = engine::start(env, s.store, pkg, tone, s.tick);
    notices.extend(pause_notices(s, &before));
    let rec = match started {
        Ok(rec) => rec,
        Err(e) => return failed_with(s, e, notices),
    };
    notices.push(pkg.tone_text(tone, "open")?);
    notices.extend(open_notices(s, pkg, &rec));
    notices.push("When you think it is solved: /scenario check".into());
    Ok(Reply {
        notices,
        turn: Some(start_turn(
            pkg,
            &pkg.instruction()?,
            tone,
            &ledger.summary(),
        )),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::env::internal;
    use crate::scenario::package::tests::tmp;

    #[test]
    fn a_failed_ps_goes_to_the_log_and_only_the_refusal_reaches_the_learner() {
        let root = tmp("start_refusal");
        let ctx = Ctx {
            global: root.join("global"),
            project_root: root.clone(),
            topic: "linux".into(),
            pid: 1,
        };
        assert!(refusal(&ctx, internal("docker ps failed: boom")).is_none());
        let told = refusal(
            &ctx,
            anyhow!("another Usta project already has a task running"),
        );
        assert!(matches!(told, Some(NoServer::Refused(_))));
        let _ = std::fs::remove_dir_all(root);
    }
}
