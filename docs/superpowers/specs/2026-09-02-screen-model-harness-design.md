# Tasarım — Ekran modeli koşumu: kalıntıyı iddia etmeyi bırak, ÖLÇ (v0.31.0)

**Tarih:** 2026-09-02
**Kapsam:** `Screen`'in ürettiği bayt dizisini gerçekten bir ekrana uygulayan, yeniden boyutlandırmayı iki farklı terminal politikasıyla modelleyen bir test koşumu; ardından resize kalıntısının ölçüme dayalı düzeltmesi.
**Durum:** Onaylandı → implement (Anil: "2" = önce ölçüm, 2026-09-02)

## Neden — iki başarısız turun ortak sebebi

v0.29.1 ve v0.30.1'in ikisi de kâğıt üstünde doğru, ekranda yanlış çıktı. Ortak sebep test yüzeyi:

`screen.rs`'in tüm testleri sahte bir `Write`'a bakıyor ve **üretilen bayt dizisini** doğruluyor — "şu kadar `ESC[2K` çıktı", "`ESC[0J` ile bitti". Hiçbiri o baytları bir ekrana **uygulamıyor**. Bayt dizisi doğru, sonuç yanlış olabiliyor; 524 test yeşilken ekran çizgi yığını olabiliyor. v0.30.1'in kanıtı bu ekran görüntüsüdür.

Ölçülmeyen ikinci şey: terminalin daraltmada sert-sonlandırılmış satırları yeniden sarıp sarmadığı. Kod bunu VARSAYIYOR ve doküman üç yerde "ölçülmedi" diye yazıyor. Varsayım yanlışsa hesap ters yöne kayıyor.

## v0.30.1'in somut hatası (teşhis, düzeltme değil)

`Screen::resize` silmeyi yapıp sonunda `forget_block()` çağırıyor — `painted = 0`. Sonraki `paint` silmeyi `painted` üzerinden yaptığı için **hiçbir şey silmiyor**; bloğu imlecin bulunduğu yere basıyor. Yani tüm silme sorumluluğu tek bir atışa, `resize`'ın kendisine yüklenmiş. O atış eksik kalırsa artık **kalıcıdır** — bir daha kimse o satırları temizlemez. Sürüklerken düzinelerce resize olayı gelir, her biri artığını bırakır, birikir.

Eksik kalmasının garantili bir yolu da var: `rewrapped_rows`, `painted..=painted*2` aralığına kırpılıyor. Kurallar tam genişlikte olduğu için terminal ikiden fazla kat daraltıldığında blok 2×'ten fazla satır kaplar ve kırpma silmeyi kesin olarak yetersiz bırakır.

**Bu tasarım bu teşhisi doğrulanacak hipotez olarak alır, düzeltmeyi ondan türetmez.** Düzeltme koşumun ürettiği ölçümden çıkar.

## Karar

**M1 — Kendi ekran modelimizi yazarız, genel bir VT emülatörü değil.** `Screen`'in ürettiği kaçış kümesi bize ait ve dardır: `MoveUp(n)` · `MoveDown(n)` · `MoveToColumn(n)` · `Clear(CurrentLine)` · `Clear(FromCursorDown)` · `CR`/`LF` · düz metin. Yalnız bunları anlayan ~150 satırlık bir hücre ızgarası modeli tam ve kesindir. Yeni bağımlılık yok.

**M2 — Model, tanımadığı kaçışta PANİK eder.** Bu ikinci bir K3 bekçisidir: üretim kodu modelin bilmediği bir kaçış yayarsa test kırmızı olur. `MoveToRow`'un iki koruma katmanından kaçtığı emsal (v0.30.0 final review) bir daha yaşanmaz.

**M3 — Yeniden boyutlandırma İKİ politikayla modellenir.** `ResizePolicy::Reflow` (satırlar yeni genişliğe göre yeniden sarılır — iTerm2, Ghostty, kitty, WezTerm) ve `ResizePolicy::NoReflow` (sert-sonlandırılmış satır bir fiziksel satır kalır, taşan kısım kırpılır — bazı terminaller ve tmux yapılandırmaları). **Düzeltme her iki politikada da kalıntısız geçmek ZORUNDADIR.** Böylece "gerçek terminal hangisini yapıyor" sorusu düzeltmenin doğruluğu için gereksizleşir — ölçülmemiş varsayım ortadan kalkar.

**M4 — Kabul ölçütü ekran içeriğidir, bayt dizisi değil.** Testler modelin ızgarasını okur: "bloğun üstünde kural karakteri içeren satır YOK", "blok tam olarak şu satırlarda", "bloğun altı boş".

**M5 — Önce gözlenen hata yeniden üretilir.** Düzeltmeye dokunmadan, bugünkü kodla, Anil'in ekran görüntüsündeki birikmeyi gösteren bir test yazılır ve **kırmızı** olduğu görülür. Kırmızı olmuyorsa model veya senaryo yanlıştır; düzeltmeye geçilmez.

## Davranış

### Model (test-only, `src/tui/screen_model.rs`, `#[cfg(test)]`)

```
struct TermModel { rows: Vec<String>, cursor: (u16, u16), w: u16, h: u16 }
fn apply(&mut self, bytes: &[u8])            // M1'deki kaçış kümesi; bilinmeyende panic (M2)
fn resize(&mut self, w: u16, h: u16, policy: ResizePolicy)
fn rows_containing(&self, needle: char) -> Vec<u16>
fn snapshot(&self) -> Vec<String>
```

- İmleç ekran sınırlarında durur (gerçek terminal davranışı): son satırda `MoveDown` hareket etmez, ilk satırda `MoveUp` hareket etmez.
- Son satırda `LF` ekranı yukarı kaydırır ve en üstteki satırı düşürür (scrollback modellenmez — düşen satır kaybolur; testler bloğun kendisine bakar).
- `Reflow` politikasında satırlar yeniden sarılırken imleç, üzerinde bulunduğu içerikle birlikte taşınır.

### Senaryolar (en az bunlar)

1. **Taze oturum, uzun terminal:** blok ekranın ortasında, altında boş satırlar. Yatay daralt → genişlet → daralt. Her adımdan sonra `paint`. Kabul: kural karakteri içeren satır sayısı tam olarak 2 (üst + alt kural).
2. **Sürükleme:** tek `paint` arasına 20 ardışık resize; her biri farklı genişlik. Kabul: aynı.
3. **Sert daralma:** genişlik 200 → 60 (üçten fazla kat). `painted*2` kırpmasının yetersiz kaldığı vaka.
4. **Dip senaryosu:** blok ekranın son satırlarında, üstünde dolu transcript. Kabul: kalıntı yok **ve** bloğun üstündeki transcript satırları silinmemiş (metin kaybı yasağı).
5. Her senaryo `Reflow` ve `NoReflow` ile ayrı ayrı koşar.

### Düzeltme

Düzeltmenin şekli **bu tasarımda belirlenmez** — koşum kırmızıya döndükten sonra, testleri yeşile çeviren en küçük değişiklik olarak türetilir. Bağlayıcı olan tek şey ölçüttür: beş senaryo × iki politika, hepsi yeşil, ve K3/K4 ihlal edilmeden.

Mevcut teşhisin işaret ettiği yön (`resize`'ın hem silip hem `forget_block` çağırması; kırpmanın `painted*2`'de tavan yapması) implementasyona **hipotez** olarak verilir, reçete olarak değil.

## Test

- Model kendi kendini test eder: `apply` ile yazılan metin ızgarada doğru yerde · `Clear(CurrentLine)` yalnız o satırı siler · `Clear(FromCursorDown)` imleçten aşağısını siler · `MoveUp`/`MoveDown` sınırda durur · son satırda `LF` kaydırır · bilinmeyen kaçış panik eder (M2).
- `Reflow`/`NoReflow` davranışı ayrı test edilir (bir satır dar genişlikte iki satıra bölünür / bölünmez).
- Beş senaryo × iki politika (M5'e göre önce kırmızı görülür).
- Mevcut bayt-seviyesi testler KALIR — ucuz ve K3'ü koruyorlar; ekran testleri onların yerine değil üstüne gelir.
- Elle doğrulama (Anil): gerçek terminalde taze oturum + sürükleyerek boyutlandırma. Artık ölçütün doğrulaması, keşfi değil.

## Kapsam dışı

- Genel amaçlı VT100 emülatörü veya `vt100` bağımlılığı
- Gerçek pty açan entegrasyon testi (model daha kesin ve daha hızlı; pty reflow'u zaten yapmaz — reflow terminal emülatöründe olur)
- Scrollback modellemesi
- Girdi çerçevesinin görüntüsü (v0.30.0'da yerleşti, Anil onayladı)
