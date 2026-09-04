# TLS → rustls — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** `main` dalı, v0.31.1, temiz ağaç. Spec: `docs/superpowers/specs/2026-09-04-rustls-portability-design.md`; çelişkide spec kazanır. İş dalı: `git switch -c rustls-portability`.

**Goal:** `reqwest`'in TLS arka ucunu `native-tls`'ten `rustls`'e taşı; Linux'ta `libssl-dev` gereksinimini kaldır. Kullanıcıya görünen davranış değişmez. Hedef sürüm **v0.31.2**.

**Architecture:** Tek satır `Cargo.toml` değişikliği + lock tazeleme + doküman. Rust kaynak kodu DEĞİŞMEZ.

## Global Constraints

- **Varsayılan özellikler elle geri konur** (spec T1): `charset`, `http2`, `system-proxy` korunacak. Sadece `default-features = false` yazıp bırakmak HTTP/2'yi ve karakter-kümesi çözümünü sessizce düşürür.
- **Kök sertifika kaynağı `rustls-tls-native-roots`** (spec T2). `rustls-tls` (webpki kökleri) KULLANILMAZ — bugünkü davranış işletim sisteminin güven deposunu okuyor, o korunacak.
- `src/` altındaki hiçbir `.rs` dosyası değişmez. Değişiyorsa DUR ve raporla.
- CI, release, musl, durum senkronu KAPSAM DIŞI.
- Bilinen ortam hatası, düzeltilmez, engellemez: `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt`.
- `cargo clippy --all-targets` 0 uyarı, `cargo fmt --check` temiz.
- **Push / merge / tag / `cargo install` YOK. `git stash` YOK.**
- İngilizce: yorumlar ve commit mesajları.

---

### Task 1: TLS arka ucunu değiştir

**Files:** Modify `Cargo.toml`, `Cargo.lock`

- [ ] **Step 1:** `reqwest` satırını spec T1'deki hâliyle yaz.
- [ ] **Step 2:** `cargo check` ile lock'u tazele.
- [ ] **Step 3: Doğrulama — bu task'ın asıl çıktısı.** Üçünü de koş ve çıktılarını commit mesajına DEĞİL, ledger'a yaz:
  - `cargo tree -i openssl-sys` → eşleşme YOK
  - `cargo tree -i native-tls` → eşleşme YOK
  - `cargo tree -i rustls` → `reqwest` üzerinden bağlı
  Herhangi biri beklenenden farklıysa **DUR ve raporla** — başka bir bağımlılık OpenSSL çekiyor olabilir, o ayrı bir iştir.
- [ ] **Step 4:** `cargo test` tümü yeşil, `cargo clippy --all-targets` 0 uyarı, `cargo fmt --check` temiz.
- [ ] **Step 5:** Commit: `build: swap reqwest's TLS backend to rustls with native roots`

---

### Task 2: Belgeleme + sürüm

**Files:** Modify `README.md`, `SPEC.md`, `docs/ROADMAP.md`, `Cargo.toml` (0.31.2), `src/tui/welcome_tests.rs`, `Cargo.lock`

- [ ] **Step 1:** `README.md` Install bölümü: Linux'ta gereken tek şeyin Rust araç zinciri + C derleyicisi olduğu; `libssl-dev`/`openssl-devel` GEREKMEDİĞİ. Ayrıca `cargo install --git https://github.com/cursedxp/usta --locked` satırı eklenir (repo public, `Cargo.lock` depoda).
- [ ] **Step 2:** `SPEC.md` §11 Decisions'a kısa madde: TLS arka ucu rustls + native roots; gerekçe (çapraz platform kurulum, C bağımlılığı yok) ve korunan davranış (işletim sistemi güven deposu, HTTP/2, charset, system-proxy).
- [ ] **Step 3:** `docs/ROADMAP.md` `## Completed` başına kayıt, `v0.31.2.` ile biter. Elle doğrulamanın (API turu + temiz Linux kurulumu) henüz KOŞULMADIĞI açıkça yazılır.
- [ ] **Step 4:** Sürüm 0.31.2 (`Cargo.toml` + `src/tui/welcome_tests.rs` pini), `cargo check`.
- [ ] **Step 5:** `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`. Commit: `docs: README + SPEC + ROADMAP — rustls TLS backend; bump to v0.31.2`

---

## Manuel Doğrulama (Anil ile)

1. **API yolu — TLS'in tek gerçek sınavı.** `cargo install --path .` sonrası `ANTHROPIC_API_KEY` ile API arka ucuna zorlanmış tek bir tur koş. Varsayılan yol Claude CLI olduğu için bu adım atlanırsa değişiklik sınanmamış sayılır.
2. `otool -L $(which usta)` → `Security.framework` satırı kalmamalı.
3. **Linux makinede, `libssl-dev` KURMADAN:** `cargo install --git https://github.com/cursedxp/usta --locked` → derleme başarılı. Asıl hedef bu.
4. Linux'ta bir oturum aç, bir tur koş.
