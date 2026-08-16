# Plan — Resume Continuation Panel (v0.21.0)

**Tasarım kaynağı:** Claude Design `f8cc2dc7` → `06-Resume-Continuation.dc.html`, **Varyant A** (onaylandı).
**Problem:** TUI'de konu seçicisinden resume edilince iki kutu üst üste basılıyor (`run.rs:369` identity + `run.rs:707` full-mode). Sol sütun (logo, selam, model, dizin) birebir aynı, `This week` satırı iki kere görünüyor. Hiçbir yüzey "kaldığın yerden devam ediyorsun" demiyor.
**Çözüm:** Resume yolunda full-mode kutusu yerine **tek sütunlu, logosuz continuation paneli** bas.

## Global Constraints

Bunlar bağlayıcı — her task bunlara uymak zorunda:

1. **`usta start <topic>` yolu DEĞİŞMEZ.** Orada identity kutusu hiç basılmıyor, tekrar yok. `had_topic_arg == true` → eskisi gibi `render_welcome`. Sadece `resumed == true` yolu değişir.
2. **Panelde kimlik tekrarı YOK:** logo yok, selamlama yok, model satırı yok, dizin satırı yok, `This week / streak` satırı yok, `/help` ipucu yok. Bunların hepsi bir üstteki identity kutusunda zaten basıldı.
3. **Turuncu disiplini (SPEC.md:217):** panelde en fazla **2** turuncu öğe. Panelde logo olmadığı için bütçe: başlık (1) + `Reviews` sayısı (2). Harita çubuğu DIM, yüzde DIM. Mevcut `orange_element_count` yardımcısıyla test edilir.
4. **Genişlik:** diğer tüm çerçevelerle aynı — `(width as usize).clamp(60, 100)`. Panelin kenarları üstündeki welcome kutusuyla hizalanmalı.
5. **Etiket sütunu:** 12 karakter, sola dayalı, DIM. Satır gövdesi `2 boşluk + pad(etiket,12) + 1 boşluk + değer` → değer 15. sütunda başlar.
6. **Eksik veri satırı DÜŞÜRÜR**, placeholder basmaz. Yalnız başlık her zaman var.
7. **Utandırma yok (SPEC.md:168 ADHD-safe):** uzun ara nötr yazılır (`12 days ago`). "streak 0", "uzun zamandır yoksun" benzeri hiçbir ifade yok.
8. **Saf render:** panel fonksiyonu IO yapmaz, model çağırmaz. Veri `WelcomeData`'dan gelir.
9. **Mevcut testler yeşil kalmalı:** `render_welcome`, `render_welcome_identity` ve `gather`'ın mevcut davranışı korunur (yalnız `gather` yeni bir alan doldurur).
10. **Türkçe girdi kabulü ve `Çırak→Usta` seviye adları DEĞİŞMEZ.** Panel gösterim dili İngilizce.

## Panel görsel kontratı (Varyant A — birebir)

```
· resuming: kaynak-ingest
╭─── Continuing · kaynak-ingest ──────────────────────────────────────╮
│  Last session 2 days ago · Level Başlangıç — sıfır noktası          │
│  Map          ▓▓▓░░░░░░░░░ 25%                                      │
│                                                                     │
│  Up next      URL → HTML fetch, then strip to text                  │
│  Reviews      3 due today                                           │
╰─────────────────────────────────────────────────────────────────────╯
```

Satır satır:

| sıra | içerik | kaynak | düşme koşulu |
|---|---|---|---|
| başlık | `Continuing · {topic}` | `d.topic` | asla |
| 1 | `Last session {rel}` + varsa ` · Level {level}` | `d.last_session`, `d.level` | ikisi de `None` |
| 2 | `Map          {bar} {p}%` | `d.map_percent` | `None` |
| 3 | boş satır | — | 4. ve 5. satırın ikisi de yoksa |
| 4 | `Up next      {next_item}` | `d.next_item` | `None` |
| 5 | `Reviews      {n} due today` | `d.due_count > 0` | `due_count == 0` |

- `{rel}`: `today` / `yesterday` / `{n} days ago`
- `{bar}`: 12 hücre, dolu `▓`, boş `░`
- 1. satırda `Last session` yoksa ama `Level` varsa satır `Level {level}` olarak basılır (etiket sütunu `Level`).

Stil: başlık `theme::brand()` + BOLD. Etiketler (`Last session`, `Level`, `Map`, `Up next`, `Reviews`) DIM. Çubuk DIM. Yüzde DIM. `Up next` ve `Level` değerleri düz. `Reviews` sayısı `theme::brand()` — panelde tek turuncu vurgu odur. Satır 1'deki ` · ` ayırıcı DIM.

---

## Task 1 — `last_session_ago` + `WelcomeData.last_session`

**Dosya:** `src/tui/welcome.rs` (ve gerekiyorsa `src/history.rs` salt okuma).

Bu task yalnız veri katmanı. Render yok.

### Yapılacaklar

1. `WelcomeData` struct'ına yeni alan: `pub last_session: Option<String>`. Alanı `streak`'ten sonra ekle.

2. Yeni saf fonksiyon:

```rust
/// Relative phrasing for the newest history entry of `topic`, EXCLUDING the
/// session being opened right now (its line is appended at close, not at open).
/// `0` → `today`, `1` → `yesterday`, `n` → `n days ago`. A future-dated entry
/// (clock skew) collapses to `today` rather than printing a negative count.
/// ADHD-safe: the phrasing is a neutral timestamp at every distance — no
/// streak-zero, no "it has been a while" (SPEC §"ADHD-safe rules").
pub fn last_session_ago(entries: &[crate::history::Entry], topic: &str, today: &str) -> Option<String>
```

- `entries` içinden `e.topic == topic` olanları süz, tarihi `chrono::NaiveDate::parse_from_str(&e.date, "%Y-%m-%d")` ile çöz, çözülemeyeni atla.
- En **yeni** tarihi seç (dosya sırasına güvenme — max al).
- `today` da aynı formatla çözülür; çözülemezse `None` dön.
- Fark gün olarak: `(today - last).num_days()`. `<= 0` → `"today"`, `1` → `"yesterday"`, `n` → `format!("{n} days ago")`.

3. `gather` içinde alanı doldur: mevcut `match history` bloğunda `entries` zaten hesaplanıyor (`let es = crate::history::entries(h);`) — aynı `es` üzerinden `last_session_ago(&es, topic, today)` çağır, `history` `None` ise alan `None`.

### Testler (hepsi yeni, `#[cfg(test)] mod tests` içine)

- `last_session_ago_today_yesterday_and_days`: aynı konudan 3 farklı tarihli entry → beklenen 3 ifade.
- `last_session_ago_picks_newest_not_last_line`: dosyada eski tarih SONRA yazılmış → yine en yeni tarih seçilir.
- `last_session_ago_filters_by_topic`: başka konunun daha yeni entry'si sonucu etkilemez.
- `last_session_ago_none_without_entry`: konuya ait entry yok → `None`.
- `last_session_ago_future_date_is_today`: yarının tarihli entry'si → `"today"` (negatif sayı basılmaz).
- `gather_fills_last_session`: `history` içeriği verilince alan dolu, `None` verilince boş.

### Doğrulama

`cargo test` tamamı yeşil (mevcut 347 + yeniler). `cargo clippy` yeni uyarı üretmemeli.

---

## Task 2 — `render_resume` + tek sütunlu kutu çizici

**Dosya:** `src/tui/welcome.rs`. Task 1 bitmiş olacak (`last_session` alanı mevcut).

### Yapılacaklar

1. Tek sütunlu kutu çizici — mevcut `render_box`'ın YANINA, onu değiştirmeden:

```rust
/// Draw a single-column bordered panel. `title` goes in the top border in
/// brand+bold; each row is a span list padded to the inner width. Same width
/// clamp as `render_box`, so the panel's edges line up with the welcome box
/// printed above it.
fn solo_box(title: &str, rows: Vec<Vec<Span<'static>>>, width: u16) -> Text<'static>
```

- `total = (width as usize).clamp(60, 100)`, `inner = total - 2`.
- Üst kenar `render_box` ile **aynı formülde**, ama başlık span'lara ayrılır:
  `Span::raw("╭─── ")` + `Span::styled(title, theme::brand().add_modifier(Modifier::BOLD))` + `Span::raw("─".repeat(inner.saturating_sub(4 + title.width())))` + `Span::raw("╮")`.
  (`render_box`'taki `4 + title.width()` formülü aynen korunur — eşit genişlik testi buna bağlı.)
- Her satır: `Span::raw("│")` + satırın span'ları + `Span::raw(" ".repeat(inner - satır_genişliği))` + `Span::raw("│")`. Satır genişliği span içeriklerinin `UnicodeWidthStr::width` toplamıdır. Taşarsa `saturating_sub` ile 0 dolgu (panik yok).
- Alt kenar `render_box` ile aynı: `╰` + `─`×inner + `╯`.

2. Yüzde çubuğu yardımcısı:

```rust
/// 12-cell progress bar: `▓` filled, `░` empty. `pct` is clamped to 0..=100.
fn map_bar(pct: u8) -> String
```
Dolu hücre sayısı = `(pct as usize * 12).div_ceil(100)` DEĞİL — yuvarlama: `((pct as f32) * 12.0 / 100.0).round() as usize`, sonra `.min(12)`. `pct > 0` ise en az 1 hücre dolu olmalı (yüzde 1'de çubuk tamamen boş görünmesin).

3. Ana render:

```rust
/// Resume mode: printed after the identity welcome when a saved topic is
/// picked. Deliberately carries NO identity — no logo, greeting, model, cwd
/// or week/streak line; all of those are already on screen in the identity
/// box above, and repeating them was the bug this panel replaces. Its job is
/// continuity: what you are picking up, when you were last here, how far
/// along the map you are. Design: Claude Design f8cc2dc7 page 06, variant A.
pub fn render_resume(d: &WelcomeData, width: u16) -> Text<'static>
```

Yukarıdaki "Panel görsel kontratı" tablosunu birebir uygula. Etiket dolgusu için mevcut `pad` yardımcısını kullan (`pad(label, 12)`).

`Up next` değeri uzun olursa: mevcut `wrap` ile inner genişliğine göre sar; devam satırları 15. sütuna hizalanır (yani `" ".repeat(15)` önekiyle). ASLA kısaltma/`…` yok — `render_welcome_long_next_item_wraps_full_text_no_ellipsis` testinin koruduğu kural burada da geçerli.

### Testler

- `render_resume_lines_have_equal_display_width`: tüm satırlar eşit görünür genişlikte (mevcut `render_welcome_lines_have_equal_display_width` testinin aynısı, `render_resume` için).
- `render_resume_orange_discipline`: dolu veri (level + map + next + due 3) → `orange_element_count(&t) <= 2`.
- `render_resume_has_no_identity`: çıktının düz metninde logo satırı (`██`), `Welcome back`, `opus · cli`, `This week` ve `/help` ipucu **geçmemeli**.
- `render_resume_title_names_the_topic`: üst kenarda `Continuing · kaynak-ingest` geçer.
- `render_resume_sparse_drops_rows`: `map_percent=None`, `due_count=0`, `level=None` → o satırlar hiç yok; `Last session` ve `Up next` var.
- `render_resume_bar_reflects_percent`: `%25` → 3 dolu hücre; `%1` → en az 1 dolu; `%100` → 12 dolu.
- `render_resume_long_next_item_wraps_no_ellipsis`: uzun `next_item` tam metin olarak çıkar, `…` yok.

### Doğrulama

`cargo test` tamamı yeşil. `cargo clippy` yeni uyarı üretmemeli.

---

## Task 3 — Wiring + belgeler + sürüm

**Dosyalar:** `src/tui/run.rs`, `SPEC.md`, `docs/ROADMAP.md`, `Cargo.toml`, `src/tui/welcome.rs` (yalnız sürüm testi).

### Yapılacaklar

1. `src/tui/run.rs` ~688-708: `if had_topic_arg || resumed { … render_welcome … }` bloğunu ikiye ayır:
   - `had_topic_arg` → eskisi gibi `welcome::render_welcome(&data, w)` (DEĞİŞMEZ).
   - `resumed` → `welcome::render_resume(&data, w)`.
   - `data` her iki dalda da aynı `welcome::gather(...)` çağrısından gelir — çağrıyı tekrarlama, tek `gather` + dallanan render.
   - Blok üstündeki yorumu (`two boxes stacked — similar to Claude Code's flow`) gerçeği yansıtacak şekilde güncelle: resume artık kimlik tekrarı değil continuation paneli basar.

2. `SPEC.md`: §5 welcome bölümüne (satır ~169 `Welcome line` maddesinin yanına) yeni madde ekle — resume yolunun continuation paneli bastığı, kimlik tekrarının kaldırıldığı, `usta start <topic>` yolunun değişmediği, panelin turuncu bütçesi. `- **Resume continuation panel (v0.21.0):** …` formatında tek madde.

3. `docs/ROADMAP.md`: en üste tarihli tek satır girdi (mevcut format).

4. Sürüm: `Cargo.toml` `0.20.4` → `0.21.0` (yeni yüzey = minor bump), `src/tui/welcome.rs` içindeki `version_aligned_with_spec` testindeki dizgeyi de `0.21.0` yap.

### Doğrulama

- `cargo test` tamamı yeşil.
- `cargo clippy` yeni uyarı yok.
- Elle doğrulama ATLA — Anil koşturacak.

---

## Notlar

- Claude Design sayfa `02-Welcome-and-Input.dc.html` §04 hâlâ eski `[E/h]` + "Enter picks the bright default" metnini gösteriyor. Düzeltme sayfa 06 §05'te belgelendi; sayfa 02'nin kendisi bu plan kapsamı DIŞINDA (ayrı iş).
- `render_box`'ın üst kenarındaki başlık kodda düz renk; sayfa 02 mock'u turuncu gösteriyor. Bu da ayrı bir drift; bu plan yalnız yeni panelde başlığı turuncu yapar, mevcut kutulara dokunmaz.
