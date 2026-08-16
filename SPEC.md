# Usta — Tasarım Spec'i

> Terminal'de çalışan, Claude (Opus) destekli, Rust ile yazılmış, domain-agnostik **Socratic öğrenim mentoru**. Yaparak-öğrenmeyi yürütür. Asla kullanıcının yerine iş yapmaz. Uydurmaz — bilmezse araştırır.

- **Durum:** taslak (v0.1) — 2026-08-06
- **Sahip:** Anil (cursedxp)
- **İlk kullanıcı:** Anil (Rust öğrenmek). İleride: herhangi bir dil/domain, herkes.
- **Repo:** `cursedxp/usta` (public), headspace içinde ayrı proje — kendi git'i, headspace `.gitignore`'da.

---

## 1. Amaç ve Felsefe

**Tek cümle:** Yanında oturan, sen gerçek işi yaparken seni yetiştiren usta.

**Çekirdek felsefe:** Öğrenerek-yaparak (learning by doing). Pasif ders yok. Gerçek projeyi inşa ederken, o akışın içinde öğrenirsin. Proje = öğrenme aracı.

**Kapatılacak gap:** Kullanıcı "nasıl yapılır"ı biliyor; "bir mühendisin işe nasıl yaklaştığı"nı değil — iyi spek, iyi mimari, ölçek okuma, teknoloji seçimi. Usta bu meta-beceriyi öğretir.

**İlk amaç:** Anil'in kendi kullanımı + kullandıkça feedback verip Usta'yı birlikte geliştirmek. Çift döngü: Usta Anil'e öğretir, Anil Usta'yı iyileştirir.

## 2. Sert Kurallar (ihlal edilemez)

1. **Sıfır otonom aksiyon.** Usta kod yazmaz, dosya düzeltmez, kullanıcının işini yapmaz.
   - **Gösterebilir:** neyin hatalı olduğunu · nasıl yapılması gerektiğini (yaklaşım, mantık, yön) · kavramı gösteren minik illüstrasyon/pseudocode.
   - **Yazamaz:** kullanıcının projesine çalışan/kopyala-yapıştır çözüm. Düzelten hep kullanıcı.
2. **Uydurmaz.** Bilmediği konuda fikir yürütmez → **araştırır** (web/kaynak), sonra öğretir.
3. **Felç önler.** Spek/plan mükemmelleştirmek işe başlamayı öldürmemeli. Usta "yeter, suya gir" bekçisidir (ADHD-aware).
4. **Proje-temelli.** Feedback havada teori değil — kullanıcının gerçek projesine demirli, gerekçeli.
5. **Kendi sağlığını denetler.** Kırık wiki-link, eskimiş/tutarsız dosya → otonom yakalar, onarır/uyarır.

## 3. Persona

- **Senior / domain uzmanı.** O konuya hakim biri gibi davranır.
- **Kod kalitesinden sorumlu** (kod domainlerinde) — "çalışıyor mu" değil "iyi mi" standardını tutar.
- **Nazik ama iten.** Mükemmeliyetçiliği keser, ADHD-aware (headspace `mentorship-mode` ruhu): yargı yok, eşik düşür, parçaya böl.
- Kullanıcıyı tanır (ADHD, "suya gir" mantrası, kişilik, iletişim tarzı) → doğru desteği ona göre verir.

## 4. Yetenekler

1. **Öğretim (akış-içi):** proje inşa edilirken, o an, o adımda öğretir — sıradaki adım + dil/konu püf noktası.
2. **Meta-beceri öğretimi:** iyi spek nasıl yazılır, mühendis nasıl düşünür, iş planlama.
3. **Ölçeğe duyarlı mimari:** projenin ölçeğini okumayı öğretir (1 kişilik vs 1000 kişilik). Over/under-engineering'i önler. Ezber pattern değil — "bu bağlamda ne yeter, neden".
4. **Teknoloji seçimi:** göreve uygun teknolojileri önerir/açıklar, kullanıcının bilmediklerini yüzeye çıkarır, neden-o-teknoloji gerekçesini öğretir. Güncellik → canlı araştırmadan gelir (ayrı "kendini güncelleyen sistem" YOK — YAGNI).
5. **Proaktif feedback:** yazılan kodu izler, sorun görürse söyler ("burası şöyle olmamalı, çünkü...") — sen sormadan.
6. **Eksik teşhisi:** kullanıcının işini izleyerek zayıf noktaları yakalar (kanıta dayalı: hangi kodda görüldü).
7. **Eğitim planlama:** tespit edilen gap'lere göre hedefli mini-dersler/alıştırmalar kurgular. Planlar ve önerir — yaptırmaz.
8. **Domaine göre yaklaşım seçimi:** her proje speklik değil. Yazılım → spek+mimari; marketing → brief/hipotez/ölçüm; git öğrenme → "spek gereksiz, direkt yap". Usta doğru yapılandırma-adımını seçer.

## 4.5 Başlatma / Kullanım

- **`usta`** (argsız) → eksikse global + proje `.usta/` **otomatik kurulur** (bootstrap), sonra konuyu sorar (TTY'de) veya `genel`'e düşer (piped).
- **`usta start <konu>`** → konu açıkça verilir (slug'lanır: `"JavaScript Basics"` → `javascript`). Kısayol.
- **`usta init`** → opsiyonel; sadece kurar (başlatmadan), per-dosya durum yazar. Artık zorunlu değil.
- Konu = öğrenme başlığı/dosyalama anahtarı (`progress/<konu>.md`). **Ne inşa ettiğin** `mentor/PROJECT.md`'de yaşar (Usta tanışmadan yazar, kullanıcı elle düzenleyebilir); Usta önce oraya bakar, orada yoksa sorar. Projenin durumu `mentor/PROGRESS.md`'de (Bitti/Yapılıyor/Sırada + append-only Kararlar).
- **Proje-farkında başlangıç:** ilk oturumda (`local` boş) `mentor/PROJECT.md` doluysa konu girişinde **boş Enter = başlangıç önerisi** — Usta PROJECT.md'den konu + gerekçe + somut ilk adım önerir (tek mini-çağrı, sonrası koşulsuz session reset — slug mini-oturum paritesi), kullanıcı onaylarsa oturum o konuyla açılır; öneri metni `intro` olarak onboarding'e taşınır (Usta kendi önerisini tekrar anlatmaz, ilk adımdan devam eder). Welcome/prompt satırı bu durumda "PROJECT.md found — press Enter, Usta suggests where to start." ipucunu gösterir. `local` doluysa boş Enter = **resume** (öncelikli, değişmedi). Plain/pipe yolunda öneri yok.

## 4.6 Pedagoji Katmanı (v0.3)

Öğretim yönü değil, **geri çağırma yönü** optimize edilir — kalıcı öğrenme kullanıcıdan çıkan üretimde olur (testing/generation effect):

1. **Açılış drilli (vade-farkında, v0.13):** oturum başında progress'teki "Geri çağırma soruları"ndan yalnız **vadesi gelenler** sorulur (`due:` bugün veya öncesi; kuyruksuz eski madde vadeli sayılır), en fazla 3, en eski vadeli önce (progress varsa shell tetikler, Usta ilk sözü alır). Vadeli soru yoksa tek cümle "no reviews due today" ile drill atlanır, doğrudan işe geçilir. 2 dk ısınma — ADHD için düşük eşikli "suya girme" rampası. Detay: §4.13.
2. **Anlat-modu (Feynman):** parça kapanışında roller döner — kullanıcı yazdığını açıklar; açıklamadaki boşluk gap sinyalidir (koddan iyi).
3. **İpucu merdiveni:** soru → kavram adı → pseudocode; kod asla (Sert Kural 1). Seviye yükseldikçe merdiven kısalır (fading); bir basamakta ~2 tur takılınca bir basamak inilir (ADHD dengesi).
4. **Tahmin protokolü:** kayıtta `cargo check` koşar (60 sn timeout, 4KB kırpma, Rust dışı projede sessiz atlanır); hata varsa Usta sonucu söylemez, önce tahmin ettirir (hypercorrection).
5. **Hata günlüğü:** progress'te `hata tipi | sayaç | son örnek`; 3+ tekrar = `GAP ADAYI` → curriculum'a mini-alıştırma önerisi.

Kuralların tamamı USTA.md'de yaşar; Rust sadece tetikler (açılış turn'ü, check koşucusu, progress formatı).

## 4.7 Yönetim Komutları (v0.4)

- **`usta topics`** — global katalog listelenir: `konu | proje | son oturum`. LLM gerekmez.
- **`usta reset <konu>`** — bulunduğun projenin o konudaki progress'i silinir (`[e/H]` onaylı), katalogdan düşülür. "Bu konuyu baştan öğreneyim" senaryosu.
- **`usta reset --factory`** — katalogdaki TÜM projelerin `.usta/`'sı + global brain silinir; liste önce gösterilir, onay için "evet" yazılır. Katalogda olmayan eski projeler kapsam dışı (uyarı + `find` ipucu basılır).
- **Katalog otomatik güncellenir:** kapanış flush'ı `learner/index.md` sonundaki `## Kayıtlar` bölümüne `- konu | proje-yolu | YYYY-MM-DD` upsert eder. Yan etki: index system prompt'ta olduğundan Usta tüm başlıklardan haberdardır — izolasyon bozulmaz (progress yalnız aktif konudan yüklenir).

## 4.8 Her-Konu Öğrenimi (v0.6)

Domain listesi elle genişletilmez — sistem kendi kendini genişletir:

- **Yaklaşım üretimi:** yaklaşımı olmayan konuda ilk oturum `[YENİ KONU — TANIŞMA]` ile açılır (açık sohbet, form değil; yön kullanıcıda). Usta domain doğasını `_default.md`'nin üç sorusuyla türetir (pratik / çıktı / feedback), kapanışta `.usta/approaches/<konu>.md` yazılır. **Canlı belge:** oturum içinde revize edilir, elle düzenlenebilir.
- **Müfredat haritası:** ilk oturumda web araştırmalı TAM harita `.usta/learner/curriculum/<konu>.md`'ye çıkarılır; her madde `görülmedi/görüldü/oturdu/derinleşildi`. Kapanışta güncellenir. Kapsam bekçiliği ("haritada X hâlâ açık"), drill beslemesi (oturdu-ama-eskidi bölgesi) ve derinlik ayarı (sığlaşma yasağı) buradan çalışır.
- **Brain yüklemesi genel:** `approaches/` altındaki TÜM dosyalar (global ∪ proje, override proje lehine, alfabetik) + aktif konunun curriculum + progress'i system prompt'a girer. Yaklaşım seçimini kod değil USTA.md kuralı yapar.
- **Kapanış çok-dosya:** tek çağrı `===DOSYA: <ad>===` bölücülü `progress`(her zaman) / `approach` / `curriculum`(değiştiğinde) üretir; bölücüsüz yanıt geriye-uyumlu progress sayılır; bilinmeyen ad uyarıyla atlanır.

## 4.9 Bağlam Yönetimi (v0.7)

- **Gösterge:** her yanıt altında `▓▓░░░░░░ bağlam 41k/1000k` (son çağrının input+cache toplamı / **modele göre pencere**); ≥%70 sarı. Düz modda ve token bilgisi yoksa çizilmez.
- **Otomatik ara-kayıt + kompaksiyon:** %70 eşiğinde flush çalışır (progress/approach/curriculum diske iner), system prompt taze dosyalarla yeniden yüklenir, history `[ARA KAYIT]` notu + son 4 turn'e kırpılır, CLI `session_id` sıfırlanır. Kullanıcı akışı kesilmez. Flush başarısızsa kompaksiyon iptal — veri yazılmadan history atılmaz. Kayıp minimal: önemli olan zaten dosyalarda (progress = damıtılmış oturum).
- **Görsel:** banner model etiketi taşır (`opus · cli`), kullanıcı promptu `❯`, Usta bloğu 2 boşluk padding + genişliğe sarma.

## 4.10 Hedefli Öğrenme (v0.8)

Öğrenmenin iki modu tek sistemde: **keşif** (açık uçlu merak) ve **hedef** (sertifika/seviye/teslim — tarih + eşik). Tanışma hangisi olduğunu netleştirir.

- **Hedef kaydı jenerik:** approach `## Hedef` (ne / tarih / eşik / format), progress `## Hedef Durumu` (kalan süre / harita % / tempo / ölçüm logu). AWS SAA da Goethe B1 de aynı kalıp.
- **Harita resmi çerçeveden:** sınav müfredatı / exam guide / CEFR — web araştırmalı, tahmin değil.
- **Tempo bekçiliği:** system prompt'un `===== TODAY =====` bölümü sayesinde kalan süre hesaplanır; her açılışta tek satır durum, riskliyse plan revizesi.
- **Format-uyumlu drill:** senaryo çoktan-seçmeli / yazma görevi / prova — hedefin gerçek sınav formatı.
- **Medium sınırı:** terminalde çalışmayan modüller haritada `dış kaynak gerekli` olarak işaretlenir — sahte tamlık yok.

## 4.11 Sağlamlaştırma (v0.9)

- **Ham oturum kaydı:** her turn anında `.usta/sessions/<konu>-<zaman>.jsonl`'e iner; başarılı kapanışta `.done.jsonl`. Flush ölse/terminal çökse oturum diskte — açılışta yarım oturum bildirilir.
- **Olay seli tavanı:** 5+ dosyalık debounce batch'i (git checkout, format-all) LLM'siz geçer; `FileMemory` sessizce senkronlanır.
- **Konu kilidi:** `.usta/.lock-<konu>` — eşzamanlı ikinci oturum onayla açılır, progress sessizce ezilmez. Pipe modunda uyarı + devam.
- **Yedek:** `write_atomic` önceki sürümü `.bak`'a kopyalar — kötü model çıktısı geri alınabilir.
- **Budama + sır filtresi:** progress 20-madde eşiğiyle budanır; `.pem`/`.key`/`secret`/`credential` dosyaları watcher'dan LLM'e asla gitmez.

## 4.12 Egzersiz Döngüsü (v0.12)

- **`exercises/` konvansiyonu:** görünür klasör, scaffold kurar; Usta teslimatı sohbette atar, dosyayı kullanıcı yazar.
- **Path-tanıma:** `is_exercise_path` — root-göreli veya mutlak path'te `exercises/` bileşeni geçen her kayıt egzersiz sayılır.
- **Egzersiz feedback çerçevesi:** watcher turn'üne "AS AN EXERCISE" işareti düşer — atamaya karşı değerlendir (mükemmelliğe karşı değil), hint ladder aynen uygulanır, çözüm veya tamamlanabilir iskelet asla yazılmaz.
- **Check-atlama:** `exercises/` altındaki path'lerde `cargo check` koşulmaz — egzersiz her domain'de çalışır, kod-özel doğrulama zorunlu değildir.
- **Kalıcılık:** progress'te `## Açık egzersiz` bölümü açık atamayı tutar; oturum açılışında hatırlatılır, tamamlanınca `Kapatılanlar`a taşınır.

## 4.13 Spaced Repetition (v0.13)

Roadmap #3: geri çağırma sorularına vade (due-date) verilir, vadesi gelmeyen sorulmaz, geleni açılışta görünür kılınır — spacing effect'i drill'e taşır.

- **Format (makine-okur kuyruk):** `## Geri çağırma soruları` maddeleri `- <soru> — <tek satır cevap> | due: YYYY-MM-DD | ivl: <gün>`. Kuyruksuz eski madde bugün vadeli sayılır (migrasyon: ilk kapanışta model kuyruk ekler).
- **Basitleştirilmiş SM-2 (ease factor YOK):** aralık merdiveni gün cinsinden `1 → 3 → 7 → 16 → 35 → 90`. Rahat hatırlandı → bir üst basamak (`due = bugün + yeni ivl`); zorlandı/yanlış veya yeni soru → `ivl: 1` (yarın vadeli); drill'e girmeyen soru → kuyruk değişmez.
- **Emeklilik:** `ivl: 90` basamağını rahat geçen soru `Kapatılanlar`a tek satır özetle taşınır, soru listesinden düşer — progress şişmez.
- **Açılış drilli:** yalnız vadesi gelenler (`due` ≤ bugün), en fazla 3, en eski önce; hiçbiri vadeli değilse tek cümle "no reviews due today" ile atlanır.
- **Welcome göstergesi:** saf fonksiyon `due_count(progress, today)` — `Reviews due today: N` (N>0) / `No reviews due today` (soru var, vadeli yok) / satır yok (hiç soru yok).
- **Hesap sahibi = model** (aralık seçimi, kuyruk yazımı kapanış flush'ında zaten dosyayı model yazıyor); **kabuk yalnız sayar** (`due_count` — welcome göstergesi). "İnce kabuk" korunur.

Kuralların tamamı USTA.md'de yaşar (kapanış/açılış prompt'ları); Rust yalnız `due_count` sayacını ve welcome render'ını taşır. Tasarım detayı: `docs/superpowers/specs/2026-08-15-spaced-repetition-design.md`.

## 4.14 Onboarding-Lite Sihirbazı (v0.13)

Roadmap #4'ün ilk yarısı: `backend::select()` bulamayınca çıplak hatayla ölmek yerine, uygun ortamda yönlendiren hafif ilk-çalıştırma sihirbazı devreye girer — kurulum tamamlanınca **aynı süreçte** devam eder.

- **Tetik koşulu:** `select()` `Err` döner VE stdin+stdout TTY ise (`std::io::IsTerminal`) VE `USTA_BACKEND` set değilse. Koşullardan biri sağlanmazsa (TTY yok — pipe/CI, veya `USTA_BACKEND` set edilmiş) sihirbaz devreye GİRMEZ, mevcut `bail!` korunur. `USTA_BACKEND` geçersiz bir değere sahipse bu konfigürasyon hatasıdır, eksik-backend değildir — sihirbaz burada da devreye girmez.
- **Akış:** sihirbaz iki seçenek gösterir — Claude Code CLI kurulumu (link + "then just press Enter here") veya Anthropic API key yapıştırma (`sk-ant-...`). Girdi yorumu:
  - boş satır → **Recheck**: `select()` yeniden denenir; başarılıysa normal akışa devam, değilse aynı prompt tekrar.
  - `sk-ant-` ile başlayan satır → **Key**: trim'lenir, yalnız süreç env'ine (`std::env::set_var`) yazılır — **DİSKE ASLA YAZILMAZ**, ekrana geri yazdırılmaz; ardından `select()` yeniden denenir (API yolu artık bulunur) + tek satır kalıcılaştırma ipucu ("add to your shell profile to skip this next time").
  - `q`/`quit` (case-insensitive) → **Quit**: sihirbaz mesajıyla temiz çıkış.
  - Diğer her girdi → kısa uyarı + aynı prompt tekrar.
- **Süreç-içi kapsam:** girilen API key yalnızca çalışan process'in ortam değişkeninde yaşar — dosyaya, keychain'e veya profile hiçbir şekilde yazılmaz; süreç kapanınca kaybolur (kalıcılaştırma kullanıcının kendi tercihi, sihirbaz sadece hatırlatır).
- **Kapsam dışı (bilinçli ertelendi):** prebuilt binary, GitHub Releases, Homebrew tap, CI release workflow, key'in diske/keychain'e kalıcılaştırılması, tam sihirbaz (dil/isim/tanışma akışı), model seçimi sihirbazı.

Tasarım detayı: `docs/superpowers/specs/2026-08-15-onboarding-lite-design.md`.

**Sürümleme politikası:** her tamamlanan roadmap maddesi minor bump ile işaretlenir, tag `vX.Y.Z`.

## 4.15 Materyal Yutma (v0.14)

Roadmap #5: kullanıcı kendi kitabını/kurs notunu getirir, müfredat onun bölümlerine demirlenir — web araştırması artık spine değil, tamamlayıcı.

- **`materials/` konvansiyonu:** görünür klasör, diğerleri gibi (`exercises/`, `progress/`) scaffold tarafından kurulur. Kullanıcı md/txt dosyasını buraya koyar; kabuk bunu otomatik keşfeder, model hiçbir şey oluşturmaz.
- **Digest enjeksiyonu YALNIZ yeni-konu tanışmasında:** `materials/` doluysa ve konu ilk kez açılıyorsa (resume/opening akışında DEĞİL) kabuk deterministik bir digest üretip modele enjekte eder — başlık iskeleti + kısa alıntılar. Devam eden oturumlarda digest tekrar enjekte edilmez; kalıcılık müfredat haritasındaki kaynak referanslarından gelir.
- **pdftotext opsiyonel:** PATH'te `pdftotext` varsa PDF dosyaları otomatik txt'ye çevrilir. Yoksa PDF atlanır, tek satır bilgi mesajı basılır (`brew install poppler` önerisiyle) — sert hata yok, akış devam eder.
- **Kaynak-ref demirleme:** müfredat haritası maddeleri `— kaynak: <dosya> §<bölüm>` referanslarıyla materyale bağlanır. Kalıcı olan digest DEĞİL, bu referanslardır — digest tek seferlik enjeksiyon, oturumlar arası taşınmaz.
- **Cap'ler:** dosya başına 8_000 karakter, toplam 16_000 karakter; kesme UTF-8 güvenli sınırda yapılır ve `[truncated]` ile işaretlenir.
- **Web araştırma kapsam bekçiliği korunur:** materyalin kapsamadığı kritik bir konu varsa harita web araştırmasıyla doldurulur, ilgili madde `— kaynak: web` ile işaretlenir.

Tasarım detayı: docs/superpowers/specs/2026-08-15-material-ingest-design.md

## 4.16 İlerleme Özeti / Motivasyon (v0.15)

Roadmap #6: görünür ilerleme = ADHD için yakıt, sıfır suçlama. Tamamen kabuk işi — LLM çağrısı yok ("kabuk sayar").

- **Oturum geçmişi:** global `~/.config/usta/learner/history.md`, append-only, başlık `# Oturum Geçmişi`. Kapanış flush'ı katalog güncellemesinin (`index::record`) hemen yanında bir satır düşer: `- YYYY-MM-DD | <konu> | map <P>% | settled <N>` (P = `curriculum_percent`, N = `oturdu`+`derinleşildi` madde sayısı; curriculum dosyası flush SONRASI diskten okunur — curriculum yoksa `map -` / `settled -`). Aynı gün aynı konuda birden çok oturum = birden çok satır. Yazım hatası = warn, oturumu düşürmez (katalogla aynı tolerans).
- **`usta stats` komutu:** son 7 gün penceresi — konu başına oturum sayısı + map% ilk→son delta + settled ilk→son delta; genel: toplam oturum, güncel streak (ardışık gün, herhangi bir konu — bugünden veya dünden geriye), en uzun streak. LLM gerekmez, saf parser + hesap. `usta help`/`/help` metninde listelenir.
- **ADHD-safe kurallar:** `current streak: 0` **hiçbir yüzeyde yazılmaz.** Streak kırıksa yalnız `longest streak: N day(s)` pozitif çerçeveyle basılır. Boş hafta (7 gün oturumsuz): `quiet week — your longest streak is still N day(s)`. Hiç kayıt yoksa: `no sessions recorded yet — streaks start with the first one.` Karşılaştırma/utandırma dili hiçbir çıktıda yok.
- **Welcome satırı:** identity + full-mode kutularının ikisinde de `week_sessions > 0` iken `This week: N session(s) · streak M day(s)` (M=0 ise streak kısmı düşer, hiç satır kaybolmaz). Veri `history.md`'den saf parser ile okunur — ayrı sayaç tutulmaz.

Tasarım detayı: `docs/superpowers/specs/2026-08-15-progress-stats-design.md`.

## 4.17 Deneme Sınavı / Mock Exam (v0.16)

Roadmap #7: hedefli (GOAL modlu) öğrenmede gerçek prova mekanizması. `/exam` **prompt-enjeksiyon komutu** — statik intercept değil; kabuk hedef kapısını tutar, sınavın kendisi (soru üretimi, değerlendirme) LLM işi.

- **Kapı: hedef şart.** Konunun approach dosyasında (proje override öncelikli, yoksa global — `brain.rs`'teki öncelik sırasıyla aynı) `## Hedef` yoksa kabuk bir kapı bildirimi basar ("no goal set for this topic — /exam needs a goal (exam/certificate); set one in the introduction") ve **LLM'e hiç gitmez**. Hedefli konuda `exam_prompt(topic)` normal kullanıcı turu gibi oturuma enjekte edilir (`session.push_user` + recorder + ask akışı — açılış drilinin oturum-içi muadili).
- **Sınav akışı:** model müfredat haritasından bir deneme sınavı kurar, approach'taki `## Hedef` formatını izler (soru stili, süre bütçesi, geçme eşiği), zayıf/`oturdu`-olmayan maddelere ağırlık verir, soru sayısı ve süre bütçesini baştan söyler. Ardından **tek seferde tek soru** sorar ve cevabı bekler; sınav sırasında **ipucu merdiveni ve öğretim ASKIDADIR** — gerçek prova hissi, ara geri bildirim yok. "sınavı durdur" denirse erken biter, o ana kadarki cevaplar puanlanır.
- **Sonuç:** son cevaptan sonra model hedefin eşiğine göre skor verir, kısa harita-maddesi kırılımı (güçlü/zayıf) sunar, zayıf maddeleri gap adayı olarak adlandırır ve sonucun kapanışta kaydedileceğini hatırlatır.
- **Zamanlama yumuşak (v1):** kabuk süre tutmaz — sert zamanlayıcı kapsam dışı; süre bütçesi yalnız modelin sözlü taahhüdüdür.
- **Kapanışta kayıt:** `closing_prompt`'a tek kural cümlesi eklendi — bu oturumda bir deneme sınavı (`/exam`) çalıştıysa sonucu `## Hedef Durumu` ölçüm günlüğüne (`date | mock exam | score`) işlenir, zayıf çıkan maddeler `## Gap'ler`e yazılır.
- **Kural evi: GOAL.md** — embedded, yalnız hedefli konularda yüklenir. `## Mock Exams` bölümü sınav yürütüm kurallarını (tek soru, askıya alınan ipucu merdiveni, eşiğe göre skor, kırılım, erken bitirme, kayıt hatırlatması) ve pedagojik notu (deneme = en güçlü retrieval practice; sınav sonrası zayıf maddeler normal öğretim moduna döner) taşır.
- **Kapsam dışı:** sert zamanlayıcı, ayrı sınav geçmişi dosyası (ölçüm günlüğü yeter), soru bankası/tekrar eden kalıplar, hedefsiz konuda genel quiz modu (drill zaten var).

Tasarım detayı: `docs/superpowers/specs/2026-08-15-mock-exam-design.md`.

## 4.18 Gamification Modu (v0.17)

Roadmap #8: opt-in oyunlaştırma — ADHD beyni için görünür dopamin döngüsü. Anlatı tamamen prompt/TEACHING katmanında; kabuk yalnız toggle kalıcılığı + açılış streak beslemesi yapar ("ince kabuk").

- **Toggle + kalıcılık:** `/game on|off` USER.md `## Tercihler` bölümüne `- gamification: on|off` satırını yazar (kabuk-yönetimli `set_game_pref` — idempotent, dosyanın diğer içeriğini bozmaz). `/game` (argümansız) = durum bildirimi, **LLM'e gitmez**. On/Off `/exam` ile aynı enjeksiyon desenini kullanır: satır `[GAME MODE ON/OFF]` bilgi turuyla değiştirilip normal ask akışına bırakılır → model TEACHING.md kurallarını o noktadan itibaren uygular.
- **Anlatıyı model yapar, kabuk saymaz:** XP müfredat durumlarından (görüldü 10 · oturdu 25 · derinleşildi 50) + süreç puanlarından (oturum +5, tahmin +2, egzersiz teslimi +10 — doğruluktan bağımsız) türetilir; seviye eşikleri 0/100/250/500/1000/2000 (Çırak → Usta); rozetler gap kapanışı / ilk egzersiz / 7-gün streak / ilk boss; `/exam` = boss fight.
- **Açılış [GAME] beslemesi:** oyun açıkken kabuk açılış turuna `history.md`'den tek satır ekler (`game_streak_line`): streak>0 → `streak: N day(s) (longest M)`; kırık seri → yalnız `longest streak: M day(s)`. **ADHD-safe kod garantisi:** `streak: 0` yapısal olarak üretilemez (test-kilitli) — bir prompt kuralı değil, kabuk garantisi.
- **Kapanış koruması:** `closing_prompt` profil kuralına tek cümle — `## Tercihler` bölümü kabuk-yönetimli, olduğu gibi korunur. **Kabuk restore garantisi:** "kabuk-yönetimli" artık yalnız prompt-korumalı değil — kapanış flush'ında kabuk `- gamification:` satırının disk durumunu yazımdan ÖNCE yakalar, profil yazıldıktan SONRA model KEEP kuralını ihmal edip satırı düşürmüş veya değerini çevirmişse (`restore_game_pref`) tercihi geri yazar; kullanıcı hiç toggle etmemişse (satır yok) dokunmaz.
- **Kural evi: TEACHING.md `## Gamification`** (embedded, pedagoji katmanı — GOAL.md değil; yalnız `- gamification: on` iken aktif, kapalıyken tek oyun kelimesi yok). DOZ: kilometre taşında tek satır, her mesajda skor YOK. Overjustification bekçisi: puan süreçte, ceza mekaniği yok.
- **Kapsam dışı:** kabuğun XP hesabı/persist'i, lider tablosu, görsel rozet, ayrı oyun-veri dosyası (seviye müfredattan türetilir — idempotent).

Tasarım detayı: `docs/superpowers/specs/2026-08-15-gamification-design.md`.

## 4.19 TUI Tasarım Sistemi (v0.18)

Onaylı tasarım sistemi (Claude Design projesi) koda uygulandı — davranış değişmez, yalnız sunum. Amaç: sakin ekran (ADHD), renk semantiği, renk-körü güvenliği, monokrom dayanıklılığı.

- **Tek kaynak `src/tui/theme.rs`:** tüm TUI modülleri (+ `ui.rs` plain-ANSI, termimad skin) renk/glifi buradan alır — dağınık `Color::` literal'leri temizlendi. Renkler `Color::Indexed` (truecolor terminalde de doğru).
- **Semantik palet + glif çiftleri** (renk körlüğü/monokrom — renk yalnız glifi pekiştirir, tek başına anlam taşımaz):

  | Rol | Renk | Glif |
  |---|---|---|
  | Marka / kimlik | turuncu 208 | `●` bullet · `❯` prompt |
  | Bilgi / ortam | dim 244 | `·` |
  | Başarı | yeşil 149 | `✓` |
  | Uyarı | amber 179 (eski `Color::Yellow` öldü) | `⚠` |
  | Hata | kırmızı 210 | `✗` |
  | Oyun / XP | mor 141 | `▸` |
  | Kod (inline) | yeşil 114 | — |

- **Turuncu disiplini:** durağan ekranda ≤2 turuncu öğe (logo bloğu = 1) — test-kilitli (`welcome_orange_discipline`). Turuncu = kimlik, asla statü.
- **Kutu/gösterge dili:** canlı çerçeveler yuvarlak `╭╮╰╯`; tablo başlığı altı ince `─` çizgi; gauge `▓░`, ≥%70'te amber; spinner `⠋⠙⠸⠴` ~120ms; exam ilerleme `●○`.
- **Notice katmanları:** `page_notice` `·` dim · `page_warn` `⚠` amber · `page_error` `✗` kırmızı — mevcut metinler aynen, yalnız ön-ek + stil. `ui::warn` buffer'ı flush'ta amber katmana yönlenir.
- **Exam kartı kabukta DEĞİL:** GOAL.md `## Mock Exams`'a format kuralı eklendi (`── Question N/M ──` başlık, `●○` ilerleme, kırılım tablosu) — model çizer, kabuk parse etmez ("ince kabuk"). Game satırı glif notu TEACHING.md DOZ kuralına (`▸`).

Tasarım detayı: `docs/superpowers/specs/2026-08-16-tui-design-apply-design.md`.

## 5. Akış (bir öğrenme oturumu)

```
usta start rust-takvim
  → domain algıla → yaklaşım seç (spek gerekli mi? değil mi?)
  → PARÇA başı: SORAR ("bu parça için spek'in ne?")
      → sen mini-spek yazarsın
      → birlikte yorumlarsınız
      → sen kod yazarsın
  → dosya kaydını izler → proaktif, proje-temelli feedback
  → bilmezse → araştırır → sonra öğretir
  → parça biter → progress + gaps + curriculum güncelle
  → sonraki parça
```

**Parça-başı spek:** spek hiçbir zaman dev baştan-belge değil. Her parçanın başında küçük, o parçaya ait. Yap-geç, sonraki parçada tekrar mini-spek. Felç bu kadans ile çözülür.

## 6. Mimari — "ince kabuk, kalın beyin"

- **Rust = ince kabuk:** CLI, LLM backend, dosya izleme (`notify` crate), web araştırma, sağlık denetimi.
- **Zekâ + kişilik = markdown dosyalarında** (headspace deseni). Davranış değiştirmek = markdown düzenle, Rust'a dokunma.
- **Takılabilir LLM backend (her ikisi de destekli — kimisinde API var, kimisinde yok):**
  - **CLI (default):** yerel `claude` CLI (Claude Code) → mevcut auth/abonelik, **API key yok, token faturası yok**. `--allowedTools WebSearch` araştırmayı açar + "dokunmaz"ı araç seviyesinde zorlar.
  - **API (opsiyonel):** `ANTHROPIC_API_KEY` ile Anthropic Messages API (reqwest), model `claude-opus-4-8`, server-side web_search, adaptive thinking.
  - Seçim: `USTA_BACKEND=cli|api` öncelikli; yoksa `claude` PATH'te → CLI, yoksa key varsa → API.
- **Çağrı:** non-streaming (raw reqwest'te client timeout yok → sağlam). Streaming sonraki sürüm. CLI backend oturumu `--resume <session_id>` ile sürdürür — ilk çağrı `--output-format json`'dan id yakalar, sonraki turn'ler yalnız yeni mesajı gönderir (stale oturumda tam transcript'e düşülür).

## 7. Dosya Yapısı (wiki-linkli)

```
usta/
  SPEC.md                # bu dosya
  USTA.md                # çekirdek davranış: Socratic, dokunmaz, uydurmaz, senior
  learner/
    index.md             # TÜM öğrenme başlıkları kataloğu — "## Kayıtlar" bölümü kapanışta otomatik upsert edilir (v0.4)
    profile.md           # kullanıcı: ADHD, "suya gir", kişilik, iletişim tarzı
    progress/
      rust.md            # başlık-başı ilerleme + seviye (tekrar anlatmasın)
      javascript.md
    gaps/
      rust.md            # tespit edilen eksikler + kanıt
    curriculum/          # gap'lere göre planlanan dersler + müfredat haritası — v0.6'dan itibaren proje-lokal .usta/learner/curriculum/<konu>.md
      rust.md            # görülmedi/görüldü/oturdu/derinleşildi durumlu konu ağacı
    tech-notes.md        # (opsiyonel, sonra) araştırılan teknoloji notları — iki kez araştırmasın
  approaches/
    software.md          # spek + mimari (ölçek okuma) + teknoloji seçimi + kod kalitesi
    marketing.md         # brief/hipotez/ölçüm
    _default.md          # "spek gereksiz, direkt yap" mantığı
  projects/
    rust-takvim/         # aktif iş bağlamı, per-slice mini-spekler
  mentor/                # KULLANICIYA DÖNÜK, görünür — proje root'unda (.usta DIŞINDA)
    PROJECT.md           # proje tanımı (Ne/Neden/Ölçek/Stack/Kapsam Dışı) — Usta tanışmadan yazar, kullanıcı editler
    PROGRESS.md          # proje durumu (Bitti/Yapılıyor/Sırada) + append-only Kararlar — kapanış flush'ı yazar, reset dokunmaz
  src/                   # Rust: cli, claude client, watcher, research, health
  Cargo.toml
```

**İzolasyon prensibi (headspace'ten):** her öğrenme başlığı tam izole — Rust oturumunda JS eksikleri karışmaz. `index.md` hepsini üstten bağlar. `profile.md` paylaşılır (kullanıcı hep aynı).

## 8. Eş Zamanlı Çok Başlık

- Paralel aktif başlıklar (bugün Rust, yarın JS — ikisi de AKTİF).
- Usta oturumda hangi başlıktaysan o başlığın bağlamını yükler.
- Çoklu terminal: bir terminalde Rust, öbüründe JS. Ayrı bağlam, ortak `profile.md`, ayrı progress.

## 9. Hafıza & Durum

- **Kalıcı (v0.2'de gerçeklendi).** Oturum kapanışında (`/quit`, Ctrl-C, Ctrl-D) Usta oturumu özetleyip `.usta/learner/progress/<konu>.md`'yi tam içerik olarak yeniden yazar (atomik: tmp+rename). Sonraki oturum bu dosyayı system prompt'a yükler → tekrar anlatmaz. Boş oturum dosyaya dokunmaz.
- **Çoklu terminal (MVP sonrası):** ortak beyin, eşzamanlı yazım nadir → sağlamlaştırma sonraya.

## 10. MVP Sınırı

**İçinde:** sohbet döngüsü (proje seç → sor → sen yaz → Socratic feedback) + **dosya izleme** (proaktif kod feedback) + **araştırma** (uydurmama).

**Dışında (sonraki sürümler):** çoklu terminal sağlamlaştırma · model routing · marketing dışı çok-domain cilası · `tech-notes` cache · kendini-güncelleyen tech sistemi.

## 11. Alınan Kararlar

**(v0.2)**

- **Dosya izleme granülaritesi:** 1 sn debounce (son kayıttan itibaren). İlk görüşte tam içerik, sonraki kayıtlarda unified diff, 64KB üstü dosya izleme dışı (tek seferlik yerel uyarı).
- **Proaktiflik:** girdi ayrı thread'de (rustyline + ready el-sıkışması), ana döngü `tokio::select!` — feedback için Enter beklenmez.
- **Pedagoji tetikleri (v0.3):** açılış drilli shell'den tetiklenir (progress boş değilse); `cargo check` sonucu LLM'e `[... SADECE SENİN GÖZÜN İÇİN ...]` bloğuyla gider — saklama/tahmin kararı USTA.md kuralında, kodda değil.
- **Global USTA.md güncellemesi (v0.3):** scaffold var olan dosyanın üstüne yazmaz — davranış güncellemesinden sonra `rm ~/.config/usta/USTA.md` + bir kez `usta` çalıştırmak gerekir. Bilinçli kabul; dosya versiyonlama v0.4 adayı.
- **Katalog formatı (v0.4):** `learner/index.md` sonunda `## Kayıtlar`; satır `- konu | proje-yolu | YYYY-MM-DD`; bölüm-üstü serbest metin korunur; tarih `chrono` ile yerel saat.
- **Reset onayları (v0.4):** konu reseti `[e/H]`, factory reset kelime onayı ("evet"); stdin kapalı/boş = hayır (güvenli varsayılan). Reset komutları backend'siz çalışır.
- **Sunum katmanı (v0.5):** roller ikonla ayrılır — `●` (turuncu 208) Usta bloğu, `■` kullanıcı promptu, soluk `·`/`!` sistem bildirimi. Usta yanıtları termimad ile markdown render edilir; LLM beklerken spinner. TTY değilse veya `NO_COLOR` set'liyse düz çıktı (pipe/test uyumu). Davranış katmanına dokunulmadı.
- **Konu girişi (v0.5, sonra rafine):** TTY promptunda **ret yok** — kısa yaz ya da cümleyle anlat. Kısa girdi (≤2 kelime) yerel slug'lanır (Türkçe sadeleştirme + tire; "temel Linux güvenliği" → `temel-linux-guvenligi`). **Cümle yazılırsa modele bir kısa çağrı** ne öğrenmek istediğini çıkarıp en mantıklı slug'ı seçer ("ben rust ile bir todo yapmak istiyorum" → `rust-todo`); format `slugify_topic`'le garantilenir, çağrı hatasında yerel slug'a düşülür. Seçilen slug bildirilir. Detay yine sohbette. `usta start <konu>` ve pipe davranışı değişmedi.
- **Her-konu (v0.6):** yaklaşım dosyaları elle değil ilk-oturum tanışmasıyla üretilir; curriculum proje-lokal (`.usta/learner/curriculum/`) yaşar — §7'deki global `learner/curriculum/` yerine (izolasyon: harita da konu+proje bağlamına ait). Kapanış bölücü formatı `===DOSYA: <ad>===`.
- **Bağlam (v0.7):** pencere **modele göre türetilir** (`backend.context_window()`: opus/sonnet/fable 1M, haiku 200k) — sabit değil; kompaksiyon eşiği %70, korunan kuyruk 4 mesaj; ölçüm = son çağrının `usage` toplamı (input + cache_read + cache_creation) — ayrı sayaç tutulmaz, kaynak API/CLI raporudur.
- **Hedefli öğrenme (v0.8):** hedef ayrı mod değil approach alanı; tarih referansı system prompt `TODAY` bölümünden (`load_system_prompt` `today` parametresi aldı — model saati güvenilmez). Tempo/ölçüm progress'te yaşar, kod tarafında hedef mantığı YOK (ince kabuk korundu).
- **Sağlamlaştırma (v0.9):** transcript/lock hataları warn-and-continue (ana akışı asla kırmaz); batch tavanı 5; yedek tek nesil (`.bak`); yarım oturum otomatik işlenmez, sadece bildirilir (kurtarma kullanıcı kararı — YAGNI).
- **Konu girişi TUI'de (v0.11):** konusuz `usta` interaktif yolda önce kimlik-welcome (logo + kayıtlı konular) basar, sonra girdi kutusundan konuyu sorar — Claude tarzı "welcome üstte, soru altta". `usta start <konu>` tam-mod welcome (öğrenme durumu) + doğrudan drill. Slug çözümü TUI içinde (≤2 kelime yerel `slugify_topic`, cümle → `SLUG_SYSTEM` LLM + spinner, `finalize_slug`). Konu-bağımlı kurulum `build_session` yardımcısında (system+session+lock+recorder+has_progress) — hem TUI hem plain paylaşır; `run` artefaktları döndürür, kapanış `main`'de ortak. Lock-çakışma onayı TUI'de tek-tuş. Plain yol (`NO_COLOR`/pipe) birebir korundu (rustyline `resolve_topic`). **Gömülü default profil isimsiz** — yeni kullanıcı jenerik karşılanır (kişisel kimlik seed'den çıkarıldı). Detay: `docs/superpowers/specs/2026-08-07-tui-topic-entry-design.md`.
- **Arayüz — inline TUI (v0.10):** interaktif yol Claude Code tarzı ratatui `Viewport::Inline`'a taşındı — alt bölge (canlı girdi kutusu + durum satırı: spinner + bağlam göstergesi) sürekli çizilir, kalıcı içerik (açılış kutusu, Usta yanıtları, dosya feedback'i) `insert_before` ile normal **scrollback**'e iner. **Alternate screen YOK** — terminal geçmişi korunur (yukarı kaydır/kopyala). Girdi rustyline yerine crossterm `EventStream` + `tui-input`; LLM beklerken iç `select!` spinner döndürür, Enter kilitli (tek turn). v0.5'in `●`/`■`/markdown görsel dili korundu ama artık TUI akışında yaşar. **Plain yol (`ui::is_plain()`: TTY yok / `NO_COLOR`) birebir eski davranış** — rustyline döngüsü aynen; TUI hiç açılmaz (pipe/CI/test güvenli). Kompaksiyon/flush çıktısı TUI'de `TUI_ACTIVE` gate ile izole (raw-mode'da stdout kirletmez; spinner no-op, notice buffer→viewport). Detay tasarım: `docs/superpowers/specs/2026-08-07-tui-interface-design.md`.
- **İlerleme özeti (v0.15):** `history.md` global ve append-only — proje-lokal değil, çünkü streak "herhangi bir konu"da ardışık gündür (izolasyon ilkesine istisna, çünkü motivasyon sinyali konu-üstü). `current streak: 0` yasağı kod seviyesinde zorlanır (`render_stats` saf fonksiyonu test-kilitli) — ADHD-safe ton bir prompt kuralı değil, kabuk garantisi. Sürüm: `0.15.0`.
- **Deneme sınavı (v0.16):** `/exam` statik intercept değil, prompt-enjeksiyon komutu — kabuk yalnız hedef kapısını tutar (`## Hedef` yoksa LLM'e hiç gitmez), sınavın kendisi (soru üretimi, tek-soru akışı, askıya alınan ipucu merdiveni, skor + kırılım) tamamen `exam_prompt` enjeksiyonu ve GOAL.md `## Mock Exams` kuralları üzerinden LLM'de akar — "ince kabuk" ilkesi korundu. Sürüm: `0.16.0`.
- **Gamification (v0.17):** `/game` statik intercept değil, `/exam` gibi prompt-enjeksiyon komutu — kabuk yalnız toggle kalıcılığını (USER.md `## Tercihler`, `set_game_pref` idempotent) ve açılış streak satırını (`game_streak_line`; `streak: 0` yapısal olarak üretilemez — ADHD-safe kabuk garantisi) tutar; XP/seviye/rozet anlatısı tamamen TEACHING.md `## Gamification` kuralları üzerinden LLM'de akar ("ince kabuk"). Sürüm: `0.17.0`.
- **TUI tasarım sistemi (v0.18):** görsel dil tek kaynağa (`src/tui/theme.rs`) çekildi — semantik renk `Color::Indexed` + glif çiftleri, tüm TUI modülleri + `ui.rs` plain-ANSI + termimad skin buradan beslenir (dağınık `Color::` literal'i yok). Renk statü değil glifi pekiştirir (renk-körü/monokrom güvenliği); turuncu = kimlik, durağan ekranda ≤2 (test-kilitli). Exam kartı kabukta çizilmez — GOAL.md `## Mock Exams` format kuralıyla model çizer, kabuk soru-durumu parse etmez ("ince kabuk" korundu). Davranış/metin değişmez, yalnız sunum. Sürüm: `0.18.0`.

## 12. Açık Karar Noktaları (implementasyon planında netleşir)

- `approaches/*` şablonlarının tam formatı (domaine göre yapılandırma-adımı temsili).
- Örnek/pseudocode sınırı: "kavramı gösteren minik illüstrasyon OK, projene çözüm yazmak yasak" — pratikte nasıl zorlanır (prompt kuralı).
- Araştırma aracı: hangi web arama/fetch mekanizması.
