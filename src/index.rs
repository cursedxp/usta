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

/// Bu projede devam edilebilir konular: `.usta/learner/progress/*.md`
/// (boş olmayan) dosya adları. Sıralama: global index tarihi yeniden-eskiye;
/// index kaydı olmayan konu dosya mtime'ına düşer (factory reset sonrası
/// katalog boş olabilir — progress hâlâ gerçek kaynak). `[0]` = son konu.
/// Henüz hiçbir çağıran yok (kamu API) — welcome box + resume mantığı bir
/// sonraki task'ta buna bağlanacak; o zamana kadar clippy için allow.
#[allow(dead_code)]
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
            // Sıralama anahtarı: index tarihi (YYYY-MM-DD sıralanabilir);
            // yoksa mtime'dan üretilmiş kaba anahtar (epoch saniye, sabit genişlik).
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
    out.sort_by(|a, b| b.0.cmp(&a.0)); // yeniden-eskiye
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

    #[test]
    fn local_topics_lists_progress_stems_sorted_by_index_date_desc() {
        let base = std::env::temp_dir().join(format!("usta_localtopics_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let pdir = base.join(".usta/learner/progress");
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(pdir.join("eski-konu.md"), "içerik").unwrap();
        std::fs::write(pdir.join("yeni-konu.md"), "içerik").unwrap();
        std::fs::write(pdir.join("bos.md"), "  ").unwrap(); // boş → listelenmez
        let index = format!(
            "## Kayıtlar\n- eski-konu | {p} | 2026-08-01\n- yeni-konu | {p} | 2026-08-07\n- baska-proje-konu | /tmp/baska | 2026-08-06\n",
            p = base.display()
        );
        let t = local_topics(&base, &index);
        assert_eq!(t, vec!["yeni-konu".to_string(), "eski-konu".to_string()]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn local_topics_without_index_entry_still_lists_by_mtime() {
        let base = std::env::temp_dir().join(format!("usta_localtopics_mtime_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let pdir = base.join(".usta/learner/progress");
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(pdir.join("tek-konu.md"), "içerik").unwrap();
        let t = local_topics(&base, ""); // index boş (factory reset senaryosu)
        assert_eq!(t, vec!["tek-konu".to_string()]);
        let _ = std::fs::remove_dir_all(&base);
    }
}
