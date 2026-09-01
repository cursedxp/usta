# Tasarım eki — `Screen::resize`: ekran-dibi varsayımını kaldır (v0.30.1)

**Tarih:** 2026-09-01
**Ana tasarım:** `docs/superpowers/specs/2026-09-01-relative-render-design.md` (v0.30.0). Bu dosya onun **yalnız `resize` bölümünü** değiştirir; K1–K5 ve diğer her şey aynen geçerlidir.
**Durum:** Onaylandı → implement (Anil: "a" = önce düzelt, sonra merge, 2026-09-01)

## Düzeltilen hata — ve kimin hatası

v0.30.0 dalı kapandığında final review şunu bildirdi ve algoritmaya **bilinçli olarak dokunmadı** (doğru davranış: "tıkanırsan dur, tasarım uydurma" — v0.29.1 tam da uydurmaktan battı).

Ana tasarım şöyle diyordu: *"Blok her zaman en alttadır: kalıcı içerik hep yukarı basılır."* **Bu cümle yanlıştır** — ancak yeterince çıktı biriktikten SONRA doğru olur. Yeni açılmış bir oturumda, uzun bir terminalde blok ekranın ortasında durur ve altında boş satırlar vardır.

`Screen::resize` bu cümleye yaslanıyor:

```rust
let down = self.painted.saturating_mul(2);
queue!(self.out, MoveDown(down))?;   // "terminal son satırda durur" → ekran dibi
```

Altta boş satır varken imleç bloğun ALTINA, boşluğa iner. Yukarı silme K4'ün zaten boşalttığı satırları temizler ve bloğun üst satırları sağlam kalır → **hayalet**. v0.29.1'in semptomu, başka mekanizmayla. Analitik olarak kesin, olasılık değil.

**Hata spec'indir, kodun değil.** Ana tasarımın yazarı (Suzi) ölçülmemiş bir premise'i "her zaman" diye yazdı. Ek bu premise'i tamamen kaldırır.

## Karar

**A1 — Ekran dibine inilmez. Kendi sayılarımızla inilir.** `Screen` zaten `cursor_up`'ı tutuyor: görünür imleçten bloğun son satırına inmek için gereken satır sayısı. Yeniden boyutlandırma olmasaydı iniş tam olarak bu olurdu. Genişlik değiştiğinde imlecin ALTINDA kalan blok satırlarının yeniden sarması bu sayıya eklenir — hepsi `last_widths`'ten hesaplanır. `painted * 2` gibi bir üst sınır tahminine ve ekranın neresinde olduğumuza dair hiçbir varsayıma gerek kalmaz.

**A2 — İmleç sütunu da saklanır.** İmlecin bulunduğu satır yeniden sarılırsa, imleç o satırın kaçıncı görsel satırında kaldığı `cursor_col / new_width` ile bilinir. Bugün `cursor_col` `paint`'e geliyor ama saklanmıyor; saklanacak.

**A3 — Kalan risk açıkça yazılır ve ölçülür.** Hesap, terminalin sert-sonlandırılmış satırları daraltmada YENİDEN SARDIĞI varsayımına dayanır. Yaygın terminaller (iTerm2, Terminal.app, Ghostty, kitty, WezTerm) sarar; sarmayan bir terminalde iniş `extra_below` kadar fazla olur ve daraltmada kalıntı kalabilir. Bu, v0.30.0'ın kesin hatasının yerine geçen **koşullu ve dar** bir risktir; tek yerde (`descend_rows`) yaşar ve ileride tek satırlık bir politika anahtarıyla çevrilebilir. Ölçüm elle testtedir.

## Davranış

`Screen`'e bir alan eklenir: `cursor_col: u16` (son `paint`'te verilen sütun).

Yeni saf yardımcı:

```
descend_rows(last_widths, painted, cursor_up, cursor_col, new_width) -> u16
```

`c = painted - 1 - cursor_up` (imlecin bulunduğu mantıksal satırın indisi) olmak üzere:

```
rows(w)      = max(1, ceil(w / new_width))          // bir mantıksal satırın kapladığı görsel satır
descend      = (rows(w_c) - 1 - min(cursor_col / new_width, rows(w_c) - 1))
             + Σ_{i = c+1}^{painted-1} rows(w_i)
```

Yani: imlecin kendi satırında altında kalan görsel satırlar + ondan sonraki tüm satırların sarılmış yükseklikleri. `painted == 0` ise `0`.

`resize` gövdesi:

1. `descend = descend_rows(...)`; `descend != 0` ise `MoveDown(descend)`
2. `MoveToColumn(0)`
3. `rewrapped = rewrapped_rows(...)` — **DEĞİŞMEZ**, bugünkü hesap ve kırpma aynen kalır
4. `rewrapped` kez `Clear(CurrentLine)`, sonuncu hariç `MoveUp(1)`
5. `MoveToColumn(0)`, `size` güncellenir, `forget_block()`, flush

`MoveDown(painted * 2)` satırı ve ona dayanan doküman yorumu kaldırılır; yerine A1/A3 anlatılır.

## İkinci madde — `editor::content_rows` silinir

Ana tasarımın "İsimlendirme (bağlayıcı)" bölümü `content_rows`'u zorunlu kıldı, ama `frame_lines` işi zaten yapıyor; fonksiyon çağrısız kaldı ve `#[allow(dead_code)]` ile taşınıyor. Bağlamak tavanı iki ayrı yerde uygulatır — daha kötü. **Silinir**, ana spec'in isim listesinden çıkarılır. Testleri de gider; `frame_lines`'ın satır sayısı ve tavan testleri zaten kapsıyor.

## Test

- Birim `descend_rows`:
  - genişlik değişmemiş, imleç son satırda → `0`
  - genişlik değişmemiş, imleç iki satır yukarıda → `cursor_up` ile aynı
  - imlecin satırı ikiye sarılıyor, imleç ilk yarıda → +1
  - imlecin satırı ikiye sarılıyor, imleç ikinci yarıda → +0
  - imleç altındaki iki satır ikiye sarılıyor → +2
  - `painted == 0` → `0`
  - `new_width == 0` → panik yok, `rows` tabanı 1
- Birim `resize` (sahte `Write`): üretilen bayt dizisinde `MoveDown` miktarı `descend_rows`'un döndürdüğüne EŞİT; `painted * 2` değil. Aynı testte mutlak konum dizisi (`H`, `d`) yok (K3).
- **Isıran regresyon testi:** blok ekranın ortasındaymış gibi (`painted` küçük, altında boşluk) bir `resize` çağrısı — iniş `painted * 2` olsaydı FAIL edecek şekilde yazılır. v0.30.0'ın hatasının mekanik bekçisi budur.
- Kaynak-pin: `screen.rs` üretim gövdesinde `saturating_mul(2)` YOK.
- Elle doğrulama (Anil): **uzun terminalde yeni oturum aç** (blok ortada, altında boşluk kalsın) → yatay genişlet/daralt → hayalet yok · sonra uzun bir oturumda (blok dipte) aynı testler → hayalet yok, metin kaybı yok · sürükleyerek boyutlandır → aynı.

## Kapsam dışı

- `rewrapped_rows` hesabı ve kırpması (dokunulmaz)
- Yeniden sarmayan terminaller için politika anahtarı (A3 — ölçüm sonrası)
- Ana tasarımın diğer tüm bölümleri
