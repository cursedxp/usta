//! Raw session recording: each turn is appended immediately to
//! `.usta/sessions/<topic>-<timestamp>.jsonl` — even if the closing flush
//! dies or the terminal crashes, the session is on disk. After a successful
//! flush the file becomes `.done.jsonl`; on startup a file without `.done` =
//! a recoverable half-finished session. A recording error NEVER blocks the session.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;

/// JSON line for a single turn.
pub fn line(role: &str, text: &str) -> String {
    let mut l = serde_json::json!({ "role": role, "text": text }).to_string();
    l.push('\n');
    l
}

/// Session file path: `.usta/sessions/<topic>-<timestamp>.jsonl`.
pub fn session_path(project_root: &Path, topic: &str, stamp: &str) -> PathBuf {
    project_root
        .join(".usta/sessions")
        .join(format!("{topic}-{stamp}.jsonl"))
}

/// Successful close: `.jsonl` → `.done.jsonl`.
pub fn mark_done(path: &Path) -> Result<()> {
    let done = path.with_extension("done.jsonl");
    std::fs::rename(path, done)?;
    Ok(())
}

/// Session files without a `.done` marker — half-finished sessions that couldn't be flushed.
pub fn find_unfinished(project_root: &Path) -> Vec<PathBuf> {
    let dir = project_root.join(".usta/sessions");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().to_string());
            matches!(name, Some(n) if n.ends_with(".jsonl") && !n.ends_with(".done.jsonl"))
        })
        .collect();
    out.sort();
    out
}

/// Turn recorder — errors are silent, warns ONCE on the first error.
pub struct Recorder {
    path: PathBuf,
    warned: AtomicBool,
}

impl Recorder {
    pub fn new(path: PathBuf) -> Recorder {
        Recorder { path, warned: AtomicBool::new(false) }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn user(&self, text: &str) {
        self.append("user", text);
    }

    pub fn assistant(&self, text: &str) {
        self.append("assistant", text);
    }

    fn append(&self, role: &str, text: &str) {
        let res = (|| -> std::io::Result<()> {
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            f.write_all(line(role, text).as_bytes())
        })();
        if res.is_err() && !self.warned.swap(true, Ordering::Relaxed) {
            crate::ui::warn("session recording failed — continuing without recording");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn line_is_json_with_role_and_text() {
        let l = line("user", "merhaba \"usta\"");
        let v: serde_json::Value = serde_json::from_str(l.trim()).unwrap();
        assert_eq!(v["role"], "user");
        assert_eq!(v["text"], "merhaba \"usta\"");
        assert!(l.ends_with('\n'));
    }

    #[test]
    fn session_path_builds_expected_layout() {
        let p = session_path(Path::new("/proje"), "rust", "20260807-1030");
        assert_eq!(p, Path::new("/proje/.usta/sessions/rust-20260807-1030.jsonl"));
    }

    #[test]
    fn find_unfinished_skips_done_files() {
        let base = std::env::temp_dir().join(format!(
            "usta_transcript_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let sdir = base.join(".usta/sessions");
        std::fs::create_dir_all(&sdir).unwrap();
        std::fs::write(sdir.join("rust-1.jsonl"), "x").unwrap();
        std::fs::write(sdir.join("rust-2.done.jsonl"), "x").unwrap();
        let found = find_unfinished(&base);
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("rust-1.jsonl"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn mark_done_renames_jsonl() {
        let base = std::env::temp_dir().join(format!(
            "usta_transcript_done_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let p = base.join("rust-1.jsonl");
        std::fs::write(&p, "x").unwrap();
        mark_done(&p).unwrap();
        assert!(!p.exists());
        assert!(base.join("rust-1.done.jsonl").exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
