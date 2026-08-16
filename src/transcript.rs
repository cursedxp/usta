//! Raw session recording: each turn is appended immediately to
//! `.usta/sessions/<topic>-<timestamp>.jsonl` — even if the closing flush
//! dies or the terminal crashes, the session is on disk. After a successful
//! flush the file becomes `.done.jsonl`; on startup a file without `.done` =
//! a recoverable half-finished session. A recording error NEVER blocks the session.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{bail, Result};

use crate::anthropic::Message;

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

/// Successful close: `.jsonl` → `.done.jsonl`. Returns the new path.
pub fn mark_done(path: &Path) -> Result<PathBuf> {
    let done = path.with_extension("done.jsonl");
    std::fs::rename(path, &done)?;
    Ok(done)
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

/// Delete the given half-finished session records. Only ever called with the
/// list produced by `find_unfinished` — never touches `.done` files by construction.
/// Errors are collected, not fatal: a leftover record must never block startup.
pub fn delete_unflushed(files: &[PathBuf]) -> (usize, Vec<String>) {
    let mut deleted = 0;
    let mut errors = Vec::new();
    for f in files {
        match std::fs::remove_file(f) {
            Ok(()) => deleted += 1,
            Err(e) => errors.push(format!("{}: {e}", f.display())),
        }
    }
    (deleted, errors)
}

/// Read a transcript file written by `Recorder` (`{"role","text"}` per line)
/// and reconstruct it as `Message` history, preserving order. Any line that
/// fails to parse, or carries a role other than "user"/"assistant", fails
/// the whole read — a salvage flush must not silently drop turns.
pub fn read_history(path: &Path) -> Result<Vec<Message>> {
    let content = std::fs::read_to_string(path)?;
    let mut out = Vec::new();
    for l in content.lines() {
        if l.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(l)?;
        let role = v
            .get("role")
            .and_then(|r| r.as_str())
            .ok_or_else(|| anyhow::anyhow!("transcript line missing role: {l}"))?;
        let text = v
            .get("text")
            .and_then(|t| t.as_str())
            .ok_or_else(|| anyhow::anyhow!("transcript line missing text: {l}"))?;
        match role {
            "user" | "assistant" => out.push(Message {
                role: role.to_string(),
                content: serde_json::Value::String(text.to_string()),
            }),
            other => bail!("unknown role in transcript line: {other}"),
        }
    }
    Ok(out)
}

/// Recover the topic from a transcript filename by stripping the trailing
/// `-<YYYYMMDD>-<HHMMSS>` timestamp (see `now_stamp`'s format). The topic
/// itself may contain hyphens. `None` if the filename doesn't end in the
/// expected two numeric blocks.
pub fn topic_from_record(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let parts: Vec<&str> = stem.split('-').collect();
    if parts.len() < 2 {
        return None;
    }
    let last = parts[parts.len() - 1];
    let second_last = parts[parts.len() - 2];
    let is_digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    if !is_digits(last) || second_last.len() != 8 || !is_digits(second_last) {
        return None;
    }
    Some(parts[..parts.len() - 2].join("-"))
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

    #[test]
    fn read_history_roundtrips_recorder_output() {
        let base = std::env::temp_dir().join(format!(
            "usta_transcript_history_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let p = base.join("rust-1.jsonl");

        let r = Recorder::new(p.clone());
        r.user("merhaba");
        r.assistant("selam");
        r.user("nasılsın");

        let history = read_history(&p).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].role, "user");
        assert_eq!(history[0].content, serde_json::Value::String("merhaba".into()));
        assert_eq!(history[1].role, "assistant");
        assert_eq!(history[1].content, serde_json::Value::String("selam".into()));
        assert_eq!(history[2].role, "user");
        assert_eq!(history[2].content, serde_json::Value::String("nasılsın".into()));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn topic_from_record_strips_timestamp_keeps_hyphenated_topic() {
        assert_eq!(
            topic_from_record(Path::new("kaynak-ingest-20260814-153309.jsonl")).as_deref(),
            Some("kaynak-ingest")
        );
        assert_eq!(
            topic_from_record(Path::new("rust-20260807-1030.jsonl")).as_deref(),
            Some("rust")
        );
        assert!(topic_from_record(Path::new("garip.jsonl")).is_none());
    }

    #[test]
    fn mark_done_renames_and_unflushed_no_longer_finds_it() {
        let base = std::env::temp_dir().join(format!(
            "usta_transcript_markdone2_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let sdir = base.join(".usta/sessions");
        std::fs::create_dir_all(&sdir).unwrap();
        let p = sdir.join("x-20260807-1030.jsonl");
        std::fs::write(&p, "x").unwrap();

        let done = mark_done(&p).unwrap();
        assert!(done.ends_with("x-20260807-1030.done.jsonl"));
        assert!(done.exists());
        assert!(find_unfinished(&base).is_empty());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn delete_unflushed_removes_only_given_files_reports_errors() {
        let base = std::env::temp_dir().join(format!("usta_transcript_del_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let sdir = base.join(".usta/sessions");
        std::fs::create_dir_all(&sdir).unwrap();
        std::fs::write(sdir.join("a-1.jsonl"), "x").unwrap();
        std::fs::write(sdir.join("b-2.jsonl"), "x").unwrap();
        std::fs::write(sdir.join("c-3.done.jsonl"), "x").unwrap();

        let files = vec![sdir.join("a-1.jsonl"), sdir.join("b-2.jsonl"), sdir.join("yok.jsonl")];
        let (deleted, errors) = delete_unflushed(&files);
        assert_eq!(deleted, 2);
        assert_eq!(errors.len(), 1);
        assert!(!sdir.join("a-1.jsonl").exists());
        assert!(sdir.join("c-3.done.jsonl").exists()); // .done untouched

        let _ = std::fs::remove_dir_all(&base);
    }
}
