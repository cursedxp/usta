//! Thin seam over the `docker` CLI so every command line is testable without Docker.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use crate::scenario::env::{internal, Internal};

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
        run_bounded("docker", args, time_limit(args))
    }
}

/// How long one docker call may take. A wedged daemon answers nothing, so every
/// call has a bound: short for the ones launch and exit wait on, long for the
/// work that legitimately takes time (image builds, setup and check scripts).
fn time_limit(args: &[&str]) -> Duration {
    let secs = match args.first().copied() {
        // `-s` walks the container's files: slow after a big step.
        Some("inspect") if args.contains(&"-s") => 60,
        Some("version" | "ps" | "inspect" | "image") => 10,
        Some("stop") => 15,
        Some("rm") => 30,
        Some("create" | "start" | "cp") => 60,
        Some("build") => 1800,
        _ => 600,
    };
    Duration::from_secs(secs)
}

/// How long to wait for the output once the process has ended; only something
/// it left behind holding the pipe open can use this up.
const DRAIN: Duration = Duration::from_secs(1);

/// A check or build can print without end, and this buffer is on the host.
/// Only the tail is ever read, so only the tail is kept.
const KEEP: usize = 1 << 20;

/// Read a stream on its own thread so a full pipe never stalls the process.
fn drain(mut stream: impl Read + Send + 'static) -> Receiver<Vec<u8>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut chunk = [0u8; 65536];
        loop {
            let n = match stream.read(&mut chunk) {
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            };
            if n == 0 {
                break;
            }
            kept.extend_from_slice(&chunk[..n]);
            if kept.len() > 2 * KEEP {
                kept.drain(..kept.len() - KEEP);
            }
        }
        if kept.len() > KEEP {
            kept.drain(..kept.len() - KEEP);
        }
        let _ = tx.send(kept);
    });
    rx
}

fn drained(rx: &Receiver<Vec<u8>>) -> String {
    String::from_utf8_lossy(&rx.recv_timeout(DRAIN).unwrap_or_default()).into_owned()
}

/// Run `program`, killing it after `limit`. Running past the limit is an
/// internal error; failing to spawn is a plain one (`probe`: not installed).
fn run_bounded(program: &str, args: &[&str], limit: Duration) -> Result<Out> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("could not run the {program} command"))?;
    let stdout = drain(child.stdout.take().expect("stdout is piped"));
    let stderr = drain(child.stderr.take().expect("stderr is piped"));
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(internal(format!(
                "{program} {} timed out after {}s",
                args.first().unwrap_or(&""),
                limit.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    Ok(Out {
        code: status.code().unwrap_or(-1),
        stdout: drained(&stdout),
        stderr: drained(&stderr),
    })
}

/// `run` for everything but `probe`, the one reader of the difference between "cannot
/// start the tool" and "the tool failed". Elsewhere it is tool text, for the log only.
pub(crate) fn run_tool(d: &dyn Docker, args: &[&str]) -> Result<Out> {
    d.run(args).map_err(|e| internal(format!("{e:#}")))
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
        // Timed out: the CLI is there, the engine does not answer.
        Err(e) if e.downcast_ref::<Internal>().is_some() => Runtime::Stopped,
        Err(_) => Runtime::NotInstalled,
        Ok(o) if o.ok() => Runtime::Ready,
        Ok(_) => Runtime::Stopped,
    }
}

// Docker is named below on purpose: the learner cannot install or open it
// without the name. Everywhere else learner text says "task" or "server".
const NOT_INSTALLED: &str = "Practice tasks run on a small server that Usta starts for you, and that needs Docker. Docker is not installed on this computer. Install Docker Desktop from https://docs.docker.com/get-docker/ (on Linux, install your distribution's docker package), open it once, then try again.";
const CANNOT_START: &str = "Usta needs Docker and it is installed, but Usta could not start it. Open Docker yourself, wait until it says it is running, then try again.";
const START_TIMEOUT_MSG: &str = "Docker did not finish starting within 90 seconds. Wait until it says it is running, then try again.";

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
    stop: &dyn Fn() -> bool,
) -> Result<()> {
    match probe(d) {
        Runtime::Ready => return Ok(()),
        Runtime::NotInstalled => bail!(NOT_INSTALLED),
        Runtime::Stopped => {}
    }
    if !s.start() {
        bail!(CANNOT_START);
    }
    // Probes take time of their own on a daemon that does not answer: `begun` keeps
    // the wait at `timeout`, `waited` keeps it deterministic when `sleep` is a test's.
    let begun = Instant::now();
    let mut waited = Duration::ZERO;
    while waited < timeout && begun.elapsed() < timeout {
        sleep(poll);
        waited += poll;
        if stop() {
            bail!("stopped while waiting for the practice server software");
        }
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
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use super::*;

    /// Records every call as one joined line and answers from a closure.
    pub(crate) struct FakeDocker {
        pub calls: RefCell<Vec<String>>,
        respond: Box<dyn Fn(&str) -> Out>,
        missing: Rc<Cell<bool>>,
    }

    impl FakeDocker {
        pub fn new(respond: impl Fn(&str) -> Out + 'static) -> FakeDocker {
            FakeDocker {
                calls: RefCell::new(vec![]),
                respond: Box::new(respond),
                missing: Rc::default(),
            }
        }
        /// Answers from a closure, and behaves like `missing` while the cell is set.
        pub fn switchable(
            missing: Rc<Cell<bool>>,
            respond: impl Fn(&str) -> Out + 'static,
        ) -> FakeDocker {
            FakeDocker {
                missing,
                ..FakeDocker::new(respond)
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
                missing: Rc::new(Cell::new(true)),
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
            if self.missing.get() {
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
        ensure_ready(
            &FakeDocker::new(|_| ok("27")),
            &st,
            LIMIT,
            POLL,
            &|_| {},
            &|| false,
        )
        .unwrap();
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
        ensure_ready(
            &d,
            &st,
            LIMIT,
            POLL,
            &|_| slept.set(slept.get() + 1),
            &|| false,
        )
        .unwrap();
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
            &|| false,
        )
        .unwrap_err()
        .to_string();
        assert!(
            e.contains("did not finish starting") && !e.contains("/scenario"),
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
            &|| false,
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
        let e = ensure_ready(&FakeDocker::missing(), &st, LIMIT, POLL, &|_| {}, &|| false)
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("not installed")
                && e.contains("https://docs.docker.com/get-docker/")
                && !e.contains("/scenario"),
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

    /// A daemon that never answers: `RealDocker` turns that into an internal error.
    struct Wedged;
    impl Docker for Wedged {
        fn run(&self, _: &[&str]) -> Result<Out> {
            Err(internal("docker version timed out after 10s"))
        }
    }

    fn sh(script: &str, limit: Duration) -> Result<Out> {
        run_bounded("sh", &["-c", script], limit)
    }

    #[test]
    fn a_command_past_its_limit_is_killed_and_becomes_an_internal_error() {
        let begun = Instant::now();
        let e = sh("sleep 30", Duration::from_millis(150)).err().unwrap();
        assert!(
            begun.elapsed() < Duration::from_secs(5),
            "must not wait it out"
        );
        assert!(e.downcast_ref::<Internal>().is_some(), "{e:?}");
        assert!(e.to_string().contains("timed out"), "{e}");
    }

    #[test]
    fn a_bounded_run_keeps_exit_code_and_both_streams() {
        let o = sh("echo out; echo err >&2; exit 3", Duration::from_secs(10)).unwrap();
        assert_eq!(
            (o.code, o.stdout.trim(), o.stderr.trim()),
            (3, "out", "err")
        );
    }

    #[test]
    fn output_larger_than_a_pipe_buffer_does_not_stall_the_run() {
        let o = sh(
            "dd if=/dev/zero bs=1024 count=512 2>/dev/null",
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(o.stdout.len(), 512 * 1024);
    }

    #[test]
    fn a_flood_of_output_keeps_only_the_tail_in_memory() {
        let o = sh(
            "yes x | head -c 6000000; echo; echo the-end",
            Duration::from_secs(20),
        )
        .unwrap();
        assert!(o.stdout.len() <= KEEP, "{}", o.stdout.len());
        assert!(o.stdout.trim_end().ends_with("the-end"));
    }

    #[test]
    fn a_background_process_holding_the_pipe_does_not_hang_the_read() {
        let begun = Instant::now();
        let o = sh("sleep 30 & echo done", Duration::from_secs(10)).unwrap();
        assert!(o.ok());
        assert!(begun.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_missing_program_is_not_an_internal_error() {
        let e = run_bounded("usta-no-such-binary", &[], Duration::from_secs(1))
            .err()
            .unwrap();
        assert!(e.downcast_ref::<Internal>().is_none(), "{e:?}");
    }

    #[test]
    fn short_steps_get_short_bounds_and_long_steps_long_ones() {
        let secs = |args: &[&str]| time_limit(args).as_secs();
        for quick in [
            &["version", "--format", "x"][..],
            &["ps", "-a"],
            &["inspect", "n"],
            &["image", "inspect", "i"],
        ] {
            assert!(secs(quick) <= 10, "{quick:?}");
        }
        assert_eq!(secs(&["inspect", "-s", "-f", "{{.SizeRw}}", "n"]), 60);
        assert!(secs(&["stop", "-t", "2", "n"]) <= 15);
        assert!(secs(&["rm", "-f", "n"]) <= 30);
        assert!(secs(&["start", "n"]) <= 60);
        assert!(secs(&["exec", "n", "sh", "setup.sh"]) >= 600);
        assert!(secs(&["build", "-t", "i", "."]) >= 1800);
    }

    #[test]
    fn a_wedged_daemon_counts_as_stopped_not_as_missing() {
        assert!(matches!(probe(&Wedged), Runtime::Stopped));
    }

    #[test]
    fn slow_probes_count_against_the_start_wait() {
        let d = FakeDocker::new(|_| {
            std::thread::sleep(Duration::from_millis(20));
            fail(1, "x")
        });
        let st = FakeStarter {
            started: Default::default(),
            can: true,
        };
        let limit = Duration::from_millis(100);
        let e = ensure_ready(&d, &st, limit, Duration::from_millis(1), &|_| {}, &|| false);
        assert!(e.is_err());
        assert!(d.calls().len() < 20, "polled {} times", d.calls().len());
    }
}
