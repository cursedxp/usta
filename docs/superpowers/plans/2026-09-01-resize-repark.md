# `Screen::resize` — ekran-dibi varsayımını kaldır — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** Dal `relative-render`, HEAD `9d9cd74`, v0.30.0, temiz ağaç. **Bu dalda devam edilir, yeni dal AÇILMAZ** — v0.30.0 henüz merge edilmedi ve bu düzeltme onun bilinen hatasını kapatıyor; ayrı dal iki kez merge demek olur. Spec: `docs/superpowers/specs/2026-09-01-resize-repark-amendment-design.md`, ana tasarım `docs/superpowers/specs/2026-09-01-relative-render-design.md`. Çelişkide EK kazanır, ekin susduğu her yerde ana tasarım geçerlidir.

**Goal:** `Screen::resize`'ın "blok her zaman ekranın altındadır" varsayımını kaldır. Varsayım yeni açılmış oturumda YANLIŞ — blok ortada durur, altında boşluk olur, `MoveDown(painted * 2)` bloğun altına iner ve yukarı silme bloğun üst satırlarını sağlam bırakır: hayalet. İniş artık `Screen`'in kendi kayıtlarından hesaplanır. Hedef sürüm **v0.30.1**.

**Architecture:** Tek modül (`src/tui/screen.rs`) + bir saf fonksiyon (`descend_rows`) + `cursor_col` alanı. `paint`, `page`, `erase_block`, `rewrapped_rows`, `clip_to_width` DEĞİŞMEZ. Ayrıca ana tasarımın isim listesinden düşen `editor::content_rows` silinir.

**Tech Stack:** Rust, crossterm 0.29 (yalnız göreli hareket + satır silme). Binary crate — filtreli `cargo test <filtre>`.

## Global Constraints

- **K3 aynen geçerli:** üretim kaynağında `MoveTo(`, `MoveToRow`, `cursor::position()`, `SavePosition`/`RestorePosition` YOK. Yalnız `MoveUp`/`MoveDown`/`MoveToColumn`.
- **K4 aynen geçerli:** her `paint` `Clear(ClearType::FromCursorDown)` ile biter.
- `rewrapped_rows` gövdesine DOKUNULMAZ (ek bunu açıkça kapsam dışı bırakıyor).
- Her task: TDD — önce failing test, DOĞRU sebeple fail ettiği görülür, sonra minimal implementasyon; task sonunda `cargo test` TÜMÜ yeşil, sonra commit.
- Bilinen ortam hatası, düzeltilmez ve engellemez: `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt` (`pdftotext` bu makinede kurulu, test yokluğunu varsayıyor).
- `cargo clippy --all-targets` 0 uyarı; `cargo fmt` dokunulan dosyalara scoped; sonda `cargo fmt --check` crate-geneli temiz.
- **Push / merge / tag / `cargo install` YOK. `git stash` YOK** (bu repoda eski stash kayıtları var, bir kez zarar verdi).
- `src/tui/run.rs` (589) ve `src/plain.rs` DEĞİŞMEZ.
- Tanımlayıcılar, kullanıcıya dönük string'ler, yorumlar ve commit mesajları İNGİLİZCE.

---

### Task 1: `descend_rows` + `cursor_col`, ve `resize`'ın yeni inişi

**Files:** Modify `src/tui/screen.rs` (üretim + test modülü)

**Interfaces:**
- Produces: `fn descend_rows(last_widths: &[u16], painted: u16, cursor_up: u16, cursor_col: u16, new_width: u16) -> u16` — saf, ekin "Davranış" bölümündeki formülün birebir kendisi.
- Modify: `Screen.cursor_col: u16` alanı; `paint` onu kaydeder (`forget_block` sıfırlar).
- Modify: `Screen::resize` — 1. adım `MoveDown(descend_rows(...))`. 3-5. adımlar DEĞİŞMEZ.

- [x] **Step 1: Failing test — önce ISIRAN regresyon testi.** `screen.rs` test modülüne, bloğun ekran ORTASINDA olduğu durumu temsil eden bir `resize` testi yaz: küçük bir blok boya, sonra `resize` çağır, üretilen baytlardaki `MoveDown` miktarını oku. Beklenen değer `descend_rows`'un döndürdüğü sayı; bugünkü `painted * 2` ile FAIL etmeli. Yorumda neden yazılır: *v0.30.0 assumed the block always sits at the bottom of the screen; in a freshly opened session it does not, and the descent overshot into blank rows, leaving the block's top rows alive as a ghost.*
- [x] **Step 2:** Ekin test listesindeki `descend_rows` birim testlerini yaz (yedi vaka: değişmemiş genişlik/imleç sonda · değişmemiş/iki yukarıda · imleç satırı ikiye sarılıyor, imleç ilk yarıda · aynısı ikinci yarıda · alttaki iki satır ikiye sarılıyor · `painted == 0` · `new_width == 0`).
- [x] **Step 3:** Testleri koş, DOĞRU sebeple FAIL ettiklerini gör (fonksiyon yok / iniş `painted * 2`).
- [x] **Step 4:** Implementasyon: alan, `paint`'te kayıt, `forget_block`'ta sıfırlama, `descend_rows`, `resize`'ın 1. adımı. Doküman yorumu ekin A1/A3'ünü anlatır — ekran-dibi anlatan eski paragraf SİLİNİR, yerine yeniden-sarma varsayımı ve kalan riski yazılır.
- [x] **Step 5:** Kaynak-pin: `screen.rs` üretim gövdesinde `saturating_mul(2)` YOK. Pin doğrulaması (protokol): geçici olarak inişi `painted * 2`'ye çevir → hem regresyon testi hem pin FAIL etsin → geri al.
- [x] **Step 6:** `cargo test` TÜMÜ, `cargo clippy --all-targets`, `cargo fmt -- src/tui/screen.rs`. Commit: `fix: descend by our own recorded rows on resize, not to the screen bottom`

---

### Task 2: `content_rows`'u sök + belgeleme + sürüm

**Files:** Modify `src/tui/editor.rs`, `docs/superpowers/specs/2026-09-01-relative-render-design.md`, `SPEC.md`, `docs/ROADMAP.md`, `Cargo.toml`, `src/tui/welcome_tests.rs`, `Cargo.lock`

- [x] **Step 1:** `editor::content_rows` ve `INPUT_MAX_ROWS`'un yalnız onun için var olan parçaları silinir (`INPUT_MAX_ROWS` `frame_lines` tarafından kullanılıyorsa KALIR — önce `grep` ile doğrula). İlgili testler kaldırılır; `frame_lines`'ın satır sayısı ve tavan testleri KALIR. `#[allow(dead_code)]` kalmaz.
- [x] **Step 2:** Ana tasarım dosyasının "İsimlendirme (bağlayıcı)" listesinden `content_rows` satırı çıkarılır ve tek cümlelik neden yazılır (çağrısız kaldı, `frame_lines` işi yapıyor, bağlamak tavanı iki kez uygulatırdı). Ana tasarımın `resize` bölümüne, ekin dosya yolunu gösteren tek satırlık "bu bölüm v0.30.1'de değişti" notu düşülür — bölüm SİLİNMEZ.
- [x] **Step 3:** `SPEC.md` §4.19'daki v0.30.0 paragrafında "ölçülmemiş ikinci risk" olarak yazılan ekran-dibi premise'i **kapandı** olarak güncellenir: neyin yanlış olduğu, inişin artık `Screen`'in kendi kayıtlarından hesaplandığı, ve yerine geçen dar riskin (yeniden sarmayan terminal) ne olduğu. Paragraf silinmez, düzeltilir.
- [x] **Step 4:** `docs/ROADMAP.md` `## Completed` başına kayıt, `v0.30.1.` ile biter. v0.30.0 kaydı silinmez; oradaki "ölçülmemiş risk" ifadesine bu kaydın kapattığı not düşülür.
- [x] **Step 5:** Sürüm 0.30.1 (`Cargo.toml` + `src/tui/welcome_tests.rs` pini), `cargo check` ile lock tazele.
- [x] **Step 6:** `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`. Commit: `docs: close the bottom-of-screen premise; drop the unused content_rows; bump to v0.30.1`

---

## Manuel Doğrulama (Anil ile, plan bittikten sonra)

`cargo install --path .` sonrası:

1. **Uzun terminalde YENİ oturum aç** — blok ekranın ortasında, altında boş satırlar kalsın. Yatay genişlet, sonra daralt → hayalet yok. **Bu, v0.30.0'ın kesin hatasının testidir, ilk bunu koş.**
2. Uzun bir oturumda (çıktı birikmiş, blok dipte) aynı testler → hayalet yok, ekrandaki metin kaybolmuyor.
3. Kenardan yavaşça sürükleyerek boyutlandır → sürükleme bitince tek temiz blok.
4. Boş girdi → üstte/altta düz çizgi, yan kenar yok, 4 satır. Uzun metin ve Ctrl+J → alt çizgi aşağı kayar. Tavanı aş → içerik kayar.
5. Mentor yanıtı beklerken resize · giriş akışı · onay sorusu · `/show` · `/context` → hepsi normal.
6. Yukarı kaydır → transcript bütün ve kopyalanabilir.
