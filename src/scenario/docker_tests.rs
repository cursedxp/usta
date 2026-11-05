//! Real-Docker tests. `#[ignore]`d so the default `cargo test` (and CI) never
//! needs Docker. Run on a machine with Docker:
//!   cargo test scenario::docker_tests -- --ignored --test-threads=1
//! They touch only containers named `usta-cert0000-*`.

use crate::scenario::container::ContainerEnv;
use crate::scenario::docker::{Docker, RealDocker};
use crate::scenario::env::{CheckOutcome, Environment};
use crate::scenario::folder::FolderEnv;
use crate::scenario::model::NoModel;
use crate::scenario::package::tests::tmp;
use crate::scenario::package::Package;
use crate::scenario::validate::{prove_script, Proof};

const TAG: &str = "cert0000";

/// Removes the test container even when an assertion panics.
struct Cleanup(String);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = RealDocker.run(&["rm", "-f", &self.0]);
    }
}

/// Removes a test image, and the build's temporary tag, even when an assertion panics.
struct ImageCleanup(String);
impl Drop for ImageCleanup {
    fn drop(&mut self) {
        let _ = RealDocker.run(&["rmi", "-f", &self.0]);
        let _ = RealDocker.run(&["rmi", "-f", &format!("{}-bld", self.0)]);
    }
}

fn is_fail(o: &CheckOutcome) -> bool {
    matches!(o, CheckOutcome::Fail { .. })
}

/// A container task whose start script appends a line to a log, so a restart is countable.
fn lifecycle_container(root: &std::path::Path) -> Package {
    let d = root.join("gen-cert-3");
    for (f, c) in [
        ("task.toml", "id = \"gen-cert-3\"\ntopic = \"cert\"\ntitle = \"T\"\nversion = 1\nskills = [\"x\"]\n\n[environment]\nkind = \"container\"\nstart = \"/opt/x/start.sh\"\n\n[check]\nkind = \"script\"\nscript = \"check.sh\"\n"),
        ("instruction.md", "Make /root/done exist.\n"),
        ("tone/adventure.md", "## open\nGo.\n\n## close\nDone.\n"),
        ("environment/Dockerfile", "FROM debian:12-slim\nCOPY start.sh /opt/x/\nWORKDIR /root\n"),
        ("environment/start.sh", "echo up >> /var/log/starts.log\n"),
        ("solution/solve.sh", "touch /root/done\n"),
        ("tests/check.sh", "[ -f /root/done ] || exit 1\nexit 0\n"),
    ] {
        std::fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
        std::fs::write(d.join(f), c).unwrap();
    }
    Package::load(&d).unwrap()
}

#[test]
#[ignore = "needs Docker"]
fn pause_keeps_files_and_resume_reruns_the_start_script() {
    let root = tmp("cert_lifecycle");
    let pkg = lifecycle_container(&root);
    assert_eq!(pkg.lint(), Vec::<String>::new());
    let d = RealDocker;
    let mut env = ContainerEnv {
        bases: None,
        tasks: std::path::PathBuf::from("/nonexistent-usta-tasks"),
        docker: &d,
        project_tag: TAG.into(),
    };
    let _guard = Cleanup(format!("usta-{TAG}-cert-life"));
    let _image = ImageCleanup(ContainerEnv::image(&pkg));
    let h = env.prepare(&pkg, "cert-life").unwrap();
    let name = h.reference.clone();
    assert!(d
        .run(&["exec", &name, "touch", "/root/marker"])
        .unwrap()
        .ok());
    env.pause(&h).unwrap();
    let st = d
        .run(&["inspect", "-f", "{{.State.Running}}", &name])
        .unwrap();
    assert_eq!(st.stdout.trim(), "false", "paused container must not run");
    assert_eq!(env.resume(&pkg, &h).unwrap(), None);
    assert!(
        d.run(&["exec", &name, "test", "-f", "/root/marker"])
            .unwrap()
            .ok(),
        "files survive a stop"
    );
    let starts = d
        .run(&["exec", &name, "wc", "-l", "/var/log/starts.log"])
        .unwrap();
    assert!(
        starts.stdout.trim_start().starts_with('2'),
        "start script ran at prepare and resume: {}",
        starts.stdout
    );
    env.clean(&h).unwrap();
    let gone = d.run(&["inspect", &name]).unwrap();
    assert!(!gone.ok(), "clean must remove the container");
    let _ = std::fs::remove_dir_all(root);
}

/// A generated package on disk, as staging would hold it.
fn generated_container(root: &std::path::Path) -> Package {
    let d = root.join("gen-cert-1");
    for (f, c) in [
        ("task.toml", "id = \"gen-cert-1\"\ntopic = \"cert\"\ntitle = \"T\"\nversion = 1\nskills = [\"x\"]\n\n[environment]\nkind = \"container\"\n\n[check]\nkind = \"script\"\nscript = \"check.sh\"\n"),
        ("instruction.md", "Make /root/done exist.\n"),
        ("tone/adventure.md", "## open\nGo.\n\n## close\nDone.\n"),
        ("environment/Dockerfile", "FROM debian:12-slim\nRUN mkdir -p /opt/x\nCOPY note.txt /opt/x/\nWORKDIR /root\nENV MODE=fast\n"),
        ("environment/note.txt", "hello\n"),
        ("solution/solve.sh", "touch /root/done\n"),
        ("tests/check.sh", "[ -f /root/done ] || { echo not yet; exit 1; }\n[ \"$(cat /opt/x/note.txt)\" = hello ] || exit 1\nexit 0\n"),
    ] {
        std::fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
        std::fs::write(d.join(f), c).unwrap();
    }
    Package::load(&d).unwrap()
}

#[test]
#[ignore = "needs Docker"]
fn a_generated_recipe_builds_inside_limits_and_is_certified() {
    let root = tmp("cert_gen");
    let pkg = generated_container(&root);
    assert_eq!(pkg.lint(), Vec::<String>::new());
    let d = RealDocker;
    let _guard = Cleanup(format!("usta-{TAG}-cert-gen"));
    let _image = ImageCleanup(ContainerEnv::image(&pkg));
    let mut env = ContainerEnv {
        bases: None,
        tasks: std::path::PathBuf::from("/nonexistent-usta-tasks"),
        docker: &d,
        project_tag: TAG.into(),
    };
    assert_eq!(
        prove_script(&mut env, &pkg, "cert-gen").unwrap(),
        Proof::Passed
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "needs Docker"]
fn a_generated_tasks_image_survives_the_end_of_any_run() {
    let root = tmp("cert_rmi");
    let pkg = generated_container(&root);
    let image = ContainerEnv::image(&pkg);
    let _image = ImageCleanup(image.clone());
    let d = RealDocker;
    let mut env = ContainerEnv {
        bases: None,
        tasks: std::path::PathBuf::from("/nonexistent-usta-tasks"),
        docker: &d,
        project_tag: TAG.into(),
    };
    let here = || {
        d.run(&["image", "inspect", &image])
            .map(|o| o.ok())
            .unwrap_or(false)
    };
    for run in ["val-cert-rmi", "cert-rmi"] {
        let _guard = Cleanup(format!("usta-{TAG}-{run}"));
        let h = env.prepare(&pkg, run).unwrap();
        env.clean(&h).unwrap();
        assert!(here(), "{run}: the image outlives the run");
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "needs Docker"]
fn a_recipe_cannot_exceed_the_build_memory() {
    let root = tmp("cert_mem");
    let pkg = generated_container(&root);
    std::fs::write(
        pkg.dir.join("environment/Dockerfile"),
        "FROM debian:12-slim\nRUN cat /proc/self/cgroup >/dev/null; v=$(cat /sys/fs/cgroup/memory.max); [ \"$v\" = 1073741824 ]\n",
    )
    .unwrap();
    let _image = ImageCleanup(ContainerEnv::image(&pkg));
    crate::scenario::limited_build::build(&RealDocker, &pkg, &ContainerEnv::image(&pkg), TAG, None)
        .unwrap();
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "needs Docker"]
fn a_task_container_runs_under_the_file_size_limit_for_every_exec() {
    let root = tmp("cert_ulimit");
    let pkg = generated_container(&root);
    let d = RealDocker;
    let _guard = Cleanup(format!("usta-{TAG}-cert-ul"));
    let _image = ImageCleanup(ContainerEnv::image(&pkg));
    let mut env = ContainerEnv {
        bases: None,
        tasks: std::path::PathBuf::from("/nonexistent-usta-tasks"),
        docker: &d,
        project_tag: TAG.into(),
    };
    let h = env.prepare(&pkg, "cert-ul").unwrap();
    let out = d
        .run(&["exec", &h.reference, "sh", "-c", "ulimit -f"])
        .unwrap();
    let v = out.stdout.trim();
    assert!(
        v != "unlimited" && v.parse::<u64>().is_ok_and(|n| n > 0),
        "ulimit -f under exec: {v}"
    );
    env.clean(&h).unwrap();
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "needs Docker"]
fn a_folder_script_check_cannot_write_to_the_task_and_has_no_network() {
    let root = tmp("cert_folder");
    let d = root.join("gen-cert-2");
    for (f, c) in [
        ("task.toml", "id = \"gen-cert-2\"\ntopic = \"cert\"\ntitle = \"T\"\nversion = 1\nskills = [\"x\"]\n\n[environment]\nkind = \"folder\"\n\n[check]\nkind = \"script\"\nscript = \"check.sh\"\n"),
        ("instruction.md", "Write plan.md.\n"),
        ("tone/adventure.md", "## open\nGo.\n\n## close\nDone.\n"),
        ("environment/files/brief.md", "Brief.\n"),
        ("solution/files/plan.md", "Plan.\n"),
        ("tests/check.sh", "if touch /task/x 2>/dev/null; then echo wrote; exit 2; fi\nif python3 -c 'import socket; socket.create_connection((\"1.1.1.1\", 53), 3)' 2>/dev/null; then echo online; exit 2; fi\n[ -f /task/plan.md ] || { echo plan.md missing; exit 1; }\nexit 0\n"),
    ] {
        std::fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
        std::fs::write(d.join(f), c).unwrap();
    }
    let pkg = Package::load(&d).unwrap();
    let docker = RealDocker;
    let mut env = FolderEnv {
        bases: None,
        docker: &docker,
        project_tag: TAG.into(),
        tasks: root.join("tasks"),
        model: &NoModel,
    };
    assert_eq!(
        prove_script(&mut env, &pkg, "cert-folder").unwrap(),
        Proof::Passed
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "needs Docker"]
fn a_workspace_is_shared_both_ways_checked_and_moved_to_done() {
    let root = tmp("cert_ws");
    let d = root.join("cert-ws");
    for (f, c) in [
        ("task.toml", "id = \"cert-ws\"\ntopic = \"cert\"\ntitle = \"T\"\nversion = 1\nskills = [\"x\"]\n\n[environment]\nkind = \"container\"\nworkspace = \"/work\"\nsetup = \"/opt/setup.sh\"\n\n[check]\nkind = \"script\"\nscript = \"check.sh\"\n"),
        ("instruction.md", "Write note.txt.\n"),
        ("tone/adventure.md", "## open\nGo.\n\n## close\nDone.\n"),
        ("environment/Dockerfile", "FROM debian:12-slim\nCOPY setup.sh /opt/setup.sh\n"),
        ("environment/setup.sh", "echo x > /work/.hidden\nmkdir /work/.vscode\nln -s /etc/passwd /work/link\nln -s /etc /work/src_link\n"),
        ("environment/files/start.py", "print('hi')\n"),
        ("solution/solve.sh", "echo done > /work/note.txt\n"),
        ("tests/check.sh", "[ \"$(pwd)\" = /work ] || exit 1\ngrep -q done /work/note.txt || exit 1\nexit 0\n"),
    ] {
        std::fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
        std::fs::write(d.join(f), c).unwrap();
    }
    let pkg = Package::load(&d).unwrap();
    assert_eq!(pkg.lint(), Vec::<String>::new());
    let docker = RealDocker;
    let _guard = Cleanup(format!("usta-{TAG}-cert-ws"));
    let _image = ImageCleanup(ContainerEnv::image(&pkg));
    let tasks = root.join("usta-tasks");
    let mut env = ContainerEnv {
        bases: None,
        docker: &docker,
        project_tag: TAG.into(),
        tasks: tasks.clone(),
    };
    let h = env.prepare(&pkg, "cert-ws").unwrap();
    let folder = tasks.join("cert-ws");
    let sh = |cmd: &str| {
        docker
            .run(&["exec", &h.reference, "sh", "-c", cmd])
            .unwrap()
    };
    // The starting file is inside, and the shell starts at the workspace.
    assert_eq!(sh("cat /work/start.py").stdout.trim(), "print('hi')");
    assert_eq!(sh("pwd").stdout.trim(), "/work");
    // What setup left (a hidden file, a hidden folder, links) is gone on the host too.
    for gone in [".hidden", ".vscode", "link", "src_link"] {
        assert!(folder.join(gone).symlink_metadata().is_err(), "{gone}");
    }
    assert!(folder.join("start.py").is_file());
    // Host to container, and container to host.
    std::fs::write(folder.join("from_host.txt"), "host\n").unwrap();
    assert_eq!(sh("cat /work/from_host.txt").stdout.trim(), "host");
    assert!(sh("echo box > /work/from_box.txt").ok());
    assert_eq!(
        std::fs::read_to_string(folder.join("from_box.txt"))
            .unwrap()
            .trim(),
        "box"
    );
    // The check fails until the learner's file exists, then passes.
    assert!(is_fail(&env.check(&pkg, &h).unwrap()));
    std::fs::write(folder.join("note.txt"), "done\n").unwrap();
    assert_eq!(env.check(&pkg, &h).unwrap(), CheckOutcome::Pass);
    // Pause and resume keep the files; clean moves them.
    env.pause(&h).unwrap();
    assert_eq!(env.resume(&pkg, &h).unwrap(), None);
    env.clean(&h).unwrap();
    assert!(!folder.exists());
    let kept = tasks.join("done/cert-ws");
    assert!(kept.join("note.txt").is_file() && kept.join("from_box.txt").is_file());
    assert!(docker
        .run(&["inspect", &h.reference])
        .is_ok_and(|o| !o.ok()));
    let _ = std::fs::remove_dir_all(root);
}
