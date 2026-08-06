# Usta v0.4 — Yönetim Komutları Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Öğrenme kayıtlarını görünür ve yönetilebilir yap: global katalog otomatik güncellenir (`## Kayıtlar` upsert), `usta topics` listeler, `usta reset <konu>` tek konuyu siler, `usta reset --factory` her şeyi (bilinen tüm proje `.usta/`'ları + global brain) onaylı sıfırlar.

**Architecture:** Katalog `~/.config/usta/learner/index.md` sonundaki `## Kayıtlar` bölümüdür — satır formatı `- konu | proje-yolu | YYYY-MM-DD`. Saf mantık yeni `src/index.rs`'te (parse/upsert/remove/render), kapanış flush'ı `index::record` ile upsert eder. Argüman ayrıştırma `parse_command` enum'una refactor edilir (mevcut ad-hoc `is_init`/`explicit_topic` gider). Reset komutları backend seçiminden ÖNCE koşar — LLM gerektirmez. Yan etki: index zaten system prompt'a yükleniyor (brain.rs) → Usta tüm başlıkları görür; izolasyon bozulmaz (progress yalnız aktif konudan yüklenir).

**Tech Stack:** v0.3 sonrası mevcut yığın + `chrono = "0.4"` (yeni — katalog tarihi).

## Global Constraints

- **ÖN KOŞUL: v0.2 (`2026-08-07-usta-v02-hafiza-proaktiflik.md`) VE v0.3 (`2026-08-07-usta-v03-pedagoji-katmani.md`) planları TAMAMEN uygulanmış ve commit'lenmiş olmalı.** Bu plan `progress::write_atomic`, `flush_progress` ve v0.3 sonrası `main.rs` akışının üstüne kurulur. Bitmemişse DUR ve bildir.
- Bu repo kendi git'ine sahip (`usta/` içinde çalış, headspace repo'suna commit atma).
- Tüm kod yorumları, kullanıcıya görünen mesajlar ve doc-comment'ler **Türkçe**. Modül başları `//!` doc taşır.
- Commit başlık deseni: `<scope>: kısa türkçe özet`.
- Her görev sonunda `cargo test` ve `cargo build` temiz olmalı (uyarı çıkarsa düzelt).
- Test isimleri `snake_case`, davranışı cümle gibi anlatır.
- Saf mantık test edilebilir fonksiyonda; IO/async kabukta.
- Commit mesajı sonuna ekle: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`

## Dosya Haritası

| Dosya | Sorumluluk | Değişim |
|---|---|---|
| `src/index.rs` | **YENİ** — katalog parse/upsert/remove/render + `record` IO | oluşturulur |
| `src/main.rs` | `parse_command` refactor, `run_topics`/`run_reset_*` handler'ları, flush'a `record` bağlanır | güncellenir |
| `Cargo.toml` | `chrono = "0.4"` | güncellenir |
| `SPEC.md` | §4.7 Yönetim Komutları + v0.4 kararları | güncellenir |

---

### Task 1: `index.rs` çekirdeği + kapanışta katalog kaydı (TDD)

**Files:**
- Create: `src/index.rs`
- Modify: `src/main.rs` (`mod index;` + `flush_progress`'e record + `today()`)
- Modify: `Cargo.toml` (chrono)

**Interfaces:**
- Consumes: `progress::write_atomic` (v0.2), `config::global_root`, `flush_progress` (v0.2).
- Produces:
  - `index::IndexEntry { topic: String, project: PathBuf, date: String }` (`Debug, PartialEq` türetilmiş)
  - `index::entries(content: &str) -> Vec<IndexEntry>`
  - `index::upsert(content: &str, topic: &str, project: &Path, date: &str) -> String`
  - `index::record(global: &Path, topic: &str, project: &Path, date: &str) -> Result<()>`
  - (`index::remove` Task 3'te eklenir — burada EKLEME, dead-code uyarısı olur)

- [x] **Step 1: Cargo.toml'a chrono ekle**

```toml
chrono = "0.4"
```

- [x] **Step 2: Failing testleri yaz**

`src/index.rs`'i test modülüyle oluştur, `src/main.rs`'e `mod index;` ekle:

```rust
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
}
```

- [x] **Step 3: Fail'i gör**

Run: `cargo test index`
Expected: FAIL (fonksiyonlar tanımsız — derleme hatası).

- [x] **Step 4: Implemente et**

`src/index.rs` gövdesi:

```rust
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

/// Kapanışta çağrılır: kataloğu oku → upsert → atomik yaz.
pub fn record(global: &Path, topic: &str, project: &Path, date: &str) -> Result<()> {
    let path = global.join("learner/index.md");
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    let updated = upsert(&current, topic, project, date);
    crate::progress::write_atomic(&path, &updated)
}
```

- [x] **Step 5: Testlerin geçtiğini gör**

Run: `cargo test index`
Expected: 6 test PASS.

- [x] **Step 6: Kapanış flush'ına bağla**

`src/main.rs` — `flush_progress` içinde, `println!("(progress güncellendi: ...)")` satırından SONRA:

```rust
    // Global kataloğu güncelle — başarısızlık progress yazımını geri almaz,
    // sadece not düşülür (katalog konfor katmanı, hafızanın kendisi değil).
    match config::global_root() {
        Ok(global) => {
            if let Err(e) = index::record(&global, &session.topic, project_root, &today()) {
                eprintln!("(katalog güncellenemedi: {e})");
            }
        }
        Err(e) => eprintln!("(katalog güncellenemedi: {e})"),
    }
```

ve yardımcıyı ekle:

```rust
/// Bugünün yerel tarihi — katalog satırlarının tarih alanı.
fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}
```

- [x] **Step 7: Test + build**

Run: `cargo test && cargo build`
Expected: hepsi PASS, uyarı yok (`record` flush'tan çağrılıyor — dead code yok).

- [x] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock src/index.rs src/main.rs
git commit -m "index: global katalog — kapanışta konu|proje|tarih upsert

'Nerede ne öğreniyorum' tek bakışta; topics/reset komutlarının veri
kaynağı. Bölüm index.md sonunda, üstündeki serbest metin korunur.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: `parse_command` refactor + `usta topics` (TDD)

**Files:**
- Modify: `src/main.rs` (`Command` enum, `parse_command`, `run_topics`, dispatch; `is_init`/`explicit_topic` silinir, `resolve_topic` imzası değişir)

**Interfaces:**
- Produces:
  - `Command` enum (`Debug, PartialEq`): `Init` | `Topics` | `Start(Option<String>)` — Task 3/4 `Reset` varyantını ekleyecek.
  - `parse_command(args: &[String]) -> Result<Command>`
  - `resolve_topic(topic_arg: Option<String>) -> Result<String>` (eski `args` parametresi yerine)
- Davranış değişikliği (bilinçli): `usta <bilinmeyen>` artık net hata verir (eskiden sessizce konu promptuna düşüyordu).

- [x] **Step 1: Failing testleri yaz**

`src/main.rs` test modülünde `explicit_topic_*` ve `is_init_*` testlerini SİL, yerlerine:

```rust
#[test]
fn parse_bare_is_start_without_topic() {
    let args = vec!["usta".to_string()];
    assert_eq!(parse_command(&args).unwrap(), Command::Start(None));
}

#[test]
fn parse_start_keeps_topic_arg() {
    let args = vec!["usta".into(), "start".into(), "javascript".into()];
    assert_eq!(
        parse_command(&args).unwrap(),
        Command::Start(Some("javascript".to_string()))
    );
}

#[test]
fn parse_start_without_arg_is_start_none() {
    let args = vec!["usta".into(), "start".into()];
    assert_eq!(parse_command(&args).unwrap(), Command::Start(None));
}

#[test]
fn parse_init_and_topics() {
    assert_eq!(
        parse_command(&["usta".into(), "init".into()]).unwrap(),
        Command::Init
    );
    assert_eq!(
        parse_command(&["usta".into(), "topics".into()]).unwrap(),
        Command::Topics
    );
}

#[test]
fn parse_unknown_command_errors() {
    assert!(parse_command(&["usta".into(), "rust".into()]).is_err());
}
```

- [x] **Step 2: Fail'i gör**

Run: `cargo test parse`
Expected: FAIL (`Command`/`parse_command` tanımsız).

- [x] **Step 3: Implemente et**

`src/main.rs`'e ekle; `is_init` ve `explicit_topic` fonksiyonlarını sil:

```rust
/// Komut satırı komutu — argüman ayrıştırma tek yerde, saf ve test edilebilir.
#[derive(Debug, PartialEq)]
pub enum Command {
    /// `usta init` — iskelet kur, per-dosya durum yazdır.
    Init,
    /// `usta topics` — global katalogdan konu listesi.
    Topics,
    /// `usta` / `usta start [konu]` — öğrenme oturumu.
    Start(Option<String>),
}

/// Argümanları komuta çevir. Bilinmeyen komut net hata — sessiz sürpriz yok.
pub fn parse_command(args: &[String]) -> Result<Command> {
    let mut rest = args.iter().skip(1);
    match rest.next().map(String::as_str) {
        None => Ok(Command::Start(None)),
        Some("start") => Ok(Command::Start(rest.next().cloned())),
        Some("init") => Ok(Command::Init),
        Some("topics") => Ok(Command::Topics),
        Some(other) => anyhow::bail!(
            "bilinmeyen komut: '{other}'. Komutlar: start [konu], init, topics"
        ),
    }
}
```

`main` başındaki dispatch'i değiştir (`if is_init(&args) { return run_init(); }` yerine):

```rust
    let args: Vec<String> = std::env::args().collect();
    let topic_arg = match parse_command(&args)? {
        Command::Init => return run_init(),
        Command::Topics => return run_topics(),
        Command::Start(t) => t,
    };
```

`resolve_topic` imzası (gövde mantığı aynı, sadece kaynak değişir):

```rust
fn resolve_topic(topic_arg: Option<String>) -> Result<String> {
    if let Some(raw) = topic_arg {
        return Ok(slugify_topic(&raw));
    }
    if !std::io::stdin().is_terminal() {
        return Ok("genel".to_string());
    }
    let mut rl = DefaultEditor::new()?;
    match rl.readline("Ne öğreneceksin/yapacaksın? (ör. rust, javascript): ") {
        Ok(line) => Ok(slugify_topic(&line)),
        Err(_) => Ok("genel".to_string()),
    }
}
```

Çağrı: `let topic = resolve_topic(topic_arg)?;`

`run_topics` ekle:

```rust
/// `usta topics` — global katalogdaki kayıtları listele. LLM gerekmez.
fn run_topics() -> Result<()> {
    let global = config::global_root()?;
    let content =
        std::fs::read_to_string(global.join("learner/index.md")).unwrap_or_default();
    let list = index::entries(&content);
    if list.is_empty() {
        println!("Kayıtlı konu yok — 'usta start <konu>' ile başla.");
        return Ok(());
    }
    println!("Konu | Proje | Son oturum");
    for e in list {
        println!("{} | {} | {}", e.topic, e.project.display(), e.date);
    }
    Ok(())
}
```

- [x] **Step 4: Test + build**

Run: `cargo test && cargo build`
Expected: yeni 5 test dahil hepsi PASS; `is_init`/`explicit_topic` kalıntısı yok, uyarı yok.

- [x] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "main: parse_command enum'u + usta topics

Ad-hoc is_init/explicit_topic gitti; bilinmeyen komut artık net hata.
topics komutu backend'siz çalışır — katalogdan okur.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: `usta reset <konu>` (TDD)

**Files:**
- Modify: `src/main.rs` (`Command::Reset` + `ResetTarget` + `confirm` + `run_reset_topic`)
- Modify: `src/index.rs` (`remove` eklenir)

**Interfaces:**
- Produces:
  - `ResetTarget` enum (`Debug, PartialEq`): `Topic(String)` — Task 4 `Factory` ekleyecek.
  - `Command::Reset(ResetTarget)` varyantı.
  - `index::remove(content: &str, topic: &str, project: &Path) -> String`
  - `confirm(prompt: &str, yes: &[&str]) -> Result<bool>` — stdin'den okur; boş/kapalı stdin = hayır (güvenli varsayılan). Task 4 da kullanır.
- Davranış: konu parse'ta slug'lanır (`"C++"` → `c`) — progress dosya adıyla aynı kural.

- [x] **Step 1: Failing testleri yaz**

`src/index.rs` testlerine ekle:

```rust
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
```

`src/main.rs` testlerine ekle:

```rust
#[test]
fn parse_reset_topic_is_slugified() {
    let args = vec!["usta".into(), "reset".into(), "C++".into()];
    assert_eq!(
        parse_command(&args).unwrap(),
        Command::Reset(ResetTarget::Topic("c".to_string()))
    );
}

#[test]
fn parse_reset_without_arg_errors() {
    assert!(parse_command(&["usta".into(), "reset".into()]).is_err());
}
```

- [x] **Step 2: Fail'i gör**

Run: `cargo test 'reset'`
Expected: FAIL (`ResetTarget`/`remove` tanımsız).

- [x] **Step 3: Implemente et**

`src/index.rs`'e ekle:

```rust
/// (konu, proje) satırını düş — eşleşme yoksa kayıtlar değişmeden kalır.
pub fn remove(content: &str, topic: &str, project: &Path) -> String {
    let list: Vec<IndexEntry> = entries(content)
        .into_iter()
        .filter(|e| !(e.topic == topic && e.project == project))
        .collect();
    render(content, &list)
}
```

`src/main.rs` — enum'ları genişlet:

```rust
/// Reset kapsamı.
#[derive(Debug, PartialEq)]
pub enum ResetTarget {
    /// Bulunduğun projede tek konunun progress'i.
    Topic(String),
}
```

`Command`'a varyant ekle: `Reset(ResetTarget),` — ve `parse_command`'a kol ekle (`Some("topics")` kolundan sonra):

```rust
        Some("reset") => match rest.next().map(String::as_str) {
            Some(topic) => Ok(Command::Reset(ResetTarget::Topic(slugify_topic(topic)))),
            None => anyhow::bail!("kullanım: usta reset <konu>"),
        },
```

Dispatch'e kol ekle:

```rust
        Command::Reset(ResetTarget::Topic(t)) => return run_reset_topic(&t),
```

Handler + onay yardımcısı:

```rust
/// `usta reset <konu>` — bulunduğun projenin o konudaki progress'ini sil
/// (onaylı) ve global katalogdan düş. LLM gerekmez.
fn run_reset_topic(topic: &str) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let Some(root) = config::find_project_root(&cwd) else {
        anyhow::bail!("bu dizinde (veya üstünde) .usta yok — resetlenecek proje bulunamadı");
    };
    let path = progress::progress_path(&root, topic);
    if !path.is_file() {
        println!("kayıt yok: {}", path.display());
        return Ok(());
    }
    if !confirm(&format!("{} silinecek. Emin misin? [e/H] ", path.display()), &["e", "evet"])? {
        println!("vazgeçildi.");
        return Ok(());
    }
    std::fs::remove_file(&path)
        .with_context(|| format!("silinemedi: {}", path.display()))?;
    println!("silindi: {}", path.display());

    // Katalogdan da düş — katalog yoksa/okunamıyorsa sessizce geç.
    let global = config::global_root()?;
    let index_path = global.join("learner/index.md");
    if let Ok(current) = std::fs::read_to_string(&index_path) {
        let updated = index::remove(&current, topic, &root);
        progress::write_atomic(&index_path, &updated)?;
    }
    Ok(())
}

/// Onay iste: stdin'den tek satır oku, kabul listesiyle (küçük harf)
/// karşılaştır. Stdin kapalı/boş = hayır — güvenli varsayılan.
fn confirm(prompt: &str, yes: &[&str]) -> Result<bool> {
    use std::io::Write;
    print!("{prompt}");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(yes.contains(&line.trim().to_lowercase().as_str()))
}
```

- [x] **Step 4: Test + build**

Run: `cargo test && cargo build`
Expected: yeni 4 test dahil hepsi PASS.

- [x] **Step 5: Commit**

```bash
git add src/main.rs src/index.rs
git commit -m "reset: usta reset <konu> — onaylı progress silme + katalog düşümü

'Bu konuyu baştan öğreneyim' senaryosu; stdin kapalıysa hayır sayılır.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: `usta reset --factory` — tam fabrika sıfırlaması

**Files:**
- Modify: `src/main.rs` (`ResetTarget::Factory` + `run_reset_factory`)

**Interfaces:**
- Consumes: `index::entries`, `config::global_root`, `confirm` (Task 3).
- Davranış sözleşmesi: katalogdaki TÜM projelerin `.usta/` dizinleri + global brain silinir. Liste ÖNCE gösterilir; onay kelime bazlıdır ("evet" yazılmalı — [e/H]'den bilinçli olarak daha sert). Katalogda olmayan eski projeler kapsam dışıdır — uyarı + `find` komutu basılır.

- [x] **Step 1: Parse testini yaz**

`src/main.rs` testlerine:

```rust
#[test]
fn parse_reset_factory_flag() {
    let args = vec!["usta".into(), "reset".into(), "--factory".into()];
    assert_eq!(
        parse_command(&args).unwrap(),
        Command::Reset(ResetTarget::Factory)
    );
}
```

Run: `cargo test parse_reset_factory`
Expected: FAIL (`Factory` varyantı yok).

- [x] **Step 2: Implemente et**

`ResetTarget`'a varyant ekle:

```rust
    /// Bilinen tüm proje `.usta/`'ları + global brain — sıfır nokta.
    Factory,
```

`parse_command`'ın reset kolunu güncelle (`--factory` slug'lanmadan ÖNCE yakalanmalı):

```rust
        Some("reset") => match rest.next().map(String::as_str) {
            Some("--factory") => Ok(Command::Reset(ResetTarget::Factory)),
            Some(topic) => Ok(Command::Reset(ResetTarget::Topic(slugify_topic(topic)))),
            None => anyhow::bail!("kullanım: usta reset <konu>  veya  usta reset --factory"),
        },
```

Dispatch kolu:

```rust
        Command::Reset(ResetTarget::Factory) => return run_reset_factory(),
```

Handler:

```rust
/// `usta reset --factory` — katalogdaki tüm projelerin `.usta/`'sı + global
/// brain silinir. Sonraki `usta` çalıştırması her şeyi varsayılanlardan
/// baştan kurar (bootstrap) — Usta kullanıcıyı hiç tanımamış gibi başlar.
fn run_reset_factory() -> Result<()> {
    let global = config::global_root()?;
    let index_content =
        std::fs::read_to_string(global.join("learner/index.md")).unwrap_or_default();
    let mut targets: Vec<PathBuf> = index::entries(&index_content)
        .into_iter()
        .map(|e| e.project.join(".usta"))
        .filter(|p| p.is_dir())
        .collect();
    targets.sort();
    targets.dedup();

    println!("FABRİKA SIFIRLAMASI — silinecekler:");
    for t in &targets {
        println!("  {}", t.display());
    }
    println!("  {} (global brain)", global.display());
    println!("Not: katalogda olmayan eski projeler listede DEĞİL.");
    println!("Kontrol: find ~ -maxdepth 5 -name .usta -type d");

    if !confirm("Hepsi kalıcı silinecek. Onay için 'evet' yaz: ", &["evet"])? {
        println!("vazgeçildi.");
        return Ok(());
    }
    for t in &targets {
        std::fs::remove_dir_all(t)
            .with_context(|| format!("silinemedi: {}", t.display()))?;
        println!("silindi: {}", t.display());
    }
    if global.is_dir() {
        std::fs::remove_dir_all(&global)
            .with_context(|| format!("silinemedi: {}", global.display()))?;
        println!("silindi: {}", global.display());
    }
    println!("Sıfır nokta. Sonraki 'usta' çalıştırması her şeyi baştan kurar.");
    Ok(())
}
```

- [x] **Step 3: Test + build**

Run: `cargo test && cargo build`
Expected: hepsi PASS, uyarı yok.

- [x] **Step 4: Manuel duman testi (sandbox'ta!)** — NOT çalıştırılmadı (güvenlik kısıtı gereği); insan final doğrulamasına ertelendi.

```bash
export XDG_CONFIG_HOME=$(mktemp -d)/config
PROJ=$(mktemp -d)/deneme && mkdir -p $PROJ && cd $PROJ
echo "" | cargo run --manifest-path <usta-repo>/Cargo.toml -- start deneme  # scaffold kurulsun (backend hatası normal)
# Katalog satırını elle ekle (flush LLM ister — sandbox'ta elle simüle):
mkdir -p $XDG_CONFIG_HOME/usta/learner
printf '## Kayıtlar\n- deneme | %s | 2026-08-07\n' "$PROJ" > $XDG_CONFIG_HOME/usta/learner/index.md
cargo run --manifest-path <usta-repo>/Cargo.toml -- topics          # satırı listelemeli
echo "evet" | cargo run --manifest-path <usta-repo>/Cargo.toml -- reset --factory
# Beklenen: $PROJ/.usta ve $XDG_CONFIG_HOME/usta silinmiş.
```

**DİKKAT: `XDG_CONFIG_HOME` set etmeden factory reset'i ASLA deneme — gerçek `~/.config/usta` gider.**

- [x] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "reset: --factory — bilinen tüm .usta'lar + global brain, kelime onaylı

Liste önce gösterilir, 'evet' yazılmadan silinmez; katalog-dışı eski
projeler için find ipucu basılır.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: SPEC v0.4 güncellemesi

**Files:**
- Modify: `SPEC.md`

**Interfaces:** yok (yalnız doküman).

- [ ] **Step 1: §4.6'dan sonra yeni bölüm ekle**

`## 4.6 Pedagoji Katmanı (v0.3)` bölümünün sonuna (bir sonraki `## 5` başlığından önce):

```markdown
## 4.7 Yönetim Komutları (v0.4)

- **`usta topics`** — global katalog listelenir: `konu | proje | son oturum`. LLM gerekmez.
- **`usta reset <konu>`** — bulunduğun projenin o konudaki progress'i silinir (`[e/H]` onaylı), katalogdan düşülür. "Bu konuyu baştan öğreneyim" senaryosu.
- **`usta reset --factory`** — katalogdaki TÜM projelerin `.usta/`'sı + global brain silinir; liste önce gösterilir, onay için "evet" yazılır. Katalogda olmayan eski projeler kapsam dışı (uyarı + `find` ipucu basılır).
- **Katalog otomatik güncellenir:** kapanış flush'ı `learner/index.md` sonundaki `## Kayıtlar` bölümüne `- konu | proje-yolu | YYYY-MM-DD` upsert eder. Yan etki: index system prompt'ta olduğundan Usta tüm başlıklardan haberdardır — izolasyon bozulmaz (progress yalnız aktif konudan yüklenir).
```

- [ ] **Step 2: §7 dosya yapısında index.md açıklamasını güncelle**

`index.md             # TÜM öğrenme başlıkları kataloğu (rust: AKTİF, js: AKTİF, marketing: duraklamış)` satırını şununla değiştir:

```
    index.md             # TÜM öğrenme başlıkları kataloğu — "## Kayıtlar" bölümü kapanışta otomatik upsert edilir (v0.4)
```

- [ ] **Step 3: "Alınan Kararlar" bölümüne ekle**

`## 11. Alınan Kararlar` sonuna:

```markdown
- **Katalog formatı (v0.4):** `learner/index.md` sonunda `## Kayıtlar`; satır `- konu | proje-yolu | YYYY-MM-DD`; bölüm-üstü serbest metin korunur; tarih `chrono` ile yerel saat.
- **Reset onayları (v0.4):** konu reseti `[e/H]`, factory reset kelime onayı ("evet"); stdin kapalı/boş = hayır (güvenli varsayılan). Reset komutları backend'siz çalışır.
```

- [ ] **Step 4: Commit + push**

```bash
git add SPEC.md
git commit -m "SPEC: v0.4 yönetim komutları — topics, reset, katalog upsert

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Bitiş Doğrulaması (tüm görevler sonrası)

- [ ] `cargo test` — tamamı PASS
- [ ] `cargo build` — uyarısız
- [ ] Sandbox duman testi (`XDG_CONFIG_HOME` set edilmiş halde):
  1. Backend varsa: oturum aç-kapa → `usta topics` yeni satırı göstersin (`konu | proje | bugünün tarihi`).
  2. `usta reset <konu>` → `[e/H]` sorsun, "e" ile progress dosyası silinsin, `topics`'ten düşsün.
  3. `usta reset --factory` → listeyi bassın, "evet" ile sandbox'taki tüm `.usta` + global gitsin; sonraki `usta` sıfırdan kursun.
  4. `usta bilinmeyenkomut` → net hata mesajı.
- [ ] **Güvenlik kontrolü:** hiçbir adımda gerçek `~/.config/usta`'ya dokunulmadı (tüm manuel testler `XDG_CONFIG_HOME` sandbox'ıyla).
