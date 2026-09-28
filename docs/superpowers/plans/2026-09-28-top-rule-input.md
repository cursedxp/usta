# Top-Rule Input — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Input kutusunun alt `─` çizgisini kaldır; üst çizgi kalsın, açılan satır dördüncü metin satırı olsun.

**Architecture:** Tek fonksiyon: `InputBox::render` (`src/tui/editor.rs`). `Borders::TOP`, `visible = height - 1`, imleç y sınırı `height - 1`. Viewport, `page.rs`, resize, CPR dokunulmaz.

**Tech Stack:** Rust, ratatui 0.30.2 (`TestBackend`).

**Spec:** `docs/superpowers/specs/2026-09-28-top-rule-input-design.md` — önce oku.

**Ön-koşul notu:** `web-fetch` dalında koşar (Task 1–2 orada tamam). Sürüm/doküman işi bu planda YOK — `2026-09-26-web-fetch.md` Task 3'ündeki v0.31.3 bump'ına katılır.

## Global Constraints

- Dokunulmayanlar: `VIEWPORT_H` (6), `src/tui/page.rs`, `src/tui/term.rs`, `src/tui/backend_wrap.rs`, resize yolu, `wrap_visual`, submit değeri.
- Sarma genişliği (`width - 2`) ve imleç x (`area.x + 2 + cur_col`, sınır `area.x + width - 1`) DEĞİŞMEZ.
- Kenar rengi `theme::DIM` kalır.
- `cargo fmt -- src/tui/editor.rs` (crate genelinde değil).
- Bilinen ortam hatası, düzeltilmez: `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt`.
- Push yok, merge yok.

---

### Task 1: `InputBox::render` — sadece üst çizgi

**Files:**
- Modify: `src/tui/editor.rs` (`render` + doc comment; test modülü)

**Interfaces:** `pub fn render(&self, f: &mut Frame, area: Rect)` — imza DEĞİŞMEZ.

- [ ] **Step 1: Testleri güncelle / ekle**

Mevcut `render_draws_rules_above_and_below_without_side_borders` testini tamamen şununla değiştir:

```rust
    #[test]
    fn render_draws_only_a_top_rule() {
        let b = InputBox::new();
        let (rows, _) = render_rows(&b, 20, 5);
        assert_eq!(rows[0], "─".repeat(20), "top rule");
        assert!(rows[1].starts_with("> "), "prompt row: {:?}", rows[1]);
        for (i, r) in rows.iter().enumerate().skip(1) {
            assert!(!r.contains('─'), "row {i} has a rule: {r:?}");
        }
        for (i, r) in rows.iter().enumerate() {
            for c in ['│', '╭', '╮', '╰', '╯'] {
                assert!(!r.contains(c), "row {i} has {c:?}: {r:?}");
            }
        }
    }
```

Test modülünün sonuna (son `}`'den önce) ekle:

```rust
    #[test]
    fn render_uses_the_freed_row_for_a_fourth_content_line() {
        let mut b = InputBox::new();
        type_str(&mut b, &"a".repeat(55)); // 18 + 18 + 18 + 1 → four visual rows
        let (rows, cur) = render_rows(&b, 20, 5);
        assert_eq!(rows[1], format!("> {}", "a".repeat(18)));
        assert_eq!(rows[2], format!("  {}", "a".repeat(18)));
        assert_eq!(rows[3], format!("  {}", "a".repeat(18)));
        assert_eq!(rows[4], format!("  a{}", " ".repeat(17)));
        assert_eq!(cur, (3, 4), "cursor after the last char on row 4");
    }
```

- [ ] **Step 2: Kırmızı gör**

Run: `cargo test tui::editor::tests::render_`
Expected: 2 FAIL — `render_draws_only_a_top_rule` (satır 4 `─`), `render_uses_the_freed_row_for_a_fourth_content_line` (sadece 3 içerik satırı; satır 1 ikinci parçayı gösterir / imleç y=3).

- [ ] **Step 3: Implementasyon**

`render`'ın doc comment'inin ilk satırını:

```rust
    /// Draw the box: a rule above and below (no side borders) + `> ` prefix +
```

şununla değiştir:

```rust
    /// Draw the box: a single rule above (no side or bottom borders) + `> ` prefix +
```

`visible` satırı:

```rust
        let visible = area.height.saturating_sub(1).max(1) as usize; // lines under the rule
```

Block:

```rust
        let para = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(theme::DIM)),
        );
```

İmleç sınırı:

```rust
        f.set_cursor_position((
            x.min(area.x + area.width - 1),
            y.min(area.y + area.height - 1),
        ));
```

Başka satır değişmez.

- [ ] **Step 4: Yeşil + suite**

Run: `cargo fmt -- src/tui/editor.rs && cargo test && cargo clippy --all-targets`
Expected: editor testlerinin hepsi PASS (diğer render testleri — sarma, imleç x — değişmeden geçer); suite yalnız bilinen `materials` hatasıyla; clippy 0 uyarı.

- [ ] **Step 5: Commit**

```bash
git add src/tui/editor.rs
git commit -m "feat: input box keeps only its top rule — the freed row holds a fourth line"
```
