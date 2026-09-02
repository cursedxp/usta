# Reflow politikası girdisi — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** Dal `screen-model-harness`, HEAD `ff036a6`, temiz ağaç, 6 commit. **Bu dalda devam edilir, yeni dal AÇILMAZ.** Spec: `docs/superpowers/specs/2026-09-02-reflow-policy-design.md`; bağlam: `2026-09-02-screen-model-harness-design.md` (koşum) ve `2026-09-01-relative-render-design.md` (K1–K5). Çelişkide en yeni spec kazanır.

**Goal:** Silme aritmetiğini kör bir sayı olmaktan çıkar. Terminalin yeniden-sarma davranışı bir girdi olur, her politika için silme TAM hesaplanır, `painted*2` kırpması tamamen kalkar. Böylece koşumdaki `#[ignore]` gerçekten açılır ve beş senaryo iki politikada da yeşil olur. Hedef sürüm **v0.31.0**.

**Architecture:** Üretimde iki dosya: `src/tui/screen.rs` (politika alanı + aritmetik), `src/tui/term.rs` (kurulumda tespit). `detect_reflow` saf ve enjekte edilebilir bir `get` alır — proses ortamına dokunan test YAZILMAZ (paralel test koşumunda `set_var` yarış üretir). Test modeli (`screen_model.rs`) üretime TAŞINMAZ.

**Tech Stack:** Rust, std env. Yeni bağımlılık YOK. Binary crate — filtreli `cargo test <filtre>`.

## Global Constraints

- **Varsayılanın yönü bağlayıcı:** bilinmeyen terminal → `NoReflow`. Gerekçe spec P3: eksik silme kalıntı bırakır (geri alınabilir), fazla silme kullanıcının metnini yok eder (geri gelmez). Bu yönü tersine çeviren hiçbir "daha iyi görünüyor" argümanı kabul edilmez.
- **`painted*2` kırpması silinir** ve kaynak-pin ile geri gelmesi engellenir.
- **Hiçbir senaryo `#[ignore]` ile susturulmaz.** Bir senaryo eşleşen politikayla yeşil olamıyorsa DUR ve raporla.
- K3: üretimde `MoveTo(`, `MoveToRow`, `cursor::position()`, `SavePosition`/`RestorePosition` YOK. K4: her `paint` `Clear(ClearType::FromCursorDown)` ile biter.
- Metin kaybı yasağı: eşleşen politikada bloğun ÜSTÜNDEKİ transcript satırları silinmez.
- Her task: TDD; task sonunda `cargo test` TÜMÜ yeşil, `cargo clippy --all-targets` 0 uyarı, sonra commit.
- Bilinen ortam hatası, düzeltilmez, engellemez: `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt`.
- **Push / merge / tag / `cargo install` YOK. `git stash` YOK.**
- `src/tui/run.rs`, `src/plain.rs`, `src/tui/editor.rs` DEĞİŞMEZ.
- İngilizce: tanımlayıcılar, string'ler, yorumlar, commit mesajları.

---

### Task 1: `detect_reflow` — saf tespit

**Files:** Modify `src/tui/screen.rs` (veya `term.rs` — implementasyonun seçimi, ama `Screen` ile aynı modülde olması tercih edilir)

**Interfaces:** `pub(crate) enum ReflowPolicy { Reflow, NoReflow }` · `pub(crate) fn detect_reflow(get: impl Fn(&str) -> Option<String>) -> ReflowPolicy` — spec P2'deki dört adımlı sıra birebir.

- [ ] **Step 1: Failing test.** Sahte `get` ile: `USTA_TERM_REFLOW=1` → `Reflow`; `=0` → `NoReflow`; override `TMUX` tanımlıyken bile kazanır · `TMUX` tanımlı + `TERM_PROGRAM=vscode` → `NoReflow` (çoklayıcı önceliği) · `TERM=screen-256color` → `NoReflow` · `TERM=tmux-256color` → `NoReflow` · `TERM_PROGRAM=vscode` → `Reflow` · `TERM_PROGRAM=iTerm.app` → `Reflow` · `KITTY_WINDOW_ID` tanımlı → `Reflow` · boş ortam → `NoReflow` · bilinmeyen `TERM_PROGRAM=SomeThing` → `NoReflow`.
- [ ] **Step 2:** FAIL, doğru sebep.
- [ ] **Step 3:** Implementasyon. **Proses ortamına dokunan test yok** — hepsi enjekte edilen `get` üzerinden.
- [ ] **Step 4:** test + clippy + fmt. Commit: `feat: detect the terminal's reflow policy from the environment, conservative by default`

---

### Task 2: Aritmetiği politikaya bağla

**Files:** Modify `src/tui/screen.rs`, `src/tui/term.rs`

**Interfaces:** `Screen::new(out, size, policy)`; `rewrapped_rows(..., policy)`, `descend_rows(..., policy)`. `NoReflow`'da sırasıyla `painted` ve `cursor_up` döner. `painted*2` kırpması SİLİNİR. `term::setup()` `detect_reflow(|k| std::env::var(k).ok())` çağırır.

- [ ] **Step 1: Failing test.** `rewrapped_rows`/`descend_rows` iki politikada: `NoReflow`'da genişlik yarıya inse bile `painted`/`cursor_up` döner; `Reflow`'da `Σ ceil(w_i/new_w)` döner ve **tavan yoktur** (200→60 vakasında 10 döner, 8 değil).
- [ ] **Step 2:** FAIL, doğru sebep.
- [ ] **Step 3:** Implementasyon.
- [ ] **Step 4:** Kaynak-pin: `screen.rs` üretim gövdesinde `saturating_mul(2)` YOK.
- [ ] **Step 5:** test + clippy + fmt. Commit: `fix: compute the erase exactly per reflow policy, drop the painted*2 ceiling`

---

### Task 3: Senaryo matrisi — `#[ignore]` kalkar

**Files:** Modify `src/tui/screen.rs` (test modülü)

- [ ] **Step 1:** Senaryo yardımcıları model politikası ile `Screen` politikasını AYRI parametre alacak şekilde genelleştirilir.
- [ ] **Step 2:** Beş senaryo × eşleşen politika (Reflow/Reflow, NoReflow/NoReflow) → hepsi yeşil. `src/tui/screen.rs:1114`'teki `#[ignore]` KALDIRILIR. Yeşil olmayan kalırsa **DUR ve raporla**.
- [ ] **Step 3: Uyuşmazlık testleri (spec P4).** İki çapraz kombinasyon yazılır ve beklenen bozulma ölçüsüyle assert edilir: model `NoReflow` + Screen `Reflow` → kaç transcript satırı kaybedildiği; model `Reflow` + Screen `NoReflow` → kaç kalıntı satırı kaldığı. Test adları ve yorumları bunların **kabul edilmiş maliyet** olduğunu, hata olmadığını söyler.
- [ ] **Step 4:** Isırma doğrulaması: `detect_reflow`'un varsayılanını geçici olarak `Reflow`'a çevir → `NoReflow` senaryolarının kırmızıya döndüğünü gör → geri al.
- [ ] **Step 5:** test + clippy + fmt. Commit: `test: the scenario matrix passes under both policies; no scenario stays ignored`

---

### Task 4: Belgeleme + sürüm

**Files:** Modify `SPEC.md` (§4.19), `docs/ROADMAP.md`, `Cargo.toml` (0.31.0), `src/tui/welcome_tests.rs`, `Cargo.lock`; ayrıca `docs/superpowers/plans/2026-09-02-screen-model-harness.md` manuel doğrulama listesine VS Code entegre terminali BİRİNCİ sıraya eklenir (önceki turdan devreden iş).

- [ ] **Step 1:** `SPEC.md` §4.19: v0.30.1'in neden yetmediği (bayt dizisi doğru, ekran yanlış — test yüzeyi boşluğu) · `TermModel` koşumu · **ölçülen imkânsızlık** (iki politika aynı durumdan zıt davranış istiyor) · politikanın artık girdi olduğu ve ortamdan okunduğu · varsayılanın yönü ve gerekçesi (metin kaybı geri gelmez, kalıntı gelir) · yanlış tespitin ölçülü bedeli · `USTA_TERM_REFLOW` kaçış kapısı. v0.29.1/v0.30.0/v0.30.1 paragrafları SİLİNMEZ; "ölçülmedi" ifadeleri ölçüm sonucuyla değiştirilir.
- [ ] **Step 2:** `docs/ROADMAP.md` `## Completed` başına kayıt, `v0.31.0.` ile biter. Koşumun kendisi de kayda girer — asıl kazanım o.
- [ ] **Step 3:** Sürüm 0.31.0 (`Cargo.toml` + `welcome_tests.rs` pini), `cargo check`.
- [ ] **Step 4:** `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`. Commit: `docs: SPEC 4.19 + ROADMAP — reflow policy as an input; bump to v0.31.0`

---

## Manuel Doğrulama (Anil ile, plan bittikten sonra)

`cargo install --path .` sonrası:

1. **VS Code entegre terminali — ilk ve asıl test** (bozulduğu yer). `usta reset --factory` sonrası uzun panelde taze oturum → blok ortada, altında boşluk. Daralt / genişlet / kenardan hızlıca sürükle → kural çizgisi tam 2 tane, birikme yok, üstteki metin duruyor.
2. Terminali üçten fazla kat daralt → aynı.
3. Dolu transcript'li uzun oturumda aynı testler.
4. `USTA_TERM_REFLOW=0 usta` ile aynı testler → kalıntı görülebilir, ama **metin kaybı OLMAMALI**. Varsayılanın yönünün doğrulaması budur.
5. Mentor yanıtı beklerken (spinner) resize → aynı.
