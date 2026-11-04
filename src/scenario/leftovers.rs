//! What a crash or a quit mid-proof leaves outside the run records: `val-` proof
//! images, `-bld` temporary tags, drafts in `scenarios/staging`, and `validate/`
//! scratch. Only `usta-` names and Usta's own folders are touched, and nothing with a
//! live owner: a second session's sweep must not pull the floor from under a proof.

use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::scenario::docker::{probe, Docker, Runtime};
use crate::scenario::library;
use crate::scenario::store::{RunRecord, Store};
use crate::scenario::sweep::owner_alive;

/// A draft in the shared staging folder may belong to a generation in ANOTHER project,
/// which this project's records cannot show. It is only a leftover once it has been
/// idle longer than any generation can run (the budget is 30 minutes).
pub(crate) const DRAFT_IDLE: Duration = Duration::from_secs(24 * 3600);

/// Generated task images and drafts are named `gen-<topic>-<stamp>`.
const GENERATED: &str = "gen-";
const IMAGE_PREFIX: &str = "usta-scn-";
const BUILD_SUFFIX: &str = "-bld";

/// The folders always, the images only when the runtime answers. Never asks, never
/// reads stdin. `idle_since`: drafts last touched before it have no one waiting on them.
pub(crate) fn sweep_leftovers(
    store: &Store,
    global: &Path,
    docker: &dyn Docker,
    alive: &dyn Fn(u32) -> bool,
    idle_since: SystemTime,
) {
    let records = store.all();
    // A proof's record names its package and is written before anything is built.
    let owned = |package: &str| {
        records
            .iter()
            .any(|r| r.package == package && owner_alive(r, alive))
    };
    sweep_staging(global, &owned, idle_since);
    sweep_scratch(store, &records, alive);
    if matches!(probe(docker), Runtime::Ready) {
        sweep_images(global, docker, &owned);
    }
}

fn sweep_staging(global: &Path, owned: &dyn Fn(&str) -> bool, idle_since: SystemTime) {
    let Ok(dir) = std::fs::read_dir(library::staging_root(global)) else {
        return;
    };
    for e in dir.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| t < idle_since);
        let real_dir = e.file_type().is_ok_and(|t| t.is_dir());
        if real_dir && name.starts_with(GENERATED) && old && !owned(&name) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// `.usta/scenarios/validate` holds only proof folders (`val-` runs).
fn sweep_scratch(store: &Store, records: &[RunRecord], alive: &dyn Fn(u32) -> bool) {
    let in_use = records
        .iter()
        .any(|r| r.run_id.starts_with("val-") && owner_alive(r, alive));
    if !in_use {
        let _ = std::fs::remove_dir_all(store.scratch_dir());
    }
}

/// `usta-scn-<package>:v<N>` is a proof's image (kept once the package is installed);
/// `...:v<N>-bld` is a build's temporary tag. Names that are not Usta's generated
/// ones, including the built-in tasks' images, are never listed for removal.
fn sweep_images(global: &Path, docker: &dyn Docker, owned: &dyn Fn(&str) -> bool) {
    let filter = format!("reference={IMAGE_PREFIX}{GENERATED}*");
    let Ok(out) = docker.run(&[
        "image",
        "ls",
        "--format",
        "{{.Repository}}:{{.Tag}}",
        "--filter",
        &filter,
    ]) else {
        return;
    };
    if !out.ok() {
        return;
    }
    for line in out.stdout.lines().map(str::trim) {
        let Some((repo, tag)) = line.split_once(':') else {
            continue;
        };
        let Some(id) = repo
            .strip_prefix(IMAGE_PREFIX)
            .filter(|id| id.starts_with(GENERATED))
        else {
            continue;
        };
        let kept = owned(id)
            || (!tag.ends_with(BUILD_SUFFIX)
                && (library::library_root(global).join(id).exists()
                    || library::staging_root(global).join(id).exists()));
        if !kept {
            let _ = docker.run(&["rmi", "-f", line]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::docker::fake::{ok, FakeDocker};
    use crate::scenario::package::tests::tmp;
    use crate::scenario::store::{RunRecord, RunState};
    use std::time::Duration;

    const DEAD: &dyn Fn(u32) -> bool = &|_| false;

    fn long_ago() -> SystemTime {
        SystemTime::now() + Duration::from_secs(3600) // everything on disk is older than this
    }

    fn never() -> SystemTime {
        SystemTime::UNIX_EPOCH // nothing on disk is older than this
    }

    fn proof_rec(pkg: &str, owner: u32) -> RunRecord {
        RunRecord {
            run_id: format!("val-{pkg}-r1"),
            package: pkg.into(),
            topic: "linux".into(),
            tone: "plain".into(),
            state: RunState::Preparing,
            owner_pid: owner,
            started: "2026-11-04".into(),
            updated: "2026-11-04".into(),
            reference: String::new(),
            access: String::new(),
            env: "container".into(),
            last_check: None,
        }
    }

    fn listing(lines: &'static str) -> FakeDocker {
        FakeDocker::new(move |l| {
            if l.starts_with("image ls") {
                ok(lines)
            } else {
                ok("27")
            }
        })
    }

    fn rmis(d: &FakeDocker) -> Vec<String> {
        d.calls()
            .into_iter()
            .filter(|c| c.starts_with("rmi"))
            .collect()
    }

    const IMAGES: &str = "usta-scn-gen-a:v1\nusta-scn-gen-a:v1-bld\nusta-scn-gen-lib:v1\n\
usta-scn-gen-stage:v2\nusta-scn-linux:v2\nkiln-db:1\nusta-scn-gen-b:v1-bld\n";

    fn world(name: &str) -> (std::path::PathBuf, std::path::PathBuf, Store) {
        let root = tmp(name);
        let g = root.join("global");
        std::fs::create_dir_all(g.join("scenarios/library/gen-lib")).unwrap();
        std::fs::create_dir_all(g.join("scenarios/staging/gen-stage")).unwrap();
        (root.clone(), g, Store::new(&root))
    }

    #[test]
    fn proof_images_and_build_tags_without_an_owner_are_removed_and_nothing_else() {
        let (root, g, store) = world("lo_images");
        let d = listing(IMAGES);
        sweep_leftovers(&store, &g, &d, DEAD, never());
        let mut got = rmis(&d);
        got.sort();
        assert_eq!(
            got,
            vec![
                "rmi -f usta-scn-gen-a:v1",
                "rmi -f usta-scn-gen-a:v1-bld",
                "rmi -f usta-scn-gen-b:v1-bld",
            ],
            "the library's and the fresh draft's images, built-in ones and other names stay"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_live_proof_keeps_its_image_and_its_build_tag() {
        let (root, g, store) = world("lo_images_live");
        store.save(&proof_rec("gen-a", 4242)).unwrap();
        let d = listing(IMAGES);
        sweep_leftovers(&store, &g, &d, &|p| p == 4242, never());
        let got = rmis(&d);
        assert_eq!(got, vec!["rmi -f usta-scn-gen-b:v1-bld"], "{got:?}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn with_no_runtime_the_images_are_not_asked_about_but_the_folders_are_cleaned() {
        let (root, g, store) = world("lo_nodocker");
        let d = FakeDocker::missing();
        sweep_leftovers(&store, &g, &d, DEAD, long_ago());
        assert!(!g.join("scenarios/staging/gen-stage").exists());
        assert!(g.join("scenarios/library/gen-lib").is_dir());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_old_draft_without_an_owner_goes_and_a_fresh_or_owned_or_foreign_one_stays() {
        let (root, g, store) = world("lo_staging");
        let st = g.join("scenarios/staging");
        std::fs::create_dir_all(st.join("gen-owned")).unwrap();
        std::fs::create_dir_all(st.join("notes")).unwrap();
        store.save(&proof_rec("gen-owned", 4242)).unwrap();
        let d = listing("");
        sweep_leftovers(&store, &g, &d, &|p| p == 4242, long_ago());
        assert!(!st.join("gen-stage").exists(), "old and ownerless");
        assert!(st.join("gen-owned").is_dir(), "a live proof's draft");
        assert!(st.join("notes").is_dir(), "not a name Usta gives");
        let (root2, g2, store2) = world("lo_staging_fresh");
        sweep_leftovers(&store2, &g2, &listing(""), DEAD, never());
        assert!(
            g2.join("scenarios/staging/gen-stage").is_dir(),
            "fresh: maybe another project's draft"
        );
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(root2);
    }

    #[test]
    fn validate_scratch_goes_when_no_live_proof_needs_it() {
        let (root, g, store) = world("lo_scratch");
        let scratch = store.scratch_dir();
        std::fs::create_dir_all(scratch.join("val-gen-a-r1")).unwrap();
        store.save(&proof_rec("gen-a", 4242)).unwrap();
        sweep_leftovers(&store, &g, &listing(""), &|p| p == 4242, never());
        assert!(
            scratch.join("val-gen-a-r1").is_dir(),
            "a live proof is using it"
        );
        sweep_leftovers(&store, &g, &listing(""), DEAD, never());
        assert!(!scratch.exists());
        // The learner's folder next to it is never part of this.
        assert!(store.tasks_dir().parent().unwrap().exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
