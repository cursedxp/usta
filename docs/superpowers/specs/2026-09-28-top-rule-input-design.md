# Input kutusu — alt çizgi de kalkıyor (sadece görünüm)

**Tarih:** 2026-09-28 · **Kapsam:** tek fonksiyon (`InputBox::render`, `src/tui/editor.rs`)

## İstek

Anil: "üstteki çizgi kalsın, alttaki çizgiyi de kaldıralım." v0.31.2'de yan çizgiler gitmişti
(`2026-09-26-borderless-input-design.md`); şimdi alttaki `─` de gidiyor. Kutunun tek çerçeve izi:
üstte `theme::DIM` renkli düz çizgi.

## Tasarım

Açılan satır boş bırakılmaz, **bir metin satırı daha** olur (3 → 4). Kutu yüksekliği ve
`VIEWPORT_H` (6) aynı kalır — viewport'a dokunmak v0.29.1/v0.30.0'da batan yoldur.

| Ne | Şimdi (v0.31.2) | Sonra |
|---|---|---|
| Block kenarları | `Borders::TOP \| Borders::BOTTOM` | `Borders::TOP` |
| Görünür satır `visible` | `height - 2` | `height - 1` |
| İmleç y sınırı | `area.y + height - 2` | `area.y + height - 1` |
| Sarma genişliği, imleç x | `width - 2`, `area.x + 2 + cur_col` | **aynı** |

Doc comment ("a rule above and below") ve `visible` satırının yorumu yeni hâle göre güncellenir.

**Dokunulmayanlar:** `VIEWPORT_H`, `page.rs` (kutu alanı `VIEWPORT_H - 1` = 5 satır, altında durum
satırı), `term.rs`, `backend_wrap.rs`, resize yolu, `wrap_visual`, submit değeri.

## Test

`editor.rs` testleri (20×5 alan, mevcut `render_rows` yardımcısı):

1. Mevcut `render_draws_rules_above_and_below_without_side_borders` → yeniden adlandırılır ve
   güncellenir: satır 0 tamamen `─`; **satır 0 dışında hiçbir satırda `─` yok**; `│╭╮╰╯` yok.
2. Yeni: 55 karakterlik metin (18+18+18+1 → 4 görsel satır) → satır 1–4 hepsi içerik (`> ` / `  `
   önekli), imleç `(3, 4)`.

Önce kırmızı: mevcut kodla 1 (satır 4 çizgi) ve 2 (sadece 3 satır görünür, imleç y=3'e kıstırılır)
düşer.

## Doğrulama

- `cargo test` yeşil (bilinen `materials::…pdftotext` hariç), clippy temiz, fmt sadece `editor.rs`.
- **Elle (bağlayıcı):** gerçek terminalde üstte çizgi, altta çizgi yok; durum satırı metnin hemen
  altında; 4 satırı aşan metinde pencere imleci izliyor.

## Bilinen

Resize hayaletleri (v0.29.3 davranışı) sürer; artık tek çizgi olarak görünür.

## Sürüm

Ayrı bump yok — `web-fetch` dalında, o planın Task 3'ündeki v0.31.3 bump'ına katılır (SPEC + ROADMAP
satırı orada).
