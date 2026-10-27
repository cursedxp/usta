//! Topic lock (`.usta/.lock-<topic>`, content: the owner's pid): is the owner
//! still running? Liveness is probed with the platform's own tool — no new
//! crates (an OS advisory lock would need one, or Rust 1.89). Review P2-8.
//! `LockGuard` removes the lock on every exit path (review P2-9).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Is a process with this pid running? Unix: `kill -0 <pid>` exits 0. A pid
/// owned by another user answers EPERM -> non-zero -> treated as dead (a false
/// "dead" only means recovery is offered). Windows: `tasklist` lists the pid.
/// A tool that can't be spawned -> dead.
pub(crate) fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
    #[cfg(windows)]
    {
        let want = pid.to_string();
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .is_ok_and(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .split_whitespace()
                    .any(|w| w == want)
            })
    }
}

/// The pid of ANOTHER running usta holding this topic's lock, if any. A lock
/// left by a crashed run (dead pid) or by this process reads as no owner.
pub(crate) fn live_owner(root: &Path, topic: &str) -> Option<u32> {
    let pid: u32 = std::fs::read_to_string(crate::lifecycle::lock_path(root, topic))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    (pid != 0 && pid != std::process::id() && pid_alive(pid)).then_some(pid)
}

/// Split unfinished transcripts into (recoverable, topics a running usta owns).
/// A live session's transcript must not be flushed, renamed or deleted under it.
pub(crate) fn split_live(root: &Path, records: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<String>) {
    let mut free = Vec::new();
    let mut live: Vec<String> = Vec::new();
    for p in records {
        match crate::transcript::topic_from_record(&p) {
            Some(t) if live_owner(root, &t).is_some() => {
                if !live.contains(&t) {
                    live.push(t);
                }
            }
            _ => free.push(p),
        }
    }
    (free, live)
}

/// The topic lock this process holds. Dropping it removes the file — on the
/// normal close, on any `?` error and on a panic that unwinds.
pub(crate) struct LockGuard(PathBuf);

impl LockGuard {
    /// Write the own pid to `path`. A write failure only warns: the lock is a
    /// courtesy check, it must never block the session.
    pub(crate) fn acquire(path: PathBuf) -> LockGuard {
        if let Err(e) = std::fs::write(&path, std::process::id().to_string()) {
            crate::ui::warn(&format!("topic lock could not be written: {e}"));
        }
        LockGuard(path)
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let mine = std::fs::read_to_string(&self.0)
            .is_ok_and(|s| s.trim() == std::process::id().to_string());
        if mine {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("usta_lock_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join(".usta/sessions")).unwrap();
        base
    }

    #[test]
    fn lock_guard_writes_own_pid_and_removes_the_file_on_drop() {
        let root = project("guard");
        let path = crate::lifecycle::lock_path(&root, "rust");
        let guard = LockGuard::acquire(path.clone());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            std::process::id().to_string()
        );
        assert_eq!(
            live_owner(&root, "rust"),
            None,
            "own lock is not another owner"
        );
        drop(guard);
        assert!(!path.exists(), "every exit path drops the guard");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn guard_drop_leaves_a_lock_another_process_now_owns() {
        let root = project("foreign");
        let path = crate::lifecycle::lock_path(&root, "rust");
        let guard = LockGuard::acquire(path.clone());
        std::fs::write(&path, (std::process::id() + 1).to_string()).unwrap();
        drop(guard);
        assert!(path.exists(), "another session's lock stays");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    fn reaped_pid() -> u32 {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    #[test]
    fn pid_alive_true_for_self() {
        assert!(pid_alive(std::process::id()));
    }

    #[cfg(unix)]
    #[test]
    fn pid_alive_false_for_a_reaped_child() {
        assert!(!pid_alive(reaped_pid()));
    }

    #[cfg(unix)]
    #[test]
    fn live_owner_ignores_missing_own_and_dead_pids() {
        let root = project("owner");
        let lock = crate::lifecycle::lock_path(&root, "rust");
        assert_eq!(live_owner(&root, "rust"), None, "no lock");
        std::fs::write(&lock, std::process::id().to_string()).unwrap();
        assert_eq!(live_owner(&root, "rust"), None, "own pid");
        std::fs::write(&lock, reaped_pid().to_string()).unwrap();
        assert_eq!(live_owner(&root, "rust"), None, "dead pid = crashed run");
        std::fs::write(&lock, "garbage").unwrap();
        assert_eq!(live_owner(&root, "rust"), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn split_live_skips_records_a_running_usta_owns() {
        let root = project("split");
        let mut other = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        std::fs::write(
            crate::lifecycle::lock_path(&root, "rust"),
            other.id().to_string(),
        )
        .unwrap();
        std::fs::write(
            crate::lifecycle::lock_path(&root, "dns"),
            reaped_pid().to_string(),
        )
        .unwrap();
        let rec = |n: &str| root.join(".usta/sessions").join(n);
        let records = vec![
            rec("dns-20261026-101500.jsonl"),
            rec("odd.jsonl"),
            rec("rust-20261026-101500.jsonl"),
            rec("rust-20261026-111500.jsonl"),
        ];
        let (free, live) = split_live(&root, records);
        assert_eq!(
            free,
            vec![rec("dns-20261026-101500.jsonl"), rec("odd.jsonl")]
        );
        assert_eq!(live, vec!["rust".to_string()]);
        let _ = other.kill();
        let _ = other.wait();
        let _ = std::fs::remove_dir_all(&root);
    }
}
