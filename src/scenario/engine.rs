//! Scenario engine: drives one run through the `Environment` contract and
//! keeps the run record honest. Knows nothing about Docker.

use anyhow::{bail, Result};

use crate::scenario::env::{CheckOutcome, Environment};
use crate::scenario::package::{Package, Tone};
use crate::scenario::store::{RunRecord, RunState, Store};

/// Clock and process facts, injected so the engine is deterministic in tests.
pub(crate) struct Tick<'a> {
    pub pid: u32,
    pub alive: &'a dyn Fn(u32) -> bool,
    /// `YYYYMMDDHHMMSS`, makes run ids unique.
    pub stamp: &'a str,
    /// `YYYY-MM-DD`.
    pub today: &'a str,
}

pub(crate) struct CheckReport {
    pub outcome: CheckOutcome,
    /// Set when the run finished but the environment could not be cleaned; the startup sweep retries.
    pub clean_warning: Option<String>,
}

/// A run a different live Usta owns is never paused from here. Asked on its own too,
/// so a start can refuse before paying for a generation.
pub(crate) fn refuse_other_owner(store: &Store, t: &Tick, except: Option<&str>) -> Result<()> {
    for rec in store.all() {
        if rec.state == RunState::Active
            && Some(rec.run_id.as_str()) != except
            && rec.owner_pid != 0
            && rec.owner_pid != t.pid
            && (t.alive)(rec.owner_pid)
        {
            bail!(
                "another usta (pid {}) is running scenario '{}'; close it first",
                rec.owner_pid,
                rec.package
            );
        }
    }
    Ok(())
}

/// At most one run is active per project: pause the others, but never one a
/// different live Usta owns.
pub(crate) fn release_others(
    env: &mut dyn Environment,
    store: &Store,
    t: &Tick,
    except: Option<&str>,
) -> Result<()> {
    refuse_other_owner(store, t, except)?;
    for mut rec in store.all() {
        if rec.state == RunState::Active && Some(rec.run_id.as_str()) != except {
            pause(env, store, &mut rec, t)?;
        }
    }
    Ok(())
}

pub(crate) fn start(
    env: &mut dyn Environment,
    store: &Store,
    pkg: &Package,
    tone: Tone,
    t: &Tick,
) -> Result<RunRecord> {
    release_others(env, store, t, None)?;
    let mut rec = RunRecord {
        run_id: format!("{}-{}", pkg.id, t.stamp),
        package: pkg.id.clone(),
        topic: pkg.topic.clone(),
        tone: tone.as_str().into(),
        state: RunState::Preparing,
        owner_pid: t.pid,
        started: t.today.into(),
        updated: t.today.into(),
        reference: String::new(),
        access: String::new(),
        env: pkg.environment.kind.clone(),
        last_check: None,
    };
    // Record first: a crash mid-prepare leaves a record the sweep can resolve,
    // never an unrecorded container.
    store.save(&rec)?;
    match env.prepare(pkg, &rec.run_id) {
        Ok(h) => {
            rec.reference = h.reference;
            rec.access = h.access;
            rec.state = RunState::Active;
            store.save(&rec)?;
            Ok(rec)
        }
        Err(e) => {
            let _ = store.remove(&rec.run_id);
            Err(e)
        }
    }
}

/// `Some(warning)`: the run is back but its programs are not (see `Environment::resume`).
pub(crate) fn resume(
    env: &mut dyn Environment,
    store: &Store,
    pkg: &Package,
    rec: &mut RunRecord,
    t: &Tick,
) -> Result<Option<String>> {
    if matches!(
        rec.state,
        RunState::Finished | RunState::Preparing | RunState::Dropped
    ) {
        bail!("run '{}' cannot be resumed ({:?})", rec.run_id, rec.state);
    }
    release_others(env, store, t, Some(&rec.run_id))?;
    if rec.state == RunState::Active
        && rec.owner_pid != 0
        && rec.owner_pid != t.pid
        && (t.alive)(rec.owner_pid)
    {
        bail!(
            "another usta (pid {}) is running this scenario",
            rec.owner_pid
        );
    }
    let warning = env.resume(pkg, &rec.handle())?;
    rec.state = RunState::Active;
    rec.owner_pid = t.pid;
    rec.updated = t.today.into();
    store.save(rec)?;
    Ok(warning)
}

pub(crate) fn pause(
    env: &mut dyn Environment,
    store: &Store,
    rec: &mut RunRecord,
    t: &Tick,
) -> Result<()> {
    if rec.state == RunState::Paused {
        return Ok(());
    }
    env.pause(&rec.handle())?;
    rec.state = RunState::Paused;
    rec.owner_pid = 0;
    rec.updated = t.today.into();
    store.save(rec)
}

/// Run the check. On a pass the record is written as finished FIRST, then the
/// environment is cleaned, so a crash in between leaves a finished record and a
/// leftover the sweep removes, never an unfinished-looking task.
pub(crate) fn check(
    env: &mut dyn Environment,
    store: &Store,
    pkg: &Package,
    rec: &mut RunRecord,
    t: &Tick,
) -> Result<CheckReport> {
    if rec.state != RunState::Active {
        bail!("run '{}' is not active; use /scenario resume", rec.run_id);
    }
    let outcome = env.check(pkg, &rec.handle())?;
    rec.updated = t.today.into();
    let mut clean_warning = None;
    match &outcome {
        CheckOutcome::Pass => {
            rec.last_check = Some("pass".into());
            rec.state = RunState::Finished;
            rec.owner_pid = 0;
            store.save(rec)?;
            let _ = std::fs::remove_file(store.said_path(&rec.run_id));
            clean_warning = env.clean(&rec.handle()).err().map(|e| e.to_string());
        }
        CheckOutcome::Fail { .. } => {
            rec.last_check = Some("fail".into());
            store.save(rec)?;
        }
    }
    Ok(CheckReport {
        outcome,
        clean_warning,
    })
}

/// Abandon an unfinished run. Like finishing, the record is written FIRST
/// (as `Dropped`), then the environment is cleaned, so a crash in between
/// leaves a leftover the startup sweep removes, never a half-dropped task.
/// Returns a warning when the clean failed.
pub(crate) fn drop_run(
    env: &mut dyn Environment,
    store: &Store,
    rec: &mut RunRecord,
    t: &Tick,
) -> Result<Option<String>> {
    if matches!(rec.state, RunState::Finished | RunState::Dropped) {
        bail!("'{}' is already over", rec.package);
    }
    if rec.state == RunState::Active
        && rec.owner_pid != 0
        && rec.owner_pid != t.pid
        && (t.alive)(rec.owner_pid)
    {
        bail!(
            "another usta (pid {}) is running this scenario",
            rec.owner_pid
        );
    }
    rec.state = RunState::Dropped;
    rec.owner_pid = 0;
    rec.updated = t.today.into();
    store.save(rec)?;
    let _ = std::fs::remove_file(store.said_path(&rec.run_id));
    if rec.reference.is_empty() {
        return Ok(None); // interrupted before anything existed
    }
    Ok(env.clean(&rec.handle()).err().map(|e| e.to_string()))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::path::PathBuf;

    use crate::scenario::env::EnvHandle;

    use super::*;
    use crate::scenario::package::tests::{tmp, write_pkg};

    /// Test double for the contract: no Docker, records every call.
    struct FakeEnv {
        calls: Vec<String>,
        checks: VecDeque<CheckOutcome>,
        fail_prepare: bool,
        fail_clean: bool,
        store: Option<Store>,
        state_seen_at_clean: Option<RunState>,
        resume_warning: Option<String>,
    }

    impl FakeEnv {
        fn new() -> FakeEnv {
            FakeEnv {
                calls: vec![],
                checks: VecDeque::new(),
                fail_prepare: false,
                fail_clean: false,
                store: None,
                state_seen_at_clean: None,
                resume_warning: None,
            }
        }
    }

    impl Environment for FakeEnv {
        fn prepare(&mut self, _p: &Package, run_id: &str) -> Result<EnvHandle> {
            self.calls.push(format!("prepare {run_id}"));
            if self.fail_prepare {
                anyhow::bail!("boom");
            }
            Ok(EnvHandle {
                reference: format!("fake-{run_id}"),
                access: "fake access".into(),
                kind: "container".into(),
            })
        }
        fn pause(&mut self, h: &EnvHandle) -> Result<()> {
            self.calls.push(format!("pause {}", h.reference));
            Ok(())
        }
        fn resume(&mut self, _p: &Package, h: &EnvHandle) -> Result<Option<String>> {
            self.calls.push(format!("resume {}", h.reference));
            Ok(self.resume_warning.clone())
        }
        fn check(&mut self, _p: &Package, h: &EnvHandle) -> Result<CheckOutcome> {
            self.calls.push(format!("check {}", h.reference));
            Ok(self.checks.pop_front().unwrap_or(CheckOutcome::Pass))
        }
        fn clean(&mut self, h: &EnvHandle) -> Result<()> {
            self.calls.push(format!("clean {}", h.reference));
            if let Some(s) = &self.store {
                self.state_seen_at_clean = s.all().first().map(|r| r.state);
            }
            if self.fail_clean {
                anyhow::bail!("rm failed");
            }
            Ok(())
        }
    }

    fn tick<'a>(pid: u32, stamp: &'a str, alive: &'a dyn Fn(u32) -> bool) -> Tick<'a> {
        Tick {
            pid,
            alive,
            stamp,
            today: "2026-11-04",
        }
    }

    fn setup(name: &str) -> (PathBuf, Store, Package) {
        let root = tmp(name);
        let pkg = Package::load(&write_pkg(&root)).unwrap();
        (root.clone(), Store::new(&root), pkg)
    }

    const NOBODY: &dyn Fn(u32) -> bool = &|_| false;

    #[test]
    fn start_writes_an_active_record_owned_by_us() {
        let (root, store, pkg) = setup("eng_start");
        let mut env = FakeEnv::new();
        let rec = start(
            &mut env,
            &store,
            &pkg,
            Tone::Adventure,
            &tick(10, "20261104090000", NOBODY),
        )
        .unwrap();
        assert_eq!(rec.run_id, "demo-pkg-20261104090000");
        assert_eq!(rec.state, RunState::Active);
        assert_eq!(rec.owner_pid, 10);
        assert_eq!(rec.tone, "adventure");
        assert_eq!(rec.reference, "fake-demo-pkg-20261104090000");
        assert_eq!(store.load(&rec.run_id).unwrap(), rec);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_prepare_leaves_no_record() {
        let (root, store, pkg) = setup("eng_failprep");
        let mut env = FakeEnv::new();
        env.fail_prepare = true;
        assert!(start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).is_err());
        assert!(store.all().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn second_start_pauses_the_first_so_one_runs_at_a_time() {
        let (root, store, pkg) = setup("eng_two");
        let mut env = FakeEnv::new();
        let a = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        let b = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "2", NOBODY)).unwrap();
        assert_eq!(store.load(&a.run_id).unwrap().state, RunState::Paused);
        assert_eq!(store.load(&b.run_id).unwrap().state, RunState::Active);
        assert_eq!(
            env.calls,
            vec![
                "prepare demo-pkg-1".to_string(),
                "pause fake-demo-pkg-1".into(),
                "prepare demo-pkg-2".into()
            ]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_run_owned_by_another_live_usta_blocks_start() {
        let (root, store, pkg) = setup("eng_live");
        let mut env = FakeEnv::new();
        start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        let alive: &dyn Fn(u32) -> bool = &|p| p == 10;
        let err = start(&mut env, &store, &pkg, Tone::Plain, &tick(99, "2", alive)).unwrap_err();
        assert!(err.to_string().contains("another usta (pid 10)"), "{err}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pause_is_idempotent_and_clears_the_owner() {
        let (root, store, pkg) = setup("eng_pause");
        let mut env = FakeEnv::new();
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        pause(&mut env, &store, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        pause(&mut env, &store, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        assert_eq!(
            env.calls.iter().filter(|c| c.starts_with("pause")).count(),
            1
        );
        let saved = store.load(&rec.run_id).unwrap();
        assert_eq!((saved.state, saved.owner_pid), (RunState::Paused, 0));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resume_reactivates_and_takes_ownership() {
        let (root, store, pkg) = setup("eng_resume");
        let mut env = FakeEnv::new();
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        pause(&mut env, &store, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        resume(&mut env, &store, &pkg, &mut rec, &tick(20, "x", NOBODY)).unwrap();
        let saved = store.load(&rec.run_id).unwrap();
        assert_eq!((saved.state, saved.owner_pid), (RunState::Active, 20));
        assert!(env.calls.contains(&"resume fake-demo-pkg-1".to_string()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_resume_warning_still_reactivates_the_run_and_reaches_the_caller() {
        let (root, store, pkg) = setup("eng_resume_warn");
        let mut env = FakeEnv::new();
        env.resume_warning = Some("start.sh failed".into());
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        pause(&mut env, &store, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        let w = resume(&mut env, &store, &pkg, &mut rec, &tick(20, "x", NOBODY)).unwrap();
        assert_eq!(w.as_deref(), Some("start.sh failed"));
        assert_eq!(store.load(&rec.run_id).unwrap().state, RunState::Active);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resume_of_a_finished_run_is_refused() {
        let (root, store, pkg) = setup("eng_resume_fin");
        let mut env = FakeEnv::new();
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        check(&mut env, &store, &pkg, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        assert!(resume(&mut env, &store, &pkg, &mut rec, &tick(10, "x", NOBODY)).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failing_check_keeps_the_run_active() {
        let (root, store, pkg) = setup("eng_checkfail");
        let mut env = FakeEnv::new();
        env.checks.push_back(CheckOutcome::Fail {
            output: "nope".into(),
        });
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        let r = check(&mut env, &store, &pkg, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        assert_eq!(
            r.outcome,
            CheckOutcome::Fail {
                output: "nope".into()
            }
        );
        let saved = store.load(&rec.run_id).unwrap();
        assert_eq!(saved.state, RunState::Active);
        assert_eq!(saved.last_check.as_deref(), Some("fail"));
        assert!(!env.calls.iter().any(|c| c.starts_with("clean")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn passing_check_writes_finished_before_clean() {
        let (root, store, pkg) = setup("eng_checkpass");
        let mut env = FakeEnv::new();
        env.store = Some(store.clone());
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        let r = check(&mut env, &store, &pkg, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        assert_eq!(r.outcome, CheckOutcome::Pass);
        assert_eq!(env.state_seen_at_clean, Some(RunState::Finished));
        let saved = store.load(&rec.run_id).unwrap();
        assert_eq!((saved.state, saved.owner_pid), (RunState::Finished, 0));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn clean_failure_is_a_warning_not_an_error() {
        let (root, store, pkg) = setup("eng_cleanfail");
        let mut env = FakeEnv::new();
        env.fail_clean = true;
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        let r = check(&mut env, &store, &pkg, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        assert_eq!(r.outcome, CheckOutcome::Pass);
        assert!(r.clean_warning.unwrap().contains("rm failed"));
        assert_eq!(store.load(&rec.run_id).unwrap().state, RunState::Finished);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn drop_writes_dropped_before_cleaning_and_keeps_the_record() {
        let (root, store, pkg) = setup("eng_drop");
        let mut env = FakeEnv::new();
        env.store = Some(store.clone());
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        let warn = drop_run(&mut env, &store, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        assert!(warn.is_none());
        assert_eq!(env.state_seen_at_clean, Some(RunState::Dropped));
        let saved = store.load(&rec.run_id).unwrap();
        assert_eq!((saved.state, saved.owner_pid), (RunState::Dropped, 0));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn drop_works_on_a_paused_run_and_reports_a_failed_clean_as_a_warning() {
        let (root, store, pkg) = setup("eng_drop_paused");
        let mut env = FakeEnv::new();
        env.fail_clean = true;
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        pause(&mut env, &store, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        let warn = drop_run(&mut env, &store, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        assert!(warn.unwrap().contains("rm failed"));
        assert_eq!(store.load(&rec.run_id).unwrap().state, RunState::Dropped);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn drop_of_a_finished_or_dropped_run_is_refused_and_so_is_resuming_it() {
        let (root, store, pkg) = setup("eng_drop_over");
        let mut env = FakeEnv::new();
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        drop_run(&mut env, &store, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        assert!(drop_run(&mut env, &store, &mut rec, &tick(10, "x", NOBODY)).is_err());
        assert!(resume(&mut env, &store, &pkg, &mut rec, &tick(10, "x", NOBODY)).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn drop_by_another_live_usta_owner_is_refused() {
        let (root, store, pkg) = setup("eng_drop_live");
        let mut env = FakeEnv::new();
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        let alive: &dyn Fn(u32) -> bool = &|p| p == 10;
        assert!(drop_run(&mut env, &store, &mut rec, &tick(99, "x", alive)).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn check_on_a_paused_run_is_refused() {
        let (root, store, pkg) = setup("eng_check_paused");
        let mut env = FakeEnv::new();
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        pause(&mut env, &store, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        assert!(check(&mut env, &store, &pkg, &mut rec, &tick(10, "x", NOBODY)).is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
