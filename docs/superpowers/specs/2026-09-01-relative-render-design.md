# Tasarım — Alt bölge: göreli render (Ink modeli) + kenarsız girdi çerçevesi (v0.30.0)

**Tarih:** 2026-09-01
**Kapsam:** Canlı alt bölge (girdi alanı + durum satırı) ratatui'nin `Viewport::Inline`'ından ÇIKARILIR; Claude Code'un kullandığı **göreli silme + yeniden basma** modeliyle kendimiz çizeriz. Girdi çerçevesi aynı hamlede kenarsız hale gelir: üstte ve altta tam genişlikte düz çizgi, yan kenar yok, yükseklik içerikle büyür.
**Durum:** Onaylandı → implement (Anil: "b", 2026-09-01)

## Neden — v0.29.1'in dersi ve kanıt

v0.29.1 resize kalıntısını göreli silmeyle çözmeye çalıştı ve **kötüleştirdi**; v0.29.3'te geri alındı. Kök sebep tek bir satırdı:

```rust
off = terminal.get_cursor_position()?.y - terminal.get_frame().area().y
```

İki değer farklı zamanlara ait — ilki `TrackedBackend::clamp_to_screen`'den geçip resize SONRASI yüksekliğe kırpılıyor, ikincisi hâlâ resize ÖNCESİ çapa. Fark gerçek ofsetten küçük çıkıyor, silme penceresi aşağı kayıyor, çerçevenin üst çizgileri ekranda kalıyor.

Ama asıl mesele o satır değil, **onu yazmak zorunda kalmamız.** Zincir şu: ratatui inline viewport MUTLAK satıra çapa atar → çapayı bilmek için imleç konumunu sorar (CPR) → CPR `EventStream` ile ölümcül çakışır (v0.26.1) → `TrackedBackend` icat edildi → terminal kendi reflow'unu yapınca izlenen konum bayatlar. Her halka bir öncekinin yamasıydı.

**Kurulu Claude Code binary'sinden çıkarılan kanıt** (`@anthropic-ai/claude-code-darwin-arm64`, v2.1.233):

- `github.com/vadimdemedes/ink/#israwmodesupported` hata metni → **Ink** (terminal için React reconciler), `yogaNode` 96 kez → **Yoga** flexbox layout.
- `ansi-escapes` modülü binary içinde açık: `cursorUp/cursorDown/cursorTo/eraseLine/eraseLines/eraseDown`. `eraseLines(n)` gövdesi birebir: her satırda `ESC[2K`, sonuncu hariç `ESC[A`, en sonda `ESC[G`.
- `handleResize = () => { if (!this.syncTerminalSize()) return; if (this.currentNode !== null) this.render(this.currentNode) }` — **resize'da özel bir şey yapmıyor, sadece yeniden render ediyor.**
- Çift tampon var: `frontFrame`/`backFrame`, ve şüphe anında tam boyamayı zorlayan `prevFrameContaminated` bayrağı; alternatif ekran yolunda ayrıca `needsEraseBeforePaint`.

**Belirleyici gözlem:** render döngüsünde MUTLAK satır adresi yok. Ne `MoveTo(row)`, ne CPR. İmleç bloğun sonunda park eder; yeniden çizerken *kendi yazdığı* satır sayısı kadar yukarı yürür, siler, yeniden basar. Yükseklik kütüphaneden geri okunan bir şey değil, **kendi tuttuğu bir sayıdır.** Bizim v0.29.1'de eksik olan şey tam olarak buydu.

## Karar

**K1 — Alt bölge ratatui'ye çizdirilmez.** `Viewport::Inline`, `Terminal::draw`, `insert_before`, `TrackedBackend`, `term::VIEWPORT_H` bu yoldan çıkar. Alt bölge birkaç satırlık ANSI metindir; onu doğrudan basmak, çapa/CPR/izleme zincirinin tamamını gereksiz kılar. `Terminal::draw`'un içindeki `autoresize` penceresi de böylece kapanır (v0.29.1'in açık bıraktığı kısıt).

**K2 — Tek gerçek: `painted`.** Ekrandaki canlı bloğun kaç satır olduğu, kütüphaneden okunan değil BİZİM yazdığımız sayıdır. Aynısı imleç için: bloğun son satırına göre kaç satır yukarıda olduğunu (`cursor_up`) biz kaydederiz.

**K3 — Mutlak satır adresi YASAK.** Yalnız göreli hareket (`MoveUp`/`MoveDown`) ve sütun adresi (`MoveToColumn` — sütun reflow'dan etkilenmez). `MoveTo(_, row)` üretim kodunda geçmez. CPR yasağı aynen sürer.

**K4 — Bloğun altı her boyamada silinir.** Her `paint` sonunda `Clear(FromCursorDown)`. Bloğun ALTINDA bize ya da kullanıcıya ait hiçbir şey yaşamaz, dolayısıyla bu silme her zaman güvenlidir — ve reflow taşması hep aşağı doğru olduğu için kalıntının ana kaynağını kapatır.

**K5 — Scrollback dokunulmaz.** Kalıcı içerik bir kez basılır, bir daha çizilmez (Ink'in `<Static>` karşılığı). Yazma sırası: canlı bloğu sil → içeriği bas → bloğu yeniden bas.

## Mimari

Yeni modül `src/tui/screen.rs` — canlı bloğun tek sahibi.

```
pub(crate) struct Screen {
    painted: u16,          // blok şu an kaç satır (K2)
    cursor_up: u16,        // görünür imleç, bloğun SON satırından kaç satır yukarıda
    last_widths: Vec<u16>, // en son basılan satırların görüntü genişlikleri (resize hesabı için)
    size: Size,
}
```

**Değişmez:** `paint` bittiğinde imleç girdi imlecinin göründüğü yerdedir; `cursor_up` oradan bloğun son satırına inmek için gereken sayıdır.

### `paint(lines, cursor_line, cursor_col)`

1. `MoveDown(cursor_up)` (0 değilse) → bloğun son satırı; `MoveToColumn(0)`
2. Yukarı silme: `painted` kez `Clear(CurrentLine)`, sonuncu hariç `MoveUp(1)`; ardından `MoveToColumn(0)`
3. Satırları `\r\n` ile yaz (ham modda `\n` satır başı yapmaz). **Her satır görüntü genişliği `width`'i AŞMAYACAK şekilde kırpılır** — otomatik sarma hiç tetiklenmez.
4. `Clear(FromCursorDown)` (K4)
5. İmleci yerleştir: `MoveUp(k)` + `MoveToColumn(col)`; `cursor_up = k`
6. `painted = lines.len()`, `last_widths` güncellenir

### `page(content)` — scrollback

1. `MoveDown(cursor_up)`, `MoveToColumn(0)`, `painted` satır yukarı silme
2. `painted = 0`, `cursor_up = 0`
3. İçerik + `\r\n` yazılır (terminal doğal olarak kaydırır)
4. Çağıran `paint` ile bloğu geri basar

### Resize

> **Superseded in v0.30.1** by `docs/superpowers/specs/2026-09-01-resize-repark-amendment-design.md` — step 1 below (the `painted * 2` descent and its bottom-of-screen premise) is no longer what the code does.

Terminal daraldığında ekranda duran bloğumuzun satırları yeniden sarılmış olabilir — `painted` gerçeği yansıtmaz. Sıra:

1. `MoveDown(painted * 2)` — terminaller son satırda durur, yani imleç **ekranın en alt satırında**, bilinen bir yerde. (Blok her zaman en alttadır: kalıcı içerik hep yukarı basılır.)
2. Yeniden sarma sonrası yükseklik `last_widths` ve YENİ genişlikten hesaplanır:
   `rewrapped = Σ ceil(w_i / new_width)`, `painted..=painted*2` aralığına kırpılır.
3. `rewrapped` satır yukarı silinir, blok yeni genişliğe göre yeniden basılır, K4 altı temizler.

**Dürüst kalan risk:** reflow davranışı terminale göre değişir — bazıları sert-sonlandırılmış satırları hiç yeniden sarmaz. O durumda `rewrapped` fazla çıkar ve blok üstündeki birkaç transcript satırı silinebilir. Kırpma aralığı bunu `painted` ile sınırlar; gerçek davranış Anil'in elle testinden sonra ayarlanır. Bugünkü halden (her resize'da kalıntı) kesin olarak daha iyi, ama "kanıtlanmış sıfır" değil — ölçülecek.

### Girdi çerçevesi

```
──────────────────────────────────────────────   ← üst kural, tam genişlik
> yazdığın metin sarınca                          ← içerik satırları, N ≥ 1
  bir alt satıra geçer
──────────────────────────────────────────────   ← alt kural
◐ watching   context 45k/1000k                    ← durum satırı (içeriği DEĞİŞMEZ)
```

- Yan kenar YOK. Kurallar `─`, `theme::DIM`, her zaman görünür (boş girdide de).
- Sarma genişliği `width - 2` (yalnız `> ` / `  ` öneki düşülür), bugünkü `width - 4` değil.
- Yükseklik `N + 3`. Taban `N = 1` → 4 satır. Tavan `INPUT_MAX_ROWS = 10` içerik satırı, ayrıca ekran yüksekliğinin yarısı — hangisi küçükse. Tavanda mevcut dikey pencere mantığı korunur.
- Yükseklik değişimi artık bir olay DEĞİL: `paint` her turda `painted` kadar siler, yeni satır sayısını basar. Viewport yeniden kurulumu diye bir şey yoktur.

## İsimlendirme (bağlayıcı)

- `screen::Screen` + `paint` / `page` / `resize` / `clear_block`
- `editor::INPUT_MAX_ROWS: usize = 10`
- `editor::frame_lines(&self, width: u16, screen_h: u16) -> (Vec<String>, u16, u16)` — basılacak satırlar + imleç satırı + imleç sütunu
- `term::VIEWPORT_H` KALDIRILIR
- `backend_wrap.rs` KALDIRILIR (CPR yamasının varlık sebebi ortadan kalkar)
- ~~`editor::content_rows`~~ — dropped from this binding list in v0.30.1: it ended up called by nothing but its own tests, because `frame_lines` already does the row arithmetic, and binding the two together would have applied the cap in two places (`docs/superpowers/specs/2026-09-01-resize-repark-amendment-design.md`).

## Test

- Birim `frame_lines` yüksekliği: boş → 1 · tam sığan → 1 · bir karakter taşan → 2 · `\n` → artar · tavan → `INPUT_MAX_ROWS` · kısa ekran → yarıyı geçmez. (Originally specified as a `content_rows` unit; that function was dropped in v0.30.1 and its five tests went with it — the surviving cases are `frame_lines`' own row-count and cap tests.)
- Birim `Screen`: sahte bir `Write` üzerine basılır, üretilen bayt dizisi doğrulanır — ilk `paint` silme üretmez; ikinci `paint` tam `painted` kez `ESC[2K` üretir; her `paint` `ESC[0J` ile biter; hiçbir çıktı `ESC[<row>;<col>H` İÇERMEZ (K3'ün mekanik bekçisi).
- Birim `rewrapped` hesabı: genişlik yarıya inince iki katına çıkar, kırpma aralığı tutar.
- Kaynak-pin: üretim kaynağında `cursor::position()` YOK, `MoveTo(` YOK, `Viewport::Inline` YOK, `insert_before` YOK.
- Elle doğrulama (Anil): boş girdi → 4 satır, iki çizgi, yan kenar yok · uzun metin → alt çizgi aşağı kayar · Ctrl+J → aynı · tavanı aş → içerik kayar · genişlet/daralt ve **sürükleyerek** boyutlandır → kalıntı yok, ekrandaki metin kaybolmaz · mentor yanıtı beklerken resize → aynı.

## Kapsam dışı

- Alternatif ekran / mouse takibi (Claude Code'da var, bizde gerek yok)
- Durum satırının içeriği veya biçimi
- Girdi düzenleme davranışı (tuşlar, geçmiş, yapıştırma)
- plain yol (`src/plain.rs`) — TUI yok, DEĞİŞMEZ
- Yoga benzeri layout motoru — alt bölge dikey bir liste, layout motoru gerekmez
