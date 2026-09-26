# Input kutusu — yan çizgiler kalkıyor (sadece görünüm)

**Tarih:** 2026-09-26 · **Kapsam:** tek fonksiyon (`InputBox::render`, `src/tui/editor.rs`)

## İstek

Anil: "kullanıcının input girdi alanının çizgilerini kaldıralım" → seçilen varyant **B**: yan dikey
çizgiler (`│`) ve yuvarlak köşeler gider, üst ve alt yatay `─` çizgisi kalır (Claude Code
input'u gibi). Yazdıkça büyüyen yükseklik konuşuldu ve **bilinçli olarak kapsam dışı bırakıldı**
("çok zorlamadan").

## Bağlam — neden bu kadar dar

Bu görünüm v0.30.0'da bir kez geldi, ama relative renderer ile **paket halinde**; renderer gerçek
terminalde çöktü ve hepsi birlikte v0.31.1'de geri alındı (`f8ef640`, SPEC v0.31.0 paragrafı
"REVERTED"). Çerçevesiz görünümün kendisi sorun değildi — batan render mekanizmasıydı. Bu yüzden
bu iş **sadece çizim**: render yolu, viewport, resize, CPR'a dokunulmaz.

Büyüyen yükseklik reddedildi çünkü ratatui inline viewport yüksekliği sabit (`Viewport::Inline(VIEWPORT_H)`);
dinamik yükseklik ya viewport'u yeniden kurmayı (v0.29.1'de batan yol, `set_viewport_area` `pub(crate)`)
ya da altta boş satır rezervi tutmayı gerektirir. İkisi de "çok zorlamadan" ile uyuşmuyor.

## Tasarım

`InputBox::render` (`src/tui/editor.rs`) içinde:

| Ne | Şimdi | Sonra |
|---|---|---|
| Block kenarları | `Borders::ALL` + `BorderType::Rounded` | `Borders::TOP \| Borders::BOTTOM` (BorderType satırı silinir; düz çizgide etkisiz) |
| Kenar rengi | `theme::DIM` | aynı |
| Sarma genişliği `inner_w` | `width - 4` (2 kenar + `> `) | `width - 2` (sadece `> ` / `  ` öneki) |
| İmleç x | `area.x + 3 + cur_col`, sınır `area.x + width - 2` | `area.x + 2 + cur_col`, sınır `area.x + width - 1` |
| İmleç y | `area.y + 1 + …`, sınır `area.y + height - 2` | **aynı** (üst/alt çizgi hâlâ var) |
| Görünür satır `visible` | `height - 2` | **aynı** |

Doc comment ("rounded border + `> ` prefix") yeni hâle göre güncellenir; `inner_w` satırının yorumu
da (`// "> " prefix`). `BorderType` importu kullanılmaz hale gelirse kaldırılır.

**Dokunulmayanlar:** `VIEWPORT_H` (6: 3 metin + 2 çizgi + status), `page.rs`, `term.rs`,
`backend_wrap.rs`, resize yolu, `wrap_visual`, submit değeri.

## Test

`editor.rs` test modülüne ratatui `TestBackend` ile render testi (ör. 20×5 alan):

1. Boş input: satır 0 tamamen `─`, satır 4 tamamen `─`, satır 1 `> ` ile başlar; hiçbir satırda `│`,
   `╭`, `╮`, `╰`, `╯` yok.
2. Sarma genişliği: `width - 2` karakterlik metin tek içerik satırına sığar (satır 2 boş);
   `width - 1` karakter ikinci satıra taşar ve ikinci satır `  ` önekiyle başlar.
3. İmleç: `TestBackend` imleç konumu — boş inputta `(area.x + 2, area.y + 1)`.

Önce test yazılır ve kırmızı görülür (mevcut `Borders::ALL` ile 1 ve 2 düşer), sonra değişiklik.

## Doğrulama

- `cargo test` yeşil; `cargo fmt` sadece `src/tui/editor.rs` üzerinde; `cargo clippy` yeni uyarı yok.
- **Elle (bağlayıcı):** gerçek terminalde (VS Code entegre terminali) `usta` aç → kutuda yan çizgi yok,
  üst/alt çizgi var, uzun metin tam genişlikte sarıyor, imleç `> `'den sonra doğru yerde. Geçen seferin
  dersi: model/test yeşili gerçek terminalin yerine geçmez.

## Bilinen, bu işin çözmediği

Resize'daki hayalet çerçeve davranışı (v0.29.3 hâli) aynen sürer — hayaletler artık kutu değil
düz çizgi olarak görünür. Ne iyileşir ne kötüleşir.

## Sürüm

Patch bump + SPEC'te tek satırlık not (input frame: side borders removed, rules above/below, fixed
height). Bekleyen `2026-09-04-rustls-portability` planı da bir patch bump içeriyor (v0.31.2) —
hangisi önce koşarsa o sürümü alır, diğeri bir sonrakini; iki plan kod olarak çakışmaz.
