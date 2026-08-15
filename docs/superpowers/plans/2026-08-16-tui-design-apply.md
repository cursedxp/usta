# TUI Design System Uygulaması Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** `2026-08-15-progress-stats`, `2026-08-15-mock-exam`, `2026-08-15-gamification` planları MERGE EDİLMİŞ olmalı (stats/exam/game yüzeyleri mevcut olmalı). Spec: `docs/superpowers/specs/2026-08-16-tui-design-apply-design.md` — önce oku. Kaynak-of-truth: Claude Design projesi `f8cc2dc7-e09d-4b62-9000-1a023265fdc9` — ToolSearch ile `mcp__claude-design__list_files,mcp__claude-design__read_file` yükle ve BEŞ mockup sayfasını oku; mockup ile bu plan çelişirse mockup kazanır (sapmayı raporla).

**Goal:** Onaylı tasarım sistemi koda: `theme.rs` (semantik renk+glif), welcome/input/status restyle, notice katmanları (`·`/`⚠`/`✗`), exam format kuralı (model-çizimli kart), stats/topics/help hizalama; v0.18.0.

**Architecture:** Tek kaynak `src/tui/theme.rs`; tüm TUI modülleri + `ui.rs` plain-ANSI oradan beslenir. Davranış değişmez — yalnız sunum. Exam kartı kabukta DEĞİL, GOAL.md format kuralıyla model çizer.

**Tech Stack:** Rust/ratatui (mevcut), yeni bağımlılık YOK.

## Global Constraints

- Renkler `Color::Indexed`: marka 208 · başarı 149 · uyarı 179 (amber — `Color::Yellow` literal'leri ölür) · hata 210 · oyun 141 · ortam dim.
- Glif çiftleri: `·` bilgi · `✓` başarı · `⚠` uyarı · `✗` hata · `▸` oyun · `●` marka · `❯` prompt.
- Turuncu disiplini: durağan ekranda ≤2 turuncu öğe (logo bloğu = 1 öğe).
- Davranış/metin regresyonu yok — glif ön-eki eklenen assert'ler güncellenir, başka metin değişmez.
- Her task sonunda commit (Türkçe mesaj) + push + `cargo test` yeşil.

---

### Task 1: `src/tui/theme.rs` — tek kaynak

**Files:** Create: `src/tui/theme.rs` · Modify: `src/tui/mod.rs` (`pub mod theme;`) · testler in-module

**Interfaces (Produces):**
```rust
pub const BRAND: Color = Color::Indexed(208);
pub const SUCCESS: Color = Color::Indexed(149);
pub const WARN: Color = Color::Indexed(179);
pub const ERROR: Color = Color::Indexed(210);
pub const GAME: Color = Color::Indexed(141);
pub const G_INFO: &str = "·"; pub const G_OK: &str = "✓"; pub const G_WARN: &str = "⚠";
pub const G_ERR: &str = "✗"; pub const G_GAME: &str = "▸"; pub const G_BRAND: &str = "●"; pub const G_PROMPT: &str = "❯";
pub fn info() -> Style; pub fn success() -> Style; pub fn warn() -> Style;
pub fn error() -> Style; pub fn game() -> Style; pub fn brand() -> Style;
pub const SPINNER: [&str; 4] = ["⠋", "⠙", "⠸", "⠴"];
```

- [ ] **Step 1: Failing test** — semantik eşleme assert'i (vacuous değil):

```rust
#[test]
fn theme_semantics_locked() {
    assert_eq!(WARN, Color::Indexed(179)); // amber — NOT Color::Yellow
    assert_ne!(WARN, Color::Yellow);
    assert_eq!(GAME, Color::Indexed(141));
    assert_eq!(BRAND, Color::Indexed(208));
    for (g, s) in [(G_OK, success()), (G_WARN, warn()), (G_ERR, error()), (G_GAME, game())] {
        assert!(!g.is_empty());
        assert!(s.fg.is_some());
    }
}
```

- [ ] **Step 2:** Run: `cargo test theme` → derleme hatası
- [ ] **Step 3:** Implement (yukarıdaki sabitler + `Style::default().fg(..)` yardımcıları; `info()` = dim: `Style::default().add_modifier(Modifier::DIM)`).
- [ ] **Step 4:** `cargo test theme` → PASS
- [ ] **Step 5:** Commit + push: `tema: theme.rs — semantik renk + glif tek kaynak`

---

### Task 2: Renk literal'i göçü (welcome, run, ui)

**Files:** Modify: `src/tui/welcome.rs`, `src/tui/run.rs`, `src/tui/*` (renk kullanan her yer), `src/ui.rs` (plain-mode ANSI eşdeğerleri: 208→`\x1b[38;5;208m` vb. — mevcut ANSI yapısı neyse ona uyarla)

- [ ] **Step 1:** `grep -rn "Color::" src/` çıkar — her literal theme sabitine eşlenir (Yellow→WARN, mevcut 208/114→BRAND/SUCCESS kararını mockup 01'e göre ver; 114 yeşilse SUCCESS=149'a göç, sapmayı raporla).
- [ ] **Step 2:** Göç + turuncu disiplini: welcome render'larında turuncu span sayısını say, ≤2'ye indir (mockup 02 hangi öğeleri turuncu bırakıyorsa onlar: logo + tek vurgu).
- [ ] **Step 3: Test** — welcome render'da turuncu disiplin assert'i:

```rust
#[test]
fn welcome_orange_discipline() {
    // render identity welcome; BRAND fg'li span gruplarını say (logo bloğu = 1) → ≤2
}
```
(Gövde mevcut render-test desenleriyle; span stillerine erişim yolu koda göre.)
- [ ] **Step 4:** `cargo test` → PASS (bozulan stil assert'leri güncellenir — davranış değil sunum).
- [ ] **Step 5:** Commit + push: `tema: renk literal göçü — amber uyarı, turuncu disiplini`

---

### Task 3: Input + status satırı

**Files:** Modify: `src/tui/run.rs` (render_status, draw), input kutusu çizen modül (koda bak — `InputBox`/term)

- [ ] **Step 1:** Mockup 02'nin 4 durumu: idle (`❯` turuncu, dim yardım), thinking (SPINNER glifleri ~120ms — mevcut spinner mekanizmasının frame setini değiştir), watching göstergesi (`·` dim), context gauge `▓░` + ≥%70 amber (eşik hesabı: `tokens/context_window`).
- [ ] **Step 2:** Status tek `Line` ≤3 span kuralına indirgenir (fazlaysa birleştir).
- [ ] **Step 3:** Testler: spinner seti assert; gauge eşik fonksiyonu saf test (`gauge_style(pct) -> Style`: 69→normal, 70→WARN).
- [ ] **Step 4:** `cargo test` → PASS. Commit + push: `tema: input/status — spinner, gauge amber eşiği, span diyeti`

---

### Task 4: Notice katmanları

**Files:** Modify: `src/tui/run.rs` (`page_notice` çağrı noktaları / yardımcıları), `src/ui.rs` (`notice`, `warn`, hata basımları)

- [ ] **Step 1:** Üç katman: bilgi `· ` dim · uyarı `⚠ ` amber · hata `✗ ` kırmızı. Mevcut metinler AYNEN — yalnız ön-ek + stil. Hata yolu: `Err` basan yerler (`file feedback skipped`, flush hataları) `✗`.
- [ ] **Step 2:** Testler: `ui::warn` çıktısı `⚠` içerir (yakalanabilirse; değilse fonksiyon dönüş/format saf testi), glif ön-eki assert'leri; glif ön-eki bekleyen ESKİ assert'ler güncellenir.
- [ ] **Step 3:** `cargo test` → PASS. Commit + push: `tema: notice katmanları — · / ⚠ / ✗ glif+renk çiftleri`

---

### Task 5: Exam + game + stats/topics/help biçimi

**Files:** Modify: `GOAL.md` (Mock Exams format kuralı), `TEACHING.md` (game doz kuralına glif notu), `src/main.rs` (`render_stats`, topics çıktısı), `src/help.rs`

- [ ] **Step 1: GOAL.md** Mock Exams'a ek: `Format each question as a `── Question N/M ──` header line; after the options/prompt, show progress as filled/empty dots (e.g. ●●●○○). The final scorecard is a table: map item | result (✓/✗) | note, with a light rule under the header.`
- [ ] **Step 2: TEACHING.md** Gamification doz kuralına: `Game lines open with ▸ (one short line, violet in the TUI).`
- [ ] **Step 3: stats/topics/help** — mockup 05: kolon hizası (unicode-width), başlık altı ince çizgi (`─` tekrarı), stats'ta `✓`/`·` glifleri; help kartı bölüm başlıkları. Mevcut içerik metinleri korunur; testlerdeki içerik assert'leri dayanır, hizalama assert'i eklenir (çizgi satırı var).
- [ ] **Step 4:** `cargo test` → PASS. Commit + push: `tema: exam format kuralı + stats/topics/help hizalama`

---

### Task 6: Docs + v0.18.0

**Files:** `SPEC.md`, `README.md`, `docs/ROADMAP.md`, `Cargo.toml`(+lock), sürüm testi

- [ ] **Step 1:** SPEC yeni § (v0.18): tema tek-kaynak, semantik tablo (renk+glif), turuncu disiplini, exam format kuralı.
- [ ] **Step 2:** README (İngilizce) Terminal UI satırına ek: `...; a calm, colorblind-safe visual language (glyph+color pairs, one accent), designed in a full TUI design system.`
- [ ] **Step 3:** ROADMAP Tamamlananlar'a satır (tasarım uygulaması — roadmap numarası yok, bağımsız iş).
- [ ] **Step 4:** Cargo `0.18.0`; sürüm testi; `cargo build`.
- [ ] **Step 5:** Verify: `cargo test` PASS · clippy yeni uyarı 0 · `cargo install --path .`.

```bash
git add SPEC.md README.md docs/ROADMAP.md Cargo.toml Cargo.lock src/ GOAL.md TEACHING.md
git commit -m "tema: SPEC + README + tasarım sistemi uygulandı — v0.18.0"
git push
git tag v0.18.0 && git push --tags
```

- [ ] **Step 6 (elle doğrulama — ATLA, Anil koşacak):** dark+light terminalde welcome/status/notice görünümü; `/exam` soru formatı; `usta stats` hizalama; turuncu sayımı.
