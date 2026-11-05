//! The folder environment, a simulated world on disk. `prepare` writes data files
//! only, inside the project's task folder; nothing from the package ever runs on
//! the host. A script check runs in a throwaway container with the folder mounted
//! read-only. The reference has the `usta-<tag>-<run>` shape of a container name
//! because the sweep treats any other reference as foreign; the path is derived
//! from it, so a moved project keeps working.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use crate::scenario::container::{ContainerEnv, FILE_LIMIT};
use crate::scenario::docker::Docker;
use crate::scenario::env::{internal, CheckOutcome, EnvHandle, Environment, FOLDER};
use crate::scenario::limited_build::ensure_image;
use crate::scenario::lint::walk;
use crate::scenario::model::Model;
use crate::scenario::package::Package;
use crate::scenario::rubric::{panel, read_work};
use crate::scenario::workspace::mount_arg;

pub(crate) const TASKS_DIR: &str = "usta-tasks";
/// The learner's work is theirs: `clean` moves a task here and never deletes it.
pub(crate) const DONE_DIR: &str = "done";
/// Fixed, so a folder package needs no model-written recipe.
pub(crate) const CHECK_IMAGE: &str = "python:3.12-slim-bookworm";
const TASK_MOUNT: &str = "/task";

pub(crate) struct FolderEnv<'a> {
    pub docker: &'a dyn Docker,
    pub project_tag: String,
    pub tasks: PathBuf,
    pub model: &'a dyn Model,
    pub bases: Option<PathBuf>,
}

/// Copy regular files as plain data: bytes only, so no mode or link survives.
pub(crate) fn copy_data(from: &Path, to: &Path) -> Result<()> {
    let files = walk(from).map_err(|e| anyhow!(e))?;
    for f in files {
        let rel = f.strip_prefix(from).context("file outside its folder")?;
        let dest = to.join(rel);
        std::fs::create_dir_all(dest.parent().context("file has a parent")?)?;
        // A learner's link here must not be written through.
        let _ = std::fs::remove_file(&dest);
        std::fs::write(&dest, std::fs::read(&f)?)?;
    }
    Ok(())
}

/// One plain sentence: the learner may have deleted or moved the task folder.
pub(crate) fn gone_error(run_id: &str) -> anyhow::Error {
    anyhow!(
        "the files for this task are gone, so it cannot be continued; give it up with /scenario drop {run_id} confirm and start it again"
    )
}

pub(crate) fn valid_run_id(id: &str) -> bool {
    !id.is_empty()
        && id != DONE_DIR
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// `done/<run>`, or a numbered sibling: a clean never overwrites earlier work.
fn free_dest(done: &Path, run_id: &str) -> PathBuf {
    let first = done.join(run_id);
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| done.join(format!("{run_id}-{n}")))
        .find(|p| !p.exists())
        .expect("an unused number")
}

/// The learner's folder goes to `done/`, never deleted. Already gone is success.
pub(crate) fn move_to_done(tasks: &Path, dir: &Path, run_id: &str) -> Result<()> {
    match std::fs::symlink_metadata(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
        Ok(_) => {}
    }
    let done = tasks.join(DONE_DIR);
    std::fs::create_dir_all(&done)?;
    let dest = free_dest(&done, run_id);
    std::fs::rename(dir, &dest).with_context(|| format!("cannot move {}", dir.display()))
}

impl FolderEnv<'_> {
    fn reference(&self, run_id: &str) -> String {
        format!("usta-{}-{run_id}", self.project_tag)
    }

    /// The handle comes from a TOML file on disk, so it is checked before any path is built.
    pub fn dir(&self, h: &EnvHandle) -> Result<PathBuf> {
        let run_id = self
            .run_id(h)
            .filter(|id| valid_run_id(id))
            .ok_or_else(|| {
                internal(format!(
                    "refusing a folder that is not this project's: {}",
                    h.reference
                ))
            })?;
        Ok(self.tasks.join(run_id))
    }

    fn run_id<'h>(&self, h: &'h EnvHandle) -> Option<&'h str> {
        h.reference
            .strip_prefix(&format!("usta-{}-", self.project_tag))
    }

    fn check_script(&self, pkg: &Package, run_id: &str, dir: &Path) -> Result<CheckOutcome> {
        let c = ContainerEnv {
            bases: None,
            docker: self.docker,
            project_tag: self.project_tag.clone(),
            tasks: self.tasks.clone(),
        };
        ensure_image(self.docker, CHECK_IMAGE, self.bases.as_deref())?;
        let run_id = format!("chk-{run_id}");
        let name = self.reference(&run_id);
        let _ = self.docker.run(&["rm", "-f", &name]);
        let project = format!("usta.project={}", self.project_tag);
        let run = format!("usta.run={run_id}");
        let pids = pkg.environment.pids.to_string();
        let mount = mount_arg(dir, TASK_MOUNT, true);
        let result = (|| {
            c.must(&[
                "create",
                "--name",
                &name,
                "--label",
                "usta=1",
                "--label",
                &project,
                "--label",
                &run,
                "--memory",
                &pkg.environment.memory,
                "--cpus",
                &pkg.environment.cpus,
                "--pids-limit",
                &pids,
                "--network",
                "none",
                "--security-opt",
                "no-new-privileges",
                "--ulimit",
                FILE_LIMIT,
                "--mount",
                &mount,
                "--workdir",
                TASK_MOUNT,
                CHECK_IMAGE,
                "sleep",
                "600",
            ])?;
            c.must(&["start", &name])?;
            c.run_check(pkg, &name)
        })();
        let _ = self.docker.run(&["rm", "-f", &name]);
        result
    }
}

impl Environment for FolderEnv<'_> {
    fn prepare(&mut self, pkg: &Package, run_id: &str) -> Result<EnvHandle> {
        let h = EnvHandle {
            reference: self.reference(run_id),
            access: String::new(),
            kind: FOLDER.into(),
        };
        let dir = self.dir(&h)?;
        std::fs::create_dir_all(&self.tasks)
            .with_context(|| format!("cannot create {}", self.tasks.display()))?;
        // `create_dir` fails on an existing folder: never write into, or roll back, someone's work.
        std::fs::create_dir(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
        if let Err(e) = copy_data(&pkg.dir.join("environment/files"), &dir) {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
        Ok(h)
    }

    fn pause(&mut self, _: &EnvHandle) -> Result<()> {
        Ok(()) // files stay; the record is the state
    }

    fn resume(&mut self, _: &Package, h: &EnvHandle) -> Result<Option<String>> {
        let dir = self.dir(h)?;
        if !dir.is_dir() {
            return Err(gone_error(self.run_id(h).unwrap_or_default()));
        }
        Ok(None)
    }

    fn check(&mut self, pkg: &Package, h: &EnvHandle) -> Result<CheckOutcome> {
        let dir = self.dir(h)?;
        let run_id = self.run_id(h).unwrap_or_default();
        if !dir.is_dir() {
            return Err(gone_error(run_id));
        }
        if pkg.is_rubric() {
            return Ok(match read_work(&dir, &pkg.check.deliverables)? {
                Err(missing) => CheckOutcome::Fail { output: missing },
                Ok(work) => panel(self.model, pkg, &work)?,
            });
        }
        self.check_script(pkg, run_id, &dir)
    }

    fn clean(&mut self, h: &EnvHandle) -> Result<()> {
        let dir = self.dir(h)?;
        move_to_done(&self.tasks, &dir, self.run_id(h).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::docker::fake::{fail, ok, FakeDocker};
    use crate::scenario::env::Internal;
    use crate::scenario::model::fake::FakeModel;
    use crate::scenario::model::NoModel;
    use crate::scenario::package::tests::tmp;

    fn setup(name: &str) -> (PathBuf, Package) {
        let root = tmp(name);
        let d = crate::scenario::lint::tests::folder_pkg(&root);
        (root.clone(), Package::load(&d).unwrap())
    }

    fn env<'a>(d: &'a FakeDocker, root: &Path) -> FolderEnv<'a> {
        FolderEnv {
            bases: None,
            docker: d,
            project_tag: "aaaa1111".into(),
            tasks: root.join("usta-tasks"),
            model: &NoModel,
        }
    }

    #[test]
    fn prepare_writes_data_files_and_returns_a_reference_the_sweep_accepts() {
        let (root, pkg) = setup("fold_prep");
        let d = FakeDocker::new(|_| ok(""));
        let mut e = env(&d, &root);
        let h = e.prepare(&pkg, "r1").unwrap();
        assert_eq!(h.reference, "usta-aaaa1111-r1");
        assert_eq!(h.kind, "folder");
        assert_eq!(
            std::fs::read_to_string(root.join("usta-tasks/r1/brief.md")).unwrap(),
            "Audience: bakers.\n"
        );
        assert!(d.calls().is_empty(), "no runtime for a folder");
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn prepared_files_are_never_executable() {
        use std::os::unix::fs::PermissionsExt;
        let (root, pkg) = setup("fold_mode");
        let f = pkg.dir.join("environment/files/brief.md");
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        let d = FakeDocker::new(|_| ok(""));
        env(&d, &root).prepare(&pkg, "r1").unwrap();
        let mode = std::fs::metadata(root.join("usta-tasks/r1/brief.md"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pause_keeps_files_resume_needs_them_clean_moves_them_to_done() {
        let (root, pkg) = setup("fold_life");
        let d = FakeDocker::new(|_| ok(""));
        let mut e = env(&d, &root);
        let h = e.prepare(&pkg, "r1").unwrap();
        e.pause(&h).unwrap();
        assert_eq!(e.resume(&pkg, &h).unwrap(), None);
        std::fs::write(root.join("usta-tasks/r1/plan.md"), "my work\n").unwrap();
        e.clean(&h).unwrap();
        assert!(!root.join("usta-tasks/r1").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("usta-tasks/done/r1/plan.md")).unwrap(),
            "my work\n"
        );
        e.clean(&h).unwrap(); // already moved is success
        assert!(root.join("usta-tasks/done/r1/plan.md").is_file());
        assert!(e.resume(&pkg, &h).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_clean_never_overwrites_earlier_work_in_done() {
        let (root, pkg) = setup("fold_keep");
        let d = FakeDocker::new(|_| ok(""));
        let mut e = env(&d, &root);
        std::fs::create_dir_all(root.join("usta-tasks/done/r1")).unwrap();
        std::fs::write(root.join("usta-tasks/done/r1/old.md"), "old\n").unwrap();
        let h = e.prepare(&pkg, "r1").unwrap();
        e.clean(&h).unwrap();
        assert!(root.join("usta-tasks/done/r1/old.md").is_file());
        assert!(root.join("usta-tasks/done/r1-2/brief.md").is_file());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_reference_from_another_project_or_outside_the_folder_is_refused() {
        let (root, _) = setup("fold_owned");
        let d = FakeDocker::new(|_| ok(""));
        let mut e = env(&d, &root);
        for r in [
            "usta-bbbb2222-r1",
            "usta-aaaa1111-../../home",
            "usta-aaaa1111-",
            "usta-aaaa1111-done",
        ] {
            let h = EnvHandle {
                reference: r.into(),
                access: String::new(),
                kind: "folder".into(),
            };
            assert!(e.clean(&h).is_err(), "{r}");
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_script_check_runs_in_a_throwaway_read_only_container() {
        let (root, mut pkg) = setup("fold_script");
        pkg.check.kind = "script".into();
        pkg.check.script = "check.sh".into();
        pkg.check.timeout_secs = 30;
        std::fs::create_dir_all(pkg.dir.join("tests")).unwrap();
        std::fs::write(pkg.dir.join("tests/check.sh"), "exit 1\n").unwrap();
        let d = FakeDocker::new(|l| {
            if l.contains(" timeout ") {
                fail(1, "plan.md is missing")
            } else {
                ok("true")
            }
        });
        let mut e = env(&d, &root);
        let h = e.prepare(&pkg, "r1").unwrap();
        assert_eq!(
            e.check(&pkg, &h).unwrap(),
            CheckOutcome::Fail {
                output: "plan.md is missing".into()
            }
        );
        let c = d.calls();
        let create = c.iter().find(|l| l.starts_with("create")).unwrap();
        assert!(create.contains("--name usta-aaaa1111-chk-r1"), "{create}");
        assert!(
            create.contains("--network none")
                && create.contains("--memory")
                && create.contains("--pids-limit")
                && create.contains("--ulimit fsize=")
        );
        assert!(create.contains(",target=/task,readonly"), "{create}");
        assert!(create.contains("label usta.project=aaaa1111") && create.contains(CHECK_IMAGE));
        assert_eq!(c.last().unwrap(), "rm -f usta-aaaa1111-chk-r1");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_helper_is_removed_even_when_the_check_breaks() {
        let (root, mut pkg) = setup("fold_break");
        pkg.check.kind = "script".into();
        pkg.check.script = "check.sh".into();
        let d = FakeDocker::new(|l| {
            if l.starts_with("start") {
                fail(1, "boom")
            } else {
                ok("true")
            }
        });
        let mut e = env(&d, &root);
        let h = e.prepare(&pkg, "r1").unwrap();
        assert!(e
            .check(&pkg, &h)
            .unwrap_err()
            .downcast_ref::<Internal>()
            .is_some());
        assert_eq!(d.calls().last().unwrap(), "rm -f usta-aaaa1111-chk-r1");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_check_on_gone_files_gives_the_plain_sentence() {
        let (root, mut pkg) = setup("fold_gone");
        pkg.check.kind = "script".into();
        pkg.check.script = "check.sh".into();
        let d = FakeDocker::new(|_| ok("true"));
        let mut e = env(&d, &root);
        let h = e.prepare(&pkg, "r1").unwrap();
        std::fs::remove_dir_all(root.join("usta-tasks/r1")).unwrap();
        let err = e.check(&pkg, &h).unwrap_err();
        assert!(err.downcast_ref::<Internal>().is_none(), "{err}");
        assert!(
            err.to_string().contains("files for this task are gone"),
            "{err}"
        );
        assert!(d.calls().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn prepare_over_an_existing_folder_is_an_error_and_leaves_it_alone() {
        let (root, pkg) = setup("fold_collide");
        let d = FakeDocker::new(|_| ok(""));
        let mut e = env(&d, &root);
        std::fs::create_dir_all(root.join("usta-tasks/r1")).unwrap();
        std::fs::write(root.join("usta-tasks/r1/mine.md"), "mine\n").unwrap();
        assert!(e.prepare(&pkg, "r1").is_err());
        assert!(root.join("usta-tasks/r1/mine.md").is_file());
        assert!(!root.join("usta-tasks/r1/brief.md").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_rubric_check_fails_on_a_missing_deliverable_and_then_asks_the_judge_twice() {
        let (root, pkg) = setup("fold_rubric");
        let d = FakeDocker::new(|_| ok(""));
        let pass = r#"{"criteria":[{"id":"names-audience","met":true,"note":""},{"id":"gives-reason","met":true,"note":""}]}"#;
        let model = FakeModel::with(&[pass, pass]);
        let mut e = env(&d, &root);
        e.model = &model;
        let h = e.prepare(&pkg, "r1").unwrap();
        let CheckOutcome::Fail { output } = e.check(&pkg, &h).unwrap() else {
            panic!()
        };
        assert!(output.contains("plan.md"), "{output}");
        assert!(model.prompts.lock().unwrap().is_empty());
        std::fs::write(
            root.join("usta-tasks/r1/plan.md"),
            "Bakers, because a is cheap.\n",
        )
        .unwrap();
        assert_eq!(e.check(&pkg, &h).unwrap(), CheckOutcome::Pass);
        assert_eq!(model.prompts.lock().unwrap().len(), 2);
        assert!(d.calls().is_empty(), "a rubric check needs no runtime");
        let _ = std::fs::remove_dir_all(root);
    }
}
