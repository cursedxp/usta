//! Background file watcher (`notify` v6). Forwards saved files to the REPL
//! for proactive Socratic feedback.
//!
//! `notify` runs synchronously — we don't force it into async. Background
//! thread + std mpsc is the right pattern: the watcher is kept alive inside
//! the thread, changed paths flow through the channel.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use notify::{EventKind, RecursiveMode, Watcher};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
use tokio::time::Instant;

/// Watch recursively under `root`; send file paths from modify events.
/// The returned receiver is consumed in the select-loop with `recv().await`
/// (or `try_recv`).
pub fn spawn(root: &Path) -> Result<UnboundedReceiver<PathBuf>> {
    let (out_tx, out_rx) = unbounded_channel::<PathBuf>();
    let (ev_tx, ev_rx) = mpsc::channel::<notify::Result<notify::Event>>();

    let mut watcher = notify::recommended_watcher(move |res| {
        // If the receiver has been dropped, swallow silently — the thread will close soon.
        let _ = ev_tx.send(res);
    })
    .context("failed to create file watcher")?;

    watcher
        .watch(root, RecursiveMode::Recursive)
        .with_context(|| format!("failed to watch: {}", root.display()))?;

    thread::spawn(move || {
        // Keep the watcher alive inside the thread — watching stops if it's dropped.
        let _watcher = watcher;
        for res in ev_rx {
            let Ok(event) = res else { continue };
            if matches!(event.kind, EventKind::Modify(_)) {
                for path in event.paths {
                    if is_ignored(&path) {
                        continue;
                    }
                    // End the thread if the REPL receiver has closed.
                    if out_tx.send(path).is_err() {
                        return;
                    }
                }
            }
        }
    });

    Ok(out_rx)
}

/// Filter out build/VCS/hidden-dir noise — ignore if a path component is
/// `target`, `node_modules`, or starts with `.` (e.g. `.git`, `.venv`).
/// Language-agnostic: we don't filter by extension, Usta works across domains.
pub fn is_ignored(path: &Path) -> bool {
    path.components().any(|c| match c {
        std::path::Component::Normal(s) => {
            let s = s.to_string_lossy().to_ascii_lowercase();
            s == "target"
                || s == "node_modules"
                || s.starts_with('.')
                // Secret files never go to the LLM.
                || s.ends_with(".pem")
                || s.ends_with(".key")
                || s.contains("secret")
                || s.contains("credential")
        }
        _ => false,
    })
}

/// Pure debounce state that smooths out a save storm. Editors produce
/// multiple modify events for a single save; each event is `push`ed, the
/// `deadline` expires `window` after the last event, and the select-loop
/// processes them all at once with `flush`.
pub struct Debouncer {
    pending: Vec<PathBuf>,
    deadline: Option<Instant>,
    window: Duration,
}

impl Debouncer {
    pub fn new(window: Duration) -> Self {
        Debouncer { pending: Vec::new(), deadline: None, window }
    }

    /// Accumulate the path (dedupe while preserving first-seen order) and
    /// push the deadline forward.
    pub fn push(&mut self, path: PathBuf, now: Instant) {
        if !self.pending.contains(&path) {
            self.pending.push(path);
        }
        self.deadline = Some(now + self.window);
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Drain the accumulator, reset the deadline.
    pub fn flush(&mut self) -> Vec<PathBuf> {
        self.deadline = None;
        std::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::Instant;

    #[test]
    fn debouncer_push_dedups_and_preserves_order() {
        let mut d = Debouncer::new(Duration::from_millis(1000));
        let now = Instant::now();
        d.push(PathBuf::from("a.rs"), now);
        d.push(PathBuf::from("b.rs"), now);
        d.push(PathBuf::from("a.rs"), now);
        assert_eq!(d.flush(), vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")]);
    }

    #[test]
    fn debouncer_push_extends_deadline() {
        let mut d = Debouncer::new(Duration::from_millis(1000));
        let t0 = Instant::now();
        d.push(PathBuf::from("a.rs"), t0);
        let t1 = t0 + Duration::from_millis(500);
        d.push(PathBuf::from("b.rs"), t1);
        assert_eq!(d.deadline(), Some(t1 + Duration::from_millis(1000)));
    }

    #[test]
    fn debouncer_flush_clears_pending_and_deadline() {
        let mut d = Debouncer::new(Duration::from_millis(1000));
        d.push(PathBuf::from("a.rs"), Instant::now());
        let _ = d.flush();
        assert!(d.deadline().is_none());
        assert!(d.flush().is_empty());
    }

    #[test]
    fn debouncer_empty_has_no_deadline() {
        let d = Debouncer::new(Duration::from_millis(1000));
        assert!(d.deadline().is_none());
    }

    #[test]
    fn is_ignored_flags_target_dir() {
        assert!(is_ignored(Path::new("target/debug/x.rs")));
    }

    #[test]
    fn is_ignored_flags_hidden_dir() {
        assert!(is_ignored(Path::new(".git/HEAD")));
    }

    #[test]
    fn is_ignored_flags_node_modules() {
        assert!(is_ignored(Path::new("node_modules/foo/index.js")));
    }

    #[test]
    fn is_ignored_allows_src_file() {
        assert!(!is_ignored(Path::new("src/main.rs")));
    }

    #[test]
    fn is_ignored_allows_arbitrary_extension() {
        assert!(!is_ignored(Path::new("foo.py")));
    }

    #[test]
    fn is_ignored_blocks_secret_files() {
        assert!(is_ignored(Path::new("config/server.pem")));
        assert!(is_ignored(Path::new("keys/deploy.key")));
        assert!(is_ignored(Path::new("config/client_secrets.yaml")));
        assert!(is_ignored(Path::new("aws/CREDENTIALS.json")));
    }

    #[test]
    fn is_ignored_allows_normal_config() {
        assert!(!is_ignored(Path::new("config/settings.yaml")));
    }
}
