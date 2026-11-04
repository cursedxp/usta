//! Real-Docker tests. `#[ignore]`d so the default `cargo test` (and CI) never
//! needs Docker. Run on a machine with Docker:
//!   cargo test scenario::docker_tests -- --ignored --test-threads=1
//! They touch only containers named `usta-cert0000-*`.

use crate::scenario::container::ContainerEnv;
use crate::scenario::docker::{Docker, RealDocker};
use crate::scenario::env::{CheckOutcome, Environment};
use crate::scenario::library;
use crate::scenario::package::tests::tmp;

const TAG: &str = "cert0000";

/// Removes the test container even when an assertion panics.
struct Cleanup(String);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = RealDocker.run(&["rm", "-f", &self.0]);
    }
}

fn is_fail(o: &CheckOutcome) -> bool {
    matches!(o, CheckOutcome::Fail { .. })
}

fn certify(id: &str) {
    let global = tmp(&format!("cert_{id}"));
    library::sync(&global).unwrap();
    let pkg = library::load_all(&global, "linux")
        .unwrap()
        .into_iter()
        .find(|p| p.id == id)
        .expect("package present");
    let d = RealDocker;
    let mut env = ContainerEnv {
        docker: &d,
        project_tag: TAG.into(),
    };
    let _guard = Cleanup(format!("usta-{TAG}-cert-{id}"));
    let h = env.prepare(&pkg, &format!("cert-{id}")).unwrap();
    // Condition 1: on the fresh environment (also the "do nothing" attempt) the check fails.
    let fresh = env.check(&pkg, &h).unwrap();
    assert!(is_fail(&fresh), "{id}: fresh check must fail");
    // Condition 2: after the reference solution, the check passes.
    env.apply_solution(&pkg, &h).unwrap();
    assert_eq!(
        env.check(&pkg, &h).unwrap(),
        CheckOutcome::Pass,
        "{id}: solved check must pass"
    );
    env.clean(&h).unwrap();
    let _ = std::fs::remove_dir_all(global);
}

#[test]
#[ignore = "needs Docker"]
fn backup_permissions_is_certified() {
    certify("linux-backup-permissions");
}

#[test]
#[ignore = "needs Docker"]
fn disk_hog_is_certified() {
    certify("linux-disk-hog");
}

#[test]
#[ignore = "needs Docker"]
fn config_typo_is_certified() {
    certify("linux-config-typo");
}

#[test]
#[ignore = "needs Docker"]
fn pause_keeps_files_and_resume_reruns_the_start_script() {
    let global = tmp("cert_lifecycle");
    library::sync(&global).unwrap();
    let pkg = library::load_all(&global, "linux")
        .unwrap()
        .into_iter()
        .find(|p| p.id == "linux-config-typo")
        .unwrap();
    let d = RealDocker;
    let mut env = ContainerEnv {
        docker: &d,
        project_tag: TAG.into(),
    };
    let _guard = Cleanup(format!("usta-{TAG}-cert-life"));
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
        .run(&["exec", &name, "wc", "-l", "/var/log/shopd/starts.log"])
        .unwrap();
    assert!(
        starts.stdout.trim_start().starts_with('2'),
        "start script ran at prepare and resume: {}",
        starts.stdout
    );
    env.clean(&h).unwrap();
    let gone = d.run(&["inspect", &name]).unwrap();
    assert!(!gone.ok(), "clean must remove the container");
    let _ = std::fs::remove_dir_all(global);
}
