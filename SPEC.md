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
- Konu = öğrenme başlığı/dosyalama anahtarı (`progress/<konu>.md`). **Ne inşa ettiğin** (proje/hedef) klasör + sohbette söylenir — Usta parça-başı "spek'in ne?" diye sorar.

## 4.6 Pedagoji Katmanı (v0.3)

Öğretim yönü değil, **geri çağırma yönü** optimize edilir — kalıcı öğrenme kullanıcıdan çıkan üretimde olur (testing/generation effect):

1. **Açılış drilli:** oturum başında progress'teki "Geri çağırma soruları"ndan 2-3'ü sorulur (progress varsa shell tetikler, Usta ilk sözü alır). 2 dk ısınma — ADHD için düşük eşikli "suya girme" rampası.
2. **Anlat-modu (Feynman):** parça kapanışında roller döner — kullanıcı yazdığını açıklar; açıklamadaki boşluk gap sinyalidir (koddan iyi).
3. **İpucu merdiveni:** soru → kavram adı → pseudocode; kod asla (Sert Kural 1). Seviye yükseldikçe merdiven kısalır (fading); bir basamakta ~2 tur takılınca bir basamak inilir (ADHD dengesi).
4. **Tahmin protokolü:** kayıtta `cargo check` koşar (60 sn timeout, 4KB kırpma, Rust dışı projede sessiz atlanır); hata varsa Usta sonucu söylemez, önce tahmin ettirir (hypercorrection).
5. **Hata günlüğü:** progress'te `hata tipi | sayaç | son örnek`; 3+ tekrar = `GAP ADAYI` → curriculum'a mini-alıştırma önerisi.

Kuralların tamamı USTA.md'de yaşar; Rust sadece tetikler (açılış turn'ü, check koşucusu, progress formatı).

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
    index.md             # TÜM öğrenme başlıkları kataloğu (rust: AKTİF, js: AKTİF, marketing: duraklamış)
    profile.md           # kullanıcı: ADHD, "suya gir", kişilik, iletişim tarzı
    progress/
      rust.md            # başlık-başı ilerleme + seviye (tekrar anlatmasın)
      javascript.md
    gaps/
      rust.md            # tespit edilen eksikler + kanıt
    curriculum/
      rust.md            # gap'lere göre planlanan dersler/alıştırmalar
    tech-notes.md        # (opsiyonel, sonra) araştırılan teknoloji notları — iki kez araştırmasın
  approaches/
    software.md          # spek + mimari (ölçek okuma) + teknoloji seçimi + kod kalitesi
    marketing.md         # brief/hipotez/ölçüm
    _default.md          # "spek gereksiz, direkt yap" mantığı
  projects/
    rust-takvim/         # aktif iş bağlamı, per-slice mini-spekler
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

## 12. Açık Karar Noktaları (implementasyon planında netleşir)

- `approaches/*` şablonlarının tam formatı (domaine göre yapılandırma-adımı temsili).
- Örnek/pseudocode sınırı: "kavramı gösteren minik illüstrasyon OK, projene çözüm yazmak yasak" — pratikte nasıl zorlanır (prompt kuralı).
- Araştırma aracı: hangi web arama/fetch mekanizması.
