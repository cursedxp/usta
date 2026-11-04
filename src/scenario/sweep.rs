//! Startup cleanup. Touches only containers carrying `usta=1` AND this
//! project's tag. Pure `plan` decides; `execute` acts. An unfinished task is
//! never removed without asking.

use std::path::Path;

use chrono::NaiveDate;

use crate::scenario::container::{list_labeled, ContainerEnv, Found};
use crate::scenario::docker::{available, Docker, RealDocker};
use crate::scenario::engine::{self, Tick};
use crate::scenario::env::Environment;
use crate::scenario::store::{project_tag, RunRecord, RunState, Store};
use crate::ui;

/// Provisional: the spec lists the idle age as an open measurement.
pub(crate) const STALE_DAYS: i64 = 14;

#[derive(Debug, PartialEq)]
pub(crate) enum Action {
    RemoveContainer(String),
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
    /// The record points outside this project's container namespace (the
    /// project folder moved). It can never be acted on, so it must not block
    /// a new start; it is marked dropped, never deleted.
    DropForeign {
        run_id: String,
        package: String,
    },
}

fn age_days(updated: &str, today: &str) -> Option<i64> {
    let u = NaiveDate::parse_from_str(updated, "%Y-%m-%d").ok()?;
    let t = NaiveDate::parse_from_str(today, "%Y-%m-%d").ok()?;
    Some((t - u).num_days())
}

fn prefix(tag: &str) -> String {
    format!("usta-{tag}-")
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
    for c in containers {
        match records.iter().find(|r| r.run_id == c.run_id) {
            None => acts.push(Action::RemoveContainer(c.name.clone())),
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
        if unfinished && !r.reference.is_empty() && !r.reference.starts_with(&mine) {
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
    docker_sweep(project_root, today, asker);
}

/// Record-only sweep, no Docker: unfinished records from a moved project are
/// marked dropped so they can never block a start.
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

/// Docker-side sweep for use INSIDE a session (call right after `ensure_ready`
/// in the start and resume paths). It never asks and never reads stdin: a stale
/// task is skipped and asked about at the next launch. It parks an own `Active`
/// record whose owner is dead, even when the container is gone, so that record
/// cannot block starting. Failures are warnings, never fatal.
pub(crate) fn sweep_docker(project_root: &Path, today: &str) {
    docker_sweep(project_root, today, None);
}

fn docker_sweep(project_root: &Path, today: &str, ask: Option<&dyn Fn(&str) -> bool>) {
    let docker = RealDocker;
    if !Store::new(project_root).exists() || available(&docker).is_err() {
        return;
    }
    sweep_with(project_root, today, &docker, &crate::lock::pid_alive, ask);
}

fn sweep_with(
    project_root: &Path,
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
    execute(&store, docker, &tag, &containers, alive, today, ask);
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
            "task '{package}' belongs to a project folder that moved; marked it as dropped"
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
    let records = store.all();
    let mine = prefix(tag);
    let mut env = ContainerEnv {
        docker,
        project_tag: tag.to_string(),
    };
    for a in plan(&records, containers, alive, today, tag) {
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
                let _ = docker.run(&["rm", "-f", &name]);
            }
            Action::DeleteRecord(id) => {
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
                    rec.state = RunState::Paused;
                    rec.owner_pid = 0;
                    rec.updated = today.to_string();
                    store.save(&rec)
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
    fn a_record_from_a_moved_project_is_dropped_not_deleted() {
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
    fn a_moved_project_record_stops_blocking_a_new_start() {
        let root = tmp("sweep_moved");
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
        assert!(d.calls().contains(&"rm -f usta-aaaa1111-x".to_string()));
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
        assert_eq!(d2.calls(), vec!["rm -f usta-aaaa1111-ghost".to_string()]);
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
        sweep_with(&root, TODAY, &d, DEAD, None);
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
        sweep_with(&root, TODAY, &d, DEAD, Some(&counting));
        assert_eq!(asked.get(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn foreign_records_are_dropped_with_no_docker_at_all() {
        let root = tmp("sweep_foreign_nodocker");
        let store = Store::new(&root);
        let mut r = rec("m", RunState::Active, 999_999, "2026-11-04");
        r.reference = "usta-bbbb2222-m".into();
        store.save(&r).unwrap();
        let mut own = rec("own", RunState::Paused, 0, "2026-11-04");
        own.reference = format!("usta-{}-own", project_tag(&root));
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
        assert_eq!(d.calls(), vec!["rm -f usta-aaaa1111-old".to_string()]);
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
        // Foreign reference: dropped by the moved-project rule, docker untouched.
        assert!(d.calls().is_empty());
        assert_eq!(store.load("old").unwrap().state, RunState::Dropped);
        let _ = std::fs::remove_dir_all(root);
    }
}
