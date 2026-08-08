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

/// Konuya özel yaklaşım dosyası: `.usta/approaches/<konu>.md`.
pub fn approach_path(project_root: &Path, topic: &str) -> PathBuf {
    project_root.join(".usta/approaches").join(format!("{topic}.md"))
}

/// Konunun müfredat haritası: `.usta/learner/curriculum/<konu>.md`.
pub fn curriculum_path(project_root: &Path, topic: &str) -> PathBuf {
    project_root
        .join(".usta/learner/curriculum")
        .join(format!("{topic}.md"))
}

/// Kapanış yanıtı bölücüsü — model her dosyayı bununla başlatır.
pub const FILE_DELIM: &str = "===DOSYA:";

/// Kapanış yanıtını (ad, içerik) çiftlerine ayır. Bölücü yoksa tüm yanıt
/// tek "progress" dosyası sayılır — eski format geriye uyumlu kalır.
pub fn split_files(reply: &str) -> Vec<(String, String)> {
    if !reply.contains(FILE_DELIM) {
        return vec![("progress".to_string(), clean_markdown_reply(reply))];
    }
    let mut out = Vec::new();
    for chunk in reply.split(FILE_DELIM).skip(1) {
        let Some((header, body)) = chunk.split_once("===") else {
            continue;
        };
        let name = header.trim().to_string();
        if name.is_empty() {
            continue;
        }
        out.push((name, clean_markdown_reply(body)));
    }
    out
}

/// Kapanış çağrısının user-turn içeriği: üç dosyanın mevcut hali + üretim
/// kuralları. progress her zaman; approach/curriculum canlı belge —
/// ilk oturumda veya değiştiğinde üretilir (USTA.md "Kapsam Bekçiliği").
pub fn closing_prompt(
    topic: &str,
    progress: Option<&str>,
    approach: Option<&str>,
    curriculum: Option<&str>,
) -> String {
    let p = progress.unwrap_or("(dosya henüz yok)");
    let a = approach.unwrap_or("(dosya henüz yok)");
    let c = curriculum.unwrap_or("(dosya henüz yok)");
    format!(
        "[OTURUM KAPANIYOR — DOSYA GÜNCELLEME]\n\
         Görev: aşağıdaki üç dosyadan güncellenmesi gerekenleri üret. Her dosyayı şu \
         satırla başlat: `===DOSYA: <ad>===` (ad: progress | approach | curriculum).\n\n\
         Mevcut progress ({topic}):\n---\n{p}\n---\n\n\
         Mevcut approach:\n---\n{a}\n---\n\n\
         Mevcut curriculum:\n---\n{c}\n---\n\n\
         Kurallar:\n\
         - `progress` HER ZAMAN üretilir. Yapı: `# {topic} — İlerleme` başlığı + \
         `## Seviye` / `## Kapatılanlar` / `## Gap'ler` (KANITLA) / \
         `## Geri çağırma soruları` (3-5 soru + tek satır cevap; oturmuş eskileri çıkar, \
         bu oturumdan yenileri ekle) / `## Hata günlüğü` (`tip | kaç kez | son örnek`, \
         3+ tekrar = GAP ADAYI) / `## İpucu merdiveni` / `## Hedef Durumu` — SADECE \
         approach'ta `## Hedef` tanımlıysa yaz: kalan süre (BUGÜN bölümünden hesapla), \
         harita ilerlemesi (%), tempo değerlendirmesi (yetişir / riskli / yetişmez + tek \
         cümle gerekçe), ölçüm logu (`tarih | ölçüm | skor` — deneme sınavı, yazma \
         değerlendirmesi vb.). Hedef yoksa bu bölümü hiç yazma.\n\
         - `approach` yalnız ilk oturumda veya yaklaşım bu oturumda değiştiyse üretilir — \
         canlı belge, _default.md'deki üç soruya cevap verir (pratik / çıktı / feedback). \
         Hedefli öğrenmede approach `## Hedef` bölümü içerir: ne (sertifika/seviye/çıktı), \
         sınav-değerlendirme tarihi (YYYY-MM-DD), geçme eşiği, sınav/değerlendirme formatı.\n\
         - `curriculum` ilk oturumda TAM harita olarak çıkarılır (konu/alt-konu ağacı; her \
         madde `görülmedi/görüldü/oturdu/derinleşildi` durumuyla; gerekiyorsa web \
         araştırmasına dayan); sonraki oturumlarda yalnız durum değiştiyse üretilir. \
         Kapsanmamış kritik madde haritada görünür kalmalı.\n\
         - Dosyaları ŞİŞİRME: `Kapatılanlar` 20 maddeyi aşarsa en eskileri tek satırlık \
         dönem özetine indir; `Hata günlüğü`nde çözülüp uzun süredir görülmeyen satırları \
         kaldır; curriculum'da değişmeyen bölümleri olduğu gibi koru (yeniden üretme).\n\
         - Oturumda kanıtı olmayanı ekleme; mevcut dosyalardaki geçerli bilgiyi koru \
         (kullanıcı elle düzenlemiş olabilir — düzenlemesini ez-me).\n\
         - Bölücü satırları dışında açıklama/selamlama yazma; her dosya saf markdown."
    )
}

/// Açılış drilli turn'ü: progress varsa oturum başında Usta ilk sözü alır ve
/// geri çağırma sorusu sorar (testing effect — USTA.md "Açılış Drilli" kuralı).
/// main.rs'e bağlandığı yer: Task 3 (açılış drilli tetiği).
pub fn opening_prompt(topic: &str) -> String {
    format!(
        "[OTURUM AÇILIŞI — GERİ ÇAĞIRMA DRİLLİ]\n\
         Konu: {topic}. Progress dosyandaki 'Geri çağırma soruları'ndan 2-3 tanesini seç \
         ve bana SOR — cevaplarını verme, anlatma. Kısa tut: 2 dakikalık ısınma, sonra \
         günün işine geçeriz. Progress'te soru yoksa seviyeme uygun 2 küçük hatırlama \
         sorusu üret. Drill bitince haritadan tek cümle söyle: neredeyiz, sırada ne var \
         (curriculum dosyan system prompt'ta)."
    )
}

/// Yeni konu tanışma turn'ü: yaklaşım + müfredat haritası henüz yok — Usta
/// açık sohbetle türetir (USTA.md "Yeni Konu Tanışması"). Sabit form değil:
/// kullanıcının söylediğinden türetilir, yön kullanıcıda kalır.
pub fn onboarding_prompt(topic: &str) -> String {
    format!(
        "[YENİ KONU — TANIŞMA]\n\
         Konu: {topic}. Bu konunun yaklaşımı ve müfredat haritası henüz yok.\n\
         Kısa, DOĞAL bir tanışma yap — bu bir form değil: tek mesajda en fazla iki soru \
         sor, cevaba göre devam et; numaralı soru listesi basma. Öğren: ne yapmak/öğrenmek \
         istiyor, elinde ne var. Keşif/hedef ayrımını KENDİN çıkar — kullanıcıya bu \
         terimlerle sorma; söylediklerinden belli olmuyorsa jargonsuz tek soru: 'belirli \
         bir tarihe/sınava mı hazırlanıyorsun, yoksa merakına mı bakıyoruz?'. Hedefliyse \
         ne/tarih/eşik/format bilgisini sohbet içinde topla — approach'un `## Hedef` \
         bölümüne yazılacak; harita resmi çerçeveden kurulur (sınav müfredatı / exam \
         guide / CEFR) — web'de araştır. Alanı yeterince bilmiyorsan web'de araştır. \
         Oturum kapanışında senden yaklaşım + TAM müfredat haritası İÇERİĞİ istenecek; \
         dosyaları Usta kabuğu yazar, sen oturum içinde dosya yazmaya çalışma (Sert Kural 6) — \
         tanışmayı buna göre derinleştir ama derse çevirme, kısa tut."
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
    // Önceki sürümü yedekle — kötü model çıktısı tek kopyayla geri alınır.
    if path.exists() {
        let bak = path.with_extension("md.bak");
        let _ = std::fs::copy(path, &bak);
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
        let s = closing_prompt("rust", Some("- Seviye: orta"), None, None);
        assert!(s.contains("rust"));
        assert!(s.contains("- Seviye: orta"));
    }

    #[test]
    fn closing_prompt_marks_missing_file() {
        let s = closing_prompt("rust", None, None, None);
        assert!(s.contains("(dosya henüz yok)"));
    }

    #[test]
    fn closing_prompt_includes_pruning_rule() {
        let s = closing_prompt("rust", None, None, None);
        assert!(s.contains("20 madde"));
    }

    #[test]
    fn closing_prompt_requests_rich_sections() {
        let s = closing_prompt("rust", None, None, None);
        assert!(s.contains("Geri çağırma soruları"));
        assert!(s.contains("Hata günlüğü"));
        assert!(s.contains("İpucu merdiveni"));
    }

    #[test]
    fn paths_build_expected_layout() {
        assert_eq!(
            approach_path(Path::new("/proje"), "gtm"),
            Path::new("/proje/.usta/approaches/gtm.md")
        );
        assert_eq!(
            curriculum_path(Path::new("/proje"), "gtm"),
            Path::new("/proje/.usta/learner/curriculum/gtm.md")
        );
    }

    #[test]
    fn split_files_without_delimiter_is_progress() {
        let out = split_files("# Rust — İlerleme\niçerik");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "progress");
        assert!(out[0].1.contains("içerik"));
    }

    #[test]
    fn split_files_separates_three_files() {
        let reply = "===DOSYA: progress===\nP İÇERİK\n===DOSYA: approach===\nA İÇERİK\n===DOSYA: curriculum===\nC İÇERİK\n";
        let out = split_files(reply);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0], ("progress".to_string(), "P İÇERİK".to_string()));
        assert_eq!(out[1], ("approach".to_string(), "A İÇERİK".to_string()));
        assert_eq!(out[2], ("curriculum".to_string(), "C İÇERİK".to_string()));
    }

    #[test]
    fn split_files_cleans_fenced_content() {
        let reply = "===DOSYA: progress===\n```markdown\n# başlık\n```\n";
        let out = split_files(reply);
        assert_eq!(out[0].1, "# başlık");
    }

    #[test]
    fn closing_prompt_embeds_all_three_currents_and_delimiter() {
        let s = closing_prompt("rust", Some("PMEVCUT"), Some("AMEVCUT"), Some("CMEVCUT"));
        assert!(s.contains("PMEVCUT"));
        assert!(s.contains("AMEVCUT"));
        assert!(s.contains("CMEVCUT"));
        assert!(s.contains("===DOSYA:"));
        assert!(s.contains("görülmedi/görüldü/oturdu/derinleşildi"));
    }

    #[test]
    fn closing_prompt_defines_goal_sections() {
        let s = closing_prompt("almanca", None, None, None);
        assert!(s.contains("## Hedef Durumu"));
        assert!(s.contains("## Hedef"));
        assert!(s.contains("tempo"));
    }

    #[test]
    fn onboarding_prompt_infers_goal_without_jargon_and_limits_questions() {
        let s = onboarding_prompt("almanca");
        // Keşif/hedef terimleri kullanıcıya SORULMAZ — model kendisi çıkarır.
        assert!(!s.contains("keşif mi"));
        assert!(s.contains("KENDİN çıkar"));
        // Jargonsuz yedek soru + soru limiti.
        assert!(s.contains("tarihe/sınava"));
        assert!(s.contains("en fazla iki soru"));
        assert!(s.contains("Hedef"));
    }

    #[test]
    fn opening_prompt_embeds_topic_and_asks_to_quiz() {
        let s = opening_prompt("rust");
        assert!(s.contains("rust"));
        assert!(s.contains("GERİ ÇAĞIRMA DRİLLİ"));
        assert!(s.contains("SOR"));
    }

    #[test]
    fn onboarding_prompt_embeds_topic_and_open_conversation() {
        let s = onboarding_prompt("linux-guvenlik");
        assert!(s.contains("linux-guvenlik"));
        assert!(s.contains("TANIŞMA"));
        assert!(s.contains("form"));
    }

    #[test]
    fn onboarding_prompt_does_not_tell_model_it_writes_files() {
        // Sert Kural 6: modelin dosya yazma aracı yok — kapanış içeriğini üretir,
        // dosyayı kabuk yazar. Prompt modeli yazma denemesine itmemeli.
        let s = onboarding_prompt("rust");
        assert!(!s.contains("dosyalara yazacaksın"));
        assert!(s.contains("kabuğu yazar"));
    }

    #[test]
    fn opening_prompt_mentions_curriculum_position() {
        let s = opening_prompt("rust");
        assert!(s.contains("harita"));
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

    #[test]
    fn write_atomic_backs_up_previous_version() {
        let base = std::env::temp_dir().join(format!(
            "usta_progress_bak_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let target = base.join("rust.md");
        write_atomic(&target, "ilk sürüm").unwrap();
        assert!(!target.with_extension("md.bak").exists()); // ilk yazımda yedek yok
        write_atomic(&target, "ikinci sürüm").unwrap();
        assert_eq!(
            std::fs::read_to_string(target.with_extension("md.bak")).unwrap(),
            "ilk sürüm"
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "ikinci sürüm");
        let _ = std::fs::remove_dir_all(&base);
    }
}
