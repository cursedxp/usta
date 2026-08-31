# Tasarım — Resize Sonrası Hayalet Çerçeveler (v0.29.1)

**Tarih:** 2026-08-31
**Kapsam:** Terminal genişliği değiştirilince inline viewport'un eski boyasının ekranda kalması — her resize bir yarım kutu iskeleti bırakıyor, üst üste binerek ekranı çöplüyor. v0.24.6 (event kolları) ve v0.26.0 (0.30 migrasyonu) bu semptomu KAPATMADI; ikisi de farklı bir katmanı düzeltti.
**Durum:** Onay bekliyor (Anil ekran görüntüsü, 2026-08-31)

## Semptom

Anil oturum açıkken terminali birkaç kez yeniden boyutlandırdı. Ekranda dört ayrı `>` girdi kutusu kalıntısı: üst kenarları yok, alt kenarları eski (farklı) genişliklerde, aralarında `watching` durum satırları. Sadece en alttaki canlı; üstündekiler ölü boya.

## Kök neden (doğrulandı, kaynak okundu)

Üç mekanizma üst üste biniyor:

1. **`TrackedBackend`'in imleç konumu resize'da bayatlıyor.** `src/tui/backend_wrap.rs` imleci kendi izler (CPR sorgusu yok — v0.26.1 kararı, EventStream stdin kilidini tuttuğu için sorgu ölümcül). İzleme yalnız *bizim* yaptığımız hareketlerden beslenir: `draw`, `set_cursor_position`, `append_lines`. Terminalin KENDİ hareketi hiç görünmez — genişlik değişince terminal scrollback'i yeniden sarar (reflow), tüm içerik yukarı/aşağı kayar ve gerçek imleç onunla birlikte taşınır. `pos` eski satırda kalır. `clamp_to_screen` yalnız dikey taşmayı kırpar, bu kaymayı görmez.

2. **ratatui yeni viewport'un yerini o bayat satırdan hesaplıyor.** `ratatui-core-0.1.2/src/terminal/resize.rs` → `compute_inline_size` ilk iş `backend.get_cursor_position()` çağırır ve `row`'u oradan türetir. Bayat satır k kadar sapmışsa yeni viewport da k satır sapar.

3. **Temizlik yalnız AŞAĞIYI siliyor.** `buffers.rs::clear_viewport` inline yolda: imleci yeni viewport'un tepesine taşı + `clear_region(AfterCursor)`. Yani yeni tepenin ÜSTÜNDE kalan eski boya asla silinmez. Genişleme yönünde ratatui ayrıca hiç tam temizlik yapmaz (`next_area.width < viewport_area.width` koşulu yalnız daralmada tutar). Sonuç: her resize'da eski kutunun üst k satırı ekranda kalır → hayalet.

Ek yan etki (aynı kökten): daralmada ratatui `next_area.y = 0` yapıp ekranı tamamen siliyor — kutu boş ekranın TEPESİNE yapışıyor, içerik biriktikçe yavaşça aşağı iniyor. Bozuk değil ama tutarsız.

## Karar — imleci sormak yerine KOYMAK

Reflow sonrası imlecin nerede olduğu CPR olmadan bilinemez; CPR bu mimaride yasak (v0.26.1). Ama **mutlak `MoveTo` reflow'dan etkilenmez** — imleci bilinen bir yere koymak, izlenen durumu gerçekle yeniden senkronlar. Tahmin yok, sorgu yok.

Çapa noktası: **ekranın sol-alt köşesi** (`x=0, y=h-1`). Zaten `fallback_seed`'in kullandığı model; usta sürekli yukarı doğru yazdığı için viewport pratikte hep altta.

## Davranış

`page::handle_resize` şu sırayı uygular:

1. **Boyut kapısı.** `terminal.size()` (ioctl, CPR değil) okunur; `Tui`'ye eklenen `last_size` ile aynıysa hiçbir şey yapılmaz, `Ok(())`. Sürükleyerek boyutlandırmada onlarca `Resize` event'i gelir — temizliğin her birinde patlaması yanıp sönmeye yol açar.
2. **Hayaleti öldür.** `backend_mut().clear_region(ClearType::All)` — reflow sonrası eski çerçevenin yeri bilinemediği için görünen ekranın tamamı silinir. Kısmi silme için güvenli bir üst sınır YOK (reflow kayması bir paragrafın yeniden sarılması kadar büyük olabilir).
3. **İmleci senkronla.** `terminal.set_cursor_position(Position { x: 0, y: h-1 })` — hem gerçek imleci mutlak olarak taşır (TrackedBackend `pos`'u aynı değere yazar), hem ratatui'nin `last_known_cursor_pos`'unu tazeler.
4. **Viewport'u alta çapala.** `terminal.resize(Rect::from(size))` **iki kez**, arada imleç yeniden alta konarak. İlk çağrı yatay daralmada `next_area.y = 0`'a zorlar (ratatui'nin kendi kolu, kaçınılmaz); ikinci çağrıda `viewport_area.width` artık yeni genişliğe eşittir, o kol tutmaz ve `compute_inline_size` imleçten alt çapayı üretir. Ekran zaten boş olduğu için ikinci temizlik bedava.
5. `last_size` güncellenir. Döngüler zaten her turda `draw` çağırıyor — ek redraw yok.

Matematik (h = ekran yüksekliği, VIEWPORT_H = 6): imleç `h-1`, `offset = (h-1) - viewport_top`, `lines_after_cursor = 6 - offset - 1`; alt satırda `available_lines = 0` olduğundan `missing = lines_after_cursor` ve `row = h-1-lines_after_cursor-offset = h-6`. Sonuç offset'ten BAĞIMSIZ: viewport her zaman son 6 satır.

`autoresize()` artık çağrılmaz — kendi `resize()`'ımız aynı işi yapıyor ve boyut kapısı zaten no-op'u üstleniyor.

## Kabul edilen bedel

Her gerçek boyut değişiminde **görünen ekran silinir**. Ekranda okunmakta olan mentor yanıtı kaybolur (scrollback'e düşmemiş kısım geri gelmez). Alternatifler elendi: kısmi silme için sapma sınırı bilinemez; ekranı newline'larla scrollback'e itmek hayaleti kalıcı kılar; CPR ile yeniden tohumlama v0.26.1'in kapattığı çökme sınıfını geri açar. Tahribatsız gerçek çözüm — resize sonrası transcript'i yeniden basmak — ayrı bir özellik, KAPSAM DIŞI. ratatui zaten daralmada aynı şeyi yapıyor; bu karar davranışı iki yönde de tutarlı kılıyor.

## İsimlendirme (bağlayıcı)

- `Tui::last_size: ratatui::layout::Size` — `term::setup()` içinde `terminal.size()?` ile tohumlanır.
- `page::resize_anchor(size: Size) -> (Position, Rect)` — saf yardımcı: alt-sol imleç + tam ekran dikdörtgeni. Test edilebilir çekirdek.
- `page::handle_resize(tui: &mut Tui) -> Result<()>` — imza değişmiyor, dört çağrı yeri (run.rs, ask.rs ×2, entry.rs) aynen kalır.

## Test

- Birim: `resize_anchor` — imleç `y = h-1`, dikdörtgen `Rect::new(0, 0, w, h)`; `h = 0` kenar durumunda satır 0'a doymalı.
- Birim: boyut kapısı — `size_changed(prev, now)` benzeri saf karşılaştırma; aynı boyutta `false`.
- Kaynak-pin (TUI sürülemiyor, mevcut desen): `handle_resize` üretim kaynağı `ClearType::All`, `set_cursor_position` ve İKİ `resize(` çağrısı içerir; dört döngüdeki `Event::Resize` kolları hâlâ pinli; `backend_wrap.rs`/`page.rs`'te ikinci bir gerçek CPR yok (mevcut `cpr_seed_happens_before_event_stream` testi genişletilmeden korunur).
- Elle doğrulama (Anil): boşta iken genişlet → tek kutu, alta yapışık, hayalet yok · daralt → aynı · sürükleyerek yavaşça boyutlandır → yanıp sönme kabul edilebilir, kalıntı yok · mentor yanıtı beklenirken (spinner) resize → aynı · giriş akışında (`entry.rs`) ve onay sorusunda (`ask.rs`) resize → aynı.

## Kapsam dışı

- Eski scrollback'in yeniden sarılması / transcript'in yeniden basılması
- plain yol (TUI yok)
- ratatui'nin daralmadaki `y = 0` kolunu upstream'de düzeltmek
