//! Kalıcı hafıza: oturum kapanışında Usta'ya oturumu özetletip
//! `.usta/learner/progress/<konu>.md`'yi TAM içerik olarak yeniden yazdırırız.
//! Sonraki oturum bu dosyayı system prompt'a yükler (brain.rs) → Usta
//! bildiğini tekrar anlatmaz, eksiği hedefler. SPEC §9'un gerçeklenmesi.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Konu için progress dosya yolu: `<proje>/.usta/learner/progress/<konu>.md`.
pub fn progress_path(project_root: &Path, topic: &str) -> PathBuf {
    project_root
        .join(".usta/learner/progress")
        .join(format!("{topic}.md"))
}

/// Kapanış çağrısının user-turn içeriği: mevcut dosya + katı üretim kuralları.
/// Format pedagoji katmanını taşır: geri çağırma soruları (açılış drilli
/// bunlardan seçer), hata günlüğü (tekrar = gap adayı), merdiven notu (fading).
pub fn closing_prompt(topic: &str, existing: Option<&str>) -> String {
    let current = existing.unwrap_or("(dosya henüz yok)");
    format!(
        "[OTURUM KAPANIYOR — PROGRESS GÜNCELLEME]\n\
         Görev: `.usta/learner/progress/{topic}.md` dosyasının YENİ TAM içeriğini üret.\n\n\
         Mevcut dosya:\n---\n{current}\n---\n\n\
         Kurallar:\n\
         - Bu oturumdaki konuşmaya ve dosya feedback'lerine göre güncelle.\n\
         - Yapı: `# {topic} — İlerleme` başlığı + şu bölümler:\n\
           `## Seviye` — tek satır durum.\n\
           `## Kapatılanlar` — madde madde.\n\
           `## Gap'ler` — KANITLA (hangi kodda/konuşmada görüldü).\n\
           `## Geri çağırma soruları` — 3-5 soru + tek satır cevap. Sonraki oturumun \
           açılış drilli bunlardan seçer: bu oturumda kapatılan konudan yeni soru ekle, \
           iyice oturmuş eskileri çıkar.\n\
           `## Hata günlüğü` — `hata tipi | kaç kez | son örnek` satırları. Bu oturumda \
           görülen derleme/mantık hatalarını mevcut satırlarla BİRLEŞTİR (sayaç artır). \
           3+ tekrar eden tipin yanına `GAP ADAYI` yaz.\n\
           `## İpucu merdiveni` — hangi konuda hangi basamakta takıldı (fading kararı için).\n\
         - Oturumda kanıtı olmayan hiçbir şeyi ekleme, mevcut dosyadaki hâlâ geçerli bilgiyi koru.\n\
         - SADECE dosya içeriğini döndür — açıklama, selamlama, kod bloğu işareti yok."
    )
}

/// Açılış drilli turn'ü: progress varsa oturum başında Usta ilk sözü alır ve
/// geri çağırma sorusu sorar (testing effect — USTA.md "Açılış Drilli" kuralı).
/// main.rs'e bağlanması Task 3'te — o güne kadar dead_code uyarısını bastır.
#[allow(dead_code)]
pub fn opening_prompt(topic: &str) -> String {
    format!(
        "[OTURUM AÇILIŞI — GERİ ÇAĞIRMA DRİLLİ]\n\
         Konu: {topic}. Progress dosyandaki 'Geri çağırma soruları'ndan 2-3 tanesini seç \
         ve bana SOR — cevaplarını verme, anlatma. Kısa tut: 2 dakikalık ısınma, sonra \
         günün işine geçeriz. Progress'te soru yoksa seviyeme uygun 2 küçük hatırlama \
         sorusu üret."
    )
}

/// Model yanıtındaki olası ```-fence sargısını soy — dosyaya saf markdown yazılır.
pub fn clean_markdown_reply(reply: &str) -> String {
    let t = reply.trim();
    if let Some(rest) = t.strip_prefix("```") {
        // İlk satır fence etiketi (```markdown vb.) — at.
        let body = rest.split_once('\n').map(|(_, b)| b).unwrap_or("");
        let body = body.trim_end();
        let body = body.strip_suffix("```").unwrap_or(body);
        return body.trim().to_string();
    }
    t.to_string()
}

/// Atomik yazım: tmp'ye yaz, üstüne taşı — yarım dosya asla kalmaz.
pub fn write_atomic(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("dizin oluşturulamadı: {}", parent.display()))?;
    }
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, content)
        .with_context(|| format!("yazılamadı: {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("taşınamadı: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn progress_path_builds_expected_layout() {
        let p = progress_path(Path::new("/proje"), "rust");
        assert_eq!(
            p,
            Path::new("/proje/.usta/learner/progress/rust.md")
        );
    }

    #[test]
    fn closing_prompt_embeds_topic_and_existing() {
        let s = closing_prompt("rust", Some("- Seviye: orta"));
        assert!(s.contains("rust"));
        assert!(s.contains("- Seviye: orta"));
    }

    #[test]
    fn closing_prompt_marks_missing_file() {
        let s = closing_prompt("rust", None);
        assert!(s.contains("(dosya henüz yok)"));
    }

    #[test]
    fn closing_prompt_requests_rich_sections() {
        let s = closing_prompt("rust", None);
        assert!(s.contains("Geri çağırma soruları"));
        assert!(s.contains("Hata günlüğü"));
        assert!(s.contains("İpucu merdiveni"));
    }

    #[test]
    fn opening_prompt_embeds_topic_and_asks_to_quiz() {
        let s = opening_prompt("rust");
        assert!(s.contains("rust"));
        assert!(s.contains("GERİ ÇAĞIRMA DRİLLİ"));
        assert!(s.contains("SOR"));
    }

    #[test]
    fn clean_reply_strips_fenced_block() {
        let raw = "```markdown\n# Rust — İlerleme\n- Seviye: orta\n```";
        assert_eq!(
            clean_markdown_reply(raw),
            "# Rust — İlerleme\n- Seviye: orta"
        );
    }

    #[test]
    fn clean_reply_passes_plain_text_through() {
        assert_eq!(clean_markdown_reply("  # Başlık\niçerik  "), "# Başlık\niçerik");
    }

    #[test]
    fn write_atomic_creates_parents_and_writes() {
        let base = std::env::temp_dir().join(format!(
            "usta_progress_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let target = base.join("derin/dizin/rust.md");
        write_atomic(&target, "içerik").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "içerik");
        // tmp dosyası kalmamalı.
        assert!(!target.with_extension("md.tmp").exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
