//! Pure logic for the polite watcher: queues file-change feedback while a
//! mentor question is open, and decides when to flush it.

use std::path::PathBuf;
use std::time::Duration;

/// How long to wait for a keystroke before flushing a queued file-change
/// notice even without an answer (inactivity backstop).
pub(crate) const POLITE_BACKSTOP: Duration = Duration::from_secs(180);

/// Order-preserving, dedup'd queue of file-change paths withheld while a
/// mentor question is open.
pub(crate) struct PoliteQueue {
    paths: Vec<PathBuf>,
}

impl PoliteQueue {
    pub(crate) fn new() -> Self {
        Self { paths: Vec::new() }
    }

    /// Pushes `path` if not already queued. Returns `true` when this push
    /// is the first into an empty queue (i.e. it's time to announce).
    pub(crate) fn push(&mut self, path: PathBuf) -> bool {
        let was_empty = self.paths.is_empty();
        let added = !self.paths.contains(&path);
        if added {
            self.paths.push(path);
        }
        was_empty && added
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.paths.len()
    }

    /// Drains all queued paths in order, resetting the queue to empty.
    pub(crate) fn drain(&mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.paths)
    }
}

/// Whether `text` contains an open question (a `?`).
pub(crate) fn question_open(text: &str) -> bool {
    text.contains('?')
}

/// The backstop flush deadline: `None` while the queue is empty, otherwise
/// `last_key + POLITE_BACKSTOP`.
pub(crate) fn backstop_deadline(
    queue_empty: bool,
    last_key: tokio::time::Instant,
) -> Option<tokio::time::Instant> {
    if queue_empty {
        None
    } else {
        Some(last_key + POLITE_BACKSTOP)
    }
}

/// Whether any line in `text`, trimmed and lowercased, is `watch: live`.
pub(crate) fn live_from_approach(text: &str) -> bool {
    text.lines()
        .any(|l| l.trim().eq_ignore_ascii_case("watch: live"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn queue_push_dedups_and_preserves_order() {
        let mut q = PoliteQueue::new();
        assert!(q.push(PathBuf::from("a.rs"))); // first push into empty queue → announce
        assert!(!q.push(PathBuf::from("b.rs"))); // queue already non-empty → silent
        assert!(!q.push(PathBuf::from("a.rs"))); // duplicate → silent, not re-added
        assert_eq!(q.len(), 2);
        assert_eq!(
            q.drain(),
            vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")]
        );
        assert!(q.is_empty());
        assert!(q.push(PathBuf::from("a.rs"))); // empty again → announce again
    }

    #[test]
    fn question_open_detects_question_mark() {
        assert!(question_open("What does ownership mean?"));
        assert!(question_open("Try it in parallel. What happens?"));
        assert!(!question_open("Good. Keep going."));
        assert!(!question_open(""));
    }

    #[test]
    fn backstop_deadline_only_when_queue_nonempty() {
        let now = tokio::time::Instant::now();
        assert_eq!(backstop_deadline(true, now), None);
        assert_eq!(backstop_deadline(false, now), Some(now + POLITE_BACKSTOP));
    }

    #[test]
    fn live_from_approach_matches_watch_live_line() {
        assert!(live_from_approach("# JS\n\nwatch: live\n"));
        assert!(live_from_approach("  WATCH: LIVE  \n")); // forgiving case/whitespace
        assert!(!live_from_approach("watch: polite\n")); // unknown value → default
        assert!(!live_from_approach("# nothing here\n"));
        assert!(!live_from_approach(""));
    }
}
