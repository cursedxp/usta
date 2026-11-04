//! A model-made task that cannot be solved makes the learner blame themselves, so
//! nothing reaches a learner unproven. Script checks: on a fresh environment the
//! check fails; after the reference solution it passes. Rubric checks: the rubric
//! rests only on the task's material, and the reference answer meets it.

#![allow(dead_code)] // consumed by later steps (Task 11)

use anyhow::Result;
use serde::Deserialize;

use crate::scenario::container::ContainerEnv;
use crate::scenario::docker::{probe, Docker, Runtime};
use crate::scenario::engine::Tick;
use crate::scenario::env::{CheckOutcome, EnvHandle, Environment, Internal, CONTAINER};
use crate::scenario::folder::{copy_data, FolderEnv};
use crate::scenario::lint::walk;
use crate::scenario::model::{model_failed, Model};
use crate::scenario::package::Package;
use crate::scenario::prompt::review_system;
use crate::scenario::rubric::{parse_json, read_work, vote};
use crate::scenario::store::{RunRecord, RunState, Store};

#[derive(Debug, PartialEq)]
pub(crate) enum Proof {
    Passed,
    Problems(Vec<String>),
}

pub(crate) trait Prover {
    fn prove(&mut self, pkg: &Package, round: usize) -> Result<Proof>;
    /// A proven package was cancelled before install: remove what its proof left.
    fn discard(&mut self, _pkg: &Package) {}
}

/// The contract plus what only validation does. Certification is not a sixth operation.
pub(crate) trait Certify: Environment {
    fn apply_solution(&mut self, pkg: &Package, h: &EnvHandle) -> Result<()>;
    fn tools_problem(&mut self, _h: &EnvHandle) -> Option<String> {
        None
    }
}

impl Certify for ContainerEnv<'_> {
    fn apply_solution(&mut self, pkg: &Package, h: &EnvHandle) -> Result<()> {
        ContainerEnv::apply_solution(self, pkg, h)
    }
    fn tools_problem(&mut self, h: &EnvHandle) -> Option<String> {
        self.missing_tools(&h.reference)
    }
}

/// Data files overlaid on a copy of the task. Never a script: a folder solution stays on the host.
impl Certify for FolderEnv<'_> {
    fn apply_solution(&mut self, pkg: &Package, h: &EnvHandle) -> Result<()> {
        copy_data(&pkg.dir.join("solution/files"), &self.dir(h)?)
    }
}

/// A draft is linted before anything is built, copied or run from it: its paths and
/// scripts are the model's, and a build reads the context folder it names.
fn lint_first(pkg: &Package) -> Option<Proof> {
    let problems = pkg.lint();
    (!problems.is_empty()).then_some(Proof::Problems(problems))
}

/// Tool output caused by the package goes back to the model; anything else stops the run.
/// A `ModelFailed` is not `Internal`, so an outage can never become a round.
fn feedback(stage: &str, e: anyhow::Error) -> Result<Proof> {
    if e.downcast_ref::<Internal>().is_some() {
        Ok(Proof::Problems(vec![format!("{stage}: {e}")]))
    } else {
        Err(e)
    }
}

fn two_conditions(env: &mut dyn Certify, pkg: &Package, h: &EnvHandle) -> Result<Proof> {
    if let Some(p) = env.tools_problem(h) {
        return Ok(Proof::Problems(vec![p]));
    }
    match env.check(pkg, h) {
        Err(e) => return feedback("the check broke on a fresh task", e),
        Ok(CheckOutcome::Pass) => {
            return Ok(Proof::Problems(vec![
                "the check already passes on a fresh task, so doing nothing would count as solved"
                    .into(),
            ]))
        }
        Ok(CheckOutcome::Fail { .. }) => {}
    }
    if let Err(e) = env.apply_solution(pkg, h) {
        return feedback("the reference solution failed", e);
    }
    match env.check(pkg, h) {
        Err(e) => feedback("the check broke after the reference solution", e),
        Ok(CheckOutcome::Pass) => Ok(Proof::Passed),
        Ok(CheckOutcome::Fail { output }) => Ok(Proof::Problems(vec![format!(
            "the check still fails after the reference solution: {output}"
        )])),
    }
}

pub(crate) fn prove_script(env: &mut dyn Certify, pkg: &Package, run_id: &str) -> Result<Proof> {
    if let Some(p) = lint_first(pkg) {
        return Ok(p);
    }
    let h = match env.prepare(pkg, run_id) {
        Ok(h) => h,
        Err(e) => return feedback("the task's environment did not build", e),
    };
    let result = two_conditions(env, pkg, &h);
    let _ = env.clean(&h);
    result
}

#[derive(Deserialize)]
struct Review {
    ok: bool,
    #[serde(default)]
    problems: Vec<String>,
}

fn material(pkg: &Package) -> Result<String> {
    let mut out = format!("Task:\n{}\n", pkg.instruction()?);
    let files = pkg.dir.join("environment/files");
    if files.is_dir() {
        let names: Vec<String> = walk(&files)
            .map_err(anyhow::Error::msg)?
            .iter()
            .filter_map(|f| f.strip_prefix(&files).ok().map(|r| r.display().to_string()))
            .collect();
        match read_work(&files, &names)? {
            Ok(text) => out.push_str(&format!("Files:\n{text}\n")),
            Err(e) => out.push_str(&format!("Files could not be read: {e}\n")),
        }
    }
    let rubric: Vec<String> = pkg
        .check
        .rubric
        .iter()
        .map(|c| format!("- {}: {}", c.id, c.text))
        .collect();
    Ok(format!("{out}Rubric:\n{}\n", rubric.join("\n")))
}

fn reference_answer(pkg: &Package) -> Result<String> {
    let answer = pkg.dir.join("solution/answer.md");
    if answer.is_file() {
        return Ok(std::fs::read_to_string(answer)?);
    }
    Ok(
        read_work(&pkg.dir.join("solution/files"), &pkg.check.deliverables)?
            .unwrap_or_else(|missing| format!("(missing: {missing})")),
    )
}

pub(crate) fn prove_rubric(model: &dyn Model, pkg: &Package) -> Result<Proof> {
    if let Some(p) = lint_first(pkg) {
        return Ok(p);
    }
    let reply = model
        .ask(&review_system(), &material(pkg)?)
        .map_err(model_failed)?;
    let review: Review = match parse_json(&reply.text) {
        Ok(r) => r,
        Err(e) => {
            return Ok(Proof::Problems(vec![format!(
                "the rubric review could not be read: {e}"
            )]))
        }
    };
    if !review.ok {
        return Ok(Proof::Problems(if review.problems.is_empty() {
            vec!["the rubric rests on something outside the task".into()]
        } else {
            review.problems
        }));
    }
    match vote(model, pkg, &reference_answer(pkg)?) {
        Ok(v) if v.met => Ok(Proof::Passed),
        Ok(v) => Ok(Proof::Problems(vec![format!(
            "the reference answer does not meet the rubric: {}",
            v.unmet.join("; ")
        )])),
        Err(e) => feedback("the rubric could not be applied", e),
    }
}

pub(crate) struct RealProver<'a> {
    pub docker: &'a dyn Docker,
    pub store: &'a Store,
    pub tag: String,
    pub model: &'a dyn Model,
    pub tick: &'a Tick<'a>,
    pub say: &'a dyn Fn(&str),
    /// Pauses a running task. Called only when a proof needs a server, because at
    /// most one runs; generation that never gets here leaves the task running.
    pub free_server: &'a mut dyn FnMut() -> Result<()>,
}

impl RealProver<'_> {
    /// Record first, so a crash here is resolved by the sweep like an interrupted prepare,
    /// and a live owner keeps a second sweep's hands off the container.
    fn record(&self, pkg: &Package, run_id: &str) -> Result<()> {
        self.store.save(&RunRecord {
            run_id: run_id.into(),
            package: pkg.id.clone(),
            topic: pkg.topic.clone(),
            tone: "plain".into(),
            env: pkg.environment.kind.clone(),
            state: RunState::Preparing,
            owner_pid: self.tick.pid,
            started: self.tick.today.into(),
            updated: self.tick.today.into(),
            reference: String::new(),
            access: String::new(),
            last_check: None,
        })
    }
}

impl Prover for RealProver<'_> {
    fn prove(&mut self, pkg: &Package, round: usize) -> Result<Proof> {
        if pkg.is_rubric() {
            (self.say)(crate::scenario::generate::SAY_RUBRIC);
            return prove_rubric(self.model, pkg);
        }
        // Before a running task is paused for it: a package that cannot run costs nothing.
        if let Some(p) = lint_first(pkg) {
            return Ok(p);
        }
        (self.free_server)()?;
        (self.say)(crate::scenario::generate::SAY_PROVING);
        let run_id = format!("val-{}-r{round}", pkg.id);
        self.record(pkg, &run_id)?;
        let scratch = self.store.scratch_dir();
        let proof = if pkg.environment.kind == CONTAINER {
            let mut env = ContainerEnv {
                docker: self.docker,
                project_tag: self.tag.clone(),
            };
            prove_script(&mut env, pkg, &run_id)
        } else {
            let mut env = FolderEnv {
                docker: self.docker,
                project_tag: self.tag.clone(),
                tasks: scratch.clone(),
                model: self.model,
            };
            prove_script(&mut env, pkg, &run_id)
        };
        let _ = self.store.remove(&run_id);
        let _ = std::fs::remove_dir_all(scratch);
        if matches!(proof, Ok(Proof::Problems(_))) {
            if pkg.environment.kind == CONTAINER {
                let _ = self.docker.run(&["rmi", "-f", &ContainerEnv::image(pkg)]);
            }
            if !matches!(probe(self.docker), Runtime::Ready) {
                anyhow::bail!("the practice server software stopped answering");
            }
        }
        proof
    }

    fn discard(&mut self, pkg: &Package) {
        if pkg.environment.kind == CONTAINER {
            let _ = self.docker.run(&["rmi", "-f", &ContainerEnv::image(pkg)]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::docker::fake::{fail as dfail, ok, FakeDocker};
    use crate::scenario::env::internal;
    use crate::scenario::model::fake::FakeModel;
    use crate::scenario::model::ModelFailed;
    use crate::scenario::package::tests::{tmp, write_pkg};

    struct Fake {
        fresh: Result<CheckOutcome, String>,
        solved: Result<CheckOutcome, String>,
        prepare_fails: bool,
        prepare_plain: bool,
        solve_fails: bool,
        solved_yet: bool,
        calls: Vec<&'static str>,
    }

    impl Fake {
        fn new(fresh: CheckOutcome, solved: CheckOutcome) -> Fake {
            Fake {
                fresh: Ok(fresh),
                solved: Ok(solved),
                prepare_fails: false,
                prepare_plain: false,
                solve_fails: false,
                solved_yet: false,
                calls: vec![],
            }
        }
    }

    fn fail() -> CheckOutcome {
        CheckOutcome::Fail {
            output: "not yet".into(),
        }
    }

    impl Environment for Fake {
        fn prepare(&mut self, _: &Package, _: &str) -> Result<EnvHandle> {
            self.calls.push("prepare");
            if self.prepare_fails {
                return Err(internal("docker build failed: no such package"));
            }
            if self.prepare_plain {
                anyhow::bail!("another project has a task running");
            }
            Ok(EnvHandle {
                reference: "r".into(),
                access: String::new(),
                kind: "fake".into(),
            })
        }
        fn pause(&mut self, _: &EnvHandle) -> Result<()> {
            Ok(())
        }
        fn resume(&mut self, _: &Package, _: &EnvHandle) -> Result<Option<String>> {
            Ok(None)
        }
        fn check(&mut self, _: &Package, _: &EnvHandle) -> Result<CheckOutcome> {
            self.calls.push("check");
            let r = if self.solved_yet {
                &self.solved
            } else {
                &self.fresh
            };
            r.clone().map_err(internal)
        }
        fn clean(&mut self, _: &EnvHandle) -> Result<()> {
            self.calls.push("clean");
            Ok(())
        }
    }

    impl Certify for Fake {
        fn apply_solution(&mut self, _: &Package, _: &EnvHandle) -> Result<()> {
            self.calls.push("solve");
            if self.solve_fails {
                return Err(internal("solve.sh: line 3: not found"));
            }
            self.solved_yet = true;
            Ok(())
        }
    }

    fn pkg(name: &str) -> Package {
        let root = tmp(name);
        Package::load(&write_pkg(&root)).unwrap()
    }

    #[test]
    fn a_good_package_passes_and_is_cleaned() {
        let mut f = Fake::new(fail(), CheckOutcome::Pass);
        assert_eq!(
            prove_script(&mut f, &pkg("val_ok"), "v").unwrap(),
            Proof::Passed
        );
        assert_eq!(f.calls, vec!["prepare", "check", "solve", "check", "clean"]);
    }

    #[test]
    fn a_check_that_already_passes_on_a_fresh_environment_is_rejected() {
        let mut f = Fake::new(CheckOutcome::Pass, CheckOutcome::Pass);
        let Proof::Problems(p) = prove_script(&mut f, &pkg("val_fresh"), "v").unwrap() else {
            panic!()
        };
        assert!(p.concat().contains("fresh"), "{p:?}");
        assert!(f.calls.contains(&"clean"));
    }

    #[test]
    fn a_solution_that_does_not_pass_is_rejected_with_the_check_output() {
        let mut f = Fake::new(fail(), fail());
        let Proof::Problems(p) = prove_script(&mut f, &pkg("val_nosol"), "v").unwrap() else {
            panic!()
        };
        assert!(p.concat().contains("not yet"), "{p:?}");
    }

    #[test]
    fn a_build_or_solution_error_goes_back_to_the_model() {
        let mut f = Fake::new(fail(), CheckOutcome::Pass);
        f.prepare_fails = true;
        let Proof::Problems(p) = prove_script(&mut f, &pkg("val_build"), "v").unwrap() else {
            panic!()
        };
        assert!(p.concat().contains("no such package"));
        let mut f = Fake::new(fail(), CheckOutcome::Pass);
        f.solve_fails = true;
        let Proof::Problems(p) = prove_script(&mut f, &pkg("val_solve"), "v").unwrap() else {
            panic!()
        };
        assert!(p.concat().contains("line 3") && f.calls.contains(&"clean"));
    }

    #[test]
    fn a_broken_check_script_is_feedback_not_a_learner_failure() {
        let mut f = Fake::new(fail(), CheckOutcome::Pass);
        f.fresh = Err("check run failed (2): syntax error".into());
        let Proof::Problems(p) = prove_script(&mut f, &pkg("val_exit"), "v").unwrap() else {
            panic!()
        };
        assert!(p.concat().contains("syntax error"));
    }

    #[test]
    fn a_plain_error_such_as_another_projects_task_is_not_a_round() {
        let mut f = Fake::new(fail(), CheckOutcome::Pass);
        f.prepare_plain = true;
        let e = prove_script(&mut f, &pkg("val_plain"), "v").unwrap_err();
        assert!(e.to_string().contains("another project"), "{e:?}");
    }

    #[test]
    fn a_package_that_fails_lint_is_rejected_before_anything_is_prepared() {
        let root = tmp("val_lint");
        let d = write_pkg(&root);
        std::fs::remove_file(d.join("tone/adventure.md")).unwrap();
        let p = Package::load(&d).unwrap();
        let mut f = Fake::new(fail(), CheckOutcome::Pass);
        let Proof::Problems(x) = prove_script(&mut f, &p, "v").unwrap() else {
            panic!()
        };
        assert!(x.concat().contains("adventure.md"), "{x:?}");
        assert!(
            f.calls.is_empty(),
            "lint comes before prepare: {:?}",
            f.calls
        );

        let store = Store::new(&root);
        let docker = FakeDocker::new(|_| ok(""));
        let alive: &dyn Fn(u32) -> bool = &|_| true;
        let tick = Tick {
            pid: 1,
            alive,
            stamp: "s",
            today: "2026-11-04",
        };
        let m = FakeModel::default();
        let say: &dyn Fn(&str) = &|_| {};
        let mut freed = 0;
        let mut free = || {
            freed += 1;
            Ok(())
        };
        let mut prover = RealProver {
            docker: &docker,
            store: &store,
            tag: "t".into(),
            model: &m,
            tick: &tick,
            say,
            free_server: &mut free,
        };
        assert!(matches!(prover.prove(&p, 0).unwrap(), Proof::Problems(_)));
        assert!(docker.calls().is_empty(), "{:?}", docker.calls());
        assert!(store.all().is_empty());
        assert_eq!(
            freed, 0,
            "a running task is not paused for a package that cannot run"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    const OK_REVIEW: &str = r#"{"ok":true,"problems":[]}"#;
    fn all_met(p: &Package) -> String {
        let c: Vec<String> = p
            .check
            .rubric
            .iter()
            .map(|c| format!("{{\"id\":\"{}\",\"met\":true,\"note\":\"\"}}", c.id))
            .collect();
        format!("{{\"criteria\":[{}]}}", c.join(","))
    }

    #[test]
    fn a_rubric_passes_when_it_rests_on_the_task_and_the_reference_meets_it() {
        let root = tmp("val_rub");
        let p = Package::load(&crate::scenario::lint::tests::folder_pkg(&root)).unwrap();
        let m = FakeModel::with(&[OK_REVIEW, &all_met(&p)]);
        assert_eq!(prove_rubric(&m, &p).unwrap(), Proof::Passed);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_rubric_on_outside_material_or_one_the_reference_cannot_meet_is_rejected() {
        let root = tmp("val_rub_bad");
        let p = Package::load(&crate::scenario::lint::tests::folder_pkg(&root)).unwrap();
        let m = FakeModel::with(&[
            r#"{"ok":false,"problems":["names-audience needs a report that is not in the task"]}"#,
        ]);
        let Proof::Problems(x) = prove_rubric(&m, &p).unwrap() else {
            panic!()
        };
        assert!(x.concat().contains("not in the task"));
        let miss = r#"{"criteria":[{"id":"names-audience","met":false,"note":"who?"},{"id":"gives-reason","met":true,"note":""}]}"#;
        let m = FakeModel::with(&[OK_REVIEW, miss]);
        let Proof::Problems(x) = prove_rubric(&m, &p).unwrap() else {
            panic!()
        };
        assert!(x.concat().contains("reference answer"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_model_outage_during_validation_is_an_error_never_a_round() {
        let root = tmp("val_rub_down");
        let p = Package::load(&crate::scenario::lint::tests::folder_pkg(&root)).unwrap();
        // Down at the review.
        let m = FakeModel::default();
        m.replies.lock().unwrap().push_back(Err("503".into()));
        let e = prove_rubric(&m, &p).unwrap_err();
        assert!(e.downcast_ref::<ModelFailed>().is_some(), "{e:?}");
        // Down while the reference answer is judged.
        let m = FakeModel::with(&[OK_REVIEW]);
        m.replies.lock().unwrap().push_back(Err("503".into()));
        let e = prove_rubric(&m, &p).unwrap_err();
        assert!(e.downcast_ref::<ModelFailed>().is_some(), "{e:?}");
        assert!(e.downcast_ref::<Internal>().is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_real_proof_writes_a_record_first_and_leaves_none() {
        let root = tmp("val_rec");
        let p = Package::load(&write_pkg(&root)).unwrap();
        let store = Store::new(&root);
        let seen = store.clone();
        let d = FakeDocker::new(move |l| {
            if l.starts_with("create") {
                let r = seen.all();
                assert_eq!(r.len(), 1, "record first");
                assert_eq!(r[0].state, RunState::Preparing);
            }
            if l.starts_with("ps ") {
                ok("")
            } else if l.contains(" timeout ") {
                dfail(1, "not yet")
            } else {
                ok("true")
            }
        });
        let alive: &dyn Fn(u32) -> bool = &|_| true;
        let tick = Tick {
            pid: 1,
            alive,
            stamp: "s",
            today: "2026-11-04",
        };
        let m = FakeModel::default();
        let say: &dyn Fn(&str) = &|_| {};
        let mut free = || Ok(());
        let mut prover = RealProver {
            docker: &d,
            store: &store,
            tag: "aaaa1111".into(),
            model: &m,
            tick: &tick,
            say,
            free_server: &mut free,
        };
        let _ = prover.prove(&p, 0).unwrap();
        assert!(store.all().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_running_task_is_paused_only_when_a_script_proof_needs_a_server() {
        let root = tmp("val_free");
        let p = Package::load(&crate::scenario::lint::tests::folder_pkg(&root)).unwrap();
        let store = Store::new(&root);
        let d = FakeDocker::new(|_| ok(""));
        let alive: &dyn Fn(u32) -> bool = &|_| true;
        let tick = Tick {
            pid: 1,
            alive,
            stamp: "s",
            today: "2026-11-04",
        };
        let m = FakeModel::with(&[OK_REVIEW, &all_met(&p)]);
        let say: &dyn Fn(&str) = &|_| {};
        let mut freed = 0;
        let mut free = || {
            freed += 1;
            Ok(())
        };
        let mut prover = RealProver {
            docker: &d,
            store: &store,
            tag: "t".into(),
            model: &m,
            tick: &tick,
            say,
            free_server: &mut free,
        };
        assert_eq!(prover.prove(&p, 0).unwrap(), Proof::Passed);
        assert_eq!(freed, 0, "a rubric proof needs no server");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_runtime_that_died_is_an_error_not_a_round() {
        let root = tmp("val_dead");
        let p = Package::load(&write_pkg(&root)).unwrap();
        let store = Store::new(&root);
        let d = FakeDocker::new(|l| {
            if l.starts_with("ps ") {
                ok("")
            } else {
                dfail(1, "Cannot connect to the Docker daemon")
            }
        });
        let alive: &dyn Fn(u32) -> bool = &|_| true;
        let tick = Tick {
            pid: 1,
            alive,
            stamp: "s",
            today: "2026-11-04",
        };
        let m = FakeModel::default();
        let say: &dyn Fn(&str) = &|_| {};
        let mut free = || Ok(());
        let mut prover = RealProver {
            docker: &d,
            store: &store,
            tag: "t".into(),
            model: &m,
            tick: &tick,
            say,
            free_server: &mut free,
        };
        assert!(prover.prove(&p, 0).is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
