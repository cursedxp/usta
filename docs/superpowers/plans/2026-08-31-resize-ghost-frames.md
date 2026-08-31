# Resize Sonrası Hayalet Çerçeveler — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** `main` dalı, v0.29.0, temiz ağaç. Bekleyen/çakışan başka plan YOK (`2026-08-29-session-awareness.md` tamamlandı, `9eb3983`). `docs/superpowers/specs/2026-08-31-resize-ghost-frames-design.md` ağaçta OLMALI ve çelişkide o kazanır — özellikle "Kök neden", "Davranış" ve "İsimlendirme (bağlayıcı)" bölümleri. İş dalını main'den aç (`git switch -c resize-ghost-frames`).

**Goal:** Terminal yeniden boyutlandırılınca ekranda kalan hayalet kutu kalıntılarını bitir. Kök neden: `TrackedBackend`'in izlediği imleç konumu terminalin kendi reflow'unda bayatlıyor, ratatui yeni viewport'u o bayat satırdan hesaplıyor, ve inline temizlik yalnız yeni tepenin AŞAĞISINI siliyor. Çözüm imleci *sormak* değil *koymak*: mutlak `MoveTo` reflow'dan etkilenmez. Hedef sürüm **v0.29.1**.

**Architecture:** Tüm değişiklik `src/tui/page.rs::handle_resize` + `src/tui/term.rs`'e tek alan (`Tui::last_size`). Saf mantık iki küçük yardımcıya çıkarılır (`resize_anchor`, `size_changed`) — TUI birim testten sürülemediği için gerçek testin tutunacağı yer burası. Çağrı yerleri (run.rs, ask.rs ×2, entry.rs) DEĞİŞMEZ.

**Tech Stack:** Rust, ratatui 0.30.2 / ratatui-core 0.1.2 (`Terminal::resize`, `Terminal::set_cursor_position`, `Terminal::backend_mut` public), crossterm 0.29. Binary crate — filtreli `cargo test <filtre>`.

## Global Constraints

- TÜM tanımlayıcılar, kullanıcıya dönük string'ler, kod yorumları ve commit mesajları İNGİLİZCE. Türkçe yalnız spec/plan düzyazısında.
- Her task: TDD — önce failing test, DOĞRU sebeple fail ettiği görülür, sonra minimal implementasyon; task sonunda `cargo test` TÜMÜ yeşil, sonra commit.
- `cargo clippy --all-targets` her task sonunda **0 uyarı**.
- `cargo fmt` yalnız dokunulan dosyalara scoped (`cargo fmt -- <dosyalar>`); son task'ta `cargo fmt --check` crate-genelinde temiz.
- **Push / tag / `cargo install` YOK** — manuel doğrulama sonrası insanın kararı. Plan yalnız lokal commit üretir.
- Satır bütçesi (SPEC §11, 600): `page.rs` 140 → ≈190, `term.rs` 120 → ≈124. `run.rs` (596) DOKUNULMAZ.
- **CPR yasağı korunur (v0.26.1):** yeni kodun hiçbir yerinde `crossterm::cursor::position()` YOK. `term.rs`'teki `cpr_seed_happens_before_event_stream` testi gevşetilmez — `page.rs`'i zaten tarıyor, yeşil kalmalı.
- Davranış regresyonu yasak: `Event::Resize` kolları dört döngüde de yerinde kalır; `page::draw`, `page::page`, `insert_before` akışı değişmez; plain yol (`src/plain.rs`) DEĞİŞMEZ.
- Mevcut pin testi `resize_events_are_handled_in_every_event_loop` gevşetilmez, yalnız genişletilir.

---

### Task 1: Saf çapa mantığı — `resize_anchor` + `size_changed`

**Files:**
- Modify: `src/tui/page.rs` (iki saf fonksiyon + testleri)

**Interfaces:**
- Produces: `fn resize_anchor(size: Size) -> (Position, Rect)` — `(Position { x: 0, y: size.height.saturating_sub(1) }, Rect::new(0, 0, size.width, size.height))`.
- Produces: `fn size_changed(prev: Size, now: Size) -> bool` — `prev != now`.
- Her ikisi de bu task'ta henüz çağrılmıyor → `#[allow(dead_code)]` alır ve **Task 2'de SÖKÜLÜR**.

- [ ] **Step 1: Failing test.** `page.rs` test modülüne ekle:

```rust
#[test]
fn resize_anchor_targets_the_bottom_left_and_the_full_screen() {
    // The tracked cursor goes stale when the terminal reflows on resize, so
    // handle_resize stops asking where it is and puts it somewhere known:
    // an absolute MoveTo to the bottom-left. compute_inline_size then lands
    // the viewport on the last VIEWPORT_H rows, independent of the stale offset.
    let (pos, area) = resize_anchor(Size::new(100, 30));
    assert_eq!(pos, Position { x: 0, y: 29 });
    assert_eq!(area, Rect::new(0, 0, 100, 30));
}

#[test]
fn resize_anchor_saturates_on_a_zero_height_screen() {
    let (pos, _) = resize_anchor(Size::new(80, 0));
    assert_eq!(pos.y, 0, "row must not underflow");
}

#[test]
fn size_changed_is_false_for_an_identical_size() {
    // Drag-resizing emits a burst of Resize events; clearing the screen on
    // every one of them would strobe. Only a real size change acts.
    assert!(!size_changed(Size::new(80, 24), Size::new(80, 24)));
    assert!(size_changed(Size::new(80, 24), Size::new(81, 24)));
    assert!(size_changed(Size::new(80, 24), Size::new(80, 25)));
}
```

- [ ] **Step 2:** `cargo test resize_anchor` → derleme hatası (fonksiyon yok). Doğru sebep.
- [ ] **Step 3:** İki fonksiyonu `page.rs` üretim kısmına yaz (`handle_resize`'ın hemen üstüne), `#[allow(dead_code)]` ile. Gerekli import: `ratatui::layout::{Position, Rect, Size}`.
- [ ] **Step 4:** `cargo test`, `cargo clippy --all-targets`, `cargo fmt -- src/tui/page.rs`. Commit: `feat: resize anchor primitives — bottom-left cursor target, size gate`

---

### Task 2: `Tui::last_size` + yeni `handle_resize`

**Files:**
- Modify: `src/tui/term.rs` (`Tui` struct'ına alan, `setup()`'ta tohum)
- Modify: `src/tui/page.rs` (`handle_resize` gövdesi, `#[allow(dead_code)]` sökülür, pin testi genişler)

**Interfaces:**
- Consumes: Task 1'in `resize_anchor` / `size_changed`'i.
- Produces: `Tui { pub terminal, pub last_size: Size }` — `setup()` içinde `terminal.size()?` ile tohumlanır (Terminal kurulduktan SONRA, ioctl; CPR değil).
- `handle_resize` imzası DEĞİŞMEZ → dört çağrı yerine dokunulmaz.

- [ ] **Step 1: Failing test.** `page.rs`'teki mevcut `resize_events_are_handled_in_every_event_loop` testinin ALTINA, aynı kaynak-pin desenini kullanan yeni test:

```rust
#[test]
fn handle_resize_clears_reanchors_and_resizes_twice() {
    // Ghost frames (v0.29.0): after a width change the terminal reflows and the
    // tracked cursor goes stale, so ratatui anchored the new viewport at a
    // wrong row — and inline clear_viewport only erases DOWNWARD from that row,
    // so the old frame's top rows survived. Three needles, one per half of the
    // fix: erase the whole visible screen (the old frame's position is
    // unknowable), re-sync the cursor with an absolute move, and resize twice
    // (the first call is forced to y=0 on a horizontal shrink; the second sees
    // an already-narrow viewport and honours the bottom anchor).
    let prod = include_str!("page.rs").split("#[cfg(test)]").next().unwrap();
    let body = prod
        .split("fn handle_resize")
        .nth(1)
        .expect("page.rs lost its handle_resize helper");
    assert!(body.contains("ClearType::All"), "handle_resize no longer clears the screen");
    assert!(body.contains("set_cursor_position"), "handle_resize no longer re-anchors the cursor");
    assert_eq!(
        body.matches(".resize(").count(),
        2,
        "handle_resize must resize twice — see the spec's horizontal-shrink note"
    );
}
```

- [ ] **Step 2:** `cargo test handle_resize_clears` → FAIL (mevcut gövde yalnız `autoresize()` çağırıyor). Doğru sebep.
- [ ] **Step 3:** `term.rs`: `pub struct Tui { pub terminal: Terminal<TrackedBackend<Stdout>>, pub last_size: ratatui::layout::Size }`. `setup()` sonunda:
```rust
let last_size = terminal.size()?;
Ok(Tui { terminal, last_size })
```
- [ ] **Step 4:** `page.rs`: `handle_resize` gövdesini spec'in "Davranış" sırasıyla yaz — boyut kapısı → `clear_region(ClearType::All)` → `set_cursor_position(anchor)` → `resize(area)` → `set_cursor_position(anchor)` → `resize(area)` → `last_size` güncelle. `autoresize()` çağrısı KALKAR. Doküman yorumu neden-odaklı olsun: bayat izlenen imleç, yalnız-aşağı temizlik, çift resize'ın gerekçesi. `#[allow(dead_code)]`'lar sökülür. Gerekli import: `ratatui::backend::{Backend, ClearType}`.
- [ ] **Step 5:** Pin doğrulaması (protokol): `handle_resize` içindeki `clear_region` satırını geçici yorum satırı yap → yeni testin FAIL ettiğini gör → geri al.
- [ ] **Step 6:** `cargo test` (TÜMÜ — özellikle `cpr_seed_happens_before_event_stream` yeşil kalmalı), `cargo clippy --all-targets`, `cargo fmt -- src/tui/page.rs src/tui/term.rs`. Satır bütçesi: `grep -c "" src/tui/page.rs src/tui/term.rs`. Commit: `fix: kill ghost frames on terminal resize — put the cursor instead of asking`

---

### Task 3: Belgeleme + sürüm

**Files:**
- Modify: `SPEC.md` (§4.19 blokunun sonuna, v0.26.2 notunun ALTINA yeni paragraf)
- Modify: `Cargo.toml` (`version = "0.29.1"`), `Cargo.lock` (`cargo check` ile tazelenir)
- Modify: `README.md` — YALNIZ sürüm/changelog satırı varsa; davranış anlatımı gerekmiyorsa dokunma

**Interfaces:** Kod değişikliği yok.

- [ ] **Step 1:** `SPEC.md`'e İngilizce paragraf: v0.29.1'in ne düzelttiği (hayalet çerçeveler), kök nedenin üç katmanı (bayat izlenen imleç · `compute_inline_size`'ın onu okuması · `clear_viewport`'un yalnız aşağı silmesi), kararın özü (mutlak `MoveTo` reflow'dan etkilenmez), kabul edilen bedel (her gerçek boyut değişiminde görünen ekran silinir; tahribatsız transcript yeniden basımı kapsam dışı) ve tasarım dosyası yolu.
- [ ] **Step 2:** `Cargo.toml` sürüm 0.29.1, `cargo check` ile lock tazele.
- [ ] **Step 3:** `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check` (crate-geneli temiz). Commit: `docs: SPEC 4.19 — resize ghost-frame fix; bump to v0.29.1`

---

## Manuel Doğrulama (Anil ile, plan bittikten sonra)

`cargo install --path .` sonrası, gerçek terminalde:

1. Boşta bir oturumda pencereyi **genişlet** → tek kutu, ekranın altına yapışık, hiçbir kalıntı yok.
2. **Daralt** → aynı. Kutu tepeye yapışıp kalmıyor.
3. Kenardan **yavaşça sürükle** → yanıp sönme kabul edilebilir; sürükleme bitince tek temiz kutu.
4. Mentor yanıtı beklenirken (spinner dönerken) resize → aynı; yanıt geldiğinde doğru genişlikte sarılıyor.
5. Giriş akışında (`entry.rs`, konu kilitlenmeden) ve onay sorusunda (`ask.rs` confirm) resize → aynı.
6. Resize sonrası uzun bir mesaj yaz (kutu çok satıra büyür) → kutu doğru büyüyor, durum satırı altta kalıyor.
