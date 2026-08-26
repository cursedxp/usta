# Polite Watcher Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** v0.23.0 üstüne. Spec: `docs/superpowers/specs/2026-08-26-polite-watcher-design.md` — önce oku, özellikle "İsimlendirme (bağlayıcı)" bölümü.

**Goal:** Watch mode'a `polite` bayrağı: mentorun açık sorusu varken dosya feedback'i kuyruğa alınır, kullanıcının cevabından sonra (veya 180 sn tuş hareketsizliği backstop'unda) normal yoldan işlenir. Konu approach dosyasındaki `watch: live` satırı veya `/watch polite off` bugünkü anında-feedback davranışını korur. v0.24.0.

**Architecture:** Saf mantık (kuyruk, soru-açık tespiti, backstop deadline, approach parse) yeni `src/tui/polite.rs` modülünde, birim-testli. `run.rs` (595 satır — 600 bütçesi, ŞİŞİRME) yalnız ince bağlantı alır: watcher dalında kapı, submit sonrası flush, select'e backstop dalı. Slash genişlemesi `slash.rs`'te, gösterge `status.rs`'te.

**Tech Stack:** Rust, tokio (`select!`, `Instant`), ratatui. Binary crate — test filtreli `cargo test <filtre>`.

## Global Constraints

- TÜM yeni kod adları, kullanıcıya dönük string'ler, kod yorumları, commit mesajları İNGİLİZCE (spec "İsimlendirme" bölümü birebir).
- Mevcut davranış korunur: `watching && !polite` yolu bugünkü akışla BİREBİR aynı; mevcut testler kırılmaz.
- `run.rs` 600 satır bütçesini aşmamalı — mantık `polite.rs`'e, `run.rs`'e yalnız çağrılar.
- Her task: TDD (önce failing test), sonda commit (İngilizce) + push. `cargo fmt` yalnız dokunulan dosyalara, clippy yeni uyarı 0.
- Backstop süresi tek yerde: `polite.rs` içinde `pub(crate) const POLITE_BACKSTOP: Duration = Duration::from_secs(180);`

---

### Task 1: Pure logic module (`src/tui/polite.rs`)

**Files:**
- Create: `src/tui/polite.rs` (testler in-module)
- Modify: `src/tui/mod.rs` (modül kaydı: `pub(crate) mod polite;`)

**Interfaces (Produces):**
- `pub(crate) struct PoliteQueue` — `new()`, `push(&mut self, path: PathBuf) -> bool` (sıra-korumalı dedup; **kuyruk boşken yapılan ilk push `true`** döner = notis zamanı), `is_empty(&self) -> bool`, `len(&self) -> usize`, `drain(&mut self) -> Vec<PathBuf>` (sırayla boşaltır, kuyruğu sıfırlar)
- `pub(crate) fn question_open(text: &str) -> bool` — metin `?` içeriyor mu
- `pub(crate) fn backstop_deadline(queue_empty: bool, last_key: tokio::time::Instant) -> Option<tokio::time::Instant>` — kuyruk doluysa `Some(last_key + POLITE_BACKSTOP)`, boşsa `None`
- `pub(crate) fn live_from_approach(text: &str) -> bool` — satırlardan biri trim + ascii-lowercase sonrası `watch: live` mi
- `pub(crate) const POLITE_BACKSTOP: Duration`

- [ ] **Step 1: Failing testler**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn queue_push_dedups_and_preserves_order() {
        let mut q = PoliteQueue::new();
        assert!(q.push(PathBuf::from("a.rs")));  // first push into empty queue → announce
        assert!(!q.push(PathBuf::from("b.rs"))); // queue already non-empty → silent
        assert!(!q.push(PathBuf::from("a.rs"))); // duplicate → silent, not re-added
        assert_eq!(q.len(), 2);
        assert_eq!(q.drain(), vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")]);
        assert!(q.is_empty());
        assert!(q.push(PathBuf::from("a.rs"))); // empty again → announce again
    }

    #[test]
    fn question_open_detects_question_mark() {
        assert!(question_open("What does ownership mean?"));
        assert!(question_open("Try it in parallel. What happens?"));
        assert!(!question_open("Good. Keep going."));
        assert!(!question_open(""));
    }

    #[test]
    fn backstop_deadline_only_when_queue_nonempty() {
        let now = tokio::time::Instant::now();
        assert_eq!(backstop_deadline(true, now), None);
        assert_eq!(backstop_deadline(false, now), Some(now + POLITE_BACKSTOP));
    }

    #[test]
    fn live_from_approach_matches_watch_live_line() {
        assert!(live_from_approach("# JS\n\nwatch: live\n"));
        assert!(live_from_approach("  WATCH: LIVE  \n")); // forgiving case/whitespace
        assert!(!live_from_approach("watch: polite\n"));  // unknown value → default
        assert!(!live_from_approach("# nothing here\n"));
        assert!(!live_from_approach(""));
    }
}
```

- [ ] **Step 2:** `cargo test polite` → derleme hatası (tipler yok)
- [ ] **Step 3:** Implement — `PoliteQueue` içte `Vec<PathBuf>`; `push` önce `contains` kontrolü (küçük kuyruk, linear arama yeter), dönüş değeri `was_empty && added`. `question_open` = `text.contains('?')`. `live_from_approach` = `text.lines().any(|l| l.trim().eq_ignore_ascii_case("watch: live"))`. Modül başına 1-2 satır İngilizce doc-comment (house style: diğer `tui/` modüllerine bak).
- [ ] **Step 4:** `cargo test polite` → PASS
- [ ] **Step 5:** Commit + push: `feat: polite watcher core — queue, question detection, backstop, approach parse`

---

### Task 2: Slash extension (`src/slash.rs`)

**Files:**
- Modify: `src/slash.rs` (`WatchCmd`, `parse_watch_command`, yeni `apply_polite`; testler in-module mevcut test bloğuna)

**Interfaces:**
- Consumes: mevcut `WatchCmd{On,Off,Toggle}`, `parse_watch_command`, `apply_watch` (slash.rs:14-42)
- Produces: `WatchCmd::{PoliteOn, PoliteOff, PoliteToggle}` varyantları; `pub(crate) fn apply_polite(cmd: WatchCmd, cur: bool) -> (bool, &'static str)` — yalnız polite varyantlarıyla çağrılır (diğerlerinde `unreachable!` DEĞİL — `cur` döndür, savunmacı)

- [ ] **Step 1: Failing testler** (mevcut `parse_watch_command_variants` testinin yanına)

```rust
#[test]
fn parse_watch_polite_variants() {
    assert_eq!(parse_watch_command("/watch polite"), Some(WatchCmd::PoliteToggle));
    assert_eq!(parse_watch_command("/watch polite on"), Some(WatchCmd::PoliteOn));
    assert_eq!(parse_watch_command("/watch polite off"), Some(WatchCmd::PoliteOff));
    assert_eq!(parse_watch_command("/WATCH POLITE OFF"), Some(WatchCmd::PoliteOff));
    assert_eq!(parse_watch_command("/watch politely"), None);
}

#[test]
fn apply_polite_transitions() {
    assert!(apply_polite(WatchCmd::PoliteOn, false).0);
    assert!(!apply_polite(WatchCmd::PoliteOff, true).0);
    assert!(apply_polite(WatchCmd::PoliteToggle, false).0);
    assert!(!apply_polite(WatchCmd::PoliteToggle, true).0);
    assert!(apply_polite(WatchCmd::PoliteOn, false).1.contains("polite"));
}
```

- [ ] **Step 2:** `cargo test slash` → derleme hatası
- [ ] **Step 3:** Implement — `parse_watch_command` match'ine üç satır. `apply_polite` mesajları: on → `"polite mode on — file feedback waits while a question is open"`, off → `"polite mode off — instant file feedback"`. Mevcut `apply_watch` DEĞİŞMEZ.
- [ ] **Step 4:** `cargo test slash` → PASS (mevcut varyant testleri dahil)
- [ ] **Step 5:** Commit + push: `feat: /watch polite slash commands`

---

### Task 3: Status indicator (`src/tui/status.rs` + çağrı zinciri)

**Files:**
- Modify: `src/tui/status.rs` (`render_status` imzası), `src/tui/page.rs` (`draw` imzası — koda bak), `src/tui/run.rs` (2 `draw` çağrısı: ~414-421 ve varsa diğeri)

**Interfaces:**
- `render_status(s, tokens, window, watch: Option<(bool, bool)>)` — `(watching, polite)`. Metinler: `(false, _)` → `"watch off "`, `(true, false)` → `"👁 watching "`, `(true, true)` → `"👁 watching·polite "`. `None` → gösterge yok (mevcut davranış).
- `run.rs` çağrılarında ŞİMDİLİK `Some((watching, false))` — gerçek `polite` değişkeni Task 4'te bağlanır (geçici literal, Task 4 değiştirir).

- [ ] **Step 1: Failing test** (status.rs test bloğuna; mevcut `watch_indicator_shows_when_some` güncellenir)

```rust
#[test]
fn watch_indicator_shows_polite_state() {
    assert!(text(&render_status(&Status::Idle, None, 1_000_000, Some((true, true)))).contains("watching·polite"));
    let live = text(&render_status(&Status::Idle, None, 1_000_000, Some((true, false))));
    assert!(live.contains("watching") && !live.contains("polite"));
    assert!(text(&render_status(&Status::Idle, None, 1_000_000, Some((false, true)))).contains("watch off"));
    assert!(!text(&render_status(&Status::Idle, None, 1_000_000, None)).contains("watch"));
}
```

- [ ] **Step 2:** `cargo test status` → derleme hatası (imza)
- [ ] **Step 3:** Implement — imza değişikliği + `draw` zinciri + `run.rs` çağrı yerleri (`Some(watching)` → `Some((watching, false))`). Mevcut `watch_indicator_shows_when_some` testi yeni imzaya uyarlanır (sil değil, güncelle).
- [ ] **Step 4:** `cargo build && cargo test status` → PASS
- [ ] **Step 5:** Commit + push: `feat: status line shows polite watch state`

---

### Task 4: Wiring (`src/tui/run.rs`)

**Files:**
- Modify: `src/tui/run.rs` (init ~408, key arm ~428, watch-cmd dalı 438-444, submit akışı 516-530, watcher dalları 544-587, select'e 1 yeni dal)

**Interfaces (Consumes):** Task 1'in tamamı, Task 2 `apply_polite`, Task 3 draw imzası. Mevcut: `handle_file_change(backend, &mut session, &mut files, project_root, &path, &recorder)` (file_feedback.rs:124), `visual::last_assistant_text(&session)` (run.rs:454 kullanım örneği), `lifecycle::sleep_until_deadline`, `max_feedback_batch` (run() parametresi).

- [ ] **Step 1: Init** — `let mut watching = true;` (run.rs:408) yanına:

```rust
    // Polite default comes from the topic's approach file: a `watch: live`
    // line opens the session in instant-feedback mode.
    let mut polite = !crate::tui::polite::live_from_approach(
        &approach_text_for(global, project_root, &topic),
    );
    let mut pq = crate::tui::polite::PoliteQueue::new();
    // The opening turn usually ends with a question — seed from history.
    let mut question_open =
        crate::visual::last_assistant_text(&session).is_some_and(|t| crate::tui::polite::question_open(&t));
    let mut last_key = tokio::time::Instant::now();
```

`approach_text_for`: konunun approach dosyasını okuyan küçük yardımcı — çözümleme sırası proje `.usta/approaches/<topic>.md` → global `approaches/<topic>.md` (brain.rs:112-125 desenine bak; pub bir yardımcı varsa ONU kullan, yoksa `polite.rs`'e `pub(crate) fn approach_text(global: &Path, project_usta: Option<&Path>, topic: &str) -> String` ekle — okunamayan dosya = boş string = default polite). `last_assistant_text` dönüş tipine bak (`Option<String>` beklenir) — uymuyorsa uyarlama.

- [ ] **Step 2: Key aktivitesi** — key event kolunda (run.rs:428 `Event::Key(k)` dalı, `editor.handle_key`'den ÖNCE): `last_key = tokio::time::Instant::now();`

- [ ] **Step 3: Slash dalı** — 438-444 bloğu polite varyantlarını ayırır:

```rust
    if let Some(cmd) = crate::slash::parse_watch_command(&line) {
        crate::tui::page::page_user_echo(&mut tui, &line)?;
        use crate::slash::WatchCmd::*;
        let msg = match cmd {
            PoliteOn | PoliteOff | PoliteToggle => {
                let (next, msg) = crate::slash::apply_polite(cmd, polite);
                polite = next;
                msg
            }
            _ => {
                let (next, msg) = crate::slash::apply_watch(cmd, watching);
                watching = next;
                msg
            }
        };
        crate::tui::page::page_notice(&mut tui, msg)?;
        continue;
    }
```

- [ ] **Step 4: Feedback işleme yardımcısı** — 568-585'teki `for path in batch { match handle_file_change(...) ... }` gövdesi olduğu gibi yerel bir bloktan iki yerden çağrılabilir hale gelir. Borrow yükü (tui, editor, events, backend, session, files, recorder, last_tokens…) nedeniyle ayrı `fn` yerine **makro değil, düz kod tekrarı da değil**: mevcut döngü `let paths: Vec<PathBuf>` üzerinden çalışan tek blok olarak kalır ve flush noktaları o bloğa `paths` verir. Pratikte: 567-586 `else` gövdesini `paths` değişkenli bloğa çevir; watcher dalı `paths = batch`, flush noktaları `paths = pq.drain()` ile aynı bloğu koşturur. (En temiz mekanik: bloğu `process_paths!`-benzeri makroya DEĞİL, küçük `async fn`'e almayı önce dene — imza çok şişerse loop gövdesini tek yerde tutup flush'ı `select!` sonrası ortak noktaya düşür. 600 satır bütçesini aşarsan taşan mantığı `polite.rs`/`file_feedback.rs`'e it.) Yanit kolunda İKİ ek satır:

```rust
    Ok(crate::file_feedback::FileFeedback::Yanit { tokens, reply, show_topic }) => {
        question_open = crate::tui::polite::question_open(&reply.text); // feedback replies can ask too
        // ... mevcut satırlar aynen ...
    }
```

- [ ] **Step 5: Watcher dalı kapısı** — debounce dalında (549-586) `else` zincirine polite kapısı:

```rust
    } else if polite && question_open {
        // A question is open — queue instead of interrupting (spec: polite mode).
        for path in batch {
            if pq.push(path) {
                crate::tui::page::page_notice(&mut tui, "change noticed — feedback after your answer")?;
            }
        }
    } else {
        /* mevcut anında-feedback bloğu (paths = batch) */
    }
```

Bulk limiti: mevcut `batch.len() > max_feedback_batch` kontrolü kuyruk YOLUNDAN ÖNCE, en üstte kalır (değişmez); flush'ta da `pq` uzunluğu limitten büyükse aynı bulk-notice + `files.observe` senkronu + kuyruk temizlenir (aynı davranış, `paths` kaynağı farklı).

- [ ] **Step 6: Flush noktası 1 (cevap sonrası)** — submit akışında: `session.push_user(&outgoing);` (516) satırından önce `question_open = false;`. `AskOutcome::Reply` kolunda (`clean` hazır olduktan sonra, 528 civarı): `question_open = crate::tui::polite::question_open(&clean);` ve blok kapanıp `trigger_auto_visual` tamamlandıktan sonra: `if !pq.is_empty() { /* paths = pq.drain() ile ortak feedback bloğu */ }` — KOŞULSUZ (yeni yanıt soru içerse de bekleyenler işlenir; spec).

- [ ] **Step 7: Backstop select dalı** — `select!` içine (watcher dallarının yanına):

```rust
    _ = crate::lifecycle::sleep_until_deadline(
        crate::tui::polite::backstop_deadline(pq.is_empty(), last_key)
    ), if polite && !pq.is_empty() => {
        // User went quiet mid-question — don't sit on the feedback forever.
        /* paths = pq.drain() ile ortak feedback bloğu */
    }
```

`sleep_until_deadline` imzasına bak (debouncer kullanımı 547 ile aynı desen).

- [ ] **Step 8:** Task 3'ün geçici `Some((watching, false))` çağrıları → `Some((watching, polite))`.
- [ ] **Step 9:** `cargo build && cargo test` → TÜMÜ PASS (mevcut testler kırılmamalı). `cargo clippy` yeni uyarı 0. `wc -l src/tui/run.rs` → ≤600; aşıyorsa taşan mantığı `polite.rs`'e taşı ve yeniden koş.
- [ ] **Step 10:** Commit + push: `feat: polite watcher wiring — queue while question open, flush after answer or backstop`

---

### Task 5: SPEC + v0.24.0 + release

**Files:**
- Modify: `SPEC.md` (yeni `## 4.21 Polite Watcher (v0.24)` bölümü + §11'e karar satırı), `Cargo.toml`, `Cargo.lock`

- [ ] **Step 1:** SPEC.md §4.21: 4-6 satır — polite bayrağı (default on), soru-açık kuyruğu, cevap-sonrası/180 sn backstop flush, `watch: live` approach anahtarı, `/watch polite` ailesi, plain yol kapsam dışı. §11'e: "Polite watcher: `?` heuristiği yeter, LLM protokol bayrağı yok (prompt diet); oturumluk override approach'a yazılmaz." Mevcut bölüm üslubuna uy (İngilizce).
- [ ] **Step 2:** Cargo.toml `version = "0.24.0"`; sürüm testi varsa `"0.24.0"`e güncelle (grep `0.23.0`).
- [ ] **Step 3:** Verify: `cargo build && cargo test` PASS · clippy yeni uyarı 0 · `cargo install --path .`
- [ ] **Step 4:** Commit + push + tag:

```bash
git add SPEC.md Cargo.toml Cargo.lock src/
git commit -m "feat: polite watcher — v0.24.0"
git push
git tag v0.24.0 && git push --tags
```

- [ ] **Step 5 (elle doğrulama — ATLA, Anil koşacak):** soru açıkken dosya kaydet → "change noticed" notisi, cevaptan sonra feedback gelir · cevap yazmadan ~3 dk bekle → feedback kendiliğinden gelir · `/watch polite off` → anında feedback · approach'a `watch: live` → oturum live açılır (status: `watching`, polite yok).
