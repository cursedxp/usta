//! File change payload: the pure logic here decides what goes to the LLM.
//! Full content on first sight (so context gets established), unified diff on
//! subsequent saves (token savings + "what changed" signal), a one-time local
//! warning above the size ceiling. No IO — main reads, we decide.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use similar::TextDiff;

/// Files above this size are not sent to the LLM (context + cost protection).
pub const MAX_FILE_BYTES: usize = 64 * 1024;

/// How a save event is reflected to the LLM.
pub enum ChangePayload {
    /// File seen for the first time — full content is sent.
    FirstSight(String),
    /// Unified diff relative to the previous sighting.
    Diff(String),
    /// Size ceiling exceeded — local warning only, no LLM call (once per file).
    TooLarge(usize),
    /// Content unchanged, or a large file already warned about — skip silently.
    Skip,
}

/// Memory of file contents seen during the session.
pub struct FileMemory {
    seen: HashMap<PathBuf, String>,
    warned_large: HashSet<PathBuf>,
}

impl FileMemory {
    pub fn new() -> Self {
        FileMemory {
            seen: HashMap::new(),
            warned_large: HashSet::new(),
        }
    }

    /// Pre-load baseline content for a file WITHOUT producing feedback.
    /// Used at session start to register mentor docs that are already embedded
    /// in the system prompt: a later unchanged re-save then observes as `Skip`
    /// (not a redundant `FirstSight` that re-sends the whole file to the LLM),
    /// and an edited save observes as a `Diff` instead of full content.
    pub fn seed(&mut self, path: &Path, content: String) {
        self.seen.insert(path.to_path_buf(), content);
    }

    /// Observe newly saved content, produce the LLM payload, update memory.
    pub fn observe(&mut self, path: &Path, current: String) -> ChangePayload {
        if current.len() > MAX_FILE_BYTES {
            if self.warned_large.insert(path.to_path_buf()) {
                return ChangePayload::TooLarge(current.len());
            }
            return ChangePayload::Skip;
        }
        match self.seen.insert(path.to_path_buf(), current.clone()) {
            None => ChangePayload::FirstSight(current),
            Some(prev) if prev == current => ChangePayload::Skip,
            Some(prev) => {
                let diff = TextDiff::from_lines(&prev, &current)
                    .unified_diff()
                    .context_radius(3)
                    .header("before", "after")
                    .to_string();
                ChangePayload::Diff(diff)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn first_sight_returns_full_content() {
        let mut m = FileMemory::new();
        match m.observe(Path::new("a.rs"), "fn main() {}".into()) {
            ChangePayload::FirstSight(s) => assert_eq!(s, "fn main() {}"),
            _ => panic!("ilk görüş FirstSight olmalı"),
        }
    }

    #[test]
    fn unchanged_content_is_skipped() {
        let mut m = FileMemory::new();
        let _ = m.observe(Path::new("a.rs"), "ayni".into());
        assert!(matches!(
            m.observe(Path::new("a.rs"), "ayni".into()),
            ChangePayload::Skip
        ));
    }

    #[test]
    fn changed_content_yields_unified_diff() {
        let mut m = FileMemory::new();
        let _ = m.observe(Path::new("a.rs"), "eski satir\n".into());
        match m.observe(Path::new("a.rs"), "yeni satir\n".into()) {
            ChangePayload::Diff(d) => {
                assert!(d.contains("-eski satir"));
                assert!(d.contains("+yeni satir"));
            }
            _ => panic!("değişiklik Diff olmalı"),
        }
    }

    #[test]
    fn oversized_file_warns_once_then_skips() {
        let mut m = FileMemory::new();
        let big = "x".repeat(MAX_FILE_BYTES + 1);
        assert!(matches!(
            m.observe(Path::new("big.rs"), big.clone()),
            ChangePayload::TooLarge(_)
        ));
        assert!(matches!(
            m.observe(Path::new("big.rs"), big),
            ChangePayload::Skip
        ));
    }

    #[test]
    fn seeded_unchanged_content_is_skipped_not_first_sight() {
        // Baseline seeded at session start (mentor doc already in system prompt):
        // saving the file unchanged must NOT re-send full content.
        let mut m = FileMemory::new();
        m.seed(Path::new("mentor/PROJECT.md"), "spec govdesi\n".into());
        assert!(matches!(
            m.observe(Path::new("mentor/PROJECT.md"), "spec govdesi\n".into()),
            ChangePayload::Skip
        ));
    }

    #[test]
    fn seeded_then_edited_content_yields_diff_not_first_sight() {
        // Editing a seeded file surfaces a diff (fark), never full content.
        let mut m = FileMemory::new();
        m.seed(Path::new("mentor/PROJECT.md"), "eski hedef\n".into());
        match m.observe(Path::new("mentor/PROJECT.md"), "yeni hedef\n".into()) {
            ChangePayload::Diff(d) => {
                assert!(d.contains("-eski hedef"));
                assert!(d.contains("+yeni hedef"));
            }
            _ => panic!("seed sonrası değişiklik Diff olmalı (FirstSight değil)"),
        }
    }

    #[test]
    fn unseeded_file_still_first_sights() {
        // Regression lock: a file that was never seeded keeps the original
        // FirstSight-on-first-save behavior.
        let mut m = FileMemory::new();
        m.seed(Path::new("mentor/PROJECT.md"), "seeded\n".into());
        assert!(matches!(
            m.observe(Path::new("src/other.rs"), "brand new\n".into()),
            ChangePayload::FirstSight(_)
        ));
    }

    #[test]
    fn diff_is_per_file_not_global() {
        let mut m = FileMemory::new();
        let _ = m.observe(Path::new("a.rs"), "a icerik\n".into());
        // b.rs is seen for the first time — must not be diffed against a.rs's history.
        assert!(matches!(
            m.observe(Path::new("b.rs"), "b icerik\n".into()),
            ChangePayload::FirstSight(_)
        ));
    }
}
