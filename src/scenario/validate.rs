//! A model-made task that cannot be solved makes the learner blame themselves, so
//! nothing reaches a learner unproven. Script checks: on a fresh environment the
//! check fails; after the reference solution it passes. Rubric checks: the rubric
//! rests only on the task's material, and the reference answer meets it.

use anyhow::Result;
use serde::Deserialize;

use crate::scenario::container::ContainerEnv;
use crate::scenario::docker::{probe, Docker, Runtime};
use crate::scenario::engine::Tick;
use crate::scenario::env::{internal, CheckOutcome, EnvHandle, Environment, Internal, CONTAINER};
use crate::scenario::folder::{copy_data, FolderEnv};
use crate::scenario::lint::walk;
use crate::scenario::model::{model_failed, Model, ModelFailed};
use crate::scenario::package::Package;
use crate::scenario::prompt::review_system;
use crate::scenario::rubric::{panel, parse_json, read_work};
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

/// A check that cannot run is the package's doing (it stopped the task, or ate its own
/// files), so its text goes to the model. It is never the plain "use /scenario resume"
/// sentence: no learner run exists here to resume. Whether the runtime itself died is
/// the prover's question, asked after a proof that did not pass.
fn check_in_proof(env: &mut dyn Certify, pkg: &Package, h: &EnvHandle) -> Result<CheckOutcome> {
    env.check(pkg, h).map_err(|e| {
        if e.downcast_ref::<Internal>().is_some() || e.downcast_ref::<ModelFailed>().is_some() {
            e
        } else {
            internal("the task was not running when its check ran")
        }
    })
}

fn two_conditions(env: &mut dyn Certify, pkg: &Package, h: &EnvHandle) -> Result<Proof> {
    if let Some(p) = env.tools_problem(h) {
        return Ok(Proof::Problems(vec![p]));
    }
    // Twice: a check that changes the task when it runs would pass the second time.
    for n in 0..2 {
        match check_in_proof(env, pkg, h) {
            Err(e) => return feedback("the check broke on a fresh task", e),
            Ok(CheckOutcome::Pass) if n == 0 => {
                return Ok(Proof::Problems(vec![
                    "the check already passes on a fresh task, so doing nothing would count as solved"
                        .into(),
                ]))
            }
            Ok(CheckOutcome::Pass) => {
                return Ok(Proof::Problems(vec![
                    "the check passes the second time it runs on a fresh task, so running it changes the task"
                        .into(),
                ]))
            }
            Ok(CheckOutcome::Fail { .. }) => {}
        }
    }
    if let Err(e) = env.apply_solution(pkg, h) {
        return feedback("the reference solution failed", e);
    }
    match check_in_proof(env, pkg, h) {
        Err(e) => feedback("the check broke after the reference solution", e),
        Ok(CheckOutcome::Pass) => Ok(Proof::Passed),
        Ok(CheckOutcome::Fail { output }) => Ok(Proof::Problems(vec![format!(
            "the check still fails after the reference solution: {output}"
        )])),
    }
}

#[cfg(test)]
pub(crate) fn prove_script(env: &mut dyn Certify, pkg: &Package, run_id: &str) -> Result<Proof> {
    prove_and_clean(env, pkg, run_id).0
}

/// The proof, and whether everything it made is gone. `false`: `clean` failed, so a
/// resource may remain and the caller must keep its record for the sweep.
fn prove_and_clean(env: &mut dyn Certify, pkg: &Package, run_id: &str) -> (Result<Proof>, bool) {
    if let Some(p) = lint_first(pkg) {
        return (Ok(p), true);
    }
    let h = match env.prepare(pkg, run_id) {
        Ok(h) => h,
        Err(e) => return (feedback("the task's environment did not build", e), true),
    };
    let result = two_conditions(env, pkg, &h);
    let cleaned = env.clean(&h).is_ok();
    (result, cleaned)
}

#[derive(Deserialize)]
struct Review {
    ok: bool,
    #[serde(default)]
    problems: Vec<String>,
}

/// What the review model reads, at most this many characters.
const MATERIAL_LIMIT: usize = 60_000;

fn material(pkg: &Package) -> Result<String> {
    let mut out = format!("Task:\n{}\n", pkg.instruction()?);
    let files = pkg.dir.join("environment/files");
    if files.is_dir() {
        out.push_str("Files:\n");
        for f in walk(&files).map_err(anyhow::Error::msg)? {
            let Ok(rel) = f.strip_prefix(&files) else {
                continue;
            };
            let name = rel.display().to_string();
            // One file at a time: a file that is not text must not hide the others.
            match read_work(&files, std::slice::from_ref(&name))? {
                Ok(text) => out.push_str(&text),
                Err(e) => out.push_str(&format!("=== {name} ===\n(not shown: {e})\n")),
            }
        }
    }
    let rubric: Vec<String> = pkg
        .check
        .rubric
        .iter()
        .map(|c| format!("- {}: {}", c.id, c.text))
        .collect();
    let rubric = format!("Rubric:\n{}\n", rubric.join("\n"));
    // The rubric is what is judged, so the files give way first.
    let room = MATERIAL_LIMIT.saturating_sub(rubric.chars().count());
    if out.chars().count() > room {
        out = out.chars().take(room).collect();
        out.push_str("\n[cut: the rest of the files is not shown]\n");
    }
    Ok(format!("{out}{rubric}"))
}

/// The reference answer as text. Inner `Err`: it is missing or blank, which is the
/// package's problem; a blank reference would be judged as work and could pass.
fn reference_answer(pkg: &Package) -> Result<Result<String, String>> {
    let answer = pkg.dir.join("solution/answer.md");
    if answer.is_file() {
        let text = std::fs::read_to_string(answer)?;
        return Ok(if text.trim().is_empty() {
            Err("solution/answer.md is empty".into())
        } else {
            Ok(text)
        });
    }
    let files = pkg.dir.join("solution/files");
    let mut out = String::new();
    for name in &pkg.check.deliverables {
        match read_work(&files, std::slice::from_ref(name))? {
            Err(e) => return Ok(Err(format!("the reference answer has a problem: {e}"))),
            Ok(text) => {
                let body = text.split_once('\n').map_or("", |(_, b)| b);
                if body.trim().is_empty() {
                    return Ok(Err(format!("solution/files/{name} is empty")));
                }
                out.push_str(&text);
            }
        }
    }
    Ok(if out.is_empty() {
        Err("the package has no reference answer".into())
    } else {
        Ok(out)
    })
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
    // `ok` with problems listed is not ok: the problems are what the model must fix.
    if !review.ok || !review.problems.is_empty() {
        return Ok(Proof::Problems(if review.problems.is_empty() {
            vec!["the rubric rests on something outside the task".into()]
        } else {
            review.problems
        }));
    }
    let reference = match reference_answer(pkg)? {
        Ok(text) => text,
        Err(why) => return Ok(Proof::Problems(vec![why])),
    };
    match panel(model, pkg, &reference) {
        Ok(CheckOutcome::Pass) => Ok(Proof::Passed),
        Ok(CheckOutcome::Fail { output }) => Ok(Proof::Problems(vec![format!(
            "the reference answer does not meet the rubric: {}",
            output.replace('\n', "; ")
        )])),
        Err(e) => feedback("the rubric could not be applied", e),
    }
}

#[allow(dead_code)] // reached from the command layer in Task 12
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

#[allow(dead_code)] // reached from the command layer in Task 12
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

#[allow(dead_code)] // reached from the command layer in Task 12
impl Prover for RealProver<'_> {
    fn prove(&mut self, pkg: &Package, round: usize) -> Result<Proof> {
        // Only Usta-assigned ids take the capped build; a built-in id would reach the
        // unlimited `docker build` path.
        if !pkg.is_generated() {
            return Err(internal(format!(
                "validation refused '{}': not a generated package",
                pkg.id
            )));
        }
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
        let (proof, cleaned) = if pkg.environment.kind == CONTAINER {
            let mut env = ContainerEnv {
                docker: self.docker,
                project_tag: self.tag.clone(),
            };
            prove_and_clean(&mut env, pkg, &run_id)
        } else {
            let mut env = FolderEnv {
                docker: self.docker,
                project_tag: self.tag.clone(),
                tasks: scratch.clone(),
                model: self.model,
            };
            prove_and_clean(&mut env, pkg, &run_id)
        };
        // A failed clean keeps the record: the sweep needs it to find what is left.
        if cleaned {
            let _ = self.store.remove(&run_id);
            let _ = std::fs::remove_dir_all(scratch);
        }
        // Anything but a pass leaves no image behind, and an error is no reason to skip the probe.
        if !matches!(proof, Ok(Proof::Passed)) {
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
        second_passes: bool,
        fresh_runs: usize,
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
                second_passes: false,
                fresh_runs: 0,
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
            if !self.solved_yet {
                self.fresh_runs += 1;
                if self.second_passes && self.fresh_runs == 2 {
                    return Ok(CheckOutcome::Pass);
                }
            }
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

    /// What the prover accepts: the same package under a Usta-assigned id.
    fn as_generated(p: Package) -> Package {
        let to = p.dir.with_file_name("gen-demo");
        let _ = std::fs::remove_dir_all(&to);
        std::fs::rename(&p.dir, &to).unwrap();
        let file = to.join("task.toml");
        let text = std::fs::read_to_string(&file).unwrap();
        let text = text.replace(&format!("id = \"{}\"", p.id), "id = \"gen-demo\"");
        std::fs::write(&file, text).unwrap();
        Package::load(&to).unwrap()
    }

    #[test]
    fn a_good_package_passes_and_is_cleaned() {
        let mut f = Fake::new(fail(), CheckOutcome::Pass);
        assert_eq!(
            prove_script(&mut f, &pkg("val_ok"), "v").unwrap(),
            Proof::Passed
        );
        assert_eq!(
            f.calls,
            vec!["prepare", "check", "check", "solve", "check", "clean"]
        );
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
    fn a_check_that_changes_the_task_when_it_runs_is_rejected() {
        let mut f = Fake::new(fail(), CheckOutcome::Pass);
        f.second_passes = true;
        let Proof::Problems(p) = prove_script(&mut f, &pkg("val_twice"), "v").unwrap() else {
            panic!()
        };
        assert!(p.concat().contains("second time"), "{p:?}");
        assert!(f.calls.contains(&"clean") && !f.calls.contains(&"solve"));
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
        let p = as_generated(Package::load(&d).unwrap());
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
        let m = FakeModel::with(&[OK_REVIEW, &all_met(&p), &all_met(&p)]);
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
        let m = FakeModel::with(&[OK_REVIEW, miss, miss]);
        let Proof::Problems(x) = prove_rubric(&m, &p).unwrap() else {
            panic!()
        };
        assert!(x.concat().contains("reference answer"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_missing_or_blank_reference_answer_is_a_problem_even_if_the_judge_would_pass_it() {
        let root = tmp("val_blank");
        let d = crate::scenario::lint::tests::folder_pkg(&root);
        let p = Package::load(&d).unwrap();
        for content in [None, Some("  \n")] {
            match content {
                None => std::fs::remove_file(d.join("solution/files/plan.md")).unwrap(),
                Some(c) => std::fs::write(d.join("solution/files/plan.md"), c).unwrap(),
            }
            let m = FakeModel::with(&[OK_REVIEW, &all_met(&p), &all_met(&p), &all_met(&p)]);
            let Proof::Problems(x) = prove_rubric(&m, &p).unwrap() else {
                panic!("{content:?}")
            };
            assert!(x.concat().contains("plan.md"), "{x:?}");
            assert_eq!(m.prompts.lock().unwrap().len(), 1, "only the review ran");
        }
        std::fs::write(d.join("solution/answer.md"), "\n").unwrap();
        let m = FakeModel::with(&[OK_REVIEW, &all_met(&p), &all_met(&p)]);
        assert!(matches!(prove_rubric(&m, &p).unwrap(), Proof::Problems(_)));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_review_that_lists_problems_is_not_ok_whatever_its_flag_says() {
        let root = tmp("val_ok_problems");
        let p = Package::load(&crate::scenario::lint::tests::folder_pkg(&root)).unwrap();
        let m = FakeModel::with(&[r#"{"ok":true,"problems":["x needs outside data"]}"#]);
        let Proof::Problems(x) = prove_rubric(&m, &p).unwrap() else {
            panic!()
        };
        assert!(x.concat().contains("outside data"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_reference_is_judged_by_the_panel_not_one_vote() {
        let root = tmp("val_panel");
        let p = Package::load(&crate::scenario::lint::tests::folder_pkg(&root)).unwrap();
        let miss = r#"{"criteria":[{"id":"names-audience","met":false,"note":"who?"},{"id":"gives-reason","met":true,"note":""}]}"#;
        // One lenient vote and one strict vote: the third decides.
        let m = FakeModel::with(&[OK_REVIEW, &all_met(&p), miss, miss]);
        assert!(matches!(prove_rubric(&m, &p).unwrap(), Proof::Problems(_)));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_file_that_is_not_text_does_not_hide_the_others_and_the_size_is_capped() {
        let root = tmp("val_material");
        let d = crate::scenario::lint::tests::folder_pkg(&root);
        std::fs::write(d.join("environment/files/logo.bin"), [0u8, 1, 2]).unwrap();
        let p = Package::load(&d).unwrap();
        let m = material(&p).unwrap();
        assert!(m.contains("Audience: bakers"), "{m}");
        assert!(m.contains("logo.bin") && m.contains("not shown"), "{m}");
        for i in 0..4 {
            std::fs::write(
                d.join(format!("environment/files/big{i}.txt")),
                "x".repeat(40_000),
            )
            .unwrap();
        }
        let m = material(&p).unwrap();
        assert!(m.chars().count() < MATERIAL_LIMIT + 200, "{}", m.len());
        assert!(
            m.contains("Rubric:") && m.contains("names-audience"),
            "the rubric survives the cut"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    fn prover_with<R>(
        root: &std::path::Path,
        d: &FakeDocker,
        f: impl FnOnce(&mut RealProver) -> R,
    ) -> (R, Store) {
        let store = Store::new(root);
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
            docker: d,
            store: &store,
            tag: "t".into(),
            model: &m,
            tick: &tick,
            say,
            free_server: &mut free,
        };
        let r = f(&mut prover);
        (r, store)
    }

    fn rmi(d: &FakeDocker) -> bool {
        d.calls().iter().any(|c| c.starts_with("rmi "))
    }

    #[test]
    fn a_package_that_is_not_generated_is_refused_before_anything_runs() {
        let root = tmp("val_not_gen");
        let p = Package::load(&write_pkg(&root)).unwrap();
        assert!(!p.is_generated());
        let d = FakeDocker::new(|_| ok("true"));
        let (r, store) = prover_with(&root, &d, |pr| pr.prove(&p, 0));
        assert!(
            r.is_err(),
            "a built-in id would take the unlimited build path"
        );
        assert!(d.calls().is_empty(), "{:?}", d.calls());
        assert!(store.all().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_image_is_removed_after_a_problem_and_an_error_and_kept_after_a_pass() {
        // Problems: the check never passes.
        let root = tmp("val_img_problem");
        let p = as_generated(Package::load(&write_pkg(&root)).unwrap());
        let d = FakeDocker::new(|l| {
            if l.starts_with("ps ") {
                ok("")
            } else if l.contains(" sh /opt/.usta-check/check.sh") {
                dfail(1, "not yet")
            } else {
                ok("true")
            }
        });
        let (r, _) = prover_with(&root, &d, |pr| pr.prove(&p, 0));
        assert!(matches!(r.unwrap(), Proof::Problems(_)));
        assert!(rmi(&d), "{:?}", d.calls());
        // Err: the task is not running when the check runs. No learner is told to resume.
        let d = FakeDocker::new(|l| {
            if l.starts_with("inspect -f {{.State.Running}}") {
                ok("false")
            } else {
                ok("")
            }
        });
        let (r, _) = prover_with(&root, &d, |pr| pr.prove(&p, 1));
        let shown = format!("{r:?}");
        assert!(!shown.contains("/scenario resume"), "{shown}");
        assert!(rmi(&d), "{:?}", d.calls());
        // Passed: the image stays so the engine does not build twice.
        let n = std::cell::Cell::new(0);
        let d2 = FakeDocker::new(move |l| {
            if l.starts_with("ps ") {
                return ok("");
            }
            if l.contains(" sh /opt/.usta-check/check.sh") {
                n.set(n.get() + 1);
                // Two fresh runs fail, the one after the solution passes.
                return if n.get() <= 2 {
                    dfail(1, "not yet")
                } else {
                    ok("")
                };
            }
            ok("true")
        });
        let (r, _) = prover_with(&root, &d2, |pr| pr.prove(&p, 2));
        assert_eq!(r.unwrap(), Proof::Passed, "{:?}", d2.calls());
        assert!(!rmi(&d2), "{:?}", d2.calls());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_failed_clean_keeps_the_record_for_the_sweep() {
        let root = tmp("val_keep_rec");
        let p = as_generated(Package::load(&write_pkg(&root)).unwrap());
        let d = FakeDocker::new(|l| {
            if l.starts_with("ps ") {
                ok("")
            } else if l.starts_with("rm ") {
                dfail(1, "permission denied")
            } else if l.contains(" sh /opt/.usta-check/check.sh") {
                dfail(1, "not yet")
            } else {
                ok("true")
            }
        });
        let (r, store) = prover_with(&root, &d, |pr| pr.prove(&p, 0));
        assert!(r.is_ok());
        assert_eq!(store.all().len(), 1, "the record stays");
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
        let p = as_generated(Package::load(&write_pkg(&root)).unwrap());
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
        let p =
            as_generated(Package::load(&crate::scenario::lint::tests::folder_pkg(&root)).unwrap());
        let store = Store::new(&root);
        let d = FakeDocker::new(|_| ok(""));
        let alive: &dyn Fn(u32) -> bool = &|_| true;
        let tick = Tick {
            pid: 1,
            alive,
            stamp: "s",
            today: "2026-11-04",
        };
        let m = FakeModel::with(&[OK_REVIEW, &all_met(&p), &all_met(&p)]);
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
        let p = as_generated(Package::load(&write_pkg(&root)).unwrap());
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
