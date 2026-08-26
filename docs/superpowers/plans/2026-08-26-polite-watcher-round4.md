# Polite Watcher Round 4 (v0.24.4) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** v0.24.3 (`40f8eb8`) üstüne. Spec: `docs/superpowers/specs/2026-08-26-polite-watcher-design.md` — "v0.24.4 Düzeltmeler" + "İsimlendirme (bağlayıcı)". Bulgu kaynağı: `.superpowers/sdd/progress.md` "v0.24.4 için açık bulgular" bölümü.

**Goal:** Üç mekanik düzeltme: polite-off onay mesajı yutulmaz (J1), yalan doc comment düzelir (J2), H3 pin testi kendi turunun fix'ini de pinler (J3). v0.24.4.

**Architecture:** Hepsi `polite.rs` içinde; run.rs'e satır eklenmez. (Not: 600-satır bütçesi SPEC.md:360'a göre yalnız production kod — polite.rs 375 production satırda, baskı yok.)

**Tech Stack:** Rust, tokio. Binary crate — `cargo test <filtre>`.

## Global Constraints

- TÜM yeni kod adları, string'ler, yorumlar, commit mesajları İNGİLİZCE.
- `run.rs` production satırı ≤600 (şu an 599) — bu plan run.rs'e DOKUNMAZ.
- TDD; her task sonunda `cargo build && cargo test` yeşil, clippy yeni uyarı 0, fmt dokunulan dosyalara, İngilizce commit + push.
- Davranış değişikliği YALNIZ J1'in onay mesajı — başka hiçbir akış değişmez.

---

### Task 1: J1 + J2 — Polite-off onay mesajı + doc comment

**Files:**
- Modify: `src/tui/polite.rs` (`deliver_queue_on_polite_off` dönüş anlamı + :206-207 civarı doc comment; testler in-module)

**Interfaces:**
- Consumes: `deliver_queue_on_polite_off(...) -> Result<bool>` (mevcut — koda bak), `polite_off_delivery_notice(queue_len, max_batch) -> Option<&'static str>`, run.rs:455-457'deki okuma (`true` = "kendi mesajını bastı, apply_polite mesajını basma").
- Produces: dönüş değeri artık "bu fonksiyon kendi notice'ını bastı mı" gerçeğini taşır — limit-üstü kuyrukta `false` döner, run.rs değişmeden `apply_polite`'ın "polite mode off — instant file feedback" mesajı akar.

- [ ] **Step 1: Failing test** (mevcut deliver/notice test desenine bak — fonksiyon Tui gerektiriyorsa test edilebilir çekirdek `polite_off_delivery_notice` zaten var; bu task'ta dönüş-değeri sözleşmesini test et):

```rust
#[test]
fn deliver_queue_reports_whether_it_printed_a_notice() {
    // Contract: the bool mirrors polite_off_delivery_notice(...).is_some().
    // Deliverable queue (len <= max) → true (notice printed here).
    // Over-limit queue (len > max) → false (bulk-skip will speak; caller's
    // apply_polite confirmation must still print).
    // Empty queue → false.
}
```

(Gövde: `deliver_queue_on_polite_off` doğrudan test edilemiyorsa — Tui bağımlılığı — dönüş kararını `polite_off_delivery_notice(...).is_some()` ifadesine indirger ve O sözleşmeyi mevcut `polite_off_notice_only_when_queue_actually_deliverable` testine 3 assert ekleyerek pinle; ayrıca H3-tarzı source-pin assert'i ile `deliver_queue_on_polite_off` gövdesinin `notice.is_some()` içerdiğini doğrula. Zorlama mock yazma — en küçük gerçek kanıt yeter.)

- [ ] **Step 2:** İlgili test → FAIL (veya derleme hatası)
- [ ] **Step 3:** Implement — dönüş `Ok(notice.is_some())` desenine çekilir; :206-207 doc comment gerçek davranışa göre yeniden yazılır (İngilizce): fonksiyon yalnız teslim edilebilir kuyrukta kendi notice'ını basar, aksi halde false döner ve çağıran apply_polite mesajını basar.
- [ ] **Step 4:** `cargo build && cargo test` → PASS
- [ ] **Step 5:** Commit + push: `fix: polite-off confirmation is not swallowed on over-limit queue`

---

### Task 2: J3 — Pin needle'ları + v0.24.4 release

**Files:**
- Modify: `src/tui/polite.rs` (`run_rs_wiring_call_sites_are_pinned` needle listesi), `SPEC.md` (§4.21 tek satır), `Cargo.toml`, `Cargo.lock`, sürüm testi (`welcome_tests.rs` — grep `0.24.3`)

- [ ] **Step 1:** Pin testine iki needle: `"bulk_skip_absorbing_queue"` ve `"process_paths"`. Negatif kanıt: run.rs:570'teki `process_paths` çağrısını geçici boz → test FAIL → geri al (commit'e girmez, rapora yazılır).
- [ ] **Step 2:** SPEC.md §4.21'e tek satır: v0.24.4 — polite-off confirmation no longer swallowed; wiring pin covers bulk absorb + post-turn flush.
- [ ] **Step 3:** Cargo.toml `0.24.4`; sürüm testi güncelle.
- [ ] **Step 4:** Verify: `cargo build && cargo test` PASS · clippy yeni uyarı 0 · `cargo install --path .`
- [ ] **Step 5:** Commit + push + tag:

```bash
git add -A
git commit -m "fix: polite watcher round 4 — v0.24.4"
git push
git tag v0.24.4 && git push --tags
```

- [ ] **Step 6 (elle doğrulama — ATLA, Anil koşacak):** oturumu yeniden başlat · 11+ dosya kuyruktayken `/watch polite off` → "polite mode off — instant file feedback" onayı GÖRÜNÜR + bulk-skip bildirimi gelir · normal (küçük) kuyrukta `/watch polite off` → "delivering queued feedback" + feedback'ler.
