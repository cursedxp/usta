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

pub(crate) fn build(d: &dyn Docker, pkg: &Package, image: &str, tag: &str) -> Result<()> {
    let ctx = pkg.dir.join(&pkg.environment.context);
    let text = std::fs::read_to_string(ctx.join("Dockerfile"))
        .map_err(|e| internal(format!("no recipe: {e}")))?;
    let recipe = dockerfile::parse(&text).map_err(|p| internal(p.join("; ")))?;
    ensure_image(d, &recipe.base)?;
    let name = format!("usta-{tag}-bld-{}", pkg.id);
    let result = run_recipe(d, pkg, &recipe, &name, image, tag);
    let _ = d.run(&["rm", "-f", &name]);
    result?;
    let size = must(d, &["image", "inspect", "-f", "{{.Size}}", image])?;
    if size
        .stdout
        .trim()
        .parse::<u64>()
        .is_ok_and(|b| b > MAX_IMAGE_BYTES)
    {
        let _ = d.run(&["rmi", "-f", image]);
        return Err(internal(
            "the task's server is too large; keep it under 2 GB",
        ));
    }
    Ok(())
}

fn run_recipe(
    d: &dyn Docker,
    pkg: &Package,
    recipe: &dockerfile::Recipe,
    name: &str,
    image: &str,
    tag: &str,
) -> Result<()> {
    let _ = d.run(&["rm", "-f", name]); // an earlier crash may have left one
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
    let mut workdir = String::from("/");
    let mut envs: Vec<String> = Vec::new();
    for step in &recipe.steps {
        match step {
            Step::Workdir(w) => {
                must(d, &["exec", name, "mkdir", "-p", w])?;
                workdir = w.clone();
            }
            Step::Env(k, v) => envs.push(format!("{k}={v}")),
            Step::Copy { srcs, dest } => {
                let dir = if dest.ends_with('/') {
                    dest.as_str()
                } else {
                    dest.rsplit_once('/')
                        .map_or("/", |(a, _)| if a.is_empty() { "/" } else { a })
                };
                must(d, &["exec", name, "mkdir", "-p", dir])?;
                for s in srcs {
                    let src = ctx.join(s).to_string_lossy().into_owned();
                    must(d, &["cp", &src, &format!("{name}:{dest}")])?;
                }
            }
            Step::Run(cmd) => {
                let left = BUILD_BUDGET.saturating_sub(begun.elapsed()).as_secs();
                if left == 0 {
                    return Err(internal(
                        "building the task's server took more than 10 minutes",
                    ));
                }
                let secs = left.to_string();
                let mut args: Vec<&str> = vec!["exec", "-w", &workdir];
                for e in &envs {
                    args.extend(["-e", e.as_str()]);
                }
                args.extend([name, "timeout", &secs, "sh", "-c", cmd]);
                must(d, &args)?;
            }
        }
    }
    let wd = format!("WORKDIR {workdir}");
    let env_changes: Vec<String> = envs.iter().map(|e| format!("ENV {e}")).collect();
    let mut args: Vec<&str> = vec!["commit", "--change", &wd];
    for c in &env_changes {
        args.extend(["--change", c.as_str()]);
    }
    args.extend([name, image]);
    must(d, &args).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::docker::fake::{fail, ok, FakeDocker};
    use crate::scenario::env::Internal;
    use crate::scenario::package::tests::{tmp, write_pkg};

    #[test]
    fn the_recipe_runs_inside_limits_and_is_committed() {
        let root = tmp("lb_ok");
        let mut p = Package::load(&write_pkg(&root)).unwrap();
        p.id = "gen-x-1".into();
        std::fs::write(
            p.dir.join("environment/Dockerfile"),
            "FROM debian:12-slim\nWORKDIR /app\nENV A=1\nRUN make it\nCOPY setup.sh /opt/\n",
        )
        .unwrap();
        std::fs::write(p.dir.join("environment/setup.sh"), "true\n").unwrap();
        let d = FakeDocker::new(|l| {
            if l.starts_with("image inspect -f") {
                ok("1000\n")
            } else if l.starts_with("image inspect") {
                fail(1, "No such image")
            } else {
                ok("")
            }
        });
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
        ] {
            assert!(create.contains(flag), "{flag}: {create}");
        }
        assert!(c[find("make it")].contains(&format!("exec -w /app -e A=1 {name} timeout ")));
        assert!(find("mkdir -p /opt/") < find("/environment/setup.sh"));
        assert!(find("commit") > find("make it"));
        assert!(c[find("commit")].contains("--change WORKDIR /app"));
        assert!(
            c.iter()
                .rposition(|l| *l == format!("rm -f {name}"))
                .unwrap()
                > find("commit")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_failing_step_or_an_oversized_result_removes_what_it_made() {
        let root = tmp("lb_bad");
        let mut p = Package::load(&write_pkg(&root)).unwrap();
        p.id = "gen-x-1".into();
        std::fs::write(
            p.dir.join("environment/Dockerfile"),
            "FROM debian:12-slim\nRUN boom\n",
        )
        .unwrap();
        let d = FakeDocker::new(|l| {
            if l.contains("boom") {
                fail(2, "no such thing")
            } else {
                ok("")
            }
        });
        let e = build(&d, &p, "i:v1", "t").unwrap_err();
        assert!(e.downcast_ref::<Internal>().is_some() && e.to_string().contains("no such thing"));
        assert!(d.calls().last().unwrap().starts_with("rm -f"));
        let d = FakeDocker::new(|l| {
            if l.starts_with("image inspect -f") {
                ok("99999999999\n")
            } else {
                ok("")
            }
        });
        std::fs::write(
            p.dir.join("environment/Dockerfile"),
            "FROM debian:12-slim\n",
        )
        .unwrap();
        assert!(build(&d, &p, "i:v1", "t")
            .unwrap_err()
            .to_string()
            .contains("too large"));
        assert!(d.calls().iter().any(|l| l == "rmi -f i:v1"));
        let _ = std::fs::remove_dir_all(root);
    }
}
