//! `/scenario start` and `/scenario talk`: a package from the library, or a new one.

use anyhow::{anyhow, Result};

use crate::scenario::command::{failure, Reply};
use crate::scenario::docker::{probe, runc_warning, Runtime};
use crate::scenario::engine;
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
    finished_ids, log_error, map_text, model_failed, open_notices, pause_notices, ready,
    ready_to_release, resume_run, sync_ledger, Step, NO_RUNTIME,
};
use crate::scenario::store::{project_tag, RunState};
use crate::scenario::sweep;
use crate::scenario::turns::start_turn;
use crate::scenario::validate::RealProver;

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

pub(crate) fn start(
    s: &Step,
    env: &mut Router,
    sel: Option<String>,
    notices: Vec<String>,
) -> Result<Reply> {
    let ctx = s.ctx;
    ready_to_release(s, None)?;
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
    let mut pick = resolve(!matches!(probe(s.seams.docker), Runtime::NotInstalled))?;
    // Only a learner who named a task hears why it cannot run; a bare start moves on.
    let mut down = None;
    if sel.is_none() && matches!(pick, Pick::Package(p) if p.needs_runtime()) {
        down = ready(s).err();
        if down.is_some() {
            pick = resolve(false)?;
        }
    }
    match pick {
        Pick::Package(pkg) => {
            if pkg.needs_runtime() {
                ready(s)?;
                sweep::sweep_docker(
                    &ctx.project_root,
                    s.tick.today,
                    s.seams.docker,
                    s.seams.alive,
                );
            }
            begin(s, env, pkg, &ledger, notices)
        }
        Pick::Generate { request } => {
            let why_not = down.or_else(|| ready(s).err());
            generate_and_begin(s, env, &ledger, request, why_not, notices)
        }
    }
}

/// Nothing suitable is in the library. `why_not`: this computer cannot run a server now.
fn generate_and_begin(
    s: &Step,
    env: &mut Router,
    ledger: &Ledger,
    request: Option<String>,
    why_not: Option<anyhow::Error>,
    mut notices: Vec<String>,
) -> Result<Reply> {
    let ctx = s.ctx;
    let runtime = why_not.is_none();
    if runtime {
        sweep::sweep_docker(
            &ctx.project_root,
            s.tick.today,
            s.seams.docker,
            s.seams.alive,
        );
        // Say so now, not after paying for a generation that cannot be proven.
        env.container.guard_single()?;
    }
    let b = brief(s, ledger, request, runtime, false);
    let outcome = {
        let paused = &mut notices;
        let mut free = || {
            paused.extend(pause_notices(s, None));
            engine::release_others(&mut *env, s.store, s.tick, None)
        };
        produce(s, &b, ledger, &mut free)
    };
    match outcome {
        Err(e) => Ok(not_generated(s, e, notices)),
        Ok(Outcome::Cancelled) => Ok(with_notices(Reply::default(), notices)),
        Ok(Outcome::Ready(p)) => {
            if !runtime && s.store.note_once("no-runtime") {
                notices.push(NO_RUNTIME.into());
            }
            begin(s, env, &p, ledger, notices)
        }
        Ok(Outcome::NeedsSystem(why)) => {
            log_error(ctx, &why);
            if let Some(e) = why_not {
                notices.push(e.to_string());
            }
            notices.push(offer(&b));
            Ok(with_notices(Reply::default(), notices))
        }
        Ok(Outcome::Failed(log)) => {
            log_error(ctx, &log.join("\n"));
            notices.push(format!(
                "Usta tried to build a tested task here and could not, so none was started. {}",
                offer(&b)
            ));
            Ok(with_notices(Reply::default(), notices))
        }
    }
}

fn offer(b: &Brief) -> String {
    let skill = b.request.as_deref().unwrap_or(&b.topic);
    format!("You can still practise it as a conversation: /scenario talk {skill}")
}

fn with_notices(mut r: Reply, mut notices: Vec<String>) -> Reply {
    notices.append(&mut r.notices);
    r.notices = notices;
    r
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
    notices: Vec<String>,
) -> Result<Reply> {
    let ctx = s.ctx;
    ready_to_release(s, None)?;
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
    log_error(ctx, &log.join("\n"));
    if (s.seams.stopping)() {
        return Ok(with_notices(Reply::default(), notices));
    }
    let line = skill.as_deref().unwrap_or(&ctx.topic);
    let p = generate::fallback_talk(&ctx.global, s.tick.stamp, &ctx.topic, line)?;
    begin(s, env, &p, &ledger, notices)
}

/// Open `pkg`: continue an unfinished run of it, or start one.
fn begin(
    s: &Step,
    env: &mut Router,
    pkg: &Package,
    ledger: &Ledger,
    mut notices: Vec<String>,
) -> Result<Reply> {
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
    let tone = s.store.tone();
    if pkg.needs_runtime() {
        notices.extend(runc_warning(s.seams.docker));
    }
    let paused = pause_notices(s, None);
    let rec = engine::start(env, s.store, pkg, tone, s.tick)?;
    notices.extend(paused);
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
