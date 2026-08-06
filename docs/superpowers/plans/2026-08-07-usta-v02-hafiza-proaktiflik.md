# Usta v0.2 — Hafıza + Gerçek Proaktiflik Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Usta'ya kalıcı hafıza (kapanışta progress yazımı), gerçekten proaktif dosya feedback'i (select-loop + debounce + diff + boyut tavanı) ve ucuz CLI oturum sürdürme (`--resume`) ekle.

**Architecture:** Bloklayan `rustyline` döngüsü, girdi-thread'i + `tokio::select!` event-loop'una dönüşür (girdi, watcher ve debounce zamanlayıcısı aynı döngüde yarışır). Dosya feedback'i saf `FileMemory` durumundan geçer (ilk görüşte tam içerik → sonra unified diff → 64KB üstü yerel uyarı). Oturum kapanışında son bir LLM çağrısı `progress/<konu>.md`'nin tam yeni içeriğini üretir ve atomik yazılır. CLI backend ilk çağrıda `--output-format json`'dan `session_id` yakalayıp sonraki turn'leri `--resume` ile sürdürür.

**Tech Stack:** Rust 2021, tokio (`macros`, `rt-multi-thread`, `process`, `io-util`, + yeni: `sync`, `time`), notify 6, rustyline 14, similar 2 (yeni), serde/serde_json, anyhow.

## Global Constraints

- Bu repo kendi git'ine sahip (`usta/` içinde çalış, headspace repo'suna commit atma).
- Tüm kod yorumları, kullanıcıya görünen mesajlar ve doc-comment'ler **Türkçe** (mevcut stil). Modül başları `//!` doc taşır.
- Commit başlık deseni mevcut log'la uyumlu: `<scope>: kısa türkçe özet` (ör. `watcher: tokio kanalına geç`).
- Her görev sonunda `cargo test` ve `cargo build` temiz olmalı (uyarı çıkarsa düzelt).
- Test isimleri mevcut desende: `snake_case`, davranışı cümle gibi anlatır (`slugify_takes_first_token_only`).
- Saf (pure) mantık her zaman test edilebilir fonksiyon/struct'a çıkarılır; IO/async kabukta kalır (mevcut mimari deseni).
- Commit mesajı sonuna ekle: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`

## Dosya Haritası

| Dosya | Sorumluluk | Değişim |
|---|---|---|
| `src/watcher.rs` | notify → kanal + `Debouncer` saf durumu | tokio kanalına geçiş, `Debouncer` eklenir, `dedup_paths` silinir |
| `src/input.rs` | **YENİ** — rustyline'ı ayrı thread'de koşturan girdi kanalı | oluşturulur |
| `src/feedback.rs` | **YENİ** — `FileMemory`: ilk-görüş/diff/boyut kararı | oluşturulur |
| `src/progress.rs` | **YENİ** — kapanış promptu, yanıt temizleme, atomik yazım | oluşturulur |
| `src/backend.rs` | CLI `--resume` + JSON çıktı parse | `complete` `&mut self` olur, `parse_cli_output` eklenir |
| `src/main.rs` | select-loop kabuğu, kapanış flush'ı | ana döngü yeniden yazılır |
| `src/session.rs` | `topic` artık kullanılıyor | `#[allow(dead_code)]` kalkar |
| `Cargo.toml` | tokio feature + `similar` | güncellenir |
| `SPEC.md` | v0.2 kararları | güncellenir |

---

### Task 1: Watcher'ı tokio kanalına geçir

**Files:**
- Modify: `src/watcher.rs`
- Modify: `src/main.rs:60` (drain döngüsü — `try_recv` aynı kalır, tip değişir)
- Modify: `Cargo.toml:8` (tokio features)

**Interfaces:**
- Produces: `watcher::spawn(root: &Path) -> Result<tokio::sync::mpsc::UnboundedReceiver<PathBuf>>` — Task 3'ün select-loop'u bu receiver'ı `recv().await` ile kullanacak.
- `is_ignored`, `dedup_paths` değişmez (dedup Task 2'de silinecek).

- [ ] **Step 1: Cargo.toml'a tokio `sync` + `time` feature'larını ekle**

```toml
tokio = { version = "1", features = ["macros", "rt-multi-thread", "process", "io-util", "sync", "time"] }
```

(`time` bu görevde gerekmiyor ama Task 2-3 kullanacak — tek Cargo değişikliği burada toplanır.)

- [ ] **Step 2: `src/watcher.rs`'te kanalı değiştir**

`use std::sync::mpsc::{self, Receiver};` satırını kaldır, yerine:

```rust
use std::sync::mpsc;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
```

`spawn` imzası ve gövdesindeki out-kanalı:

```rust
/// `root` altını özyinelemeli izle; modify olaylarındaki dosya yollarını gönder.
/// Dönen alıcı select-loop'ta `recv().await` (veya `try_recv`) ile tüketilir.
pub fn spawn(root: &Path) -> Result<UnboundedReceiver<PathBuf>> {
    let (out_tx, out_rx) = unbounded_channel::<PathBuf>();
    let (ev_tx, ev_rx) = mpsc::channel::<notify::Result<notify::Event>>();
```

Geri kalan gövde aynı — `out_tx.send(path).is_err()` tokio `UnboundedSender`'da da senkron çalışır, thread içinden gönderim güvenli.

- [ ] **Step 3: `src/main.rs` drain'i derlet**

`main` içindeki drain bloğunda tek değişiklik: `watch_rx` artık tokio receiver, `try_recv()` `Result<T, TryRecvError>` döndürmeye devam eder — `while let Ok(p) = watch_rx.try_recv()` olduğu gibi derlenir. `let watch_rx` → `let mut watch_rx` yap (tokio `try_recv` `&mut self` ister).

- [ ] **Step 4: Test + build**

Run: `cargo test && cargo build`
Expected: tüm mevcut testler PASS (watcher testleri saf fonksiyonlarda — etkilenmez), build temiz.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/watcher.rs src/main.rs
git commit -m "watcher: std mpsc yerine tokio unbounded kanal

select-loop (sonraki adım) için async recv gerekli.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Debouncer (saf durum, TDD)

**Files:**
- Modify: `src/watcher.rs` (Debouncer eklenir, `dedup_paths` + testleri silinir)
- Modify: `src/main.rs` (`dedup_paths` çağrısı geçici olarak `Debouncer`'sız düz döngüye döner — Task 3 gerçek entegrasyonu yapar)

**Interfaces:**
- Produces:
  - `watcher::Debouncer::new(window: std::time::Duration) -> Debouncer`
  - `Debouncer::push(&mut self, path: PathBuf, now: tokio::time::Instant)` — path'i dedup'layarak biriktirir, deadline'ı `now + window`'a atar
  - `Debouncer::deadline(&self) -> Option<tokio::time::Instant>`
  - `Debouncer::flush(&mut self) -> Vec<PathBuf>` — birikeni ilk-görülme sırasıyla döndürür, deadline'ı temizler
- Consumes: yok (saf).

- [ ] **Step 1: Failing testleri yaz**

`src/watcher.rs` test modülüne ekle (eski `dedup_*` testlerini SİL, yerlerine):

```rust
use std::time::Duration;
use tokio::time::Instant;

#[test]
fn debouncer_push_dedups_and_preserves_order() {
    let mut d = Debouncer::new(Duration::from_millis(1000));
    let now = Instant::now();
    d.push(PathBuf::from("a.rs"), now);
    d.push(PathBuf::from("b.rs"), now);
    d.push(PathBuf::from("a.rs"), now);
    assert_eq!(d.flush(), vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")]);
}

#[test]
fn debouncer_push_extends_deadline() {
    let mut d = Debouncer::new(Duration::from_millis(1000));
    let t0 = Instant::now();
    d.push(PathBuf::from("a.rs"), t0);
    let t1 = t0 + Duration::from_millis(500);
    d.push(PathBuf::from("b.rs"), t1);
    assert_eq!(d.deadline(), Some(t1 + Duration::from_millis(1000)));
}

#[test]
fn debouncer_flush_clears_pending_and_deadline() {
    let mut d = Debouncer::new(Duration::from_millis(1000));
    d.push(PathBuf::from("a.rs"), Instant::now());
    let _ = d.flush();
    assert!(d.deadline().is_none());
    assert!(d.flush().is_empty());
}

#[test]
fn debouncer_empty_has_no_deadline() {
    let d = Debouncer::new(Duration::from_millis(1000));
    assert!(d.deadline().is_none());
}
```

- [ ] **Step 2: Testlerin fail ettiğini gör**

Run: `cargo test debouncer`
Expected: FAIL — "cannot find struct `Debouncer`" (derleme hatası da fail sayılır).

- [ ] **Step 3: Debouncer'ı implemente et, `dedup_paths`'i sil**

`src/watcher.rs`'e ekle:

```rust
use std::time::Duration;
use tokio::time::Instant;

/// Kayıt fırtınasını yatıştıran saf debounce durumu. Editörler tek kayıtta
/// birden çok modify olayı üretir; her olay `push`lanır, son olaydan `window`
/// sonra `deadline` dolar ve select-loop `flush` ile hepsini tek seferde işler.
pub struct Debouncer {
    pending: Vec<PathBuf>,
    deadline: Option<Instant>,
    window: Duration,
}

impl Debouncer {
    pub fn new(window: Duration) -> Self {
        Debouncer { pending: Vec::new(), deadline: None, window }
    }

    /// Yolu biriktir (tekrarları ilk-görülme sırasını koruyarak ele) ve
    /// deadline'ı ileri at.
    pub fn push(&mut self, path: PathBuf, now: Instant) {
        if !self.pending.contains(&path) {
            self.pending.push(path);
        }
        self.deadline = Some(now + self.window);
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Birikeni boşalt, deadline'ı sıfırla.
    pub fn flush(&mut self) -> Vec<PathBuf> {
        self.deadline = None;
        std::mem::take(&mut self.pending)
    }
}
```

`dedup_paths` fonksiyonunu ve `HashSet` import'unu sil. `src/main.rs`'te drain bloğunu geçici olarak şuna indir (Task 3 tamamen yeniden yazacak):

```rust
let mut changed: Vec<PathBuf> = Vec::new();
while let Ok(p) = watch_rx.try_recv() {
    if !changed.contains(&p) {
        changed.push(p);
    }
}
for path in changed {
```

- [ ] **Step 4: Test + build**

Run: `cargo test && cargo build`
Expected: 4 yeni debouncer testi PASS, eski dedup testleri yok, build temiz.

- [ ] **Step 5: Commit**

```bash
git add src/watcher.rs src/main.rs
git commit -m "watcher: Debouncer — kayıt fırtınası tek feedback'e iner

dedup_paths yerini alır; 1sn pencere select-loop'ta bağlanacak.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Girdi thread'i + select-loop — gerçek proaktiflik

**Files:**
- Create: `src/input.rs`
- Modify: `src/main.rs` (ana döngü yeniden yazılır, `resolve_topic` imzası değişir)

**Interfaces:**
- Consumes: `watcher::spawn` (Task 1), `watcher::Debouncer` (Task 2).
- Produces:
  - `input::InputEvent` enum: `Line(String)` | `Eof`
  - `input::spawn(prompt: &'static str, ready_rx: std::sync::mpsc::Receiver<()>) -> tokio::sync::mpsc::UnboundedReceiver<InputEvent>`
  - `main` içinde `handle_file_change(&Backend, &mut Session, &Path)` imzası korunur (Task 4 değiştirecek).
- Davranış sözleşmesi: dosya kaydı geldiğinde kullanıcı Enter'a basmasa bile ~1 sn sonra feedback basılır. `/quit`, Ctrl-C, Ctrl-D üçü de döngüden temiz çıkar.

- [ ] **Step 1: `src/input.rs`'i yaz**

```rust
//! Kullanıcı girdisi: rustyline ayrı thread'de koşar, satırlar tokio kanalına
//! akar. Böylece ana döngü girdi beklerken watcher olaylarını da işleyebilir
//! (gerçek proaktif feedback). `ready` el-sıkışması, prompt'un Usta yanıtının
//! ORTASINA basılmasını önler: ana döngü bir turn'ü bitirince `()` yollar,
//! thread ancak o zaman yeni `sen> ` çizer.

use std::sync::mpsc::Receiver as ReadyReceiver;
use std::thread;

use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

/// Girdi olayı: bir satır veya kapanış isteği (Ctrl-C / Ctrl-D / girdi hatası).
pub enum InputEvent {
    Line(String),
    Eof,
}

/// Girdi thread'ini başlat. Her `ready_rx` sinyalinden sonra TEK satır okur ve
/// kanala yollar; ana döngü işleyip yeni `ready` gönderene dek tekrar okumaz.
/// `ready_tx` düşerse (ana döngü bitti) thread sessizce kapanır.
pub fn spawn(
    prompt: &'static str,
    ready_rx: ReadyReceiver<()>,
) -> UnboundedReceiver<InputEvent> {
    let (tx, rx) = unbounded_channel();
    thread::spawn(move || {
        let mut rl = match DefaultEditor::new() {
            Ok(rl) => rl,
            Err(_) => {
                let _ = tx.send(InputEvent::Eof);
                return;
            }
        };
        while ready_rx.recv().is_ok() {
            match rl.readline(prompt) {
                Ok(line) => {
                    if !line.trim().is_empty() {
                        let _ = rl.add_history_entry(&line);
                    }
                    if tx.send(InputEvent::Line(line)).is_err() {
                        return;
                    }
                }
                // Ctrl-D / Ctrl-C → kapanış sinyali, thread biter.
                Err(ReadlineError::Eof) | Err(ReadlineError::Interrupted) => {
                    let _ = tx.send(InputEvent::Eof);
                    return;
                }
                Err(_) => {
                    let _ = tx.send(InputEvent::Eof);
                    return;
                }
            }
        }
    });
    rx
}
```

- [ ] **Step 2: `resolve_topic`'i rustyline parametresinden arındır**

`src/main.rs` — imza `fn resolve_topic(args: &[String]) -> Result<String>` olur; editörünü kendi içinde kurar, iş bitince düşer (girdi thread'i kendi editörünü sonra kurar — çakışma yok):

```rust
/// Konuyu çöz: açık argüman > TTY promptu > sessiz "genel" default'u.
/// Stdin pipe'lanmışsa (TTY değilse) cevaplanamayacak bir prompt'a takılmadan
/// direkt "genel" döner.
fn resolve_topic(args: &[String]) -> Result<String> {
    if let Some(raw) = explicit_topic(args) {
        return Ok(slugify_topic(&raw));
    }
    if !std::io::stdin().is_terminal() {
        return Ok("genel".to_string());
    }
    let mut rl = DefaultEditor::new()?;
    match rl.readline("Ne öğreneceksin/yapacaksın? (ör. rust, javascript): ") {
        Ok(line) => Ok(slugify_topic(&line)),
        // Ctrl-D / Ctrl-C promptta → engellemeden "genel"e düş.
        Err(_) => Ok("genel".to_string()),
    }
}
```

- [ ] **Step 3: Ana döngüyü select-loop olarak yeniden yaz**

`src/main.rs` — `mod input;` ekle. `main` gövdesinde `let mut rl = DefaultEditor::new()?;` satırını sil; döngü şu hale gelir:

```rust
    let mut session = Session::new(topic.clone(), system);

    // Dosya izleyici + girdi thread'i + debounce durumu.
    let mut watch_rx = watcher::spawn(&project_root)?;
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
    let mut input_rx = input::spawn("sen> ", ready_rx);
    let mut debouncer = watcher::Debouncer::new(std::time::Duration::from_millis(1000));

    println!("Usta hazır — konu: {topic}. Kod yaz, kaydet; ben izlerim. (/quit ile çık)");
    let _ = ready_tx.send(()); // ilk prompt

    loop {
        tokio::select! {
            maybe_ev = input_rx.recv() => match maybe_ev {
                Some(input::InputEvent::Line(line)) => {
                    let line = line.trim().to_string();
                    if line == "/quit" {
                        break;
                    }
                    if !line.is_empty() {
                        session.push_user(&line);
                        match backend.complete(&session.system, session.history()).await {
                            Ok((reply, web)) => {
                                print_reply(&reply, web);
                                session.push_assistant(reply);
                            }
                            Err(e) => eprintln!("(hata: {e})"),
                        }
                    }
                    let _ = ready_tx.send(());
                }
                Some(input::InputEvent::Eof) | None => break,
            },
            Some(path) = watch_rx.recv() => {
                debouncer.push(path, tokio::time::Instant::now());
            },
            _ = sleep_until_deadline(debouncer.deadline()), if debouncer.deadline().is_some() => {
                // Kullanıcı prompt'tayken de çalışır — gerçek proaktiflik.
                println!(); // yarım kalan prompt satırını kirletme
                for path in debouncer.flush() {
                    if let Err(e) = handle_file_change(&backend, &mut session, &path).await {
                        // Binary/silinmiş dosya vb. — sessizce geç, REPL yaşar.
                        eprintln!("(dosya feedback atlandı: {}: {e})", path.display());
                    }
                }
            }
        }
    }

    println!("Görüşürüz — suya girmeye devam et.");
    Ok(())
```

Döngü altına yardımcıyı ekle:

```rust
/// Deadline varsa ona kadar uyu; yoksa asla dönmeyen future (select guard'ı
/// zaten bu kolu deadline'sız poll etmez — bu sadece tip güvenliği).
async fn sleep_until_deadline(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(d) => tokio::time::sleep_until(d).await,
        None => std::future::pending().await,
    }
}
```

Eski drain bloğu (`let mut changed ...` + `rl.readline` match'i) tamamen silinir. `use rustyline::error::ReadlineError;` ve `use rustyline::DefaultEditor;` import'ları main.rs'te `resolve_topic` için kalır (`ReadlineError` artık kullanılmıyorsa import'unu sil).

- [ ] **Step 4: Test + build + manuel duman testi**

Run: `cargo test && cargo build`
Expected: tüm testler PASS, build temiz.

Manuel (mümkünse): boş bir temp dizinde `cargo run -- start deneme` başlat, İKİNCİ terminalden `echo "fn main() {}" > /tmp/<odizin>/x.rs` yaz. Beklenen: Enter'a basmadan ~1 sn içinde Usta feedback'i düşer. `/quit`, Ctrl-C ve Ctrl-D üçü de temiz çıkar. (Backend yoksa `USTA_BACKEND` hatası normaldir — o durumda sadece derleme + testle yetin.)

- [ ] **Step 5: Commit**

```bash
git add src/input.rs src/main.rs
git commit -m "main: select-loop — dosya feedback'i Enter beklemez

rustyline ayrı thread'e taşındı (ready el-sıkışması), watcher+debounce
aynı döngüde yarışır. SPEC'in 'proaktif feedback' vaadi artık gerçek.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: Diff + boyut tavanı (`feedback.rs`, TDD)

**Files:**
- Create: `src/feedback.rs`
- Modify: `src/main.rs` (`handle_file_change` + select-loop çağrısı)
- Modify: `Cargo.toml` (`similar = "2"`)

**Interfaces:**
- Produces:
  - `feedback::MAX_FILE_BYTES: usize` (= 65536)
  - `feedback::ChangePayload` enum: `FirstSight(String)` | `Diff(String)` | `TooLarge(usize)` | `Skip`
  - `feedback::FileMemory::new() -> FileMemory`
  - `FileMemory::observe(&mut self, path: &Path, current: String) -> ChangePayload`
- Consumes: Task 3'ün select-loop'u — `handle_file_change` imzası `(&Backend, &mut Session, &mut FileMemory, &Path)` olur.

- [ ] **Step 1: Cargo.toml'a `similar` ekle**

```toml
similar = "2"
```

- [ ] **Step 2: Failing testleri yaz**

`src/feedback.rs`'i test-önce iskeletle oluştur (önce sadece testler + `use`'lar — derlenmeyecek, o fail'imiz):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn first_sight_returns_full_content() {
        let mut m = FileMemory::new();
        match m.observe(Path::new("a.rs"), "fn main() {}".into()) {
            ChangePayload::FirstSight(s) => assert_eq!(s, "fn main() {}"),
            _ => panic!("ilk görüş FirstSight olmalı"),
        }
    }

    #[test]
    fn unchanged_content_is_skipped() {
        let mut m = FileMemory::new();
        let _ = m.observe(Path::new("a.rs"), "ayni".into());
        assert!(matches!(
            m.observe(Path::new("a.rs"), "ayni".into()),
            ChangePayload::Skip
        ));
    }

    #[test]
    fn changed_content_yields_unified_diff() {
        let mut m = FileMemory::new();
        let _ = m.observe(Path::new("a.rs"), "eski satir\n".into());
        match m.observe(Path::new("a.rs"), "yeni satir\n".into()) {
            ChangePayload::Diff(d) => {
                assert!(d.contains("-eski satir"));
                assert!(d.contains("+yeni satir"));
            }
            _ => panic!("değişiklik Diff olmalı"),
        }
    }

    #[test]
    fn oversized_file_warns_once_then_skips() {
        let mut m = FileMemory::new();
        let big = "x".repeat(MAX_FILE_BYTES + 1);
        assert!(matches!(
            m.observe(Path::new("big.rs"), big.clone()),
            ChangePayload::TooLarge(_)
        ));
        assert!(matches!(
            m.observe(Path::new("big.rs"), big),
            ChangePayload::Skip
        ));
    }

    #[test]
    fn diff_is_per_file_not_global() {
        let mut m = FileMemory::new();
        let _ = m.observe(Path::new("a.rs"), "a icerik\n".into());
        // b.rs ilk kez görülüyor — a.rs'nin geçmişiyle diff'lenmemeli.
        assert!(matches!(
            m.observe(Path::new("b.rs"), "b icerik\n".into()),
            ChangePayload::FirstSight(_)
        ));
    }
}
```

- [ ] **Step 3: Fail'i gör**

`src/main.rs`'e `mod feedback;` ekle. Run: `cargo test feedback`
Expected: FAIL — `FileMemory`/`ChangePayload` tanımsız (derleme hatası).

- [ ] **Step 4: Implemente et**

`src/feedback.rs` başına:

```rust
//! Dosya değişiklik yükü: LLM'e ne gideceğine buradaki saf mantık karar verir.
//! İlk görüşte tam içerik (bağlam kurulsun), sonraki kayıtlarda unified diff
//! (token tasarrufu + "ne değişti" sinyali), boyut tavanı üstünde tek seferlik
//! yerel uyarı. IO yok — main okur, biz karar veririz.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use similar::TextDiff;

/// Bu boyutun üstündeki dosyalar LLM'e gönderilmez (context + maliyet koruması).
pub const MAX_FILE_BYTES: usize = 64 * 1024;

/// Bir kayıt olayının LLM'e yansıma biçimi.
pub enum ChangePayload {
    /// Dosya ilk kez görüldü — tam içerik gönderilir.
    FirstSight(String),
    /// Önceki görüşe göre unified diff.
    Diff(String),
    /// Boyut tavanı aşıldı — sadece yerel uyarı, LLM çağrısı yok (dosya başına bir kez).
    TooLarge(usize),
    /// İçerik değişmemiş veya daha önce uyarılmış büyük dosya — sessiz geç.
    Skip,
}

/// Oturum boyunca görülen dosya içeriklerinin hafızası.
pub struct FileMemory {
    seen: HashMap<PathBuf, String>,
    warned_large: HashSet<PathBuf>,
}

impl FileMemory {
    pub fn new() -> Self {
        FileMemory {
            seen: HashMap::new(),
            warned_large: HashSet::new(),
        }
    }

    /// Yeni kaydedilen içeriği gözlemle, LLM yükünü üret, hafızayı güncelle.
    pub fn observe(&mut self, path: &Path, current: String) -> ChangePayload {
        if current.len() > MAX_FILE_BYTES {
            if self.warned_large.insert(path.to_path_buf()) {
                return ChangePayload::TooLarge(current.len());
            }
            return ChangePayload::Skip;
        }
        match self.seen.insert(path.to_path_buf(), current.clone()) {
            None => ChangePayload::FirstSight(current),
            Some(prev) if prev == current => ChangePayload::Skip,
            Some(prev) => {
                let diff = TextDiff::from_lines(&prev, &current)
                    .unified_diff()
                    .context_radius(3)
                    .header("önce", "sonra")
                    .to_string();
                ChangePayload::Diff(diff)
            }
        }
    }
}
```

- [ ] **Step 5: Testlerin geçtiğini gör**

Run: `cargo test feedback`
Expected: 5 test PASS.

- [ ] **Step 6: `handle_file_change`'i bağla**

`src/main.rs` — imza ve gövde:

```rust
/// Kaydedilen dosyayı FileMemory'den geçir; ilk görüşte tam içerik, sonrasında
/// diff olarak sentetik user turn'e çevir → Socratic feedback.
async fn handle_file_change(
    backend: &Backend,
    session: &mut Session,
    files: &mut feedback::FileMemory,
    path: &Path,
) -> Result<()> {
    let contents = std::fs::read_to_string(path)?;
    let injected = match files.observe(path, contents) {
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
    session.push_user(&injected);
    let (reply, web) = backend.complete(&session.system, session.history()).await?;
    print_reply(&reply, web);
    session.push_assistant(reply);
    Ok(())
}
```

Select-loop'ta: `let mut files = feedback::FileMemory::new();` (debouncer'ın yanına) ve çağrı `handle_file_change(&backend, &mut session, &mut files, &path).await`.

- [ ] **Step 7: Test + build**

Run: `cargo test && cargo build`
Expected: hepsi PASS, build temiz.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock src/feedback.rs src/main.rs
git commit -m "feedback: ilk görüşte tam içerik, sonra unified diff, 64KB tavan

Her kayıtta tüm dosyayı LLM'e basmak context'i şişiriyordu ve 'ne
değişti' sinyalini kaybettiriyordu.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: Kapanışta progress yazımı (`progress.rs`, TDD) — hafıza

**Files:**
- Create: `src/progress.rs`
- Modify: `src/main.rs` (çıkış yolunda flush), `src/session.rs` (`#[allow(dead_code)]` kaldır)

**Interfaces:**
- Consumes: `Session { topic, system, history() }`, `Backend::complete`, `anthropic::Message` (`Clone` türetilmiş durumda).
- Produces:
  - `progress::progress_path(project_root: &Path, topic: &str) -> PathBuf` → `<root>/.usta/learner/progress/<topic>.md`
  - `progress::closing_prompt(topic: &str, existing: Option<&str>) -> String`
  - `progress::clean_markdown_reply(reply: &str) -> String` — olası ```-fence'leri soyar
  - `progress::write_atomic(path: &Path, content: &str) -> Result<()>` — tmp + rename
- Davranış sözleşmesi: `/quit`, Ctrl-C, Ctrl-D hepsinde, history boş değilse, progress dosyası oturum bilgisiyle TAM içerik olarak yeniden yazılır. Sonraki oturum bu dosyayı system prompt'a zaten yüklüyor (`brain.rs` — değişiklik gerekmez).

- [ ] **Step 1: Failing testleri yaz**

`src/progress.rs` (önce testler):

```rust
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
```

- [ ] **Step 2: Fail'i gör**

`src/main.rs`'e `mod progress;` ekle. Run: `cargo test progress`
Expected: FAIL (fonksiyonlar tanımsız).

- [ ] **Step 3: Implemente et**

`src/progress.rs` başına:

```rust
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
pub fn closing_prompt(topic: &str, existing: Option<&str>) -> String {
    let current = existing.unwrap_or("(dosya henüz yok)");
    format!(
        "[OTURUM KAPANIYOR — PROGRESS GÜNCELLEME]\n\
         Görev: `.usta/learner/progress/{topic}.md` dosyasının YENİ TAM içeriğini üret.\n\n\
         Mevcut dosya:\n---\n{current}\n---\n\n\
         Kurallar:\n\
         - Bu oturumdaki konuşmaya ve dosya feedback'lerine göre güncelle.\n\
         - Yapı: `# {topic} — İlerleme` başlığı + `Seviye` / `Kapatılanlar` / `Gap'ler` maddeleri.\n\
         - Gap'leri KANITLA yaz (hangi kodda/konuşmada görüldü).\n\
         - Oturumda kanıtı olmayan hiçbir şeyi ekleme, mevcut dosyadaki hâlâ geçerli bilgiyi koru.\n\
         - SADECE dosya içeriğini döndür — açıklama, selamlama, kod bloğu işareti yok."
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
```

- [ ] **Step 4: Testlerin geçtiğini gör**

Run: `cargo test progress`
Expected: 6 test PASS.

- [ ] **Step 5: Çıkış yoluna flush'ı bağla**

`src/session.rs`: `topic` alanındaki `#[allow(dead_code)]` satırını ve üstündeki "şimdilik oturum kimliği" notunu sil (doc şu olur: `/// Aktif öğrenme başlığı (ör. "rust") — kapanışta progress dosyasını seçer.`).

`src/main.rs` — `use crate::anthropic::Message;` ekle; döngüden sonra, `println!("Görüşürüz...")` satırından ÖNCE:

```rust
    if let Err(e) = flush_progress(&backend, &session, &project_root).await {
        eprintln!("(progress güncellenemedi: {e})");
    }
```

ve fonksiyonu ekle:

```rust
/// Oturum kapanışında progress dosyasını LLM'e tam-içerik yeniden yazdır.
/// Boş oturumda (hiç turn yok) dosyaya dokunma.
async fn flush_progress(backend: &Backend, session: &Session, project_root: &Path) -> Result<()> {
    if session.history().is_empty() {
        return Ok(());
    }
    println!("(oturum özetleniyor — progress yazılıyor…)");
    let path = progress::progress_path(project_root, &session.topic);
    let existing = std::fs::read_to_string(&path).ok();
    let mut history = session.history().to_vec();
    history.push(Message::user(progress::closing_prompt(
        &session.topic,
        existing.as_deref(),
    )));
    let (reply, _) = backend.complete(&session.system, &history).await?;
    let content = progress::clean_markdown_reply(&reply);
    if content.is_empty() {
        anyhow::bail!("model boş içerik döndürdü — dosya yazılmadı");
    }
    progress::write_atomic(&path, &content)?;
    println!("(progress güncellendi: {})", path.display());
    Ok(())
}
```

- [ ] **Step 6: Test + build**

Run: `cargo test && cargo build`
Expected: hepsi PASS, build temiz (dead_code uyarısı kalmadı).

- [ ] **Step 7: Commit**

```bash
git add src/progress.rs src/main.rs src/session.rs
git commit -m "progress: kapanışta oturum özeti progress/<konu>.md'ye yazılır

Usta'nın hafızası artık gerçek: SPEC §9 (oturumlar arası hatırlama)
gerçeklendi. Atomik yazım (tmp+rename), boş oturumda dokunmaz.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: CLI backend `--resume` — oturum sürdürme

**Files:**
- Modify: `src/backend.rs`
- Modify: `src/main.rs` (`let mut backend`, `&mut` geçişler)

**Interfaces:**
- Produces:
  - `Backend::Cli { model: String, session_id: Option<String> }`
  - `Backend::complete(&mut self, system: &str, history: &[Message]) -> Result<(String, bool)>` (imza `&mut self` olur — TÜM çağrı yerleri güncellenir)
  - `backend::parse_cli_output(stdout: &str) -> (String, Option<String>)` — saf, test edilebilir
- Davranış sözleşmesi: ilk CLI çağrısı tam transcript + `--output-format json` → `session_id` saklanır. Sonraki çağrılar SADECE son user mesajını `--resume <id>` ile yollar. Resume çağrısı hata verirse bir kez tam-transcript'e düşülür (stale session toparlama), `session_id` sıfırlanır.

- [ ] **Step 1: Failing testleri yaz**

`src/backend.rs` test modülüne ekle:

```rust
#[test]
fn parse_cli_output_reads_json_result_and_session() {
    let out = r#"{"type":"result","result":"merhaba","session_id":"abc-123","is_error":false}"#;
    let (text, sid) = parse_cli_output(out);
    assert_eq!(text, "merhaba");
    assert_eq!(sid, Some("abc-123".to_string()));
}

#[test]
fn parse_cli_output_falls_back_to_plain_text() {
    let (text, sid) = parse_cli_output("  düz metin yanıt  ");
    assert_eq!(text, "düz metin yanıt");
    assert_eq!(sid, None);
}

#[test]
fn last_user_text_takes_final_user_message() {
    let history = vec![
        Message::user("ilk"),
        Message {
            role: "assistant".into(),
            content: serde_json::Value::String("yanıt".into()),
        },
        Message::user("son soru"),
    ];
    assert_eq!(last_user_text(&history), "son soru");
}

#[test]
fn last_user_text_empty_history_is_empty() {
    assert_eq!(last_user_text(&[]), "");
}
```

- [ ] **Step 2: Fail'i gör**

Run: `cargo test backend`
Expected: FAIL (`parse_cli_output`, `last_user_text` tanımsız; `Cli` varyantında `session_id` yok).

- [ ] **Step 3: Implemente et**

`src/backend.rs` değişiklikleri:

```rust
/// Kullanılabilir LLM backend'leri.
pub enum Backend {
    /// Yerel `claude` CLI'a shell'ler — Claude Code auth'u, key yok.
    /// `session_id`: ilk yanıttan yakalanır, sonraki turn'ler `--resume` ile
    /// sürdürülür → tam transcript her seferinde yeniden gönderilmez.
    Cli {
        model: String,
        session_id: Option<String>,
    },
    /// Anthropic Messages API — reqwest, key gerektirir.
    Api {
        client: anthropic::Client,
        model: String,
    },
}
```

`cli_backend()`:

```rust
fn cli_backend() -> Backend {
    Backend::Cli {
        model: DEFAULT_CLI_MODEL.to_string(),
        session_id: None,
    }
}
```

`complete` — `&mut self`, Cli kolunda resume mantığı + stale-session fallback:

```rust
impl Backend {
    /// Seçilen backend'e göre tamamlama iste. `(metin, web_arandı_mı)` döner.
    /// CLI modunda web kullanımı metinden tespit edilemez → `false`.
    pub async fn complete(&mut self, system: &str, history: &[Message]) -> Result<(String, bool)> {
        match self {
            Backend::Api { client, model } => client.complete(model, system, history).await,
            Backend::Cli { model, session_id } => {
                let resume = session_id.clone();
                let input = match &resume {
                    Some(_) => last_user_text(history),
                    None => render_transcript(history),
                };
                let attempt = run_claude_cli(model, system, &input, resume.as_deref()).await;
                let (text, new_sid) = match attempt {
                    Ok(v) => v,
                    // Stale/silinmiş oturum — bir kez tam transcript'le baştan dene.
                    Err(_) if resume.is_some() => {
                        *session_id = None;
                        run_claude_cli(model, system, &render_transcript(history), None).await?
                    }
                    Err(e) => return Err(e),
                };
                if new_sid.is_some() {
                    *session_id = new_sid;
                }
                Ok((text, false))
            }
        }
    }
}

/// History'deki SON user mesajının düz metnini döndür — resume çağrısında
/// sunucu taraflı oturum bağlamı zaten var, sadece yeni turn gönderilir.
fn last_user_text(history: &[Message]) -> String {
    history
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| match &m.content {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .unwrap_or_default()
}

/// `claude -p --output-format json` çıktısını ayrıştır. JSON değilse (eski
/// sürüm / beklenmedik çıktı) ham metne düş — session id'siz devam edilir.
pub fn parse_cli_output(stdout: &str) -> (String, Option<String>) {
    #[derive(serde::Deserialize)]
    struct CliJson {
        result: Option<String>,
        session_id: Option<String>,
    }
    match serde_json::from_str::<CliJson>(stdout) {
        Ok(j) => (j.result.unwrap_or_default(), j.session_id),
        Err(_) => (stdout.trim().to_string(), None),
    }
}
```

`run_claude_cli` — imza + arg kurulumu değişir, gövdenin stdin/stdout kısmı aynı kalır:

```rust
/// `claude -p` alt sürecini çalıştır: girdi stdin'e yazılır, JSON çıktı okunur.
/// `resume` verilirse `--resume <id>` ile sunucu taraflı oturum sürdürülür.
async fn run_claude_cli(
    model: &str,
    system: &str,
    input: &str,
    resume: Option<&str>,
) -> Result<(String, Option<String>)> {
    let mut cmd = Command::new("claude");
    cmd.arg("-p")
        .arg("--output-format")
        .arg("json")
        .arg("--append-system-prompt")
        .arg(system)
        .arg("--model")
        .arg(model)
        .arg("--allowedTools")
        .arg("WebSearch");
    if let Some(id) = resume {
        cmd.arg("--resume").arg(id);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("`claude` CLI başlatılamadı — PATH'te mi?")?;
```

(devamı mevcut kodla aynı: stdin'e `input` yaz + shutdown, `wait_with_output`, başarısızsa stderr ile `bail!`; son satır değişir:)

```rust
    Ok(parse_cli_output(&String::from_utf8_lossy(&output.stdout)))
```

- [ ] **Step 4: Çağrı yerlerini `&mut`'a geçir**

`src/main.rs`:
- `let backend = backend::select()?;` → `let mut backend = backend::select()?;`
- select-loop'taki `backend.complete(...)` çağrısı olduğu gibi çalışır (backend zaten yerel mut).
- `handle_file_change(backend: &Backend, ...)` → `backend: &mut Backend`; çağrı `handle_file_change(&mut backend, &mut session, &mut files, &path)`.
- `flush_progress(backend: &Backend, ...)` → `backend: &mut Backend`; çağrı `flush_progress(&mut backend, &session, &project_root)`.

- [ ] **Step 5: Test + build**

Run: `cargo test && cargo build`
Expected: yeni 4 test dahil hepsi PASS, build temiz.

- [ ] **Step 6: Commit**

```bash
git add src/backend.rs src/main.rs
git commit -m "backend: claude CLI --resume — turn başına tam transcript bitti

İlk çağrı --output-format json'dan session_id yakalar; sonraki turn'ler
sadece yeni mesajı gönderir. Stale oturumda tam transcript'e düşer.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 7: SPEC + dokümantasyon güncellemesi

**Files:**
- Modify: `SPEC.md`

**Interfaces:** yok (yalnız doküman).

- [ ] **Step 1: SPEC.md'yi v0.2 kararlarıyla güncelle**

Şu üç düzenlemeyi yap:

1. §6'daki `- **Çağrı:** non-streaming ...` satırını şununla değiştir:

```markdown
- **Çağrı:** non-streaming (raw reqwest'te client timeout yok → sağlam). Streaming sonraki sürüm. CLI backend oturumu `--resume <session_id>` ile sürdürür — ilk çağrı `--output-format json`'dan id yakalar, sonraki turn'ler yalnız yeni mesajı gönderir (stale oturumda tam transcript'e düşülür).
```

2. §9'daki `- **Kalıcı.** progress/ seviyeyi tutar, oturumlar arası hatırlar → tekrar anlatmaz.` satırını şununla değiştir:

```markdown
- **Kalıcı (v0.2'de gerçeklendi).** Oturum kapanışında (`/quit`, Ctrl-C, Ctrl-D) Usta oturumu özetleyip `.usta/learner/progress/<konu>.md`'yi tam içerik olarak yeniden yazar (atomik: tmp+rename). Sonraki oturum bu dosyayı system prompt'a yükler → tekrar anlatmaz. Boş oturum dosyaya dokunmaz.
```

3. §11'deki `- Dosya izleme granülaritesi: her kayıtta mı, debounce mı, kullanıcı tetikli mi.` satırını sil; §11'in ÜSTÜNE yeni bölüm ekle:

```markdown
## 11. Alınan Kararlar (v0.2)

- **Dosya izleme granülaritesi:** 1 sn debounce (son kayıttan itibaren). İlk görüşte tam içerik, sonraki kayıtlarda unified diff, 64KB üstü dosya izleme dışı (tek seferlik yerel uyarı).
- **Proaktiflik:** girdi ayrı thread'de (rustyline + ready el-sıkışması), ana döngü `tokio::select!` — feedback için Enter beklenmez.
```

(eski `## 11. Açık Karar Noktaları` başlığı `## 12. Açık Karar Noktaları` olur, kalan maddeleri korunur.)

- [ ] **Step 2: Commit + push**

```bash
git add SPEC.md
git commit -m "SPEC: v0.2 kararları — hafıza gerçeklendi, debounce/diff/resume

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Bitiş Doğrulaması (tüm görevler sonrası)

- [ ] `cargo test` — tamamı PASS
- [ ] `cargo build` — uyarısız
- [ ] Manuel duman testi (backend varsa): oturum aç → bir dosya kaydet → Enter'sız feedback gelsin → `/quit` → `.usta/learner/progress/<konu>.md` oluşmuş/güncellenmiş olsun → yeni oturum aç → Usta önceki seviyeyi bildiğini göstersin.
