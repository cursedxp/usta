# Spec — Yeni Konu Onayı (yanlışlıkla konu açmayı önle)

**Tarih:** 2026-08-07
**Durum:** Onaylandı (Anil: "yeni konuyu sorsun — ya devam edeceğim ya yeni konuya geçeceğim, önemli")
**Ön koşul:** `2026-08-07-topic-resume` planı UYGULANMIŞ olmalı — bu spec onun `TopicChoice::New` dalının üzerine kurulur.

## 1. Amaç

Konu girişi yeni konuya çözümlendiğinde (kullanıcı devam etmek istiyor olabilirken) sistem sormadan konu açmasın. Tek tuşluk onay: kullanıcı ya yeni konuyu onaylar ya konu seçimine geri döner (oradan Enter'la devam edebilir).

## 2. Davranış

**Tetik:** Konu seçimi `TopicChoice::New` ile sonuçlandı VE slug türetildi (yerel slug ya da LLM) VE proje-yerel konu listesi DOLU. (İlk oturumda — devam edilecek şey yokken — onay sorulmaz: gereksiz sürtünme.)

**TUI:**
```
· yeni konu: rust-cli — açayım mı? [e = evet / başka tuş = geri dön]
```
- `e`/`E` → yeni konu açılır (mevcut akış: "konu: rust-cli — detayı sohbette anlatırsın" + tanışma).
- Başka tuş / Ctrl-C → konu giriş döngüsüne GERİ dönülür (welcome yeniden basılmaz; sadece giriş sorusu tekrar aktif — kullanıcı Enter'la devam seçebilir, başka şey yazabilir, Ctrl-C/D ile çıkabilir).
- Onay `Resume` yollarının HİÇBİRİNDE sorulmaz (Enter/rakam/eşleşme/niyet → doğrudan devam).
- LLM slug'ı `Resume`'a çözümlenirse (mevcut konuya eşleşme) onay yok — devam.

**Plain yol:** Aynı koşulda mevcut `confirm()` yardımıyla:
```
Yeni konu 'rust-cli' açılsın mı? [e/H]:
```
- `e/evet` → aç. Aksi → "Konu nedir?" sorusuna geri dön (döngü). Pipe/boş-stdin yolu değişmez (`genel` — onay yok, etkileşim yok).

## 3. Kapsam Dışı

- Onay reddinde "hangi konuya devam?" alt-menüsü — geri dönüş yeterli, seçenekler zaten welcome'da.
- İlk oturumda (yerel konu yokken) onay.

## 4. Test + Doğrulama

- Onay mesajı üretimi saf fn (`new_topic_confirm_msg(slug)`) + unit test.
- Akış davranışı elle: (1) mevcut konu varken yeni konu yaz → onay çıkar → `e` → açılır; (2) aynı durumda başka tuş → giriş sorusuna dönülür → Enter → devam; (3) ilk oturumda onay ÇIKMAZ; (4) Enter/rakam/niyet devam yollarında onay ÇIKMAZ; (5) pipe yolu değişmez.
- Mevcut tüm testler yeşil.
