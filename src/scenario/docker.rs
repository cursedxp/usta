//! Thin seam over the `docker` CLI so every command line is testable without Docker.

use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{bail, Context, Result};

pub(crate) struct Out {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Out {
    pub fn ok(&self) -> bool {
        self.code == 0
    }

    /// stdout then stderr, trimmed; for error messages and check output.
    pub fn text(&self) -> String {
        format!("{}{}", self.stdout, self.stderr).trim().to_string()
    }
}

pub(crate) trait Docker {
    fn run(&self, args: &[&str]) -> Result<Out>;
}

pub(crate) struct RealDocker;

impl Docker for RealDocker {
    fn run(&self, args: &[&str]) -> Result<Out> {
        let o = Command::new("docker")
            .args(args)
            .output()
            .context("could not run the docker command")?;
        Ok(Out {
            code: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        })
    }
}

pub(crate) enum Runtime {
    Ready,
    /// The CLI exists but the engine does not answer.
    Stopped,
    /// No docker CLI at all.
    NotInstalled,
}

pub(crate) fn probe(d: &dyn Docker) -> Runtime {
    match d.run(&["version", "--format", "{{.Server.Version}}"]) {
        Err(_) => Runtime::NotInstalled,
        Ok(o) if o.ok() => Runtime::Ready,
        Ok(_) => Runtime::Stopped,
    }
}

// Docker is named below on purpose: the learner cannot install or open it
// without the name. Everywhere else learner text says "task" or "server".
const NOT_INSTALLED: &str = "Practice tasks run on a small server that Usta starts for you, and that needs Docker. Docker is not installed on this computer. Install Docker Desktop from https://docs.docker.com/get-docker/ (on Linux, install your distribution's docker package), open it once, then run /scenario start again.";
const CANNOT_START: &str = "Usta needs Docker and it is installed, but Usta could not start it. Open Docker yourself, wait until it says it is running, then try /scenario start again.";
const START_TIMEOUT_MSG: &str = "Docker did not finish starting within 90 seconds. Wait until it says it is running, then try /scenario start again.";

/// The runtime must already be running (used by check, pause and the startup sweep).
pub(crate) fn available(d: &dyn Docker) -> Result<()> {
    match probe(d) {
        Runtime::Ready => Ok(()),
        Runtime::Stopped => bail!("Docker is not running. Run /scenario start, which starts it for you, or open Docker yourself."),
        Runtime::NotInstalled => bail!(NOT_INSTALLED),
    }
}

/// Tries to launch the stopped runtime. True = a launch was attempted.
pub(crate) trait RuntimeStarter {
    fn start(&self) -> bool;
}

pub(crate) const START_TIMEOUT: Duration = Duration::from_secs(90);
pub(crate) const START_POLL: Duration = Duration::from_secs(2);

pub(crate) struct SystemStarter;

/// Per-platform launch command. Unverified on real systems (plan, constraint 6);
/// a failure here only produces the plain "could not start it" message.
fn start_command() -> (&'static str, Vec<&'static str>) {
    if cfg!(target_os = "macos") {
        ("open", vec!["-a", "Docker"])
    } else if cfg!(target_os = "windows") {
        (
            "cmd",
            vec![
                "/c",
                "start",
                "",
                "C:\\Program Files\\Docker\\Docker\\Docker Desktop.exe",
            ],
        )
    } else {
        ("systemctl", vec!["start", "docker"])
    }
}

impl RuntimeStarter for SystemStarter {
    fn start(&self) -> bool {
        let (program, args) = start_command();
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
    }
}

/// Make the runtime ready: nothing to do if it runs, start it and wait (up to
/// `timeout`, polling every `poll`) if it is installed but stopped, install
/// guidance if it is missing. `sleep` is injected so tests do not wait.
pub(crate) fn ensure_ready(
    d: &dyn Docker,
    s: &dyn RuntimeStarter,
    timeout: Duration,
    poll: Duration,
    sleep: &dyn Fn(Duration),
) -> Result<()> {
    match probe(d) {
        Runtime::Ready => return Ok(()),
        Runtime::NotInstalled => bail!(NOT_INSTALLED),
        Runtime::Stopped => {}
    }
    if !s.start() {
        bail!(CANNOT_START);
    }
    let mut waited = Duration::ZERO;
    while waited < timeout {
        sleep(poll);
        waited += poll;
        if matches!(probe(d), Runtime::Ready) {
            return Ok(());
        }
    }
    bail!(START_TIMEOUT_MSG)
}

/// Whether a runc version predates the November 2025 container-escape fixes
/// (CVE-2025-31133, CVE-2025-52565, CVE-2025-52881). None = unparseable.
/// Fixed releases per branch were verified in Task 6 Step 0.
pub(crate) fn runc_is_old(version: &str) -> Option<bool> {
    let v = version.trim().trim_start_matches('v');
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (v, None),
    };
    let mut it = core.split('.').map(|n| n.parse::<u32>().ok());
    let (maj, min, pat) = (it.next()??, it.next()??, it.next()??);
    Some(match (maj, min) {
        (0, _) | (1, 0) | (1, 1) => true,
        (1, 2) => pat < 8,
        (1, 3) => pat < 3,
        (1, 4) => {
            pat == 0
                && pre.is_some_and(|p| {
                    p.strip_prefix("rc.")
                        .and_then(|n| n.parse::<u32>().ok())
                        .is_none_or(|n| n < 3)
                })
        }
        _ => false,
    })
}

/// Warning text when the engine's runc is old; None when fine or unknown.
pub(crate) fn runc_warning(d: &dyn Docker) -> Option<String> {
    let o = d
        .run(&["version", "--format", "{{json .Server.Components}}"])
        .ok()
        .filter(Out::ok)?;
    let comps: serde_json::Value = serde_json::from_str(o.stdout.trim()).ok()?;
    let v = comps.as_array()?.iter().find(|c| c["Name"] == "runc")?["Version"].as_str()?;
    runc_is_old(v)?.then(|| {
        format!(
            "The software that runs your practice tasks (Docker, runc {v}) is older than the November 2025 security fixes (CVE-2025-31133, CVE-2025-52565, CVE-2025-52881). Update Docker before you continue."
        )
    })
}

#[cfg(test)]
pub(crate) mod fake {
    use std::cell::RefCell;

    use super::*;

    /// Records every call as one joined line and answers from a closure.
    pub(crate) struct FakeDocker {
        pub calls: RefCell<Vec<String>>,
        respond: Box<dyn Fn(&str) -> Out>,
        missing: bool,
    }

    impl FakeDocker {
        pub fn new(respond: impl Fn(&str) -> Out + 'static) -> FakeDocker {
            FakeDocker {
                calls: RefCell::new(vec![]),
                respond: Box::new(respond),
                missing: false,
            }
        }
        /// The docker CLI is not installed: every call errors.
        pub fn missing() -> FakeDocker {
            FakeDocker {
                calls: RefCell::new(vec![]),
                respond: Box::new(|_| Out {
                    code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                }),
                missing: true,
            }
        }
        pub fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl Docker for FakeDocker {
        fn run(&self, args: &[&str]) -> Result<Out> {
            let line = args.join(" ");
            self.calls.borrow_mut().push(line.clone());
            if self.missing {
                bail!("docker: command not found");
            }
            Ok((self.respond)(&line))
        }
    }

    pub(crate) fn ok(stdout: &str) -> Out {
        Out {
            code: 0,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    pub(crate) fn fail(code: i32, stderr: &str) -> Out {
        Out {
            code,
            stdout: String::new(),
            stderr: stderr.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;

    #[test]
    fn runc_versions_before_the_fixes_are_old() {
        assert_eq!(runc_is_old("1.1.12"), Some(true));
        assert_eq!(runc_is_old("1.2.7"), Some(true));
        assert_eq!(runc_is_old("1.3.2"), Some(true));
        assert_eq!(runc_is_old("1.4.0-rc.2"), Some(true));
    }

    #[test]
    fn runc_versions_with_the_fixes_are_not_old() {
        assert_eq!(runc_is_old("1.2.8"), Some(false));
        assert_eq!(runc_is_old("v1.3.3"), Some(false));
        assert_eq!(runc_is_old("1.4.0-rc.3"), Some(false));
        assert_eq!(runc_is_old("1.4.0"), Some(false));
        assert_eq!(runc_is_old("1.5.1"), Some(false));
    }

    #[test]
    fn unparseable_runc_version_is_unknown() {
        assert_eq!(runc_is_old("weird"), None);
        assert_eq!(runc_is_old(""), None);
    }

    #[test]
    fn probe_tells_ready_stopped_and_not_installed_apart() {
        assert!(matches!(
            probe(&FakeDocker::new(|_| ok("27"))),
            Runtime::Ready
        ));
        assert!(matches!(
            probe(&FakeDocker::new(|_| fail(1, "Cannot connect"))),
            Runtime::Stopped
        ));
        assert!(matches!(
            probe(&FakeDocker::missing()),
            Runtime::NotInstalled
        ));
    }

    #[test]
    fn available_has_plain_messages_for_both_failures() {
        assert!(available(&FakeDocker::new(|_| ok("27"))).is_ok());
        let stopped = available(&FakeDocker::new(|_| fail(1, "x")))
            .unwrap_err()
            .to_string();
        assert!(
            stopped.contains("not running") && stopped.contains("/scenario start"),
            "{stopped}"
        );
        let missing = available(&FakeDocker::missing()).unwrap_err().to_string();
        assert!(missing.contains("not installed"), "{missing}");
    }

    struct FakeStarter {
        started: std::cell::Cell<bool>,
        can: bool,
    }
    impl RuntimeStarter for FakeStarter {
        fn start(&self) -> bool {
            self.started.set(true);
            self.can
        }
    }

    const POLL: Duration = Duration::from_secs(2);
    const LIMIT: Duration = Duration::from_secs(10);

    #[test]
    fn a_ready_runtime_is_not_started_again() {
        let st = FakeStarter {
            started: Default::default(),
            can: true,
        };
        ensure_ready(&FakeDocker::new(|_| ok("27")), &st, LIMIT, POLL, &|_| {}).unwrap();
        assert!(!st.started.get());
    }

    #[test]
    fn a_stopped_runtime_is_started_and_waited_for() {
        use std::cell::Cell;
        use std::rc::Rc;
        // First probe fails (stopped), then two more fail while it boots, then it answers.
        let n = Rc::new(Cell::new(0));
        let n2 = n.clone();
        let d = FakeDocker::new(move |_| {
            n2.set(n2.get() + 1);
            if n2.get() <= 3 {
                fail(1, "starting")
            } else {
                ok("27")
            }
        });
        let st = FakeStarter {
            started: Default::default(),
            can: true,
        };
        let slept = Cell::new(0u32);
        ensure_ready(&d, &st, LIMIT, POLL, &|_| slept.set(slept.get() + 1)).unwrap();
        assert!(st.started.get());
        assert_eq!(slept.get(), 3);
    }

    #[test]
    fn a_runtime_that_never_comes_up_times_out_with_a_plain_message() {
        let st = FakeStarter {
            started: Default::default(),
            can: true,
        };
        let slept = std::cell::Cell::new(0u32);
        let e = ensure_ready(
            &FakeDocker::new(|_| fail(1, "x")),
            &st,
            LIMIT,
            POLL,
            &|_| slept.set(slept.get() + 1),
        )
        .unwrap_err()
        .to_string();
        assert!(
            e.contains("did not finish starting") && e.contains("try /scenario start again"),
            "{e}"
        );
        assert_eq!(slept.get(), 5); // LIMIT / POLL, no real waiting in the test
    }

    #[test]
    fn a_runtime_that_cannot_be_launched_says_so_without_waiting() {
        let st = FakeStarter {
            started: Default::default(),
            can: false,
        };
        let slept = std::cell::Cell::new(0u32);
        let e = ensure_ready(
            &FakeDocker::new(|_| fail(1, "x")),
            &st,
            LIMIT,
            POLL,
            &|_| slept.set(1),
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("could not start it"), "{e}");
        assert_eq!(slept.get(), 0);
    }

    #[test]
    fn a_missing_runtime_gets_install_guidance_and_is_never_started() {
        let st = FakeStarter {
            started: Default::default(),
            can: true,
        };
        let e = ensure_ready(&FakeDocker::missing(), &st, LIMIT, POLL, &|_| {})
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("not installed") && e.contains("https://docs.docker.com/get-docker/"),
            "{e}"
        );
        assert!(!st.started.get());
    }

    #[test]
    fn runc_warning_fires_only_for_an_old_runc() {
        let old = FakeDocker::new(|_| {
            ok(r#"[{"Name":"Engine","Version":"27"},{"Name":"runc","Version":"1.2.5"}]"#)
        });
        assert!(runc_warning(&old).unwrap().contains("CVE-2025-31133"));
        let new = FakeDocker::new(|_| ok(r#"[{"Name":"runc","Version":"1.3.3"}]"#));
        assert!(runc_warning(&new).is_none());
        let none = FakeDocker::new(|_| ok(r#"[{"Name":"Engine","Version":"27"}]"#));
        assert!(runc_warning(&none).is_none());
        let broken = FakeDocker::new(|_| fail(1, "x"));
        assert!(runc_warning(&broken).is_none());
    }

    #[test]
    fn out_text_joins_both_streams() {
        let o = Out {
            code: 1,
            stdout: "a\n".into(),
            stderr: "b\n".into(),
        };
        assert_eq!(o.text(), "a\nb");
    }
}
