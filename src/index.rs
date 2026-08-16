//! Global learning catalog: the `tokens::H_RECORDS` section at the end of
//! `~/.config/usta/learner/index.md`. Line format: `- topic | project-path | YYYY-MM-DD`.
//! The closing flush upserts it → "where I'm learning what" is visible at a glance;
//! `usta topics` lists them, factory reset finds project paths from here.
//! The section lives at the END of the file — free text above it is preserved.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::tokens;

const SECTION: &str = tokens::H_RECORDS;

/// A single entry in the catalog.
#[derive(Debug, PartialEq)]
pub struct IndexEntry {
    pub topic: String,
    pub project: PathBuf,
    pub date: String,
}

/// Parse the `- topic | path | date` lines under `tokens::H_RECORDS`.
/// Empty if the section is missing; lines that don't match the format are silently skipped.
pub fn entries(content: &str) -> Vec<IndexEntry> {
    let Some(idx) = content.find(SECTION) else {
        return Vec::new();
    };
    content[idx..]
        .lines()
        .filter_map(|l| {
            let rest = l.strip_prefix("- ")?;
            let mut parts = rest.splitn(3, '|').map(str::trim);
            let topic = parts.next()?.to_string();
            let project = PathBuf::from(parts.next()?);
            let date = parts.next()?.to_string();
            Some(IndexEntry { topic, project, date })
        })
        .collect()
}

/// Add/update the (topic, project) line — if the section doesn't exist, it's opened at the end of the file.
pub fn upsert(content: &str, topic: &str, project: &Path, date: &str) -> String {
    let mut list = entries(content);
    match list
        .iter_mut()
        .find(|e| e.topic == topic && e.project == project)
    {
        Some(e) => e.date = date.to_string(),
        None => list.push(IndexEntry {
            topic: topic.to_string(),
            project: project.to_path_buf(),
            date: date.to_string(),
        }),
    }
    render(content, &list)
}

/// Preserve the free text before the section, rewrite `tokens::H_RECORDS` with the lines.
fn render(content: &str, list: &[IndexEntry]) -> String {
    let prefix = match content.find(SECTION) {
        Some(idx) => &content[..idx],
        None => content,
    };
    let mut out = prefix.trim_end().to_string();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(SECTION);
    out.push('\n');
    for e in list {
        out.push_str(&format!("- {} | {} | {}\n", e.topic, e.project.display(), e.date));
    }
    out
}

/// Drop the (topic, project) line — if there's no match, entries remain unchanged.
pub fn remove(content: &str, topic: &str, project: &Path) -> String {
    let list: Vec<IndexEntry> = entries(content)
        .into_iter()
        .filter(|e| !(e.topic == topic && e.project == project))
        .collect();
    render(content, &list)
}

/// Called on close: read the catalog → upsert → write atomically.
pub fn record(global: &Path, topic: &str, project: &Path, date: &str) -> Result<()> {
    let path = global.join("learner/index.md");
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    let updated = upsert(&current, topic, project, date);
    crate::progress::write_atomic(&path, &updated)
}

/// Topics that can be resumed in this project: (non-empty) file names under
/// `.usta/learner/progress/*.md`. Sorted by global index date, newest to oldest;
/// a topic with no index entry falls back to the file's mtime (the catalog can be
/// empty after a factory reset — progress is still the source of truth). `[0]` = most recent topic.
/// The identity welcome box (numbered list) + resume selection logic reads this.
pub fn local_topics(project_root: &Path, index_content: &str) -> Vec<String> {
    let dir = project_root.join(".usta/learner/progress");
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let idx = entries(index_content);
    let date_of = |topic: &str| -> Option<String> {
        idx.iter()
            .find(|e| e.topic == topic && e.project == project_root)
            .map(|e| e.date.clone())
    };
    let mut out: Vec<(String, String)> = rd
        .flatten()
        .filter_map(|f| {
            let p = f.path();
            if p.extension().and_then(|e| e.to_str()) != Some("md") { return None; }
            let stem = p.file_stem()?.to_str()?.to_string();
            let content = std::fs::read_to_string(&p).ok()?;
            if content.trim().is_empty() { return None; }
            // Sort key: index date (YYYY-MM-DD is sortable);
            // otherwise a coarse key derived from mtime (epoch seconds, fixed width).
            let key = date_of(&stem).unwrap_or_else(|| {
                let secs = f.metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                format!("0000-epoch-{secs:020}")
            });
            Some((key, stem))
        })
        .collect();
    out.sort_by(|a, b| b.0.cmp(&a.0)); // newest to oldest
    out.into_iter().map(|(_, t)| t).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn entries_empty_when_no_section() {
        assert!(entries("# Katalog\nserbest metin").is_empty());
    }

    #[test]
    fn upsert_creates_section_preserving_prose() {
        let out = upsert("# Katalog\naçıklama satırı", "rust", Path::new("/p/a"), "2026-08-07");
        assert!(out.contains("açıklama satırı"));
        assert!(out.contains("## Records"));
        assert!(out.contains("- rust | /p/a | 2026-08-07"));
    }

    #[test]
    fn upsert_updates_date_without_duplicating() {
        let v1 = upsert("", "rust", Path::new("/p/a"), "2026-08-01");
        let v2 = upsert(&v1, "rust", Path::new("/p/a"), "2026-08-07");
        assert_eq!(entries(&v2).len(), 1);
        assert_eq!(entries(&v2)[0].date, "2026-08-07");
    }

    #[test]
    fn upsert_same_topic_different_project_adds_line() {
        let v1 = upsert("", "rust", Path::new("/p/a"), "2026-08-01");
        let v2 = upsert(&v1, "rust", Path::new("/p/b"), "2026-08-07");
        assert_eq!(entries(&v2).len(), 2);
    }

    #[test]
    fn entries_parses_topic_project_date() {
        let content = "önsöz\n\n## Records\n- rust | /p/a | 2026-08-07\n- js | /p/b | 2026-08-01\n";
        let list = entries(content);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].topic, "rust");
        assert_eq!(list[0].project, Path::new("/p/a").to_path_buf());
        assert_eq!(list[1].date, "2026-08-01");
    }

    #[test]
    fn entries_skips_malformed_lines() {
        let content = "## Records\n- bozuk satır boru yok\n- rust | /p/a | 2026-08-07\n";
        assert_eq!(entries(content).len(), 1);
    }

    #[test]
    fn remove_drops_only_matching_line() {
        let v = upsert(
            &upsert("", "rust", Path::new("/p/a"), "2026-08-07"),
            "js",
            Path::new("/p/a"),
            "2026-08-07",
        );
        let out = remove(&v, "rust", Path::new("/p/a"));
        let list = entries(&out);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].topic, "js");
    }

    #[test]
    fn remove_without_match_keeps_entries() {
        let v = upsert("", "rust", Path::new("/p/a"), "2026-08-07");
        let out = remove(&v, "rust", Path::new("/p/BASKA"));
        assert_eq!(entries(&out).len(), 1);
    }

    #[test]
    fn local_topics_lists_progress_stems_sorted_by_index_date_desc() {
        let base = std::env::temp_dir().join(format!("usta_localtopics_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let pdir = base.join(".usta/learner/progress");
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(pdir.join("eski-konu.md"), "içerik").unwrap();
        std::fs::write(pdir.join("yeni-konu.md"), "içerik").unwrap();
        std::fs::write(pdir.join("bos.md"), "  ").unwrap(); // empty → not listed
        let index = format!(
            "## Records\n- eski-konu | {p} | 2026-08-01\n- yeni-konu | {p} | 2026-08-07\n- baska-proje-konu | /tmp/baska | 2026-08-06\n",
            p = base.display()
        );
        let t = local_topics(&base, &index);
        assert_eq!(t, vec!["yeni-konu".to_string(), "eski-konu".to_string()]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn record_writes_resumable_catalog_row_without_flush() {
        let base = std::env::temp_dir().join(format!("usta_index_open_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        // A single open-time upsert — no flush, no progress file — must still catalog the project.
        record(&base, "rust", Path::new("/p/a"), "2026-08-16").unwrap();

        let content = std::fs::read_to_string(base.join("learner/index.md")).unwrap();
        let list = entries(&content);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].topic, "rust");
        assert_eq!(list[0].project, Path::new("/p/a").to_path_buf());
        assert_eq!(list[0].date, "2026-08-16");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn local_topics_without_index_entry_still_lists_by_mtime() {
        let base = std::env::temp_dir().join(format!("usta_localtopics_mtime_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let pdir = base.join(".usta/learner/progress");
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(pdir.join("tek-konu.md"), "içerik").unwrap();
        let t = local_topics(&base, ""); // index empty (factory reset scenario)
        assert_eq!(t, vec!["tek-konu".to_string()]);
        let _ = std::fs::remove_dir_all(&base);
    }
}
