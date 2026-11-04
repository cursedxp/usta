//! Builds a generated package's image under the same kind of limits a task runs
//! under: the recipe's steps run inside a limited container, then it is committed.
//! Network is on here and only here, since installing packages needs it. The
//! helper is unrecorded on purpose: it holds no work, and the sweep removes any
//! unrecorded `usta-` container left by a crash.

use std::time::{Duration, Instant};

use anyhow::Result;

use crate::scenario::container::FILE_LIMIT;
use crate::scenario::docker::{Docker, Out};
use crate::scenario::dockerfile::{self, Step};
use crate::scenario::env::internal;
use crate::scenario::package::Package;

const BUILD_MEMORY: &str = "1g";
const BUILD_CPUS: &str = "1";
const BUILD_PIDS: &str = "256";
const BUILD_BUDGET: Duration = Duration::from_secs(600);
const MAX_IMAGE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

fn must(d: &dyn Docker, args: &[&str]) -> Result<Out> {
    let o = d.run(args)?;
    if o.ok() {
        Ok(o)
    } else {
        Err(internal(format!(
            "docker {} failed: {}",
            args.first().unwrap_or(&""),
            o.text()
        )))
    }
}

/// Fetch `image` unless it is here. `create` is bounded at 60 s, too short for a pull.
pub(crate) fn ensure_image(d: &dyn Docker, image: &str) -> Result<()> {
    if d.run(&["image", "inspect", image])?.ok() {
        return Ok(());
    }
    must(d, &["pull", image]).map(|_| ())
}

/// A number the runtime printed. Unreadable is a failure, never a pass.
fn read_bytes(d: &dyn Docker, args: &[&str]) -> Result<u64> {
    let o = must(d, args)?;
    o.stdout
        .trim()
        .parse::<u64>()
        .map_err(|_| internal(format!("could not read a size from docker: {}", o.text())))
}

fn too_large() -> anyhow::Error {
    internal("the task's server is too large; keep it under 2 GB")
}

/// What the helper has written so far, checked while it can still be stopped.
fn check_disk(d: &dyn Docker, name: &str) -> Result<()> {
    let written = read_bytes(d, &["inspect", "-s", "-f", "{{.SizeRw}}", name])?;
    if written > MAX_IMAGE_BYTES {
        return Err(too_large());
    }
    Ok(())
}

pub(crate) fn build(d: &dyn Docker, pkg: &Package, image: &str, tag: &str) -> Result<()> {
    build_within(d, pkg, image, tag, BUILD_BUDGET)
}

fn build_within(
    d: &dyn Docker,
    pkg: &Package,
    image: &str,
    tag: &str,
    budget: Duration,
) -> Result<()> {
    let ctx = pkg.dir.join(&pkg.environment.context);
    let text = std::fs::read_to_string(ctx.join("Dockerfile"))
        .map_err(|e| internal(format!("no recipe: {e}")))?;
    let recipe = dockerfile::parse(&text).map_err(|p| internal(p.join("; ")))?;
    ensure_image(d, &recipe.base)?;
    let name = format!("usta-{tag}-bld-{}", pkg.id);
    let temp = format!("{image}-bld");
    let result = run_recipe(d, pkg, &recipe, &name, &temp, tag, budget);
    // `-v` takes the helper's anonymous volumes with it.
    let _ = d.run(&["rm", "-fv", &name]);
    let result = result.and_then(|_| publish(d, &temp, image));
    if result.is_err() {
        let _ = d.run(&["rmi", "-f", &temp]);
    }
    result
}

/// The committed image is only given its real name once its size is known and small.
fn publish(d: &dyn Docker, temp: &str, image: &str) -> Result<()> {
    if read_bytes(d, &["image", "inspect", "-f", "{{.Size}}", temp])? > MAX_IMAGE_BYTES {
        return Err(too_large());
    }
    must(d, &["tag", temp, image])?;
    must(d, &["rmi", temp]).map(|_| ())
}

fn run_recipe(
    d: &dyn Docker,
    pkg: &Package,
    recipe: &dockerfile::Recipe,
    name: &str,
    temp: &str,
    tag: &str,
    budget: Duration,
) -> Result<()> {
    let _ = d.run(&["rm", "-fv", name]); // an earlier crash may have left one
    let project = format!("usta.project={tag}");
    let run = format!("usta.run=bld-{}", pkg.id);
    must(
        d,
        &[
            "create",
            "--name",
            name,
            "--label",
            "usta=1",
            "--label",
            &project,
            "--label",
            &run,
            "--memory",
            BUILD_MEMORY,
            "--cpus",
            BUILD_CPUS,
            "--pids-limit",
            BUILD_PIDS,
            "--security-opt",
            "no-new-privileges",
            "--cap-drop",
            "NET_RAW",
            "--ulimit",
            FILE_LIMIT,
            "--entrypoint",
            "",
            &recipe.base,
            "sleep",
            "3600",
        ],
    )?;
    must(d, &["start", name])?;
    let begun = Instant::now();
    let ctx = pkg.dir.join(&pkg.environment.context);
    // None until the recipe sets one: steps then run in the base image's own WORKDIR.
    let mut workdir: Option<String> = None;
    let mut envs: Vec<String> = Vec::new();
    for step in &recipe.steps {
        match step {
            Step::Workdir(w) => {
                must(d, &["exec", "-u", "0", name, "mkdir", "-p", w])?;
                workdir = Some(w.clone());
            }
            Step::Env(k, v) => envs.push(format!("{k}={v}")),
            Step::Copy { srcs, dest } => {
                let dir = if dest.ends_with('/') {
                    dest.as_str()
                } else {
                    dest.rsplit_once('/')
                        .map_or("/", |(a, _)| if a.is_empty() { "/" } else { a })
                };
                must(d, &["exec", "-u", "0", name, "mkdir", "-p", dir])?;
                for s in srcs {
                    let src = ctx.join(s).to_string_lossy().into_owned();
                    must(d, &["cp", &src, &format!("{name}:{dest}")])?;
                }
            }
            Step::Run(cmd) => {
                let left = budget.saturating_sub(begun.elapsed()).as_secs();
                if left == 0 {
                    return Err(internal(
                        "building the task's server took more than 10 minutes",
                    ));
                }
                let secs = left.to_string();
                let mut args: Vec<&str> = vec!["exec", "-u", "0"];
                if let Some(w) = &workdir {
                    args.extend(["-w", w.as_str()]);
                }
                for e in &envs {
                    args.extend(["-e", e.as_str()]);
                }
                args.extend([name, "timeout", "-k", "5", &secs, "sh", "-c", cmd]);
                must(d, &args)?;
                check_disk(d, name)?;
            }
        }
    }
    check_disk(d, name)?;
    let wd = workdir.map(|w| format!("WORKDIR {w}"));
    let env_changes: Vec<String> = envs.iter().map(|e| format!("ENV {e}")).collect();
    let mut args: Vec<&str> = vec!["commit"];
    for c in wd.iter().chain(env_changes.iter()) {
        args.extend(["--change", c.as_str()]);
    }
    args.extend([name, temp]);
    must(d, &args).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::docker::fake::{fail, ok, FakeDocker};
    use crate::scenario::env::Internal;
    use crate::scenario::package::tests::{tmp, write_pkg};

    fn pkg_with(name: &str, recipe: &str) -> (std::path::PathBuf, Package) {
        let root = tmp(name);
        let mut p = Package::load(&write_pkg(&root)).unwrap();
        p.id = "gen-x-1".into();
        std::fs::write(p.dir.join("environment/Dockerfile"), recipe).unwrap();
        (root, p)
    }

    /// Image present or not, sizes small.
    fn small(l: &str) -> crate::scenario::docker::Out {
        if l.starts_with("inspect -s") || l.starts_with("image inspect -f") {
            ok("1000\n")
        } else if l.starts_with("image inspect") {
            fail(1, "No such image")
        } else {
            ok("")
        }
    }

    #[test]
    fn the_recipe_runs_inside_limits_and_is_committed() {
        let (root, p) = pkg_with(
            "lb_ok",
            "FROM debian:12-slim\nWORKDIR /app\nENV A=1\nRUN make it\nCOPY setup.sh /opt/\n",
        );
        std::fs::write(p.dir.join("environment/setup.sh"), "true\n").unwrap();
        let d = FakeDocker::new(small);
        build(&d, &p, "usta-scn-gen-x-1:v1", "aaaa1111").unwrap();
        let c = d.calls();
        let name = "usta-aaaa1111-bld-gen-x-1";
        let find = |needle: &str| {
            c.iter()
                .position(|l| l.contains(needle))
                .unwrap_or_else(|| panic!("{needle}: {c:?}"))
        };
        assert!(find("pull debian:12-slim") < find("create --name"));
        let create = &c[find("create --name")];
        for flag in [
            "--memory 1g",
            "--cpus 1",
            "--pids-limit 256",
            "label usta=1",
            "--ulimit fsize=",
            "no-new-privileges",
            "--cap-drop NET_RAW",
        ] {
            assert!(create.contains(flag), "{flag}: {create}");
        }
        for banned in [" -v ", "--volume", "--privileged", "--cap-add", "--mount"] {
            assert!(!create.contains(banned), "{banned}: {create}");
        }
        assert!(
            c[find("make it")].contains(&format!("exec -u 0 -w /app -e A=1 {name} timeout -k 5 "))
        );
        assert!(find("inspect -s") > find("make it"));
        assert!(find("mkdir -p /opt/") < find("/environment/setup.sh"));
        assert!(find("commit") > find("make it"));
        let commit = &c[find("commit")];
        assert!(commit.contains("--change WORKDIR /app") && commit.contains("--change ENV A=1"));
        assert!(
            commit.ends_with(&format!("{name} usta-scn-gen-x-1:v1-bld")),
            "{commit}"
        );
        assert!(
            c.iter()
                .rposition(|l| *l == format!("rm -fv {name}"))
                .unwrap()
                > find("commit")
        );
        assert!(
            find("tag usta-scn-gen-x-1:v1-bld usta-scn-gen-x-1:v1")
                < find("rmi usta-scn-gen-x-1:v1-bld")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_recipe_without_workdir_uses_the_bases_own() {
        let (root, p) = pkg_with("lb_nowd", "FROM debian:12-slim\nRUN make it\n");
        let d = FakeDocker::new(small);
        build(&d, &p, "i:v1", "t").unwrap();
        let c = d.calls();
        assert!(
            !c.iter().any(|l| l.contains("-w ") || l.contains("WORKDIR")),
            "{c:?}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_failing_step_removes_what_it_made() {
        let (root, p) = pkg_with("lb_bad", "FROM debian:12-slim\nRUN boom\n");
        let d = FakeDocker::new(|l| {
            if l.contains("boom") {
                fail(2, "no such thing")
            } else {
                small(l)
            }
        });
        let e = build(&d, &p, "i:v1", "t").unwrap_err();
        assert!(e.downcast_ref::<Internal>().is_some() && e.to_string().contains("no such thing"));
        let c = d.calls();
        assert!(c.iter().any(|l| l == "rm -fv usta-t-bld-gen-x-1"));
        assert!(c.iter().any(|l| l == "rmi -f i:v1-bld"));
        assert!(!c.iter().any(|l| l.starts_with("tag ")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_oversized_image_never_gets_its_real_name() {
        let (root, p) = pkg_with("lb_big", "FROM debian:12-slim\n");
        let d = FakeDocker::new(|l| {
            if l.starts_with("image inspect -f") {
                ok("99999999999\n")
            } else {
                small(l)
            }
        });
        assert!(build(&d, &p, "i:v1", "t")
            .unwrap_err()
            .to_string()
            .contains("too large"));
        let c = d.calls();
        assert!(c.iter().any(|l| l == "rmi -f i:v1-bld"));
        assert!(!c.iter().any(|l| l.starts_with("tag ")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_unreadable_size_is_a_failure() {
        let (root, p) = pkg_with("lb_unread", "FROM debian:12-slim\n");
        let d = FakeDocker::new(|l| {
            if l.starts_with("image inspect -f") {
                ok("<nil>\n")
            } else {
                small(l)
            }
        });
        assert!(build(&d, &p, "i:v1", "t").is_err());
        assert!(d.calls().iter().any(|l| l == "rmi -f i:v1-bld"));
        assert!(!d.calls().iter().any(|l| l.starts_with("tag ")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_runaway_run_stops_the_build_before_commit() {
        let (root, p) = pkg_with("lb_disk", "FROM debian:12-slim\nRUN fill\n");
        let d = FakeDocker::new(|l| {
            if l.starts_with("inspect -s") {
                ok("99999999999\n")
            } else {
                small(l)
            }
        });
        assert!(build(&d, &p, "i:v1", "t")
            .unwrap_err()
            .to_string()
            .contains("too large"));
        let c = d.calls();
        assert!(!c.iter().any(|l| l.starts_with("commit")));
        assert!(c.iter().any(|l| l == "rm -fv usta-t-bld-gen-x-1"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_exhausted_budget_stops_before_the_next_run() {
        let (root, p) = pkg_with("lb_budget", "FROM debian:12-slim\nRUN slow\n");
        let d = FakeDocker::new(small);
        let e = build_within(&d, &p, "i:v1", "t", Duration::ZERO).unwrap_err();
        assert!(e.to_string().contains("10 minutes"), "{e}");
        let c = d.calls();
        assert!(!c
            .iter()
            .any(|l| l.contains("slow") || l.starts_with("commit")));
        assert!(c.iter().any(|l| l == "rm -fv usta-t-bld-gen-x-1"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_base_already_here_is_not_pulled() {
        let d = FakeDocker::new(|_| ok(""));
        ensure_image(&d, "debian:12-slim").unwrap();
        assert_eq!(d.calls(), vec!["image inspect debian:12-slim".to_string()]);
    }
}
