# Tasarım — Entry Flow Rewrite: Topic Lock'tan Önce Tanışma → Çıkarımsal Rol → Plan (Madde 13; 13a = v0.27.0)

**Tarih:** 2026-08-28
**Kapsam:** Roadmap madde 13'ün TAMAMI (A–H) bu spec'te karara bağlanır; uygulama üç aşamadır (13a/13b/13c) ve her aşama kendi plan dosyasını alır. Bu spec'le birlikte gelen plan yalnız **13a**'yı kapsar: tanışma topic lock'tan önce + üç konuşma kuralı + çıkarımsal `role:` alanı + öneri akışının öğrenciyi görmesi + `start_suggest_system` ve yes/no onay kapısının kaldırılması.
**Durum:** Onaylandı → implement (roadmap satırı 2026-08-28, Fable adversarial review sonrası; Anil kararları işlendi)
**Kaynak:** `docs/ROADMAP.md` madde 13 satırı — bu spec o satırın bağlayıcı açılımıdır. Çelişki durumunda bu spec kazanır ve roadmap satırı düzeltilir.

## Gerekçe

Bugün en sonuçsal seçim, Usta'nın en az bildiği anda veriliyor: boş Enter'da tek atımlık prompt'a YALNIZ `mentor/PROJECT.md` gidiyor (`topic::start_suggest_system` — seviye kriteri yok, öğrenci durumu yok), kullanıcıyı tanıyacak tanışma ise topic kilitlendikten SONRA çalışıyor (`tui/run.rs:331`, `MEET_BLOCK` `progress.rs:48`). Dürüst çerçeve: yanlış ilk topic'in maliyeti bir öksüz progress/curriculum çifti + **boşa giden ilk oturum** — yeni kullanıcının aracı kullanıp kullanmayacağına karar verdiği oturum (retention maliyeti, disk maliyeti değil). Rewrite'ı asıl meşrulaştıran ise plan özelliği: mevcut dosya setinin hiçbiri sıralama/önkoşul/"sırada ne ve neden" tutamıyor.

## İsimlendirme (bağlayıcı — tüm yeni kod İngilizce)

- Modül: `src/tui/intro.rs` — lock-öncesi konuşma döngüsü (TUI-only)
- Tipler: `IntroTurn { user: bool, text: String }` · `IntroOutcome::{Topic { slug: String, turns: Vec<IntroTurn> }, Fallback, Quit}`
- Fonksiyonlar: `run_intro(...)` (döngü) · `stitch(turns, session, recorder)` (session'a aktarma) · `messages(turns) -> Vec<Message>` · `intro_system(global, project_root, today) -> String`
- Marker parse: `topic::parse_topic_marker(reply) -> Option<(String, String)>` — (slug, görünen metin)
- Token'lar (`src/tokens.rs`): `TOPIC_MARKER = "TOPIC:"` · `ROLE_PREFIX = "role: "` · `ROLE_GUIDED = "guided"` · `ROLE_PROJECT = "project"` · `ROLE_OBSERVATION = "observation"`
- Prompt'lar (`src/progress.rs`): `introduction_prompt(project_known, materials)` · `start_here_prompt(materials)` (parametre review fix'inde eklendi: dönen kullanıcı yolu bu olmasa materyal taramasını sessizce kaybederdi) · paylaşılan parça: `topic_marker_rule()` (private)
- İlk-çalıştırma işareti (`src/setup.rs`): dosya `<global>/learner/.introduced` · `intro_marker_path` · `intro_needed(global, index_content) -> bool` (kendi kendini tohumlar) · `mark_intro_done(global, how)` — `how ∈ {"completed", "seeded"}`
- Welcome çıkarımı (`src/tui/entry.rs`): `print_identity_welcome(...)` — `ask_topic` ile intro yolu paylaşır
- SİLİNENLER (13a): `topic::start_suggest_system`, `topic::parse_start_suggestion` + dört testi + `run.rs`'teki tek-atımlık Suggest gövdesi ve `start with '{slug}'? (yes/no)` onay kapısı. `new_topic_confirm_msg` ve yeni-topic onayı KALIR (ölen yalnız önerinin onayıdır).

## Madde 13 kararları (A–G — 13b/13c için de bağlayıcı)

- **(A) Tanışma topic lock'tan önce.** Yeni bir kapılı faz DEĞİL: mevcut `MEET_BLOCK` konuşması yeniden sıralanır. `TEACHING.md` "işe girmeyi geciktirme" ilkesi korunur — o kural *işi* geciktirmeye karşıdır; lock'tan önce tanışma o anın işinin ta kendisidir. 1-2 soru tavanı bilinçli olarak DÜŞER (tavan test edilmemiş bir varsayımdı; derinlik planı kişisel yapan şeydir). Sayının yerine üç kural: **(i)** her soru Usta'nın bir sonraki adımını değiştirmeli (envanter sınavı yok — gerçek bilgi zaten iş sırasında yüzeye çıkar), **(ii)** Usta anladığını ilerledikçe geri yansıtır (uzunluk sorgu değil birikim olarak okunur), **(iii)** çıkış BİR kez, açıkça duyurulur ("istediğin an 'başlayalım' de, gerisini çalışırken öğrenirim") ve tartışmasız onurlandırılır. Kısa cevaptan sıkılma ÇIKARIMI YASAK (sinyal karışık; `progress.rs:150` tam da bu tür çıkarımı yasaklıyor). Tavan İLERİDE gerçek transkriptlerden eklenebilir, korkudan değil.
- **(B) Rol sorulmaz, çıkarılır; kullanıcı-scoped değildir.** Üç şekil: guided (rota omurga) / project (kendi projesini yaparken öğrenme) / observation (uzman; watch + review ister, curriculum sürücüsü hakarete yakın sürtünme). Rol **kullanıcı × alan** çiftinin özelliğidir → topic'in approach dosyasında `role:` satırı olarak yaşar (`## Goal` emsali: "mode değil approach alanı", `SPEC.md` §11 v0.8 kararı; conditional prompt loading bedavaya gelir, `brain.rs` GOAL deseni). Üçlü menü olarak sormak `TEACHING.md:54`'ün yasakladığı form anti-deseni — çıkar, en fazla BİR jargonsuz soru.
- **(C) Plan = curriculum dosyasının terfisi, yedinci dosya değil.** (13b) Bugünkü curriculum sırasız envanter; plan onun üst kümesi. İki model-yazımlı dosyanın tek topic ağacını paylaşması `PROGRESS.md` için teşhis edilen drift'i `.usta` içine taşırdı. Closing flush 6 dosyada kalır. Yeni içerik: build ekseni × learn ekseni JOIN'i; milestone = sıradaki kavramın en ucuz inşa aracı. İki çözünürlük: tüm yol kaba (6-10 milestone), yalnız AKTİF milestone adım adım açık. İLK milestone açıkça kalibrasyon milestone'u ("planın bundan sonra oynamasını bekle") — tanışma self-report verir, davranışsal kanıt iş başlayınca başlar.
- **(D) Her plan bir son ilan eder.** (13b) Formal `## Goal` varsa son odur; yoksa kullanıcının kabul ettiği gösterilebilir yetkinlik. ROTA sık ve sessiz revize edilir; HEDEF nadiren, bilinçli ve görünür.
- **(E) İki shell garantisi.** (13b) (i) Hedef değişmezliği: flush öncesi yakala, sonrası diff'le, yetkisiz değişikliği geri yükle ve YÜZEYE ÇIKAR (`restore_game_pref` emsali, `lifecycle.rs:157,184`). (ii) Plan→map atıfları stabil ID ister (token + migration işi — bütçelenir).
- **(F) "Sırada" ve "%"nin tek sahibi.** (13b) Üç kaynak var: plan-olacak, `PROGRESS.md ## Sırada` (prompt düzeltmesi), ve shell'in kendisi (`next_unseen` `welcome_data.rs:73`, `curriculum_percent` `:54` — welcome kutusu + history satırı). 13b'de plan sahiplenir; `next_unseen` emekli edilir veya yeniden hedeflenir (kod değişikliği: `welcome_data.rs`, `welcome.rs`, history formatı).
- **(G) Sahiplik asimetrisi.** (13b) Learn ekseni Usta'nın (rota elle edit edilmez; konuşmayla yönlendirilir). Build-ekseni beyanları ("Cuma demosuna auth lazım") GERÇEK olarak kabul edilir, pazarlık edilmez — `USER.md`/`PROJECT.md` elle-editlenebilir kalır (`progress.rs:146` kuralı değişmez).

## Blocker kararları (H)

### H2 — Tanışmanın teşhis çıktısının evi (KAPANDI)

**Karar: yeni dosya YOK. Tanışma, gerçek oturumun BAŞIDIR.** Konuşma turları (`IntroTurn` listesi) topic kilitlenince `stitch` ile session history'ye + transcript'e aynen aktarılır; oturum oradan devam eder. Herhangi bir flush anına gelindiğinde topic ARTIK VARDIR — dolayısıyla:

- Konu bilgisi (seviye, yanlış bilinenler, gap'ler) → normal closing sözleşmesiyle `progress/<topic>.md`'ye akar (mevcut kurallar, değişiklik yok).
- Kişi gerçekleri (isim, arka plan, öğrenme tarzı) → `USER.md`'ye akar (mevcut kanıt kuralı aynen; `NO TOPIC KNOWLEDGE` yasağı İHLAL EDİLMEZ çünkü profile'a konu bilgisi yazılmıyor).
- Çıkarılan rol → approach dosyasının `role:` satırına (yeni closing kuralı, aşağıda).

Tek gerçekten evsiz durum: kullanıcı topic kilitlenmeden `/quit` ederse. **Karar: hiçbir şey yazılmaz** — bugünkü "topic prompt'ta çık" semantiğinin birebir karşılığı (`run.rs` `return Ok(None)`: session yok, lock yok, flush yok). Kayıp, tanışmanın uzunluğuyla sınırlı ve kural (iii) kullanıcı istediğinde onu kısa tutuyor. Profile-only flush makinesi YAGNI — 13a'da kurulmaz. (Çökme ayrı: intro turları stitch SONRASI transcript'te olduğundan, kilitlenmiş bir oturumun yarım-kayıt kurtarması tanışmayı da kapsar — mevcut salvage akışı bedavaya çalışır.)

### H3 — Kalıcı ilk-çalıştırma işareti (KAPANDI)

**Karar: `<global>/learner/.introduced` dosyası.** İçerik tek satır: `YYYY-MM-DD | completed` veya `YYYY-MM-DD | seeded` (içerik teşhis içindir; shell YALNIZ varlığına bakar — deterministik).

- **Yazılma:** (1) TUI tanışması bir topic kilitleyerek tamamlanınca `completed`. (2) Kanıt-tohumlama: marker yokken profil dolmuşsa (`profile_is_generic == false` VE boş değil) VEYA global katalogda kayıt varsa (`index::entries` boş değil) → `seeded` yazılır, tanışma atlanır. Tohumlama `intro_needed` çağrısının içindedir — tek çağrı noktası (`tui/run.rs`), main'e kablo gerekmez.
- **`profile_is_generic` NEDEN taşıyıcı olamaz:** embed şablonuna exact-match (`setup.rs:319`) ve şablonun kendisi elle editi davet ediyor — tek edit tanışmayı sonsuza dek atlatır, profil reset'i emektara yeniden tanışma dayatır. Marker bu ikisinden bağımsızdır: `reset_profile_files` marker'a DOKUNMAZ (test kilidi), factory reset global dizini sildiği için marker da gider → sıfır noktasında tanışma yeniden çalışır (doğru).
- **Boş-string tuzağı:** `profile_is_generic("")` `false` döner (şablonla eşleşmez) — yani "USER.md okunamadı" durumu yanlışlıkla "dolu profil" sayılırdı. `intro_needed` bu yüzden boş/eksik profili açıkça generic sayar (test kilidi).

### H1 — Observation rolünün topic'i (KISMEN ERTELENDİ — köşe boyanmıyor)

Tam tasarım (pseudo-topic slug/lock/index semantiği) 13b'nin işi. **13a'nın köşe boyamama garantisi:** 13a hiçbir slug'ı rezerve etmez, hiçbir role özel kod yolu açmaz — rol yalnız approach dosyasında kayıtlı bir satırdır ve her oturum topic-anahtarlı kalır. Tanışma `role: observation` çıkarırsa bile normal bir topic slug'ı kilitlenir (model projenin alanından bir slug önerir); 13b bu topic'i pseudo-topic'e taşımakta/yeniden adlandırmakta serbesttir çünkü 13a'da ona bağlanan tek şey dosya adlarıdır ve `migrate.rs` emsalinde idempotent taşıma zaten var. 13a'nın observation'a tek dokunuşu prompt düzeyindedir (aşağıda, "insulting map" tutarlılığı).

### H4 — Plain/pipe/CI yolu (KAPANDI)

**Karar: plain yolu 13a'da DEĞİŞMEZ.** Gerekçe: (1) pipe/CI çok-turlu konuşmada bloklanamaz — bugünkü "TTY değilse sessizce `general`" davranışı (`plain.rs:56`) aynen kalır; (2) plain zaten `interpret_topic_input(raw, &local, false)` çağırır — `TopicChoice::Suggest` orada `unreachable!` (`plain.rs:123`), yani `start_suggest_system` silmek plain'e hiç dokunmaz; (3) plain'in lock-sonrası tanışması `MEET_BLOCK` ile sürer (mevcut davranış, regresyon yok). Marker'ı plain asla yazmaz/okumaz; plain-only bir kullanıcı sonradan TUI açarsa kanıt-tohumlama (dolu profil / katalog kaydı) onu tanışmadan muaf tutar. `usta start <topic>` her iki yüzeyde değişmez.

### H5 — Plan terki ADHD-güvenli (13c'ye) · H6 — Yanlış öz-değerlendirme (13c'ye)

H5: terk edilen plan "%40'ta donmuş" suçluluk objesi olamaz — arşivle + yeniden çerçevele (`streak: 0` basılamazlığı emsali). H6: gap'ler loglanır, kanıta dayalı rota TEKLİF edilir, rol asla zorla değiştirilmez — ama sessizce değil: "tekrar eden desenleri not ediyorum" BİR kez söylenir (gözetleme hissi yasağı).

## 13a Davranış (bağlayıcı detay)

### Akış

Bare `usta` (TUI, topic argümanı yok):

1. `intro_needed(global, index_content)` — marker var → false; kanıt var → tohumla, false; yoksa true.
2. **true** → identity welcome basılır (`print_identity_welcome`, prompt satırı OLMADAN), materials PDF dönüştürme notisleri + digest toplanır, `intro_system` (brain, `tokens::DEFAULT_TOPIC` ile — topic-scoped dosyalar zaten yok) yüklenir, `run_intro(... introduction_prompt(project_known, digest))` çalışır.
3. **false** → bugünkü `ask_topic` döngüsü. `TopicChoice::Suggest` (boş Enter + dolu PROJECT.md + local boş) artık tek-atımlık öneri DEĞİL: `run_intro(... start_here_prompt())` — konuşmalı öneri. Yes/no kapısı yerine anlaşma konuşmada olur ("hayır, daha kolay bir şey" artık çalışır — binary kapının yapamadığı).
4. `run_intro` dönüşleri: `Topic{slug, turns}` → `topic: <slug>` notisi, slug kilitlenir (`mark_intro_done` burada YAZILMAZ — bkz. adım 5); `Fallback` (İLK model çağrısı iptal/hata) → `introduction failed — type a topic` / `suggestion failed — type a topic` + manuel giriş akışı; `Quit` → `Ok(None)`.
5. Lock sonrası: lock-çakışması onayı + `build_session` AYNEN. `mark_intro_done("completed")` gerçek kodda TAM BURADA yazılır — `run.rs`'te lock-çakışması onayından VE `build_session`'ın başarısından SONRA (ilk-çalıştırmaysa). Sıralama bilinçli (8bd72ba review fix'i): adım 4'te yazılsaydı, kullanıcı lock-çakışması onayını reddettiğinde (SPEC §4.22: session yok, lock yok) marker yine de diskte kalır ve kullanıcı bir daha asla tanıştırılmaz. Ardından intro yolundaysa: `stitch(turns, session, recorder)` + `backend.reset_session()` (sistem prompt'u gerçek topic'le yenilendi; CLI backend `--resume` yeni system'i almaz — reset şart, `maybe_compact` emsali) ve **opening/onboarding turu ENJEKTE EDİLMEZ** (`opening: Option<String>` = None) — konuşma zaten akıyor, son tur asistanın önerisi, sıra kullanıcıda.

`usta start <topic>`: değişmez (tanışma yok; `MEET_BLOCK` gerekiyorsa mevcut yerlerinde). Resume akışı: değişmez.

### Marker sözleşmesi

Model, kullanıcı başlamayı kabul ettiğinde (veya "sen seç" dediğinde) cevabının SON satırına tam olarak `TOPIC: <topic-slug>` yazar (lowercase, tireli, 1-3 kelime; satırdan sonra hiçbir şey; anlaşmadan önce ASLA). Shell her intro cevabında yalnız son boş-olmayan satırı yoklar (`parse_topic_marker`): prefix `tokens::TOPIC_MARKER`, slug `parse_start_suggestion`'ın tire→boşluk hilesiyle `slugify_topic`'ten geçer, boş slug → None. Görünen metin marker satırı düşülerek basılır; `turns`'a HAM cevap itilir (rol sırası bozulmaz, tek-satır-marker cevabında ardışık iki user turu oluşmaz). Model marker'ı erken basarsa kabul edilir — topic kilitlenir, konuşma oturum içinde sürer (öğrenme kesintisiz; eski akışın "yanlış öneriyi onaylatma" maliyetinden daha ucuz bir hata).

### Prompt'lar

- `introduction_prompt(project_known: bool, materials: Option<&str>)`: üç kural (i-iii) + kısa-cevaptan-çıkarım yasağı + rol çıkarımı (menü yasak, en fazla bir jargonsuz soru; closing'de `role:` satırına gideceği söylenir) + project bloğu (varsa "temel soruları yeniden sorma / öneriyi projeye çapala", yoksa onboarding'in "ne yapıyorsun, neden, ölçek, stack" doğal keşfi + closing'de `project` dosyası) + materyal bloğu (onboarding ile paylaşılan yardımcı) + marker sözleşmesi + closing sözleşmesi notu (approach + TAM harita + profil; dosyaları shell yazar, Hard Rule 6). Dil: SOUL kilidi system prompt'ta zaten var (intro TAM brain ile koşar — eski çıplak one-shot'ın "Reply in English" yaması gereksizleşir).
- `start_here_prompt()`: profil + PROJECT.md system prompt'ta; profildeki seviyeye KALİBRE tek başlangıç önerisi (neden bu, bugün başlanacak kadar küçük ilk adım); kısa, selamsız; gerçekten bilinmeyen varsa en fazla bir soru; marker sözleşmesi + closing sözleşmesi notu.

### `role:` alanı (approach dosyası)

`closing_prompt`'un approach kuralına eklenir: başlığın altındaki İLK satır `role: guided|project|observation` — kullanıcının seninle nasıl çalıştığından çıkarılır; oturumlar arası stabil tutulur; yalnız açık davranışsal kanıtla değişir ve değişince tek cümleyle söylenir. Curriculum kuralına tutarlılık istisnası: approach `role: observation` kaydediyorsa ilk oturumda spekülatif TAM harita ÇIKARILMAZ — yalnız oturumlarda fiilen kanıtlanan öğeler (görülen gap'ler, konuşulan konular) listelenir. (13b'nin gap-defteri makinesi değil; yalnız "uzmana müfredat yazma" tutarsızlığının prompt düzeyinde kapatılması.) Shell 13a'da `role:` PARSE ETMEZ (YAGNI — deterministik bir tüketici yok; 13b `role_of` yardımcısını ekler). Token'lar yine de `tokens.rs`'te tek-kaynaklanır çünkü iki prompt aynı string'leri kullanır.

### Kenar durumlar ve kabul edilen açıklar

- İlk model çağrısı iptal/hata → `Fallback` + manuel giriş (eski "suggestion failed" esnekliği korunur). Konuşma ORTASINDA iptal/hata → notis, kullanıcı girişi beklenir (`/quit` her an çıkar).
- Intro sırasında `/help` çalışır; `/show`, `/watch` "oturum içinde çalışır" notisi alır (ask_topic paritesi); boş Enter yutulur (lock öncesi resume sentineli yok).
- Intro system prompt'u `DEFAULT_TOPIC` ile yüklenir; kullanıcının gerçekten `general` adlı bir topic'i varsa onun progress'i intro'ya sızar — zararsız, kabul edildi (model fazladan bağlam görür; yanlış dosyaya yazım olmaz, closing gerçek slug'la koşar).
- Suggest yolunda `[GAME]` streak satırı bu oturumun açılışına girmez (opening turu atlandı) — bilinen küçük boşluk, kabul edildi; oyun anlatısı oturum içinde sürer.
- Kilitlenen slug'ın diskte progress'i varsa (`has_progress == true`, ör. katalogdan düşmüş eski dosya): opening yine None — yenilenen system prompt o progress'i zaten yükler, model görür. Kabul edildi.
- `run.rs` 585/600 — intro dalı ince tutulur (gövde `intro.rs`'te); Görev 5 satır bütçesini doğrular.

## Test (13a — plan görevlerinde ayrıntılı)

- `parse_topic_marker`: son satır çıkarımı, dağınık slug normalizasyonu, orta-satır marker'ın reddi, boş slug reddi, markersız None.
- Marker kalıcılığı: taze global'de true; `mark_intro_done` sonrası false; dolu profille tohumlama; katalog kaydıyla tohumlama; boş profil ≠ emektar; profil reset'i marker'ı silmez.
- Prompt'lar: üç kural + marker sözleşmesi + rol çıkarım talimatı var, 1-2 tavanı YOK; `start_here_prompt` profil+PROJECT.md+kalibrasyon; `closing_prompt` `role:` kuralı + observation harita istisnası.
- `intro.rs`: `messages` rol eşlemesi; `stitch` session+transcript replay'i; `run.rs` kablo pin testi (`include_str!` iğneleri — `polite.rs` H3 emsali).
- Elle uçtan uca: taze HOME ile ilk `usta` (tanışma → TOPIC kilidi → oturum → kapanışta profil+approach `role:` satırı); dolu profil + PROJECT.md + boş Enter (konuşmalı öneri); `usta start rust` (tanışma yok); pipe (`echo | usta`, `general`e düşer); `/quit` tanışma ortasında (hiçbir dosya yazılmaz).

## 13b/13c sınırı

13b: curriculum terfisi + stabil map ID'leri + hedef-restore garantisi + plan üretimi + giriş noktası türetimi + next/% tek sahibi (H1 tam çözümü dahil). 13c: milestone sınırında yeniden planlama + hak-edilmiş plan teklifi + iki yönlü rol geçişleri + ADHD-güvenli plan terki (H5) + H6 şeffaflık cümlesi. Her biri kendi spec → plan döngüsünü alır; bu spec'in A–G kararları onlar için bağlayıcı zemindir.
