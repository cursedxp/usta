# Usta v0.3 — Pedagoji Katmanı Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Usta'yı "feedback veren bot"tan "seni sınayan usta"ya çevir: oturum-açılış geri çağırma drilli (testing effect), anlat-modu (Feynman), ipucu merdiveni (fading) ve `cargo check` tahmin protokolü (hypercorrection) ekle.

**Architecture:** Pedagoji kuralları markdown'da yaşar (USTA.md — "ince kabuk, kalın beyin"). Rust tarafı üç küçük parça ekler: (1) oturum açılışında progress varsa sentetik açılış turn'ü (drill'i Usta başlatır), (2) `progress.rs` kapanış promptu zengin formata geçer (geri çağırma soruları + hata günlüğü + merdiven notu), (3) yeni `check.rs` kayıt sonrası `cargo check` koşturur ve sonucu LLM'e "sadece senin gözün için" bloğuyla verir — saklama/tahmin-ettirme kararı USTA.md kuralındadır.

**Tech Stack:** v0.2 sonrası mevcut yığın (Rust 2021, tokio, notify, rustyline, similar). Yeni bağımlılık YOK.

## Global Constraints

- **ÖN KOŞUL: v0.2 planı (`2026-08-07-usta-v02-hafiza-proaktiflik.md`) TAMAMEN uygulanmış ve commit'lenmiş olmalı.** Bu plan v0.2'nin `progress.rs`, select-loop, `FileMemory` ve `&mut Backend` imzalarının üstüne kurulur. v0.2 bitmemişse DUR ve bildir.
- Bu repo kendi git'ine sahip (`usta/` içinde çalış, headspace repo'suna commit atma).
- Tüm kod yorumları, kullanıcıya görünen mesajlar ve doc-comment'ler **Türkçe** (mevcut stil). Modül başları `//!` doc taşır.
- Commit başlık deseni: `<scope>: kısa türkçe özet`.
- Her görev sonunda `cargo test` ve `cargo build` temiz olmalı (uyarı çıkarsa düzelt).
- Test isimleri mevcut desende: `snake_case`, davranışı cümle gibi anlatır.
- Saf mantık test edilebilir fonksiyonda; IO/async kabukta.
- Commit mesajı sonuna ekle: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`

## Dosya Haritası

| Dosya | Sorumluluk | Değişim |
|---|---|---|
| `USTA.md` | Pedagoji kuralları: drill, anlat-modu, merdiven, tahmin protokolü | 4 yeni bölüm eklenir |
| `src/progress.rs` | `closing_prompt` zengin format + yeni `opening_prompt` | güncellenir |
| `src/main.rs` | Açılış drilli tetiği + `handle_file_change`'e check bloğu | güncellenir |
| `src/check.rs` | **YENİ** — `cargo check` koşucusu (timeout + kırpma) | oluşturulur |
| `SPEC.md` | §4.6 Pedagoji Katmanı + v0.3 kararları | güncellenir |

**Not (migrasyon):** `defaults.rs` USTA.md'yi derleme zamanında gömer ama scaffold var olan `~/.config/usta/USTA.md`'nin ÜSTÜNE YAZMAZ (`should_write`). Yeni kurallar çalışan kurulumda görünsün diye Bitiş Doğrulaması'nda manuel silme adımı var — koda migration mekanizması EKLEME (YAGNI, v0.4 konusu).

---

### Task 1: USTA.md pedagoji kuralları

**Files:**
- Modify: `USTA.md`

**Interfaces:**
- Produces: `[cargo check sonucu — SADECE SENİN GÖZÜN İÇİN, ...]` blok etiketi burada TANIMLANIR — Task 5'in enjekte ettiği etiketle birebir aynı olmalı.
- Consumes: yok (markdown).

- [x] **Step 1: USTA.md'ye dört bölüm ekle**

`## Meta-beceri (asıl öğretilen)` bölümünden ÖNCE, `## Çalışma Kadansı — parça-başı mini-spek` bölümünden sonra şunları ekle:

```markdown
## Açılış Drilli — geri çağırma (retrieval)

Her oturum açılışında sana `[OTURUM AÇILIŞI — GERİ ÇAĞIRMA DRİLLİ]` turn'ü gelir (progress varsa shell tetikler). Kural:

- Progress'teki "Geri çağırma soruları"ndan 2-3'ünü SOR. Anlatma, sor — hatırlama çabasının kendisi öğrenmedir (testing effect).
- Kısa tut: 2 dakikalık ısınma, sonra günün işine geç. Drill'i uzatma, derse çevirme.
- Yanlış/eksik cevapta düzelt-geç. Ama **kendinden emin yanlışta dur** — en değerli öğrenme anı orası (hypercorrection): doğrusunu söyleme, buldurt.
- ADHD notu: drill "suya girme" rampasıdır — gün küçük kazanılmış zaferle açılır. Yargı yok, skor tutma yok.

## Anlat-Modu (Feynman) — parça kapanışı

Parça bitti = roller döner: "Şimdi bana anlat — ben junior'ım. Bu fonksiyon neden böyle?"

- Kullanıcı KENDİ yazdığını açıklar. Açıklamadaki boşluk, el sallama, ezber tekrarı = gerçek gap sinyali — koddan daha iyi.
- Geçiştirilen yeri nazikçe yakala: "Şurayı hızlı geçtin — neden `&str`, neden `String` değil?"
- Yakalanan gap'i oturum kapanışında progress'in Gap'ler bölümüne kanıtıyla işle.

## İpucu Merdiveni (fading)

Kullanıcı takıldığında yardımı merdivenle ver, basamak atlama:

1. **Soru** — "Bu değişkenin sahibi kim şu satırda?"
2. **Kavram adı** — "Buna move semantics deniyor — hatırlıyor musun?"
3. **Pseudocode / minik illüstrasyon** — projeye kopyalanamaz.
4. Merdivenin sonu 3'tür. Hiçbir basamakta kullanıcının projesine kod yazılmaz (Sert Kural 1).

- Seviye yükseldikçe merdiveni KISALT (fading): ileri seviyede 1. basamakta daha uzun bekle, kolay inme.
- ADHD dengesi: bir basamakta ~iki tur takılı kalındıysa bir basamak in — frustrasyon-quit eşiği düşük, yardımı esirgemek de hata.
- Hangi konuda hangi basamağa inildiğini kapanışta progress'in "İpucu merdiveni" bölümüne not et.

## Tahmin Protokolü — derleme sonuçları

Dosya feedback turn'ünde sana `[cargo check sonucu — SADECE SENİN GÖZÜN İÇİN, kullanıcıya doğrudan aktarma; tahmin protokolünü uygula]` bloğu gelebilir. Kural:

- **Hata varsa:** sonucu SÖYLEME. Önce tahmin ettir: "Bence bu kayıt temiz derlenmedi — nerede, ne hatası olabilir?" Tahmin geldikten SONRA gerçek çıktıyı aç ve tartış. Kendinden emin yanlış tahmin = altın an, orada derinleş.
- **Temizse ("TEMİZ" yazıyorsa):** normal feedback ver. Arada bir (her kayıtta değil) kalibrasyon sorusu sor: "Derleneceğinden emin miydin? Nereden?"
- Tekrarlayan hata tipini kapanışta progress'in "Hata günlüğü"ne işle — 3+ tekrar `GAP ADAYI`dır: hedefli mini-alıştırma öner (planla, yaptırma).
- Blok hiç gelmemişse (Rust dışı proje / check koşamadı) protokol atlanır — normal feedback.
```

- [x] **Step 2: Gözden geçir + commit**

Dosyanın kalanıyla ton/terim tutarlılığını kontrol et (Türkçe, "Sert Kural" referansları doğru).

```bash
git add USTA.md
git commit -m "USTA: pedagoji katmanı — drill, anlat-modu, ipucu merdiveni, tahmin protokolü

Öğretim yönü değil geri çağırma yönü: testing effect, Feynman,
fading, hypercorrection. Kurallar markdown'da — kabuk tetikleri sonraki
görevlerde.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: `progress.rs` — zengin kapanış formatı + `opening_prompt` (TDD)

**Files:**
- Modify: `src/progress.rs`

**Interfaces:**
- Consumes: v0.2'nin `closing_prompt(topic, existing)` fonksiyonu (gövdesi değişir, imza aynı).
- Produces: `progress::opening_prompt(topic: &str) -> String` — Task 3 açılış drilli bunu sentetik user turn olarak enjekte eder.

- [x] **Step 1: Failing testleri yaz**

`src/progress.rs` test modülüne ekle:

```rust
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
```

- [x] **Step 2: Fail'i gör**

Run: `cargo test progress`
Expected: FAIL — `closing_prompt` yeni bölümleri içermiyor, `opening_prompt` tanımsız.

- [x] **Step 3: Implemente et**

`closing_prompt`'un gövdesini şununla değiştir (imza aynı):

```rust
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
```

Yeni fonksiyonu ekle:

```rust
/// Açılış drilli turn'ü: progress varsa oturum başında Usta ilk sözü alır ve
/// geri çağırma sorusu sorar (testing effect — USTA.md "Açılış Drilli" kuralı).
pub fn opening_prompt(topic: &str) -> String {
    format!(
        "[OTURUM AÇILIŞI — GERİ ÇAĞIRMA DRİLLİ]\n\
         Konu: {topic}. Progress dosyandaki 'Geri çağırma soruları'ndan 2-3 tanesini seç \
         ve bana SOR — cevaplarını verme, anlatma. Kısa tut: 2 dakikalık ısınma, sonra \
         günün işine geçeriz. Progress'te soru yoksa seviyeme uygun 2 küçük hatırlama \
         sorusu üret."
    )
}
```

- [x] **Step 4: Test + build**

Run: `cargo test && cargo build`
Expected: yeni 2 test dahil hepsi PASS (v0.2'nin `closing_prompt_embeds_topic_and_existing` ve `closing_prompt_marks_missing_file` testleri de geçmeye devam etmeli — format değişikliği topic/existing gömmeyi bozmuyor).

- [x] **Step 5: Commit**

```bash
git add src/progress.rs
git commit -m "progress: zengin format — geri çağırma soruları, hata günlüğü, merdiven notu

Kapanış promptu pedagoji katmanının veri modelini üretir; opening_prompt
açılış drillinin turn'ü.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Açılış drilli tetiği (`main.rs`)

**Files:**
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: `progress::opening_prompt` (Task 2), `progress::progress_path` (v0.2), select-loop + `&mut backend` (v0.2).
- Davranış sözleşmesi: `.usta/learner/progress/<konu>.md` var ve boş değilse, ilk `sen> ` promptundan ÖNCE Usta drill sorusunu basar. Dosya yoksa (ilk oturum) hiçbir şey değişmez. Drill çağrısı hata verirse oturum normal devam eder.

- [x] **Step 1: Drill bloğunu ekle**

`src/main.rs` — `println!("Usta hazır — ...")` satırı ile `let _ = ready_tx.send(());` satırı ARASINA:

```rust
    // Açılış drilli: önceki oturumlardan progress varsa Usta ilk sözü alır,
    // 2-3 geri çağırma sorusuyla ısındırır (testing effect — USTA.md kuralı).
    let has_progress = std::fs::read_to_string(progress::progress_path(&project_root, &topic))
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    if has_progress {
        session.push_user(&progress::opening_prompt(&topic));
        match backend.complete(&session.system, session.history()).await {
            Ok((reply, web)) => {
                print_reply(&reply, web);
                session.push_assistant(reply);
            }
            // Drill başarısız → oturumu engelleme, sessizce normal akışa düş.
            Err(e) => eprintln!("(açılış drilli atlandı: {e})"),
        }
    }
```

- [x] **Step 2: Test + build**

Run: `cargo test && cargo build`
Expected: hepsi PASS, build temiz.

- [x] **Step 3: Manuel duman testi (backend varsa)**

Temp dizinde `.usta/learner/progress/deneme.md` dosyasına elle şu içeriği yaz:

```markdown
# deneme — İlerleme
## Geri çağırma soruları
- Rust'ta `let` ile `let mut` farkı nedir? — mut yeniden atamaya izin verir.
```

`cargo run -- start deneme` başlat. Beklenen: `sen> ` promptundan önce Usta soru sorar. Dosyayı silip tekrar başlat → drill yok, direkt prompt. (Backend yoksa derleme+test yeterli.)

- [x] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "main: oturum açılış drilli — progress varsa Usta ilk sözü alır

Geri çağırma draili ADHD için düşük eşikli açılış rampası; ilk oturumda
(progress yok) davranış değişmez.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: `check.rs` — cargo check koşucusu (TDD)

**Files:**
- Create: `src/check.rs`
- Modify: `src/main.rs` (sadece `mod check;` satırı)

**Interfaces:**
- Produces:
  - `check::MAX_CHECK_BYTES: usize` (= 4096)
  - `check::is_cargo_project(root: &Path) -> bool`
  - `check::truncate_output(s: &str, max: usize) -> String`
  - `check::run_check(root: &Path) -> Option<String>` (async) — Cargo projesi değilse / cargo koşamazsa / 60 sn'de bitmezse `None`; temizse `"TEMİZ — cargo check hatasız geçti."`, hatalıysa kırpılmış stderr.
- Consumes: yok.

- [x] **Step 1: Failing testleri yaz**

`src/check.rs` (önce testler):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn is_cargo_project_true_when_manifest_exists() {
        let base = std::env::temp_dir().join(format!(
            "usta_check_test_manifest_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("Cargo.toml"), "[package]").unwrap();
        assert!(is_cargo_project(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn is_cargo_project_false_without_manifest() {
        let base = std::env::temp_dir().join(format!(
            "usta_check_test_nomanifest_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        assert!(!is_cargo_project(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn truncate_passes_short_output_through() {
        assert_eq!(truncate_output("kısa", 100), "kısa");
    }

    #[test]
    fn truncate_cuts_long_output_with_note() {
        let long = "a".repeat(200);
        let out = truncate_output(&long, 100);
        assert!(out.len() < 200);
        assert!(out.contains("kırpıldı"));
    }

    #[test]
    fn truncate_respects_utf8_char_boundary() {
        // "ö" 2 bayt — tavan bir char'ın ortasına denk gelirse panik atmamalı.
        let s = "ööööö";
        let out = truncate_output(s, 3);
        assert!(out.contains("kırpıldı"));
    }
}
```

- [x] **Step 2: Fail'i gör**

`src/main.rs`'e `mod check;` ekle. Run: `cargo test check`
Expected: FAIL (fonksiyonlar tanımsız).

- [x] **Step 3: Implemente et**

`src/check.rs` başına:

```rust
//! Kayıt sonrası `cargo check` — tahmin protokolünün hammaddesi. Sonuç LLM'e
//! "sadece senin gözün için" bloğu olarak gider; kullanıcıya ne zaman
//! açılacağına (önce tahmin ettirerek) USTA.md kuralları karar verir.
//! Cargo projesi değilse / check koşamazsa sessizce yok sayılır — feedback
//! akışı asla engellenmez.

use std::path::Path;
use std::time::Duration;

use tokio::process::Command;

/// Çıktı tavanı — devasa hata listeleri context'i şişirmesin.
pub const MAX_CHECK_BYTES: usize = 4 * 1024;

/// Check zaman tavanı — soğuk cache'te ilk check uzun sürebilir.
const CHECK_TIMEOUT: Duration = Duration::from_secs(60);

/// Proje kökünde Cargo.toml var mı?
pub fn is_cargo_project(root: &Path) -> bool {
    root.join("Cargo.toml").is_file()
}

/// Çıktıyı tavana kırp — UTF-8 char sınırına saygıyla; kırpıldıysa not düş.
pub fn truncate_output(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut cut = max;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n… (kırpıldı — toplam {} bayt)", &s[..cut], s.len())
}

/// `cargo check --message-format=short` koştur. Cargo projesi değilse,
/// cargo çalıştırılamazsa veya timeout'a takılırsa `None` — tahmin protokolü
/// o kayıtta atlanır, feedback normal akar.
pub async fn run_check(root: &Path) -> Option<String> {
    if !is_cargo_project(root) {
        return None;
    }
    let fut = Command::new("cargo")
        .arg("check")
        .arg("--message-format=short")
        .current_dir(root)
        .output();
    let output = tokio::time::timeout(CHECK_TIMEOUT, fut).await.ok()?.ok()?;
    if output.status.success() {
        return Some("TEMİZ — cargo check hatasız geçti.".to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Some(truncate_output(stderr.trim(), MAX_CHECK_BYTES))
}
```

- [x] **Step 4: Test + build**

Run: `cargo test && cargo build`
Expected: 5 yeni test dahil hepsi PASS. (`run_check` unit-test edilmez — IO kabuğu; Task 5 duman testiyle doğrulanır.)

- [x] **Step 5: Commit**

```bash
git add src/check.rs src/main.rs
git commit -m "check: cargo check koşucusu — timeout + UTF-8 güvenli kırpma

Tahmin protokolünün hammaddesi; Rust dışı projede sessiz atlanır.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: Check sonucunu feedback turn'üne enjekte et

**Files:**
- Modify: `src/main.rs` (`handle_file_change` imzası + gövdesi + çağrı yeri)

**Interfaces:**
- Consumes: `check::run_check` (Task 4), USTA.md blok etiketi (Task 1 — birebir aynı metin).
- Produces: `handle_file_change(&mut Backend, &mut Session, &mut FileMemory, &Path /*project_root*/, &Path /*path*/)` — yeni imza.

- [x] **Step 1: `handle_file_change`'i güncelle**

İmza `project_root` alır; `injected` kurulduktan sonra check bloğu eklenir:

```rust
/// Kaydedilen dosyayı FileMemory'den geçir; ilk görüşte tam içerik, sonrasında
/// diff olarak sentetik user turn'e çevir → Socratic feedback. Cargo projesiyse
/// check sonucu "sadece Usta'nın gözü için" bloğuyla eklenir (tahmin protokolü).
async fn handle_file_change(
    backend: &mut Backend,
    session: &mut Session,
    files: &mut feedback::FileMemory,
    project_root: &Path,
    path: &Path,
) -> Result<()> {
    let contents = std::fs::read_to_string(path)?;
    let mut injected = match files.observe(path, contents) {
        feedback::ChangePayload::Skip => return Ok(()),
        feedback::ChangePayload::TooLarge(len) => {
            println!("(büyük dosya izleme dışı: {} — {len} bayt)", path.display());
            return Ok(());
        }
        feedback::ChangePayload::FirstSight(full) => format!(
            "[Dosya kaydedildi: {}]\n{full}\n\nBu değişikliğe proje-temelli, Socratic geri bildirim ver.",
            path.display()
        ),
        feedback::ChangePayload::Diff(diff) => format!(
            "[Dosya değişti: {}]\nDeğişiklik (unified diff):\n{diff}\n\nBu değişikliğe proje-temelli, Socratic geri bildirim ver — değişen kısma odaklan.",
            path.display()
        ),
    };
    if let Some(check_result) = check::run_check(project_root).await {
        injected.push_str(&format!(
            "\n\n[cargo check sonucu — SADECE SENİN GÖZÜN İÇİN, kullanıcıya doğrudan aktarma; tahmin protokolünü uygula]\n{check_result}"
        ));
    }
    session.push_user(&injected);
    let (reply, web) = backend.complete(&session.system, session.history()).await?;
    print_reply(&reply, web);
    session.push_assistant(reply);
    Ok(())
}
```

Select-loop'taki çağrıyı güncelle:

```rust
if let Err(e) = handle_file_change(&mut backend, &mut session, &mut files, &project_root, &path).await {
```

- [x] **Step 2: Test + build**

Run: `cargo test && cargo build`
Expected: hepsi PASS, build temiz.

- [x] **Step 3: Manuel duman testi (backend varsa)**

Temp'te mini cargo projesi kur (`cargo init deneme-proje`), içinde `usta start rust` aç, `src/main.rs`'e bilerek tip hatası yaz-kaydet. Beklenen: Usta hata mesajını YAPIŞTIRMAZ — "nerede patlar?" diye tahmin sorar. Hatasız kayıtta normal feedback. (Backend yoksa derleme+test yeterli.)

- [x] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feedback: cargo check sonucu tahmin protokolüyle enjekte

Hata kullanıcıya dökülmez — Usta önce tahmin ettirir (hypercorrection).
Blok etiketi USTA.md kuralıyla birebir.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: SPEC v0.3 güncellemesi

**Files:**
- Modify: `SPEC.md`

**Interfaces:** yok (yalnız doküman).

- [x] **Step 1: §4.5'ten sonra yeni bölüm ekle**

`## 4.5 Başlatma / Kullanım` bölümünün SONUNA (bir sonraki `## 5` başlığından önce):

```markdown
## 4.6 Pedagoji Katmanı (v0.3)

Öğretim yönü değil, **geri çağırma yönü** optimize edilir — kalıcı öğrenme kullanıcıdan çıkan üretimde olur (testing/generation effect):

1. **Açılış drilli:** oturum başında progress'teki "Geri çağırma soruları"ndan 2-3'ü sorulur (progress varsa shell tetikler, Usta ilk sözü alır). 2 dk ısınma — ADHD için düşük eşikli "suya girme" rampası.
2. **Anlat-modu (Feynman):** parça kapanışında roller döner — kullanıcı yazdığını açıklar; açıklamadaki boşluk gap sinyalidir (koddan iyi).
3. **İpucu merdiveni:** soru → kavram adı → pseudocode; kod asla (Sert Kural 1). Seviye yükseldikçe merdiven kısalır (fading); bir basamakta ~2 tur takılınca bir basamak inilir (ADHD dengesi).
4. **Tahmin protokolü:** kayıtta `cargo check` koşar (60 sn timeout, 4KB kırpma, Rust dışı projede sessiz atlanır); hata varsa Usta sonucu söylemez, önce tahmin ettirir (hypercorrection).
5. **Hata günlüğü:** progress'te `hata tipi | sayaç | son örnek`; 3+ tekrar = `GAP ADAYI` → curriculum'a mini-alıştırma önerisi.

Kuralların tamamı USTA.md'de yaşar; Rust sadece tetikler (açılış turn'ü, check koşucusu, progress formatı).
```

- [x] **Step 2: "Alınan Kararlar" bölümüne ekle**

v0.2'nin eklediği `## 11. Alınan Kararlar (v0.2)` bölümünü `## 11. Alınan Kararlar` olarak yeniden adlandır, mevcut maddeleri `**(v0.2)**` altında bırak ve sonuna ekle:

```markdown
- **Pedagoji tetikleri (v0.3):** açılış drilli shell'den tetiklenir (progress boş değilse); `cargo check` sonucu LLM'e `[... SADECE SENİN GÖZÜN İÇİN ...]` bloğuyla gider — saklama/tahmin kararı USTA.md kuralında, kodda değil.
- **Global USTA.md güncellemesi (v0.3):** scaffold var olan dosyanın üstüne yazmaz — davranış güncellemesinden sonra `rm ~/.config/usta/USTA.md` + bir kez `usta` çalıştırmak gerekir. Bilinçli kabul; dosya versiyonlama v0.4 adayı.
```

- [x] **Step 3: Commit + push**

```bash
git add SPEC.md
git commit -m "SPEC: v0.3 pedagoji katmanı — drill, Feynman, merdiven, tahmin protokolü

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Bitiş Doğrulaması (tüm görevler sonrası)

- [ ] `cargo test` — tamamı PASS
- [ ] `cargo build` — uyarısız
- [ ] **Migrasyon:** `rm ~/.config/usta/USTA.md` çalıştır, ardından herhangi bir dizinde `usta` bir kez aç-kapa — global USTA.md yeni pedagoji kurallarıyla yeniden yazılmış olmalı (`grep "Tahmin Protokolü" ~/.config/usta/USTA.md` dolu dönmeli).
- [ ] Manuel duman testi (backend varsa): (1) progress'li konuda oturum aç → Usta ilk sözü alıp soru sorsun. (2) Cargo projesinde bozuk kod kaydet → hata dökülmesin, tahmin sorulsun. (3) `/quit` → progress dosyasında `## Geri çağırma soruları` ve `## Hata günlüğü` bölümleri oluşsun.
