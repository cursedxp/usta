# Polite Watcher Fixes (v0.24.1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** v0.24.0 (`c258c3c`) üstüne. Spec: `docs/superpowers/specs/2026-08-26-polite-watcher-design.md` — "v0.24.1 Düzeltmeler" bölümü + "İsimlendirme (bağlayıcı)" bölümü önce okunur.

**Goal:** Canlı oturumda doğrulanan dört kusur kapanır: backstop ilk penceresi kuyruk kurulma anına çapalanır (F1), `/watch off` bekleyen kuyruğu susturur (F2), `/watch polite` + `watch: live` dokümante edilir (F3), dizin event'leri sessiz elenir (F4). v0.24.1.

**Architecture:** F1 saf mantık değişikliği `src/tui/polite.rs`'te (`PoliteQueue.armed_at` + `backstop_deadline` imza değişikliği), `run.rs`'te yalnız çağrı uyumu + `db0b47b`'nin artık gereksiz elle bump'ının kaldırılması. F2 `run.rs` slash dalı + select guard. F3 yalnız docs/help. F4 `watcher.rs` kaynak filtresi + `file_feedback.rs` sınıflandırma.

**Tech Stack:** Rust, tokio `Instant`. Binary crate — `cargo test <filtre>`.

## Global Constraints

- TÜM yeni kod adları, string'ler, yorumlar, commit mesajları İNGİLİZCE.
- `run.rs` ≤600 satır (şu an 599 — F2 satır ekliyor, F1 iki satır siliyor; aşarsan mantığı `polite.rs`'e it).
- TDD; her task sonunda `cargo build && cargo test` yeşil, clippy yeni uyarı 0, `cargo fmt` dokunulan dosyalara, İngilizce commit + push.
- Mevcut davranış: polite kapalıyken (`/watch polite off` veya `watch: live`) hiçbir akış değişmez.

---

### Task 1: F1 — Backstop penceresi kuyruk kurulma anına çapalanır

**Files:**
- Modify: `src/tui/polite.rs` (PoliteQueue + backstop_deadline + testler), `src/tui/run.rs` (çağrı uyumu, ~584-589 backstop dalı)

**Interfaces:**
- Consumes: mevcut `PoliteQueue{push,drain,is_empty,len}`, `backstop_deadline(queue_empty: bool, last_key: Instant)`, run.rs backstop dalı (`db0b47b` sonrası hali: fire içinde `last_key = Instant::now();`)
- Produces: `PoliteQueue::armed_at(&self) -> Option<tokio::time::Instant>` (boş kuyruk → None; ilk push damgalar, `drain` temizler) · YENİ imza `backstop_deadline(armed_at: Option<Instant>, last_key: Instant) -> Option<Instant>` = `armed_at.map(|a| a.max(last_key) + POLITE_BACKSTOP)`

- [ ] **Step 1: Failing testler** (polite.rs test bloğuna; mevcut `backstop_deadline_only_when_queue_nonempty` yeni imzaya güncellenir — sil değil, uyarla)

```rust
#[test]
fn queue_stamps_armed_at_on_first_push_and_clears_on_drain() {
    let mut q = PoliteQueue::new();
    assert_eq!(q.armed_at(), None);
    let before = tokio::time::Instant::now();
    q.push(std::path::PathBuf::from("a.rs"));
    let armed = q.armed_at().expect("armed after first push");
    assert!(armed >= before);
    q.push(std::path::PathBuf::from("b.rs"));
    assert_eq!(q.armed_at(), Some(armed)); // second push does not re-stamp
    q.drain();
    assert_eq!(q.armed_at(), None);
}

#[test]
fn backstop_window_never_shorter_than_arm_time() {
    let last_key = tokio::time::Instant::now();
    let armed = last_key + POLITE_BACKSTOP; // user idle 180s, THEN saves a file
    // Old bug: deadline = last_key + 180 = already past → fired immediately.
    assert_eq!(backstop_deadline(Some(armed), last_key), Some(armed + POLITE_BACKSTOP));
    // Typing after the queue armed still extends the window:
    let late_key = armed + std::time::Duration::from_secs(10);
    assert_eq!(backstop_deadline(Some(armed), late_key), Some(late_key + POLITE_BACKSTOP));
    assert_eq!(backstop_deadline(None, last_key), None);
}
```

- [ ] **Step 2:** `cargo test polite` → derleme hatası
- [ ] **Step 3:** Implement — `PoliteQueue`'ya `armed_at: Option<Instant>` alanı; `push` boş kuyruğa ilk eklemede `Instant::now()` damgalar; `drain` `None`'a çeker. `backstop_deadline` yeni imza. run.rs çağrısı: `backstop_deadline(pq.armed_at(), last_key)`. Backstop dalındaki `last_key = tokio::time::Instant::now();` satırı VE üstündeki "Re-arm last_key on fire" yorumu KALDIRILIR (`db0b47b` bu tasarımla kapsanıyor: drain → armed_at None → sonraki push taze pencere açar) — kaldırma gerekçesi tek satır İngilizce yorumla dalda bırakılır (`// window anchoring in backstop_deadline covers re-arm; see spec v0.24.1 F1`).
- [ ] **Step 4:** `cargo build && cargo test` → TÜMÜ PASS
- [ ] **Step 5:** Commit + push: `fix: anchor polite backstop window to queue arm time`

---

### Task 2: F2 — `/watch off` bekleyen kuyruğu susturur

**Files:**
- Modify: `src/tui/run.rs` (slash dalı ~438-460 ve backstop guard ~586)

**Interfaces (Consumes):** `apply_watch`, `pq.drain()`, `files.observe(&path, contents)` (mevcut `!watching` dalındaki baseline senkron deseni, run.rs ~560-566).

- [ ] **Step 1: Implement** — slash dalında `apply_watch` sonucu `watching` false'a düştüğünde:

```rust
    _ => {
        let (next, msg) = crate::slash::apply_watch(cmd, watching);
        watching = next;
        if !watching && !pq.is_empty() {
            // Watch off silences pending feedback too — sync the diff baseline
            // so the next save doesn't produce a giant stale diff.
            for path in pq.drain() {
                if let Ok(c) = std::fs::read_to_string(&path) {
                    let _ = files.observe(&path, c);
                }
            }
        }
        msg
    }
```

Backstop guard: `if polite && !pq.is_empty()` → `if watching && polite && !pq.is_empty()`.

- [ ] **Step 2: Test** — kuyruk+guard mantığı `select!` içinde birim-test edilemiyor; birim test edilebilir kısım yoksa bu task için kod incelemesi + Step 3 build yeterli (mevcut testler kırılmamalı). `polite.rs`'te test edilebilir yardımcı çıkarsa (örn. drain+observe path listesi) oraya taşı ve test yaz — zorlama, 600 bütçesine bak.
- [ ] **Step 3:** `cargo build && cargo test` → PASS · `wc -l src/tui/run.rs` ≤600
- [ ] **Step 4:** Commit + push: `fix: /watch off drops queued feedback and disarms backstop`

---

### Task 3: F4 — Dizin event'leri sessiz

**Files:**
- Modify: `src/watcher.rs` (event path filtresi, ~21-54 spawn bloğu — koda bak), `src/file_feedback.rs` (`is_silent_skip`, mevcut NotFound/InvalidData deseni :64 civarı; testler in-module)

**Interfaces (Consumes):** `is_silent_skip(e: &anyhow::Error) -> bool` mevcut sınıflandırma; watcher'ın event → `send(path)` noktası.

- [ ] **Step 1: Failing test** (file_feedback.rs test bloğuna, mevcut `is_silent_skip_*` testlerinin deseniyle):

```rust
#[test]
fn is_silent_skip_true_for_wrapped_is_a_directory() {
    let io = std::io::Error::new(std::io::ErrorKind::IsADirectory, "is a directory");
    let e = anyhow::Error::from(io).context("reading changed file");
    assert!(is_silent_skip(&e));
}
```

- [ ] **Step 2:** `cargo test is_silent_skip` → FAIL
- [ ] **Step 3:** Implement — `is_silent_skip` match'ine `ErrorKind::IsADirectory` eklenir (rustc 1.98; 1.83'te stabil). Watcher'da event path'leri gönderilmeden önce `path.is_dir()` olanlar elenir (kaynak filtresi; TOCTOU artığını is_silent_skip yakalar). Watcher değişikliği mevcut `is_ignored` deseninin yanına — koda bak, filtre nerede uygulanıyorsa oraya.
- [ ] **Step 4:** `cargo build && cargo test` → PASS
- [ ] **Step 5:** Commit + push: `fix: silence directory events in file watcher`

---

### Task 4: F3 — Dokümantasyon + SPEC + v0.24.1 + release

**Files:**
- Modify: `src/help.rs` (yardım metni — mevcut `/watch` satırının formatına bak), `README.md` (İngilizce polite mode bölümü), `SPEC.md` (§4.21 "180s window" cümlesi F1 ile artık doğru — kontrol; §4.21'e v0.24.1 tek satır not), `Cargo.toml`, `Cargo.lock`

- [ ] **Step 1:** `help.rs`: `/watch` satırının yanına `/watch polite [on|off] — queue file feedback while a question is open (default: on)`. Help testi varsa güncelle (grep `watch` in help tests).
- [ ] **Step 2:** README.md: kısa "Polite watching" alt bölümü — default davranış (feedback waits while a question is open, 180s idle backstop), `/watch polite off`, approach dosyasına `watch: live`. README zaten İngilizce — üsluba uy.
- [ ] **Step 3:** SPEC.md §4.21: v0.24.1 düzeltme notu tek-iki satır (backstop anchor, /watch off queue, directory events). "180s" cümlesini doğrula.
- [ ] **Step 4:** Cargo.toml `0.24.1`; sürüm testi varsa güncelle (grep `0.24.0` src/).
- [ ] **Step 5:** Verify: `cargo build && cargo test` PASS · clippy yeni uyarı 0 · `cargo install --path .`
- [ ] **Step 6:** Commit + push + tag:

```bash
git add src/ README.md SPEC.md Cargo.toml Cargo.lock
git commit -m "fix: polite watcher follow-ups — docs, v0.24.1"
git push
git tag v0.24.1 && git push --tags
```

- [ ] **Step 7 (elle doğrulama — ATLA, Anil koşacak):** usta oturumunu YENİDEN başlat (install eski açık oturumu değiştirmez) · soru açıkken 3+ dk klavyeye dokunmadan dosya kaydet → notis gelir, feedback ~180 sn SONRA gelir (anında değil) · `/watch off` → bekleyen feedback gelmez · `cargo new` benzeri dizin oluşturan işlem → "Is a directory" hatası yok.
