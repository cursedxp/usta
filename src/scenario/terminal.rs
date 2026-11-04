//! Opens a new terminal window already inside the task, so the learner never
//! types a Docker command. Platform seam: `plan` is pure and tested for every
//! system; `SystemLauncher` executes the plan; tests use a fake `Launcher`.

use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) struct WindowReq<'a> {
    /// Command line that opens a shell inside the task (`EnvHandle::access`).
    pub command_line: &'a str,
    pub run_id: &'a str,
    /// Folder for the macOS launcher script (`<project>/.usta/scenarios`).
    pub scratch: &'a Path,
}

pub(crate) trait Launcher {
    /// True when a window was opened.
    fn open(&self, req: &WindowReq) -> bool;
}

#[derive(Debug, PartialEq)]
pub(crate) enum Plan {
    /// macOS: write a `.command` file and `open` it. No Apple Events, so no Automation prompt.
    CommandFile,
    Spawn {
        program: String,
        args: Vec<String>,
    },
}

/// Known Linux terminals in preference order, with the arguments that precede the command.
const LINUX_TERMINALS: [(&str, &[&str]); 5] = [
    ("x-terminal-emulator", &["-e"]),
    ("gnome-terminal", &["--"]),
    ("konsole", &["-e"]),
    ("xfce4-terminal", &["-x"]),
    ("xterm", &["-e"]),
];

pub(crate) fn plan(
    os: &str,
    on_path: &dyn Fn(&str) -> bool,
    display: bool,
    command_line: &str,
) -> Option<Plan> {
    match os {
        "macos" => Some(Plan::CommandFile),
        "windows" => {
            let mut args: Vec<String> = ["/c", "start", "Usta task", "cmd", "/k"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            args.extend(command_line.split_whitespace().map(String::from));
            Some(Plan::Spawn {
                program: "cmd".into(),
                args,
            })
        }
        "linux" if display => LINUX_TERMINALS
            .iter()
            .find(|(p, _)| on_path(p))
            .map(|(p, pre)| {
                let mut args: Vec<String> = pre.iter().map(|s| s.to_string()).collect();
                args.extend(["sh".into(), "-c".into(), command_line.to_string()]);
                Plan::Spawn {
                    program: p.to_string(),
                    args,
                }
            }),
        _ => None,
    }
}

pub(crate) fn command_file_text(command_line: &str) -> String {
    format!("#!/bin/sh\nclear\nexec {command_line}\n")
}

#[cfg(unix)]
fn write_command_file(req: &WindowReq) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = req.scratch.join(format!("window-{}.command", req.run_id));
    std::fs::create_dir_all(req.scratch).ok()?;
    std::fs::write(&path, command_file_text(req.command_line)).ok()?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).ok()?;
    Some(path)
}

#[cfg(not(unix))]
fn write_command_file(_req: &WindowReq) -> Option<PathBuf> {
    None
}

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

pub(crate) struct SystemLauncher;

impl Launcher for SystemLauncher {
    fn open(&self, req: &WindowReq) -> bool {
        let display =
            std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
        match plan(std::env::consts::OS, &on_path, display, req.command_line) {
            Some(Plan::CommandFile) => write_command_file(req).is_some_and(|f| {
                Command::new("open")
                    .arg(f)
                    .status()
                    .is_ok_and(|s| s.success())
            }),
            Some(Plan::Spawn { program, args }) => Command::new(program).args(args).spawn().is_ok(),
            None => false,
        }
    }
}

/// What the learner reads after start or resume. Only the fallback names the
/// command (spec: "falls back to showing the command").
pub(crate) fn window_notice(l: &dyn Launcher, req: &WindowReq) -> String {
    if l.open(req) {
        "A terminal window opened inside your task. Work there.".to_string()
    } else {
        format!(
            "Usta could not open a terminal window on this system. Open a terminal yourself and run: {}",
            req.command_line
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = "docker exec -it usta-aaaa1111-demo-1 bash";

    fn none(_: &str) -> bool {
        false
    }

    #[test]
    fn macos_uses_a_command_file() {
        assert_eq!(plan("macos", &none, false, LINE), Some(Plan::CommandFile));
    }

    #[test]
    fn linux_needs_a_display_and_a_known_terminal() {
        assert_eq!(plan("linux", &|_| true, false, LINE), None);
        assert_eq!(plan("linux", &none, true, LINE), None);
    }

    #[test]
    fn linux_picks_the_first_known_terminal_in_order() {
        let only_konsole_and_xterm = |p: &str| p == "konsole" || p == "xterm";
        match plan("linux", &only_konsole_and_xterm, true, LINE).unwrap() {
            Plan::Spawn { program, args } => {
                assert_eq!(program, "konsole");
                assert_eq!(args, vec!["-e", "sh", "-c", LINE]);
            }
            other => panic!("{other:?}"),
        }
        match plan("linux", &|p| p == "gnome-terminal", true, LINE).unwrap() {
            Plan::Spawn { args, .. } => assert_eq!(args[0], "--"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn windows_starts_a_new_cmd_window() {
        match plan("windows", &none, false, LINE).unwrap() {
            Plan::Spawn { program, args } => {
                assert_eq!(program, "cmd");
                assert_eq!(&args[..5], ["/c", "start", "Usta task", "cmd", "/k"]);
                assert_eq!(args[5..].join(" "), LINE);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn unknown_systems_have_no_plan() {
        assert_eq!(plan("plan9", &|_| true, true, LINE), None);
    }

    #[test]
    fn the_command_file_runs_the_command_in_place_of_the_shell() {
        let t = command_file_text(LINE);
        assert!(t.starts_with("#!/bin/sh\n"));
        assert!(t.contains(&format!("exec {LINE}")));
    }

    #[cfg(unix)]
    #[test]
    fn the_command_file_is_written_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("usta_scn_term_{}", std::process::id()));
        let req = WindowReq {
            command_line: LINE,
            run_id: "demo-1",
            scratch: &dir,
        };
        let path = write_command_file(&req).unwrap();
        assert_eq!(path.file_name().unwrap(), "window-demo-1.command");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o755,
            0o755
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            command_file_text(LINE)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    struct FakeLauncher {
        opens: bool,
        seen: std::cell::RefCell<Vec<String>>,
    }
    impl Launcher for FakeLauncher {
        fn open(&self, req: &WindowReq) -> bool {
            self.seen.borrow_mut().push(req.command_line.to_string());
            self.opens
        }
    }

    fn req() -> WindowReq<'static> {
        WindowReq {
            command_line: LINE,
            run_id: "demo-1",
            scratch: Path::new("/tmp/x"),
        }
    }

    #[test]
    fn success_notice_never_shows_the_command_or_infrastructure_words() {
        let l = FakeLauncher {
            opens: true,
            seen: Default::default(),
        };
        let n = window_notice(&l, &req());
        assert_eq!(l.seen.borrow().as_slice(), [LINE]);
        let low = n.to_lowercase();
        assert!(
            n.contains("terminal window")
                && !low.contains("docker")
                && !low.contains("container")
                && !low.contains("image"),
            "{n}"
        );
    }

    #[test]
    fn fallback_notice_shows_the_command() {
        let l = FakeLauncher {
            opens: false,
            seen: Default::default(),
        };
        let n = window_notice(&l, &req());
        assert!(
            n.contains("could not open a terminal window") && n.contains(LINE),
            "{n}"
        );
    }
}
