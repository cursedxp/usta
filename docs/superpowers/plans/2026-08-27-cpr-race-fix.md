# CPR Race Fix Implementation Plan (v0.26.1)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** v0.26.0 üstüne. Spec: `docs/superpowers/specs/2026-08-27-cpr-race-fix-design.md` — TAMAMINI oku. Migration bağlamı: `.superpowers/sdd/rat30-task-1-report.md` + progress.md (vendored kaynak yolları orada).

**Goal:** `TrackedBackend` sarmalayıcısı: imleç konumu takip edilir, `get_cursor_position()` stdin'e sormadan tracked değeri döner; gerçek CPR yalnız setup'ta (EventStream öncesi) bir kez. Canlıdaki "cursor position could not be read" ölümü kökten biter. v0.26.1.

**Architecture:** Yeni `src/tui/backend_wrap.rs` (delegasyon + takip), `term.rs`'te kuruluma tohumlama, `Tui.terminal` tipi güncellenir. Kaynak gerçeği: `~/.cargo/registry/src/*/ratatui-core-0.1.2/` + `ratatui-crossterm`/crossterm 0.29 vendored kaynağı — ezber API YASAK.

## Global Constraints

- TÜM yeni kod/yorum/commit İNGİLİZCE. `run.rs` ≤600 production satır. TDD.
- Parite: mevcut testlerin davranış assert'leri değişmez; görünür TUI davranışı birebir.
- Her task sonunda build+test yeşil, clippy 0, fmt dokunulan dosyalara, İngilizce commit + push.
- Keşif kararları rapora yazılır; "muhtemelen"le implementasyon YASAK (spec'in doğruluk ölçütü).

---

### Task 1: Keşif (read-only) — CPR çağrı haritası + Backend trait yüzeyi

**Files:** rapor: `.superpowers/sdd/cpr-task-1-report.md`

- [ ] **Step 1:** Vendored ratatui-core 0.1.2'de `get_cursor_position` çağrılarının TAM listesi (dosya:satır) + her çağrının değeri nasıl kullandığı (inline kurulum `compute_inline_size`, `clear`, `insert_before`, `resize`, draw yolu). Usta'nın hangi operasyonları bu yollara giriyor (page/insert_before, draw/autoresize, setup) — eşleme tablosu.
- [ ] **Step 2:** `ratatui::backend::Backend` trait'inin (0.30 gerçek imzaları) tam metot listesi + hangileri imleci hareket ettirir (`draw`, `set_cursor_position`, `append_lines`, `clear_region`, `flush`…) — vendored kaynaktan, tablo: metot → tracked konuma etkisi.
- [ ] **Step 3:** `CrosstermBackend`'in generic yazımı ve `Terminal<B>` tip zinciri: `Tui` tipi `Terminal<TrackedBackend>` olduğunda etkilenen imzalar (`term.rs`, `page.rs`, testler). `TestBackend` benzeri test stratejisi: TrackedBackend'i `std::io::Write` sahte yazarla kurmak mümkün mü (CrosstermBackend<W: Write> generic mi?) — test planını yaz.
- [ ] **Step 4:** Tohum değeri: setup anında (EventStream YOK) gerçek `crossterm::cursor::position()` güvenli mi; başarısızlıkta güvenli varsayılan ne olmalı (inline kurulumun beklediği anlam: viewport çapası — kaynaktan türet). Rapor: implementasyon kararları listesi + kaçış kapısı gereken operasyon var mı.

---

### Task 2: `TrackedBackend` (`src/tui/backend_wrap.rs`) — TDD

**Files:**
- Create: `src/tui/backend_wrap.rs` (testler in-module) · Modify: `src/tui/mod.rs` (kayıt)

**Interfaces (Produces):**
- `pub(crate) struct TrackedBackend { inner: CrosstermBackend<Stdout>, cursor: Position }` (gerçek alan adları Task 1 raporuna göre)
- `pub(crate) fn new_seeded(inner: CrosstermBackend<Stdout>, seed: Position) -> TrackedBackend`
- `impl Backend for TrackedBackend` — tüm metotlar delege; imleci etkileyenler `cursor`'u Task 1 tablosuna göre günceller; `get_cursor_position()` → `Ok(self.cursor)` (stdin sorgusu YOK).

- [ ] **Step 1: Failing testler** — Task 1'in test stratejisine göre (generic yazar mümkünse sahte-yazarla; değilse mantık saf yardımcılara çekilip test edilir):

```rust
#[test]
fn get_cursor_returns_seed_without_querying() { /* seed → get == seed */ }

#[test]
fn set_cursor_position_updates_tracked_state() { /* set(x,y) → get == (x,y) */ }

#[test]
fn cursor_moving_ops_update_tracked_state() {
    // Task 1 tablosundaki her imleç-etkileyen metot sınıfı için bir assert
    // (örn. append_lines n satır → row += n, clamp terminal yüksekliğine — gerçek semantik kaynaktan)
}
```

- [ ] **Step 2:** FAIL gör → implement → PASS. Delegasyon eksiksiz (trait'in TÜM metotları — derleyici zorlar).
- [ ] **Step 3:** Commit + push: `feat: TrackedBackend — answer cursor queries from tracked state`

---

### Task 3: Kablolama — setup tohumu + tip geçişi

**Files:**
- Modify: `src/tui/term.rs` (setup: CPR tohumu EventStream'den önce + `Tui { terminal: Terminal<TrackedBackend> }`), etkilenen imzalar (`page.rs` vb. — Task 1 raporundaki liste)

- [ ] **Step 1:** `setup()`: raw mode sonrası, Terminal kurulmadan önce `crossterm::cursor::position()` bir kez → başarısızsa Task 1'in güvenli varsayılanı (warn YOK — sessiz düşüş, açılış ölmez). `CrosstermBackend::new(stdout)` → `TrackedBackend::new_seeded(...)` → `Terminal::with_options(...)`. Kurulumun kendisi artık CPR sormaz mı — Task 1 raporuna göre: `compute_inline_size` kurulumda `get_cursor_position` çağırıyorsa artık TrackedBackend cevaplıyor (stdin'e gitmez) ✓.
- [ ] **Step 2: Sıra pin testi** (term.rs veya page.rs test bloğu):

```rust
#[test]
fn cpr_seed_happens_before_event_stream() {
    // Source pin: the ONLY real CPR happens in setup(), before any EventStream
    // exists (run.rs creates EventStream after setup). Guard both halves.
    let term_src = include_str!("term.rs");
    let prod = term_src.split("#[cfg(test)]").next().unwrap();
    assert!(prod.contains("cursor::position()"));
    assert!(prod.contains("TrackedBackend"));
    let run_src = include_str!("run.rs");
    let setup_at = run_src.find("term::setup").expect("run.rs calls setup");
    let stream_at = run_src.find("EventStream::new").expect("run.rs builds the stream");
    assert!(setup_at < stream_at, "EventStream must be created after setup's CPR seed");
}
```

- [ ] **Step 3:** `cargo build && cargo test` → TÜMÜ PASS (parite: davranış assert'leri değişmedi) · clippy 0 · `wc -l src/tui/run.rs` ≤600.
- [ ] **Step 4:** Commit + push: `fix: seed tracked cursor before the event stream — kill the CPR race`

---

### Task 4: Docs + v0.26.1 release

**Files:** `SPEC.md` (§4.19 Resize/TUI bölümüne CPR paragrafı), `Cargo.toml`, `Cargo.lock`, sürüm testi (grep `0.26.0` src/)

- [ ] **Step 1:** SPEC §4.19'a kısa paragraf: **CPR race (v0.26.1)** — upstream #2640 açık; TrackedBackend answers cursor queries from tracked state, the single real CPR runs in setup before the EventStream; fatal "cursor position could not be read" class eliminated.
- [ ] **Step 2:** Cargo.toml `0.26.1`; sürüm testi güncelle.
- [ ] **Step 3:** Verify: build+test PASS · clippy 0 · `cargo install --path .`
- [ ] **Step 4:** Commit + push + tag:

```bash
git add -A
git commit -m "fix: eliminate CPR/stdin race — v0.26.1"
git push
git tag v0.26.1 && git push --tags
```

- [ ] **Step 5 (elle doğrulama — ATLA, Anil koşacak; KAPANIŞ KRİTERİ):** çökme senaryosu (açılış → boş Enter → öneri) 5-6 tekrar → CPR hatası yok · yatay/dikey resize düzgün · viewport konumu doğru (alt bölge yerinde, kayma yok) · normal ders akışı.
