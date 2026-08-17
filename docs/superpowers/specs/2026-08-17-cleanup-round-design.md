# Design — Kalan Temizlik Turu (v0.23.0)

**Bağlam:** v0.22.0 `main.rs`'i 3045 → 272 satıra indirdi ve SPEC/CLAUDE.md'ye 600 satırlık modül bütçesi yazdı. O turun sonunda üç madde bilinçli olarak açık bırakıldı. Bu tur onları kapatıyor.

## Açık üç madde

1. **`cargo fmt` crate genelinde temiz değil** — 21 dosya rustfmt-kirli. Hepsi v0.22.0'ın dokunmadığı dosyalar, yani regresyon değil; ama crate artık iki-katmanlı biçimde. `rustfmt.toml` yok, CI fmt kapısı yok. Somut tuzak: bir sonraki kişi `src/setup.rs`'i düzenler, alışkanlıkla `cargo fmt` çalıştırır, 21 alakasız dosyaya dokunan bir diff çıkarır — Task 9'un implementer'ının düştüğü ve elle geri aldığı tuzağın aynısı.
2. **İki dosya bütçe üstünde** — `src/tui/run.rs` 1185, `src/tui/welcome.rs` 797. v0.22.0 planı yalnız `main.rs`'i kapsıyordu; ikisi de dürüstçe "kapsam dışı, kendi geçişinin adayı" diye kayda geçti. Bu tur o geçiş.
3. **`run_plain_loop` 10 argüman alıyor** (`plain.rs`, clippy `too_many_arguments`). v0.22.0'da bilinçli taşındı, düzeltilmedi — düzeltilseydi taşımanın saflığı kanıtlanamazdı.

---

## Ölçüm

| dosya | toplam | üretim | test |
|---|---|---|---|
| `src/tui/run.rs` | 1185 | ~1062 | ~123 (satır 1062'den) |
| `src/tui/welcome.rs` | 797 | ~795 | 0 (Task 1'de `welcome_tests.rs`'e ayrıldı) |

### Ulaşılan (Task 8 sonrası, `wc -l src/*.rs src/tui/*.rs`)

| dosya | satır | not |
|---|---|---|
| `src/tui/run.rs` | 591 | `use` bloğu + `run()` (558, bilinçli istisna — aşağıda) |
| `src/tui/welcome.rs` | 591 | `LOGO`, `fit`/`wrap`/`pad`/`week_line`, render'lar; veri yarısı çıktı |
| `src/tui/welcome_data.rs` | 215 | Task 2 — Spec §5 veri toplama, ratatui bağımsız |
| `src/tui/paint.rs` | 237 | Task 3 — saf builder'lar + key sınıflandırıcılar |
| `src/tui/page.rs` | 90 | Task 4 — sayfalama katmanı |
| `src/tui/ask.rs` | 107 | Task 5 — `ask_live`/`tui_confirm` |
| `src/tui/entry.rs` | 217 | Task 6 — `ask_topic` + görsel tetikleyiciler |

En büyük beş dosya (`wc -l`, en büyükten): `tui/welcome_tests.rs` 853, `visual.rs` 739, `tui/welcome.rs` 591, `tui/run.rs` 591, `setup.rs` 574. Toplam (tüm `src/*.rs` + `src/tui/*.rs`): 13182 satır.

**`tui/welcome_tests.rs` (853) bütçe dışı, gözden kaçmadı — muaf.** `#[cfg(test)] #[path = "welcome_tests.rs"]` deseniyle ayrılmış bağımsız bir test dosyası (`welcome.rs:590`); kuralın kendi metni "test modülü bütçeye girmez" diyor. Değerlendirildi ve muafiyetle uyumlu, atlanmadı.

**`visual.rs` (739 toplam) uyumlu ama izlenmeli — ihlal değil.** Test modülü 264. satırda başlıyor, yani ~263 satır üretim kodu — bütçenin (600) çok altında. Ama dosyanın toplamı büyük, ve `welcome.rs`/`progress.rs`/`setup.rs`'in zaten aldığı `#[path]` test-ayrımı büyürse bariz bir sonraki adım. Bu tur çıkarmıyor — kapsam dışı; yalnız kayda geçiyor.

Sonuç: crate'te 600 satır üretim bütçesini aşan modül yok. `cargo test` → 372/372, `cargo clippy --all-targets` → sıfır uyarı, `cargo fmt --check` → temiz.

## `run.rs` — bütçeye `run()`'a dokunmadan iniliyor

Harita çıkarıldıktan sonraki kritik bulgu: **`run()` tek başına 558 satır** (493-1051) ve `Tui`, `InputBox`, `EventStream`, `Session`, `Backend`'i `.await`'ler boyunca canlı tutuyor. Dosyadaki fonksiyonların neredeyse hepsi `&mut Tui` taşıyor.

Ama `&mut Tui` **parametre**, closure yakalaması değil — modül sınırını geçmesi sorun değil. Yani çevresindeki her şey saf taşınabilir:

| küme | öğeler | ~satır |
|---|---|---|
| saf builder'lar | `notice_line`, `warn_line`, `error_line`, `user_echo_text`, `wrap_cells`, `classify_locked_key`, `LockedKey`, `short_dir` | ~100 |
| sayfalama | `page`, `page_reply`, `page_notice`, `page_warn`, `page_error`, `flush_notices`, `page_user_echo`, `current_width`, `draw` | ~120 |
| soru sorma | `AskOutcome`, `ask_live`, `tui_confirm` | ~110 |
| konu girişi + görsel | `ask_topic`, `run_visual_generation`, `trigger_auto_visual` | ~180 |

Toplam ~510 satır çıkar → `run.rs`'te `run()` + `use` bloğu ≈ **580 satır**, bütçenin altında.

### `run()` bölünmüyor — gerekçe

558 satır tek fonksiyon için çok, ama bölmemesi bilinçli:

- **Sıfır testi var.** Dosyadaki 7 testin hepsi saf yardımcıları kapsıyor (`user_echo_text`, `notice_line` üçlüsü, `classify_locked_key`). `run`, `ask_live`, `ask_topic`, `tui_confirm` — canlı `Tui`/`EventStream`/`Backend` isteyen hiçbir şey test edilmemiş. v0.22.0'ın "test sayısı sabit" kanıtı burada yok.
- **Beş mutable değer `.await` boyunca canlı.** `run`'ın ana `tokio::select!`'i (866-1045) `&mut tui`, `&mut editor`, `&mut events`, `&mut session`, `&mut files`, `&mut debouncer` ve `backend`'i her arm'ın `.await`'i boyunca tutuyor. Fazları çıkarmak bunları tek bir bundle struct'a toplamayı gerektirir — gerçek bir tasarım değişikliği, taşıma değil.
- **Bütçe dosya başına.** `run.rs` bütçeye iniyor. Tek fonksiyonun uzunluğu ayrı bir kural olurdu ve o kural yazılı değil.

Sonuç: `run()` 558 satırıyla kalıyor, spec'e bilinen madde olarak yazılıyor. Bölünmesi kendi spec'ini ve — asıl önemlisi — önce bir test yüzeyini hak ediyor.

## `welcome.rs` — veri/render kesimi temiz

Haritanın en net bulgusu: ayrıştırıcı yarısının **ratatui'ye sıfır bağımlılığı var**. `section`, `extract_name`, `extract_level`, `curriculum_percent`, `next_unseen`, `drill_count`, `due_items`, `due_count`, `due_questions`, `last_session_ago`, `gather` yalnız `crate::tokens`, `crate::history`, `chrono` ve `std`'ye dokunuyor. `ratatui::{style,text}` ilk kez `render_welcome`'da (satır 325) görünüyor. İki yarı arasındaki tek bağ `WelcomeData`'nın kendisi — düz struct, alanlarında ratatui tipi yok.

Kesim: veri yarısı (satır 22-225, ~200 satır) → `src/tui/welcome_data.rs`. Geriye `LOGO`, `fit`/`wrap`/`pad`/`week_line` ve tüm render'lar kalıyor ≈ 570 satır.

### Dizine çevirme YASAK

`welcome.rs` → `welcome/mod.rs` dönüşümü cazip görünüyor ama **`#[path = "welcome_tests.rs"]`'i kırar**. `#[path]` attribute'u içeren dosyanın kendi dizinine göre çözülür; dosya `welcome/mod.rs`'e taşınırsa taban kayar ve `welcome_tests.rs` yetim kalır. Bu risk v0.22.0 Task 1'in review'ında latent olarak işaretlenmişti. **Kardeş dosya kullanılır**, dizin açılmaz.

### Test ayrımı

`welcome_tests.rs` 1117 satır, `use super::*` ile `welcome.rs`'e bağlı. Veri fonksiyonlarının ~25 testi `welcome_data_tests.rs`'e gider, `use super::*` ile `welcome_data.rs`'e bağlanır.

**Gizlilik kısıtı:** yalnız üç test doğrudan private öğeye dokunuyor — `solo_box_preserves_title_trailing_space_no_trim` (`solo_box`), `render_resume_bar_reflects_percent` ve `render_resume_bar_full_only_at_100_percent` (`map_bar`). Üçü de render tarafında kalıyor, sorun çıkarmıyor.

### Dış çağrı yerleri

Veri yarısına dışarıdan 9 çağrı var, 4 dosyadan: `run.rs` (`extract_name`, `gather`, `due_questions`, `drill_count`), `lifecycle.rs` (`curriculum_percent`), `plain.rs` (`due_questions`, `drill_count`), `defaults.rs` (test içinde `extract_name`). Hepsi `crate::tui::welcome_data::` olur.

## `run_plain_loop` — 10 argüman

Tek çağrı yeri var (`main.rs`). İki savunulabilir seçenek vardı:

- `#[allow(clippy::too_many_arguments)]` + gerekçe yorumu — sıfır risk, ama uyarıyı susturmak düzeltmek değil.
- Argümanları bir struct'a toplamak — clippy'yi gerçekten çözer, ama imza değişikliği ve `run_plain_loop`'un testi yok.

**Seçilen: struct.** Kullanıcı "düzelt" dedi, susturmak değil. Risk azaltıcı: tek çağrı yeri, alanlar birebir aynı değerler, derleyici eksik/fazla alanı yakalar.

## Sıra

`cargo fmt` **önce** gider. Sonra çalıştırılsaydı bölme diff'leriyle çakışır, hangi satırın taşıma hangisinin biçim olduğu ayrılamazdı.

## Doğrulama

Bölme task'ları için v0.22.0'ın barı aynen geçerli: **`cargo test` → 372 passed, 0 failed**, her task sonunda birebir. `run_plain_loop` struct task'ı bu barın istisnası değil — imza değişiyor ama davranış değişmiyor, sayı yine 372.

`cargo fmt` task'ından sonra `cargo fmt --check` temiz çıkmalı — bu turun ürünlerinden biri.

## İlgili
- Plan: `docs/superpowers/plans/2026-08-17-cleanup-round.md`
- Önceki tur: `docs/superpowers/specs/2026-08-16-module-split-design.md`
