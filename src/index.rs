//! Global öğrenme kataloğu: `~/.config/usta/learner/index.md` sonundaki
//! `## Kayıtlar` bölümü. Satır formatı `- konu | proje-yolu | YYYY-MM-DD`.
//! Kapanış flush'ı upsert eder → "nerede ne öğreniyorum" tek bakışta görünür;
//! `usta topics` listeler, factory reset proje yollarını buradan bulur.
//! Bölüm dosyanın SONUNDA yaşar — üstündeki serbest metin korunur.

use std::path::{Path, PathBuf};

use anyhow::Result;

const SECTION: &str = "## Kayıtlar";

/// Katalogdaki tek kayıt.
#[derive(Debug, PartialEq)]
pub struct IndexEntry {
    pub topic: String,
    pub project: PathBuf,
    pub date: String,
}

/// `## Kayıtlar` altındaki `- konu | yol | tarih` satırlarını ayrıştır.
/// Bölüm yoksa boş; formata uymayan satır sessizce atlanır.
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

/// (konu, proje) satırını ekle/güncelle — bölüm yoksa dosya sonuna açılır.
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

/// Bölüm-öncesi serbest metni koru, `## Kayıtlar`ı satırlarla yeniden yaz.
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

/// (konu, proje) satırını düş — eşleşme yoksa kayıtlar değişmeden kalır.
pub fn remove(content: &str, topic: &str, project: &Path) -> String {
    let list: Vec<IndexEntry> = entries(content)
        .into_iter()
        .filter(|e| !(e.topic == topic && e.project == project))
        .collect();
    render(content, &list)
}

/// Kapanışta çağrılır: kataloğu oku → upsert → atomik yaz.
pub fn record(global: &Path, topic: &str, project: &Path, date: &str) -> Result<()> {
    let path = global.join("learner/index.md");
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    let updated = upsert(&current, topic, project, date);
    crate::progress::write_atomic(&path, &updated)
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
        assert!(out.contains("## Kayıtlar"));
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
        let content = "önsöz\n\n## Kayıtlar\n- rust | /p/a | 2026-08-07\n- js | /p/b | 2026-08-01\n";
        let list = entries(content);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].topic, "rust");
        assert_eq!(list[0].project, Path::new("/p/a").to_path_buf());
        assert_eq!(list[1].date, "2026-08-01");
    }

    #[test]
    fn entries_skips_malformed_lines() {
        let content = "## Kayıtlar\n- bozuk satır boru yok\n- rust | /p/a | 2026-08-07\n";
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
}
