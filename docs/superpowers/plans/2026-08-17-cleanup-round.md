# Plan — Kalan Temizlik Turu (v0.23.0)

**Spec:** `docs/superpowers/specs/2026-08-17-cleanup-round-design.md`
**Branch:** `cleanup-round`, taban `c33d32e` (= main, v0.22.0)

## Global Constraints

1. **Test sayısı sabit: 372.** Her task sonunda `cargo test` → `372 passed; 0 failed`. Task 7 (struct) dahil — imza değişiyor, davranış değişmiyor.
2. **Task 1 dışında SAF TAŞIMA.** Gövde değişmez, imza değişmez, isim değişmez, yorum "iyileştirilmez". Task 7 tek istisna ve kapsamı planda tanımlı.
3. **`welcome.rs` DİZİNE ÇEVRİLMEZ.** `welcome/mod.rs` dönüşümü `#[path = "welcome_tests.rs"]`'i kırar — `#[path]` içeren dosyanın kendi dizinine göre çözülür. Kardeş dosya kullanılır.
4. **`run()` BÖLÜNMEZ.** 558 satırıyla kalır. Gerekçe spec'te: sıfır test, beş mutable değer `.await` boyunca canlı, bütçe dosya başına.
5. **Yeni modüller `src/tui/` altında kardeş dosya**, `mod.rs`'e `pub mod` satırı eklenir (alfabetik).
6. **Görünürlük:** taşınan öğe bugünkü görünürlüğünü korur. `main.rs` bölünmesinde olduğu gibi, modül sınırını geçmek için gereken minimum genişletme yapılır — bir fazlası bulgu.
7. **Glob `use` veya re-export YOK.** Çağrı yerini kapatmak için değil.
8. `cargo build` uyarısız; taşımanın öksüz bıraktığı import'ları temizle.

## Doğrulama (her task)

```
cargo test                 → 372 passed; 0 failed
cargo clippy --all-targets → Task 7'ye kadar yalnız run_plain_loop uyarısı; Task 7'den sonra SIFIR
cargo build                → uyarısız
```

---

## Task 1 — Crate genelinde `cargo fmt`

Kendi commit'i. Sonraki hiçbir task'ın diff'ine karışmasın diye önce gider.

### Yapılacaklar

`cargo fmt` çalıştır (tüm crate). 21 dosya değişecek — hepsi v0.22.0'ın dokunmadığı, `main` ile birebir aynı dosyalar.

Ardından `rustfmt.toml` **ekleme** — varsayılan rustfmt ayarları yeterli, yapılandırma eklemek ayrı bir karar.

`CLAUDE.md`'de v0.22.0'da eklenen "cargo fmt crate genelinde temiz değil" notunu **kaldır** — artık doğru değil. Yerine tek satır: fmt temiz, düzenlediğin dosyalarda çalıştır.

### Doğrulama

`cargo fmt --check` → temiz. `cargo test` → 372. Diff yalnız biçim olmalı: `git diff --stat` büyük ama `git diff -w --stat` (boşluk yoksayan) neredeyse boş olmalı. Bu ikisi arasındaki fark, değişimin gerçekten sadece biçim olduğunun kanıtı — raporla.

---

## Task 2 — `src/tui/welcome_data.rs`

### Taşınacak öğeler (`welcome.rs` → `welcome_data.rs`)

`WelcomeData` (struct, `pub`) · `section` · `extract_name` · `extract_level` · `curriculum_percent` · `next_unseen` · `drill_count` · `due_items` · `due_count` · `due_questions` · `last_session_ago` · `gather`

Kaynak: `welcome.rs` satır 22-225 (bitişik blok).

**Kalanlar:** `LOGO`, `fit`, `wrap`, `pad`, `week_line` ve tüm render fonksiyonları `welcome.rs`'te kalır.

### Test ayrımı

`welcome_tests.rs`'ten veri testleri `src/tui/welcome_data_tests.rs`'e taşınır, `use super::*` ile bağlanır. `welcome_data.rs` sonuna:

```rust
#[cfg(test)]
#[path = "welcome_data_tests.rs"]
mod tests;
```

Taşınacak testler: `extract_name_reads_h1_after_dash` · `extract_level_reads_first_line_of_section` · `curriculum_percent_counts_non_unseen` · `next_unseen_returns_first_unseen_item_text` · `state_matching_is_exact_segment_not_substring` · `state_matching_uses_trailing_segment_not_item_text_word` · `drill_count_counts_section_bullets` · `due_count_counts_due_and_untagged_skips_future` · `due_questions_selects_and_orders_oldest_due_first` · `due_questions_caps_at_three_but_due_count_stays_uncapped` · `due_questions_excludes_other_section_bullets` · `due_count_matches_due_questions_len_when_three_or_fewer` · `gather_full_and_first_session` · `gather_fills_last_session` · `last_session_ago_*` (5 test)

**Dikkat:** `render_welcome` testleri `gather`'ı da kullanıyor (`welcome_shows_due_line_three_states`, `welcome_orange_discipline`) — bunlar render tarafında KALIR ve `gather`'ı `crate::tui::welcome_data::gather` olarak çağırır. Test yardımcıları (`plain_lines`, `orange_element_count`, `mk_entry`, `full_resume_data`, `empty_resume_data`) hangi dosyada kullanılıyorsa oraya; ikisinde de kullanılan varsa kopyalama — çağıran tarafa taşı ve diğerinden nitelikli çağır.

### Çağıran güncellemeleri (9 yer, 4 dosya)

`src/tui/run.rs`: `extract_name` (408), `gather` (767), `due_questions` (800), `drill_count` (801)
`src/lifecycle.rs`: `curriculum_percent` (211)
`src/plain.rs`: `due_questions` (174), `drill_count` (175)
`src/defaults.rs`: `extract_name` (107, `#[cfg(test)]` bloğu içinde)
`welcome.rs`: `render_welcome`/`render_welcome_identity` `WelcomeData`'yı okuyor → `use crate::tui::welcome_data::WelcomeData;`

`src/tui/mod.rs`: `pub mod welcome_data;` (alfabetik, `welcome`'dan sonra)

---

## Task 3 — `src/tui/paint.rs` (saf builder'lar)

`run.rs`'ten `&mut Tui` taşımayan her şey. Dosyanın 7 testinin **hepsi** burayı kapsıyor, yani testler bu task'la taşınır.

### Taşınacak öğeler

`notice_line` · `warn_line` · `error_line` · `user_echo_text` · `wrap_cells` · `LockedKey` (enum) · `classify_locked_key` · `short_dir`

### Taşınacak testler (7 = `run.rs`'in tamamı)

`user_echo_prefixes_first_line_and_indents_rest` · `user_echo_wraps_long_line_to_width` · `notice_layers_carry_glyph_and_color` · `user_echo_text_is_not_dim` · `user_echo_prefix_is_orange` · `classify_locked_key_ctrl_c_and_d_are_cancel_requests` · `classify_locked_key_enter_and_chars_are_edits`

Bu task sonunda `run.rs`'te `#[cfg(test)] mod tests` bloğu hiç kalmaz.

### Not

`short_dir` `run.rs`'te 567 ve 773. satırlardan çağrılıyor — `crate::tui::paint::short_dir` olur.

---

## Task 4 — `src/tui/page.rs` (sayfalama)

### Taşınacak öğeler

`page` · `page_reply` · `page_notice` · `page_warn` · `page_error` · `flush_notices` · `page_user_echo` · `current_width` · `draw`

Test yok.

`page_notice`/`page_warn`/`page_error`, Task 3'te taşınan `notice_line`/`warn_line`/`error_line`'ı çağırıyor → `crate::tui::paint::`. `page_user_echo`, `user_echo_text` ve `current_width`'i çağırıyor. `draw`, `crate::tui::status::render_status`'ı çağırıyor (dokunma).

Bu fonksiyonlar `&mut Tui` alıyor — parametre olarak, closure yakalaması değil, modül sınırını geçmesi sorunsuz.

---

## Task 5 — `src/tui/ask.rs`

### Taşınacak öğeler

`AskOutcome` (enum) · `ask_live` · `tui_confirm`

Test yok.

**Görünürlük notu:** `AskOutcome` bugün `pub` ama `run.rs` dışında hiç kullanılmıyor (harita doğruladı). Taşırken `pub(crate)`'e indir — bu bir "iyileştirme" değil, ölü genişliğin düzeltilmesi; spec'te kayıtlı.

`ask_live`, `classify_locked_key` (Task 3) ve `draw` (Task 4) çağırıyor.

---

## Task 6 — `src/tui/entry.rs`

### Taşınacak öğeler

`ask_topic` · `run_visual_generation` · `trigger_auto_visual`

Test yok.

**TUZAK:** `src/plain.rs`'in de `run_visual_generation` ve `trigger_auto_visual` adlı fonksiyonları var. Farklı fonksiyonlar, aynı isim. Birbirlerini çağırmıyorlar. `plain.rs`'e dokunma, birleştirme, "tekilleştirme".

Üçü de Task 3-5'te taşınan öğeleri çağırıyor; yollar güncellenir.

Bu task sonunda `run.rs` ≈ 580 satır (`use` bloğu + `run()`) olmalı — ölç ve raporla.

---

## Task 7 — `run_plain_loop` argümanlarını struct'a topla

**Bu task saf taşıma DEĞİL.** Tek imza değişikliği, kapsamı burada tanımlı.

### Yapılacaklar

`src/plain.rs`'te `run_plain_loop`'un 10 argümanını tek bir struct'a topla. Struct `plain.rs`'te tanımlanır, `pub(crate)` olmasına gerek yok (tek çağıran `main.rs` ama struct'ı `main.rs` kuruyorsa `pub(crate)` gerekir — hangi tasarımı seçersen gerekçesini raporla).

Fonksiyon gövdesinde alan erişimleri dışında **hiçbir şey değişmez**. Mantık aynen kalır.

Tek çağrı yeri `main.rs` — struct literal ile kur, alanlar birebir aynı değerler.

`#[allow(clippy::too_many_arguments)]` varsa kaldır.

### Doğrulama

`cargo clippy --all-targets` → **sıfır uyarı**. Bu turun ürünü bu.
`cargo test` → 372.

Argüman sırası/isimleri struct alanlarına birebir taşınmalı; derleyici eksik alanı yakalar ama **yanlış sıradaki aynı tipli iki alanı yakalamaz** — `Path` ve `PathBuf` argümanlarını tek tek karşılaştır ve raporda göster.

---

## Task 8 — Ölçüm + sürüm

1. `SPEC.md` — v0.22.0'da eklenen boyut bütçesi maddesini güncelle: artık bütçeyi aşan modül yok (varsa yaz). `run()`'ın 558 satırıyla bilinçli bırakıldığını tek cümleyle kaydet.
2. `docs/ROADMAP.md` — tarihli tek satır (2026-08-17).
3. `Cargo.toml` `0.22.0` → `0.23.0` + `welcome_tests.rs`'teki `version_aligned_with_spec` dizgesi.
4. Son ölçüm: `wc -l src/*.rs src/tui/*.rs` → spec'in "Ölçüm" tablosunun yanına ulaşılan hâl.

### Doğrulama

`cargo test` → 372, `cargo clippy --all-targets` → sıfır uyarı, `cargo fmt --check` → temiz. Elle doğrulama ATLA — Anil koşturacak.

---

## Notlar

- Task sırası bağımlılık sırası: fmt önce (diff karışmasın), sonra `paint` → `page` → `ask` → `entry` (her biri öncekinin taşıdığını çağırıyor). `welcome_data` bağımsız, Task 2'de erken alınıyor ki `run.rs` zinciri kesintisiz koşsun.
- Beklenen son hâl: `run.rs` ~580, `welcome.rs` ~570, `welcome_data.rs` ~200, dört yeni `tui/` modülü 100-180 arası.
