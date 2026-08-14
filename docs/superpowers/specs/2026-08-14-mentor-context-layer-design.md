# Tasarım — Proje Bağlam Katmanı (`mentor/`)

**Tarih:** 2026-08-14
**Kapsam:** Kullanıcıya dönük, görünür `mentor/` klasörü: `PROJECT.md` (proje tanımı) + `PROGRESS.md` (proje durumu + karar günlüğü). Açılışta system prompt'a girer, kapanış flush'ında güncellenir.
**Durum:** Onaylandı → writing-plans

## Amaç

Bugün proje bağlamı ("ne inşa ediliyor, hedef, ölçek, stack") hiçbir dosyada durmuyor — sohbette yaşıyor, oturum kapanınca uçuyor (SPEC madde 55: Usta parça-başı "spek'in ne?" diye soruyor). Projenin **durumu** (ne bitti, sırada ne var) da hiçbir yerde yok; katalog sadece `konu | proje-yolu | tarih` tutuyor. Öğrenci progress'i (`.usta/learner/progress/<konu>.md`) kullanıcının ne öğrendiğini tutar — **projenin** durumunu değil. Bu boşluk kapatılıyor.

Kararlar (brainstorm, 2026-08-14):
- **Usta doldurur** (USER.md pattern'i): dosya yoksa tanışmada sorar, kapanışta yazar; kullanıcı elle düzenleyebilir, düzenlemeleri korunur.
- **Görünür klasör:** `.usta/` gizli olduğu için kullanıcı görmez/editleyemez → kullanıcıya dönük dosyalar proje root'unda görünür klasörde yaşar.
- **Klasör adı `mentor/`** — `.usta/` ile isim ikizliği yok, CLAUDE.md/PROJECT.md gibi dosyalarla çakışmaz. `.usta/` iç mekanizma olarak aynen kalır.
- **İki dosya:** `PROJECT.md` (tanım — nadiren değişir) + `PROGRESS.md` (durum — her oturum güncellenir).
- **PROGRESS.md = işaretçi + karar günlüğü** (seçenek B): Bitti/Yapılıyor/Sırada üzerine yazılır; Kararlar birikir, silinmez. Tam tarihçe git'in işi.

## Yapı

```
proje-root/
├── mentor/              # görünür, kullanıcıya dönük — "senin dosyaların"
│   ├── PROJECT.md       # proje tanımı
│   └── PROGRESS.md      # proje durumu + karar günlüğü
└── .usta/               # gizli, iç mekanizma — DEĞİŞMİYOR
```

- `mentor/` proje-başına tektir, **konu-bağımsız** — aynı projede rust + git iki konu çalışılsa da tek PROJECT.md.
- Dosya içeriği oturum diliyle yazılır (default Türkçe); şablon başlıkları Türkçe (mevcut progress dosyası başlıklarıyla — `## Seviye` vb. — tutarlı).

### PROJECT.md şablonu (Usta üretir)

```markdown
# <Proje adı> — Proje Tanımı

## Ne
(1-2 cümle: ne inşa ediliyor)

## Neden
(hedef/motivasyon — öğrenme hedefiyle bağı)

## Ölçek
(1 kişilik mi 1000 kişilik mi — mimari öneriler buna demirlenir)

## Stack
(dil, araçlar, seçim gerekçeleri)

## Kapsam Dışı
(bilinçli yapılmayacaklar)
```

Domain-agnostik: kod-dışı konuda (marketing vb.) bölümler yaklaşıma göre esner — "Stack" yerine kanal/araç, "Ölçek" yerine bütçe/erişim olabilir; başlık iskeleti korunur, içerik domain'e uyar (SPEC madde 48 ruhu).

### PROGRESS.md şablonu (Usta üretir)

```markdown
# <Proje adı> — Durum

## Bitti
## Yapılıyor
## Sırada

## Kararlar
- YYYY-MM-DD | <karar> | <tek satır neden>
```

Bitti/Yapılıyor/Sırada: her kapanışta güncel durumla üzerine yazılır (işaretçi, günlük değil). Kararlar: yalnız eklenir, silinmez.

## Davranış

### 1. Scaffold

`ensure_scaffold` / `write_project_scaffold` (`src/main.rs:854` civarı) `.usta/` yanında **`mentor/` klasörünü de oluşturur** (boş). Dosyaları scaffold yazmaz — içerik tanışmadan gelir, kapanış flush'ı yazar (`write_atomic` zaten parent oluşturuyor; scaffold'daki boş klasör görünürlük için).

### 2. System prompt yüklemesi

`load_system_prompt` (`src/brain.rs:71`): proje root'u varken `mentor/PROJECT.md` ve `mentor/PROGRESS.md` etiketli bölüm olarak eklenir — USER.md (profil) bölümünden hemen sonra, `learner/index.md`'den önce (mevcut yükleme sırasına minimal dokunuş). Dosya yoksa sessizce atlanır (mevcut `read_section` davranışı).

Dikkat: mevcut kod `.usta/` altını yükler; mentor dosyaları **proje root'undan** okunur (`project.join("mentor/PROJECT.md")`), `.usta` altından değil.

### 3. Kapanış flush'ı

`closing_prompt` (`src/progress.rs:62`) iki yeni dosya slotu alır: `project` ve `project-progress` (mevcut delimiter mekanizması `===DOSYA: <name>===` + `split_files` değişmeden taşır). Mevcut içerikleri de prompt'a girer (elle düzenleme korunur kuralı aynen).

Üretim kuralları (closing prompt'a eklenir):
- `project`: yalnız dosya yokken (ilk tanışma sonrası) veya proje tanımı bu oturumda **esaslı** değiştiyse üretilir. Kullanıcının elle düzenlemeleri korunur. Proje konuşulmadıysa (salt kavram öğrenme oturumu) hiç üretilmez.
- `project-progress`: proje üzerinde iş yapılan her oturumda üretilir. Bitti/Yapılıyor/Sırada güncel durumla yeniden yazılır; `## Kararlar`a yalnız bu oturumda alınan proje kararları eklenir, mevcut satırlar asla silinmez. Oturumda proje işi yoksa üretilmez.

`flush_target` (`src/main.rs:381`) yeni isimleri yönlendirir:
- `"project"` → `<proje-root>/mentor/PROJECT.md`
- `"project-progress"` → `<proje-root>/mentor/PROGRESS.md`

Kapanış akışı (`src/main.rs:411-450`) iki dosyanın mevcut içeriğini okuyup `closing_prompt`'a geçirir, dönen dosyaları yazar.

### 4. Açılış davranışı

- **PROJECT.md yok** (`onboarding_prompt`, `src/progress.rs:140`): tanışma proje sorularını da kapsar — ne inşa edilecek, hedef, ölçek, stack. Mevcut kural aynen: shell dosyaları kapanışta yazar, oturum içinde dosya yazmaya kalkma.
- **PROJECT.md var** (`opening_prompt`, `src/progress.rs:124`): Usta proje bağlamını bilerek başlar — proje temellerini yeniden SORMAZ; açılış cümlesine PROGRESS.md `## Sırada`dan tek satır işaretçi ekler ("projede kaldığımız yer: ...").
- SPEC madde 55'teki "parça-başı 'spek'in ne?' sorar" davranışı → "önce PROJECT.md'ye bakar; orada yoksa sorar" olur.

### 5. Reset davranışı

`usta reset <konu>` ve `usta reset --factory` **`mentor/`'a DOKUNMAZ** — kullanıcıya dönük proje dokümanıdır, kullanıcının reposuna commit edilmiş olabilir; öğrenme state'i değil. (Kod değişikliği gerekmiyorsa test/dokümantasyonla sabitlenir.)

## Dokümantasyon güncellemeleri

- `SPEC.md`: madde 55 davranış değişikliği + dosya yerleşimi bölümüne `mentor/` eklenir.
- `USTA.md` / `approaches/software.md` (brain kuralları): proje bağlamını PROJECT.md'den okuma + parça-başı sormama kuralı işlenir (hangi dosyada "spek'in ne" kuralı yaşıyorsa orası).
- `README.md`: öne çıkanlar/kullanım bölümüne kısa not.

## Test

- `flush_target`: `"project"` → `mentor/PROJECT.md`, `"project-progress"` → `mentor/PROGRESS.md` (proje root'u altında, `.usta` altında DEĞİL).
- `closing_prompt`: iki yeni slot + kuralları içerir (`project` koşullu, `project-progress` Kararlar-silinmez kuralı) — mevcut string-assert test deseniyle.
- `split_files`: değişiklik yok (isim-genel zaten); yeni isimlerle round-trip testi.
- `load_system_prompt`: mentor dosyaları varken prompt'ta etiketli görünür; yokken bölüm hiç yok; sıra USER.md → mentor → learner/index.md.
- Scaffold: yeni projede `mentor/` klasörü oluşur.
- Reset: `reset --factory` sonrası `mentor/` dosyaları duruyor.

## Kapsam dışı

- `mentor/` altına başka dosyalar (spec, mimari doküman vb.) — YAGNI.
- Proje progress'inin git commit'lenmesi/otomasyonu — kullanıcının tasarrufu.
- Katalog (`learner/index.md`) formatı değişikliği.
- Visual explainer / TUI görsel değişikliği.
- `.usta/` yerleşiminde herhangi bir değişiklik.

## Açık sorular

Yok.
