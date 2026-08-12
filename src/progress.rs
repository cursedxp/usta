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

/// Profil boşken açılış turn'lerine eklenen tanışma talimatı (spec Ç3a).
const MEET_BLOCK: &str = "\n[PROFILE EMPTY] You don't know the user yet. Introduce \
yourself briefly at the start of the conversation — ask their name, their background \
with this topic, how they like to learn. At most 1-2 questions, not a form; don't \
delay getting into the topic. If the user already introduced themselves, don't ask \
again. What you learn will be written to their profile at session close.\n";

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
    profile: Option<&str>,
) -> String {
    let p = progress.unwrap_or("(dosya henüz yok)");
    let a = approach.unwrap_or("(dosya henüz yok)");
    let c = curriculum.unwrap_or("(dosya henüz yok)");
    let pr = profile.unwrap_or("(dosya henüz yok)");
    format!(
        "[OTURUM KAPANIYOR — DOSYA GÜNCELLEME]\n\
         Görev: aşağıdaki dosyalardan güncellenmesi gerekenleri üret. Her dosyayı şu \
         satırla başlat: `===DOSYA: <ad>===` (ad: progress | approach | curriculum | \
         profile — örn. profil üretilecekse `===DOSYA: profile===`).\n\n\
         Mevcut progress ({topic}):\n---\n{p}\n---\n\n\
         Mevcut approach:\n---\n{a}\n---\n\n\
         Mevcut curriculum:\n---\n{c}\n---\n\n\
         Mevcut profil:\n---\n{pr}\n---\n\n\
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
         - `profile` is generated only if new/changed permanent information about the \
         user was learned this session: name, background/experience, learning style, \
         preferences, recurring strengths/weaknesses. NO TOPIC KNOWLEDGE — 'learned \
         concept X' is progress's job; 'likes to learn from examples' goes in profile. \
         KEEP the valid information already in the current profile (the user may have \
         hand-edited it), ~1 page cap, merge duplicates. If nothing changed, don't \
         generate this file at all.\n\
         - Bölücü satırları dışında açıklama/selamlama yazma; her dosya saf markdown."
    )
}

/// Açılış drilli turn'ü: progress varsa oturum başında Usta ilk sözü alır ve
/// geri çağırma sorusu sorar (testing effect — USTA.md "Açılış Drilli" kuralı).
/// main.rs'e bağlandığı yer: Task 3 (açılış drilli tetiği).
pub fn opening_prompt(topic: &str, profile_generic: bool) -> String {
    let meet_block = if profile_generic { MEET_BLOCK } else { "" };
    format!(
        "[SESSION OPENING — RECALL DRILL]\n{meet_block}\
         Topic: {topic}. Pick 2-3 questions from the 'Recall questions' section of your \
         progress file and ASK me — don't answer them yourself, don't explain them. Keep \
         it short: a 2-minute warm-up, then we move to today's work. If progress has no \
         questions, come up with 2 small recall questions suited to my level. When the \
         drill is done, say one sentence from the map: where we are, what's next (your \
         curriculum file is in the system prompt)."
    )
}

/// Yeni konu tanışma turn'ü: yaklaşım + müfredat haritası henüz yok — Usta
/// açık sohbetle türetir (USTA.md "Yeni Konu Tanışması"). Sabit form değil:
/// kullanıcının söylediğinden türetilir, yön kullanıcıda kalır.
pub fn onboarding_prompt(topic: &str, intro: Option<&str>, profile_generic: bool) -> String {
    // Kullanıcının konu girişinde yazdığı ham metin tanışmanın İLK CEVABIDIR —
    // slug'a indirgenip atılırsa model zaten söylenenleri yeniden sorar
    // ("müşterime Coolify kuracağım, Fedora..." → "ne peşindesin?" faciası).
    let intro_block = match intro {
        Some(s) if !s.trim().is_empty() => format!(
            "\nWhen the user opened the topic, they wrote this — treat it as the FIRST \
             ANSWER of the introduction:\n\
             \"{}\"\n\
             USE this information: don't ask again what they already said; start by \
             picking up on what they said and only ask about what's still missing.\n",
            s.trim()
        ),
        _ => String::new(),
    };
    let meet_block = if profile_generic { MEET_BLOCK } else { "" };
    format!(
        "[NEW TOPIC — INTRODUCTION]\n\
         Topic: {topic}. This topic has no approach or curriculum map yet.\n{intro_block}{meet_block}\
         Have a short, NATURAL introduction — this is not a form: ask at most two \
         questions in a single message, continue based on the answer; don't dump a \
         numbered question list. Find out: what they want to do/learn, what they \
         already have. Whether this is exploration or goal-directed — infer it \
         YOURSELF, don't ask the user using these terms; if it's not clear from what \
         they said, ask one jargon-free question: 'are you preparing for a deadline or \
         exam, or is this just out of curiosity?'. If it's goal-directed, gather the \
         what/date/threshold/format info during the conversation — it will go into the \
         approach's `## Hedef` section; the map is built from the official framework \
         (exam syllabus / exam guide / CEFR) — research it on the web. If you don't \
         know the domain well enough, research it on the web. At session close you'll \
         be asked for the approach + FULL curriculum map CONTENT; the shell writes the \
         files, don't try to write files yourself during the session (Hard Rule 6) — \
         deepen the introduction accordingly but don't turn it into a lecture, keep it \
         short."
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
        let s = closing_prompt("rust", Some("- Seviye: orta"), None, None, None);
        assert!(s.contains("rust"));
        assert!(s.contains("- Seviye: orta"));
    }

    #[test]
    fn closing_prompt_marks_missing_file() {
        let s = closing_prompt("rust", None, None, None, None);
        assert!(s.contains("(dosya henüz yok)"));
    }

    #[test]
    fn closing_prompt_includes_pruning_rule() {
        let s = closing_prompt("rust", None, None, None, None);
        assert!(s.contains("20 madde"));
    }

    #[test]
    fn closing_prompt_requests_rich_sections() {
        let s = closing_prompt("rust", None, None, None, None);
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
        let s = closing_prompt("rust", Some("PMEVCUT"), Some("AMEVCUT"), Some("CMEVCUT"), None);
        assert!(s.contains("PMEVCUT"));
        assert!(s.contains("AMEVCUT"));
        assert!(s.contains("CMEVCUT"));
        assert!(s.contains("===DOSYA:"));
        assert!(s.contains("görülmedi/görüldü/oturdu/derinleşildi"));
    }

    #[test]
    fn closing_prompt_defines_goal_sections() {
        let s = closing_prompt("almanca", None, None, None, None);
        assert!(s.contains("## Hedef Durumu"));
        assert!(s.contains("## Hedef"));
        assert!(s.contains("tempo"));
    }

    #[test]
    fn closing_prompt_defines_profile_rules() {
        let s = closing_prompt("rust", None, None, None, Some("CURRENT PROFILE"));
        assert!(s.contains("===DOSYA: profile==="));
        assert!(s.contains("CURRENT PROFILE"));
        assert!(s.contains("NO TOPIC KNOWLEDGE"));
        assert!(s.contains("only")); // generated only if new/changed info exists
    }

    #[test]
    fn onboarding_prompt_carries_user_intro_and_forbids_reasking() {
        let s = onboarding_prompt(
            "hosting",
            Some("müşterimin hesabına coolify kuracağım, Fedora, temel güvenlik lazım"),
            false,
        );
        assert!(s.contains("coolify kuracağım"));
        assert!(s.contains("FIRST ANSWER"));
        assert!(s.contains("don't ask again"));
        // Intro yoksa blok hiç girmez.
        let bare = onboarding_prompt("hosting", None, false);
        assert!(!bare.contains("FIRST ANSWER"));
    }

    #[test]
    fn onboarding_prompt_infers_goal_without_jargon_and_limits_questions() {
        let s = onboarding_prompt("almanca", None, false);
        // Keşif/hedef terimleri kullanıcıya SORULMAZ — model kendisi çıkarır.
        assert!(!s.contains("keşif mi"));
        assert!(s.contains("infer it YOURSELF"));
        // Jargonsuz yedek soru + soru limiti.
        assert!(s.contains("a deadline or exam"));
        assert!(s.contains("at most two questions"));
        assert!(s.contains("## Hedef"));
    }

    #[test]
    fn opening_prompt_embeds_topic_and_asks_to_quiz() {
        let s = opening_prompt("rust", false);
        assert!(s.contains("rust"));
        assert!(s.contains("RECALL DRILL"));
        assert!(s.contains("ASK"));
    }

    #[test]
    fn onboarding_prompt_embeds_topic_and_open_conversation() {
        let s = onboarding_prompt("linux-guvenlik", None, false);
        assert!(s.contains("linux-guvenlik"));
        assert!(s.contains("INTRODUCTION"));
        assert!(s.contains("form"));
    }

    #[test]
    fn onboarding_prompt_does_not_tell_model_it_writes_files() {
        // Sert Kural 6: modelin dosya yazma aracı yok — kapanış içeriğini üretir,
        // dosyayı kabuk yazar. Prompt modeli yazma denemesine itmemeli.
        let s = onboarding_prompt("rust", None, false);
        assert!(!s.contains("you will write files"));
        assert!(s.contains("shell writes"));
    }

    #[test]
    fn opening_prompt_mentions_curriculum_position() {
        let s = opening_prompt("rust", false);
        assert!(s.contains("map"));
    }

    #[test]
    fn opening_prompts_include_meet_block_only_when_profile_generic() {
        let on = onboarding_prompt("rust", None, true);
        assert!(on.contains("[PROFILE EMPTY]"));
        assert!(on.contains("1-2 questions"));
        assert!(!onboarding_prompt("rust", None, false).contains("[PROFILE EMPTY]"));

        let op = opening_prompt("rust", true);
        assert!(op.contains("[PROFILE EMPTY]"));
        assert!(!opening_prompt("rust", false).contains("[PROFILE EMPTY]"));
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
