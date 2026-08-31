# Tasarım — Resize Sonrası Hayalet Çerçeveler (v0.29.1)

**Tarih:** 2026-08-31
**Kapsam:** Terminal genişliği değiştirilince inline viewport'un eski boyasının ekranda kalması — her resize bir yarım kutu iskeleti bırakıyor, üst üste binerek ekranı çöplüyor. v0.24.6 (event kolları) ve v0.26.0 (0.30 migrasyonu) bu semptomu KAPATMADI; ikisi de farklı bir katmanı düzeltti.
**Durum:** Onaylandı → implement (Anil: ekran silme kabul edilemez, 2026-08-31)

## Semptom

Anil oturum açıkken terminali birkaç kez yeniden boyutlandırdı. Ekranda dört ayrı `>` girdi kutusu kalıntısı: üst kenarları yok, alt kenarları eski (farklı) genişliklerde, aralarında `watching` durum satırları. Sadece en alttaki canlı; üstündekiler ölü boya.

## Kök neden (doğrulandı, ratatui-core 0.1.2 kaynağı okundu)

Üç mekanizma üst üste biniyor:

1. **`TrackedBackend`'in imleç konumu resize'da bayatlıyor.** `src/tui/backend_wrap.rs:22` imleci kendi izler (CPR sorgusu yok — v0.26.1 kararı; `EventStream` stdin kilidini tuttuğu için sorgu ölümcül). İzleme yalnız *bizim* yaptığımız hareketlerden beslenir: `draw`, `set_cursor_position`, `append_lines`. Terminalin KENDİ hareketi hiç görünmez — genişlik değişince terminal scrollback'i yeniden sarar (reflow), içerik k satır kayar, gerçek imleç onunla taşınır. `pos` eski satırda kalır. `clamp_to_screen` yalnız dikey taşmayı kırpar, bu kaymayı görmez.

2. **ratatui yeni viewport'un yerini o bayat satırdan hesaplıyor.** `terminal/resize.rs` → `compute_inline_size` ilk iş `backend.get_cursor_position()` çağırıp `row`'u oradan türetir. Bayat satır k kadar sapmışsa yeni viewport da k satır sapar.

3. **Inline temizlik yalnız AŞAĞIYI siliyor.** `terminal/buffers.rs::clear_viewport` inline yolda: imleci yeni viewport'un tepesine taşı + `clear_region(AfterCursor)`. Yeni tepenin ÜSTÜNDE kalan eski boya asla silinmez; genişleme yönünde ratatui hiç tam temizlik yapmaz (`next_area.width < viewport_area.width` yalnız daralmada tutar). Sonuç: her resize'da eski kutunun üst k satırı ekranda kalır → hayalet.

**Önemli ayrım:** scrollback'teki mentor yanıtları, kullanıcı ekoları vb. BOZULMUYOR. Onlar sıradan terminal metni; terminal reflow'u onları doğru sarıyor. Bozulan tek şey usta'nın mutlak konumla boyadığı kendi 6 satırlık viewport'u.

## Karar

Üç bağlayıcı karar:

**K1 — Ekran silinmez.** Görünen transcript'i silmek (`ClearType::All`) çözüm değil: kullanıcının okuduğu metin kaybolur, kaybı telafi etmek ekstra maliyettir (Anil reddi, 2026-08-31). Silme yalnız usta'nın kendi çerçevesine uygulanır.

**K2 — Eski çerçeve MUTLAK değil GÖRELİ silinir.** Reflow sonrası eski çerçevenin mutlak satırı bilinemez, ama gerçek imleç eski çerçevenin İÇİNDE ve terminal onu içerikle birlikte taşımıştır. İmlecin çerçeve içindeki ofseti reflow'dan ETKİLENMEZ:

```
off = tracked_cursor.y - viewport_area.y
```

İkisi de aynı k kadar kaydığı için fark tam doğrudur (izlenen değer bizim son yazımızdan gelir; gerçek imleç o an aynı yerdeydi). `MoveUp(off)` → eski çerçevenin gerçek tepesi. Oradan `VIEWPORT_H` satır `Clear(CurrentLine)` + `MoveDown(1)` → eski çerçeve, nereye kaydıysa oradan, tam olarak silinir. Üstündeki transcript'e dokunulmaz.

**K3 — İmleç sorulmaz, KONUR.** Mutlak `MoveTo` reflow'dan etkilenmez, CPR gerektirmez. Silme bittikten sonra imleç `(0, h - VIEWPORT_H)`'ye mutlak taşınır; artık izlenen durum gerçekle birebir aynıdır.

## Davranış

`page::handle_resize` şu sırayı uygular:

1. **Boyut kapısı.** `terminal.size()` (ioctl, CPR değil) okunur; `Tui::last_size` ile aynıysa hiçbir şey yapılmaz. Sürüklerken onlarca `Resize` event'i gelir; her birinde silme + yeniden kurulum yanıp sönmeye yol açar.
2. **Ofsetin ölçümü.** `off = terminal.get_cursor_position()?.y - terminal.get_frame().area().y` (ikisi de public API; `get_cursor_position` izlenen değeri döner, CPR yok).
3. **Göreli silme.** `execute!(stdout(), MoveUp(off) [off>0 ise], MoveToColumn(0))`, ardından `VIEWPORT_H` kez `Clear(ClearType::CurrentLine)` + (son hariç) `MoveDown(1)`.
4. **Mutlak çapa.** `execute!(stdout(), MoveTo(0, h.saturating_sub(VIEWPORT_H)))`.
5. **Terminal'i yeniden kur.** `tui.terminal = Terminal::with_options(TrackedBackend::new(CrosstermBackend::new(stdout()), Position { x: 0, y: h - VIEWPORT_H }), TerminalOptions { viewport: Inline(VIEWPORT_H) })?`.
6. `last_size` güncellenir. Döngüler zaten her turda `draw` çağırıyor — ek redraw yok.

**Neden `Terminal::resize()` değil, yeniden kurulum:** `resize()` yatay daralmada koşulsuz `next_area.y = 0` + `clear_region(All)` yapıyor (`terminal/resize.rs:41-45`) — yani K1'i ihlal ediyor ve bunu dışarıdan engellemenin public API'si yok (`set_viewport_area` `pub(crate)`). `with_options` ise inline yolda HİÇ temizlik yapmaz (`terminal/init.rs:119`), yalnız `compute_inline_size`'ı çalıştırır. Eski `Terminal` değerinin `Drop`'u sadece gizlenmiş imleci geri gösterir (`terminal.rs:473`) — yan etkisi yok, güvenle değiştirilebilir.

**Matematik (h = ekran yüksekliği, VIEWPORT_H = 6):** tohum `(0, h-6)`, `offset_in_previous_viewport = 0` → `row = h-6`, `lines_after_cursor = 5`, `available_lines = h-1-(h-6) = 5` → `missing = 0`, **kaydırma yok**, viewport = son 6 satır. `h < 6` durumunda tohum satır 0'a doyar; viewport ekran yüksekliğine kırpılır (`max_height = min(h, 6)`).

## Kabul edilen bedel

Genişletme yönünde terminal içeriği yukarı çektiğinde eski çerçeve ekranın altından k satır yukarıda kalır; onu sildikten sonra yeni çerçeve alta çapalanır, aradaki **k satır boş kalabilir**. Kozmetik boşluk — metin kaybı yok, hayalet yok. Bir sonraki `insert_before` ile dolar.

Reflow kayması ekranın tepesine dayanıp terminal imleci tam taşıyamazsa (çok dar/kısa ekran) silme bir-iki satır eksik kalabilir; bugünkü davranıştan kötü değil.

## İsimlendirme (bağlayıcı)

- `Tui::last_size: ratatui::layout::Size` — `term::setup()` içinde `terminal.size()?` ile tohumlanır.
- `term::rebuild_inline(seed: Position) -> Result<Terminal<TrackedBackend<Stdout>>>` — `setup()`'un viewport kurulum yarısı; `setup()` de bunu kullanır (tek kaynak, iki çağrı). CPR YOK: tohum parametreden gelir.
- `page::erase_plan(off: u16, rows: u16) -> ErasePlan` — saf: kaç satır yukarı, kaç satır silinecek. Test edilebilir çekirdek.
- `page::anchor_row(height: u16) -> u16` — `height.saturating_sub(VIEWPORT_H)`.
- `page::handle_resize(tui: &mut Tui) -> Result<()>` — imza DEĞİŞMEZ; dört çağrı yeri (run.rs, ask.rs ×2, entry.rs) aynen kalır.

## Test

- Birim: `anchor_row` — `30 → 24`, `6 → 0`, `4 → 0` (doyum).
- Birim: `erase_plan` — `off` kadar yukarı, `VIEWPORT_H` satır silme; `off = 0`'da `MoveUp` üretilmez (crossterm `MoveUp(0)` bazı terminallerde 1 sayılır).
- Birim: boyut kapısı — aynı boyutta `false`.
- Kaynak-pin (TUI sürülemiyor, mevcut desen): `handle_resize` üretim gövdesi `ClearType::CurrentLine` ve `MoveUp` içerir, `ClearType::All` İÇERMEZ (K1'in mekanik bekçisi), `autoresize`/`\.resize(` çağırmaz (K1: daralma kolu tetiklenmemeli), `rebuild_inline` çağırır. Dört döngüdeki `Event::Resize` kolları hâlâ pinli. `cpr_seed_happens_before_event_stream` genişletilir: `rebuild_inline` de gerçek CPR yapmamalı — `setup()`'taki tek `cursor::position()` sayısı 1 kalır.
- Elle doğrulama (Anil): mentor yanıtı ekranda dururken genişlet → **yanıt yerinde**, tek kutu altta, hayalet yok · daralt → aynı · sürükleyerek yavaş boyutlandır → kalıntı yok · spinner dönerken resize → aynı · giriş akışı (`entry.rs`) ve onay sorusu (`ask.rs`) → aynı.

## Kapsam dışı

- Eski scrollback'in yeniden basılması (terminal kendi reflow'uyla zaten doğru sarıyor)
- Genişletmede oluşabilen kozmetik boşluğun kapatılması
- plain yol (TUI yok)
- ratatui'nin daralmadaki `y = 0` + tam temizlik kolunu upstream'de düzeltmek
