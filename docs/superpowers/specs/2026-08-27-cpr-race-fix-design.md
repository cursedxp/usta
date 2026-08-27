# Tasarım — CPR Yarışı Kalıcı Fix: Tracked-Cursor Backend Wrapper (v0.26.1)

**Tarih:** 2026-08-27
**Kapsam:** v0.26.0 sonrası canlıda görülen ölümcül hata: `Error: The cursor position could not be read within a normal duration`. Kök neden upstream ve AÇIK: ratatui issue #2640 — inline viewport işlemlerinin bir kısmı CPR sorgusu (imleç konumu, cevabı stdin'den `ESC[...R`) yapıyor; crossterm `EventStream` stdin'i okurken CPR cevabını yutuyor → 2 sn timeout → uygulama ölüyor. #2483 yalnız `resize()` yolunu düzeltti; kurulum/`clear()`/diğer yollar hâlâ yarışıyor. Fix upstream'de merge edilmemiş/planlanmamış — kendi tarafımızda upstream'in önerdiği desenle kapatıyoruz: **backend sarmalayıcı, imleç konumunu kendisi takip eder ve CPR sorularını takip edilen durumdan cevaplar — stdin'e sorgu hiç gitmez.**
**Durum:** Onaylandı → implement (Anil: "başlat", 2026-08-27)

## İsimlendirme (bağlayıcı — tüm yeni kod İngilizce)

- Modül: `src/tui/backend_wrap.rs` · Tip: `TrackedBackend` (CrosstermBackend<Stdout> sarar, `ratatui::backend::Backend` implement eder)
- `Tui.terminal` tipi `Terminal<TrackedBackend>` olur (term.rs); `run.rs` tipi değişmez şekilde etkilenirse mekanik uyarlama.

## Davranış

- **Tohumlama:** gerçek CPR sorgusu YALNIZ `term::setup()` içinde, `EventStream` yaratılmadan ÖNCE, bir kez yapılır (o anda stdin'de rakip okuyucu yok — güvenli). Değer `TrackedBackend`'in imleç durumunu tohumlar. Sorgu orada dahi başarısızsa güvenli varsayılana düşülür (satır = terminal yüksekliği-1 benzeri; keşif task'ı belirler) — açılış ASLA bu yüzden ölmez.
- **Takip:** `Backend` trait'inin imleci hareket ettiren tüm operasyonları (`set_cursor_position`, `append_lines`, içerik yazımı/scroll — keşif task'ı vendored kaynaktan TAM listeyi çıkarır) tracked konumu günceller. `get_cursor_position()` HER ZAMAN tracked değeri döner; stdin'e CPR gitmez.
- **Doğruluk ölçütü:** inline viewport'un CPR'dan beklediği bilgi (viewport çapası hesabı) tracked değerle birebir aynı sonucu vermeli — keşif task'ı ratatui-core 0.1.2 vendored kaynağından `get_cursor_position` çağrı yollarını (kurulum, clear, insert_before, resize, draw) ve her birinin değeri nasıl kullandığını çıkarır; implementasyon o listeye göre yazılır. Ezber API yok.
- **Kaçış kapısı (keşif kararı):** takip edilen konumun bir operasyon için doğru üretilemeyeceği kanıtlanırsa, O operasyon için onaylı alternatif: operasyonu EventStream yokken sıralamak (örn. kurulum zaten öyle) veya gerçek sorguya kontrollü düşmek — karar raporda gerekçelenir, sessiz geçilmez.
- **Parite:** görünür davranış birebir; tüm mevcut testler davranış assert'leri değişmeden yeşil. `handle_resize`, `Event::Resize` yakalama noktaları, pin testleri aynen.

## Test

- `TrackedBackend` birim testleri: tohum → get döner; set_cursor_position → get izler; içerik/scroll operasyonu → konum güncellenir (keşif listesindeki her operasyon sınıfına bir test); stdin'e CPR yazılmadığının kanıtı (test backend'i/buffer üstünden — gerçek TTY gerekmez, `Backend` trait'i `TestBackend` benzeri sarmayla test edilebilir; koda bak).
- Pin: `term.rs` kurulum sırası — "seed BEFORE EventStream" kaynak-pin testi (setup'ta seed çağrısı var + run.rs'te EventStream::new setup'tan sonra).
- Elle doğrulama (Anil — kapanış kriteri): dünkü çökme senaryosu (açılış → boş Enter → öneri akışı) 5-6 kez üst üste → CPR hatası yok · yatay/dikey resize hâlâ düzgün · normal ders akışı.

## Kapsam dışı

- Upstream'e PR (ayrı iş, istenirse) · plain yol (TUI yok, CPR yok) · CPR hatasını yakalayıp yeniden deneme (wrapper sorguyu kökten kaldırıyor — moot).
