# Gözcü Sıra Alma (Turn-Taking) — Tasarım

> Karar: 2026-08-28. Canlı bir başlangıç-seviye oturumundan sonra, Fable'ın
> bağımsız tasarım incelemesiyle. Hedef sürüm: v0.28.0.
> Çelişki durumunda bu spec kazanır.

## Gerekçe

Canlı oturum, kullanıcı **tek kelime yazmadan**:

1. Usta ödev verdi: "`cargo init --name stagit` çalıştır · `cargo run` çalıştır ·
   `src/main.rs`'i aç oku · sonra bana ne gördüğünü söyle." → top kullanıcıda.
2. `cargo init` → `Cargo.toml` + `src/main.rs` → gözcü tetiklendi → Usta tur açtı.
   `main.rs`'in içeriğini **kendi bastı** — kullanıcının okuyup anlatması gereken
   şeydi — ve soruyu beş alt soruya çevirip yeniden sordu.
3. `cargo run` → `Cargo.lock` → gözcü yine tetiklendi → Usta yine tur açtı,
   *"sorularım hâlâ duruyor"* diyerek aynı beş soruyu üçüncü kez yazdı.

Ekranda aynı soru üç kez, her seferinde daha uzun. Mentör kendi ödevinin cevabını
kısmen verdi. Beklediğinin farkındaydı, yine de konuştu.

**Bu bir ayar hatası değil, tasarlanmış davranış.** `83813f0` ("retire the polite
queue") sonrası sistemde zamanlama kontrolü YOK: `polite::route` üç kollu ve
`Route::Feedback` "şimdi LLM turu" demek (`polite.rs:37`). Modül dokümanı bunu
açıkça yazıyor: *"polite is a prompt-frame switch, not a delay."* Kayıt ile tur
arasında duran tek şey bir prompt paragrafı (`flow_frame`, `file_feedback.rs:128`).

Ve o paragraf olayı engelleyemedi değil — **üretti.** Kuralları "adımı onayla ve
ilerlet" ve "cevaplanmamış soruyu canlı tut" diyor; ikisi de KONUŞMA emri. Model
itaat etti. Turu açma kararı Rust'ta, `run.rs:588`'de, model hiçbir şey görmeden
alınmıştı. Prompt çerçevesi turun TONUNU belirler; turu REDDEDEMEZ.

**Tetikleyici yanlış şeyi ölçüyor.** "Dosya değişti" araç etkinliğini ölçüyor,
öğrenci niyetini değil. Ateşleyen dosyalar mentörün kendi verdiği ödevin yan
ürünleriydi: ödevi yapmak, ödevi teslim etmek sayıldı.

**İki başarısız tur:** v0.24.0 sezgisel kapı denedi (`?` + 180 sn backstop) —
emekli edildi. v0.25.0 prompt disiplini denedi — ilk gerçek başlangıç-seviye
oturumunda çöktü. Kalan hipotez uzayı yine sezgisel; sürekli çöken şey de bu.

**v0.24.0'ın kuyruğu bu oturumu yakalardı.** Emeklilik gerekçesi (kuyruk adımı
mentörden saklıyor, eş-ilerleme ölüyor) yarı doğruydu: **bilgiyi** tutmak hataydı,
ama tedavi **turu** tutmayı da beraberinde götürdü. Bu ikisi ayrılabilir — tasarımın
tamamı bu ayrımdır.

## İsimlendirme (bağlayıcı — tüm yeni kod İngilizce)

- `PendingChanges` — biriken, henüz teslim edilmemiş dosya yolları
- `Route::Hold` — biriktir, tur açma
- `pending_preamble` — ride-along payload'ının başına konan tek satırlık çerçeve
- Durum satırı metni: `changes noted` (sayı ile: `👁 watching · 2 changes noted`);
  live modda bugünkü `👁 watching·live ` biçimi korunur, sayaç gösterilmez
- `WatchCmd::Live{Toggle,On,Off}` — `/watch polite` varyantlarının yerine

Türkçe yalnız bu spec'in düzyazısında; kod, kullanıcıya dönük string'ler, yorumlar
ve commit mesajları İngilizce.

## Kararlar

### K1 — Gözcü hiçbir koşulda tur açmaz

İstisna yok. `exercises/` dahil (Anil kararı, 2026-08-28: "istisna da olmasın").
Bunun sonucu `TEACHING.md:85`'in vaadini değiştirmektir — aşağıda K5.

### K2 — Değişiklikler kullanıcının bir sonraki turuna binerek gider (ride-along)

Debounce flush olduğunda batch `PendingChanges`'e girer (yalnız yollar; payload
teslim anında `build_batch_payload` ile üretilir, böylece ara kayıtlar `FileMemory`
üzerinden tek diff'e çöker). Kullanıcı Submit ettiğinde birikmiş dosya bloğu
kullanıcının metniyle **aynı** LLM çağrısına eklenir.

Bu, iki başarısızlığı tek hamlede çözer: mentör adımı kullanıcının cevabıyla
BİRLİKTE görür (v0.24.0'ın şikayeti), ve tur açmaz (v0.25.0'ın şikayeti). Eski
kuyruk teslimi AYRI bir tur olarak sıraladığı için başarısızdı.

**Maliyet modeli:** token tüketimi dosya sistemi etkinliğiyle değil, kullanıcı
turlarıyla orantılı hale gelir. Kullanıcının kontrol ettiği tek değişken budur.

### K3 — Varlık deterministiktir, üretilmiş değil

Durum satırı biriken sayıyı gösterir: `👁 watching · 2 changes noted`. Sayaç
teslimde sıfırlanır. Sıfır token, sıfır kesinti, mentörün gözü görünür şekilde işte.

**Yasak:** ambient panelin içinde LLM üretimi metin. O, panel kostümü giymiş bir
konuşma turudur — aynı maliyet, aynı hız hırsızlığı, üstelik göz ardı edilebilir
olduğu için daha az değer.

### K4 — Anlık mod korunur ama kullanıcının açık kararına bağlanır

(Anil kararı, 2026-08-28: *"bu anlık modu kullanıcı açıp kapatabilmeli, komple
silmeye gerek yok"* — bu maddenin ilk hâli silmeyi öneriyordu, geçersiz.)

**Varsayılan biriktirmedir.** Anlık geri bildirim yalnız kullanıcı açıkça isterse
çalışır — asla shell çıkarımıyla değil. Bu, pair programming'in tek devralınmaya
değer parçasıdır: gerçek eşler rolü **söyleyerek** devreder, sezerek değil.

**İki bayrak tek eksene iner.** Bugün `watching` (izliyor mu) ve `polite` (hangi
prompt çerçevesi) ayrı; `polite` zamanlama değil yalnız çerçeve seçiyor. Yeni eksen:

| Mod | Zamanlama | Çerçeve |
|---|---|---|
| **companion** (varsayılan) | biriktir, kullanıcı turunda teslim et | `flow_frame` (ders akışı) |
| **live** (açık talep) | flush anında hemen tur | `feedback_frame` (düz inceleme) |

`watching == false` her iki modda da bugünkü gibi: yalnız baseline sync, hiçbir şey
birikmez.

**Komut yüzeyi.** Bugün `/watch polite [on|off]` var (`slash.rs:29-31`). Yerine
`/watch live [on|off]` gelir — aynı ayrıştırma deseni, ters kutup ve dürüst isim:
`polite` bir çerçeve adıydı, kullanıcı ise zamanlama seçiyor. Oturum içi, kalıcı
değil. Kalıcı seçim approach dosyasındaki `watch: live` satırıdır (`polite.rs:52`,
`live_from_approach` — zaten var, artık çerçeveyi değil zamanlamayı seçiyor).
`/watch on|off` değişmez. `help.rs:20` ve `:67` güncellenir.

**Neden korunuyor:** K1 "gözcü kendiliğinden karar verip konuşmaz" der; kullanıcının
açıkça istediği geri bildirim kendiliğinden değildir. Uzman rolü (13b'de `role_of`
geldiğinde) `watch: live`'ı varsayılan olarak açabilir — kod yolu değil, varsayılan
üretimi: tek satır, run loop'ta yeni dal yok.

### K5 — Prompt tarafı: üç değişiklik

1. **Eyes-only kuralı.** Ödevi "şunu oku/çalıştır/anlat" ise, o artefaktın
   payload'daki içeriği kullanıcı rapor edene kadar modelin gözlerine mahsustur:
   alıntılanamaz, özetlenemez, açıklanamaz. Kullanıcının raporu ona karşı
   doğrulanır. Emsal aynı dosyada var: `cargo check` bloğu `FOR YOUR EYES ONLY`
   etiketli (`file_feedback.rs:197`), ve TEACHING.md'nin tahmin protokolü hatayı
   göstermeden önce kullanıcıya tahmin ettiriyor.
2. **Tekrar yasağı.** Bekleyen soru tam metin tekrar edilmez; tek cümlelik bir
   geri gönderme ile hatırlatılır.
3. **Vaat düzeltmesi.** `TEACHING.md:85` "The user writes the file; saving it
   triggers your review automatically" → kaydetmek tetiklemez; kullanıcı bitirince
   söyler, değerlendirme o turda gelir.

Kural 1 ve 2 birincil savunma DEĞİL — K1 zaten sızıntı fırsatını ortadan kaldırıyor.
Bunlar ride-along payload'ı için yedek. (v0.25.0'ın tekrar-yasağı kuralı tek başına
kaybetti: model soruyu yine üç kez sordu.)

### K6 — `Cargo.lock` yok sayılanlara girer

`watcher.rs`'in `is_ignored` filtresi `target/`, `node_modules/` ve nokta-dizinleri
eliyor; `Cargo.lock` kökte ve gizli değil. Makine yazımı bir dosya LLM turu satın
aldı. Filtre satırı her tasarım altında doğrudur.

## Davranış (bağlayıcı detay)

### Akış

1. Watcher debounce flush → `route(batch_len, max_batch, watching)`:
   - `Bulk` (batch > max): mevcut davranış — baseline sync + notis, değişmez.
   - `ObserveOnly` (watching kapalı): mevcut davranış — baseline sync, değişmez.
   - `Hold` (yeni, eski `Feedback`'in yerine): yollar `PendingChanges`'e eklenir
     (sıra korunur, tekrar edenler tekilleşir). Ekranda tur YOK. Durum satırı sayacı
     güncellenir.
2. Kullanıcı Submit eder → `PendingChanges` boş değilse: `build_batch_payload` ile
   payload üretilir, `pending_preamble` ile çerçevelenir ve kullanıcının metnine
   EKLENİR (tek mesaj, tek çağrı). `PendingChanges` boşaltılır, sayaç sıfırlanır.
3. `PendingChanges` boşsa: bugünkü Submit yolu, değişiklik yok.

### Sıralama ve içerik

- Dosya bloğu kullanıcının metninden ÖNCE gelir; kullanıcının sözü son sözdür.
- Payload teslim anında üretilir (flush anında değil) — ara kayıtlar tek diff'e
  çöker, silinen dosya teslim anında zaten yok sayılır.
- `build_batch_payload`'ın mevcut notis kanalı korunur (büyük dosya, ikili dosya):
  notisler teslim anında basılır.
- `total_included == 0` ise (her şey düştü) payload eklenmez, sayaç sıfırlanır.

### Kenar durumlar

- **Oturum kapanışı `PendingChanges` doluyken:** teslim edilmez, kaybolur. Kapanış
  flush'ı zaten diskteki güncel hâli okuyor; ayrı bir teslim turu açmak K1'i ihlal
  ederdi.
- **`/quit` bekleyen değişiklikle:** aynı — sessizce düşer.
- **Bulk skip:** batch tavanı aşarsa bugünkü gibi atlanır ve baseline senkronlanır;
  `PendingChanges`'e girmez (aksi hâlde tavan anlamsızlaşır).
- **Watching kapalıyken:** hiçbir şey birikmez; sayaç görünmez.

## Kapsam dışı (bilinçli)

- **`cargo check` tetiği.** Şekli doğru (nesnel, nadir, ödev yan ürünlerinde
  ateşlemez — `cargo init` temiz derlenir, `Cargo.lock` derlenmez) ama iki maliyeti
  var: `run_check` soğukta 60 sn'ye kadar sürüyor ve sessiz birikim sırasında
  koşacaktı; ayrıca derleme hatası tam olarak öğrencinin debelenmesi gereken an.
  Kapı açık: ileride durum satırında dim bir `✗ check failing` işareti olabilir —
  tur değil.
- **`/check` komutu** (mesaj yazmadan teslim tetikleme). "Kullanıcı turu"nun şekeri;
  ride-along yetersiz kalırsa eklenir. YAGNI.
- **Takılma/sessizlik sezgisi.** Reddedildi: "beş dakikadır `main.rs` okuyorum"
  (düşünüyor, kesilmemeli) ile "takıldım" aynı sinyali üretir. 180 sn backstop tam
  olarak buydu ve haklı olarak silindi.
- **VS Code sekme değişiminde viewport artığı.** Ayrı hata, ayrı iş: önce ölçüm
  (hangi olay dizisi geliyor), sonra düzeltme. Tahminle dokunulmayacak.
- **Rolün modu otomatik seçmesi.** 13a `role:` satırını yazıyor ama shell parse
  etmiyor; 13b `role_of`'u ekleyecek. O gelene kadar `live` yalnız kullanıcının açık
  kararıyla açılır (`/watch live` veya approach'ta `watch: live`). 13b'de rol yalnız
  VARSAYILANI üretebilir — run loop'ta yeni dal değil.

## Kısıtlar

- `src/tui/run.rs` **598/600 satır** — belgeli istisna, BÜYÜTÜLEMEZ. Submit kolu ve
  deadline kolu değişiyor; eklenen satır kadarı yerinden edilmeli, yoksa mantık
  `polite.rs`'e taşınır.
- `src/plain.rs` değişmez. Plain yolunda watcher yok.
- Prompt diet: biriktirme, tekilleştirme, sayaç ve teslim kararı tamamen shell'de;
  modele yalnız payload ve çerçeve gider.
- Closing flush sözleşmesi (altı dosya) değişmez.

## Test

- `route` artık `Hold` döndürüyor: bulk ve observe-only kolları korunuyor.
- `PendingChanges`: sıra korunur, tekrar tekilleşir, teslimde boşalır.
- Ride-along: bekleyen değişiklikle Submit → tek turda hem payload hem kullanıcı
  metni; payload kullanıcı metninden önce.
- Boş bekleyen küme → Submit yolu değişmez.
- `total_included == 0` → payload eklenmez, sayaç sıfırlanır.
- `is_ignored("Cargo.lock")` true; `is_ignored("src/main.rs")` false.
- Durum satırı: sayaç görünür ve teslimde sıfırlanır.
- Anlık mod: `live` açıkken flush anında tur açılır ve `feedback_frame` seçilir;
  kapalıyken `Hold` + `flow_frame`. `/watch live [on|off]` ayrıştırması (`/watch polite`
  varyantlarının yerine), `live_from_approach` artık zamanlama seçiyor. `/watch on|off`
  davranışı değişmedi. Wiring pin testleri güncellenir (`run_rs_wiring_call_sites_are_pinned`,
  `polite_branch_selecting_flow_frame_is_pinned` — bu testler watcher kablolamasının iki
  kez sessizce silinmesi yüzünden var, bu değişiklikte dürüst tutulmalı).
- `help.rs` çıktısı `/watch live` satırını içerir, `/watch polite` satırını içermez.
- Prompt pin: eyes-only kuralı ve tekrar yasağı `flow_frame`/TEACHING.md'de mevcut.

## İlgili

- `docs/ROADMAP.md` v0.24.0 girdisi hâlâ emekli edilmiş kuyruğu yürürlükteymiş gibi
  anlatıyor — bu değişiklikte düzeltilecek (hangi seçenek seçilirse seçilsin bayat).
- `polite.rs` modül dokümanı ("not a delay") tersine dönüyor.
- `docs/superpowers/specs/2026-08-27-flow-companion-design.md` — v0.25.0'ın gerekçesi.
- `docs/superpowers/specs/2026-08-28-entry-flow-rewrite-design.md` — 13a, rol alanı.
