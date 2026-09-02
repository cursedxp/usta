# Ekran modeli koşumu + ölçüme dayalı resize düzeltmesi — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** `main` dalı, v0.30.1, temiz ağaç. Spec: `docs/superpowers/specs/2026-09-02-screen-model-harness-design.md`; çelişkide spec kazanır. Bağlam olarak `docs/superpowers/specs/2026-09-01-relative-render-design.md` (K1–K5) geçerlidir. İş dalı: `git switch -c screen-model-harness`.

**Goal:** Resize kalıntısını iddia etmeyi bırak, ölç. `Screen`'in ürettiği baytları gerçekten bir ekrana uygulayan bir model yaz, Anil'in ekran görüntüsündeki birikmeyi **kırmızı bir testle yeniden üret**, sonra testleri yeşile çeviren en küçük değişikliği yap. Hedef sürüm **v0.31.0**.

**Architecture:** Yeni test-only modül `src/tui/screen_model.rs` (`#[cfg(test)]`). Üretim kodunda tek dosya değişir: `src/tui/screen.rs`. Yeni bağımlılık YOK — model yalnız `Screen`'in ürettiği dar kaçış kümesini anlar.

**Tech Stack:** Rust. Binary crate — filtreli `cargo test <filtre>`.

## Global Constraints

- **Görev sırası bağlayıcı: Task 3 (kırmızı üretme) Task 4'ten (düzeltme) ÖNCE gelir ve kırmızı görülmeden Task 4'e geçilmez.** Kırmızı çıkmıyorsa model veya senaryo yanlıştır; STOP ve raporla. Bu planın var olma sebebi, iki turdur ölçmeden düzeltme yapılmış olmasıdır.
- **Düzeltmenin şekli önceden verilmemiştir.** Spec'teki teşhis (`resize`'ın hem silip hem `forget_block` çağırması; `painted*2` kırpması) bir HİPOTEZDİR. Testleri yeşile çeviren en küçük değişiklik ne ise o yapılır; hipotez tutmuyorsa raporla ve ölçümü izle.
- **K3:** üretim kodunda `MoveTo(`, `MoveToRow`, `cursor::position()`, `SavePosition`/`RestorePosition` YOK. Yalnız `MoveUp`/`MoveDown`/`MoveToColumn`.
- **K4:** her `paint` `Clear(ClearType::FromCursorDown)` ile biter.
- **Metin kaybı yasağı:** hiçbir senaryoda bloğun ÜSTÜNDEKİ transcript satırları silinmemelidir. Senaryo 4 bunu ölçer.
- Mevcut bayt-seviyesi testler KALDIRILMAZ, gevşetilmez.
- Her task: TDD; task sonunda `cargo test` TÜMÜ yeşil, `cargo clippy --all-targets` 0 uyarı, sonra commit.
- Bilinen ortam hatası, düzeltilmez ve engellemez: `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt`.
- **Push / merge / tag / `cargo install` YOK. `git stash` YOK.**
- `src/tui/run.rs`, `src/plain.rs`, `src/tui/editor.rs` DEĞİŞMEZ.
- İngilizce: tanımlayıcılar, string'ler, yorumlar, commit mesajları.

---

### Task 1: `TermModel` — ekran ızgarası

**Files:** Create `src/tui/screen_model.rs`; Modify `src/tui/mod.rs` (`#[cfg(test)] mod screen_model;`)

**Interfaces:** spec "Model" bölümündeki imzalar. `apply` yalnız şunları tanır: `MoveUp(n)` `ESC[nA` · `MoveDown(n)` `ESC[nB` · `MoveToColumn(n)` `ESC[nG` · `Clear(CurrentLine)` `ESC[2K` · `Clear(FromCursorDown)` `ESC[0J`/`ESC[J` · `\r` · `\n` · düz metin (ANSI SGR `ESC[...m` sessizce yutulur, ızgarayı etkilemez). **Başka her kaçış `panic!` (M2).**

- [ ] **Step 1: Failing test.** Modelin kendi testleri: metin doğru hücreye yazılır · `\r\n` satır başı + alt satır · `ESC[2K` yalnız o satırı siler · `ESC[0J` imleçten aşağıyı siler · `MoveUp`/`MoveDown` ekran sınırında durur · son satırda `\n` ızgarayı yukarı kaydırır · SGR yutulur · bilinmeyen kaçış panik eder.
- [ ] **Step 2:** FAIL, doğru sebep.
- [ ] **Step 3:** Implementasyon.
- [ ] **Step 4:** test + clippy + fmt. Commit: `test: TermModel — apply the bytes we emit to an actual grid`

---

### Task 2: Yeniden boyutlandırma politikaları

**Files:** Modify `src/tui/screen_model.rs`

**Interfaces:** `enum ResizePolicy { Reflow, NoReflow }`; `TermModel::resize(&mut self, w: u16, h: u16, policy: ResizePolicy)`.
- `Reflow`: mantıksal satırlar yeni genişliğe göre yeniden sarılır; imleç üzerinde bulunduğu içerikle taşınır.
- `NoReflow`: her satır bir fiziksel satır kalır; yeni genişliği aşan kısım kırpılır; imleç satırı değişmez.

- [ ] **Step 1: Failing test.** 80 sütunda 80 karakterlik bir satır → 40'a daralt: `Reflow` iki satır, `NoReflow` bir satır · genişletmede `Reflow` sert-sonlandırılmış satırları BİRLEŞTİRMEZ (her satır kendi mantıksal satırıdır) · imleç taşınması iki politikada da doğrulanır.
- [ ] **Step 2:** FAIL, doğru sebep.
- [ ] **Step 3:** Implementasyon.
- [ ] **Step 4:** test + clippy + fmt. Commit: `test: model both terminal resize policies — reflow and no-reflow`

---

### Task 3: Hatayı yeniden üret — KIRMIZI GÖRÜLECEK

**Files:** Modify `src/tui/screen.rs` (yalnız test modülü)

**Interfaces:** `Screen<Vec<u8>>` ile boyanan baytlar `TermModel::apply`'a beslenir; boyut değişimlerinde önce `model.resize(...)`, sonra `screen.resize(...)`, sonra `screen.paint(...)` — gerçek sıradaki gibi (terminal önce reflow yapar, sonra olay bize ulaşır).

Spec'teki beş senaryo × iki politika. Kabul ölçütü: kural karakteri (`─`) içeren satır sayısı tam **2**; senaryo 4'te ayrıca bloğun üstündeki transcript satırları bozulmamış.

- [ ] **Step 1:** Senaryoları yaz.
- [ ] **Step 2:** Koş. **En az bir senaryonun KIRMIZI olduğunu gör ve çıktısını rapora yaz** (kaç kural satırı sayıldı, hangi politika, hangi senaryo). Kırmızı yoksa **DUR ve raporla** — model ya da senaryo gerçeği yakalamıyor demektir, düzeltmeye geçme.
- [ ] **Step 3:** Kırmızı senaryolar `#[ignore = "reproduces the v0.30.1 residue; un-ignored by the fix task"]` ile işaretlenip commit edilir — böylece hata ağaçta kanıt olarak durur. Commit: `test: reproduce the resize residue on a modelled screen (currently failing)`

---

### Task 4: Ölçüme dayalı düzeltme

**Files:** Modify `src/tui/screen.rs` (üretim + test)

- [ ] **Step 1:** Task 3'ün `#[ignore]`'ları kaldırılır; kırmızı doğrulanır.
- [ ] **Step 2:** Testleri yeşile çeviren **en küçük** değişiklik yapılır. Hipotez (reçete değil): silme sorumluluğu tek atışlık `resize`'dan alınıp `paint`'in `painted` tabanlı silmesinde tutulabilir; `painted*2` kırpması yetersiz kalıyor olabilir. Ölçüm ne diyorsa o uygulanır.
- [ ] **Step 3:** Beş senaryo × iki politika yeşil. K3/K4 taraması temiz. Mevcut bayt testleri hâlâ yeşil.
- [ ] **Step 4:** Isırma doğrulaması: düzeltmeyi geçici olarak geri al → senaryoların kırmızıya döndüğünü gör → geri koy.
- [ ] **Step 5:** test + clippy + fmt. Commit: `fix: resize residue — <ölçümün gösterdiği tek cümlelik sebep>`

---

### Task 5: Belgeleme + sürüm

**Files:** Modify `SPEC.md` (§4.19), `docs/ROADMAP.md`, `Cargo.toml` (0.31.0), `src/tui/welcome_tests.rs`, `Cargo.lock`

- [ ] **Step 1:** `SPEC.md` §4.19: v0.30.1'in neden yetmediği (bayt dizisi doğru, ekran yanlış — test yüzeyi boşluğu) · `TermModel` koşumu ve iki politika · ölçümün gösterdiği gerçek sebep · artık hangi iddianın ölçülü olduğu. v0.29.1/v0.30.0/v0.30.1 paragrafları SİLİNMEZ; "ölçülmedi" ifadeleri ölçüm sonucuyla değiştirilir.
- [ ] **Step 2:** `docs/ROADMAP.md` `## Completed` başına kayıt, `v0.31.0.` ile biter.
- [ ] **Step 3:** Sürüm 0.31.0 (`Cargo.toml` + `welcome_tests.rs` pini), `cargo check`.
- [ ] **Step 4:** `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`. Commit: `docs: SPEC 4.19 + ROADMAP — measured resize fix; bump to v0.31.0`

---

## Manuel Doğrulama (Anil ile, plan bittikten sonra)

`cargo install --path .` sonrası — artık keşif değil, ölçütün doğrulaması:

1. `usta reset --factory` sonrası uzun terminalde taze oturum → blok ortada, altında boşluk. Yatay daralt/genişlet → kural çizgisi tam olarak 2 tane.
2. Kenardan hızlıca sürükle → sürükleme bitince tek temiz blok, birikme yok.
3. Terminali üçten fazla kat daralt → aynı.
4. Dolu transcript'li uzun oturumda aynı testler → kalıntı yok **ve** yukarıdaki metin duruyor.
5. Mentor yanıtı beklerken (spinner) resize → aynı.
