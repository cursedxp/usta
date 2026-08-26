# Polite Watcher Round 3 (v0.24.3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** v0.24.2 üstüne (docs HEAD `3ca8266`). Spec: `docs/superpowers/specs/2026-08-26-polite-watcher-design.md` — "v0.24.3 Düzeltmeler" + "İsimlendirme (bağlayıcı)". Bulguların kaynağı: `.superpowers/sdd/progress.md` v0.24.2 final review bölümü.

**Goal:** Bulk rotası bekleyen kuyruğu yutamaz (H1), polite-off teslim mesajı yalnız teslim gerçekleşecekse basılır (H2), run.rs wiring'ine source-pin testi (H3). v0.24.3.

**Architecture:** Üç değişiklik de `polite.rs` ağırlıklı (run.rs 599/600 — oraya satır eklenmez; gerekirse mantık polite.rs'e). H1/H2 mevcut yardımcıların içinde (`process_paths` bulk yolu, `deliver_queue_on_polite_off`), H3 salt test.

**Tech Stack:** Rust, tokio. Binary crate — `cargo test <filtre>`.

## Global Constraints

- TÜM yeni kod adları, string'ler, yorumlar, commit mesajları İNGİLİZCE.
- `run.rs` ≤600 satır (şu an 599) — bu plan run.rs'e satır EKLEMEZ (H1/H2 polite.rs içinde; H3 test dosyası).
- TDD; her task sonunda `cargo build && cargo test` yeşil, clippy yeni uyarı 0, fmt dokunulan dosyalara, İngilizce commit + push.
- Polite kapalı (`watch: live` / `/watch polite off` sonrası) akışlar davranış değiştirmez.

---

### Task 1: H1 — Bulk rotası kuyruğu batch'e katar

**Files:**
- Modify: `src/tui/polite.rs` (bulk yolunun gövdesi nerede yaşıyorsa — v0.24.2 raporuna göre `polite.rs:246-256` civarı `sync_baseline` çağrısı; koda bak) ve gerekiyorsa `src/tui/run.rs` Bulk match kolu (satır EKLEMEDEN — mevcut çağrıya `&mut pq` geçirmek gerekirse mevcut satır içinde)

**Interfaces (Consumes):** `PoliteQueue::drain()` (armed_at'i temizler), bulk yolundaki baseline senkron döngüsü (`files.observe`), `Route::Bulk` kolu.

**Davranış:** Bulk event geldiğinde bekleyen kuyruk varsa: kuyruk `drain` edilir, path'leri bulk batch'iyle BİRLİKTE baseline senkronundan geçer. Kuyruk boş kalır, `armed_at` None olur, backstop susar. Bildirim mevcut bulk notice'ı — ek mesaj yok.

- [ ] **Step 1: Failing test** (polite.rs test bloğuna; mevcut bulk/sync test desenine bak, gerçek temp dosya + `ChangePayload` assert'leriyle):

```rust
#[test]
fn bulk_route_absorbs_pending_queue() {
    // Arrange: queue holds a real temp file with observed baseline content,
    // then the file changes on disk, then a bulk event arrives.
    // Act: run the bulk-path handler with the queue non-empty.
    // Assert: queue is empty, armed_at is None, and a fresh observe of the
    // file yields ChangePayload::Skip (baseline was synced — no stale diff,
    // no stranded promise).
}
```

(Gövde mevcut test yardımcılarıyla doldurulur — v0.24.2'nin `silence_queue_on_watch_off` testi birebir şablon: temp dosya, `files.observe`, drain sonrası `Skip` assert'i. Fonksiyon imzası koddaki gerçek bulk yardımcısına göre kurulur; bulk gövdesi hâlâ run.rs içindeyse önce onu polite.rs yardımcısına çıkar — run.rs satır kazanır, test edilebilir olur.)

- [ ] **Step 2:** `cargo test bulk_route` → FAIL (veya derleme hatası)
- [ ] **Step 3:** Implement — bulk yardımcısı `pq`'yu parametre alır, `for path in pq.drain() { /* files.observe sync */ }` bulk batch senkronuyla aynı döngüden geçirir (sıra: önce batch mi kuyruk mu fark etmez — ikisi de yalnız observe).
- [ ] **Step 4:** `cargo build && cargo test` → PASS · `wc -l src/tui/run.rs` ≤600
- [ ] **Step 5:** Commit + push: `fix: bulk route absorbs the pending polite queue instead of starving it`

---

### Task 2: H2 — Polite-off teslim mesajı koşullu

**Files:**
- Modify: `src/tui/polite.rs` (`deliver_queue_on_polite_off` — notice `polite.rs:214` civarı; koda bak)

**Interfaces (Produces):** karar saf yardımcıya çıkar: `pub(crate) fn polite_off_delivery_notice(queue_len: usize, max_batch: usize) -> Option<&'static str>` — `queue_len == 0` → None (mesaj yok, normal apply_polite mesajı akar) · `queue_len <= max_batch` → `Some("polite mode off — delivering queued feedback")` · aşıyorsa → None (bulk-skip bildirimi gerçeği söyleyecek).

- [ ] **Step 1: Failing test**

```rust
#[test]
fn polite_off_notice_only_when_queue_actually_deliverable() {
    assert_eq!(polite_off_delivery_notice(0, 10), None);
    assert_eq!(
        polite_off_delivery_notice(3, 10),
        Some("polite mode off — delivering queued feedback")
    );
    assert_eq!(polite_off_delivery_notice(11, 10), None); // bulk skip will tell the truth
    assert_eq!(polite_off_delivery_notice(10, 10).is_some(), true); // boundary: exactly max delivers
}
```

- [ ] **Step 2:** `cargo test polite_off_notice` → derleme hatası
- [ ] **Step 3:** Implement + `deliver_queue_on_polite_off` içindeki koşulsuz notice bu yardımcıya bağlanır (None → notice basılmaz, teslim/`process_paths` akışı aynen devam eder).
- [ ] **Step 4:** `cargo build && cargo test` → PASS
- [ ] **Step 5:** Commit + push: `fix: gate polite-off delivery notice on deliverable queue length`

---

### Task 3: H3 — run.rs wiring source-pin testi + v0.24.3 release

**Files:**
- Modify: `src/tui/polite.rs` (test bloğu — pin testi), `Cargo.toml`, `Cargo.lock`, `SPEC.md` (§4.21'e tek satır v0.24.3 notu)

- [ ] **Step 1: Pin testi** (polite.rs test bloğuna — kaba ama etkili: iki turdur wiring silinse de suite yeşil kalıyordu):

```rust
#[test]
fn run_rs_wiring_call_sites_are_pinned() {
    // Crude source pin: the TUI loop is bound to Terminal<CrosstermBackend<Stdout>>
    // and can't be unit-tested; this guards the polite wiring against silent deletion.
    let src = include_str!("run.rs");
    for needle in [
        "polite::route(",
        "silence_queue_on_watch_off",
        "deliver_queue_on_polite_off",
        "backstop_deadline(",
    ] {
        assert!(src.contains(needle), "run.rs lost its polite wiring: {needle}");
    }
}
```

(`include_str!` yolu: polite.rs ile run.rs aynı dizinde (`src/tui/`) — `"run.rs"` doğru; derleme hatası verirse yolu uyarla. Needle listesi gerçek çağrı adlarıyla senkron — koda bak, ad farklıysa gerçeğini yaz.)

- [ ] **Step 2:** `cargo test run_rs_wiring` → PASS (pin mevcut kodu doğrular). Negatif kanıt: needle'lardan birini geçici boz → FAIL gör → geri al (mutasyon kanıtı, commit'e girmez).
- [ ] **Step 3:** SPEC.md §4.21'e tek satır: v0.24.3 — bulk absorbs pending queue; delivery notice gated; wiring pinned by source test.
- [ ] **Step 4:** Cargo.toml `0.24.3`; sürüm testi varsa güncelle (grep `0.24.2` src/).
- [ ] **Step 5:** Verify: `cargo build && cargo test` PASS · clippy yeni uyarı 0 · `cargo install --path .`
- [ ] **Step 6:** Commit + push + tag:

```bash
git add -A
git commit -m "fix: polite watcher round 3 — v0.24.3"
git push
git tag v0.24.3 && git push --tags
```

- [ ] **Step 7 (elle doğrulama — ATLA, Anil koşacak):** oturumu yeniden başlat · soru açıkken 1 dosya kaydet + ardından bulk değişiklik (örn. `cargo new` benzeri çok dosyalı işlem) → bulk notice gelir, askıda bekleyen kalmaz; sonraki tekil kayıt normal feedback üretir · kuyruk doluyken `/watch polite off` → tek tutarlı mesaj akışı.
