# Tasarım — Egzersiz/Artefakt Döngüsü (Roadmap #2)

**Tarih:** 2026-08-15
**Kapsam:** Watcher feedback döngüsünü kod dışına genelleme: Usta teslimat (egzersiz) atar, kullanıcı `exercises/` altına yazar, kaydettiği an aynı proaktif Socratic feedback döngüsü çalışır — her domain'de (GTM brief'i, Almanca kompozisyon, Rust snippet'i).
**Durum:** Onaylandı (Anil: "roadmap kalanını bitirelim") → writing-plans
**Bağımlılık:** Yok — watcher zaten uzantı-agnostik (`watcher.rs:59` "we don't filter by extension").

## Amaç

Bugün feedback döngüsü fiilen kod-merkezli: `handle_file_change` (`src/main.rs:1283-1290`) her dosyayı "project-grounded feedback on this change" çerçevesiyle sunuyor ve her kayıtta `cargo check` koşuyor. Kod-dışı konuda (GTM, dil) Usta'nın teslimat atayacağı, kullanıcının yazacağı, kaydedince egzersiz-olarak değerlendirileceği bir yer ve çerçeve yok. Roadmap #2 bunu açar: **egzersiz = izlenen dosya**, mevcut watcher altyapısı yeniden kullanılır — yeni izleme mekanizması YOK.

Kararlar:
- **Konvansiyon:** egzersizler proje root'unda görünür `exercises/` klasöründe yaşar (mentor/ gibi; scaffold kurar). Alt yapı serbest — Usta konuya göre `exercises/<konu>/<ad>.md` önerir ama zorlamaz (path'e bakarak tanır, alt klasör şart değil).
- **Atama = doğal dil.** `/exercise` komutu YOK (YAGNI). Usta sohbette teslimatı tanımlar ("şunu exercises/gtm/positioning.md'ye yaz"), dosya adını kendisi söyler. Sıfır-otonom-aksiyon korunur: dosyayı KULLANICI yazar.
- **Tanıma = path.** Kaydedilen dosya `exercises/` altındaysa feedback çerçevesi değişir: "değişikliğe feedback" değil, "atadığın egzersizin teslimatını değerlendir" — atamayla karşılaştır, ipucu merdiveni uygula, çözümü yazma.
- **`cargo check` egzersiz dosyalarında koşulmaz** — egzersiz teslimatı derlenecek kod değil; koşmak gürültü + gecikme. (Egzersiz olarak kod parçası yazılırsa bile teslimat dosyası bağımsız — check proje köküne bakar, alakasız sonuç üretir.)
- **Kalıcılık = mevcut progress mekanizması.** Açık (tamamlanmamış) egzersiz, kapanış flush'ında progress dosyasına `## Açık egzersiz` bölümü olarak yazılır (dosya adı + teslimat tanımı tek satır + atanma tarihi). Açılış drilli açık egzersizi hatırlatır. Yeni dosya/slot YOK — `closing_prompt`/`opening_prompt` kural metni değişikliği.
- **Pedagoji kuralları brain'de:** TEACHING.md'ye egzersiz protokolü (ne zaman atanır, nasıl değerlendirilir, fading) — kod değil markdown ("ince kabuk").

## Davranış

### 1. Scaffold (`src/main.rs` `write_project_scaffold`)

`mentor/` deseniyle birebir: görünür `exercises/` klasörü + `.gitkeep` oluşturulur, `results`'a eklenir.

### 2. Feedback çerçevesi (`src/main.rs` `handle_file_change`)

Path proje-root-göreli `exercises/` altındaysa (`path.strip_prefix(project_root)` sonrası ilk bileşen `exercises`; strip başarısızsa mutlak path'te bileşen araması — watcher mutlak path verir):

- FirstSight çerçevesi: `[Exercise submission saved: <path>]\n<full>\n\nThis is the user's deliverable for the exercise you assigned. Review it AS AN EXERCISE: compare against the assignment, apply the hint ladder (start high), point at what to reconsider — do NOT rewrite or complete it for them. If no exercise was assigned this session, treat it as spontaneous practice work and review it the same way.`
- Diff çerçevesi: `[Exercise submission changed: <path>]\nChange (unified diff):\n<diff>\n\nReview the revision AS AN EXERCISE iteration: did it address your previous feedback? Move one rung down the hint ladder only if they're stuck — never hand over the solution.`
- `run_check` bu path'lerde ÇAĞRILMAZ (erken `if` ile atlanır).
- `exercises/` dışı dosyalarda mevcut çerçeve harfiyen korunur (regresyon kırmızı çizgi).

### 3. Kalıcılık (`src/progress.rs`)

- `closing_prompt` `progress` kuralına ek cümle: progress yapısına `## Açık egzersiz` bölümü — yalnız bu oturumda atanan VE tamamlanmayan egzersiz varsa yazılır (`- <dosya> | <tek satır teslimat tanımı> | assigned YYYY-MM-DD`); tamamlanan egzersiz `## Kapatılanlar`a normal madde olarak girer, `## Açık egzersiz`den düşer.
- `opening_prompt` ek cümlesi: progress'te `## Açık egzersiz` varsa drill sonrası tek cümleyle hatırlat ("open exercise: X — continue or discuss it").

### 4. Pedagoji (TEACHING.md — embedded, defaults sync otomatik)

Yeni bölüm `## Exercise Loop`:
- Ne zaman atanır: bir kavram `görüldü`ye geçtiğinde pekiştirme için; kullanıcı "pratik istiyorum" dediğinde; harita bir sonraki adıma pratik gerektirdiğinde.
- Atama formatı: tek net teslimat + dosya yolu önerisi (`exercises/<konu>/<ad>.md`) + başarı ölçütü tek cümle ("iyi bir cevap şunu içerir").
- Değerlendirme: assignment'a karşı; ipucu merdiveni; çözüm YAZILMAZ (Hard Rule 2 egzersizde de geçerli); tamamlanınca kısa geri bildirim + curriculum durum güncellemesi (`görüldü → oturdu` adayı).
- Domain örnekleri: kod (snippet dosyası), yazı (brief/kompozisyon), terminal (komut çıktısını yapıştır) — hepsi dosya = teslimat.

### 5. Dokümantasyon

- `SPEC.md`: egzersiz döngüsü bölümü (konvansiyon, path-tanıma, check-atlama, Açık egzersiz).
- `README.md` (İngilizce): Highlights'a satır + Usage'a kısa örnek.
- `docs/ROADMAP.md`: #2 → ✅.

## Test

- `is_exercise_path` (yeni saf yardımcı, main.rs): `<root>/exercises/a.md` → true; `<root>/exercises/gtm/b.md` → true; `<root>/src/exercises.rs` → false; `<root>/mentor/PROJECT.md` → false; root dışı mutlak path + `exercises/` bileşeni → true (watcher mutlak verir).
- `handle_file_change` çerçevesi: egzersiz path'inde "AS AN EXERCISE" içerir, `cargo check` bloğu yok; normal path'te mevcut metin birebir (string assert'ler — fonksiyonun prompt kurulum kısmı test edilebilir yapıya çekilir: `feedback_frame(path_kind, payload) -> String` saf fonksiyonu).
- `closing_prompt`: "Açık egzersiz" kuralını içerir; `opening_prompt`: açık egzersiz hatırlatma cümlesini içerir.
- Scaffold: `exercises/` + `.gitkeep` oluşur.

## Kapsam dışı

- `/exercise` komutu, egzersiz kütüphanesi/şablonları.
- Otomatik değerlendirme puanı (gamification — roadmap #8'e).
- Spaced repetition bağı (roadmap #3'te ele alınır).
- Egzersiz dosyalarının arşivlenmesi/temizliği.

## Açık sorular

Yok.
