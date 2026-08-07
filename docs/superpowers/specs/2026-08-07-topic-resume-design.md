# Spec — Konu Devamlılığı (bare `usta` = kaldığın yerden devam)

**Tarih:** 2026-08-07
**Durum:** Onaylandı (Anil — canlı hata raporu üzerine)
**Kaynak olay:** Kapanış sonrası aynı klasörde `usta` açıldı; kullanıcı "kaldığımız yerden devam edelim" niyetindeydi. Sistem girdiyi YENİ konu olarak slug'ladı (`memory-bos-dir`), sıfırdan tanışma başlattı → "hafıza boş" hissi. Progress dosyaları aslında diskteydi — onlara giden seçim yolu yoktu.

## 1. Kök Neden

`ask_topic` (TUI) ve `resolve_topic` (plain) her girdiyi yeni konuya çevirir:

1. **Boş Enter yutulur** (`editor` boş satırda `Action::None`) — "devam" anlamına gelebilecek en doğal tuş ölü.
2. **Mevcut konuyla eşleşme kontrolü yok** — kullanıcı var olan konunun adını yazsa bile akış aynı; şans eseri aynı slug çıkarsa devam eder, cümleyle yazarsa yeni slug doğar.
3. **Devam-niyeti tanınmıyor** — "devam", "kaldığımız yerden" gibi girdiler slug'lanıp konu adına dönüşür.
4. **"Kayıtlı:" listesi yanlış kaynaktan** — global katalog (`~/.config/usta/learner/index.md`) gösteriliyor; devam edilebilir konular ise proje-yerel (`.usta/learner/progress/*.md`). Başka projenin konusu burada "kayıtlı" görünür ama devam edilemez (progress başka klasörde).

## 2. Amaç

Bare `usta` açılışında kullanıcı tek tuşla (Enter) son konusuna dönebilmeli; konu adını/niyetini yazarsa doğru konuya bağlanmalı; yeni konu akışı bozulmadan kalmalı.

## 3. Tasarım — iki katman

### K1: Deterministik devam (LLM'siz, önce bu denenir)

**Proje-yerel konu listesi:** `.usta/learner/progress/*.md` dosya adlarından (stem = slug). Sıralama: global index'teki `konu | proje | tarih` kaydının tarihine göre yeniden-eskiye (kayıt yoksa dosya mtime fallback). En yenisi = "son konu".

**Kimlik welcome sağ kolonu değişir:**

```
│ Ne öğrenmek istiyorsun?        │
│                                │
│ Enter → brainstorm-ilk-adim'e  │
│         devam                  │
│ 1) brainstorm-ilk-adim         │
│ 2) linux-guvenlik              │
│                                │
│ Yeni konu için yaz.            │
│ (Diğer projelerde: rust)       │
```

- Numaralı liste = SADECE proje-yerel konular (en çok 6).
- "Diğer projelerde:" = global katalogda olup bu projede progress'i olmayanlar — bilgi satırı, seçilemez (progress taşınmaz; o konuya kendi klasöründe devam edilir).
- Proje-yerel konu yoksa bugünkü görünüm aynen (ilk oturum).

**Girdi yorumlama sırası** (`ask_topic` dönüşü artık `TopicChoice`):

| # | Girdi | Sonuç |
|---|---|---|
| 1 | Boş Enter, proje-yerel konu VAR | `Resume(son_konu)` |
| 2 | Boş Enter, konu YOK | yut (bugünkü davranış) |
| 3 | Tek rakam `1..=N` (liste aralığında) | `Resume(liste[n-1])` |
| 4 | `slugify_topic(girdi)` proje-yerel bir konuya EŞİT | `Resume(o_konu)` |
| 5 | ≤4 kelime VE devam-kalıbı içeriyor: `devam`, `kaldigimiz`, `kaldigim`, `continue`, `resume` (deasciify sonrası substring) | `Resume(son_konu)` |
| 6 | Diğer her şey | `New(raw)` → mevcut yeni-konu akışı (≤2 kelime yerel slug, cümle → LLM) |

Kural 5'teki ≤4 kelime sınırı bilinçli: "devam edelim ama bu sefer docker öğrenelim" gibi uzun cümleler LLM'e gider (K2 yakalar ya da yeni konu doğru açılır).

### K2: LLM güvenlik ağı (cümle yolu)

`SLUG_SYSTEM` parametreli hale gelir — `slug_system(known_topics: &[String]) -> String`:

Mevcut talimat + şu ek (liste boş değilse):

> "Mevcut konular: brainstorm-ilk-adim, linux-guvenlik. Kullanıcının yazdığı bu konulardan birine DEVAM ETME isteğiyse (aynı işin sürdürülmesi, 'kaldığımız yer', önceki çalışmaya atıf) SADECE o konunun slug'ını aynen döndür. Yeni bir konuysa yeni slug üret."

`finalize_slug` sonucu proje-yerel bir konuya eşitse akış `Resume` sayılır (yeni-konu notice'ı yerine "devam" notice'ı, onboarding yerine drill).

### Devam seçilince

Mevcut mekanizma zaten doğru çalışıyor: `build_session` progress'i bulur → `has_progress=true` → açılış drilli. Değişen tek şey: notice metni `devam: <konu>` + tam-mod welcome (öğrenme durumu kutusu) basılır — `had_topic_arg` yoluyla aynı görsel.

### Plain yol (`resolve_topic`)

Aynı K1 kuralları (rakam seçimi dahil; liste tek satır metin olarak basılır: `kayıtlı: a, b — Enter = a'ya devam`). K2 zaten `derive_slug` üzerinden ortak.

## 4. Kapsam Dışı

- Yanlış açılmış konunun mevcut konuyla birleştirilmesi/taşınması (`usta reset <konu>` var).
- Başka projedeki konuya bu klasörden devam (progress proje-yerel — tasarım gereği).
- Konu listesi 6'dan uzunsa sayfalama.

## 5. Test Stratejisi

- `local_topics` sıralama + fallback: temp dizinde progress dosyaları + sahte index içeriğiyle saf test.
- `interpret_topic_input`: 6 kuralın her biri + Türkçe deasciify kalıpları ("kaldığımız" → "kaldigimiz") + sınır durumları (rakam aralık dışı → New, 5 kelimeli devam cümlesi → New/LLM).
- `slug_system(known)`: liste enjeksiyonu var/yok.
- Welcome render: numaralı liste + Enter satırı + diğer-projeler satırı; konu yokken eski görünüm (regresyon).
- Mevcut 137+ test yeşil; plain yol davranışı boş-stdin/pipe'ta değişmez (`genel` fallback aynen).

## 6. Elle Doğrulama

1. Konu çalış → çık → aynı klasörde `usta` → Enter → "devam: <konu>" + açılış drilli geliyor (tanışma DEĞİL).
2. `usta` → "kaldığımız yerden devam edelim" yaz → aynı konuya drill.
3. `usta` → `2` yaz → listedeki ikinci konuya devam.
4. `usta` → "docker compose öğrenmek istiyorum" → YENİ konu normal açılıyor.
5. Cümleyle devam niyeti ("dün başladığımız linux işini sürdürelim") → LLM mevcut slug'ı döndürüyor → devam.
6. Boş klasörde `usta` → ilk-oturum akışı aynen (regresyon).

## 7. Başarı Ölçütü

Elle doğrulama 1-6 geçer; "hafıza boş" senaryosu tekrarlanamaz — aynı klasörde progress varken tek Enter kullanıcıyı kaldığı yere döndürür.
