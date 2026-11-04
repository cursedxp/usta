//! The container environment: the five operations over the `docker` CLI.
//! Lifecycle (spec): never `--rm`; one task one container; memory/cpu/pids
//! limits; network off unless declared; `usta-` names plus `usta=1` and
//! `usta.project=<tag>` labels so Usta only ever touches its own containers.

use anyhow::{bail, Result};

use crate::scenario::docker::{Docker, Out};
use crate::scenario::env::{internal, CheckOutcome, EnvHandle, Environment};
use crate::scenario::package::Package;

/// Check scripts enter the container only here, at check time, and are removed right after.
pub(crate) const CHECK_DIR: &str = "/opt/.usta-check";
/// Reference solution location, certification only.
#[cfg(test)]
pub(crate) const SOLUTION_DIR: &str = "/opt/.usta-solution";

pub(crate) struct ContainerEnv<'a> {
    pub docker: &'a dyn Docker,
    pub project_tag: String,
}

impl ContainerEnv<'_> {
    fn name(&self, run_id: &str) -> String {
        format!("usta-{}-{run_id}", self.project_tag)
    }

    pub fn image(pkg: &Package) -> String {
        format!("usta-scn-{}:v{}", pkg.id, pkg.version)
    }

    fn must(&self, args: &[&str]) -> Result<Out> {
        let o = self.docker.run(args)?;
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

    /// Refuse while ANOTHER Usta project has a scenario container running.
    /// Our own project's running containers are paused by the engine first.
    fn guard_single(&self) -> Result<()> {
        let o = self.must(&[
            "ps",
            "--filter",
            "label=usta=1",
            "--filter",
            "status=running",
            "--format",
            "{{.Names}}",
        ])?;
        let mine = format!("usta-{}-", self.project_tag);
        if o.stdout
            .lines()
            .map(str::trim)
            .any(|n| !n.is_empty() && !n.starts_with(&mine))
        {
            bail!("another Usta project already has a practice task running; pause it there first");
        }
        Ok(())
    }

    /// The handle comes from a TOML file on disk: only ever act on this project's own containers.
    fn owned(&self, h: &EnvHandle) -> Result<()> {
        if h.reference
            .starts_with(&format!("usta-{}-", self.project_tag))
        {
            Ok(())
        } else {
            Err(internal(format!(
                "refusing to act on a container that is not this project's: {}",
                h.reference
            )))
        }
    }

    fn run_script(&self, name: &str, path: &str) -> Result<()> {
        self.must(&["exec", name, "sh", path]).map(|_| ())
    }

    /// True only when the runtime says the server does not exist ("No such", as
    /// `clean` reads it). An unreachable runtime or any other error is not "gone".
    pub fn is_gone(&self, name: &str) -> bool {
        self.docker
            .run(&["inspect", "-f", "{{.Id}}", name])
            .map(|o| !o.ok() && o.text().contains("No such"))
            .unwrap_or(false)
    }

    pub fn is_running(&self, name: &str) -> bool {
        self.docker
            .run(&["inspect", "-f", "{{.State.Running}}", name])
            .map(|o| o.ok() && o.stdout.trim() == "true")
            .unwrap_or(false)
    }

    /// Certification only (not one of the five operations): run the reference solution inside.
    #[cfg(test)]
    pub fn apply_solution(&self, pkg: &Package, h: &EnvHandle) -> Result<()> {
        let src = format!("{}/.", pkg.dir.join("solution").display());
        let dest = format!("{}:{SOLUTION_DIR}", h.reference);
        self.must(&["cp", &src, &dest])?;
        let run = self.run_script(&h.reference, &format!("{SOLUTION_DIR}/solve.sh"));
        let _ = self
            .docker
            .run(&["exec", &h.reference, "rm", "-rf", SOLUTION_DIR]);
        run
    }

    fn start_and_setup(&self, pkg: &Package, name: &str) -> Result<()> {
        self.must(&["start", name])?;
        if let Some(s) = &pkg.environment.setup {
            self.run_script(name, s)?;
        }
        if let Some(s) = &pkg.environment.start {
            self.run_script(name, s)?;
        }
        Ok(())
    }
}

impl Environment for ContainerEnv<'_> {
    fn prepare(&mut self, pkg: &Package, run_id: &str) -> Result<EnvHandle> {
        self.guard_single()?;
        let image = Self::image(pkg);
        if !self.docker.run(&["image", "inspect", &image])?.ok() {
            let ctx = pkg.dir.join(&pkg.environment.context);
            self.must(&["build", "-t", &image, &ctx.to_string_lossy()])?;
        }
        let name = self.name(run_id);
        let project_label = format!("usta.project={}", self.project_tag);
        let run_label = format!("usta.run={run_id}");
        let pids = pkg.environment.pids.to_string();
        let mut args: Vec<&str> = vec![
            "create",
            "--name",
            &name,
            "--label",
            "usta=1",
            "--label",
            &project_label,
            "--label",
            &run_label,
            "--memory",
            &pkg.environment.memory,
            "--cpus",
            &pkg.environment.cpus,
            "--pids-limit",
            &pids,
        ];
        if !pkg.environment.network {
            args.extend(["--network", "none"]);
        }
        args.extend([image.as_str(), "sleep", "infinity"]);
        self.must(&args)?;
        if let Err(e) = self.start_and_setup(pkg, &name) {
            let _ = self.docker.run(&["rm", "-f", &name]);
            return Err(e);
        }
        Ok(EnvHandle {
            access: format!("docker exec -it {name} bash"),
            reference: name,
        })
    }

    fn pause(&mut self, h: &EnvHandle) -> Result<()> {
        self.owned(h)?;
        self.must(&["stop", "-t", "2", &h.reference]).map(|_| ())
    }

    fn resume(&mut self, pkg: &Package, h: &EnvHandle) -> Result<()> {
        self.owned(h)?;
        self.guard_single()?;
        self.must(&["start", &h.reference])?;
        if let Some(s) = &pkg.environment.start {
            self.run_script(&h.reference, s)?;
        }
        Ok(())
    }

    fn check(&mut self, pkg: &Package, h: &EnvHandle) -> Result<CheckOutcome> {
        self.owned(h)?;
        if !self.is_running(&h.reference) {
            bail!("the task is not running; use /scenario resume");
        }
        let src = format!("{}/.", pkg.dir.join("tests").display());
        let dest = format!("{}:{CHECK_DIR}", h.reference);
        self.must(&["cp", &src, &dest])?;
        let script = format!("{CHECK_DIR}/{}", pkg.check.script);
        let secs = pkg.check.timeout_secs.to_string();
        let run = self
            .docker
            .run(&["exec", &h.reference, "timeout", &secs, "sh", &script]);
        // Asked before cleanup so cleanup stays the last call; only needed on a plain failure.
        let stopped =
            matches!(&run, Ok(o) if !matches!(o.code, 0 | 124) && !self.is_running(&h.reference));
        let _ = self
            .docker
            .run(&["exec", &h.reference, "rm", "-rf", CHECK_DIR]);
        let o = run?;
        Ok(match o.code {
            0 => CheckOutcome::Pass,
            124 => CheckOutcome::Fail {
                output: format!("the check timed out after {secs}s"),
            },
            // 125-127 and 137 are docker/shell/kill failures, negative is a signal: never the learner's.
            c if c < 0 || matches!(c, 125..=127 | 137) => {
                return Err(internal(format!("check run failed ({c}): {}", o.text())))
            }
            // The daemon reports its own errors as exit 1; a stopped task means this was one.
            _ if stopped => {
                return Err(internal(format!(
                    "task stopped during the check: {}",
                    o.text()
                )))
            }
            _ => CheckOutcome::Fail {
                output: tail(&o.text(), 20, 1500),
            },
        })
    }

    fn clean(&mut self, h: &EnvHandle) -> Result<()> {
        self.owned(h)?;
        let o = self.docker.run(&["rm", "-f", &h.reference])?;
        if o.ok() || o.text().contains("No such container") {
            Ok(())
        } else {
            Err(internal(format!("docker rm failed: {}", o.text())))
        }
    }
}

/// A container carrying this project's label.
#[derive(Debug, PartialEq)]
pub(crate) struct Found {
    pub name: String,
    pub state: String,
    pub run_id: String,
}

/// Containers labelled `usta=1` AND this project's tag, running or not. Never anything else.
pub(crate) fn list_labeled(d: &dyn Docker, project_tag: &str) -> Result<Vec<Found>> {
    let project_filter = format!("label=usta.project={project_tag}");
    let o = d.run(&[
        "ps",
        "-a",
        "--filter",
        "label=usta=1",
        "--filter",
        &project_filter,
        "--format",
        "{{.Names}}|{{.State}}|{{.Label \"usta.run\"}}",
    ])?;
    if !o.ok() {
        return Err(internal(format!("docker ps failed: {}", o.text())));
    }
    Ok(o.stdout
        .lines()
        .filter_map(|l| {
            let mut p = l.trim().splitn(3, '|');
            Some(Found {
                name: p.next()?.to_string(),
                state: p.next()?.to_string(),
                run_id: p.next()?.to_string(),
            })
        })
        .collect())
}

/// Last `lines` lines of `text`, then at most `max_chars` trailing characters.
pub(crate) fn tail(text: &str, lines: usize, max_chars: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    let joined = all[all.len().saturating_sub(lines)..].join("\n");
    let n = joined.chars().count();
    joined.chars().skip(n.saturating_sub(max_chars)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::docker::fake::{fail, ok, FakeDocker};
    use crate::scenario::package::{CheckSpec, EnvSpec, StorySpec};
    use std::path::PathBuf;

    fn pkg() -> Package {
        Package {
            id: "demo-pkg".into(),
            topic: "linux".into(),
            title: "Demo".into(),
            version: 3,
            skills: vec!["permission".into()],
            environment: EnvSpec {
                kind: "container".into(),
                context: "environment".into(),
                setup: Some("/usr/local/lib/usta/setup.sh".into()),
                start: Some("/usr/local/lib/usta/start.sh".into()),
                network: false,
                memory: "256m".into(),
                cpus: "0.5".into(),
                pids: 128,
            },
            check: CheckSpec {
                kind: "script".into(),
                script: "check.sh".into(),
                timeout_secs: 30,
            },
            story: StorySpec::default(),
            dir: PathBuf::from("/lib/demo-pkg"),
        }
    }

    fn handle() -> EnvHandle {
        EnvHandle {
            reference: "usta-aaaa1111-demo-1".into(),
            access: "x".into(),
        }
    }

    #[test]
    fn prepare_builds_a_missing_image_creates_with_limits_then_starts_and_runs_scripts() {
        let d = FakeDocker::new(|l| {
            if l.starts_with("image inspect") {
                fail(1, "none")
            } else {
                ok("")
            }
        });
        let mut env = ContainerEnv {
            docker: &d,
            project_tag: "aaaa1111".into(),
        };
        let h = env.prepare(&pkg(), "demo-1").unwrap();
        assert_eq!(h.reference, "usta-aaaa1111-demo-1");
        assert_eq!(h.access, "docker exec -it usta-aaaa1111-demo-1 bash");
        let calls = d.calls();
        let idx = |needle: &str| {
            calls
                .iter()
                .position(|c| c.contains(needle))
                .unwrap_or_else(|| panic!("missing {needle} in {calls:?}"))
        };
        assert!(calls[idx("build")]
            .starts_with("build -t usta-scn-demo-pkg:v3 /lib/demo-pkg/environment"));
        let create = &calls[idx("create")];
        for want in [
            "--name usta-aaaa1111-demo-1",
            "--label usta=1",
            "--label usta.project=aaaa1111",
            "--label usta.run=demo-1",
            "--memory 256m",
            "--cpus 0.5",
            "--pids-limit 128",
            "--network none",
            "usta-scn-demo-pkg:v3 sleep infinity",
        ] {
            assert!(create.contains(want), "{want} not in {create}");
        }
        assert!(idx("create") < idx("start usta-aaaa1111-demo-1"));
        assert!(idx("start usta-aaaa1111-demo-1") < idx("sh /usr/local/lib/usta/setup.sh"));
        assert!(idx("sh /usr/local/lib/usta/setup.sh") < idx("sh /usr/local/lib/usta/start.sh"));
    }

    #[test]
    fn containers_are_never_created_with_rm() {
        let d = FakeDocker::new(|_| ok(""));
        let mut env = ContainerEnv {
            docker: &d,
            project_tag: "aaaa1111".into(),
        };
        env.prepare(&pkg(), "demo-1").unwrap();
        assert!(d.calls().iter().all(|c| !c.contains("--rm")));
    }

    #[test]
    fn existing_image_skips_the_build_and_network_true_omits_none() {
        let d = FakeDocker::new(|_| ok(""));
        let mut env = ContainerEnv {
            docker: &d,
            project_tag: "aaaa1111".into(),
        };
        let mut p = pkg();
        p.environment.network = true;
        env.prepare(&p, "demo-1").unwrap();
        let calls = d.calls();
        assert!(!calls.iter().any(|c| c.starts_with("build")));
        assert!(!calls.iter().any(|c| c.contains("--network")));
    }

    #[test]
    fn prepare_refuses_while_another_project_runs_a_scenario() {
        let d = FakeDocker::new(|l| {
            if l.starts_with("ps ") {
                ok("usta-zzzz9999-other-1\n")
            } else {
                ok("")
            }
        });
        let mut env = ContainerEnv {
            docker: &d,
            project_tag: "aaaa1111".into(),
        };
        let e = env.prepare(&pkg(), "demo-1").unwrap_err().to_string();
        assert!(
            e.contains("another Usta project") && !e.contains("usta-"),
            "{e}"
        );
        assert!(!d.calls().iter().any(|c| c.starts_with("create")));
    }

    #[test]
    fn our_own_running_container_does_not_block() {
        let d = FakeDocker::new(|l| {
            if l.starts_with("ps ") {
                ok("usta-aaaa1111-old-1\n")
            } else {
                ok("")
            }
        });
        let mut env = ContainerEnv {
            docker: &d,
            project_tag: "aaaa1111".into(),
        };
        assert!(env.prepare(&pkg(), "demo-1").is_ok());
    }

    #[test]
    fn a_failing_setup_removes_the_half_built_container() {
        let d = FakeDocker::new(|l| {
            if l.contains("setup.sh") {
                fail(1, "bad")
            } else {
                ok("")
            }
        });
        let mut env = ContainerEnv {
            docker: &d,
            project_tag: "aaaa1111".into(),
        };
        assert!(env.prepare(&pkg(), "demo-1").is_err());
        assert!(d
            .calls()
            .last()
            .unwrap()
            .starts_with("rm -f usta-aaaa1111-demo-1"));
    }

    #[test]
    fn pause_stops_and_resume_starts_then_reruns_start_script_only() {
        let d = FakeDocker::new(|_| ok(""));
        let mut env = ContainerEnv {
            docker: &d,
            project_tag: "aaaa1111".into(),
        };
        env.pause(&handle()).unwrap();
        env.resume(&pkg(), &handle()).unwrap();
        let calls = d.calls();
        assert_eq!(calls[0], "stop -t 2 usta-aaaa1111-demo-1");
        assert!(calls.contains(&"start usta-aaaa1111-demo-1".to_string()));
        assert!(calls
            .iter()
            .any(|c| c.ends_with("sh /usr/local/lib/usta/start.sh")));
        assert!(!calls.iter().any(|c| c.ends_with("setup.sh")));
    }

    fn running(l: &str) -> Option<Out> {
        l.starts_with("inspect").then(|| ok("true\n"))
    }

    #[test]
    fn check_copies_tests_in_runs_with_timeout_and_always_removes_them() {
        let d = FakeDocker::new(|l| running(l).unwrap_or_else(|| ok("")));
        let mut env = ContainerEnv {
            docker: &d,
            project_tag: "aaaa1111".into(),
        };
        let out = env.check(&pkg(), &handle()).unwrap();
        assert_eq!(out, CheckOutcome::Pass);
        let calls = d.calls();
        assert_eq!(
            calls[1],
            "cp /lib/demo-pkg/tests/. usta-aaaa1111-demo-1:/opt/.usta-check"
        );
        assert_eq!(
            calls[2],
            "exec usta-aaaa1111-demo-1 timeout 30 sh /opt/.usta-check/check.sh"
        );
        assert_eq!(
            calls[3],
            "exec usta-aaaa1111-demo-1 rm -rf /opt/.usta-check"
        );
    }

    #[test]
    fn check_maps_exit_codes_and_still_cleans_up() {
        let d = FakeDocker::new(|l| {
            if let Some(o) = running(l) {
                return o;
            }
            if l.contains("check.sh") {
                return Out {
                    code: 1,
                    stdout: "script still fails\n".into(),
                    stderr: String::new(),
                };
            }
            ok("")
        });
        let mut env = ContainerEnv {
            docker: &d,
            project_tag: "aaaa1111".into(),
        };
        assert_eq!(
            env.check(&pkg(), &handle()).unwrap(),
            CheckOutcome::Fail {
                output: "script still fails".into()
            }
        );
        assert!(d
            .calls()
            .last()
            .unwrap()
            .ends_with("rm -rf /opt/.usta-check"));

        let t = FakeDocker::new(|l| {
            if let Some(o) = running(l) {
                return o;
            }
            if l.contains("check.sh") {
                return fail(124, "");
            }
            ok("")
        });
        let mut env = ContainerEnv {
            docker: &t,
            project_tag: "aaaa1111".into(),
        };
        match env.check(&pkg(), &handle()).unwrap() {
            CheckOutcome::Fail { output } => assert!(output.contains("timed out"), "{output}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn check_requires_a_running_container() {
        let d = FakeDocker::new(|l| {
            if l.starts_with("inspect") {
                ok("false\n")
            } else {
                ok("")
            }
        });
        let mut env = ContainerEnv {
            docker: &d,
            project_tag: "aaaa1111".into(),
        };
        let e = env.check(&pkg(), &handle()).unwrap_err().to_string();
        assert!(e.contains("/scenario resume"), "{e}");
        assert!(!d.calls().iter().any(|c| c.starts_with("cp")));
    }

    #[test]
    fn clean_removes_and_treats_missing_as_success() {
        let d = FakeDocker::new(|_| fail(1, "Error: No such container: x"));
        let mut env = ContainerEnv {
            docker: &d,
            project_tag: "aaaa1111".into(),
        };
        assert!(env.clean(&handle()).is_ok());
        let d2 = FakeDocker::new(|_| fail(1, "permission denied"));
        let mut env2 = ContainerEnv {
            docker: &d2,
            project_tag: "aaaa1111".into(),
        };
        assert!(env2.clean(&handle()).is_err());
        assert_eq!(d.calls()[0], "rm -f usta-aaaa1111-demo-1");
    }

    #[test]
    fn list_labeled_filters_by_project_label_only() {
        let d = FakeDocker::new(|_| {
            ok("usta-aaaa1111-a-1|running|a-1\nusta-aaaa1111-b-2|exited|b-2\n")
        });
        let found = list_labeled(&d, "aaaa1111").unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(
            found[1],
            Found {
                name: "usta-aaaa1111-b-2".into(),
                state: "exited".into(),
                run_id: "b-2".into()
            }
        );
        let call = &d.calls()[0];
        assert!(
            call.contains("label=usta=1") && call.contains("label=usta.project=aaaa1111"),
            "{call}"
        );
    }

    #[test]
    fn tail_keeps_last_lines_and_caps_characters() {
        assert_eq!(tail("a\nb\nc\nd", 2, 100), "c\nd");
        assert_eq!(tail("abcdefghij", 5, 4), "ghij");
    }

    #[test]
    fn reserved_exit_codes_are_internal_errors_not_learner_failures() {
        for code in [125, 126, 127, 137, -1] {
            let d = FakeDocker::new(move |l| {
                if let Some(o) = running(l) {
                    return o;
                }
                if l.contains("check.sh") {
                    return fail(code, "daemon trouble");
                }
                ok("")
            });
            let mut env = ContainerEnv {
                docker: &d,
                project_tag: "aaaa1111".into(),
            };
            let e = env.check(&pkg(), &handle());
            assert!(e.is_err(), "code {code} gave {e:?}");
            assert!(d
                .calls()
                .last()
                .unwrap()
                .ends_with("rm -rf /opt/.usta-check"));
        }
    }

    #[test]
    fn a_task_that_stopped_between_calls_is_an_internal_error() {
        let n = std::cell::Cell::new(0);
        let d = FakeDocker::new(move |l| {
            if l.starts_with("inspect") {
                n.set(n.get() + 1);
                return ok(if n.get() == 1 { "true\n" } else { "false\n" });
            }
            if l.contains("check.sh") {
                return fail(1, "Error response from daemon");
            }
            ok("")
        });
        let mut env = ContainerEnv {
            docker: &d,
            project_tag: "aaaa1111".into(),
        };
        assert!(env.check(&pkg(), &handle()).is_err());
    }

    #[test]
    fn foreign_references_are_refused_without_any_docker_call() {
        let d = FakeDocker::new(|_| ok(""));
        let mut env = ContainerEnv {
            docker: &d,
            project_tag: "aaaa1111".into(),
        };
        let foreign = EnvHandle {
            reference: "kiln-x".into(),
            access: "x".into(),
        };
        let other = EnvHandle {
            reference: "usta-zzzz9999-a-1".into(),
            access: "x".into(),
        };
        for h in [&foreign, &other] {
            assert!(env.clean(h).is_err());
            assert!(env.pause(h).is_err());
            assert!(env.resume(&pkg(), h).is_err());
            assert!(env.check(&pkg(), h).is_err());
        }
        assert!(d.calls().is_empty(), "{:?}", d.calls());
    }
}
