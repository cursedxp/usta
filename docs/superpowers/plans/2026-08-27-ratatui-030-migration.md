# ratatui 0.30 Migration Implementation Plan (v0.26.0)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** v0.25.1 üstüne. Spec: `docs/superpowers/specs/2026-08-27-ratatui-030-migration-design.md` — önce TAMAMINI oku.

**Goal:** ratatui 0.29.0 → 0.30.2; crossterm ağacı tek sürümde birleşir; yatay resize düzelir; görünür davranış birebir parite. v0.26.0.

**Architecture:** Saf bağımlılık migration'ı — davranış değişikliği yok. Kaynak gerçeği: ratatui `BREAKING-CHANGES.md` (WebFetch ile okunur) + derleyici hataları. Ezberden API uydurmak YASAK (v0.24.2 MSRV dersi: ölç, uydurma).

**Tech Stack:** Rust, ratatui 0.30.2, crossterm (0.30'un gerektirdiği sürüm), tui-input, termimad.

## Global Constraints

- TÜM yeni kod/yorum/commit İNGİLİZCE. `run.rs` ≤600 production satır.
- Davranış paritesi: mevcut testlerin davranış assert'leri DEĞİŞMEZ (yalnız API adı/imza değiştiyse mekanik uyarlama — reviewer'a tek tek gerekçelenir).
- Her task sonunda `cargo build && cargo test` yeşil, clippy yeni uyarı 0, fmt dokunulan dosyalara, İngilizce commit + push.
- Manifest'teki crossterm-tekillik ilkesi korunur: iş sonunda `cargo tree -i crossterm` TEK sürüm göstermeli; pin yorumu yeni duruma göre yeniden yazılır.

---

### Task 1: Migration keşfi (read-only — kod değişikliği YOK)

**Files:** yok (rapor üretir; bulgular Task 2'nin girdisi — `.superpowers/sdd/` altına not düşülebilir)

- [ ] **Step 1:** WebFetch: `https://raw.githubusercontent.com/ratatui/ratatui/main/BREAKING-CHANGES.md` → 0.30.0/0.30.1/0.30.2 bölümlerini çıkar (modül taşınmaları, yeniden adlandırmalar, Terminal/Viewport/backend API değişimleri, crossterm re-export durumu).
- [ ] **Step 2:** Sürüm matrisi: ratatui 0.30.2'nin crossterm gereksinimi (crates.io/docs.rs) · `tui-input`'un ratatui-0.30 uyumlu sürümü · `termimad`/`coolor`/`crokey` zincirinin crossterm hedefi. Hepsi TEK crossterm sürümünde buluşabiliyor mu — hangi sürümde?
- [ ] **Step 3:** `grep -rn "ratatui::\|crossterm::" src/ | wc -l` + kaba etki listesi: hangi dosyada hangi API'ler (Terminal::with_options, Viewport::Inline, insert_before, Frame, Layout, Span/Line/Style, EventStream, execute!, raw mode…).
- [ ] **Step 4:** ratatui 0.30 MSRV'sini kaydet. Rapor: (a) değişecek Cargo.toml satırları, (b) beklenen kırılma listesi dosya-dosya, (c) `handle_resize` için 0.30'un resize semantiği ne diyor (PR #2355 sonrası `clear`/`autoresize` davranışı — `page::handle_resize` sadeleşecek mi).

---

### Task 2: Bağımlılık bump + derleme migration'ı

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock`, `src/tui/*.rs`, `src/ui.rs` (Task 1 raporundaki etki listesi)

- [ ] **Step 1:** Cargo.toml: `ratatui = "0.30"` + Task 1 matrisindeki eşlik eden sürümler (crossterm, tui-input, termimad zinciri). Manifest'teki crossterm pin YORUMU yeni gerçeğe göre yeniden yazılır (İngilizce ya da mevcut dil neyse — dokunulan yorumun diline uy; yeni yorum İngilizce).
- [ ] **Step 2:** `cargo build` → hata listesi. Hataları DOSYA DOSYA, derleyici önerisi + BREAKING-CHANGES referansıyla mekanik düzelt. Her düzeltme davranış-nötr olmalı; emin olunmayan yerde durup Task 1 raporuna/dokümana bak, uydurma.
- [ ] **Step 3:** `cargo test` → davranış assert'leri DEĞİŞMEDEN tümü PASS. API-adı kaynaklı zorunlu test uyarlamaları ayrı listelenir (reviewer için).
- [ ] **Step 4:** `cargo tree -i crossterm` → TEK sürüm; çıktı commit mesajına/rapora. `cargo metadata --locked` ile bağımlılık MSRV tabanı yeniden ölçülür → `rust-version` gerekiyorsa güncellenir (ölçüm değeri, tahmin değil).
- [ ] **Step 5:** clippy 0 · fmt · `wc -l src/tui/run.rs` ≤600. Commit + push: `chore: migrate to ratatui 0.30.2 — unified crossterm tree`

---

### Task 3: Resize semantiği + workaround mutabakatı

**Files:**
- Modify: `src/tui/page.rs` (`handle_resize` gövdesi — gerekiyorsa), pin testleri (yalnız gerekiyorsa)

- [ ] **Step 1:** Task 1 raporundaki 0.30 resize semantiğine göre karar: upstream fix (PR #2355) `clear`/draw yolunda temizliği kendisi yapıyorsa `handle_resize` sadeleştirilir (örn. yalnız `autoresize` veya boş + redraw); yapmıyorsa mevcut gövde kalır. Karar tek satır İngilizce yorumla gerekçelenir (`// ratatui 0.30 clears on resize (PR #2355); ...`).
- [ ] **Step 2:** `Event::Resize` yakalama noktaları (run/ask_live/confirm/entry) ve `resize_events_are_handled_in_every_event_loop` pin testi AYNEN kalır — davranış sözleşmesi değişmiyor.
- [ ] **Step 3:** `cargo build && cargo test` → PASS. Commit + push: `fix: reconcile resize handling with ratatui 0.30 semantics`

---

### Task 4: Docs + v0.26.0 release

**Files:** `SPEC.md` (§4.19 Resize paragrafına 0.30 notu + §11 gerekiyorsa), `README.md` (yalnız sürüm/gereksinim değiştiyse), `Cargo.toml`, `Cargo.lock`, sürüm testi (grep `0.25.1` src/)

- [ ] **Step 1:** SPEC §4.19 Resize paragrafına ek cümle: v0.26.0 — ratatui 0.30.2; horizontal resize fixed upstream (issue #2086 / PR #2355); 0.29 workaround reconciled.
- [ ] **Step 2:** Cargo.toml `0.26.0`; sürüm testi güncelle.
- [ ] **Step 3:** Verify: `cargo build && cargo test` PASS · clippy 0 · `cargo install --path .`
- [ ] **Step 4:** Commit + push + tag:

```bash
git add -A
git commit -m "chore: ratatui 0.30 migration — v0.26.0"
git push
git tag v0.26.0 && git push --tags
```

- [ ] **Step 5 (elle doğrulama — ATLA, Anil koşacak; KAPANIŞ KRİTERİ):** oturum aç → pencereyi YATAY daralt/genişlet (birkaç kez, uçlara) → alt bölge tek kopya, bozulma yok · dikey resize · spinner dönerken resize · scrollback okunabilir · renk/görünüm eskisiyle aynı.
