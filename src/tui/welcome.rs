//! Açılış kutusu: veri toplama (saf) + render. Spec §5.
//! Tüm parse'lar best-effort — bozuk/eksik girdi alanı atlar, asla panik yok.

/// Açılış kutusunun tüm verisi — render bu struct'tan çizer, IO yapmaz.
pub struct WelcomeData {
    pub version: &'static str,
    pub name: Option<String>,
    pub model: String,
    pub dir: String,
    pub topic: String,
    pub level: Option<String>,
    pub map_percent: Option<u8>,
    pub next_item: Option<String>,
    pub drill_count: usize,
    pub first_session: bool,
}

/// `## {header}` başlığından bir sonraki `## `e kadarki gövde.
fn section<'a>(md: &'a str, header: &str) -> Option<&'a str> {
    let needle = format!("## {header}");
    let start = md.find(&needle)? + needle.len();
    let rest = &md[start..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    Some(&rest[..end])
}

/// `# Öğrenci Profili — Anil` → `Anil` (em-dash veya tire sonrası).
pub fn extract_name(profile: &str) -> Option<String> {
    let h1 = profile.lines().find(|l| l.starts_with("# "))?;
    let name = h1.rsplit(['—', '-']).next()?.trim();
    if name.is_empty() || name.contains("Profil") || name.starts_with('#') { return None; }
    Some(name.to_string())
}

/// `## Seviye` bölümünün ilk dolu satırı, liste işareti soyulmuş.
pub fn extract_level(progress: &str) -> Option<String> {
    section(progress, "Seviye")?
        .lines()
        .map(|l| l.trim().trim_start_matches(['-', '*', ' ']).trim())
        .find(|l| !l.is_empty())
        .map(String::from)
}

const STATUSES: [&str; 4] = ["görülmedi", "görüldü", "oturdu", "derinleşildi"];

/// Durum içeren satır sayımından harita yüzdesi: görülmedi-olmayan / toplam.
pub fn curriculum_percent(curriculum: &str) -> Option<u8> {
    let (mut total, mut seen) = (0u32, 0u32);
    for line in curriculum.lines() {
        if line.contains("görülmedi") { total += 1; }
        else if STATUSES[1..].iter().any(|s| line.contains(s)) { total += 1; seen += 1; }
    }
    if total == 0 { return None; }
    Some(((seen * 100) / total) as u8)
}

/// İlk `görülmedi` maddesinin metni — liste işareti ve durum eki soyulur.
pub fn next_unseen(curriculum: &str) -> Option<String> {
    let line = curriculum.lines().find(|l| l.contains("görülmedi"))?;
    let text = line.split("görülmedi").next()?
        .trim()
        .trim_start_matches(['-', '*', ' '])
        .trim_end_matches([':', '—', '-', '·', '|', ' ']);
    if text.is_empty() { None } else { Some(text.to_string()) }
}

/// `## Geri çağırma soruları` bölümündeki madde sayısı.
pub fn drill_count(progress: &str) -> usize {
    section(progress, "Geri çağırma soruları")
        .map(|s| s.lines().filter(|l| l.trim().starts_with('-')).count())
        .unwrap_or(0)
}

/// Dosya içeriklerinden WelcomeData kur — hepsi Option, eksik = alan atlanır.
pub fn gather(
    profile: Option<&str>, progress: Option<&str>, curriculum: Option<&str>,
    topic: &str, model: &str, dir: &str,
) -> WelcomeData {
    WelcomeData {
        version: env!("CARGO_PKG_VERSION"),
        name: profile.and_then(extract_name),
        model: model.to_string(),
        dir: dir.to_string(),
        topic: topic.to_string(),
        level: progress.and_then(extract_level),
        map_percent: curriculum.and_then(curriculum_percent),
        next_item: curriculum.and_then(next_unseen),
        drill_count: progress.map(drill_count).unwrap_or(0),
        first_session: progress.is_none(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROFILE: &str = "# Öğrenci Profili — Anil\n\n## Kim\n- test";
    const PROGRESS: &str = "# rust — İlerleme\n## Seviye\n- Orta: ownership oturdu\n## Geri çağırma soruları\n- Soru 1? — cevap\n- Soru 2? — cevap\n- Soru 3? — cevap\n";
    const CURRICULUM: &str = "# rust haritası\n- Ownership: oturdu\n- Borrowing: görüldü\n- Lifetimes: görülmedi\n- Traits: görülmedi\n";

    #[test]
    fn extract_name_reads_h1_after_dash() {
        assert_eq!(extract_name(PROFILE), Some("Anil".to_string()));
        assert_eq!(extract_name("# Başlıksız"), None);
        assert_eq!(extract_name(""), None);
    }

    #[test]
    fn extract_level_reads_first_line_of_section() {
        assert_eq!(extract_level(PROGRESS), Some("Orta: ownership oturdu".to_string()));
        assert_eq!(extract_level("# boş"), None);
    }

    #[test]
    fn curriculum_percent_counts_non_unseen() {
        // 4 durumlu madde, 2'si görülmedi → %50
        assert_eq!(curriculum_percent(CURRICULUM), Some(50));
        assert_eq!(curriculum_percent("# durum yok"), None);
    }

    #[test]
    fn next_unseen_returns_first_unseen_item_text() {
        assert_eq!(next_unseen(CURRICULUM), Some("Lifetimes".to_string()));
        assert_eq!(next_unseen("- Hepsi: oturdu"), None);
    }

    #[test]
    fn drill_count_counts_section_bullets() {
        assert_eq!(drill_count(PROGRESS), 3);
        assert_eq!(drill_count("# soru yok"), 0);
    }

    #[test]
    fn gather_full_and_first_session() {
        let d = gather(Some(PROFILE), Some(PROGRESS), Some(CURRICULUM), "rust", "opus · cli", "~/x");
        assert!(!d.first_session);
        assert_eq!(d.name.as_deref(), Some("Anil"));
        assert_eq!(d.map_percent, Some(50));
        let d2 = gather(None, None, None, "gtm", "opus · cli", "~/x");
        assert!(d2.first_session);
        assert_eq!(d2.drill_count, 0);
    }
}
