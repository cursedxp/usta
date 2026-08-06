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

- **Rust = ince kabuk:** CLI, Claude (Opus) client, dosya izleme (`notify` crate), web araştırma, sağlık denetimi.
- **Zekâ + kişilik = markdown dosyalarında** (headspace deseni). Davranış değiştirmek = markdown düzenle, Rust'a dokunma.
- **Model:** Opus (hepsi). Config **değiştirilebilir** tutulur → ileride Haiku/Sonnet routing eklenebilir.

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

- **Kalıcı.** `progress/` seviyeyi tutar, oturumlar arası hatırlar → tekrar anlatmaz.
- **Çoklu terminal (MVP sonrası):** ortak beyin, eşzamanlı yazım nadir → sağlamlaştırma sonraya.

## 10. MVP Sınırı

**İçinde:** sohbet döngüsü (proje seç → sor → sen yaz → Socratic feedback) + **dosya izleme** (proaktif kod feedback) + **araştırma** (uydurmama).

**Dışında (sonraki sürümler):** çoklu terminal sağlamlaştırma · model routing · marketing dışı çok-domain cilası · `tech-notes` cache · kendini-güncelleyen tech sistemi.

## 11. Açık Karar Noktaları (implementasyon planında netleşir)

- `approaches/*` şablonlarının tam formatı (domaine göre yapılandırma-adımı temsili).
- Örnek/pseudocode sınırı: "kavramı gösteren minik illüstrasyon OK, projene çözüm yazmak yasak" — pratikte nasıl zorlanır (prompt kuralı).
- Dosya izleme granülaritesi: her kayıtta mı, debounce mı, kullanıcı tetikli mi.
- Araştırma aracı: hangi web arama/fetch mekanizması.
