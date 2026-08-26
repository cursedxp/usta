# Polite Watcher Round 2 (v0.24.2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** v0.24.1 (`215243d`) üstüne. Spec: `docs/superpowers/specs/2026-08-26-polite-watcher-design.md` — "v0.24.2 Düzeltmeler" + "İsimlendirme (bağlayıcı)" bölümleri önce okunur. v0.24.1 final review detayı: `.superpowers/sdd/progress.md`.

**Goal:** Routing saf fonksiyona çıkar (G0, ön-koşul), `/watch polite off` bekleyen kuyruğu hemen işler (G1 — Anil kararı), mesaj/README/SPEC düzeltmeleri (G2), MSRV + test hijyeni (G3). v0.24.2.

**Architecture:** G0 önce — run.rs'teki 4-yollu watcher kararı `polite.rs::route`'a iner, run.rs satır kazanır ve `watching` kapısı test edilebilir olur. G1 o açılan alana yazılır. G2/G3 metin ve küçük teknik işler.

**Tech Stack:** Rust, tokio. Binary crate — `cargo test <filtre>`.

## Global Constraints

- TÜM yeni kod adları, string'ler, yorumlar, commit mesajları İNGİLİZCE.
- `run.rs` ≤600 satır (şu an 600/600 — G0 yer açar; G1 o alandan büyük olamaz).
- TDD; her task sonunda `cargo build && cargo test` yeşil, clippy yeni uyarı 0, fmt dokunulan dosyalara, İngilizce commit + push.
- G0 SAF refactor: davranış birebir, mevcut 386 test değişmeden yeşil kalır (yalnız yeni testler eklenir).

---

### Task 1: G0 — Routing çıkarımı (`polite.rs::route`)

**Files:**
- Modify: `src/tui/polite.rs` (Route enum + route fn + testler), `src/tui/run.rs` (watcher debounce dalı ~549+ — mevcut if/else zinciri `match route(...)`'a döner)

**Interfaces (Produces):**
- `pub(crate) enum Route { Bulk, ObserveOnly, Queue, Feedback }` (`#[derive(Debug, PartialEq)]`)
- `pub(crate) fn route(batch_len: usize, max_batch: usize, watching: bool, polite: bool, question_open: bool) -> Route` — sıra: `batch_len > max_batch` → Bulk; `!watching` → ObserveOnly; `polite && question_open` → Queue; değilse Feedback. Mevcut zincirin birebir çevirisi — run.rs'teki gerçek sıraya BAK, uydurma.

- [ ] **Step 1: Failing testler**

```rust
#[test]
fn route_truth_table() {
    use Route::*;
    // bulk wins over everything
    assert_eq!(route(11, 10, true, true, true), Bulk);
    assert_eq!(route(11, 10, false, false, false), Bulk);
    // watching off → observe only, regardless of polite/question
    assert_eq!(route(1, 10, false, true, true), ObserveOnly);
    assert_eq!(route(1, 10, false, false, false), ObserveOnly);
    // polite + open question → queue
    assert_eq!(route(1, 10, true, true, true), Queue);
    // polite but no open question → instant feedback
    assert_eq!(route(1, 10, true, true, false), Feedback);
    // live mode ignores question state
    assert_eq!(route(1, 10, true, false, true), Feedback);
    assert_eq!(route(1, 10, true, false, false), Feedback);
    // boundary: exactly max is NOT bulk (existing `>` comparison)
    assert_eq!(route(10, 10, true, false, false), Feedback);
}
```

- [ ] **Step 2:** `cargo test route` → derleme hatası
- [ ] **Step 3:** Implement `route`; run.rs watcher dalını `match crate::tui::polite::route(batch.len(), max_feedback_batch, watching, polite, question_open)`'a çevir — dört kolun GÖVDELERİ mevcut kodun birebir taşınması (Bulk: bulk notice + observe senkronu; ObserveOnly: observe döngüsü; Queue: pq.push + notice; Feedback: process_paths). Hiçbir string/side-effect değişmez.
- [ ] **Step 4:** `cargo build && cargo test` → 386 eski + yeni testler PASS · `wc -l src/tui/run.rs` < 600 (kaç satır kazanıldığını rapora yaz)
- [ ] **Step 5:** Commit + push: `refactor: extract watcher routing decision into pure route()`

---

### Task 2: G1 — `/watch polite off` kuyruğu hemen işler

**Files:**
- Modify: `src/tui/run.rs` (slash dalındaki polite kolu, ~445-455)

**Interfaces (Consumes):** `apply_polite`, `pq.drain()`, `pq.is_empty()`, `polite::process_paths(...)` (Task 4/v0.24.1'deki imza — run.rs backstop dalındaki mevcut çağrıya bak, aynı argümanlarla).

- [ ] **Step 1: Implement** — polite kolunda `polite = next;` sonrası:

```rust
    PoliteOn | PoliteOff | PoliteToggle => {
        let (next, msg) = crate::slash::apply_polite(cmd, polite);
        polite = next;
        if !polite && !pq.is_empty() {
            // Turning polite off means "give me instant feedback" — deliver
            // the queue now instead of stranding it (spec v0.24.2 G1).
            crate::tui::page::page_notice(&mut tui, "polite mode off — delivering queued feedback")?;
            crate::tui::polite::process_paths(/* backstop dalındaki mevcut argüman listesinin aynısı */, pq.drain(), max_feedback_batch).await?;
            continue;
        }
        msg
    }
```

Kuyruk boşsa mevcut `msg` akışı aynen (notice tek kez basılmalı — `continue` yolu ikinci notice basmaz, koda uydur). Argüman listesi: backstop dalındaki `process_paths` çağrısından KOPYALA (tui, editor, events, backend, session, files, recorder, project_root, topic, last_tokens, question_open).

- [ ] **Step 2:** Route testleri zaten `watching` kapısını kanıtlıyor (Task 1); bu task'ın wiring'i için mevcut test altyapısı yoksa kod incelemesi + build yeterli. `wc -l src/tui/run.rs` ≤600.
- [ ] **Step 3:** `cargo build && cargo test` → PASS
- [ ] **Step 4:** Commit + push: `fix: /watch polite off delivers the pending queue immediately`

---

### Task 3: G2 + G3 — Mesajlar, README/SPEC metinleri, MSRV, test hijyeni

**Files:**
- Modify: `src/tui/run.rs` VEYA `src/slash.rs` (`/watch off` mesajı — kuyruk doluyken ek), `README.md`, `SPEC.md` (:249 civarı + §4.21), `Cargo.toml` (`rust-version`), `src/watcher.rs` (temp test adları)

- [ ] **Step 1:** `/watch off` kuyruk doluyken notice'a ek: mevcut `apply_watch` off mesajı sabit `&'static str` — run.rs'te kuyruk drop edilen noktada (v0.24.1 F2 bloğu) `page_notice(&mut tui, "(pending feedback dropped)")?;` tek ek satır (apply_watch imzasına DOKUNMA).
- [ ] **Step 2:** README polite bölümü: "never lost" ifadesi düzeltilir — `/watch off` drops the pending queue, `/watch polite off` delivers it; "180s of inactivity" → "180s without a keystroke in usta (editor saves don't extend the window)".
- [ ] **Step 3:** SPEC.md: :249 civarındaki "exactly the pre-v0.24 behavior" cümlesine F4 istisnası (directory events are filtered in all modes); §4.21'e v0.24.2 tek satır not (route extraction, polite-off delivery).
- [ ] **Step 4:** `Cargo.toml`'a `rust-version = "1.88"` (`[package]` altına). NOT — plan aslında 1.83 diyordu; implementasyonda `cargo metadata --locked` ile ölçüldü, kilitli bağımlılık tabanı 1.88 çıktı ve öyle ship edildi (spec G3 düzeltmesi). Bu satır geriye dönük düzeltildi.
- [ ] **Step 5:** `src/watcher.rs` temp-dir test adlarına `std::process::id()` soneki (polite.rs:325 desenine bak, aynısı).
- [ ] **Step 6:** `cargo build && cargo test` → PASS. Commit + push: `docs: correct polite watcher copy, pin MSRV, test hygiene`

---

### Task 4: v0.24.2 release

**Files:** `Cargo.toml`, `Cargo.lock`

- [ ] **Step 1:** Cargo.toml `0.24.2`; sürüm testi varsa güncelle (grep `0.24.1` src/).
- [ ] **Step 2:** Verify: `cargo build && cargo test` PASS · clippy yeni uyarı 0 · `cargo install --path .`
- [ ] **Step 3:** Commit + push + tag:

```bash
git add -A
git commit -m "fix: polite watcher round 2 — v0.24.2"
git push
git tag v0.24.2 && git push --tags
```

- [ ] **Step 4 (elle doğrulama — ATLA, Anil koşacak):** oturumu yeniden başlat · soru açıkken kaydet + `/watch polite off` → bekleyen feedback HEMEN gelir · soru açıkken kaydet + `/watch off` → "(pending feedback dropped)" ve feedback gelmez.
