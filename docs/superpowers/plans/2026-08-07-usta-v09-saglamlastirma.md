# Usta v0.9 — Sağlamlaştırma Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Mutlu-yol dışındaki beş gerçek riski kapat: (1) flush ölürse oturum kaybolmasın (ham transcript kaydı + kurtarma bildirimi), (2) watcher olay seli LLM çağrı bombasına dönmesin (git checkout senaryosu — batch tavanı), (3) aynı konuda iki terminal sessizce veri ezmesin (lockfile + onay), (4) kötü model çıktısı geri alınabilsin (`.bak`), (5) dosyalar şişmesin + sır dosyaları LLM'e gitmesin.

**Architecture:** Yeni `src/transcript.rs`: her turn `.usta/sessions/<konu>-<zaman>.jsonl`'e anında append edilir (JSON satırı); başarılı kapanış flush'ında dosya `.done.jsonl`'e taşınır — açılışta `.done` olmayan dosya = yarım oturum, kullanıcıya bildirilir. Watcher tavanı: debounce batch'i N dosyayı aşarsa LLM feedback atlanır ama `FileMemory` sessizce senkronlanır (sonraki tekil kayıt dev diff üretmesin). Lockfile: `.usta/.lock-<konu>` pid taşır; varsa onay istenir, çıkışta silinir. `.bak`: `write_atomic` üzerine yazmadan önce mevcudu kopyalar. Budama + sır filtresi: kapanış kuralı + `is_ignored` genişletmesi.

**Tech Stack:** v0.8 sonrası yığın. Yeni bağımlılık YOK.

## Global Constraints

- **ÖN KOŞUL: v0.2–v0.8 planlarının TAMAMI uygulanmış ve commit'lenmiş olmalı.** Bitmemişse DUR ve bildir.
- Bu repo kendi git'ine sahip (`usta/` içinde çalış, headspace repo'suna commit atma).
- Tüm kod yorumları ve kullanıcıya görünen mesajlar **Türkçe**. Modül başları `//!` doc.
- Commit başlık deseni: `<scope>: kısa türkçe özet`.
- Her görev sonunda `cargo test`, `cargo build`, `cargo clippy` temiz.
- Test isimleri `snake_case`; mevcut testler imza değişiminde UYARLANIR, silinmez.
- Saf mantık test edilebilir fonksiyonda; IO/async kabukta.
- Transcript/lock hataları OTURUMU ASLA ENGELLEMEZ — hepsi warn-and-continue (sağlamlaştırma katmanı ana akışı kırarsa amacına ihanet eder).
- Commit mesajı sonuna ekle: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`

## Dosya Haritası

| Dosya | Sorumluluk | Değişim |
|---|---|---|
| `src/transcript.rs` | **YENİ** — jsonl satırı, oturum yolu, append, done-işaretleme, yarım-oturum bulma | oluşturulur |
| `src/main.rs` | Recorder çağrıları, batch tavanı, lockfile, kurtarma bildirimi | güncellenir |
| `src/progress.rs` | `write_atomic` `.bak` + kapanış budama kuralı | güncellenir |
| `src/watcher.rs` | `is_ignored` sır filtreleri | güncellenir |
| `SPEC.md` | v0.9 kararları | güncellenir |

---

### Task 1: Ham transcript kaydı + kurtarma bildirimi (TDD)

**Files:**
- Create: `src/transcript.rs`
- Modify: `src/main.rs` (`mod transcript;`, Recorder oluşturma, turn kayıtları, çıkışta done-işaretleme, açılışta yarım-oturum bildirimi, `now_stamp()`)

**Interfaces:**
- Produces:
  - `transcript::line(role: &str, text: &str) -> String` — tek JSON satırı + `\n`
  - `transcript::session_path(project_root: &Path, topic: &str, stamp: &str) -> PathBuf` → `.usta/sessions/<topic>-<stamp>.jsonl`
  - `transcript::Recorder` — `new(path: PathBuf) -> Recorder`, `user(&self, text: &str)`, `assistant(&self, text: &str)` (append; hata = sessiz, ilk hatada bir kez warn)
  - `transcript::mark_done(path: &Path) -> Result<()>` — `.jsonl` → `.done.jsonl` rename
  - `transcript::find_unfinished(project_root: &Path) -> Vec<PathBuf>` — `sessions/` altındaki `.done.jsonl` OLMAYAN `.jsonl` dosyaları
- Sözleşme: her turn diske anında iner; flush/proses ölse bile oturum ham olarak durur. Kayıt hatası oturumu engellemez.

- [ ] **Step 1: Failing testleri yaz**

`src/transcript.rs` (önce testler):

```rust
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
}
```

`src/main.rs`'e `mod transcript;` ekle. Run: `cargo test transcript`
Expected: FAIL.

- [ ] **Step 2: Implemente et**

```rust
//! Ham oturum kaydı: her turn anında `.usta/sessions/<konu>-<zaman>.jsonl`'e
//! append edilir — kapanış flush'ı ölse, terminal çökse bile oturum diskte.
//! Başarılı flush sonrası dosya `.done.jsonl` olur; açılışta `.done` olmayan
//! dosya = kurtarılabilir yarım oturum. Kayıt hatası oturumu ASLA engellemez.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;

/// Tek turn'ün JSON satırı.
pub fn line(role: &str, text: &str) -> String {
    let mut l = serde_json::json!({ "role": role, "text": text }).to_string();
    l.push('\n');
    l
}

/// Oturum dosyası yolu: `.usta/sessions/<konu>-<zaman>.jsonl`.
pub fn session_path(project_root: &Path, topic: &str, stamp: &str) -> PathBuf {
    project_root
        .join(".usta/sessions")
        .join(format!("{topic}-{stamp}.jsonl"))
}

/// Başarılı kapanış: `.jsonl` → `.done.jsonl`.
pub fn mark_done(path: &Path) -> Result<()> {
    let done = path.with_extension("done.jsonl");
    std::fs::rename(path, done)?;
    Ok(())
}

/// `.done` işareti olmayan oturum dosyaları — flush edilememiş yarım oturumlar.
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

/// Turn kaydedici — hata sessizdir, ilk hatada BİR KEZ uyarır.
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
            crate::ui::warn("oturum kaydı yazılamıyor — kayıt olmadan devam");
        }
    }
}
```

- [ ] **Step 3: main.rs'e bağla**

1. Zaman damgası yardımcısı (`today()` yanına):

```rust
/// Oturum dosya adı damgası — yerel saat.
fn now_stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}
```

2. Açılışta (banner'dan önce) yarım oturum bildirimi + recorder:

```rust
    for p in transcript::find_unfinished(&project_root) {
        ui::warn(&format!("yarım oturum kaydı bulundu (flush edilememiş olabilir): {}", p.display()));
    }
    let recorder = transcript::Recorder::new(transcript::session_path(
        &project_root, &topic, &now_stamp(),
    ));
```

3. Turn kayıtları — her `session.push_user(x)` sonrasına `recorder.user(x)`, her `session.push_assistant(reply.text)` ÖNCESİNE `recorder.assistant(&reply.text)` (metin move edilmeden). DÖRT nokta: select-loop kullanıcı turn'ü, `handle_file_change` (imzaya `recorder: &transcript::Recorder` eklenir, çağrı yeri güncellenir), drill bloğu, tanışma bloğu. Flush'ın kapanış promptu KAYDEDİLMEZ (sentetik).

4. Çıkışta — final `flush_progress` BAŞARILIYSA:

```rust
    if let Err(e) = flush_progress(&mut backend, &session, &project_root).await {
        ui::warn(&format!("progress güncellenemedi: {e} — ham kayıt duruyor: {}", recorder.path().display()));
    } else if session.history().is_empty() {
        // Boş oturum: dosya hiç oluşmadı, işaretlenecek şey yok.
    } else if let Err(e) = transcript::mark_done(recorder.path()) {
        ui::warn(&format!("oturum kaydı işaretlenemedi: {e}"));
    }
```

(`maybe_compact` içindeki ara-flush `mark_done` ÇAĞIRMAZ — oturum devam ediyor.)

- [ ] **Step 4: Test + build**

Run: `cargo test && cargo build && cargo clippy`
Expected: yeni 4 test dahil hepsi PASS.

- [ ] **Step 5: Commit**

```bash
git add src/transcript.rs src/main.rs
git commit -m "transcript: ham oturum kaydı — flush ölse bile oturum diskte

Her turn anında jsonl'e iner; başarılı kapanışta .done, açılışta yarım
oturum bildirimi. Kayıt hatası oturumu engellemez.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Watcher olay seli tavanı

**Files:**
- Modify: `src/main.rs` (select-loop debounce-flush kolu)

**Interfaces:**
- Produces: `main::MAX_FEEDBACK_BATCH: usize = 5`
- Sözleşme: debounce batch'i tavanı aşarsa (git checkout, toplu formatlama) LLM feedback ATLANIR ama `FileMemory` sessizce senkronlanır — sonraki tekil kayıt, atlanmış yığına karşı dev diff üretmez. Tek bildirim satırı basılır.

- [ ] **Step 1: Implemente et**

```rust
/// Tek debounce penceresinde feedback verilecek azami dosya sayısı — üstü
/// "toplu değişiklik" sayılır (git checkout, format-all): LLM çağrısı yok.
const MAX_FEEDBACK_BATCH: usize = 5;
```

Select-loop'un deadline kolunu değiştir:

```rust
            _ = sleep_until_deadline(debouncer.deadline()), if debouncer.deadline().is_some() => {
                println!();
                let batch = debouncer.flush();
                if batch.len() > MAX_FEEDBACK_BATCH {
                    ui::notice(&format!(
                        "toplu değişiklik ({} dosya) — feedback atlandı, izleme sürüyor",
                        batch.len()
                    ));
                    // FileMemory'yi sessizce senkronla: sonraki tekil kayıt
                    // bu yığına karşı dev diff üretmesin.
                    for path in batch {
                        if let Ok(c) = std::fs::read_to_string(&path) {
                            let _ = files.observe(&path, c);
                        }
                    }
                } else {
                    for path in batch {
                        match handle_file_change(&mut backend, &mut session, &mut files, &project_root, &recorder, &path).await {
                            Ok(tokens) => maybe_compact(&mut backend, &mut session, &project_root, tokens).await,
                            Err(e) => ui::warn(&format!("dosya feedback atlandı: {}: {e}", path.display())),
                        }
                    }
                }
            }
```

(`handle_file_change` imzasındaki `recorder` parametresi Task 1'den geliyor — parametre sırası oradakiyle aynı tutulur.)

- [ ] **Step 2: Test + build + duman**

Run: `cargo test && cargo build && cargo clippy`
Expected: hepsi PASS. Duman (backend gerekmez): izlenen temp dizinde `for i in $(seq 1 20); do echo x > f$i.txt; done` → tek "toplu değişiklik (20 dosya)" bildirimi, LLM çağrısı yok.

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "main: watcher olay seli tavanı — git checkout LLM bombası olmaz

5+ dosyalık debounce batch'i feedback'siz geçer, FileMemory sessizce
senkronlanır.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Konu lockfile — eşzamanlı oturum koruması

**Files:**
- Modify: `src/main.rs`

**Interfaces:**
- Produces: `lock_path(project_root, topic) -> PathBuf` → `.usta/.lock-<topic>`
- Sözleşme: açılışta lock varsa TTY'de onay istenir ("başka oturum açık olabilir"); onay yoksa temiz çıkış. TTY değilse (pipe/test) uyarıp devam edilir — otomasyon stale lock'a takılmaz. Lock çıkışta silinir; çökme stale lock bırakır → onay mekanizması zaten karşılar.

- [ ] **Step 1: Implemente et**

```rust
/// Konu kilidi: `.usta/.lock-<konu>` — eşzamanlı iki oturumun aynı progress'i
/// sessizce ezmesini önler. İçerik: pid (teşhis için).
fn lock_path(project_root: &Path, topic: &str) -> PathBuf {
    project_root.join(".usta").join(format!(".lock-{topic}"))
}
```

Açılışta (recorder oluşturmadan önce):

```rust
    let lock = lock_path(&project_root, &topic);
    if lock.exists() {
        let pid = std::fs::read_to_string(&lock).unwrap_or_default();
        if std::io::stdin().is_terminal() {
            let msg = format!(
                "Bu konuda başka bir oturum açık görünüyor (pid {}). İki oturum aynı anda \
                 kapanırsa progress birbirini EZER. Yine de devam? [e/H] ",
                pid.trim()
            );
            if !confirm(&msg, &["e", "evet"])? {
                println!("vazgeçildi — önce diğer oturumu kapat (veya kalıntıysa sil: {})", lock.display());
                return Ok(());
            }
        } else {
            ui::warn("kalıntı konu kilidi bulundu — pipe modunda devam ediliyor");
        }
    }
    if let Err(e) = std::fs::write(&lock, std::process::id().to_string()) {
        ui::warn(&format!("konu kilidi yazılamadı: {e}"));
    }
```

Çıkışta — `flush_progress`/`mark_done` bloğundan SONRA, son `Ok(())` öncesi:

```rust
    let _ = std::fs::remove_file(&lock);
```

- [ ] **Step 2: Test + build + duman**

Run: `cargo test && cargo build && cargo clippy`
Expected: hepsi PASS. Duman: sandbox'ta bir oturum aç, İKİNCİ terminalde aynı konuyu aç → onay sorusu gelsin; "H" → temiz çıkış. İlk oturum `/quit` → lock silinmiş olsun (`ls .usta/.lock-*` boş).

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "main: konu lockfile — eşzamanlı oturum progress'i sessizce ezemez

Açılışta onay, çıkışta temizlik; pipe modunda uyarıp geçer (stale lock
otomasyonu kilitlemez).

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: `write_atomic` yedeği — `.bak` (TDD)

**Files:**
- Modify: `src/progress.rs`

**Interfaces:**
- `write_atomic` davranış ekI: hedef dosya VARSA üzerine yazmadan önce mevcut içerik `<dosya>.bak`'a kopyalanır (tek nesil yedek). Kötü model çıktısı tek `cp` ile geri alınır.

- [ ] **Step 1: Failing testi yaz**

`src/progress.rs` test modülüne:

```rust
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
```

Run: `cargo test write_atomic`
Expected: yeni test FAIL, eski `write_atomic_creates_parents_and_writes` PASS.

- [ ] **Step 2: Implemente et**

`write_atomic` içinde, tmp yazımından ÖNCE:

```rust
    // Önceki sürümü yedekle — kötü model çıktısı tek kopyayla geri alınır.
    if path.exists() {
        let bak = path.with_extension("md.bak");
        let _ = std::fs::copy(path, &bak);
    }
```

- [ ] **Step 3: Test + build + commit**

Run: `cargo test && cargo build && cargo clippy` → PASS.

```bash
git add src/progress.rs
git commit -m "progress: write_atomic .bak yedeği — kötü flush geri alınabilir

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: Budama kuralı + sır filtresi (TDD)

**Files:**
- Modify: `src/progress.rs` (`closing_prompt` kural satırı)
- Modify: `src/watcher.rs` (`is_ignored`)

**Interfaces:**
- `closing_prompt` kurallarına dosya-şişmesi budaması eklenir.
- `is_ignored` sır desenlerini de eler: `.pem`, `.key`, adında `secret`/`credential` geçen dosyalar (büyük/küçük harf duyarsız).

- [ ] **Step 1: Failing testleri yaz**

`src/watcher.rs` test modülüne:

```rust
#[test]
fn is_ignored_blocks_secret_files() {
    assert!(is_ignored(Path::new("config/server.pem")));
    assert!(is_ignored(Path::new("keys/deploy.key")));
    assert!(is_ignored(Path::new("config/client_secrets.yaml")));
    assert!(is_ignored(Path::new("aws/CREDENTIALS.json")));
}

#[test]
fn is_ignored_allows_normal_config() {
    assert!(!is_ignored(Path::new("config/settings.yaml")));
}
```

`src/progress.rs` test modülüne:

```rust
#[test]
fn closing_prompt_includes_pruning_rule() {
    let s = closing_prompt("rust", None, None, None);
    assert!(s.contains("20 madde"));
}
```

Run: `cargo test is_ignored && cargo test pruning`
Expected: FAIL.

- [ ] **Step 2: Implemente et**

`is_ignored` closure'ındaki `Normal(s)` kolunu genişlet:

```rust
        std::path::Component::Normal(s) => {
            let s = s.to_string_lossy().to_ascii_lowercase();
            s == "target"
                || s == "node_modules"
                || s.starts_with('.')
                // Sır dosyaları LLM'e asla gitmez.
                || s.ends_with(".pem")
                || s.ends_with(".key")
                || s.contains("secret")
                || s.contains("credential")
        }
```

`closing_prompt` kurallarına (budama — "Oturumda kanıtı olmayanı ekleme" satırından önce) ekle:

```
 - Dosyaları ŞİŞİRME: `Kapatılanlar` 20 maddeyi aşarsa en eskileri tek satırlık \
 dönem özetine indir; `Hata günlüğü`nde çözülüp uzun süredir görülmeyen satırları \
 kaldır; curriculum'da değişmeyen bölümleri olduğu gibi koru (yeniden üretme).
```

- [ ] **Step 3: Test + build + commit**

Run: `cargo test && cargo build && cargo clippy` → PASS (mevcut `is_ignored_allows_src_file` vb. bozulmadan).

```bash
git add src/watcher.rs src/progress.rs
git commit -m "watcher+progress: sır filtresi + dosya budama kuralı

.pem/.key/secret/credential LLM'e gitmez; progress dosyaları sınırsız
büyümez (20 madde eşiği).

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: SPEC v0.9 güncellemesi

**Files:**
- Modify: `SPEC.md`

- [ ] **Step 1: §4.10'dan sonra ekle**

```markdown
## 4.11 Sağlamlaştırma (v0.9)

- **Ham oturum kaydı:** her turn anında `.usta/sessions/<konu>-<zaman>.jsonl`'e iner; başarılı kapanışta `.done.jsonl`. Flush ölse/terminal çökse oturum diskte — açılışta yarım oturum bildirilir.
- **Olay seli tavanı:** 5+ dosyalık debounce batch'i (git checkout, format-all) LLM'siz geçer; `FileMemory` sessizce senkronlanır.
- **Konu kilidi:** `.usta/.lock-<konu>` — eşzamanlı ikinci oturum onayla açılır, progress sessizce ezilmez. Pipe modunda uyarı + devam.
- **Yedek:** `write_atomic` önceki sürümü `.bak`'a kopyalar — kötü model çıktısı geri alınabilir.
- **Budama + sır filtresi:** progress 20-madde eşiğiyle budanır; `.pem`/`.key`/`secret`/`credential` dosyaları watcher'dan LLM'e asla gitmez.
```

- [ ] **Step 2: "Alınan Kararlar" bölümüne ekle**

```markdown
- **Sağlamlaştırma (v0.9):** transcript/lock hataları warn-and-continue (ana akışı asla kırmaz); batch tavanı 5; yedek tek nesil (`.bak`); yarım oturum otomatik işlenmez, sadece bildirilir (kurtarma kullanıcı kararı — YAGNI).
```

- [ ] **Step 3: Commit + push**

```bash
git add SPEC.md
git commit -m "SPEC: v0.9 sağlamlaştırma — kayıt, tavan, kilit, yedek, filtre

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Bitiş Doğrulaması (tüm görevler sonrası)

- [ ] `cargo test` — tamamı PASS; `cargo build` + `cargo clippy` temiz
- [ ] Sandbox duman:
  1. Oturum aç, 2 turn konuş → `.usta/sessions/*.jsonl` büyüsün; `/quit` → `.done.jsonl` olsun.
  2. Oturum aç, terminali öldür (pencereyi kapat) → yeni oturumda "yarım oturum kaydı bulundu" uyarısı gelsin.
  3. İzlenen dizinde 20 dosya birden değiştir → tek "toplu değişiklik" bildirimi, LLM çağrısı yok.
  4. Aynı konuda ikinci terminal → onay sorusu; "H" → temiz çıkış; ilk oturum kapanınca lock silinsin.
  5. İki kez `/quit`'li oturum → `progress/rust.md.bak` önceki sürümü tutsun.
  6. `echo x > sunucu.pem` → feedback GELMESİN (sır filtresi).
- [ ] Düz mod regresyonu: `echo "" | cargo run -- start deneme` → ANSI yok, lock uyarısı pipe'ta engel olmamış.
