# Resize Sonrası Hayalet Çerçeveler — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** `main` dalı, v0.29.0, temiz ağaç. Bekleyen/çakışan başka plan YOK (`2026-08-29-session-awareness.md` tamamlandı, `9eb3983`). `docs/superpowers/specs/2026-08-31-resize-ghost-frames-design.md` ağaçta OLMALI ve çelişkide o kazanır — özellikle "Kök neden", **"Karar" (K1/K2/K3)** ve "İsimlendirme (bağlayıcı)". İş dalını main'den aç (`git switch -c resize-ghost-frames`).

**Goal:** Terminal yeniden boyutlandırılınca ekranda kalan hayalet kutu kalıntılarını bitir — **ekrandaki metni SİLMEDEN**. Kök neden: `TrackedBackend`'in izlediği imleç terminalin kendi reflow'unda bayatlıyor, ratatui yeni viewport'u o bayat satırdan hesaplıyor, inline temizlik ise yalnız yeni tepenin AŞAĞISINI siliyor. Çözüm: imlecin çerçeve içindeki ofseti reflow'dan etkilenmediği için eski çerçeve GÖRELİ hareketlerle tam olarak silinir, sonra imleç mutlak `MoveTo` ile bilinen bir yere KONUR ve inline viewport oradan yeniden kurulur. Hedef sürüm **v0.29.1**.

**Architecture:** Değişiklik üç dosyada: `src/tui/term.rs` (`Tui::last_size` alanı + `setup()`'tan `rebuild_inline` çıkarımı), `src/tui/page.rs` (`handle_resize` gövdesi + iki saf yardımcı), test pinleri. `src/tui/backend_wrap.rs` DEĞİŞMEZ. Çağrı yerleri (run.rs, ask.rs ×2, entry.rs) DEĞİŞMEZ. Ham imleç/silme dizileri `crossterm::execute!(std::io::stdout(), …)` ile yazılır — `term.rs`'in mevcut deseni.

**Tech Stack:** Rust, ratatui 0.30.2 / ratatui-core 0.1.2 (`Terminal::with_options`, `get_cursor_position`, `get_frame().area()`, `size()` — hepsi public), crossterm 0.29 (`cursor::{MoveUp, MoveDown, MoveTo, MoveToColumn}`, `terminal::{Clear, ClearType}`). Binary crate — filtreli `cargo test <filtre>`.

## Global Constraints

- TÜM tanımlayıcılar, kullanıcıya dönük string'ler, kod yorumları ve commit mesajları İNGİLİZCE. Türkçe yalnız spec/plan düzyazısında.
- **K1 MUTLAK:** hiçbir yeni kod `ClearType::All` YAZMAZ ve `Terminal::resize`/`autoresize` ÇAĞIRMAZ (ikisi de yatay daralmada tüm ekranı siler — spec "Neden `Terminal::resize()` değil"). Kullanıcının ekrandaki metni korunur.
- **CPR yasağı korunur (v0.26.1):** yeni kodun hiçbir yerinde `crossterm::cursor::position()` YOK. `term.rs`'teki `cpr_seed_happens_before_event_stream` testi gevşetilmez, yalnız `rebuild_inline`'ı da kapsayacak şekilde genişletilir; `cursor::position()` sayısı üretim kaynağında 1 kalır.
- Her task: TDD — önce failing test, DOĞRU sebeple fail ettiği görülür, sonra minimal implementasyon; task sonunda `cargo test` TÜMÜ yeşil, sonra commit.
- `cargo clippy --all-targets` her task sonunda **0 uyarı**.
- `cargo fmt` yalnız dokunulan dosyalara scoped (`cargo fmt -- <dosyalar>`); son task'ta `cargo fmt --check` crate-genelinde temiz.
- **Push / tag / `cargo install` YOK** — manuel doğrulama sonrası insanın kararı. Plan yalnız lokal commit üretir.
- Satır bütçesi (SPEC §11, 600): `page.rs` 140 → ≈200, `term.rs` 120 → ≈140. `run.rs` (596) DOKUNULMAZ.
- Davranış regresyonu yasak: `Event::Resize` kolları dört döngüde de yerinde kalır; `page::draw`, `page::page`, `insert_before`, `Tui::drop` akışı değişmez; `src/plain.rs` DEĞİŞMEZ.
- Mevcut pin testi `resize_events_are_handled_in_every_event_loop` gevşetilmez, yalnız genişletilir.

---

### Task 1: Saf yardımcılar — `anchor_row`, `erase_plan`, `size_changed`

**Files:**
- Modify: `src/tui/page.rs` (üç saf fonksiyon + testleri)

**Interfaces:**
- Produces: `fn anchor_row(height: u16) -> u16` = `height.saturating_sub(VIEWPORT_H)`.
- Produces: `struct ErasePlan { up: u16, rows: u16 }` + `fn erase_plan(off: u16) -> ErasePlan` = `{ up: off, rows: VIEWPORT_H }`.
- Produces: `fn size_changed(prev: Size, now: Size) -> bool` = `prev != now`.
- Üçü de bu task'ta çağrılmıyor → `#[allow(dead_code)]` alır ve **Task 3'te SÖKÜLÜR**.

- [ ] **Step 1: Failing test.** `page.rs` test modülüne ekle:

```rust
#[test]
fn anchor_row_puts_the_viewport_at_the_bottom_and_saturates() {
    // The tracked cursor goes stale when the terminal reflows on resize, so
    // handle_resize stops asking where it is and puts it somewhere known.
    // Seeding at h - VIEWPORT_H makes compute_inline_size land the viewport on
    // the last VIEWPORT_H rows without appending (and therefore scrolling) a
    // single line.
    assert_eq!(anchor_row(30), 30 - VIEWPORT_H);
    assert_eq!(anchor_row(VIEWPORT_H), 0);
    assert_eq!(anchor_row(4), 0, "a short screen must not underflow");
}

#[test]
fn erase_plan_walks_up_by_the_cursor_offset_and_erases_the_whole_frame() {
    // The offset of the cursor WITHIN the frame survives a reflow: the terminal
    // moves the cursor together with the content it sits in. That makes a
    // relative walk exact where an absolute row is not.
    let p = erase_plan(2);
    assert_eq!(p.up, 2);
    assert_eq!(p.rows, VIEWPORT_H);
    assert_eq!(erase_plan(0).up, 0, "no MoveUp when the cursor is on the top row");
}

#[test]
fn size_changed_is_false_for_an_identical_size() {
    // Drag-resizing emits a burst of Resize events; rebuilding on every one of
    // them would strobe.
    assert!(!size_changed(Size::new(80, 24), Size::new(80, 24)));
    assert!(size_changed(Size::new(80, 24), Size::new(81, 24)));
    assert!(size_changed(Size::new(80, 24), Size::new(80, 25)));
}
```

- [ ] **Step 2:** `cargo test anchor_row` → derleme hatası (fonksiyonlar yok). Doğru sebep.
- [ ] **Step 3:** Üç öğeyi `page.rs` üretim kısmına yaz (`handle_resize`'ın hemen üstüne), `#[allow(dead_code)]` ile. Import: `ratatui::layout::Size`, `crate::tui::term::VIEWPORT_H` (zaten import edilmiş).
- [ ] **Step 4:** `cargo test`, `cargo clippy --all-targets`, `cargo fmt -- src/tui/page.rs`. Commit: `feat: resize anchor + erase-plan primitives`

---

### Task 2: `term::rebuild_inline` + `Tui::last_size`

**Files:**
- Modify: `src/tui/term.rs`

**Interfaces:**
- Produces: `pub(crate) fn rebuild_inline(seed: Position) -> Result<Terminal<TrackedBackend<Stdout>>>` — `Terminal::with_options(TrackedBackend::new(CrosstermBackend::new(std::io::stdout()), seed), TerminalOptions { viewport: Viewport::Inline(VIEWPORT_H) })`. **CPR YOK** — tohum parametreden gelir.
- Modify: `setup()` — CPR tohumunu hesaplamaya devam eder (tek gerçek CPR, `EventStream`'den önce), sonra `rebuild_inline(seed)` çağırır. Davranış birebir aynı.
- Produces: `pub struct Tui { pub terminal: …, pub last_size: Size }`; `setup()` sonunda `let last_size = terminal.size()?;`.

- [ ] **Step 1: Failing test.** `term.rs` test modülünde mevcut `cpr_seed_happens_before_event_stream`'i genişlet — üretim kaynağında `rebuild_inline` tanımı bulunduğunu ve `cursor::position()` sayısının hâlâ **tam 1** olduğunu assert et:

```rust
assert!(
    prod.contains("fn rebuild_inline("),
    "term.rs lost the seed-parameterised inline viewport builder"
);
```

- [ ] **Step 2:** `cargo test cpr_seed` → FAIL (fonksiyon yok). Doğru sebep.
- [ ] **Step 3:** `rebuild_inline`'ı yaz; `setup()`'u onu kullanacak şekilde sadeleştir; `Tui`'ye `last_size` ekle. `Tui`'nin `Drop`'u DEĞİŞMEZ.
- [ ] **Step 4:** `cargo test` TÜMÜ, `cargo clippy --all-targets`, `cargo fmt -- src/tui/term.rs`. Commit: `refactor: extract term::rebuild_inline, track last_size on Tui`

---

### Task 3: Yeni `handle_resize` — göreli silme + mutlak çapa + yeniden kurulum

**Files:**
- Modify: `src/tui/page.rs` (`handle_resize` gövdesi, `#[allow(dead_code)]` sökülür, yeni pin testi)

**Interfaces:**
- Consumes: Task 1'in `anchor_row`/`erase_plan`/`size_changed`'i, Task 2'nin `term::rebuild_inline`'ı.
- `handle_resize` imzası DEĞİŞMEZ.

Gövde sırası (spec "Davranış"):
1. `let size = tui.terminal.size()?; if !size_changed(tui.last_size, size) { return Ok(()) }`
2. `let off = tui.terminal.get_cursor_position()?.y.saturating_sub(tui.terminal.get_frame().area().y);`
3. Göreli silme: `execute!(stdout(), MoveUp(off))` (yalnız `off > 0` ise), `MoveToColumn(0)`, sonra `rows` kez `Clear(ClearType::CurrentLine)` + (son hariç) `MoveDown(1)`.
4. `execute!(stdout(), MoveTo(0, anchor_row(size.height)))`
5. `tui.terminal = crate::tui::term::rebuild_inline(Position { x: 0, y: anchor_row(size.height) })?;`
6. `tui.last_size = size;`

Doküman yorumu neden-odaklı olsun: bayat izlenen imleç · ofsetin reflow-değişmezliği · `Terminal::resize`'ın neden kullanılamadığı (yatay daralmada tüm ekranı siler).

- [ ] **Step 1: Failing test.** `page.rs`'te mevcut `resize_events_are_handled_in_every_event_loop`'un ALTINA:

```rust
#[test]
fn handle_resize_erases_only_its_own_frame() {
    // Ghost frames (v0.29.0): after a width change the terminal reflows, the
    // tracked cursor goes stale, and ratatui anchored the new viewport at a
    // wrong row — inline clear_viewport only erases DOWNWARD from that row, so
    // the old frame's top rows survived. The fix walks UP by the (reflow-proof)
    // cursor offset and erases exactly VIEWPORT_H lines.
    //
    // The negative needles are the point: clearing the whole screen or calling
    // Terminal::resize (which force-clears on a horizontal shrink) would take
    // the user's transcript down with the ghost. That is not a fix.
    let prod = include_str!("page.rs").split("#[cfg(test)]").next().unwrap();
    let body = prod
        .split("fn handle_resize")
        .nth(1)
        .expect("page.rs lost its handle_resize helper");
    assert!(body.contains("ClearType::CurrentLine"));
    assert!(body.contains("MoveUp"));
    assert!(body.contains("rebuild_inline"));
    assert!(!body.contains("ClearType::All"), "handle_resize must never wipe the screen");
    assert!(!body.contains("autoresize"), "autoresize routes into the screen-clearing resize path");
    assert!(!body.contains(".resize("), "Terminal::resize force-clears on horizontal shrink");
}
```

- [ ] **Step 2:** `cargo test handle_resize_erases` → FAIL (mevcut gövde `autoresize()` çağırıyor). Doğru sebep.
- [ ] **Step 3:** Gövdeyi yaz. Import: `crossterm::cursor::{MoveDown, MoveTo, MoveToColumn, MoveUp}`, `crossterm::terminal::{Clear, ClearType}`, `ratatui::layout::Position`. `#[allow(dead_code)]`'lar sökülür.
- [ ] **Step 4:** Pin doğrulaması (protokol): `handle_resize` içindeki `rebuild_inline` satırını geçici yorum satırı yap → yeni testin FAIL ettiğini gör (derleme hatası da ısırma sayılır) → geri al.
- [ ] **Step 5:** `cargo test` TÜMÜ (özellikle `cpr_seed_happens_before_event_stream` yeşil), `cargo clippy --all-targets`, `cargo fmt -- src/tui/page.rs`. Satır bütçesi: `grep -c "" src/tui/page.rs src/tui/term.rs`. Commit: `fix: erase only usta's own frame on resize — transcript stays on screen`

---

### Task 4: Belgeleme + sürüm

**Files:**
- Modify: `SPEC.md` (§4.19 bloku, v0.26.2 notunun ALTINA yeni paragraf)
- Modify: `docs/ROADMAP.md` (`## Completed` listesinin BAŞINA tarihli kayıt)
- Modify: `Cargo.toml` (`version = "0.29.1"`), `Cargo.lock` (`cargo check` ile tazelenir)
- Modify: `README.md` — YALNIZ sürüm/changelog satırı varsa

- [ ] **Step 1:** `SPEC.md`'e İngilizce paragraf: v0.29.1'in ne düzelttiği (hayalet çerçeveler) · kök nedenin üç katmanı (bayat izlenen imleç · `compute_inline_size`'ın onu okuması · `clear_viewport`'un yalnız aşağı silmesi) · kararın özü (imlecin çerçeve içindeki ofseti reflow-değişmezdir → göreli silme; imleç sorulmaz, mutlak `MoveTo` ile konur; viewport `with_options` ile yeniden kurulur) · **açık kısıt: `Terminal::resize`/`autoresize` bu yolda kullanılamaz, yatay daralmada tüm ekranı siler ve kullanıcının transcript'ini götürür** · kabul edilen kozmetik boşluk · tasarım dosyası yolu.
- [ ] **Step 2:** `docs/ROADMAP.md` `## Completed` listesinin en başına tek paragraflık kayıt (mevcut biçim: `- 2026-08-31: <başlık> — <ne değişti, neden>. Design: <spec yolu>. v0.29.1.`). v0.24.6 ve v0.26.0 kayıtlarının bu semptomu kapatmadığını açıkça yaz — o iki kayıt bugün yanıltıcı duruyor.
- [ ] **Step 3:** `Cargo.toml` sürüm 0.29.1, `cargo check` ile lock tazele.
- [ ] **Step 4:** `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check` (crate-geneli temiz). Commit: `docs: SPEC 4.19 + ROADMAP — resize ghost-frame fix; bump to v0.29.1`

---

## Manuel Doğrulama (Anil ile, plan bittikten sonra)

`cargo install --path .` sonrası, gerçek terminalde:

1. Ekranda uzun bir mentor yanıtı dururken pencereyi **genişlet** → **yanıt yerinde duruyor**, tek kutu altta, hiçbir kalıntı yok.
2. **Daralt** → aynı; metin kaybı yok, kutu tepeye yapışmıyor.
3. Kenardan **yavaşça sürükle** → sürükleme bitince tek temiz kutu, kalıntı yok.
4. Spinner dönerken resize → aynı; yanıt geldiğinde doğru genişlikte sarılıyor.
5. Giriş akışında (`entry.rs`, konu kilitlenmeden) ve onay sorusunda (`ask.rs` confirm) resize → aynı.
6. Resize sonrası çok satırlı uzun mesaj yaz (kutu büyür) → kutu doğru büyüyor, durum satırı altta.
7. Terminali çok kısaltıp (≤6 satır) tekrar büyüt → çökme yok.
