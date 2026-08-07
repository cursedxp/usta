# Claude Code Tarzı TUI Arayüzü — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Not:** Bu plan ayrı bir session'da koşulmak üzere yazıldı. Spec: `docs/superpowers/specs/2026-08-07-tui-interface-design.md` — başlamadan önce OKU.

**Goal:** `usta start` etkileşimli oturumunu ratatui inline-viewport TUI'sine taşı: çift kolonlu açılış kutusu + canlı dört-kenarlı girdi kutusu + yapışık durum satırı; akış normal scrollback'te kalır, plain mod bugünkü yol.

**Architecture:** Ratatui `Viewport::Inline` alt bölgeyi (girdi kutusu + durum satırı) canlı çizer; kalıcı içerik `terminal.insert_before()` ile scrollback'e basılır. TUI yolunda rustyline yerine crossterm `EventStream` + `tui-input`; `ui::is_plain()` ise TUI hiç açılmaz, mevcut rustyline yolu aynen çalışır. LLM beklerken iç `select!` döngüsü spinner çizer, Enter gönderimi kilitli.

**Tech Stack:** Rust, tokio, ratatui, tui-input, ansi-to-tui, unicode-width, termimad (render-only), crossterm (event-stream), rustyline (sadece plain yol).

## Global Constraints

- Kod yorumları ve UI metinleri **Türkçe** (mevcut kod stili).
- Renkler değişmez: turuncu = ANSI 208, inline-code yeşili = 114, soluk = DIM. (`src/ui.rs:11-14`)
- **Alternate screen YASAK** — sadece `Viewport::Inline` + `insert_before`; scrollback korunur.
- `ui::is_plain()` yolu (TTY yok / `NO_COLOR`) davranışsal olarak birebir korunur; mevcut testler kırılamaz.
- Crate sürümleri hedef: `ratatui = "0.29"`, `tui-input = "0.11"`, `ansi-to-tui = "7"`, `unicode-width = "0.2"`, `crossterm = { version = "0.28", features = ["event-stream"] }`. **API'ler sürüme göre kaymış olabilir** — derleme uyuşmazlığında docs.rs'ten eşdeğer çağrıyı bul, amacı koru (ui.rs'teki termimad notuyla aynı ilke).
- Ağaçta TEK crossterm sürümü olmalı (termimad + ratatui aynı sürümü paylaşır) — Task 1 doğrular.
- Test komutu: `cargo test --quiet`. Commit mesajları Türkçe, mevcut `alan: özet — gerekçe` stili.
- Genişlik hesapları HER ZAMAN `unicode-width` ile (byte/char sayımı değil).
- Viewport yüksekliği sabit `6` (girdi kutusu 3-5 satır + durum 1); girdi 3 iç satırı aşarsa iç kaydırma (son satırlar görünür). Spec §6'daki "üst sınır + iç kaydırma"nın sınırlı gerçeklemesi budur.

## Dosya Haritası

| Dosya | Sorumluluk |
|---|---|
| `src/tui/mod.rs` | modül çatısı + `pub use` |
| `src/tui/welcome.rs` | `WelcomeData` toplama (saf parse) + `render_welcome` |
| `src/tui/editor.rs` | `InputBox` (tui-input sarmalayıcı) + satır tarihçesi + tuş → `Action` |
| `src/tui/convert.rs` | termimad ANSI → ratatui `Text` (fallback'li) |
| `src/tui/status.rs` | durum satırı: spinner karesi + bağlam göstergesi |
| `src/tui/term.rs` | terminal kurulum/restore + panic hook guard |
| `src/tui/run.rs` | TUI olay döngüsü (`run()`), `insert_before` akışı |
| `src/main.rs` | TUI/plain dallanması; mevcut döngü `run_plain_loop`'a taşınır |
| `src/ui.rs` | `render_markdown()` (skin'i String'e render) eklenir |

---

### Task 1: Bağımlılıklar + crossterm tekilliği

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/main.rs:17` (mod tui kaydı — boş modülle derleme yeşil kalsın)
- Create: `src/tui/mod.rs`

**Interfaces:**
- Produces: derlenen bağımlılık seti; `crate::tui` modül yolu.

- [ ] **Step 1: Cargo.toml'a bağımlılıkları ekle**

`[dependencies]` bölümüne:

```toml
ratatui = "0.29"
tui-input = "0.11"
ansi-to-tui = "7"
unicode-width = "0.2"
crossterm = { version = "0.28", features = ["event-stream"] }
```

- [ ] **Step 2: Boş tui modülünü kur**

`src/tui/mod.rs`:

```rust
//! Claude Code tarzı TUI: inline viewport + insert_before akışı.
//! Plain modda (ui::is_plain) bu modül hiç kullanılmaz.

pub mod convert;
pub mod editor;
pub mod run;
pub mod status;
pub mod term;
pub mod welcome;
```

Şimdilik her alt dosyayı boş oluştur (`//! yer tutucu` yorumlu) — Task 2-7 doldurur. `src/main.rs`'te mod listesine `mod tui;` ekle (alfabetik sıra: `transcript` ile `ui` arası).

- [ ] **Step 3: Derle + crossterm tekilliğini doğrula**

```bash
cargo build 2>&1 | tail -3
cargo tree -d -i crossterm 2>/dev/null; cargo tree | grep -c "crossterm v"
```

Beklenen: build OK; `cargo tree -d` crossterm için ÇIFT sürüm göstermemeli. Çift sürüm varsa: termimad'ı crossterm 0.28 kullanan sürüme yükselt (`cargo add termimad@<uygun>`), yeniden doğrula. Termimad API'si değiştiyse `ui.rs`'teki çağrıları docs.rs'e göre uyarla (amaç aynı: başlık/bold 208, inline-code 114).

- [ ] **Step 4: Test + commit**

```bash
cargo test --quiet   # mevcut 105 test yeşil kalmalı
git add Cargo.toml Cargo.lock src/main.rs src/tui/
git commit -m "tui: bağımlılıklar + modül iskeleti — ratatui inline viewport hazırlığı"
```

---

### Task 2: Açılış kutusu veri katmanı (saf parse)

**Files:**
- Create: `src/tui/welcome.rs` (veri kısmı)
- Test: aynı dosyada `#[cfg(test)]`

**Interfaces:**
- Produces:
  - `pub struct WelcomeData { pub version: &'static str, pub name: Option<String>, pub model: String, pub dir: String, pub topic: String, pub level: Option<String>, pub map_percent: Option<u8>, pub next_item: Option<String>, pub drill_count: usize, pub first_session: bool }`
  - `pub fn gather(profile: Option<&str>, progress: Option<&str>, curriculum: Option<&str>, topic: &str, model: &str, dir: &str) -> WelcomeData`
  - yardımcılar: `extract_name`, `extract_level`, `curriculum_percent`, `next_unseen`, `drill_count`, `section`
- Consumes: yok (saf string girdi).

- [ ] **Step 1: Failing testleri yaz**

`src/tui/welcome.rs`:

```rust
//! Açılış kutusu: veri toplama (saf) + render. Spec §5.
//! Tüm parse'lar best-effort — bozuk/eksik girdi alanı atlar, asla panik yok.

/// Açılış kutusunun tüm verisi — render bu struct'tan çizer, IO yapmaz.
pub struct WelcomeData {
    pub version: &'static str,
    pub name: Option<String>,
    pub model: String,
    pub dir: String,
    pub topic: String,
    pub level: Option<String>,
    pub map_percent: Option<u8>,
    pub next_item: Option<String>,
    pub drill_count: usize,
    pub first_session: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROFILE: &str = "# Öğrenci Profili — Anil\n\n## Kim\n- test";
    const PROGRESS: &str = "# rust — İlerleme\n## Seviye\n- Orta: ownership oturdu\n## Geri çağırma soruları\n- Soru 1? — cevap\n- Soru 2? — cevap\n- Soru 3? — cevap\n";
    const CURRICULUM: &str = "# rust haritası\n- Ownership: oturdu\n- Borrowing: görüldü\n- Lifetimes: görülmedi\n- Traits: görülmedi\n";

    #[test]
    fn extract_name_reads_h1_after_dash() {
        assert_eq!(extract_name(PROFILE), Some("Anil".to_string()));
        assert_eq!(extract_name("# Başlıksız"), None);
        assert_eq!(extract_name(""), None);
    }

    #[test]
    fn extract_level_reads_first_line_of_section() {
        assert_eq!(extract_level(PROGRESS), Some("Orta: ownership oturdu".to_string()));
        assert_eq!(extract_level("# boş"), None);
    }

    #[test]
    fn curriculum_percent_counts_non_unseen() {
        // 4 durumlu madde, 2'si görülmedi → %50
        assert_eq!(curriculum_percent(CURRICULUM), Some(50));
        assert_eq!(curriculum_percent("# durum yok"), None);
    }

    #[test]
    fn next_unseen_returns_first_unseen_item_text() {
        assert_eq!(next_unseen(CURRICULUM), Some("Lifetimes".to_string()));
        assert_eq!(next_unseen("- Hepsi: oturdu"), None);
    }

    #[test]
    fn drill_count_counts_section_bullets() {
        assert_eq!(drill_count(PROGRESS), 3);
        assert_eq!(drill_count("# soru yok"), 0);
    }

    #[test]
    fn gather_full_and_first_session() {
        let d = gather(Some(PROFILE), Some(PROGRESS), Some(CURRICULUM), "rust", "opus · cli", "~/x");
        assert!(!d.first_session);
        assert_eq!(d.name.as_deref(), Some("Anil"));
        assert_eq!(d.map_percent, Some(50));
        let d2 = gather(None, None, None, "gtm", "opus · cli", "~/x");
        assert!(d2.first_session);
        assert_eq!(d2.drill_count, 0);
    }
}
```

- [ ] **Step 2: Testlerin FAIL ettiğini gör**

```bash
cargo test --quiet tui::welcome 2>&1 | tail -5
```

Beklenen: derleme hatası (`extract_name` tanımsız).

- [ ] **Step 3: Minimal implementasyon**

Test bloğunun ÜSTÜNE:

```rust
/// `## {header}` başlığından bir sonraki `## `e kadarki gövde.
fn section<'a>(md: &'a str, header: &str) -> Option<&'a str> {
    let needle = format!("## {header}");
    let start = md.find(&needle)? + needle.len();
    let rest = &md[start..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    Some(&rest[..end])
}

/// `# Öğrenci Profili — Anil` → `Anil` (em-dash veya tire sonrası).
pub fn extract_name(profile: &str) -> Option<String> {
    let h1 = profile.lines().find(|l| l.starts_with("# "))?;
    let name = h1.rsplit(['—', '-']).next()?.trim();
    if name.is_empty() || name.contains("Profil") { return None; }
    Some(name.to_string())
}

/// `## Seviye` bölümünün ilk dolu satırı, liste işareti soyulmuş.
pub fn extract_level(progress: &str) -> Option<String> {
    section(progress, "Seviye")?
        .lines()
        .map(|l| l.trim().trim_start_matches(['-', '*', ' ']).trim())
        .find(|l| !l.is_empty())
        .map(String::from)
}

const STATUSES: [&str; 4] = ["görülmedi", "görüldü", "oturdu", "derinleşildi"];

/// Durum içeren satır sayımından harita yüzdesi: görülmedi-olmayan / toplam.
pub fn curriculum_percent(curriculum: &str) -> Option<u8> {
    let (mut total, mut seen) = (0u32, 0u32);
    for line in curriculum.lines() {
        if line.contains("görülmedi") { total += 1; }
        else if STATUSES[1..].iter().any(|s| line.contains(s)) { total += 1; seen += 1; }
    }
    if total == 0 { return None; }
    Some(((seen * 100) / total) as u8)
}

/// İlk `görülmedi` maddesinin metni — liste işareti ve durum eki soyulur.
pub fn next_unseen(curriculum: &str) -> Option<String> {
    let line = curriculum.lines().find(|l| l.contains("görülmedi"))?;
    let text = line.split("görülmedi").next()?
        .trim()
        .trim_start_matches(['-', '*', ' '])
        .trim_end_matches([':', '—', '-', '·', '|', ' ']);
    if text.is_empty() { None } else { Some(text.to_string()) }
}

/// `## Geri çağırma soruları` bölümündeki madde sayısı.
pub fn drill_count(progress: &str) -> usize {
    section(progress, "Geri çağırma soruları")
        .map(|s| s.lines().filter(|l| l.trim().starts_with('-')).count())
        .unwrap_or(0)
}

/// Dosya içeriklerinden WelcomeData kur — hepsi Option, eksik = alan atlanır.
pub fn gather(
    profile: Option<&str>, progress: Option<&str>, curriculum: Option<&str>,
    topic: &str, model: &str, dir: &str,
) -> WelcomeData {
    WelcomeData {
        version: env!("CARGO_PKG_VERSION"),
        name: profile.and_then(extract_name),
        model: model.to_string(),
        dir: dir.to_string(),
        topic: topic.to_string(),
        level: progress.and_then(extract_level),
        map_percent: curriculum.and_then(curriculum_percent),
        next_item: curriculum.and_then(next_unseen),
        drill_count: progress.map(drill_count).unwrap_or(0),
        first_session: progress.is_none(),
    }
}
```

- [ ] **Step 4: Testler PASS**

```bash
cargo test --quiet tui::welcome 2>&1 | tail -3
```

- [ ] **Step 5: Commit**

```bash
git add src/tui/welcome.rs
git commit -m "tui: açılış kutusu veri katmanı — profil/progress/curriculum saf parse"
```

---

### Task 3: Açılış kutusu render

**Files:**
- Modify: `src/tui/welcome.rs` (render kısmı eklenir)

**Interfaces:**
- Consumes: `WelcomeData` (Task 2).
- Produces: `pub fn render_welcome(d: &WelcomeData, width: u16) -> ratatui::text::Text<'static>` — `insert_before` ile basılacak hazır metin. Yardımcı: `pub fn fit(s: &str, max: usize) -> String` (unicode-width kırpma, `…` ekler).

- [ ] **Step 1: Failing testler**

Test modülüne ekle:

```rust
    use ratatui::text::Text;

    fn plain_lines(t: &Text) -> Vec<String> {
        t.lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect()
    }

    #[test]
    fn render_welcome_lines_have_equal_display_width() {
        use unicode_width::UnicodeWidthStr;
        let d = gather(Some(PROFILE), Some(PROGRESS), Some(CURRICULUM), "rust", "opus · cli", "~/proje");
        let t = render_welcome(&d, 80);
        let lines = plain_lines(&t);
        assert!(lines.len() >= 8);
        let w = lines[0].width();
        assert!(lines.iter().all(|l| l.width() == w), "hizasız satır: {lines:#?}");
        assert!(lines[0].starts_with('╭') && lines.last().unwrap().starts_with('╰'));
    }

    #[test]
    fn render_welcome_first_session_shows_intro_message() {
        let d = gather(None, None, None, "gtm", "opus · cli", "~/p");
        let joined = plain_lines(&render_welcome(&d, 80)).join("\n");
        assert!(joined.contains("İlk oturum"));
        assert!(joined.contains("Tekrar hoş geldin"));
    }

    #[test]
    fn fit_truncates_by_display_width_with_ellipsis() {
        assert_eq!(fit("çğşöü-uzun-metin", 8), "çğşöü-u…");
        assert_eq!(fit("kısa", 10), "kısa");
    }
```

- [ ] **Step 2: FAIL doğrula**

```bash
cargo test --quiet tui::welcome 2>&1 | tail -5   # render_welcome tanımsız
```

- [ ] **Step 3: Implementasyon**

```rust
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use unicode_width::UnicodeWidthStr;

const ORANGE: Color = Color::Indexed(208);

const LOGO: [&str; 4] = [
    "██  ██ ██████ ██████ ██████",
    "██  ██ ██       ██   ██  ██",
    "██  ██ ██████   ██   ██████",
    "██████     ██   ██   ██  ██",
];

/// Görünür genişliğe göre kırp, taşarsa `…` ekle. Padding hesapları da
/// unicode-width ile — Türkçe karakterlerde byte sayımı yanlış hizalar.
pub fn fit(s: &str, max: usize) -> String {
    if s.width() <= max { return s.to_string(); }
    let mut out = String::new();
    let mut w = 0usize;
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > max.saturating_sub(1) { break; }
        out.push(ch);
        w += cw;
    }
    out.push('…');
    out
}

fn pad(s: &str, w: usize) -> String {
    format!("{s}{}", " ".repeat(w.saturating_sub(s.width())))
}

/// Çift kolonlu açılış kutusu. Genişlik `min(width, 100)`; sol kolon logo +
/// selamlama + model + dizin, sağ kolon Öğrenme Durumu (spec §5).
pub fn render_welcome(d: &WelcomeData, width: u16) -> Text<'static> {
    let total = (width as usize).clamp(60, 100);
    let inner = total - 2;                      // kenarlar
    let left_w = 34usize;
    let right_w = inner - left_w - 3;           // " │ " ayracı

    let greet = match &d.name {
        Some(n) => format!("Tekrar hoş geldin, {n}!"),
        None => "Tekrar hoş geldin!".to_string(),
    };
    let mut left: Vec<(String, bool)> = vec![(String::new(), false)];
    for l in LOGO { left.push((format!("  {l}"), true)); }
    left.push((String::new(), false));
    left.push((format!("  {}", fit(&greet, left_w - 2)), false));
    left.push((format!("  {}", fit(&d.model, left_w - 2)), false));
    left.push((format!("  {}", fit(&d.dir, left_w - 2)), false));

    let mut right: Vec<String> = Vec::new();
    if d.first_session {
        right.push("Öğrenme Durumu".to_string());
        right.push(String::new());
        right.push(fit("İlk oturum — tanışmayla başlarız.", right_w));
    } else {
        right.push("Öğrenme Durumu".to_string());
        let konu = match &d.level {
            Some(l) => format!("Konu: {} · {}", d.topic, l),
            None => format!("Konu: {}", d.topic),
        };
        right.push(fit(&konu, right_w));
        if let Some(p) = d.map_percent { right.push(format!("Harita: %{p}")); }
        right.push("─".repeat(right_w));
        right.push("Sırada".to_string());
        if let Some(n) = &d.next_item { right.push(fit(n, right_w)); }
        if d.drill_count > 0 { right.push(format!("Drill: {} soru hazır", d.drill_count)); }
    }

    let rows = left.len().max(right.len());
    let title = format!(" Usta v{} ", d.version);
    let top = format!("╭─── {}{}╮", title.trim(), "─".repeat(inner.saturating_sub(5 + title.trim().width())));
    let bottom = format!("╰{}╯", "─".repeat(inner));

    let mut lines: Vec<Line> = vec![Line::from(top)];
    for i in 0..rows {
        let (ltxt, is_logo) = left.get(i).cloned().unwrap_or_default();
        let rtxt = right.get(i).cloned().unwrap_or_default();
        let lspan = Span::styled(
            pad(&ltxt, left_w),
            if is_logo { Style::default().fg(ORANGE) } else { Style::default() },
        );
        let rstyle = if i == 0 && !rtxt.is_empty() {
            Style::default().add_modifier(Modifier::BOLD).fg(ORANGE)
        } else { Style::default() };
        lines.push(Line::from(vec![
            Span::raw("│"),
            lspan,
            Span::raw(" │ "),
            Span::styled(pad(&rtxt, right_w), rstyle),
            Span::raw("│"),
        ]));
    }
    lines.push(Line::from(bottom));
    Text::from(lines)
}
```

NOT: `top` satırının toplam genişliği testte diğer satırlarla eşit çıkmalı — `inner.saturating_sub(...)` hesabını test kırmızıysa düzelt (başlık + `╭───` + kapanış `╮` toplamı `total` olmalı).

- [ ] **Step 4: PASS + commit**

```bash
cargo test --quiet tui::welcome 2>&1 | tail -3
git add src/tui/welcome.rs
git commit -m "tui: açılış kutusu render — çift kolon, unicode-width hizalama"
```

---

### Task 4: Girdi editörü (InputBox + tarihçe)

**Files:**
- Create: `src/tui/editor.rs`

**Interfaces:**
- Consumes: `crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers}`, `tui_input::Input`.
- Produces:
  - `pub struct InputBox` — `pub fn new() -> Self`, `pub fn handle_key(&mut self, key: KeyEvent) -> Action`, `pub fn value(&self) -> &str`, `pub fn visual_cursor(&self) -> usize`, `pub fn render(&self, f: &mut ratatui::Frame, area: ratatui::layout::Rect)`
  - `pub enum Action { None, Submit(String), Exit }`

- [ ] **Step 1: Failing testler**

`src/tui/editor.rs`:

```rust
//! Canlı girdi kutusu: tui-input editör state'i + Vec tabanlı up/down
//! tarihçesi. Rustyline'ın TUI yolundaki karşılığı. Spec §6.

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(c: char) -> KeyEvent { KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE) }
    fn code(k: KeyCode) -> KeyEvent { KeyEvent::new(k, KeyModifiers::NONE) }

    fn type_str(b: &mut InputBox, s: &str) {
        for c in s.chars() { assert!(matches!(b.handle_key(key(c)), Action::None)); }
    }

    #[test]
    fn typing_and_submit_returns_trimmed_line_and_clears() {
        let mut b = InputBox::new();
        type_str(&mut b, "  merhaba usta  ");
        match b.handle_key(code(KeyCode::Enter)) {
            Action::Submit(s) => assert_eq!(s, "merhaba usta"),
            other => panic!("Submit bekleniyordu: {other:?}"),
        }
        assert_eq!(b.value(), "");
    }

    #[test]
    fn empty_submit_is_none() {
        let mut b = InputBox::new();
        assert!(matches!(b.handle_key(code(KeyCode::Enter)), Action::None));
    }

    #[test]
    fn ctrl_c_and_ctrl_d_exit() {
        let mut b = InputBox::new();
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(matches!(b.handle_key(ctrl_c), Action::Exit));
        let ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert!(matches!(b.handle_key(ctrl_d), Action::Exit));
    }

    #[test]
    fn history_up_down_recalls_submitted_lines() {
        let mut b = InputBox::new();
        type_str(&mut b, "ilk");
        b.handle_key(code(KeyCode::Enter));
        type_str(&mut b, "iki");
        b.handle_key(code(KeyCode::Enter));
        b.handle_key(code(KeyCode::Up));
        assert_eq!(b.value(), "iki");
        b.handle_key(code(KeyCode::Up));
        assert_eq!(b.value(), "ilk");
        b.handle_key(code(KeyCode::Down));
        assert_eq!(b.value(), "iki");
        b.handle_key(code(KeyCode::Down));
        assert_eq!(b.value(), "");
    }

    #[test]
    fn turkish_chars_edit_correctly() {
        let mut b = InputBox::new();
        type_str(&mut b, "çğşü");
        b.handle_key(code(KeyCode::Backspace));
        assert_eq!(b.value(), "çğş");
    }
}
```

- [ ] **Step 2: FAIL doğrula**

```bash
cargo test --quiet tui::editor 2>&1 | tail -5
```

- [ ] **Step 3: Implementasyon**

Test bloğunun üstüne:

```rust
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

/// Tuş işlemenin sonucu — döngü buna göre davranır.
#[derive(Debug)]
pub enum Action {
    None,
    /// Trim'lenmiş, boş olmayan satır gönderildi.
    Submit(String),
    /// Ctrl-C / Ctrl-D — kapanış akışı.
    Exit,
}

pub struct InputBox {
    input: Input,
    history: Vec<String>,
    /// None = taze satır; Some(i) = history[i] gösteriliyor.
    cursor: Option<usize>,
    /// Tarihçeye girmeden önceki taze metin — Down ile geri gelir.
    stash: String,
}

impl InputBox {
    pub fn new() -> Self {
        Self { input: Input::default(), history: Vec::new(), cursor: None, stash: String::new() }
    }

    pub fn value(&self) -> &str { self.input.value() }
    pub fn visual_cursor(&self) -> usize { self.input.visual_cursor() }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('d'))
        {
            return Action::Exit;
        }
        match key.code {
            KeyCode::Enter => {
                let line = self.input.value().trim().to_string();
                if line.is_empty() { return Action::None; }
                self.history.push(line.clone());
                self.input.reset();
                self.cursor = None;
                Action::Submit(line)
            }
            KeyCode::Up => { self.recall_prev(); Action::None }
            KeyCode::Down => { self.recall_next(); Action::None }
            _ => {
                self.cursor = None;
                self.input.handle_event(&Event::Key(key));
                Action::None
            }
        }
    }

    fn recall_prev(&mut self) {
        if self.history.is_empty() { return; }
        let next = match self.cursor {
            None => { self.stash = self.input.value().to_string(); self.history.len() - 1 }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.cursor = Some(next);
        self.input = Input::new(self.history[next].clone());
    }

    fn recall_next(&mut self) {
        match self.cursor {
            None => {}
            Some(i) if i + 1 < self.history.len() => {
                self.cursor = Some(i + 1);
                self.input = Input::new(self.history[i + 1].clone());
            }
            Some(_) => {
                self.cursor = None;
                self.input = Input::new(std::mem::take(&mut self.stash));
            }
        }
    }

    /// Kutuyu çiz: yuvarlak kenar + `> ` öneki + imleç. Uzun satırda
    /// tui-input'un visual_scroll'u son kısmı gösterir (iç kaydırma).
    pub fn render(&self, f: &mut Frame, area: Rect) {
        let inner_w = area.width.saturating_sub(4) as usize; // kenarlar + "> "
        let scroll = self.input.visual_scroll(inner_w);
        let shown: String = self.input.value().chars().skip(scroll).collect();
        let para = Paragraph::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Indexed(208))),
            Span::raw(shown),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        f.render_widget(para, area);
        let x = area.x + 3 + (self.input.visual_cursor().saturating_sub(scroll)) as u16;
        f.set_cursor_position((x.min(area.x + area.width - 2), area.y + 1));
    }
}
```

NOT: `tui_input` API'si (`visual_scroll`, `handle_event`, `Input::new`) sürümle kaymışsa docs.rs'ten eşdeğerini kullan; `set_cursor_position` eski ratatui'de `set_cursor` olabilir.

- [ ] **Step 4: PASS + commit**

```bash
cargo test --quiet tui::editor 2>&1 | tail -3
git add src/tui/editor.rs
git commit -m "tui: girdi kutusu — tui-input editör + up/down tarihçe + Exit/Submit aksiyonları"
```

---

### Task 5: ANSI dönüşümü + durum satırı + ui::render_markdown

**Files:**
- Create: `src/tui/convert.rs`
- Create: `src/tui/status.rs`
- Modify: `src/ui.rs` (skin'i String'e render eden `render_markdown` eklenir)

**Interfaces:**
- Consumes: `ui::skin()` (mevcut, private — public yapılmaz; `render_markdown` ui içinde kalır).
- Produces:
  - `ui::render_markdown(md: &str, width: usize) -> String` — ANSI'li render.
  - `tui::convert::ansi_to_text(s: &str) -> ratatui::text::Text<'static>`
  - `tui::status::{Status, render_status}` — `pub enum Status { Idle, Thinking { frame: usize } }`, `pub fn render_status(s: &Status, tokens: Option<u64>, window: u64) -> ratatui::text::Line<'static>`

- [ ] **Step 1: Failing testler**

`src/tui/convert.rs`:

```rust
//! termimad ANSI çıktısını ratatui Text'ine çevir — insert_before köprüsü.

use ratatui::text::Text;

/// ANSI'li string → ratatui Text. Dönüşüm hatasında stil at, düz metin bas —
/// içerik asla kaybolmaz.
pub fn ansi_to_text(s: &str) -> Text<'static> {
    use ansi_to_tui::IntoText;
    s.into_text().unwrap_or_else(|_| Text::raw(s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_passes_through() {
        let t = ansi_to_text("merhaba\ndünya");
        assert_eq!(t.lines.len(), 2);
    }

    #[test]
    fn ansi_colors_produce_styled_spans() {
        let t = ansi_to_text("\x1b[38;5;208mturuncu\x1b[0m");
        let joined: String = t.lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, "turuncu");
    }
}
```

`src/tui/status.rs`:

```rust
//! Viewport'un alt satırı: spinner + bağlam göstergesi (ui::context_gauge'un
//! TUI karşılığı — ayrı satır basmak yerine yerinde yaşar).

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

const FRAMES: [&str; 4] = ["⠋", "⠙", "⠸", "⠴"];

pub enum Status {
    Idle,
    Thinking { frame: usize },
}

/// Tek satır durum: düşünüyorsa spinner, her durumda token varsa gauge.
pub fn render_status(s: &Status, tokens: Option<u64>, window: u64) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::new();
    if let Status::Thinking { frame } = s {
        spans.push(Span::styled(
            format!("{} Usta düşünüyor… ", FRAMES[frame % FRAMES.len()]),
            Style::default().fg(Color::DarkGray),
        ));
    }
    if let Some(t) = tokens {
        let ratio = (t as f64 / window as f64).min(1.0);
        let filled = ((ratio * 8.0).round() as usize).min(8);
        let color = if ratio >= 0.7 { Color::Yellow } else { Color::DarkGray };
        spans.push(Span::styled(
            format!("{}{} bağlam {}k/{}k", "▓".repeat(filled), "░".repeat(8 - filled), t / 1000, window / 1000),
            Style::default().fg(color),
        ));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(l: &Line) -> String { l.spans.iter().map(|s| s.content.as_ref()).collect() }

    #[test]
    fn idle_without_tokens_is_empty() {
        assert_eq!(text(&render_status(&Status::Idle, None, 1_000_000)), "");
    }

    #[test]
    fn thinking_shows_spinner_frame() {
        let l = render_status(&Status::Thinking { frame: 0 }, None, 1_000_000);
        assert!(text(&l).contains("düşünüyor"));
    }

    #[test]
    fn gauge_shows_ratio() {
        let l = render_status(&Status::Idle, Some(500_000), 1_000_000);
        assert!(text(&l).contains("bağlam 500k/1000k"));
        assert!(text(&l).contains("▓▓▓▓░░░░"));
    }
}
```

`src/ui.rs`'e ekle (`skin()` fonksiyonunun üstüne):

```rust
/// Usta yanıtını ANSI'li String'e render et — TUI yolu bunu ratatui Text'ine
/// çevirir (tui::convert). Satır başına 2 boşluk girinti print_usta_reply ile
/// aynı görsel dil.
pub fn render_markdown(md: &str, width: usize) -> String {
    let skin = skin();
    let text = skin.text(md, Some(width.saturating_sub(4)));
    format!("{text}")
        .lines()
        .map(|l| format!("  {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}
```

ui.rs test modülü yoksa dosya sonuna ekle:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_markdown_indents_and_keeps_content() {
        let out = render_markdown("**kalın** metin", 60);
        assert!(out.contains("kalın"));
        assert!(out.lines().all(|l| l.is_empty() || l.starts_with("  ")));
    }
}
```

- [ ] **Step 2: FAIL → implement → PASS**

Kod yukarıda komple — testlerle birlikte yaz, çalıştır:

```bash
cargo test --quiet tui:: 2>&1 | tail -3
cargo test --quiet ui:: 2>&1 | tail -3
```

- [ ] **Step 3: Commit**

```bash
git add src/tui/convert.rs src/tui/status.rs src/ui.rs
git commit -m "tui: ANSI→ratatui köprüsü + durum satırı + ui::render_markdown"
```

---

### Task 6: Terminal guard (kurulum/restore/panic hook)

**Files:**
- Create: `src/tui/term.rs`

**Interfaces:**
- Produces:
  - `pub const VIEWPORT_H: u16 = 6;`
  - `pub struct Tui { pub terminal: ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>> }`
  - `pub fn setup() -> anyhow::Result<Tui>` — raw mode + inline viewport + panic hook zinciri.
  - `impl Drop for Tui` — restore (raw mode kapat); `pub fn restore()` bağımsız da çağrılabilir.

- [ ] **Step 1: Implementasyon** (IO-ağır — unit test yok, davranış Task 8 elle doğrulamada)

```rust
//! Terminal yaşam döngüsü: inline viewport kur, NE OLURSA OLSUN restore et.
//! Bozuk raw-mode'da bırakılan shell = en kötü kullanıcı deneyimi; Drop +
//! panic hook çifte emniyet.

use std::io::Stdout;

use anyhow::Result;
use ratatui::backend::CrosstermBackend;
use ratatui::{Terminal, TerminalOptions, Viewport};

/// Alt bölge: girdi kutusu (3-5 satır) + durum satırı (1).
pub const VIEWPORT_H: u16 = 6;

pub struct Tui {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
}

/// Raw mode + inline viewport. Panic hook'a restore zincirlenir — önceki
/// hook korunur (test harness'inin hook'u ezilmez).
pub fn setup() -> Result<Tui> {
    crossterm::terminal::enable_raw_mode()?;
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        prev(info);
    }));
    let terminal = Terminal::with_options(
        CrosstermBackend::new(std::io::stdout()),
        TerminalOptions { viewport: Viewport::Inline(VIEWPORT_H) },
    )?;
    Ok(Tui { terminal })
}

/// Raw mode'u kapat — idempotent, hata yutar (kapanış yolunda panik yok).
pub fn restore() {
    let _ = crossterm::terminal::disable_raw_mode();
}

impl Drop for Tui {
    fn drop(&mut self) {
        // Viewport bölgesini temizle ki kapanış mesajları temiz zemine bassın.
        let _ = self.terminal.clear();
        restore();
        println!();
    }
}
```

- [ ] **Step 2: Derleme + commit**

```bash
cargo build 2>&1 | tail -3
cargo test --quiet 2>&1 | tail -3
git add src/tui/term.rs
git commit -m "tui: terminal guard — inline viewport kurulum + Drop/panic restore"
```

---

### Task 7: TUI olay döngüsü + main.rs entegrasyonu

**Files:**
- Create: `src/tui/run.rs`
- Modify: `src/main.rs` (satır ~111-217: banner + drill + loop bölgesi dallanır; mevcut döngü `run_plain_loop`'a çıkarılır)

**Interfaces:**
- Consumes: Task 2-6'nın tümü + mevcut `backend::Backend::complete`, `session::Session`, `transcript::Recorder`, `watcher::{spawn, Debouncer}`, `feedback::FileMemory`, `progress::{opening_prompt, onboarding_prompt, progress_path}`, `main.rs`'teki `handle_file_change`, `maybe_compact`, `flush_progress` (imzalar değişmez).
- Produces: `pub async fn run(...) -> anyhow::Result<()>` — TUI oturum döngüsü; dönünce terminal restore edilmiş olur, `main` kapanış flush'ını mevcut yolla koşar.

- [ ] **Step 1: `src/tui/run.rs` iskeleti**

```rust
//! TUI oturum döngüsü: tuş + watcher + LLM tek select!'te. Kalıcı içerik
//! insert_before ile scrollback'e akar; alt bölge canlı çizilir. Spec §3.

use std::path::Path;

use anyhow::Result;
use crossterm::event::{Event, EventStream};
use futures_util::StreamExt;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::Text;
use ratatui::widgets::{Paragraph, Widget};

use crate::backend::Backend;
use crate::session::Session;
use crate::transcript::Recorder;
use crate::tui::convert::ansi_to_text;
use crate::tui::editor::{Action, InputBox};
use crate::tui::status::{render_status, Status};
use crate::tui::term::{Tui, VIEWPORT_H};
use crate::tui::welcome;
use crate::{feedback, progress, ui, watcher};

/// Kalıcı içeriği viewport üstüne (scrollback'e) bas.
fn page(tui: &mut Tui, text: Text<'static>) -> Result<()> {
    let h = text.height() as u16;
    tui.terminal.insert_before(h, |buf| {
        Paragraph::new(text).render(buf.area, buf);
    })?;
    Ok(())
}

/// Usta yanıtını görsel dille bas: turuncu ● satırı + markdown + boş satır.
fn page_reply(tui: &mut Tui, reply: &str, width: u16) -> Result<()> {
    let ansi = ui::render_markdown(reply, width as usize);
    let mut t = ansi_to_text(&format!("\x1b[38;5;208m●\x1b[0m\n{ansi}\n"));
    t.lines.push(ratatui::text::Line::raw(""));
    page(tui, t)
}

/// Soluk sistem bildirimi (ui::notice'un TUI karşılığı).
fn page_notice(tui: &mut Tui, msg: &str) -> Result<()> {
    page(tui, ansi_to_text(&format!("\x1b[2m· {msg}\x1b[0m")))
}

/// Alt bölgeyi çiz: girdi kutusu (üstte) + durum satırı (altta).
fn draw(tui: &mut Tui, editor: &InputBox, status: &Status, tokens: Option<u64>, window: u64) -> Result<()> {
    tui.terminal.draw(|f| {
        let [box_area, status_area] =
            Layout::vertical([Constraint::Length(VIEWPORT_H - 1), Constraint::Length(1)])
                .areas(f.area());
        editor.render(f, box_area);
        f.render_widget(render_status(status, tokens, window), status_area);
    })?;
    Ok(())
}

/// LLM çağrısını canlı arayüzle bekle: spinner döner, tuşlar editöre işler
/// ama Submit/Exit KİLİTLİ (tek turn ilkesi) — Enter yutulur.
async fn ask_live(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
    backend: &mut Backend,
    system: &str,
    history: &[crate::anthropic::Message],
    tokens: Option<u64>,
) -> Result<crate::backend::Reply> {
    let window = backend.context_window();
    let fut = backend.complete(system, history);
    tokio::pin!(fut);
    let mut frame = 0usize;
    loop {
        draw_locked(tui, editor, frame, tokens, window)?;
        tokio::select! {
            r = &mut fut => return r,
            Some(Ok(ev)) = events.next() => {
                if let Event::Key(k) = ev {
                    // Enter/Ctrl-C burada işlem başlatamaz — sadece edit tuşları.
                    let _ = editor_key_locked(editor, k);
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(120)) => { frame += 1; }
        }
    }
}

fn draw_locked(tui: &mut Tui, editor: &InputBox, frame: usize, tokens: Option<u64>, window: u64) -> Result<()> {
    draw(tui, editor, &Status::Thinking { frame }, tokens, window)
}

/// Kilitli modda tuş: Enter ve Ctrl-C/D yutulur, gerisi editöre gider.
fn editor_key_locked(editor: &mut InputBox, k: crossterm::event::KeyEvent) -> Action {
    use crossterm::event::KeyCode;
    if matches!(k.code, KeyCode::Enter) { return Action::None; }
    match editor.handle_key(k) {
        Action::Exit => Action::None, // kapanış sadece idle'da
        other => other,
    }
}
```

- [ ] **Step 2: `run()` ana döngüsü** (aynı dosyaya devam)

`main.rs`'teki mevcut döngünün (satır 148-204) TUI karşılığı — davranış birebir, çıktılar `page*` üzerinden:

```rust
/// TUI oturumu: açılış kutusu + drill/tanışma + ana döngü. Dönüşte Tui drop
/// olur → terminal restore; kapanış flush'ı main'de plain yolla koşar.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    backend: &mut Backend,
    session: &mut Session,
    recorder: &Recorder,
    project_root: &Path,
    global: &Path,
    topic: &str,
    has_progress: bool,
    max_feedback_batch: usize,
) -> Result<()> {
    let mut tui = crate::tui::term::setup()?;
    let mut editor = InputBox::new();
    let mut events = EventStream::new();
    let mut watch_rx = watcher::spawn(project_root)?;
    let mut debouncer = watcher::Debouncer::new(std::time::Duration::from_millis(1000));
    let mut files = feedback::FileMemory::new();
    let mut last_tokens: Option<u64> = None;
    let window = backend.context_window();

    // Açılış kutusu — bir kere, scrollback'e.
    let width = tui.terminal.size()?.width;
    let read = |p: std::path::PathBuf| std::fs::read_to_string(p).ok();
    let data = welcome::gather(
        read(global.join("learner/profile.md")).as_deref(),
        read(progress::progress_path(project_root, topic)).as_deref(),
        read(progress::curriculum_path(project_root, topic)).as_deref(),
        topic,
        &backend.label(),
        &short_dir(project_root),
    );
    page(&mut tui, welcome::render_welcome(&data, width))?;

    // Açılış drilli / tanışma (main.rs 113-144'ün karşılığı).
    let opening = if has_progress { progress::opening_prompt(topic) } else { progress::onboarding_prompt(topic) };
    session.push_user(&opening);
    recorder.user(&opening);
    match ask_live(&mut tui, &mut editor, &mut events, backend, &session.system, session.history(), last_tokens).await {
        Ok(reply) => {
            last_tokens = reply.context_tokens;
            page_reply(&mut tui, &reply.text, width)?;
            recorder.assistant(&reply.text);
            session.push_assistant(reply.text);
        }
        Err(e) => page_notice(&mut tui, &format!("açılış turu atlandı: {e}"))?,
    }

    loop {
        draw(&mut tui, &editor, &Status::Idle, last_tokens, window)?;
        tokio::select! {
            Some(Ok(ev)) = events.next() => {
                let Event::Key(k) = ev else { continue };
                match editor.handle_key(k) {
                    Action::None => {}
                    Action::Exit => break,
                    Action::Submit(line) => {
                        if line == "/quit" { break; }
                        // Gönderilen satırı soluk iz olarak scrollback'e bas.
                        page(&mut tui, ansi_to_text(&format!("\x1b[2m│ > {line}\x1b[0m")))?;
                        session.push_user(&line);
                        recorder.user(&line);
                        match ask_live(&mut tui, &mut editor, &mut events, backend, &session.system, session.history(), last_tokens).await {
                            Ok(reply) => {
                                last_tokens = reply.context_tokens;
                                page_reply(&mut tui, &reply.text, width)?;
                                recorder.assistant(&reply.text);
                                session.push_assistant(reply.text);
                                crate::maybe_compact(backend, session, project_root, last_tokens).await;
                            }
                            Err(e) => page_notice(&mut tui, &format!("hata: {e}"))?,
                        }
                    }
                }
            }
            Some(path) = watch_rx.recv() => {
                debouncer.push(path, tokio::time::Instant::now());
            }
            _ = crate::sleep_until_deadline(debouncer.deadline()), if debouncer.deadline().is_some() => {
                let batch = debouncer.flush();
                if batch.len() > max_feedback_batch {
                    page_notice(&mut tui, &format!("toplu değişiklik ({} dosya) — feedback atlandı, izleme sürüyor", batch.len()))?;
                    for path in batch {
                        if let Ok(c) = std::fs::read_to_string(&path) { let _ = files.observe(&path, c); }
                    }
                } else {
                    for path in batch {
                        match crate::handle_file_change(backend, session, &mut files, project_root, &path, recorder).await {
                            Ok(tokens) => {
                                if let Some(t) = tokens { last_tokens = Some(t); }
                                crate::maybe_compact(backend, session, project_root, tokens).await;
                            }
                            Err(e) => page_notice(&mut tui, &format!("dosya feedback atlandı: {}: {e}", path.display()))?,
                        }
                    }
                }
            }
        }
    }
    Ok(()) // Tui drop → restore
}

/// `$HOME` → `~` kısaltmalı proje dizini.
fn short_dir(p: &Path) -> String {
    let s = p.display().to_string();
    match dirs::home_dir() {
        Some(h) => s.replace(&h.display().to_string(), "~"),
        None => s,
    }
}
```

DİKKAT — uyarlama noktaları (implementasyon sırasında main.rs'i OKU):
- `crate::maybe_compact`, `crate::handle_file_change`, `crate::sleep_until_deadline` şu an `main.rs`'te private — `pub(crate)` yap.
- `handle_file_change` içindeki `print_reply`/`ui::*` çağrıları stdout'a basar; TUI açıkken raw-mode'da satır sonları bozulur. Çözüm: `handle_file_change`'e çıktıyı üretme, `Result<(Option<u64>, Option<String>)>` (tokens + yanıt metni) döndürt; hem plain hem TUI yolu kendi basar. İmza değişikliğini plain yolda da uygula.
- `watcher::spawn` main'de zaten çağrılıyor (satır 76) — TUI yolunda İKİNCİ kez çağırma; `run()`'a `watch_rx`'i parametre geçir (yukarıdaki iskeleti buna göre düzelt: `watch_rx: &mut tokio::sync::mpsc::UnboundedReceiver<std::path::PathBuf>`).
- `futures_util` StreamExt için `futures-util` bağımlılığı gerekebilir (`cargo add futures-util`) — crossterm EventStream zaten `futures_core::Stream` döner.

- [ ] **Step 3: main.rs dallanması**

`main.rs`'te satır 75-146 bölgesinde (watcher/input kurulumundan drill'e kadar) ve döngüde:

```rust
    // TUI yolu: etkileşimli terminal → ratatui; değilse mevcut satır yolu.
    if !ui::is_plain() {
        tui::run(
            &mut backend, &mut session, &recorder, &project_root, &global,
            &topic, has_progress, MAX_FEEDBACK_BATCH, /* + watch_rx */
        ).await?;
    } else {
        ui::banner(&topic, &backend.label());
        run_plain_loop(/* mevcut değişkenler */).await?;
    }
```

Mevcut 148-204 döngüsü + 113-146 drill bloğu `async fn run_plain_loop(...)`'a taşınır — kod DEĞİŞMEZ, sadece taşınır (rustyline `input::spawn` + `ready_tx` el-sıkışması plain yolun içinde kalır; `input::spawn` çağrısı da plain dalına iner). Kapanış (flush_progress, lock temizliği, `mark_done`) her iki yol DÖNDÜKten sonra ortak kalır (satır 206-217 yerinde).

- [ ] **Step 4: Derleme + tüm testler**

```bash
cargo build 2>&1 | tail -5
cargo test --quiet 2>&1 | tail -3
```

Beklenen: build OK, tüm testler yeşil (plain yol regresyonu dahil).

- [ ] **Step 5: Commit**

```bash
git add src/tui/run.rs src/main.rs src/tui/mod.rs Cargo.toml Cargo.lock
git commit -m "tui: olay döngüsü + main dallanması — inline viewport oturumu canlı"
```

---

### Task 8: Regresyon + elle doğrulama + kurulum

**Files:**
- Modify: `README.md` (TUI notu — 2-3 satır: TUI default, `NO_COLOR=1` plain'e düşürür)

- [ ] **Step 1: Tam süit + plain regresyonu**

```bash
cargo test --quiet 2>&1 | tail -3
NO_COLOR=1 echo "" | cargo run --quiet -- start rust 2>&1 | head -5   # plain yol banner'ı düz basmalı, TUI açılmamalı
```

- [ ] **Step 2: Elle doğrulama (gerçek terminalde, spec §12 başarı ölçütleri)**

Sırayla dene, her biri OK olmalı:
1. `cargo run -- start rust` → açılış kutusu çift kolon, gerçek verilerle.
2. Uzun satır yaz → kutu içinde iç kaydırma, kenarlar bozulmuyor.
3. Yanıt gelirken tuşlara bas → editöre işliyor, Enter yutuluyor, spinner dönüyor.
4. Yanıt sonrası yukarı scroll → geçmiş okunuyor/kopyalanıyor.
5. Terminal resize → kutu yeni genişliğe çiziliyor (açılış kutusu sabit — kabul).
6. Ctrl-C → temiz çıkış: shell bozuk değil, kapanış flush mesajları görünür, lock silinmiş.
7. İzlenen dosyayı kaydet → feedback scrollback'e basılıyor, girdi kutusu yerinde.

- [ ] **Step 3: README + kurulum + commit**

README'ye "Arayüz" bölümü (3 satır: ratatui TUI default; scrollback korunur; `NO_COLOR=1`/pipe → düz mod). Sonra:

```bash
cargo install --path .   # kullanıcı binary'si güncellensin
git add README.md
git commit -m "tui: v1 tamam — elle doğrulama geçti, README arayüz notu"
```

---

## Self-Review Notları (yazım sonrası kontrol edildi)

- **Spec kapsaması:** §3 mimari→Task 6-7, §5 açılış→Task 2-3, §6 girdi→Task 4, §4 akış→Task 5+7, §7 kapanış/panik→Task 6, §8 plain→Task 7 Step 3 + Task 8, §10 test→her taskın TDD adımları, §11 riskler→Task 1 (crossterm), Task 6 (restore), Task 7 DİKKAT bloğu (watcher/stdout). Boşluk yok.
- **Bilinen sürüm riski:** ratatui/tui-input/ansi-to-tui API imzaları sürümle kayabilir — Global Constraints'teki docs.rs kuralı her taskta geçerli.
- **Task 7 en büyük görev** — bilinçli: olay döngüsü bölünürse yarım durumda derlenmeyen ara commit çıkar. DİKKAT bloğundaki 4 uyarlama noktası implementasyoncunun main.rs'i okumasını zorunlu kılar; imzalar oradaki gerçeğe göre bağlanır.
