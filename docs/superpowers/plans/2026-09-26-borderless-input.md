# Borderless Input — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Input kutusunun yan çizgilerini (`│`) ve yuvarlak köşelerini kaldır; üst ve alt `─` çizgisi kalsın, yükseklik sabit kalsın.

**Architecture:** Tek fonksiyon değişir: `InputBox::render` (`src/tui/editor.rs`). Block `Borders::ALL` → `Borders::TOP | Borders::BOTTOM`; sarma genişliği ve imleç x-ofseti yan kenarların payı düşülerek düzeltilir. Render yolu, viewport, resize, CPR'a dokunulmaz.

**Tech Stack:** Rust, ratatui 0.30.2 (`TestBackend` testte), tui-input.

**Spec:** `docs/superpowers/specs/2026-09-26-borderless-input-design.md` — önce oku.

**Ön-koşul notu:** Bekleyen `docs/superpowers/plans/2026-09-04-rustls-portability.md` de bir patch bump içeriyor. Kod olarak çakışmazlar; sürüm numarası Task 2'de `Cargo.toml`'daki **o anki** sürümden bir patch yukarı hesaplanır, sabit bir sayı yazılmaz.

## Global Constraints

- Dokunulmayanlar: `VIEWPORT_H` (6), `src/tui/page.rs`, `src/tui/term.rs`, `src/tui/backend_wrap.rs`, resize yolu, `wrap_visual`, submit değeri.
- Kenar rengi `theme::DIM` kalır.
- `cargo fmt` sadece düzenlenen dosyada: `cargo fmt -- src/tui/editor.rs` (crate genelinde değil).
- `src/tui/editor.rs` şu an 384 satır; 600 satır bütçesinin altında, bölme gerekmez.
- Push yok, merge yok.

---

### Task 1: `InputBox::render` — yan çizgisiz çerçeve

**Files:**
- Modify: `src/tui/editor.rs:8` (import), `src/tui/editor.rs:149-184` (`render` + doc comment)
- Test: `src/tui/editor.rs` (`#[cfg(test)] mod tests`, dosyanın sonu)

**Interfaces:**
- Consumes: yok
- Produces: `pub fn render(&self, f: &mut Frame, area: Rect)` — imza DEĞİŞMEZ; çağıran `page::draw` dokunulmaz.

- [ ] **Step 1: Failing testleri yaz**

`src/tui/editor.rs` test modülünün sonuna (son `}`'den önce) ekle:

```rust
    /// Render the box into a `TestBackend` and return (rows as strings, cursor x/y).
    fn render_rows(b: &InputBox, w: u16, h: u16) -> (Vec<String>, (u16, u16)) {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| {
            let a = f.area();
            b.render(f, a);
        })
        .unwrap();
        let buf = t.backend().buffer().clone();
        let rows = (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect())
            .collect();
        let pos = t.get_cursor_position().unwrap();
        (rows, (pos.x, pos.y))
    }

    #[test]
    fn render_draws_rules_above_and_below_without_side_borders() {
        let b = InputBox::new();
        let (rows, _) = render_rows(&b, 20, 5);
        assert_eq!(rows[0], "─".repeat(20), "top rule");
        assert_eq!(rows[4], "─".repeat(20), "bottom rule");
        assert!(rows[1].starts_with("> "), "prompt row: {:?}", rows[1]);
        for (i, r) in rows.iter().enumerate() {
            for c in ['│', '╭', '╮', '╰', '╯'] {
                assert!(!r.contains(c), "row {i} has {c:?}: {r:?}");
            }
        }
    }

    #[test]
    fn render_wraps_at_width_minus_prefix() {
        let mut b = InputBox::new();
        type_str(&mut b, &"a".repeat(18)); // 20 - "> " = 18 fits one row
        let (rows, _) = render_rows(&b, 20, 5);
        assert_eq!(rows[1], format!("> {}", "a".repeat(18)));
        assert_eq!(rows[2].trim(), "", "nothing wrapped: {:?}", rows[2]);

        let mut b = InputBox::new();
        type_str(&mut b, &"a".repeat(19)); // one over → second row
        let (rows, _) = render_rows(&b, 20, 5);
        assert_eq!(rows[1], format!("> {}", "a".repeat(18)));
        assert_eq!(rows[2], format!("  a{}", " ".repeat(17)));
    }

    #[test]
    fn render_places_cursor_after_prompt_and_clamps_to_last_column() {
        let b = InputBox::new();
        let (_, cur) = render_rows(&b, 20, 5);
        assert_eq!(cur, (2, 1), "empty input: cursor right after \"> \"");

        let mut b = InputBox::new();
        type_str(&mut b, &"a".repeat(18)); // cursor col 18 → x 20, clamped to 19
        let (_, cur) = render_rows(&b, 20, 5);
        assert_eq!(cur, (19, 1));
    }
```

(`type_str` zaten test modülünde tanımlı, `super::*` ile `InputBox` erişilebilir.)

- [ ] **Step 2: Testlerin düştüğünü gör**

Run: `cargo test tui::editor::tests::render_`
Expected: 3 test FAIL — ilki `top rule` (satır `╭──…╮`), ikincisi sarma satırı (mevcut genişlik 16), üçüncüsü `(3, 1)` ≠ `(2, 1)`.

- [ ] **Step 3: Implementasyon**

`src/tui/editor.rs:8` importu:

```rust
use ratatui::widgets::{Block, Borders, Paragraph};
```

`render` fonksiyonunu ve doc comment'ini (mevcut `/// Draw the box: rounded border …` satırından fonksiyonun kapanan `}`'ine kadar) şununla değiştir:

```rust
    /// Draw the box: a rule above and below (no side borders) + `> ` prefix +
    /// cursor. Long text WRAPS TO THE NEXT LINE at the full width minus the
    /// prefix (no horizontal scrolling); if it exceeds the inner line count,
    /// the vertical window follows the cursor.
    pub fn render(&self, f: &mut Frame, area: Rect) {
        let inner_w = area.width.saturating_sub(2) as usize; // "> " prefix
        let visible = area.height.saturating_sub(2).max(1) as usize; // inner lines
        let (rows, cur_row, cur_col) =
            wrap_visual(self.input.value(), inner_w, self.input.visual_cursor());
        // Vertical window: last `visible` lines so the cursor stays visible.
        let start = (cur_row + 1).saturating_sub(visible);
        let lines: Vec<Line> = rows
            .iter()
            .enumerate()
            .skip(start)
            .take(visible)
            .map(|(i, r)| {
                let prefix = if i == 0 { "> " } else { "  " };
                Line::from(vec![
                    Span::styled(prefix, theme::brand()),
                    Span::raw(r.clone()),
                ])
            })
            .collect();
        let para = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::TOP | Borders::BOTTOM)
                .border_style(Style::default().fg(theme::DIM)),
        );
        f.render_widget(para, area);
        let x = area.x + 2 + cur_col as u16;
        let y = area.y + 1 + (cur_row - start) as u16;
        f.set_cursor_position((
            x.min(area.x + area.width - 1),
            y.min(area.y + area.height - 2),
        ));
    }
```

- [ ] **Step 4: Testlerin geçtiğini gör**

Run: `cargo test tui::editor::tests`
Expected: editor testlerinin hepsi PASS (yeni 3 + mevcutlar).

- [ ] **Step 5: Tam suite + fmt + clippy**

Run: `cargo fmt -- src/tui/editor.rs && cargo test && cargo clippy --all-targets`
Expected: tüm testler PASS; clippy yeni uyarı yok (`BorderType` artık import edilmediği için unused-import uyarısı da yok).

- [ ] **Step 6: Commit**

```bash
git add src/tui/editor.rs
git commit -m "feat: input box drops its side borders — rules above and below only"
```

---

### Task 2: Doküman + sürüm + elle doğrulama

**Files:**
- Modify: `SPEC.md` (§4.19, v0.31.0 paragrafından sonra, `## 4.20 Prompt Diet` başlığından önce)
- Modify: `docs/ROADMAP.md` (`## Completed` altındaki ilk madde olarak)
- Modify: `Cargo.toml` (`version`), `Cargo.lock` (cargo günceller)

**Interfaces:**
- Consumes: Task 1'in commit'i
- Produces: yok

- [ ] **Step 1: Sürümü bir patch artır**

Run: `grep -n '^version' Cargo.toml`
`Cargo.toml`'daki sürümü bir patch yukarı al (ör. `0.31.1` → `0.31.2`; rustls planı önce koştuysa `0.31.2` → `0.31.3`). Sonra:

Run: `cargo check`
Expected: başarılı; `Cargo.lock`'taki `usta` sürümü güncellenir.

- [ ] **Step 2: SPEC notu**

`SPEC.md`'de v0.31.0 paragrafının (satır `**v0.31.0 (reflow policy as an input):**` ile başlayan) bittiği yerden sonra, `## 4.20 Prompt Diet (v0.19)` başlığından önce, boş satırla ayrılmış yeni paragraf ekle (`<VER>` = Step 1'deki yeni sürüm):

```markdown
**v<VER> (borderless input, look only):** the input box drops its side borders and rounded corners — a plain `─` rule above and below in `theme::DIM`, nothing left or right. Only `InputBox::render` changed: `Borders::TOP | Borders::BOTTOM`, content wraps at `width - 2` (only the `> ` / `  ` prefix is deducted; it was `width - 4`), the cursor sits at `area.x + 2` and clamps to the last column. The height stays fixed (`VIEWPORT_H = 6`: three content rows, two rules, the status line) — the inline viewport, the resize path and CPR are untouched. This is the look v0.30.0 shipped bundled with the relative renderer; the look was never the problem, so it returns alone. A height that grows with the text was considered and deferred: it needs either a rebuilt inline viewport (the v0.29.1 path) or a blank-row reserve below the status line. The resize ghosting of v0.29.3 is unchanged — ghosts now show as rules instead of boxes. Design: `docs/superpowers/specs/2026-09-26-borderless-input-design.md`.
```

- [ ] **Step 3: ROADMAP notu**

`docs/ROADMAP.md`'de `## Completed` başlığının altındaki boş satırdan sonra, mevcut ilk maddenin ÜSTÜNE ekle:

```markdown
- 2026-09-26: Borderless input (v<VER>) — the input box keeps a rule above and below and drops its side borders and rounded corners; fixed height, content wraps at the full width minus the `> ` prefix. Look only: viewport, resize and CPR untouched.
```

- [ ] **Step 4: Commit**

```bash
git add SPEC.md docs/ROADMAP.md Cargo.toml Cargo.lock
git commit -m "docs: SPEC + ROADMAP — borderless input; bump to v<VER>"
```

- [ ] **Step 5: Kur + elle doğrulama (Anil ile, BAĞLAYICI)**

Run: `cargo install --path .`

Anil'e şu kontrolleri yaptır, sonucu bekle — test yeşili bunun yerine GEÇMEZ:
1. VS Code entegre terminalinde `usta` aç → kutuda yan çizgi ve köşe yok; üstte ve altta `─` çizgisi var.
2. Pencere genişliğini aşan uzun bir satır yaz → tam genişlikte sarıyor, ikinci satır `  ` ile hizalı başlıyor.
3. İmleç `> `'den hemen sonra; satır sonuna gelince son sütunda duruyor.
4. Bir mesaj gönder, cevap gelsin → çerçeve bozulmadan yeniden çiziliyor.

Anil sorun bildirirse: düzeltme yazma, bulguyu raporla ve dur.
