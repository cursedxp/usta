# Alt bölge: göreli render (Ink modeli) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** `main` dalı, v0.29.3, temiz ağaç. Bekleyen çakışan plan YOK (resize planı v0.29.3'te geri alındı, terminology planı bitti). `docs/superpowers/specs/2026-09-01-relative-render-design.md` ağaçta OLMALI, çelişkide o kazanır — özellikle **"Karar" (K1–K5)** ve **"Mimari"**. İş dalı: `git switch -c relative-render`.

**Goal:** Canlı alt bölgeyi ratatui'nin inline viewport'undan çıkar, Claude Code'un kullandığı göreli silme + yeniden basma modeliyle kendimiz çiz. Aynı hamlede girdi çerçevesi kenarsız hale gelir (üstte/altta düz çizgi, yükseklik içerikle büyür). Böylece mutlak satır çapası → CPR → `TrackedBackend` → bayat imleç zincirinin TAMAMI ortadan kalkar. Hedef sürüm **v0.30.0**.

**Architecture — taşıyıcı karar:** İçerik üreten her şey KALIR. `welcome.rs`, `intro.rs`, `paint.rs`, `status.rs` bugün ratatui `Text<'static>` döndürüyor; imzaları DEĞİŞMEZ. Değişen yalnız **taşıma katmanı**: `Text` → ANSI satırları → `Screen` → stdout. Bu yüzden iş `src/tui/` içinde dar kalır ve her task tek başına yeşil bırakılabilir.

**Tech Stack:** Rust, crossterm 0.29 (yalnız göreli imleç + satır silme), ratatui 0.30.2 (yalnız `Text`/`Line`/`Span` veri tipleri ve mevcut widget'lar için — `Terminal`/`Backend` KULLANILMAZ). Binary crate — filtreli `cargo test <filtre>`.

## Global Constraints

- **K3 mutlak:** üretim kaynağında `MoveTo(` YOK, `cursor::position()` YOK. Yalnız `MoveUp`/`MoveDown`/`MoveToColumn`. Sütun adresi serbest (reflow'dan etkilenmez), satır adresi yasak.
- **K4:** her `paint` `Clear(ClearType::FromCursorDown)` ile biter.
- Kalıcı içerik bir kez basılır, bir daha çizilmez (K5).
- Her task: TDD — önce failing test, DOĞRU sebeple fail ettiği görülür, sonra minimal implementasyon; task sonunda `cargo test` TÜMÜ yeşil, sonra commit.
- Bilinen ortam hatası: `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt` bu makinede `pdftotext` KURULU olduğu için kırmızı. Bu planla ilgisi yok, düzeltilmez, engellemez — raporda "önceden var" diye geçilir.
- `cargo clippy --all-targets` 0 uyarı; `cargo fmt` dokunulan dosyalara scoped; sonda `cargo fmt --check` crate-geneli temiz.
- **Push / tag / `cargo install` YOK.**
- Satır bütçesi (SPEC §11, 600): `run.rs` 596 — bu plan run.rs'e satır EKLEMEMELİ, net ≤ 0 olmalı; her dokunuşta `grep -c "" src/tui/run.rs` doğrulanır.
- `src/plain.rs` DEĞİŞMEZ (kaynak ve davranış).
- Davranış regresyonu yasak: `usta start <topic>`, resume, giriş akışı, onay soruları, `/watch`, ride-along, kapanış flush'ı, görsel akış — hepsi aynı kalır. Değişen tek şey alt bölgenin ÇİZİLME biçimi ve girdi çerçevesinin görüntüsü.

---

### Task 1: `Text` → ANSI satırları

**Files:** Modify `src/tui/convert.rs` (+ testleri)

**Interfaces:** Produces `pub(crate) fn text_to_ansi_lines(t: &Text, width: u16) -> Vec<String>` — mevcut `ansi_to_text`'in tersi. Her `Line` bir `String`; `Span` stilleri `theme` ile aynı ANSI kodlarına çevrilir; her satır görüntü genişliği `width`'i aşmayacak şekilde kırpılır (unicode-width ile, ANSI kaçışları sayılmaz).

- [ ] **Step 1: Failing test.** Renkli/renksiz span karışımı bir `Text` → beklenen ANSI dizisi; genişliği aşan satırın kırpıldığı; kırpmanın ANSI kaçışını ortasından bölmediği; boş `Text` → boş vec.
- [ ] **Step 2:** Testi koş, doğru sebeple FAIL.
- [ ] **Step 3:** Implementasyon. `#[allow(dead_code)]` (Task 4'te tüketilir, orada SÖKÜLÜR).
- [ ] **Step 4:** test + clippy + fmt. Commit: `feat: Text -> ANSI line conversion for the relative renderer`

---

### Task 2: `Screen` — göreli render motoru

**Files:** Create `src/tui/screen.rs`; Modify `src/tui/mod.rs`

**Interfaces:**
```
pub(crate) struct Screen<W: Write> { … }        // painted, cursor_up, last_widths, size
pub(crate) fn paint(&mut self, lines: &[String], cursor_line: u16, cursor_col: u16) -> io::Result<()>
pub(crate) fn page(&mut self, content: &[String]) -> io::Result<()>
pub(crate) fn resize(&mut self, size: Size) -> io::Result<()>
pub(crate) fn clear_block(&mut self) -> io::Result<()>   // kapanışta
pub(crate) fn rewrapped_rows(widths: &[u16], new_width: u16, painted: u16) -> u16   // saf, ayrı test
```
Sıra spec'in "Mimari" bölümündeki adımların BİREBİR kendisidir.

- [ ] **Step 1: Failing test.** Sahte `Write` (mevcut `backend_wrap.rs` test modülündeki `SharedBuf` deseni) üzerine:
  - ilk `paint` hiç `ESC[2K` üretmez;
  - ikinci `paint` tam `painted` kez `ESC[2K` üretir;
  - her `paint` `ESC[0J` ile biter (K4);
  - hiçbir çıktı `H` ile biten mutlak konum dizisi içermez (K3 bekçisi);
  - `page` bloğu siler, içeriği `\r\n` ile basar, `painted`'ı 0'lar;
  - `rewrapped_rows`: genişlik yarıya inince ~iki katı, `painted..=painted*2` aralığına kırpılı.
- [ ] **Step 2:** FAIL, doğru sebep.
- [ ] **Step 3:** Implementasyon.
- [ ] **Step 4:** test + clippy + fmt. Commit: `feat: relative-render Screen — erase by our own line count, never by absolute row`

---

### Task 3: Kenarsız girdi çerçevesi

**Files:** Modify `src/tui/editor.rs` (+ testleri)

**Interfaces:**
- `pub(crate) const INPUT_MAX_ROWS: usize = 10`
- `pub(crate) fn content_rows(value: &str, width: u16) -> usize` — `1..=INPUT_MAX_ROWS` — v0.30.1'de silindi (bkz. `docs/superpowers/specs/2026-09-01-resize-repark-amendment-design.md`)
- `InputBox::frame_lines(&self, width: u16, screen_h: u16) -> (Vec<String>, u16, u16)` — satırlar (üst kural + içerik + alt kural) + imleç satırı + imleç sütunu
- Eski `InputBox::render(&self, f: &mut Frame, area: Rect)` KALDIRILIR. `wrap_visual` KALIR, sarma genişliği `width - 2` olur.

- [ ] **Step 1: Failing test.** İlk ve son satır tam genişlikte `─` kuralı · yan kenar karakteri (`│`, `╭`, `╰`) hiçbir satırda YOK · boş girdide 3 satır (kural + tek içerik + kural) · sarma `width - 2`'de · tavan aşılınca içerik kayar, satır sayısı sabitlenir · imleç satırı `1..=content_rows` aralığında.
- [ ] **Step 2:** FAIL, doğru sebep.
- [ ] **Step 3:** Implementasyon; `Block`/`Borders`/`BorderType` importları düşer.
- [ ] **Step 4:** Kaynak-pin: `editor.rs` üretim kaynağında `Borders::ALL` ve `BorderType::Rounded` YOK.
- [ ] **Step 5:** test + clippy + fmt. Commit: `feat: borderless input frame — rules above and below, height grows with content`

---

### Task 4: Taşıma katmanını değiştir

**Files:** Modify `src/tui/term.rs`, `src/tui/page.rs`, `src/tui/run.rs`, `src/tui/ask.rs`, `src/tui/entry.rs`, `src/tui/intro.rs`

**Interfaces:**
- `Tui { screen: Screen<Stdout> }` — `terminal` alanı gider. `setup()` ham mod + bracketed paste + kitty ayarlarını AYNEN yapar, CPR tohumu KALKAR.
- `page::draw` ve `page::page*` aileleri imzalarını KORUR, gövdeleri `Screen`'e yönlenir (`Text` → `text_to_ansi_lines` → `screen.page`/`paint`).
- `page::handle_resize` gövdesi tek satır: `tui.screen.resize(size)`.
- `Tui::drop` `screen.clear_block()` + `restore()` yapar.
- Çağrı yerleri (run/ask/entry/intro) imza değişmediği için DOKUNULMAZ — dokunulması gerekiyorsa run.rs'te net satır artışı 0 olmalı.

- [ ] **Step 1:** Önce mevcut pin testlerini oku ve hangilerinin ısırdığını not et (`resize_events_are_handled_in_every_event_loop`, `cpr_seed_happens_before_event_stream`, `run_rs_wiring_call_sites_are_pinned`). İmzası değişen her iğne, ISIRAN eşdeğeriyle değiştirilir — gevşetilmez.
- [ ] **Step 2: Failing test.** Yeni pin: `term.rs` üretim kaynağında `Viewport::Inline` YOK, `cursor::position()` YOK; `page.rs`'te `insert_before` YOK, `MoveTo(` YOK.
- [ ] **Step 3:** FAIL, doğru sebep.
- [ ] **Step 4:** Implementasyon. `Task 1`/`Task 2`/`Task 3`'ün `#[allow(dead_code)]`'ları SÖKÜLÜR.
- [ ] **Step 5:** Pin doğrulaması (protokol): `page::draw` çağrısını run.rs'te geçici yorum satırı yap → ilgili pin FAIL etsin (derleme hatası da ısırma sayılır) → geri al.
- [ ] **Step 6:** `grep -c "" src/tui/run.rs` ≤ 596. test + clippy + fmt. Commit: `refactor: route the live bottom region through Screen, drop the inline viewport`

---

### Task 5: Ölü yamayı sök

**Files:** Delete `src/tui/backend_wrap.rs`; Modify `src/tui/mod.rs`, `src/tui/term.rs`

**Interfaces:** `TrackedBackend`, `fallback_seed`, `clamp_to_screen`, `advanced_by_lines` ve `term::VIEWPORT_H` KALDIRILIR. Bunlar CPR yamasının parçalarıydı; K1 ile varlık sebepleri bitti.

- [ ] **Step 1:** Silmeden ÖNCE `cpr_seed_happens_before_event_stream` testinin yeni hâlini yaz: artık "tek CPR setup'ta" değil, **"üretim kaynağında hiç CPR yok"** demeli — tüm `src/tui/*.rs` üzerinde `cursor::position()` sayısı 0.
- [ ] **Step 2:** FAIL (setup hâlâ CPR yapıyorsa) veya Task 4'ten sonra doğrudan yeşil — hangisi olursa olsun testin ISIRDIĞI, geçici bir `cursor::position()` ekleyip kaldırarak doğrulanır.
- [ ] **Step 3:** Dosyayı sil, kalan referansları temizle.
- [ ] **Step 4:** test + clippy + fmt. Commit: `refactor: remove TrackedBackend — the CPR workaround has no reason to exist`

---

### Task 6: Belgeleme + sürüm

**Files:** Modify `SPEC.md` (§4.19), `docs/ROADMAP.md`, `README.md` (Terminal UI satırı), `Cargo.toml` (0.30.0), `src/tui/welcome_tests.rs` (sürüm pini), `Cargo.lock`

- [ ] **Step 1:** `SPEC.md` §4.19'a İngilizce paragraf: neyin değiştiği (alt bölge artık göreli render, inline viewport yok) · neden (mutlak çapa → CPR → TrackedBackend → bayat imleç zinciri; v0.29.1'in bu zincirde battığı nokta) · Claude Code kanıtı (Ink + Yoga + ansi-escapes, `handleResize` sadece yeniden render eder, `eraseLines` göreli) · K1–K5 · kalan risk (terminale göre değişen reflow davranışı, `rewrapped_rows` kırpması) · tasarım dosyası yolu. v0.24.6/v0.26.0/v0.29.1 paragrafları SİLİNMEZ, bu paragraf onların üstüne "artık bu yol tamamen değişti" notunu koyar.
- [ ] **Step 2:** `docs/ROADMAP.md` `## Completed` başına kayıt, `v0.30.0.` ile biter.
- [ ] **Step 3:** `README.md` Terminal UI satırındaki "live four-sided input box" ifadesi gerçeğe çekilir.
- [ ] **Step 4:** Sürüm 0.30.0 (`Cargo.toml` + `welcome_tests.rs` pini), `cargo check` ile lock tazele.
- [ ] **Step 5:** `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`. Commit: `docs: SPEC 4.19 + ROADMAP + README — relative render; bump to v0.30.0`

---

## Manuel Doğrulama (Anil ile, plan bittikten sonra)

`cargo install --path .` sonrası:

1. Boş girdi → üstte ve altta düz çizgi, yan kenar yok, alt bölge 4 satır.
2. Uzun metin yaz → alt çizgi bir satır aşağı kayar, üstteki transcript yukarı iter. Ctrl+J ile elle satır atla → aynı.
3. Tavanı aş (10+ satır) → içerik kayar, çerçeve büyümeyi durdurur.
4. **Genişlet / daralt / kenardan yavaşça sürükle** → kalıntı YOK ve ekrandaki metin kaybolmuyor. Asıl test bu.
5. Mentor yanıtı beklerken (spinner) resize → aynı.
6. Giriş akışı, onay sorusu, `/show` görsel akışı, `/context` → hepsi normal basıyor, blok bozulmuyor.
7. Uzun oturum: yukarı kaydır → transcript bütün, kopyalanabilir.
