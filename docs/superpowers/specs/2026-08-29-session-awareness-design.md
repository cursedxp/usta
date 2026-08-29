# Oturum Farkındalığı — Kimlik, Durum Hafızası, Yapı Sinyali, Denetim, Bağlam Görünürlüğü — Tasarım

> Karar: 2026-08-29. Kaynak: v0.28.0 canlı başlangıç-seviye oturumunun bulguları
> (`.superpowers/sdd/progress.md`, "CANLI KULLANIM BULGULARI" bölümü — BULGU A/C/D/E
> + kullanıcının bağlam-görünürlük talebi). Hedef sürüm: v0.29.0.
> Çelişki durumunda bu spec kazanır. v0.28.0'ın K1–K6 kararları
> (`2026-08-28-watcher-turn-taking-design.md`) aynen bağlayıcıdır — bu tur hiçbirini
> geri açmaz; K2'nin bir SONUCU düzeltilir (aşağıda C), kendisi değil.

## Gerekçe

v0.28.0 doğru çalıştı: gözcü tur açmıyor, değişiklikler kullanıcının mesajına binerek
gidiyor, canlı oturum bunu doğruladı. Ama aynı oturum, ride-along mimarisinin üstünü
örttüğü daha derin bir deseni açığa çıkardı:

**Usta'nın dünya modeli yalnız dosya-İÇERİĞİ kaydıyla tazeleniyor.** Kayıt yoksa model
donuyor. Bu tek kökten dört ayrı bulgu doğdu:

1. **(BULGU C)** Derleme durumu kenar-tetiklemeli: `cargo check` yalnız `PendingChanges`
   dolu olduğunda, teslim anında koşuyor (K2'nin doğrudan sonucu). Transcript kanıtı
   (`rust-20260828-224331.jsonl`): tur 11 ve 13 check bloğu + `error[E0308]` taşıdı;
   tur 14 ve 15'te kullanıcı kaydetmeden konuştuğu için payload yok, check yok — Usta'nın
   derleme bilgisi 13'te dondu ve **derlenmeyen projede "bu parça bitti, sıradaki görev"
   dedi.** "Şu an durum ne" diye soran hiçbir şey yok.
2. **(BULGU D)** Yapısal değişiklikler görünmez: Usta "brands/marka-a ve marka-b
   klasörlerini aç" ödevini verdi, sonra aynı mesajda İKİ kez "o klasörleri açtın mı?"
   diye SORMAK zorunda kaldı. `watcher::should_forward` dizinleri eliyor, boş dizin
   dosya olayı üretmiyor; silinen dosyanın olayı gelir ama `NotFound` okuma
   `is_silent_skip` ile yutulur; yeniden adlandırmada yeni ad görünür, eski adın
   kaybolduğu görünmez. Üçü de meşru "ödevi yaptım" sinyali. Anil kararı (2026-08-28):
   takip edilecek.
3. **(BULGU E)** Teslim edilen değişiklik ödeve karşı DENETLENMİYOR: mekanizma diff'i
   zaten teslim ediyor (16 turun 7'sinde FILE: bloğu), ama `flow_frame`'de "gelen
   değişikliği açık ödeve karşı tut" kuralı yok. Model bazen yapıyor — kural değil
   tesadüf. Üstelik v0.28.0'ın kendi düzeltmeleri bunu BASTIRIYOR olabilir: rule 5
   (OFF-LIMITS) geniş okunursa "yazması istenen dosyayı da değerlendirme" olur ve
   rule 1'in "only acknowledge that the step happened" eki aynı yöne çekiyor.
4. **(BULGU A)** Tanışma kim olduğunu sormuyor: `introduction_prompt` üç konuşma
   kuralını ve rol çıkarımını taşıyor ama isim/arka plan/öğrenme tarzı sorusu YOK.
   Eski `MEET_BLOCK` (`progress.rs:48`) bunu soruyordu, artık yalnız plain yolunda ve
   `usta start <topic>`'te koşuyor. `USER.md`'nin "Who" bölümü boş kalıyor, sonraki
   oturumlar kullanıcıyı tanımıyor — 13a'nın kendi gerekçesi "tanışma profili doldurur"du,
   yarısı düştü. Bu, 13a spec'inin vaadi karşısında bir regresyondur.

Beşinci iş bulgu değil talep: **(F)** kullanıcı bağlam penceresini neyin doldurduğunu
görmek istiyor (Claude Code'un `/context`'i gibi). Token-HARCAMA takibi bu turdan
açıkça çıkarıldı (kullanıcı kararı); istenen doluluk dökümü.

**Genel ilke (bu spec'in çerçevesi):** içerik ride-along'u zaten çalışıyor — doğrulayıcısı
olmayan alanlar (yazı, terminal işi, tüm yazılım-dışı domainler) için dosya içeriği
durum sinyalinin TAMAMIDIR ve bu yeterli. Bu turda tamamlanan iki eksik yarı:
**yapı** sinyali (D — her domain'de geçerli) ve **doğrulama** sinyali (C — yalnız
projenin kendi doğrulayıcısı VARSA; bugün tek örnek Cargo). Doğrulama kanalı genel
ilkenin Cargo-şekilli özel durumudur, ilkenin kendisi değildir — kod da bunu böyle
modellemelidir.

## İsimlendirme (bağlayıcı — tüm yeni kod İngilizce)

- `watcher::StructureTracker` — oturum başında ağaçtan tohumlanan dizin envanteri;
  `seed(root)` · `note_new_dir(path) -> bool` · `note_removed(path) -> bool`
- `watcher::should_forward(path, kind)` — imza büyür: olay türü artık parametre
- `feedback::FileMemory::knows(path) -> bool` — yol daha önce görüldü mü
- `polite::classify_flush(batch, tracker, files, project_root) -> (Vec<PathBuf>, Vec<String>)`
  — flush anında içerik/yapı ayrımı
- Yapı notu biçimleri: `+ <rel>/ (new directory)` · `- <rel> (no longer present)`
  (dosya) · `- <rel>/ (no longer present)` (dizin) — iki silinme biçimi de
  kasıtlı olarak AYNI kelimeyi taşır: bu katmanda bir yeniden adlandırma bir
  silmeden ayırt edilemez, "deleted"/"removed" demek bilmediği bir kesinliği
  iddia etmek olurdu; taşma satırı `… and N more structural changes`; payload
  bloğu başlığı `STRUCTURE: project tree changes`
- `polite::MAX_STRUCTURE_NOTES = 20`
- `check::Verdict::{Pass, Fail}` · `check::verdict_of(raw)` · `check::error_summary(raw)`
- `check::VerifyMonitor` — projenin doğrulama sinyalinin kabuk hafızası;
  `new(project_root)` (etkinlik = `is_cargo_project`) · `record(raw)` ·
  `is_failing()` · `note() -> Option<String>`
- Durum satırı işareti: `✗ last check failed ` (dim; yalnız watching açıkken)
- Durum notu satırı: `[build state: the last cargo check that ran was failing — first error: …]`
- `polite::handle_watch_command(cmd, watching, live, pending) -> Vec<String>` —
  run.rs'ten yerinden edilen /watch kolu
- `/context` · `slash::is_context_command` · modül `src/context_report.rs` ·
  `context_report::build(system, history, last_reported, window) -> String`
- `brain::section_sizes(system) -> Vec<(String, usize)>` — assembly'nin
  `===== label =====` biçiminin tersine çevrimi

Türkçe yalnız bu spec'in düzyazısında; kod, kullanıcıya dönük string'ler, yorumlar
ve commit mesajları İngilizce.

## Kararlar

### A1 — Tanışma kişiyi de tanır

`introduction_prompt`'a kimlik bloğu eklenir: isim, bu alana değen geçmiş, öğrenme
tarzı — **form olarak değil, konuşmaya dokunarak.** Üç kural aynen yürürlükte kalır;
kimlik soruları kural 1'i zaten geçer (her cevap kalibrasyonu değiştirir — `MEET_BLOCK`
emsali). Prompt, cevapların kapanışta profilin "Who" bölümüne gideceğini ve
sorulmazsa her sonraki oturumun bir yabancıyla açılacağını söyler.

`MEET_BLOCK` DEĞİŞMEZ: plain yolu ve `usta start <topic>` onu kullanmaya devam eder.
Bu karar yalnız TUI ilk-çalıştırma tanışmasının eksiğini kapatır.

### C1 — Doğrulama sinyali seviye-tetiklemelidir, kenar-tetiklemeli değil; ve nötr bir dikiştir

Kavram "**projenin kendi doğrulama sinyali, proje birine sahipse**" — "cargo check"
değil. Kod tabanı bu dikişi zaten doğru modelliyor: `check::run_check` yalnız
`is_cargo_project` doğruysa koşar, `PREDICTION.md` aynı koşulla yüklenir (`brain.rs`).
`VerifyMonitor` bu dikişi izler: `new(project_root)` etkinliği `is_cargo_project` ile
belirler.

**Doğrulayıcısı olmayan projede (her Cargo-dışı proje, her yazılım-dışı domain)
özelliğin tamamı sessiz no-op'tur:** durum satırı işareti yok, teslim turunda durum
satırı yok, flush'ta ek iş yok, bugüne göre sıfır davranış farkı. Bu bir test kilidi
alır.

**YAGNI:** doğrulayıcı kaydı, konfigürasyon yüzeyi, dil-başına algılama YOK. Bugün
tek implementasyon var (Cargo check). İkinci doğrulayıcı ileride yeni bir kol olarak
eklenir, yeniden tasarım olarak değil — iç adlar bu yüzden nötrdür (`VerifyMonitor`,
`Verdict`), kullanıcıya/modele dönük metin ise dürüstçe somuttur ("cargo check",
çünkü işaret yalnız Cargo projesinde görünür).

### C2 — Kararname HATIRLANIR; check daha sık koşmaz

Bulgu C'nin asgari düzeltmesi ledger'ın kendi önerisiyle aynıdır: *"hafıza yeter,
ek maliyet sıfır."* Check zaten koşuyor ve zaten gerçeği üretiyor; hata, kararnamenin
teslim biter bitmez unutulması. Bu yüzden:

- **Check'in ne zaman koştuğu DEĞİŞMEZ:** bugünkü üç yer aynen — ride-along teslimi
  (batch'te ≥1 non-exercise dosya, `deliver_pending` içinde inline), live batch turu
  (`handle_batch_change`), plain tek-dosya yolu. Yeni koşum eklenmez, kullanıcı
  turlarında check KOŞULMAZ, flush'ta arkaplan check KOŞULMAZ.
- **Kararname kabukta yaşar:** bir check gerçekten koştuğunda sonucu
  `monitor.record(raw)` ile saklanır (`Verdict::Pass` / `Verdict::Fail{summary}` —
  özet ilk hata satırı, kırpılmış). Taze eyes-only blok bugünkü gibi o turla gider
  (`check_result_block` tek kaynak, tahmin protokolü aynen).
- **Kırmızıyken, o turda taze bir check koşmayan her tur tek satır taşır:**
  `monitor.note()` — `[build state: the last cargo check that ran was failing —
  first error: <özet>. Nothing has re-verified the project since; do not treat
  the current step as complete until a later check comes back clean.]` Not,
  yalnız TAZE bir check'in koşmadığı turlara gider — payload'lı ama check'siz
  turlar (exercise-only, yapı-only, `run_check` → None) VE payload'sız turlar;
  bir check GERÇEKTEN koşan teslim bu notu DEĞİL, taze eyes-only bloğu taşır —
  ikisi karşılıklı dışlayıcıdır, "her teslim edilen tur" değil. Not kasıtlı
  olarak son ÇALIŞAN check'e atıfta bulunur, o turda teslim edilen değişikliğe
  değil — o değişiğin derleme durumu tanım gereği bilinmiyor (bu notun koştuğu
  her durumda check bu turda koşmadı). Bu, tur 14-15 sınıfını kapatır: Usta
  kırmızı projede "bitti" diyemez, çünkü kabuk ona hatırlatır. Talimat notun
  İÇİNDE taşınır — `flow_frame`'e yeni kural gerekmez (prompt diyeti).
- **Durum satırı:** kırmızıyken dim `✗ last check failed ` işareti (`theme::info()`
  dim stili — "navigator's raised eyebrow", tur değil; geçmiş zaman kasıtlı —
  bu son check'in kararnamesidir, "şu an derlenmiyor" iddiası değil). Yalnız
  watching açıkken görünür.
- **Kararname yalnız bir SONRAKİ gerçek check ile değişir** — yani non-exercise
  dosya içeren bir sonraki teslimle. **Dürüstçe kabul edilen bayatlık penceresi:**
  kullanıcı kodu düzeltip kaydettikten sonra, o kaydı taşıyan mesajına kadar önbellek
  kırmızı kalır — ama o mesaj geldiğinde teslim check'i koşar ve pencere kapanır.
  Bugünkü amnezinin tersine bu pencere tek turluk ve kendini kapatandır.
- **Oturum başı:** kararname yok (`None`) — işaret ve not yok; ilk sinyal ilk
  check'li teslimle gelir. Kırık projede açılan oturum ilk kayda kadar bilinmez —
  v0.28.0 ile aynı, gerileme değil.

**Değerlendirilip REDDEDİLEN alternatif — flush anında arkaplan check:** teslim
gecikmesini (soğuk cache'te 60 sn'ye kadar) yok ederdi, ama karşılığı bir tokio
görevi + kanal + select kolu + jenerasyon/coalescing durum makinesi + DAHA FAZLA
derleme koşumu olurdu ve v0.28.0'ın "sessiz birikim sırasında derleme olmaz" kararını
tersine çevirirdi. Mevcut inline gecikme canlı oturumda doğrulanmış, şikayet
üretmemiş davranıştır; yalnız soğuk-başlangıç gecikmesini satın alan bu karmaşıklık
YAGNI'dir. Kapı açık: gecikme gerçek kullanımda sorun çıkarırsa arkaplan koşum
`VerifyMonitor`'un arkasına yeni bir kol olarak eklenebilir — dikiş oradadır.

**K1 ihlal edilmez:** monitör hiçbir koşulda tur açmaz; ürettiği her şey ya durum
satırı pikselidir ya da zaten açılan bir turun içine iliştirilen deterministik metin.

### D1 — Yapı olayları izlenir; içerik değil, yalnız YAPI satırı gider

Üç sinyal aynı kanaldan: **yeni dizin** (boş olsa da), **silinen dosya/dizin**,
**yeniden adlandırma** (yeni ad içerik olarak + eski ad silinme satırı olarak görünür —
korelasyon YOK, kabul edilen şekil). Mekanik:

- `watcher::spawn` olay filtresi genişler: `Modify(_) | Create(_) | Remove(_)`.
  `should_forward(path, kind)`: yok sayılanlar elenir; **yaşayan dizin** yalnız
  `Create`/`Remove` türlerinde iletilir (dizinde `Modify` = içerik gürültüsü);
  dosyalar ve artık var olmayan yollar her türde iletilir. Böylece `touch` ile
  açılan boş DOSYA da görünür olur (bugün Create-only dosya görünmezdi) — o içerik
  yolundan gider (boş full-contents bloğu; dürüst).
- Sınıflandırma **flush anında**, kabukta, deterministik (`classify_flush`): var +
  dizin → tracker bilmiyorsa `+ <rel>/ (new directory)`; var + dosya → içerik yolu
  (mevcut davranış); yok + tracker'ın bildiği dizin → `- <rel>/ (no longer present)`;
  yok + `FileMemory`'nin bildiği dosya → `- <rel> (no longer present)`; yok + hiç
  bilinmeyen → sessiz (transient temp — bugünkü davranış korunur).
- `StructureTracker` oturum başında proje ağacından tohumlanır (yok sayılan dizinler
  atlanır) — var olan bir dizine gelen olay asla "yeni dizin" sayılmaz.
- **İçerik gitmez:** dizin içeriklerini göndermeme kararı (v0.24 kökenli) AYNEN
  korunur. Yanlış olan dizin OLAYINI tamamen atmaktı; satır yalnız yol + tür taşır.
- Silinen dosyanın `FileMemory` baseline'ı DÜŞÜRÜLMEZ: dosya geri gelirse diff
  silme-öncesi içeriğe karşı üretilir — geri dönüşü görünür kılar (bilinçli).

### D2 — Yapı notları tur açmaz; PendingChanges'te birikir ve ride-along ile gider

- Notlar `PendingChanges`'e girer (`hold_notes`: birebir tekrar tekilleşir, tavan
  `MAX_STRUCTURE_NOTES = 20`, taşanlar sayılır ve `take` anında tek `… and N more
  structural changes` satırına çöker). Sayaç (`len`) yollar + notlar + bastırılanları
  birlikte sayar — `changes noted` artık yapıyı da kapsar (K3 deterministik varlık).
- Teslimde notlar payload'ın İLK bloğu olur (`STRUCTURE: project tree changes`
  başlığı altında), `flow_frame` hepsini sarar. İçerik dosyası olmayan, yalnız-yapı
  teslimi de aynı çerçeveyle gider (E1'in denetim kuralı yapı satırını kanıt olarak
  kullanır: "klasörleri açtın mı?" sorusu ölür).
- **Mixed-batch olasılık ipucu (bilinçli, sınırlı — bu turda eklendi):** aynı
  teslimde en az bir kayboluş notu (`- ...`) VE en az bir belirişi notu (`+ ...`)
  varsa, not listesinin sonuna tek satır eklenir: `"(A disappearance and an
  appearance in this batch may be two halves of one move or rename.)"` Bu
  KORELASYON DEĞİL — hangi kayboluşun hangi belirişle eşleştiği bilinmez,
  söylenmez, hiçbir kod ikisini eşleştirmez; yalnız olasılığı adlandıran bir
  ipucudur. Kapsamı görünenden dar: `+` notu yalnız `classify_flush`'ın YENİ
  DİZİN sınıfından gelir — bir dosyanın belirişi İÇERİK yolundan gider (tam
  içerik bloğu) ve `+` notu üretmez. Pratikte bu yüzden ipucu yalnızca DİZİN
  yeniden adlandırmasında tetiklenir; dosya yeniden adlandırması (silme notu +
  yeni dosyanın içerik bloğu) hiçbir zaman tetiklemez — kapsam dışı bölümündeki
  "rename korelasyonu yok" kararıyla çelişmez (bkz. Kapsam dışı).
- **Her iki modda da** (companion VE live) notlar bir sonraki kullanıcı turuna biner —
  live'ın anlık turu içerik içindir; `mkdir` live modda bile tur SATIN ALMAZ (K1
  ruhu, kabul edilen sadeleştirme).
- `/watch off` bekleyen notları da düşürür (v0.28.0 kuralının doğal uzantısı; düşürme
  notisi sayıya notları da katar). Kapanışta teslim edilmemiş notlar kaybolur — K2
  kenar kararıyla aynı.
- Watching kapalıyken notlar BİRİKMEZ ama `classify_flush` tracker'ı yine günceller —
  watching yeniden açıldığında eski dizinler "yeni" diye raporlanmaz.
- Bulk batch'te içerik atlanır (mevcut davranış) ama yapı notları tutulur — bir
  branch geçişinin kütle silmeleri tavana çarpar ve tek taşma satırına çöker.

### E1 — flow_frame: OKU/YAZ asimetrisi + üç parçalı denetim şekli

İki kural yeniden yazılır:

- **Kural 1 (denetim şekli):** son mesaj kullanıcıdan bir şey YAZMASINI/DEĞİŞTİRMESİNİ
  istediyse ve gelen değişiklik o teslimse: değişikliği ödeve karşı DENETLE ve cevabı
  üç parçayla kur — (1) NE YAPILMIŞ, değişiklikten kanıtla; (2) NE EKSİK/YANLIŞ,
  açıkça; (3) TEK sonraki adım. Kullanıcının sözlerine cevap vermek yeterli değil —
  gelen diff ödeve karşı tutulur. Eksik SÖYLENİR, çözüm YAZILMAZ (Hard Rule 2 aynen).
- **Kural 5 (asimetri açık yazılır):** artefaktın AMACI ne söylenebileceğini belirler.
  OKUMASI/ÇALIŞTIRMASI/ANLATMASI istenen artefakt → kullanıcı rapor edene kadar
  OFF-LIMITS: alıntılanmaz, özetlenmez, açıklanmaz; yalnız adımın gerçekleştiği teyit
  edilir; rapor gelince görülene karşı doğrulanır (v0.28.0 K5.1 aynen). YAZMASI/
  DEĞİŞTİRMESİ istenen artefakt → tam tersi: onu görüp yargılamak işin kendisidir —
  kural 1 ile denetlenir. Kural 5, üretmesi istenen iş hakkında susma gerekçesi olarak
  OKUNAMAZ.

Kural 2-3-4 birebir korunur. Bu, v0.28.0'ın aşırı-düzeltme riskini (rule 5'in "yazılan
dosyaya" genellenmesi) prompt düzeyinde kapatır. `is_exercise_path`/hint-ladder
mekaniği değişmez.

### F1 — `/context`: bağlam dökümü, deterministik, oturum içi

- **Yüzey:** `/context` slash komutu (TUI). LLM çağrısı YOK — tamamen kabuk işi,
  `page_notice` ile basılır. `/help`'e satır eklenir.
- **Kaynak — tek kaynaklılık:** sistem prompt'u dökümü `session.system`'in KENDİSİNDEN
  çıkarılır: `brain::section_sizes` assembly'nin kendi `===== label =====` ayırıcı
  biçimini tersine çevirir. Yeniden hesaplama YOK — rapor, modelin her çağrıda
  gerçekten aldığı byte'ları ölçer (oturum ortasında diskte değişen dosya sapması
  dahil dürüst). `load_system_prompt` çıktısıyla roundtrip testi bağlar.
- **Geçmiş dökümü:** `session.history()` dört kovaya sınıflanır (deterministik önek
  eşleşmesi): kullanıcının kendi mesajları · Usta'nın cevapları · dosya teslimleri
  (`PENDING_PREAMBLE`/`[Files changed]`/`[File saved`/`[File changed`/`[Exercise
  submission` önekleri — `file_feedback::is_delivery_turn` tek kaynak) · enjekte
  direktifler (diğer `[`-önekli sentetik turlar).
- **Sayılar:** kesin BYTE raporlanır; token rakamları tahmindir ve öyle etiketlenir
  (`bytes ÷ 4` — "estimate" kelimesi çıktıda geçer). Son çağrının rapor ettiği
  `context_tokens` yanında gösterilir; tahminle arasındaki fark teşhistir (backend
  ek yükü, cache) ve çıktı bunu söyler. Henüz usage yoksa (örn. tanışma —
  `run_intro` `context_tokens` düşürüyor, bilinen M11 durumu) "nothing reported yet"
  denir.
- **Sıralama:** bölümler assembly sırasıyla listelenir (gerçek prompt'un aynası),
  boyuta göre değil.
- Tanışma ve konu-girişi sırasında `/context` "oturum içinde çalışır" notisi alır
  (`/show`/`/watch` kapısıyla aynı desen).

## Davranış (bağlayıcı detay)

### Akış — flush'tan teslime

1. Watcher olayı (Modify/Create/Remove, filtre sonrası) → debouncer (değişmez).
2. Deadline → `dispatch_flush`:
   a. `classify_flush(batch)` → `(content, notes)`; tracker güncellenir (watching'den
      bağımsız).
   b. watching açıksa `notes` → `pending.hold_notes` (kapalıysa düşer).
   c. Rota kolları v0.28.0'daki gibi işler, artık `content` üzerinden (`Hold` →
      `pending.hold(content)`; `Bulk` → `bulk_skip(content)`; `Feedback` →
      `process_batch(content)` — inline check korunur, sonucu `monitor.record` ile
      hatırlanır; `ObserveOnly` → `sync_baseline(content)`).
3. Kullanıcı Submit → `attach_pending(..., &mut monitor, line)`:
   - pending doluysa: `deliver_pending(paths, notes, monitor, user_text)` —
     STRUCTURE bloğu önce, FILE blokları sonra, `flow_frame` sarar; batch'te
     non-exercise dosya varsa check bugünkü gibi inline koşar, sonucu kaydedilir ve
     taze eyes-only blok gider; check koşmadıysa ve son kararname kırmızıysa
     `monitor.note()` satırı gider; kullanıcının sözü SON (K2 sıralaması aynen).
   - pending boş + kararname kırmızı → not + boş satır + kullanıcı metni.
   - pending boş + değilse → metin dokunulmadan geçer (bugünkü yol).

### Kenar durumlar

- **Doğrulayıcısız proje:** `VerifyMonitor` devre dışı — `record` no-op, `note` None,
  `is_failing` false; işaret ve not asla görünmez, davranış bugünle birebir. Test
  kilidi.
- **Check timeout / cargo koşamadı (`run_check` → None):** kararname GÜNCELLENMEZ
  (son bilinen durum korunur); kırmızıysa not o turda da gider — dürüst: son
  BİLİNEN durum budur.
- **Exercise-only teslim + kırmızı durum:** eyes-only blok iliştirilmez ve check
  koşmaz (v0.28.0 kapısı korunur) ama kırmızı not iliştirilir — proje durumu ödev
  incelemesinden bağımsız bir gerçektir.
- **Yalnız-yapı teslimi:** check koşmaz (bugünkü kapı: non-exercise İÇERİK gerekir) —
  kırmızıysa not gider; silinen bir dosyanın build'i kırdığı gerçeği bir sonraki
  içerikli teslimin check'inde görünür. Kabul edilen asgarilik.
- **`total_included == 0` + not yok + kararname kırmızı:** not yine iliştirilir
  (payload'sız tur kuralı — hatırlamak düzeltmenin ta kendisi).
- **Rename:** eski yol `- old (no longer present)` notu + yeni yol içerik yolundan FirstSight.
  Korelasyon yok — kabul edilen şekil.
- **Silinip geri gelen dosya:** baseline korunur → geri geliş diff olarak görünür.
- **`/exam`/`/game` direktifleri:** hiçbir şey binmez (v0.28.0 pin'i aynen — notlar
  da dahil, `attach_pending` yalnız kullanıcının kendi satırını sarar).
- **Plain yol:** watcher kanalı genişlediği için plain'e dizin/silinme yolları da
  düşer; `handle_file_change` bunları bugünkü silent-skip sınıflarıyla (NotFound /
  IsADirectory) sessizce yutar — `src/plain.rs` **KAYNAK olarak** DEĞİŞMEZ (byte-
  byte aynı dosya). Ama davranış aynı kalmaz: `should_forward` artık dosya
  `Create` olaylarını da iletiyor (D1), ve bu olay plain'in paylaştığı watcher
  kanalından geçiyor — önceden yalnız `Modify` iletiliyordu, boş bir dosyanın
  tek başına `Create`'i hiç kanala düşmüyordu. Sonuç: plain yolda BOŞ bir dosya
  oluşturmak artık `handle_file_change`'i FirstSight ile tetikler ve önceden
  hiçbir şey üretmezken şimdi anında bir tur açar — kaynak değişmeden davranış
  değişen somut bir örnek. Plain'de yapı takibi, monitör ve `/context` YOK
  (bilinçli; `/context` plain'de normal metin olarak modele gider — kabul
  edilen boşluk, help satırı TUI bağlamında yazılır).
- **`/context` sırasında bölüm-ayırıcı taklidi:** bir brain dosyasının gövdesinde
  `===== x =====` biçiminde satır varsa döküm o noktada yanlış bölünür — kabul edilen
  teorik boşluk (ayırıcı bizim kendi biçimimiz, brain dosyaları bizim kontrolümüzde).
- **Kapanış:** teslim edilmemiş yollar VE notlar düşer (v0.28.0 kararı genişler);
  closing flush sözleşmesi (altı dosya) değişmez.

## Kapsam dışı (bilinçli)

- **Kümülatif token/maliyet harcama takibi** — kullanıcı bu turdan açıkça çıkardı
  (BULGU B'nin "ne kadar harcadım" yarısı). `/context` doluluk gösterir, harcama değil.
- **VS Code sekme-değişimi viewport artığı** — önce ölçüm turu gerekiyor
  (ledger kaydı); tahminle dokunulmaz.
- **K1'i yeniden açan her şey** — gözcü tur açmaz; monitör, tracker ve `/context`
  hiçbir yolda LLM turu başlatmaz.
- **Doğrulayıcı kaydı / ikinci doğrulayıcı / dil algılama** — YAGNI (C1). Dikiş
  bırakılır, iskele kurulmaz.
- **Flush anında arkaplan check** — C2'de gerekçeli reddedildi (görev/kanal/
  coalescing karmaşıklığı yalnız soğuk-başlangıç gecikmesini satın alır; v0.28.0'ın
  "sessiz birikimde derleme yok" kararı ayakta).
- **Rename korelasyonu** — silme satırı + yeni içerik (dosya) veya silme satırı
  + yeni dizin notu (dizin) birbirinden bağımsız olgular olarak kalır, hiçbir
  kod ikisini eşleştirmez. Tek istisna, D2'de belgelenen mixed-batch ipucudur:
  aynı batch'te ikisi de varsa tek satırlık bir OLASILIK notu eklenir — bu
  eşleştirme mantığı değil, salt bir hatırlatmadır; hangi ikisinin birbirine
  karşılık geldiğini kod hâlâ bilmez.
- **Oturum başı check** — soğuk 60 sn açılışa bindirilmez.
- **Plain yolunda yapı/monitör//context** — plain dokunulmaz sözleşmesi.
- **`next_unseen`/plan sahipliği vb. 13b işleri** — kendi döngülerinde.

## Kısıtlar

- `src/tui/run.rs` **596/600** — bu turda yerinden etme ZORUNLU: /watch komut kolu
  `polite::handle_watch_command`'a taşınır (~15 satır kazanç); eklenenler (tracker
  init + monitör init + `/context` dalı, ~8 satır) bunun içinde kalır. run.rs'e
  dokunan her görev satır sayısını doğrular (`grep -c "" src/tui/run.rs` ≤ 600,
  beklenen final ≈ 590).
- `src/plain.rs` DEĞİŞMEZ.
- Prompt diyeti: ölçüm, önbellek, yapı sınıflandırması ve döküm tamamen deterministik
  kabuk işi; modele yalnız payload + çerçeve + tek satırlık durum notu gider. Ambient
  panelde LLM metni yasak (K3 aynen).
- Davranış regresyonu yasak: `usta start <topic>`, resume, lock-çakışma onayı,
  katalog upsert, transcript, altı-dosyalık closing flush, `/watch on|off|live`,
  bulk-skip, ride-along sözleşmesi, K1.
- Pin testleri gevşetilmez: imzası değişen çağrı yerlerinin iğneleri hâlâ ISIRAN
  eşdeğerleriyle değiştirilir; her yeni/değişen iğne "çağrıyı yorum satırı yap →
  testin düştüğünü gör → geri al" ile doğrulanır (iki sessiz silme + iki vakum iğne
  emsali).
- Hedef **v0.29.0**; plan tag/push/`cargo install` YAPMAZ.

## Test

- `should_forward(path, kind)`: dizin+Create → true, dizin+Modify → false,
  dosya+Modify → true, var-olmayan yol+Remove → true, yok sayılan → false.
- `StructureTracker`: seed var olan dizinleri tanır; `note_new_dir` yalnız ilk kez
  true; `note_removed` yalnız bilinen dizinde true; yok sayılan dizinler tohumlanmaz.
- `FileMemory::knows`: seed/observe sonrası true, yabancı yol false.
- `classify_flush`: beş sınıf tablosu (yeni dizin notu / bilinen dizin sessiz /
  bilinen dosya silinmesi notu / bilinmeyen kayıp sessiz / dosya → içerik).
- `PendingChanges`: notlar tekilleşir, tavan + taşma satırı, `len` üçünü sayar,
  `take` sıfırlar; `/watch off` düşürme notisi notları da sayar.
- `deliver_pending`: STRUCTURE bloğu FILE bloklarından önce; yalnız-yapı teslimi
  `flow_frame` ile gider ve check koşturmaz; check'li teslim kaydeder + taze blok
  taşır; check'siz kırmızı teslim not taşır; kullanıcının sözü hep son.
- `VerifyMonitor`: devre dışı no-op kilidi (Cargo-dışı kök → `record` etkisiz,
  `note` None, `is_failing` false); `record`/`note` doğruluk tablosu (Pass →
  sessizlik, Fail → özetli tek satır); `verdict_of`/`error_summary` (CLEAN öneki,
  ilk hata satırı, kırpma).
- `drain_and_deliver`: pending boş + kırmızı → not + kullanıcı metni; pending boş +
  yeşil/yok → metin aynen; watching kapalı → hiçbir şey iliştirilmez.
- `flow_frame`: yeni kural 1/5 iğneleri (üç parça, WRITE/READ ayrımı, "never write
  the fix") + korunan iğneler (nudge, scaffold, OFF-LIMITS, acknowledge).
- `introduction_prompt`: kimlik iğneleri (name / how they like to learn / Who) + üç
  kuralın korunması.
- `brain::section_sizes`: roundtrip (`load_system_prompt` çıktısı → etiketler ve
  boyutlar); fallback prompt `(fallback)` etiketi.
- `context_report::build`: dört kova sınıflandırması; "estimate" etiketi; usage
  yok/var iki hâli; `/help` iğnesi; `is_context_command`.
- run.rs kablo pinleri: `handle_watch_command`, güncellenmiş `attach_pending`
  tam-çağrı iğnesi (monitör argümanı dahil), `dispatch_flush` kol pinlerinin
  `content.len()`'e güncellenmesi, `is_failing()` durum-satırı beslemesi — hepsi
  ısırma-doğrulamalı.
- Elle duman testi: Cargo projesinde kasıtlı hata + kaydet + mesaj (check koşar,
  eyes-only blok) + kaydetmeden konuşmaya devam → check koşmayan her turda
  `[build state:]` satırı ve `✗ last check failed` işareti; düzelt + kaydet + mesaj →
  işaret söner; `mkdir` →
  sayaç artar, sonraki mesajda STRUCTURE satırı; Cargo-dışı dizinde aynı akış →
  hiçbir işaret/not yok; `/context` → döküm.

## İlgili

- `docs/superpowers/specs/2026-08-28-watcher-turn-taking-design.md` — K1–K6 zemini;
  C2 teslim-anı check kararını (walkback bloğu) KORUR ve üstüne hafıza ekler.
- `docs/superpowers/specs/2026-08-28-entry-flow-rewrite-design.md` — 13a; A1 onun
  "tanışma profili doldurur" vaadini tamamlar.
- `.superpowers/sdd/progress.md` "CANLI KULLANIM BULGULARI — v0.28.0" — kanıt zemini.
- `SPEC.md` §4.21 (v0.28.0 bloğu) — teslim-anı check cümlesi bu turda düzeltilir;
  yeni §4.23 bu spec'i özetler. `PREDICTION.md` `[build state:]` satırını tanır.
