//! Startup cleanup. Touches only containers carrying `usta=1` AND this
//! project's tag. Pure `plan` decides; `execute` acts. An unfinished task is
//! never removed without asking.

use std::path::Path;

use chrono::NaiveDate;

use crate::scenario::container::{list_labeled, Found};
use crate::scenario::docker::{probe, Docker, Out, RealDocker, Runtime};
use crate::scenario::engine::{self, Tick};
use crate::scenario::env::{Environment, CONTAINER, FOLDER};
use crate::scenario::folder::valid_run_id;
use crate::scenario::route::Router;
use crate::scenario::store::{project_tag, RunRecord, RunState, Store};
use crate::ui;

/// Provisional: the spec lists the idle age as an open measurement.
pub(crate) const STALE_DAYS: i64 = 14;

#[derive(Debug, PartialEq)]
pub(crate) enum Action {
    RemoveContainer(String),
    /// A paused task's container is running with nobody on it: stop it, keep the work.
    StopContainer(String),
    PauseRecord {
        run_id: String,
        container: String,
    },
    DeleteRecord(String),
    AskStale {
        run_id: String,
        package: String,
        days: i64,
    },
    /// The record points outside this project's container namespace (hand-edited,
    /// or copied from another project; a moved folder keeps its tag and is not
    /// this). It can never be acted on, so it must not block a new start; it is
    /// marked dropped, never deleted, and no container is ever removed for it.
    DropForeign {
        run_id: String,
        package: String,
    },
}

pub(crate) fn age_days(updated: &str, today: &str) -> Option<i64> {
    let u = NaiveDate::parse_from_str(updated, "%Y-%m-%d").ok()?;
    let t = NaiveDate::parse_from_str(today, "%Y-%m-%d").ok()?;
    Some((t - u).num_days())
}

fn prefix(tag: &str) -> String {
    format!("usta-{tag}-")
}

/// Takes a record out of use without the runtime. The work stays; a server that is
/// still running is stopped by a later sweep, because the record is now `Paused`.
pub(crate) fn park(store: &Store, r: &mut RunRecord, today: &str) -> anyhow::Result<()> {
    r.state = RunState::Paused;
    r.owner_pid = 0;
    r.updated = today.to_string();
    store.save(r)
}

/// True when the owner of a record is a running process.
pub(crate) fn owner_alive(r: &RunRecord, alive: &dyn Fn(u32) -> bool) -> bool {
    r.owner_pid != 0 && alive(r.owner_pid)
}

/// A `chk-<run>` helper belongs to the run it checks; a `bld-<package>` helper to a
/// proof of that package (its `val-` record is written before the build starts). With
/// no record of a live owner it is a leftover, and a second session may remove it.
fn helper_has_live_owner(run_id: &str, records: &[RunRecord], alive: &dyn Fn(u32) -> bool) -> bool {
    if let Some(run) = run_id.strip_prefix("chk-") {
        records
            .iter()
            .any(|r| r.run_id == run && owner_alive(r, alive))
    } else if let Some(package) = run_id.strip_prefix("bld-") {
        records
            .iter()
            .any(|r| r.package == package && owner_alive(r, alive))
    } else {
        false
    }
}

pub(crate) fn plan(
    records: &[RunRecord],
    containers: &[Found],
    alive: &dyn Fn(u32) -> bool,
    today: &str,
    tag: &str,
) -> Vec<Action> {
    let mut acts = Vec::new();
    let mine = prefix(tag);
    let find = |run_id: &str| containers.iter().find(|c| c.run_id == run_id);
    let foreign = |r: &RunRecord| !r.reference.is_empty() && !r.reference.starts_with(&mine);
    for c in containers {
        if helper_has_live_owner(&c.run_id, records, alive) {
            continue;
        }
        match records.iter().find(|r| r.run_id == c.run_id) {
            None => acts.push(Action::RemoveContainer(c.name.clone())),
            Some(r) if foreign(r) => {}
            Some(r) if matches!(r.state, RunState::Finished | RunState::Dropped) => {
                acts.push(Action::RemoveContainer(c.name.clone()))
            }
            Some(_) => {}
        }
    }
    for r in records {
        let unfinished = matches!(
            r.state,
            RunState::Preparing | RunState::Active | RunState::Paused
        );
        if unfinished && foreign(r) {
            acts.push(Action::DropForeign {
                run_id: r.run_id.clone(),
                package: r.package.clone(),
            });
            continue;
        }
        let owner_dead = r.owner_pid == 0 || !alive(r.owner_pid);
        match r.state {
            RunState::Preparing if owner_dead => {
                if let Some(c) = find(&r.run_id) {
                    acts.push(Action::RemoveContainer(c.name.clone()));
                }
                acts.push(Action::DeleteRecord(r.run_id.clone()));
            }
            RunState::Active if owner_dead => acts.push(Action::PauseRecord {
                run_id: r.run_id.clone(),
                container: r.reference.clone(),
            }),
            RunState::Paused => {
                if let Some(c) = find(&r.run_id).filter(|c| owner_dead && c.state == "running") {
                    acts.push(Action::StopContainer(c.name.clone()));
                }
                if let Some(days) = age_days(&r.updated, today).filter(|d| *d >= STALE_DAYS) {
                    acts.push(Action::AskStale {
                        run_id: r.run_id.clone(),
                        package: r.package.clone(),
                        days,
                    });
                }
            }
            _ => {}
        }
    }
    acts
}

/// Run once at startup, before the session UI. Record-only work first (needs
/// no Docker, which Usta itself usually starts later), then the Docker side.
/// This is the only entry point that may ask the learner a question.
pub(crate) fn startup(project_root: &Path, today: &str) {
    drop_foreign(project_root, today);
    let interactive = std::io::IsTerminal::is_terminal(&std::io::stdin())
        && std::io::IsTerminal::is_terminal(&std::io::stdout());
    let ask = |prompt: &str| crate::setup::confirm(prompt, &["y", "yes"]).unwrap_or(false);
    let asker: Option<&dyn Fn(&str) -> bool> = if interactive { Some(&ask) } else { None };
    let global = crate::config::global_root().ok();
    if let Some(global) = &global {
        let store = Store::new(project_root);
        if store.exists() {
            let idle = std::time::SystemTime::now() - crate::scenario::leftovers::DRAFT_IDLE;
            crate::scenario::leftovers::sweep_leftovers(
                &store,
                global,
                &crate::lock::pid_alive,
                idle,
            );
        }
    }
    sweep_records(
        project_root,
        today,
        &RealDocker,
        &crate::lock::pid_alive,
        asker,
    );
    // Without the global folder a proof's image cannot be judged, so its record stays.
    if let Some(global) = &global {
        docker_sweep(
            project_root,
            global,
            today,
            &RealDocker,
            &crate::lock::pid_alive,
            asker,
        );
        crate::scenario::bases::sweep(&RealDocker, &crate::scenario::bases::file(global), today);
    }
}

/// Record-only sweep, no Docker: unfinished records that point outside this
/// project are marked dropped so they can never block a start.
pub(crate) fn drop_foreign(project_root: &Path, today: &str) {
    let store = Store::new(project_root);
    if !store.exists() {
        return;
    }
    let tag = project_tag(project_root);
    let dead: &dyn Fn(u32) -> bool = &|_| false;
    for a in plan(&store.all(), &[], dead, today, &tag) {
        if let Action::DropForeign { run_id, package } = a {
            mark_foreign(&store, &run_id, &package, today);
        }
    }
}

/// A runtime that is not there: the record pass provably never calls Docker.
struct NoRuntime;

impl Docker for NoRuntime {
    fn run(&self, _: &[&str]) -> anyhow::Result<Out> {
        anyhow::bail!("no runtime on this path")
    }
}

/// The part of the sweep that needs no runtime: folder and conversation runs, and
/// container records parked when Docker is not installed (no runtime, so no server can
/// be running). Otherwise either would block the learner on a computer with no Docker.
/// `docker` is used for one probe only, and only when a container run looks active.
/// The one place a stale task may be asked about is a launch, where `ask` is `Some`.
pub(crate) fn sweep_records(
    project_root: &Path,
    today: &str,
    docker: &dyn Docker,
    alive: &dyn Fn(u32) -> bool,
    ask: Option<&dyn Fn(&str) -> bool>,
) {
    let store = Store::new(project_root);
    if !store.exists() {
        return;
    }
    // A container run whose owner is gone while the runtime does not answer (missing or
    // stopped) is parked; a still-running container is stopped by a later sweep, when the
    // runtime is back and the record is `Paused`. A live owner is never parked here.
    let stale =
        |r: &RunRecord| r.env == CONTAINER && r.state == RunState::Active && !owner_alive(r, alive);
    if store.all().iter().any(stale) && !matches!(probe(docker), Runtime::Ready) {
        for mut r in store.all().into_iter().filter(stale) {
            let _ = park(&store, &mut r, today);
        }
    }
    let tag = project_tag(project_root);
    let own: Vec<RunRecord> = store
        .all()
        .into_iter()
        .filter(|r| r.env != CONTAINER)
        .collect();
    let s = Sweeper {
        store: &store,
        docker: &NoRuntime,
        tag: &tag,
        containers: &[],
        alive,
        today,
        ask,
    };
    execute_on(&own, &s);
}

/// Docker-side sweep for use INSIDE a session (call right after `ensure_ready`
/// in the start and resume paths). It never asks and never reads stdin: a stale
/// task is skipped and asked about at the next launch. It parks an own `Active`
/// record whose owner is dead, even when the container is gone, so that record
/// cannot block starting. Failures are warnings, never fatal.
pub(crate) fn sweep_docker(
    project_root: &Path,
    global: &Path,
    today: &str,
    docker: &dyn Docker,
    alive: &dyn Fn(u32) -> bool,
) {
    docker_sweep(project_root, global, today, docker, alive, None);
}

fn docker_sweep(
    project_root: &Path,
    global: &Path,
    today: &str,
    docker: &dyn Docker,
    alive: &dyn Fn(u32) -> bool,
    ask: Option<&dyn Fn(&str) -> bool>,
) {
    if !Store::new(project_root).exists() || !matches!(probe(docker), Runtime::Ready) {
        return;
    }
    sweep_with(project_root, global, today, docker, alive, ask);
}

fn sweep_with(
    project_root: &Path,
    global: &Path,
    today: &str,
    docker: &dyn Docker,
    alive: &dyn Fn(u32) -> bool,
    ask: Option<&dyn Fn(&str) -> bool>,
) {
    let store = Store::new(project_root);
    let tag = project_tag(project_root);
    let Ok(containers) = list_labeled(docker, &tag) else {
        return;
    };
    // A cut-short proof's record is what names its image and the record pass deletes it,
    // so it is read first; the image goes last, once no container of it is left.
    let records = store.all();
    execute(&store, docker, &tag, &containers, alive, today, ask);
    crate::scenario::leftovers::sweep_images(global, docker, &records, alive);
}

fn mark_foreign(store: &Store, run_id: &str, package: &str, today: &str) {
    let Ok(mut rec) = store.load(run_id) else {
        return;
    };
    rec.state = RunState::Dropped;
    rec.owner_pid = 0;
    rec.updated = today.to_string();
    if store.save(&rec).is_ok() {
        ui::notice(&format!(
            "task '{package}' does not belong to this project folder; marked it as dropped, its server was left alone"
        ));
    }
}

/// True when a run file exists for `run_id` but is unreadable: a live container
/// may then belong to a record we cannot see, so it must not be removed as "ownerless".
fn has_unreadable_record(store: &Store, run_id: &str) -> bool {
    !run_id.is_empty()
        && !run_id.contains(['/', '\\'])
        && store.has_file(run_id)
        && store.load(run_id).is_err()
}

struct Sweeper<'a> {
    store: &'a Store,
    docker: &'a dyn Docker,
    tag: &'a str,
    containers: &'a [Found],
    alive: &'a dyn Fn(u32) -> bool,
    today: &'a str,
    ask: Option<&'a dyn Fn(&str) -> bool>,
}

/// Apply the plan. `ask` is `None` when there is no terminal: stale tasks are then left alone.
pub(crate) fn execute(
    store: &Store,
    docker: &dyn Docker,
    tag: &str,
    containers: &[Found],
    alive: &dyn Fn(u32) -> bool,
    today: &str,
    ask: Option<&dyn Fn(&str) -> bool>,
) {
    let s = Sweeper {
        store,
        docker,
        tag,
        containers,
        alive,
        today,
        ask,
    };
    execute_on(&store.all(), &s);
}

/// Moves a finished or dropped folder run that a failed or crashed `clean` left in
/// place to `done/`. Never deletes: it is the learner's work. A server run with a
/// workspace folder is the same: its server goes first, then the folder moves.
fn retry_folder_clean(records: &[RunRecord], env: &mut Router) {
    for r in records
        .iter()
        .filter(|r| matches!(r.state, RunState::Finished | RunState::Dropped))
    {
        let h = r.handle();
        let left = match r.env.as_str() {
            FOLDER => env
                .folder
                .dir(&h)
                .is_ok_and(|d| d.symlink_metadata().is_ok()),
            CONTAINER => env.container.folder_of(&h).is_some(),
            _ => false,
        };
        if left {
            let _ = env.clean(&h);
        }
    }
}

fn execute_on(records: &[RunRecord], s: &Sweeper) {
    let Sweeper {
        store,
        docker,
        tag,
        containers,
        alive,
        today,
        ask,
    } = *s;
    let mine = prefix(tag);
    let mut env = Router::new(
        docker,
        tag,
        store.tasks_dir(),
        &crate::scenario::model::NoModel,
        &[],
    );
    retry_folder_clean(records, &mut env);
    for a in plan(records, containers, alive, today, tag) {
        match a {
            Action::RemoveContainer(name) => {
                let run_id = containers
                    .iter()
                    .find(|c| c.name == name)
                    .map(|c| c.run_id.as_str())
                    .unwrap_or("");
                if !name.starts_with(&mine) {
                    continue; // names come from list_labeled, but never trust a stray one
                }
                if has_unreadable_record(store, run_id) {
                    // Keep the work, but a running one must not stay running.
                    let running = containers
                        .iter()
                        .any(|c| c.name == name && c.state == "running");
                    let stopped = running
                        && docker
                            .run(&["stop", "-t", "2", &name])
                            .map(|o| o.ok())
                            .unwrap_or(false);
                    ui::warn(if stopped {
                        "a practice task has an unreadable record; stopped it, kept its work"
                    } else {
                        "a practice task has an unreadable record; left it untouched"
                    });
                    continue;
                }
                let _ = docker.run(&["rm", "-fv", &name]);
            }
            Action::StopContainer(name) => {
                if !name.starts_with(&mine) {
                    continue;
                }
                let stopped = docker
                    .run(&["stop", "-t", "2", &name])
                    .is_ok_and(|o| o.ok());
                if !stopped {
                    ui::warn("a paused practice task is still running; could not stop it");
                }
            }
            Action::DeleteRecord(id) => {
                // A folder prepare that never returned: the learner never had its folder,
                // so it holds no work. Only a valid id of a folder run reaches the disk.
                let folder_run = records
                    .iter()
                    .any(|r| r.run_id == id && (r.env == FOLDER || r.env == CONTAINER));
                if folder_run && valid_run_id(&id) {
                    let _ = std::fs::remove_dir_all(store.tasks_dir().join(&id));
                }
                let _ = store.remove(&id);
            }
            Action::PauseRecord { run_id, container } => {
                let Ok(mut rec) = store.load(&run_id) else {
                    continue;
                };
                let t = Tick {
                    pid: std::process::id(),
                    alive,
                    stamp: "",
                    today,
                };
                let result = if containers.iter().any(|c| c.name == container) {
                    engine::pause(&mut env, store, &mut rec, &t)
                } else {
                    // The container is gone: nothing to stop. Park the record so it cannot block a new start.
                    park(store, &mut rec, today)
                };
                match result {
                    Ok(()) => ui::notice(&format!(
                        "task '{}' was left open; it is paused",
                        rec.package
                    )),
                    Err(_) => ui::warn(&format!("could not pause task '{}'", rec.package)),
                }
            }
            Action::DropForeign { run_id, package } => {
                mark_foreign(store, &run_id, &package, today)
            }
            Action::AskStale {
                run_id,
                package,
                days,
            } => {
                let Some(ask) = ask else {
                    continue; // never decide silently
                };
                let prompt = format!(
                    "task '{package}' has been untouched for {days} days. Remove it? [y/N] "
                );
                let Ok(mut rec) = store.load(&run_id) else {
                    continue;
                };
                if ask(&prompt) {
                    if env.clean(&rec.handle()).is_err() {
                        ui::warn(&format!("could not remove task '{package}'"));
                        continue;
                    }
                    rec.state = RunState::Dropped;
                    rec.owner_pid = 0;
                } // "no": ask again only after another STALE_DAYS
                rec.updated = today.to_string();
                if store.save(&rec).is_err() {
                    ui::warn(&format!("could not update the record of task '{package}'"));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::docker::fake::{fail, ok, FakeDocker};
    use crate::scenario::package::tests::tmp;

    const TAG: &str = "aaaa1111";

    fn rec(id: &str, state: RunState, owner: u32, updated: &str) -> RunRecord {
        RunRecord {
            run_id: id.into(),
            package: "demo".into(),
            topic: "linux".into(),
            tone: "plain".into(),
            state,
            owner_pid: owner,
            started: "2026-10-01".into(),
            updated: updated.into(),
            reference: format!("usta-aaaa1111-{id}"),
            access: String::new(),
            env: "container".into(),
            last_check: None,
        }
    }

    fn found(id: &str, state: &str) -> Found {
        Found {
            name: format!("usta-aaaa1111-{id}"),
            state: state.into(),
            run_id: id.into(),
        }
    }

    const DEAD: &dyn Fn(u32) -> bool = &|_| false;
    const TODAY: &str = "2026-11-04";

    fn p(recs: &[RunRecord], cs: &[Found], alive: &dyn Fn(u32) -> bool) -> Vec<Action> {
        plan(recs, cs, alive, TODAY, TAG)
    }

    #[test]
    fn unfinished_tasks_survive_startup_cleanup() {
        let recs = vec![rec("p", RunState::Paused, 0, "2026-11-01")];
        assert_eq!(p(&recs, &[found("p", "exited")], DEAD), vec![]);
    }

    #[test]
    fn a_container_without_a_record_is_removed() {
        assert_eq!(
            p(&[], &[found("ghost", "exited")], DEAD),
            vec![Action::RemoveContainer("usta-aaaa1111-ghost".into())]
        );
    }

    #[test]
    fn a_finished_leftover_is_removed_and_its_record_kept() {
        let recs = vec![rec("f", RunState::Finished, 0, "2026-11-03")];
        assert_eq!(
            p(&recs, &[found("f", "exited")], DEAD),
            vec![Action::RemoveContainer("usta-aaaa1111-f".into())]
        );
        assert_eq!(p(&recs, &[], DEAD), vec![]);
    }

    #[test]
    fn a_dropped_leftover_is_removed_and_its_record_kept() {
        let recs = vec![rec("d", RunState::Dropped, 0, "2026-11-03")];
        assert_eq!(
            p(&recs, &[found("d", "exited")], DEAD),
            vec![Action::RemoveContainer("usta-aaaa1111-d".into())]
        );
        assert_eq!(p(&recs, &[], DEAD), vec![]);
    }

    #[test]
    fn a_crash_leaving_a_container_running_pauses_it() {
        let recs = vec![rec("c", RunState::Active, 4242, "2026-11-04")];
        assert_eq!(
            p(&recs, &[found("c", "running")], DEAD),
            vec![Action::PauseRecord {
                run_id: "c".into(),
                container: "usta-aaaa1111-c".into()
            }]
        );
    }

    #[test]
    fn a_live_owner_is_left_alone() {
        let recs = vec![rec("c", RunState::Active, 4242, "2026-11-04")];
        let alive: &dyn Fn(u32) -> bool = &|p| p == 4242;
        assert_eq!(p(&recs, &[found("c", "running")], alive), vec![]);
    }

    #[test]
    fn an_interrupted_prepare_is_deleted_with_its_container() {
        let recs = vec![rec("x", RunState::Preparing, 4242, "2026-11-04")];
        assert_eq!(
            p(&recs, &[found("x", "created")], DEAD),
            vec![
                Action::RemoveContainer("usta-aaaa1111-x".into()),
                Action::DeleteRecord("x".into())
            ]
        );
        assert_eq!(p(&recs, &[], DEAD), vec![Action::DeleteRecord("x".into())]);
    }

    #[test]
    fn an_interrupted_prepare_with_a_running_container_stops_it() {
        // A crash after `docker start` but before the record turned active.
        let recs = vec![rec("x", RunState::Preparing, 4242, "2026-11-04")];
        assert_eq!(
            p(&recs, &[found("x", "running")], DEAD),
            vec![
                Action::RemoveContainer("usta-aaaa1111-x".into()),
                Action::DeleteRecord("x".into())
            ]
        );
        // No record at all: a running ghost is removed too.
        assert_eq!(
            p(&[], &[found("y", "running")], DEAD),
            vec![Action::RemoveContainer("usta-aaaa1111-y".into())]
        );
    }

    #[test]
    fn a_long_untouched_paused_task_asks_never_deletes() {
        let recs = vec![rec("old", RunState::Paused, 0, "2026-10-01")];
        let acts = p(&recs, &[found("old", "exited")], DEAD);
        assert_eq!(
            acts,
            vec![Action::AskStale {
                run_id: "old".into(),
                package: "demo".into(),
                days: 34
            }]
        );
        let fresh = vec![rec("n", RunState::Paused, 0, "2026-10-22")];
        assert_eq!(p(&fresh, &[], DEAD), vec![]);
    }

    #[test]
    fn a_bad_date_never_asks() {
        let recs = vec![rec("d", RunState::Paused, 0, "not-a-date")];
        assert_eq!(p(&recs, &[], DEAD), vec![]);
    }

    #[test]
    fn a_record_outside_the_prefix_is_dropped_not_deleted() {
        for state in [RunState::Active, RunState::Paused, RunState::Preparing] {
            let mut r = rec("m", state, 0, "2026-11-04");
            r.reference = "usta-bbbb2222-m".into();
            assert_eq!(
                p(&[r], &[], DEAD),
                vec![Action::DropForeign {
                    run_id: "m".into(),
                    package: "demo".into()
                }]
            );
        }
        let mut done = rec("m", RunState::Finished, 0, "2026-11-04");
        done.reference = "usta-bbbb2222-m".into();
        assert_eq!(p(&[done], &[], DEAD), vec![]);
    }

    fn fake() -> FakeDocker {
        FakeDocker::new(|_| ok(""))
    }

    #[test]
    fn a_foreign_record_stops_blocking_a_new_start() {
        let root = tmp("sweep_foreign");
        let store = Store::new(&root);
        let mut r = rec("m", RunState::Active, 999_999, "2026-11-04");
        r.reference = "usta-bbbb2222-m".into();
        store.save(&r).unwrap();
        let d = fake();
        execute(&store, &d, TAG, &[], &|pid| pid == 999_999, TODAY, None);
        let after = store.load("m").unwrap();
        assert_eq!(after.state, RunState::Dropped);
        assert_eq!(after.owner_pid, 0);
        assert!(d.calls().is_empty(), "no docker call for a foreign record");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn crash_recovery_stops_the_container_and_parks_the_record() {
        let root = tmp("sweep_crash");
        let store = Store::new(&root);
        store
            .save(&rec("c", RunState::Active, 4242, "2026-11-03"))
            .unwrap();
        let d = fake();
        execute(&store, &d, TAG, &[found("c", "running")], DEAD, TODAY, None);
        assert_eq!(store.load("c").unwrap().state, RunState::Paused);
        assert!(d
            .calls()
            .iter()
            .any(|c| c.starts_with("stop") && c.contains("usta-aaaa1111-c")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn crash_recovery_with_the_container_gone_still_parks_the_record() {
        let root = tmp("sweep_crash_gone");
        let store = Store::new(&root);
        store
            .save(&rec("c", RunState::Active, 4242, "2026-11-03"))
            .unwrap();
        let d = fake();
        execute(&store, &d, TAG, &[], DEAD, TODAY, None);
        assert_eq!(store.load("c").unwrap().state, RunState::Paused);
        assert!(d.calls().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_interrupted_prepare_removes_the_container_and_the_record() {
        let root = tmp("sweep_prep");
        let store = Store::new(&root);
        store
            .save(&rec("x", RunState::Preparing, 4242, "2026-11-04"))
            .unwrap();
        let d = fake();
        execute(&store, &d, TAG, &[found("x", "running")], DEAD, TODAY, None);
        assert!(store.load("x").is_err());
        assert!(d.calls().contains(&"rm -fv usta-aaaa1111-x".to_string()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_running_container_behind_a_corrupt_record_is_stopped_never_removed() {
        let root = tmp("sweep_corrupt");
        let store = Store::new(&root);
        let runs = root.join(".usta/scenarios/runs");
        std::fs::create_dir_all(&runs).unwrap();
        std::fs::write(runs.join("z.toml"), "not = [valid").unwrap();
        let d = fake();
        execute(&store, &d, TAG, &[found("z", "running")], DEAD, TODAY, None);
        assert_eq!(d.calls(), vec!["stop -t 2 usta-aaaa1111-z".to_string()]);
        // A stopped one gets no call at all.
        let d2 = fake();
        execute(&store, &d2, TAG, &[found("z", "exited")], DEAD, TODAY, None);
        assert!(d2.calls().is_empty());
        // A truly ownerless container is still removed.
        execute(
            &store,
            &d2,
            TAG,
            &[found("ghost", "running")],
            DEAD,
            TODAY,
            None,
        );
        assert_eq!(d2.calls(), vec!["rm -fv usta-aaaa1111-ghost".to_string()]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_in_session_sweep_never_prompts_for_a_stale_task() {
        let root = tmp("sweep_no_prompt");
        let store = Store::new(&root);
        let tag = project_tag(&root);
        let mut r = rec("old", RunState::Paused, 0, "2026-10-01");
        r.reference = format!("usta-{tag}-old");
        store.save(&r).unwrap();
        let d = fake();
        sweep_with(&root, Path::new("/nonexistent-g"), TODAY, &d, DEAD, None);
        assert_eq!(store.load("old").unwrap().state, RunState::Paused);
        assert!(
            d.calls().iter().all(|c| c.starts_with("ps")),
            "{:?}",
            d.calls()
        );
        // Contrast: with an asker the same record does prompt exactly once.
        let asked = std::cell::Cell::new(0);
        let counting = |_: &str| {
            asked.set(asked.get() + 1);
            false
        };
        sweep_with(
            &root,
            Path::new("/nonexistent-g"),
            TODAY,
            &d,
            DEAD,
            Some(&counting),
        );
        assert_eq!(asked.get(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn foreign_records_are_dropped_with_no_docker_at_all() {
        let root = tmp("sweep_foreign_nodocker");
        let store = Store::new(&root);
        let tag = project_tag(&root); // read first: the project's identity predates the stray record
        let mut r = rec("m", RunState::Active, 999_999, "2026-11-04");
        r.reference = "usta-bbbb2222-m".into();
        store.save(&r).unwrap();
        let mut own = rec("own", RunState::Paused, 0, "2026-11-04");
        own.reference = format!("usta-{tag}-own");
        store.save(&own).unwrap();
        drop_foreign(&root, TODAY);
        assert_eq!(store.load("m").unwrap().state, RunState::Dropped);
        assert_eq!(store.load("own").unwrap().state, RunState::Paused);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_failing_clean_after_yes_keeps_the_record_paused() {
        let root = tmp("sweep_clean_fails");
        let store = Store::new(&root);
        store
            .save(&rec("old", RunState::Paused, 0, "2026-10-01"))
            .unwrap();
        let d = FakeDocker::new(|l| {
            if l.starts_with("rm") {
                fail(1, "boom")
            } else {
                ok("")
            }
        });
        execute(
            &store,
            &d,
            TAG,
            &[found("old", "exited")],
            DEAD,
            TODAY,
            Some(&|_| true),
        );
        let after = store.load("old").unwrap();
        assert_eq!(
            (after.state, after.updated.as_str()),
            (RunState::Paused, "2026-10-01")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_stale_task_is_removed_only_after_a_yes_through_the_guarded_clean() {
        let root = tmp("sweep_stale_yes");
        let store = Store::new(&root);
        store
            .save(&rec("old", RunState::Paused, 0, "2026-10-01"))
            .unwrap();
        let d = fake();
        execute(
            &store,
            &d,
            TAG,
            &[found("old", "exited")],
            DEAD,
            TODAY,
            None,
        );
        assert_eq!(
            store.load("old").unwrap().state,
            RunState::Paused,
            "no terminal: untouched"
        );
        assert!(d.calls().is_empty());
        execute(
            &store,
            &d,
            TAG,
            &[found("old", "exited")],
            DEAD,
            TODAY,
            Some(&|_| false),
        );
        let kept = store.load("old").unwrap();
        assert_eq!(
            (kept.state, kept.updated.as_str()),
            (RunState::Paused, TODAY)
        );
        assert!(d.calls().is_empty());
        store
            .save(&rec("old", RunState::Paused, 0, "2026-10-01"))
            .unwrap();
        execute(
            &store,
            &d,
            TAG,
            &[found("old", "exited")],
            DEAD,
            TODAY,
            Some(&|_| true),
        );
        assert_eq!(store.load("old").unwrap().state, RunState::Dropped);
        assert!(d.calls().contains(&"rm -f usta-aaaa1111-old".to_string()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_left_server_of_a_finished_generated_task_leaves_its_image_to_the_idle_sweep() {
        let root = tmp("sweep_left_image");
        let store = Store::new(&root);
        store
            .save(&rec("done", RunState::Finished, 0, "2026-11-01"))
            .unwrap();
        store
            .save(&rec("open", RunState::Paused, 0, "2026-11-03"))
            .unwrap();
        let d = FakeDocker::new(|l| {
            if l.starts_with("inspect -f {{.Config.Image}}") {
                ok("usta-scn-gen-x-1:v1\n")
            } else {
                ok("")
            }
        });
        let cs = [found("done", "exited"), found("open", "exited")];
        execute(&store, &d, TAG, &cs, DEAD, TODAY, None);
        let rmis: Vec<_> = d
            .calls()
            .into_iter()
            .filter(|c| c.starts_with("rmi"))
            .collect();
        assert!(rmis.is_empty(), "{:?}", d.calls());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_tampered_reference_is_dropped_without_any_docker_call() {
        let root = tmp("sweep_stale_tamper");
        let store = Store::new(&root);
        let mut r = rec("old", RunState::Paused, 0, "2026-10-01");
        r.reference = "usta-aaaa1111-old".into();
        store.save(&r).unwrap();
        // Hand-edit the file so the reference is another container's name.
        let path = root.join(".usta/scenarios/runs/old.toml");
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("usta-aaaa1111-old", "kiln-db");
        std::fs::write(&path, text).unwrap();
        let d = fake();
        execute(&store, &d, TAG, &[], DEAD, TODAY, Some(&|_| true));
        // Foreign reference: dropped by the foreign-record rule, docker untouched.
        assert!(d.calls().is_empty());
        assert_eq!(store.load("old").unwrap().state, RunState::Dropped);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_record_outside_the_prefix_never_removes_a_container() {
        for state in [
            RunState::Preparing,
            RunState::Active,
            RunState::Paused,
            RunState::Finished,
            RunState::Dropped,
        ] {
            let mut r = rec("m", state, 0, "2026-11-04");
            r.reference = "usta-bbbb2222-m".into();
            for c in ["running", "exited"] {
                let acts = p(std::slice::from_ref(&r), &[found("m", c)], DEAD);
                assert!(
                    !acts.iter().any(|a| matches!(a, Action::RemoveContainer(_))),
                    "{state:?}/{c}: {acts:?}"
                );
            }
        }
    }

    #[test]
    fn a_foreign_record_keeps_its_container_across_two_sweeps() {
        let root = tmp("sweep_foreign_twice");
        let store = Store::new(&root);
        let mut r = rec("m", RunState::Paused, 0, "2026-11-04");
        r.reference = "usta-bbbb2222-m".into();
        store.save(&r).unwrap();
        let d = fake();
        let cs = [found("m", "exited")];
        execute(&store, &d, TAG, &cs, DEAD, TODAY, None);
        assert_eq!(store.load("m").unwrap().state, RunState::Dropped);
        // The first sweep marked it dropped; the second must not read that as a leftover.
        execute(&store, &d, TAG, &cs, DEAD, TODAY, None);
        assert!(d.calls().is_empty(), "{:?}", d.calls());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_moved_project_keeps_its_paused_task_and_its_container() {
        let base = tmp("sweep_move_keeps");
        let (old, new) = (base.join("old"), base.join("new"));
        std::fs::create_dir_all(&old).unwrap();
        let tag = project_tag(&old);
        let mut r = rec("t", RunState::Paused, 0, "2026-11-04");
        r.reference = format!("usta-{tag}-t");
        Store::new(&old).save(&r).unwrap();
        std::fs::rename(&old, &new).unwrap();
        drop_foreign(&new, TODAY);
        let name = r.reference.clone();
        let listing = format!("{name}|exited|t\n");
        let d = FakeDocker::new(move |l| {
            if l.starts_with("ps ") {
                ok(&listing)
            } else {
                ok("")
            }
        });
        sweep_with(&new, Path::new("/nonexistent-g"), TODAY, &d, DEAD, None);
        assert_eq!(Store::new(&new).load("t").unwrap().state, RunState::Paused);
        let filter = format!("label=usta.project={tag}");
        assert!(d.calls()[0].contains(&filter), "{:?}", d.calls());
        assert_eq!(d.calls().len(), 1, "nothing was removed or stopped");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_paused_record_with_a_running_container_and_no_live_owner_is_stopped() {
        let recs = vec![rec("p", RunState::Paused, 0, "2026-11-03")];
        assert_eq!(
            p(&recs, &[found("p", "running")], DEAD),
            vec![Action::StopContainer("usta-aaaa1111-p".into())]
        );
        let held = vec![rec("p", RunState::Paused, 4242, "2026-11-03")];
        let alive: &dyn Fn(u32) -> bool = &|p| p == 4242;
        assert_eq!(p(&held, &[found("p", "running")], alive), vec![]);
        assert_eq!(p(&recs, &[found("p", "exited")], DEAD), vec![]);
    }

    #[test]
    fn stopping_a_paused_task_keeps_the_container_and_the_record() {
        let root = tmp("sweep_stop_paused");
        let store = Store::new(&root);
        let r = rec("p", RunState::Paused, 0, "2026-11-03");
        store.save(&r).unwrap();
        let d = fake();
        execute(&store, &d, TAG, &[found("p", "running")], DEAD, TODAY, None);
        assert_eq!(d.calls(), vec!["stop -t 2 usta-aaaa1111-p"]);
        assert_eq!(store.load("p").unwrap(), r);
        let _ = std::fs::remove_dir_all(root);
    }

    // ---- Task 13: the sweep without a runtime -------------------------------------

    fn project_with_tag(name: &str, tag: &str) -> (std::path::PathBuf, Store) {
        let root = tmp(name);
        let here = std::fs::canonicalize(&root).unwrap();
        let file = root.join(".usta/scenarios/tag");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, format!("{tag}\n{}\n", here.display())).unwrap();
        assert_eq!(project_tag(&root), tag);
        let store = Store::new(&root);
        (root, store)
    }

    fn folder_rec(id: &str, state: RunState, owner: u32, updated: &str) -> RunRecord {
        let mut r = rec(id, state, owner, updated);
        r.env = "folder".into();
        r
    }

    fn missing() -> FakeDocker {
        FakeDocker::missing()
    }

    #[test]
    fn with_no_runtime_a_dead_folder_run_and_a_stale_container_run_are_parked() {
        let (p, store) = project_with_tag("sw_records", TAG);
        store
            .save(&folder_rec("f", RunState::Active, 999_999, TODAY))
            .unwrap();
        let mut c = rec("c", RunState::Active, 999_999, TODAY);
        c.env = "container".into();
        store.save(&c).unwrap();
        let d = missing();
        sweep_records(&p, TODAY, &d, DEAD, None);
        assert_eq!(store.load("f").unwrap().state, RunState::Paused);
        let c = store.load("c").unwrap();
        assert_eq!((c.state, c.owner_pid), (RunState::Paused, 0));
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn a_stopped_runtime_parks_a_dead_owner_container_run_and_a_live_owner_is_kept() {
        let (p, store) = project_with_tag("sw_records_stopped", TAG);
        store
            .save(&rec("dead", RunState::Active, 999_999, TODAY))
            .unwrap();
        store
            .save(&rec("live", RunState::Active, 4242, TODAY))
            .unwrap();
        let stopped = FakeDocker::new(|_| fail(1, "Cannot connect to the Docker daemon"));
        sweep_records(&p, TODAY, &stopped, &|pid| pid == 4242, None);
        assert_eq!(store.load("dead").unwrap().state, RunState::Paused);
        assert_eq!(store.load("live").unwrap().state, RunState::Active);
        // Not installed is the same: only a dead owner is parked.
        sweep_records(&p, TODAY, &missing(), &|pid| pid == 4242, None);
        assert_eq!(store.load("live").unwrap().state, RunState::Active);
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn with_a_runtime_present_the_record_pass_leaves_a_container_run_to_the_docker_sweep() {
        let (p, store) = project_with_tag("sw_records_up", TAG);
        store
            .save(&folder_rec("f", RunState::Active, 999_999, TODAY))
            .unwrap();
        store
            .save(&rec("c", RunState::Active, 999_999, TODAY))
            .unwrap();
        let d = fake();
        sweep_records(&p, TODAY, &d, DEAD, None);
        assert_eq!(store.load("f").unwrap().state, RunState::Paused);
        assert_eq!(store.load("c").unwrap().state, RunState::Active);
        assert!(
            d.calls().iter().all(|c| c.starts_with("version")),
            "{:?}",
            d.calls()
        );
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn a_live_folder_owner_is_not_parked_and_a_record_pass_on_no_project_does_nothing() {
        let (p, store) = project_with_tag("sw_live", TAG);
        store
            .save(&folder_rec("f", RunState::Active, 4242, TODAY))
            .unwrap();
        sweep_records(&p, TODAY, &missing(), &|pid| pid == 4242, None);
        assert_eq!(store.load("f").unwrap().state, RunState::Active);
        let none = tmp("sw_none");
        sweep_records(&none, TODAY, &missing(), DEAD, None);
        assert!(!none.join(".usta").exists(), "nothing is created");
        let _ = std::fs::remove_dir_all(p);
        let _ = std::fs::remove_dir_all(none);
    }

    #[test]
    fn an_interrupted_folder_prepare_leaves_no_folder_behind() {
        let (p, store) = project_with_tag("sw_prep", TAG);
        store
            .save(&folder_rec("f", RunState::Preparing, 999_999, TODAY))
            .unwrap();
        std::fs::create_dir_all(store.tasks_dir().join("f")).unwrap();
        std::fs::write(store.tasks_dir().join("f/half.md"), "x").unwrap();
        sweep_records(&p, TODAY, &missing(), DEAD, None);
        assert!(!store.tasks_dir().join("f").exists());
        assert!(!store.has_file("f"));
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn a_prepare_record_with_an_odd_id_never_deletes_anything_outside_its_folder() {
        let (p, store) = project_with_tag("sw_prep_odd", TAG);
        std::fs::write(p.join("keep.txt"), "mine").unwrap();
        std::fs::create_dir_all(store.tasks_dir()).unwrap();
        store
            .save(&folder_rec("..", RunState::Preparing, 999_999, TODAY))
            .unwrap();
        sweep_records(&p, TODAY, &missing(), DEAD, None);
        assert!(p.join("keep.txt").is_file());
        assert!(store.tasks_dir().is_dir());
        // A container run with a workspace owns its folder like a folder run does.
        store
            .save(&rec("c", RunState::Preparing, 999_999, TODAY))
            .unwrap();
        std::fs::create_dir_all(store.tasks_dir().join("c")).unwrap();
        let d = fake();
        execute(&store, &d, TAG, &[], DEAD, TODAY, None);
        assert!(!store.tasks_dir().join("c").exists());
        assert!(store.tasks_dir().is_dir());
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn the_real_sweep_keeps_a_folder_run_and_asks_before_removing_a_stale_one() {
        let (p, store) = project_with_tag("sw_stale", TAG);
        store
            .save(&folder_rec("f", RunState::Paused, 0, "2026-01-01"))
            .unwrap();
        std::fs::create_dir_all(store.tasks_dir().join("f")).unwrap();
        std::fs::write(store.tasks_dir().join("f/plan.md"), "my work").unwrap();
        sweep_records(&p, TODAY, &missing(), DEAD, None);
        assert!(
            store.tasks_dir().join("f").is_dir(),
            "never decided silently"
        );
        sweep_records(&p, TODAY, &missing(), DEAD, Some(&|_| true));
        assert!(!store.tasks_dir().join("f").exists());
        assert_eq!(
            std::fs::read_to_string(store.tasks_dir().join("done/f/plan.md")).unwrap(),
            "my work",
            "a stale task's work is kept too"
        );
        assert_eq!(store.load("f").unwrap().state, RunState::Dropped);
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn a_full_sweep_treats_folder_records_like_any_other_and_never_marks_them_foreign() {
        let (p, store) = project_with_tag("sw_foreign", TAG);
        store
            .save(&folder_rec("f", RunState::Paused, 0, TODAY))
            .unwrap();
        let d = fake();
        execute(&store, &d, TAG, &[], DEAD, TODAY, None);
        assert_eq!(store.load("f").unwrap().state, RunState::Paused);
        assert!(d.calls().is_empty());
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn a_finished_or_dropped_folder_run_still_in_place_moves_to_done_on_the_next_sweep() {
        let (p, store) = project_with_tag("sw_retry_clean", TAG);
        for (id, state) in [("a", RunState::Finished), ("b", RunState::Dropped)] {
            store.save(&folder_rec(id, state, 0, TODAY)).unwrap();
            std::fs::create_dir_all(store.tasks_dir().join(id)).unwrap();
            std::fs::write(store.tasks_dir().join(id).join("plan.md"), id).unwrap();
        }
        // Earlier work already sits at done/a: nothing is overwritten.
        std::fs::create_dir_all(store.tasks_dir().join("done/a")).unwrap();
        std::fs::write(store.tasks_dir().join("done/a/plan.md"), "older").unwrap();
        // A paused run is the learner's live work.
        store
            .save(&folder_rec("c", RunState::Paused, 0, TODAY))
            .unwrap();
        std::fs::create_dir_all(store.tasks_dir().join("c")).unwrap();
        let d = missing();
        sweep_records(&p, TODAY, &d, DEAD, None);
        let t = store.tasks_dir();
        assert!(!t.join("a").exists() && !t.join("b").exists());
        assert_eq!(
            std::fs::read_to_string(t.join("done/b/plan.md")).unwrap(),
            "b"
        );
        assert_eq!(
            std::fs::read_to_string(t.join("done/a/plan.md")).unwrap(),
            "older"
        );
        assert_eq!(
            std::fs::read_to_string(t.join("done/a-2/plan.md")).unwrap(),
            "a"
        );
        assert!(t.join("c").is_dir());
        // Nothing left to move: a second sweep changes nothing.
        sweep_records(&p, TODAY, &d, DEAD, None);
        assert!(t.join("done/a-2").is_dir() && !t.join("done/a-3").exists());
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn a_finished_server_run_with_a_workspace_folder_is_removed_then_moved_to_done() {
        let (p, store) = project_with_tag("sw_retry_ws", TAG);
        for (id, state) in [
            ("a", RunState::Finished),
            ("b", RunState::Dropped),
            ("c", RunState::Paused),
        ] {
            let mut r = rec(id, state, 0, TODAY);
            r.reference = format!("usta-{TAG}-{id}");
            store.save(&r).unwrap();
            std::fs::create_dir_all(store.tasks_dir().join(id)).unwrap();
            std::fs::write(store.tasks_dir().join(id).join("main.rs"), id).unwrap();
        }
        // A finished server run with no folder costs no extra call.
        let mut r = rec("n", RunState::Finished, 0, TODAY);
        r.reference = format!("usta-{TAG}-n");
        store.save(&r).unwrap();
        let d = fake();
        execute(&store, &d, TAG, &[], DEAD, TODAY, None);
        let t = store.tasks_dir();
        assert!(t.join("done/a/main.rs").is_file() && t.join("done/b/main.rs").is_file());
        assert!(t.join("c").is_dir(), "a paused run is live work");
        let rms: Vec<String> = d
            .calls()
            .into_iter()
            .filter(|c| c.starts_with("rm "))
            .collect();
        assert_eq!(rms.len(), 2, "{rms:?}");
        assert!(
            rms.iter().all(|c| c.ends_with("-a") || c.ends_with("-b")),
            "{rms:?}"
        );
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn a_foreign_dropped_folder_record_never_moves_a_folder() {
        let (p, store) = project_with_tag("sw_retry_foreign", TAG);
        let mut r = folder_rec("x", RunState::Dropped, 0, TODAY);
        r.reference = "usta-bbbb2222-x".into();
        store.save(&r).unwrap();
        std::fs::create_dir_all(store.tasks_dir().join("x")).unwrap();
        sweep_records(&p, TODAY, &missing(), DEAD, None);
        assert!(store.tasks_dir().join("x").is_dir());
        let _ = std::fs::remove_dir_all(p);
    }

    fn helper(kind: &str, id: &str) -> Found {
        Found {
            name: format!("usta-aaaa1111-{kind}-{id}"),
            state: "running".into(),
            run_id: format!("{kind}-{id}"),
        }
    }

    #[test]
    fn a_leftover_helper_of_a_check_or_build_has_no_record_and_is_removed() {
        let acts = p(&[], &[helper("chk", "r1")], DEAD);
        assert_eq!(
            acts,
            vec![Action::RemoveContainer("usta-aaaa1111-chk-r1".into())]
        );
        let acts = p(&[], &[helper("bld", "gen-x-1")], DEAD);
        assert_eq!(
            acts,
            vec![Action::RemoveContainer("usta-aaaa1111-bld-gen-x-1".into())]
        );
    }

    #[test]
    fn a_helper_of_a_live_session_is_left_alone_and_one_of_a_dead_session_is_removed() {
        let live: &dyn Fn(u32) -> bool = &|p| p == 4242;
        let check_run = rec("r1", RunState::Active, 4242, TODAY);
        assert_eq!(
            p(
                std::slice::from_ref(&check_run),
                &[helper("chk", "r1")],
                live
            ),
            vec![]
        );
        let dead_acts = p(&[check_run], &[helper("chk", "r1")], DEAD);
        assert!(dead_acts.contains(&Action::RemoveContainer("usta-aaaa1111-chk-r1".into())));
        // A build belongs to the proof record of its package, `val-<package>-rN`.
        let mut proof = rec("val-gen-x-1-r1", RunState::Preparing, 4242, TODAY);
        proof.package = "gen-x-1".into();
        assert_eq!(
            p(&[proof.clone()], &[helper("bld", "gen-x-1")], live),
            vec![]
        );
        // The same helper of ANOTHER package is nobody's.
        assert_eq!(
            p(&[proof], &[helper("bld", "gen-y-1")], live),
            vec![Action::RemoveContainer("usta-aaaa1111-bld-gen-y-1".into())]
        );
        // A helper whose run record belongs to nobody alive but is itself paused (owner 0).
        let parked = rec("r2", RunState::Paused, 0, TODAY);
        assert_eq!(
            p(&[parked], &[helper("chk", "r2")], live),
            vec![Action::RemoveContainer("usta-aaaa1111-chk-r2".into())]
        );
    }
}
