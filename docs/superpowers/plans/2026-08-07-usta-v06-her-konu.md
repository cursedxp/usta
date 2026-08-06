# Usta v0.6 — Her-Konu Öğrenimi Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Usta'yı gerçekten domain-agnostik yap: yeni konuda (Linux güvenliği, GTM, ne olursa) yaklaşımı Usta'nın kendisi türetir ve kalıcılaştırır; web araştırmalı TAM müfredat haritası çıkarılır ve her kapanışta güncellenir — "havada hiçbir şey kalmaz", derinlik seviyeye endekslenir, yön kullanıcı isterse revize edilir (canlı belge).

**Architecture:** Üç mekanizma: (1) `brain.rs` yaklaşım yüklemesi genelleşir — hardcode `software.md`+`_default.md` yerine `approaches/` altındaki TÜM dosyalar (global + proje override) + aktif konunun `curriculum` ve `progress`'i. (2) Kapanış flush'ı çok-dosyaya çıkar: tek LLM çağrısı `===DOSYA: <ad>===` bölücülü `progress` / `approach` / `curriculum` üretir, saf parser ayırır, her biri atomik yazılır. (3) Açılış turn'ü ikiye ayrılır: progress varsa drill (v0.3), yoksa `[YENİ KONU — TANIŞMA]` — açık sohbet, form değil. Davranış kuralları USTA.md + `_default.md`'de (meta-yaklaşım: pratik nedir / çıktı nedir / feedback neye bakar).

**Tech Stack:** v0.5 sonrası yığın. Yeni bağımlılık YOK.

## Global Constraints

- **ÖN KOŞUL: v0.2, v0.3, v0.4 VE v0.5 planları TAMAMEN uygulanmış ve commit'lenmiş olmalı** (`ask_usta`, `ui::notice/warn`, zengin `closing_prompt`, drill bloğu bu planın üstüne kurulur). Bitmemişse DUR ve bildir.
- Bu repo kendi git'ine sahip (`usta/` içinde çalış, headspace repo'suna commit atma).
- Tüm kod yorumları ve kullanıcıya görünen mesajlar **Türkçe**. Modül başları `//!` doc.
- Commit başlık deseni: `<scope>: kısa türkçe özet`.
- Her görev sonunda `cargo test` ve `cargo build` temiz (uyarı çıkarsa düzelt).
- Test isimleri `snake_case`, davranışı cümle gibi anlatır.
- Saf mantık test edilebilir fonksiyonda; IO/async kabukta.
- Commit mesajı sonuna ekle: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`

## Dosya Haritası

| Dosya | Sorumluluk | Değişim |
|---|---|---|
| `src/brain.rs` | approaches dizin taraması + curriculum yükleme | `load_system_prompt` genelleşir |
| `src/progress.rs` | çok-dosya kapanış: `split_files`, yeni `closing_prompt`, `approach_path`/`curriculum_path`, `onboarding_prompt` | güncellenir |
| `src/main.rs` | `flush_progress` çok-dosya, açılışta drill/tanışma dallanması | güncellenir |
| `USTA.md` | Yeni Konu Tanışması + Canlı Belgeler + Kapsam Bekçiliği kuralları | 3 bölüm eklenir |
| `approaches/_default.md` | Meta-yaklaşım şablonuna dönüşür | yeniden yazılır |
| `SPEC.md` | §4.8 + kararlar + dosya yapısı düzeltmesi | güncellenir |

**Not (migrasyon):** USTA.md ve `_default.md` değişiyor — scaffold var olan global dosyaların üstüne yazmaz. Bitiş Doğrulaması'nda `usta reset --factory` (veya iki dosyayı elle silme) adımı var.

---

### Task 1: `brain.rs` — yaklaşım yüklemesini genelle + curriculum (TDD)

**Files:**
- Modify: `src/brain.rs`

**Interfaces:**
- `load_system_prompt(global, project, topic)` imzası AYNI kalır — davranışı genişler:
  1. `approaches/` altındaki tüm `.md` dosyaları yüklenir (global ∪ proje; aynı ad → proje override kazanır; alfabetik sıra — deterministik prompt).
  2. Proje varsa `learner/curriculum/<topic>.md` de yüklenir (progress'in yanına).
- Mevcut testler geçmeye devam etmeli (software.md override testi dahil).

- [x] **Step 1: Failing testleri yaz**

`src/brain.rs` test modülüne ekle:

```rust
#[test]
fn loads_every_approach_file_not_just_hardcoded() {
    let (global, _project) = temp_pair("allapproaches");
    fs::create_dir_all(global.join("approaches")).unwrap();
    fs::write(global.join("approaches/software.md"), "YAZILIM YAKLAŞIMI").unwrap();
    fs::write(global.join("approaches/marketing.md"), "MARKETING YAKLAŞIMI").unwrap();
    fs::write(global.join("approaches/_default.md"), "META YAKLAŞIM").unwrap();

    let sys = load_system_prompt(&global, None, "gtm");
    assert!(sys.contains("YAZILIM YAKLAŞIMI"));
    assert!(sys.contains("MARKETING YAKLAŞIMI"));
    assert!(sys.contains("META YAKLAŞIM"));

    let _ = fs::remove_dir_all(global.parent().unwrap());
}

#[test]
fn project_only_approach_is_loaded_too() {
    let (global, project) = temp_pair("projonly");
    fs::write(global.join("USTA.md"), "ÇEKIRDEK").unwrap();
    let pa = project.join(".usta/approaches");
    fs::create_dir_all(&pa).unwrap();
    fs::write(pa.join("linux-guvenlik.md"), "KONUYA ÖZEL YAKLAŞIM").unwrap();

    let sys = load_system_prompt(&global, Some(&project), "linux-guvenlik");
    assert!(sys.contains("KONUYA ÖZEL YAKLAŞIM"));

    let _ = fs::remove_dir_all(global.parent().unwrap());
}

#[test]
fn curriculum_included_when_present() {
    let (global, project) = temp_pair("curriculum");
    fs::write(global.join("USTA.md"), "ÇEKIRDEK").unwrap();
    let cdir = project.join(".usta/learner/curriculum");
    fs::create_dir_all(&cdir).unwrap();
    fs::write(cdir.join("rust.md"), "HARITA: ownership görüldü").unwrap();

    let sys = load_system_prompt(&global, Some(&project), "rust");
    assert!(sys.contains("HARITA: ownership görüldü"));
    assert!(sys.contains("learner/curriculum/rust.md"));

    let _ = fs::remove_dir_all(global.parent().unwrap());
}
```

Run: `cargo test brain`
Expected: 3 yeni test FAIL, mevcutlar PASS.

- [x] **Step 2: Implemente et**

`load_system_prompt` içinde iki hardcoded `read_approach_with_override(...)` çağrısını şununla değiştir:

```rust
    read_all_approaches(project_usta.as_ref(), &global, &mut parts);
```

Dikkat: `read_all_approaches` `global` parametresini fonksiyonun `global: &Path` argümanından alır. Curriculum + progress bloğu:

```rust
    if let Some(dir) = &project_usta {
        for rel in [
            format!("learner/curriculum/{topic}.md"),
            format!("learner/progress/{topic}.md"),
        ] {
            read_section(&dir.join(&rel), &rel, &mut parts);
        }
    }
```

Yeni fonksiyon:

```rust
/// `approaches/` altındaki TÜM `.md` dosyalarını yükle — global ∪ proje,
/// aynı ad proje lehine override edilir (read_approach_with_override).
/// Alfabetik sıra: system prompt deterministik kalsın. Hangi yaklaşımın
/// uygulanacağını kod değil USTA.md "Domaine göre yaklaşım" kuralı seçer.
fn read_all_approaches(project_usta: Option<&PathBuf>, global: &Path, parts: &mut Vec<String>) {
    let mut names: Vec<String> = Vec::new();
    let mut collect = |dir: &std::path::Path| {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for entry in rd.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".md") && !names.contains(&name) {
                    names.push(name);
                }
            }
        }
    };
    collect(&global.join("approaches"));
    if let Some(p) = project_usta {
        collect(&p.join("approaches"));
    }
    names.sort();
    for name in names {
        read_approach_with_override(project_usta, global, &name, parts);
    }
}
```

- [x] **Step 3: Test + build**

Run: `cargo test && cargo build`
Expected: hepsi PASS (override testi dahil — tarama `software.md`'yi buluyor, override mantığı aynen çalışıyor).

- [x] **Step 4: Commit**

```bash
git add src/brain.rs
git commit -m "brain: tüm approaches + konu müfredatı yüklenir — hardcode bitti

marketing.md yazılınca artık gerçekten yüklenecek; konuya özel üretilen
yaklaşımlar (approaches/<konu>.md) ve curriculum haritası prompt'a girer.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Çok-dosya kapanış — `split_files` + yeni `closing_prompt` (TDD)

**Files:**
- Modify: `src/progress.rs`
- Modify: `src/main.rs` (`flush_progress` yeniden yazılır)

**Interfaces:**
- Produces:
  - `progress::approach_path(project_root, topic) -> PathBuf` → `.usta/approaches/<topic>.md`
  - `progress::curriculum_path(project_root, topic) -> PathBuf` → `.usta/learner/curriculum/<topic>.md`
  - `progress::split_files(reply: &str) -> Vec<(String, String)>` — `===DOSYA: <ad>===` bölücülerine ayırır; bölücü yoksa tüm yanıt `("progress", ...)` sayılır (geriye uyum). İçerikler `clean_markdown_reply`'den geçer.
  - `progress::closing_prompt(topic, progress: Option<&str>, approach: Option<&str>, curriculum: Option<&str>) -> String` — İMZA DEĞİŞİR; v0.3 testlerindeki çağrılar `None, None, None` ile güncellenir.
- Sözleşme: `progress` her kapanışta üretilir; `approach`/`curriculum` yalnız ilk oturumda veya değiştiğinde. Bilinmeyen dosya adı uyarıyla atlanır — asla keyfi yola yazılmaz.

- [x] **Step 1: Failing testleri yaz**

`src/progress.rs` test modülüne ekle (v0.3'ün `closing_prompt(...)` çağrılarını `closing_prompt("rust", None, None, None)` biçimine güncelle):

```rust
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
```

Run: `cargo test progress`
Expected: yeni testler FAIL.

- [x] **Step 2: Implemente et**

`src/progress.rs`'e ekle/değiştir:

```rust
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
```

`closing_prompt`'u değiştir (imza + gövde):

```rust
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
         3+ tekrar = GAP ADAYI) / `## İpucu merdiveni`.\n\
         - `approach` yalnız ilk oturumda veya yaklaşım bu oturumda değiştiyse üretilir — \
         canlı belge, _default.md'deki üç soruya cevap verir (pratik / çıktı / feedback).\n\
         - `curriculum` ilk oturumda TAM harita olarak çıkarılır (konu/alt-konu ağacı; her \
         madde `görülmedi/görüldü/oturdu/derinleşildi` durumuyla; gerekiyorsa web \
         araştırmasına dayan); sonraki oturumlarda yalnız durum değiştiyse üretilir. \
         Kapsanmamış kritik madde haritada görünür kalmalı.\n\
         - Oturumda kanıtı olmayanı ekleme; mevcut dosyalardaki geçerli bilgiyi koru \
         (kullanıcı elle düzenlemiş olabilir — düzenlemesini ez-me).\n\
         - Bölücü satırları dışında açıklama/selamlama yazma; her dosya saf markdown."
    )
}
```

- [x] **Step 3: `flush_progress`'i çok-dosyaya geçir**

`src/main.rs` — gövdeyi değiştir:

```rust
/// Oturum kapanışında progress/approach/curriculum dosyalarını LLM'e üretir.
/// Boş oturumda dokunmaz; bilinmeyen dosya adı uyarıyla atlanır (keyfi yola
/// asla yazılmaz).
async fn flush_progress(
    backend: &mut Backend,
    session: &Session,
    project_root: &Path,
) -> Result<()> {
    if session.history().is_empty() {
        return Ok(());
    }
    ui::notice("oturum özetleniyor — dosyalar yazılıyor…");
    let p_path = progress::progress_path(project_root, &session.topic);
    let a_path = progress::approach_path(project_root, &session.topic);
    let c_path = progress::curriculum_path(project_root, &session.topic);
    let read = |p: &Path| std::fs::read_to_string(p).ok();
    let mut history = session.history().to_vec();
    history.push(Message::user(&progress::closing_prompt(
        &session.topic,
        read(&p_path).as_deref(),
        read(&a_path).as_deref(),
        read(&c_path).as_deref(),
    )));
    let (reply, _) = ask_usta(backend, &session.system, &history).await?;
    let files = progress::split_files(&reply);
    if files.is_empty() {
        anyhow::bail!("model dosya üretmedi — hiçbir şey yazılmadı");
    }
    for (name, content) in files {
        let path = match name.as_str() {
            "progress" => p_path.clone(),
            "approach" => a_path.clone(),
            "curriculum" => c_path.clone(),
            other => {
                ui::warn(&format!("bilinmeyen kapanış dosyası atlandı: {other}"));
                continue;
            }
        };
        if content.is_empty() {
            ui::warn(&format!("boş içerik atlandı: {name}"));
            continue;
        }
        progress::write_atomic(&path, &content)?;
        ui::notice(&format!("güncellendi: {}", path.display()));
    }
```

(fonksiyonun sonundaki v0.4 katalog `index::record` bloğu OLDUĞU GİBİ kalır.)

- [x] **Step 4: Test + build**

Run: `cargo test && cargo build`
Expected: yeni 5 test dahil hepsi PASS (v0.3 çağrı güncellemeleri dahil).

- [x] **Step 5: Commit**

```bash
git add src/progress.rs src/main.rs
git commit -m "progress: çok-dosya kapanış — progress + approach + curriculum

Tek çağrı, ===DOSYA:=== bölücülü; bölücüsüz yanıt geriye-uyumlu progress
sayılır. Yaklaşım ve müfredat haritası artık kalıcılaşıyor.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Açılış dallanması — drill / yeni-konu tanışması (TDD)

**Files:**
- Modify: `src/progress.rs` (`onboarding_prompt` + drill promptuna harita satırı)
- Modify: `src/main.rs` (açılış bloğu else dalı)

**Interfaces:**
- Produces: `progress::onboarding_prompt(topic: &str) -> String`
- Davranış: progress VARSA drill (v0.3 aynen); YOKSA tanışma turn'ü — Usta ilk sözü alır, açık sohbetle konuyu tanır. Her iki durumda hata oturumu engellemez.

- [x] **Step 1: Failing testleri yaz**

`src/progress.rs` test modülüne:

```rust
#[test]
fn onboarding_prompt_embeds_topic_and_open_conversation() {
    let s = onboarding_prompt("linux-guvenlik");
    assert!(s.contains("linux-guvenlik"));
    assert!(s.contains("TANIŞMA"));
    assert!(s.contains("form"));
}

#[test]
fn opening_prompt_mentions_curriculum_position() {
    let s = opening_prompt("rust");
    assert!(s.contains("harita"));
}
```

Run: `cargo test prompt`
Expected: FAIL.

- [x] **Step 2: Implemente et**

`src/progress.rs`:

```rust
/// Yeni konu tanışma turn'ü: yaklaşım + müfredat haritası henüz yok — Usta
/// açık sohbetle türetir (USTA.md "Yeni Konu Tanışması"). Sabit form değil:
/// kullanıcının söylediğinden türetilir, yön kullanıcıda kalır.
pub fn onboarding_prompt(topic: &str) -> String {
    format!(
        "[YENİ KONU — TANIŞMA]\n\
         Konu: {topic}. Bu konunun yaklaşımı ve müfredat haritası henüz yok.\n\
         Kısa bir tanışma başlat: ne öğrenmek istiyorum, neden, hedefim ne, elimde ne \
         var? Bu bir form değil — AÇIK sohbet; ben ne söylersem oradan türet, başka bir \
         şey istiyorsam onu takip et. Alanı yeterince bilmiyorsan web'de araştır. \
         Oturum kapanışında yaklaşımı ve TAM müfredat haritasını dosyalara yazacaksın — \
         tanışmayı buna göre derinleştir ama derse çevirme, kısa tut."
    )
}
```

`opening_prompt`'un (v0.3) format string'inin SONUNA şu cümleyi ekle:

```
 Drill bitince haritadan tek cümle söyle: neredeyiz, sırada ne var (curriculum dosyan system prompt'ta).
```

- [x] **Step 3: Açılış bloğuna else dalı ekle**

`src/main.rs` — v0.3'ün `if has_progress { ... }` bloğuna else ekle:

```rust
    } else {
        // Yeni konu: yaklaşım/harita yok — tanışma turn'ü, Usta ilk sözü alır.
        session.push_user(&progress::onboarding_prompt(&topic));
        match ask_usta(&mut backend, &session.system, session.history()).await {
            Ok((reply, web)) => {
                print_reply(&reply, web);
                session.push_assistant(reply);
            }
            Err(e) => ui::warn(&format!("tanışma turu atlandı: {e}")),
        }
    }
```

- [x] **Step 4: Test + build**

Run: `cargo test && cargo build`
Expected: hepsi PASS.

- [x] **Step 5: Commit**

```bash
git add src/progress.rs src/main.rs
git commit -m "main: açılış dallanması — progress varsa drill, yoksa tanışma

Yeni konuda Usta ilk sözü alıp açık sohbetle yaklaşım/harita malzemesini
toplar; drill artık haritadan konum da söylüyor.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: USTA.md kuralları + `_default.md` meta-yaklaşım

**Files:**
- Modify: `USTA.md`
- Modify: `approaches/_default.md`

**Interfaces:** markdown — Task 2/3'ün prompt'larındaki kavramlar (`_default.md'deki üç soru`, `Kapsam Bekçiliği`, `canlı belge`) burada TANIMLANIR.

- [ ] **Step 1: USTA.md'ye üç bölüm ekle**

"Tahmin Protokolü" bölümünden sonra:

```markdown
## Yeni Konu Tanışması

`[YENİ KONU — TANIŞMA]` turn'ü geldiğinde:

- Açık sohbetle tanış: ne öğrenmek istiyor, neden, hedef, eldekiler. Sabit form YOK — kullanıcı ne söylerse oradan türet; senin sorularının dışında bir şey istiyorsa onu takip et. Yön her zaman kullanıcıda.
- Domain'in doğasını `_default.md`'deki üç soruyla belirle: pratik nedir / çıktı nedir / feedback neye bakar.
- Alanı yeterince bilmiyorsan web'de araştır — güvenilir eğitmenin haritası tahmine dayanmaz (Sert Kural 2).
- Kapanışta yaklaşım (`approach`) + TAM müfredat haritası (`curriculum`) üreteceksin.

## Canlı Belgeler

- Yaklaşım ve müfredat DOGMA DEĞİL. Kullanıcı yön değiştirmek isterse, yaklaşım uymuyorsa, "ben aslında X istiyorum" derse → oturum içinde konuş, kapanışta dosyayı revize et.
- Kullanıcı dosyaları elle düzenleyebilir — sonraki oturumda düzenlenmiş hali geçerlidir; sadece oturum kanıtıyla güncelle, üzerine yazma.

## Kapsam Bekçiliği — havada hiçbir şey kalmaz

- Müfredat haritası (`curriculum/<konu>.md`) kapsam sözleşmendir: her madde `görülmedi / görüldü / oturdu / derinleşildi`.
- Kapanışta durumları güncelle. Kritik bir madde uzun süre `görülmedi` kalıyorsa görünür kıl: "haritada X hâlâ açık" (yargısız — sadece görünürlük).
- Açılış drilli sorularını haritanın "oturdu ama eskidi" bölgesinden seç — rastgele değil, sistematik tekrar.
- **Sığlaşma yasak:** `oturdu` işaretlenen konu bitmez — daha zor varyantla geri gelir. Seviye arttıkça soruların haritanın derin katmanından (uç vakalar, tasarım kararları, "neden böyle") gelir. Zorluk hep mevcut seviyenin bir tık üstünde — öğrenme hazzı o dengeden gelir.
```

- [ ] **Step 2: `approaches/_default.md`'yi yeniden yaz**

Dosyanın tüm içeriğini şununla değiştir:

```markdown
# Varsayılan Yaklaşım — domain'i sen türet

Her konunun hazır yaklaşım dosyası yoktur ve olması da gerekmez: İLK oturumda şu üç soruyu cevaplayarak konuya özel yaklaşımı SEN üretirsin (kapanışta `approach` dosyası olarak kalıcılaşır):

1. **Pratik nedir?** Bu domain'de "yaparak öğrenme" neye benzer? — kod projesi inşa etmek · senaryo/lab'da komut denemek · öğrendiğini gerçek işine uygulamak · vaka üzerinden tartışmak
2. **Çıktı/artifact nedir?** Öğrenci ne üretir? — kod dosyası · yazılı plan/brief (`.md`) · hiçbiri (saf sohbet de geçerli)
3. **Feedback neye bakar?** — kod kalitesi · mantık/varsayım delikleri · karar gerekçesi · uygulama doğruluğu

Kurallar:

- **Örnekler sınırlayıcı DEĞİL** — üç soru açık uçludur. Domain hiçbir örneğe uymuyorsa (fiziksel beceri, sanat, dil, ne olursa) o domain'e uygun YENİ cevabı türet. Kullanıcının söylediği her zaman şablondan önce gelir.
- Spek her domain'de gerekmez — "spek gereksiz, direkt yap" da geçerli bir cevaptır.
- Artifact'lı domain'lerde kullanıcıyı düşüncesini DOSYAYA yazmaya teşvik et: dosya kaydı = proaktif feedback kancası. Marketing planı da `.md`'ye yazılır, kod gibi izlenir.
- Ürettiğin yaklaşım canlı belgedir — bkz. USTA.md "Canlı Belgeler".
```

- [ ] **Step 3: Commit**

```bash
git add USTA.md approaches/_default.md
git commit -m "USTA: her-konu kuralları — tanışma, canlı belgeler, kapsam bekçiliği

_default.md meta-yaklaşıma dönüştü: domain'i Usta üç soruyla türetir.
Sığlaşma yasağı: oturan konu daha zor varyantla döner, derinlik seviyeye
endeksli.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: SPEC v0.6 güncellemesi

**Files:**
- Modify: `SPEC.md`

- [ ] **Step 1: §4.7'den sonra yeni bölüm ekle**

```markdown
## 4.8 Her-Konu Öğrenimi (v0.6)

Domain listesi elle genişletilmez — sistem kendi kendini genişletir:

- **Yaklaşım üretimi:** yaklaşımı olmayan konuda ilk oturum `[YENİ KONU — TANIŞMA]` ile açılır (açık sohbet, form değil; yön kullanıcıda). Usta domain doğasını `_default.md`'nin üç sorusuyla türetir (pratik / çıktı / feedback), kapanışta `.usta/approaches/<konu>.md` yazılır. **Canlı belge:** oturum içinde revize edilir, elle düzenlenebilir.
- **Müfredat haritası:** ilk oturumda web araştırmalı TAM harita `.usta/learner/curriculum/<konu>.md`'ye çıkarılır; her madde `görülmedi/görüldü/oturdu/derinleşildi`. Kapanışta güncellenir. Kapsam bekçiliği ("haritada X hâlâ açık"), drill beslemesi (oturdu-ama-eskidi bölgesi) ve derinlik ayarı (sığlaşma yasağı) buradan çalışır.
- **Brain yüklemesi genel:** `approaches/` altındaki TÜM dosyalar (global ∪ proje, override proje lehine, alfabetik) + aktif konunun curriculum + progress'i system prompt'a girer. Yaklaşım seçimini kod değil USTA.md kuralı yapar.
- **Kapanış çok-dosya:** tek çağrı `===DOSYA: <ad>===` bölücülü `progress`(her zaman) / `approach` / `curriculum`(değiştiğinde) üretir; bölücüsüz yanıt geriye-uyumlu progress sayılır; bilinmeyen ad uyarıyla atlanır.
```

- [ ] **Step 2: "Alınan Kararlar" bölümüne ekle**

```markdown
- **Her-konu (v0.6):** yaklaşım dosyaları elle değil ilk-oturum tanışmasıyla üretilir; curriculum proje-lokal (`.usta/learner/curriculum/`) yaşar — §7'deki global `learner/curriculum/` yerine (izolasyon: harita da konu+proje bağlamına ait). Kapanış bölücü formatı `===DOSYA: <ad>===`.
```

- [ ] **Step 3: §7 dosya yapısında curriculum satırını güncelle**

`curriculum/` satırının açıklamasını şu hale getir: `# gap'lere göre planlanan dersler + müfredat haritası — v0.6'dan itibaren proje-lokal .usta/learner/curriculum/<konu>.md`

- [ ] **Step 4: Commit + push**

```bash
git add SPEC.md
git commit -m "SPEC: v0.6 her-konu öğrenimi — yaklaşım üretimi, müfredat haritası

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Bitiş Doğrulaması (tüm görevler sonrası)

- [ ] `cargo test` — tamamı PASS
- [ ] `cargo build` — uyarısız
- [ ] **Migrasyon:** global USTA.md + `_default.md` eski — sandbox'ta değil GERÇEK kurulumda güncellenmesi için kullanıcıya not bırak (rapora yaz): `rm ~/.config/usta/USTA.md ~/.config/usta/approaches/_default.md` + bir kez `usta` (veya `usta reset --factory`).
- [ ] Sandbox duman (backend varsa, `XDG_CONFIG_HOME` set):
  1. `usta start gtm` (hiç progress yok) → tanışma turn'ü gelsin, drill DEĞİL.
  2. Birkaç turn konuş → `/quit` → `.usta/learner/progress/gtm.md` + `.usta/approaches/gtm.md` + `.usta/learner/curriculum/gtm.md` üçü de oluşsun; curriculum'da durum etiketleri görünsün.
  3. Yeniden `usta start gtm` → drill + "haritadan neredeyiz" cümlesi gelsin; system prompt'a curriculum yüklendiğini davranıştan doğrula (harita maddelerine atıf).
  4. Rust konusu regresyon: mevcut konuda davranış bozulmamış (drill çalışıyor, progress güncelleniyor).
