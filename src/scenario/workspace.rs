//! A container task with `environment.workspace`: the learner's task folder on the
//! host is bind-mounted at that path inside, so they edit real files in their own
//! editor while build and check run in the container. The folder is made and moved
//! like a folder task's (`folder.rs`); nothing here runs a file from it on the host.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::scenario::container::{ContainerEnv, CHECK_DIR, SOLUTION_DIR};
use crate::scenario::env::{internal, EnvHandle};
use crate::scenario::folder::{copy_data, gone_error, valid_run_id};

/// True for an absolute path of plain components that is not `/` and cannot cover the
/// places Usta puts its own check and solution files.
pub(crate) fn safe_workspace(p: &str) -> bool {
    use std::path::Component;
    let plain = p
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.'));
    let path = Path::new(p);
    plain
        && p.starts_with('/')
        && path.components().count() > 1
        && path
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
        && ![CHECK_DIR, SOLUTION_DIR]
            .iter()
            .any(|d| Path::new(d).starts_with(path) || path.starts_with(d))
}

/// One CSV field for `--mount`: a comma or quote in the path must not split the option.
pub(crate) fn mount_arg(source: &Path, target: &str, readonly: bool) -> String {
    let source = source.display().to_string().replace('"', "\"\"");
    let ro = if readonly { ",readonly" } else { "" };
    format!("type=bind,\"source={source}\",target={target}{ro}")
}

/// Delete every symlink and every hidden entry below `dir`, without following links:
/// an editor runs `.vscode/tasks.json`, and a link could point out of the folder.
pub(crate) fn scrub(dir: &Path) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let kind = e.file_type()?;
        let hidden = e.file_name().to_string_lossy().starts_with('.');
        if kind.is_dir() && !hidden {
            scrub(&e.path())?;
        } else if kind.is_dir() {
            std::fs::remove_dir_all(e.path())?;
        } else if kind.is_symlink() || hidden {
            std::fs::remove_file(e.path())?;
        }
    }
    Ok(())
}

/// A folder this prepare made, removed again after a failure.
pub(crate) fn discard_folder(dir: Option<&Path>) {
    if let Some(d) = dir {
        let _ = std::fs::remove_dir_all(d);
    }
}

fn real_dir(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir())
}

impl ContainerEnv<'_> {
    /// `<tasks>/<run_id>` with the starting files, new: never over someone's work.
    pub(crate) fn new_folder(
        &self,
        pkg: &crate::scenario::package::Package,
        run_id: &str,
    ) -> Result<PathBuf> {
        if !valid_run_id(run_id) {
            return Err(internal(format!("refusing a folder named {run_id}")));
        }
        let dir = self.tasks.join(run_id);
        std::fs::create_dir_all(&self.tasks)
            .with_context(|| format!("cannot create {}", self.tasks.display()))?;
        std::fs::create_dir(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
        if let Err(e) = copy_data(&pkg.dir.join("environment/files"), &dir) {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
        Ok(dir)
    }

    /// The handle comes from disk: its folder only exists when the id is a valid one.
    pub(crate) fn folder_of(&self, h: &EnvHandle) -> Option<(PathBuf, String)> {
        let id = h
            .reference
            .strip_prefix(&format!("usta-{}-", self.project_tag))
            .filter(|id| valid_run_id(id))?;
        let dir = self.tasks.join(id);
        std::fs::symlink_metadata(&dir)
            .is_ok()
            .then(|| (dir, id.to_string()))
    }

    /// The folder must still be there: Docker would refuse to start with a missing source.
    pub(crate) fn workspace_present(&self, h: &EnvHandle) -> Result<()> {
        let id = h
            .reference
            .strip_prefix(&format!("usta-{}-", self.project_tag))
            .unwrap_or_default();
        match self.folder_of(h) {
            Some((dir, _)) if real_dir(&dir) => Ok(()),
            _ => Err(gone_error(id)),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::scenario::docker::fake::{fail, ok, FakeDocker};
    use crate::scenario::env::Environment;
    use crate::scenario::package::tests::{tmp, write_pkg};
    use crate::scenario::package::Package;

    const TAG: &str = "aaaa1111";

    /// A workspace package: `write_pkg` plus a workspace and one starting code file.
    pub(crate) fn ws_pkg(root: &Path) -> Package {
        let d = write_pkg(root);
        let toml = std::fs::read_to_string(d.join("task.toml")).unwrap();
        let toml = toml.replace("[environment]\n", "[environment]\nworkspace = \"/work\"\n");
        std::fs::write(d.join("task.toml"), toml).unwrap();
        std::fs::create_dir_all(d.join("environment/files/src")).unwrap();
        std::fs::write(d.join("environment/files/src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(d.join("environment/files/run.sh"), "#!/bin/sh\n").unwrap();
        Package::load(&d).unwrap()
    }

    fn env<'a>(d: &'a FakeDocker, root: &Path) -> ContainerEnv<'a> {
        ContainerEnv {
            docker: d,
            project_tag: TAG.into(),
            tasks: root.join("usta-tasks"),
        }
    }

    fn handle(id: &str) -> EnvHandle {
        EnvHandle {
            reference: format!("usta-{TAG}-{id}"),
            access: String::new(),
            kind: "container".into(),
        }
    }

    #[test]
    fn lint_takes_code_files_and_refuses_bad_workspace_paths() {
        let root = tmp("ws_lint");
        let mut p = ws_pkg(&root);
        assert_eq!(p.lint(), Vec::<String>::new());
        for bad in [
            "work",
            "/",
            "/a/../b",
            "/a b",
            "/a;b",
            "/opt",
            "/opt/.usta-check/x",
            "/a\"b",
        ] {
            p.environment.workspace = Some(bad.into());
            assert!(
                p.lint().iter().any(|l| l.contains("environment.workspace")),
                "{bad}"
            );
        }
        p.environment.workspace = Some("/work".into());
        std::fs::create_dir_all(p.dir.join("environment/files/.vscode")).unwrap();
        std::fs::write(p.dir.join("environment/files/.vscode/tasks.json"), "{}").unwrap();
        assert!(p.lint().iter().any(|l| l.contains("tasks.json")));
        std::fs::remove_dir_all(p.dir.join("environment/files")).unwrap();
        assert!(p.lint().iter().any(|l| l.contains("starting files")));
        p.environment.kind = "folder".into();
        assert!(p.lint().iter().any(|l| l.contains("container tasks only")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn prepare_makes_the_folder_mounts_it_and_sets_the_workdir() {
        let root = tmp("ws_prep");
        let pkg = ws_pkg(&root);
        let d = FakeDocker::new(|_| ok(""));
        let h = env(&d, &root).prepare(&pkg, "r1").unwrap();
        let dir = root.join("usta-tasks/r1");
        assert_eq!(
            std::fs::read_to_string(dir.join("src/main.rs")).unwrap(),
            "fn main() {}\n"
        );
        let calls = d.calls();
        let create = calls.iter().find(|c| c.starts_with("create")).unwrap();
        assert!(
            create.contains(&format!(
                "--mount type=bind,\"source={}\",target=/work --workdir /work",
                dir.display()
            )),
            "{create}"
        );
        assert!(!create.contains("readonly"));
        assert_eq!(h.access, "docker exec -it usta-aaaa1111-r1 bash");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_task_without_workspace_gets_no_mount_and_no_folder() {
        let root = tmp("ws_none");
        let mut pkg = ws_pkg(&root);
        pkg.environment.workspace = None;
        let d = FakeDocker::new(|_| ok(""));
        env(&d, &root).prepare(&pkg, "r1").unwrap();
        assert!(!d.calls().iter().any(|c| c.contains("--mount")));
        assert!(!root.join("usta-tasks").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_quote_or_comma_in_the_path_stays_one_csv_field() {
        let root = tmp("ws_csv").join("a,b\"c");
        let pkg = ws_pkg(&root);
        let d = FakeDocker::new(|_| ok(""));
        env(&d, &root).prepare(&pkg, "r1").unwrap();
        let calls = d.calls();
        let create = calls.iter().find(|c| c.starts_with("create")).unwrap();
        assert!(
            create.contains("\"source=")
                && create.contains("a,b\"\"c/usta-tasks/r1\",target=/work"),
            "{create}"
        );
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn an_existing_folder_is_never_written_into_or_removed() {
        let root = tmp("ws_exists");
        let pkg = ws_pkg(&root);
        let dir = root.join("usta-tasks/r1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mine.rs"), "mine").unwrap();
        let d = FakeDocker::new(|_| ok(""));
        assert!(env(&d, &root).prepare(&pkg, "r1").is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join("mine.rs")).unwrap(),
            "mine"
        );
        assert!(!dir.join("src").exists());
        assert!(!d.calls().iter().any(|c| c.starts_with("create")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_failed_create_or_setup_leaves_no_folder_and_no_container() {
        for stage in ["create ", "exec "] {
            let root = tmp("ws_fail");
            let pkg = ws_pkg(&root);
            let d = FakeDocker::new(move |l| {
                if l.starts_with(stage) {
                    fail(1, "boom")
                } else {
                    ok("")
                }
            });
            assert!(env(&d, &root).prepare(&pkg, "r1").is_err());
            assert!(!root.join("usta-tasks/r1").exists(), "{stage}");
            if stage == "exec " {
                assert!(d.calls().iter().any(|c| c.starts_with("rm -f usta-")));
            }
            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[cfg(unix)]
    #[test]
    fn what_setup_leaves_is_scrubbed_links_and_hidden_entries_only() {
        let root = tmp("ws_scrub");
        let pkg = ws_pkg(&root);
        let outside = root.join("outside.txt");
        std::fs::write(&outside, "secret").unwrap();
        let dir = root.join("usta-tasks/r1");
        let (dir2, outside2) = (dir.clone(), outside.clone());
        let d = FakeDocker::new(move |l| {
            if l.starts_with("exec ") {
                std::os::unix::fs::symlink(&outside2, dir2.join("src/link")).unwrap();
                std::fs::create_dir_all(dir2.join(".vscode")).unwrap();
                std::fs::write(dir2.join(".vscode/tasks.json"), "{}").unwrap();
                std::fs::write(dir2.join("src/.env"), "x").unwrap();
            }
            ok("")
        });
        env(&d, &root).prepare(&pkg, "r1").unwrap();
        assert!(dir.join("src/main.rs").is_file());
        assert!(dir.join("run.sh").is_file());
        for gone in ["src/link", ".vscode", "src/.env"] {
            assert!(dir.join(gone).symlink_metadata().is_err(), "{gone}");
        }
        assert_eq!(std::fs::read_to_string(outside).unwrap(), "secret");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resume_with_the_folder_gone_says_so_and_never_starts_the_container() {
        let root = tmp("ws_resume");
        let pkg = ws_pkg(&root);
        let d = FakeDocker::new(|_| ok(""));
        let mut e = env(&d, &root);
        let h = e.prepare(&pkg, "r1").unwrap();
        d.calls.borrow_mut().clear();
        assert_eq!(e.resume(&pkg, &h).unwrap(), None);
        assert!(d.calls().iter().any(|c| c.starts_with("start ")));
        std::fs::remove_dir_all(root.join("usta-tasks/r1")).unwrap();
        d.calls.borrow_mut().clear();
        let err = e.resume(&pkg, &h).unwrap_err().to_string();
        assert!(err.contains("files for this task are gone") && err.contains("drop r1 confirm"));
        assert!(d.calls().is_empty(), "{:?}", d.calls());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn clean_removes_the_container_then_moves_the_folder_without_overwriting() {
        let root = tmp("ws_clean");
        let pkg = ws_pkg(&root);
        let d = FakeDocker::new(|_| ok(""));
        let mut e = env(&d, &root);
        std::fs::create_dir_all(root.join("usta-tasks/done/r1")).unwrap();
        std::fs::write(root.join("usta-tasks/done/r1/old.rs"), "old").unwrap();
        let h = e.prepare(&pkg, "r1").unwrap();
        std::fs::write(root.join("usta-tasks/r1/mine.rs"), "mine").unwrap();
        d.calls.borrow_mut().clear();
        e.clean(&h).unwrap();
        assert_eq!(d.calls(), vec!["rm -f usta-aaaa1111-r1".to_string()]);
        assert!(!root.join("usta-tasks/r1").exists());
        assert!(root.join("usta-tasks/done/r1/old.rs").is_file());
        assert!(root.join("usta-tasks/done/r1-2/mine.rs").is_file());
        e.clean(&h).unwrap(); // moved already is success
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_failed_rm_keeps_the_folder_in_place_for_a_retry() {
        let root = tmp("ws_clean_fail");
        let pkg = ws_pkg(&root);
        let d = FakeDocker::new(|l| {
            if l.starts_with("rm ") {
                fail(1, "busy")
            } else {
                ok("")
            }
        });
        let mut e = env(&d, &root);
        let h = e.prepare(&pkg, "r1").unwrap();
        assert!(e.clean(&h).is_err());
        assert!(root.join("usta-tasks/r1/src/main.rs").is_file());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_reference_from_another_project_touches_neither_docker_nor_a_folder() {
        let root = tmp("ws_foreign");
        let d = FakeDocker::new(|_| ok(""));
        let mut e = env(&d, &root);
        std::fs::create_dir_all(root.join("usta-tasks/r1")).unwrap();
        let mut h = handle("r1");
        h.reference = "usta-bbbb2222-r1".into();
        assert!(e.clean(&h).is_err());
        assert!(d.calls().is_empty());
        assert!(root.join("usta-tasks/r1").is_dir());
        // An odd id from disk reaches no path either.
        let mut odd = handle("x");
        odd.reference = format!("usta-{TAG}-../x");
        assert!(e.folder_of(&odd).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_container_run_without_a_folder_cleans_exactly_as_before() {
        let root = tmp("ws_plain_clean");
        let d = FakeDocker::new(|_| ok(""));
        env(&d, &root).clean(&handle("r1")).unwrap();
        assert_eq!(d.calls().len(), 1);
        assert!(!root.join("usta-tasks").exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
