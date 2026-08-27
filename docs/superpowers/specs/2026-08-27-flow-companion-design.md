# Tasarım — Flow Companion: Kuyruk Yerine Akış-İçi Eşlik (v0.25.0)

**Tarih:** 2026-08-27
**Kapsam:** Polite watcher'ın kuyruk/bekletme modeli emekliye ayrılır; yerine "akış-içi eşlik": dosya değişikliği ANINDA ve batch-birleşik tek LLM turunda işlenir, feedback çerçevesi dersin akışının parçası olarak yazılır (adım kontrolü → sıradaki adım; açık soru canlı tutulur; scaffold geçiştirilir). Anil'in hedef döngüsü: *mentor ister → kullanıcı yapar → mentor kontrol eder → sıradaki aşama*.
**Durum:** Onaylandı → implement (Anil, 2026-08-27)

## Gerekçe

- v0.24.0'ın kuyruk ilacı yanlış teşhisti: asıl sorun feedback'in erken gelmesi değil, mentorun açık soruyu unutup **bağımsız kod incelemesi moduna** geçmesiydi. Kuyruk semptomu örttü, akışı bozdu: mentor "cargo new yap" deyince kullanıcının yaptığı adım kuyruğa takıldı, mentor adımı fark edemedi, "beraber ilerleme" öldü.
- `cargo new` gibi tek komut = çok dosya → her dosya ayrı LLM turu → kuyruk boşalınca üst üste yanıt spam'i, takip edilemiyor.
- Davranış zekâsı shell'den prompt'a taşınır — "thin shell, thick brain" (SPEC §6) ile hizalı.

## İsimlendirme (bağlayıcı — tüm yeni kod İngilizce)

- Batch işleme: `handle_batch_change(...)` (tek LLM turu, N dosya) — `handle_file_change`'in yerini alır veya onu sarar (koda göre).
- Çerçeve üretici: `flow_frame(...)` (polite=true) · mevcut düz inceleme çerçevesi live modda kalır (`feedback_frame`).
- Kaldırılanlar: `PoliteQueue`, `question_open`, `backstop_deadline`, `POLITE_BACKSTOP`, `armed_at`, `silence_queue_on_watch_off*`, `deliver_queue_on_polite_off`, `polite_off_delivery_notice`, `absorb_queue_into_batch`, `bulk_skip_absorbing_queue`, `Route::Queue` — testleriyle birlikte.
- Kalanlar: `watching`, `polite` (anlamı değişir: **çerçeve anahtarı**), `/watch polite [on|off]`, `watch: live`, `route()` (üç kollu: Bulk/ObserveOnly/Feedback), bulk limiti, debouncer, dizin filtresi, silent-skip sınıfları, status göstergesi (`watching·polite`).

## Davranış

- **Zamanlama tek tip:** watching açıkken her debounce batch'i ANINDA işlenir — soru-açık kontrolü, kuyruk, 180 sn backstop, cevap-sonrası flush YOK. LLM turu sürerken gelen event'ler `select!` sıralamasıyla doğal bekler (yeni mekanizma gerekmez).
- **Batch = tek tur:** debounce penceresindeki N dosya tek injected user turn'de birleşir: her dosya için payload (ilk görüşte tam içerik / sonrasında unified diff; `Skip`/`TooLarge`/silent-skip sınıfları dosya başına bugünkü gibi), tek çerçeve. Cargo check (prediction protokolü) dosya başına değil **tur başına bir kez**. Exercise işareti dosya başına korunur.
- **Çerçeve (polite=true, default) — `flow_frame`:** İngilizce, öz olarak şunları söyler: *"Bu değişiklik süregiden dersin parçası — bağımsız kod incelemesi DEĞİL. (1) Son mesajında kullanıcıdan bir adım istediysen ve bu değişiklik onu karşılıyorsa: kısaca doğrula, hataları göster, sıradaki adıma geç. (2) Cevaplanmamış sorun varsa cevabı hâlâ bekliyorsun — soruyu kısaca canlı tut, unutma. (3) İlk görüşte gelen tam-içerik dosyalar araç üretimi scaffold olabilir (cargo new şablonu gibi) — scaffold'ı tek cümleyle geç, satır satır inceleme; kullanıcının ELLE yazdığı değişikliğe odaklan. (4) Kullanıcı araya soru sorarsa cevapla, sonra görevi geri çağır."* Hint ladder ve mevcut pedagoji kuralları geçerli kalır.
- **Live (polite=false):** zamanlama aynı (zaten anındaydı); çerçeve bugünkü düz inceleme (`feedback_frame`) kalır. `watch: live` approach anahtarı ve `/watch polite off` bu çerçeveyi seçer. `/watch off` her şeyi kapatır (değişiklik yok).
- **`/watch polite off` anındalığı:** kuyruk kalktığı için "bekleyeni teslim et" akışı ve teslim mesajı da kalkar; komut yalnız çerçeve değiştirir, mesajı ona göre sadeleşir (örn. on: `polite mode on — companion follows your lesson flow`, off: `polite mode off — plain review feedback`). Kesin metin implementasyonda, İngilizce.

## Test

- `handle_batch_change`: N dosyalık batch → TEK injected turn; payload sırası deterministik; Skip/TooLarge dosyalar turdan düşer, tur boşalırsa LLM çağrısı yok; cargo check tur başına 1.
- `flow_frame`: dört kural metni pinli (adım kontrolü · soru canlı · scaffold · görev geri çağırma); exercise işaretiyle bileşimi; live çerçevesi değişmeden (mevcut `feedback_frame` testleri yeşil kalır).
- Kaldırma temizliği: kuyruk/backstop sembolleri kaynaktan silinmiş (pin testleri güncellenir — kaldırılan needle'lar çıkar, `handle_batch_change` + `flow_frame` needle'ları girer); `cargo build` ölü kod uyarısı 0.
- `route()` üç kollu doğruluk tablosu (Queue kolu silinmiş).
- Elle doğrulama (Anil): mentor adım verir → yap → mentor ANINDA görüp teyit + sıradaki adım · `cargo new` → tek toplu yanıt, scaffold tek cümle · araya soru → cevap + göreve dönüş · `/watch polite off` → düz inceleme tonu.

## Kapsam dışı

- Plain yol (plain.rs) — bugünkü davranışında kalır (tek-dosya işleme dahil; istenirse ayrı iş) · sınav modu · kullanıcının metin cevabıyla dosya değişikliğini aynı turda birleştirme (LLM turları yine ayrı; frame köprüyü kuruyor) · adım/görev durumunun shell'de tutulması (model transcript'ten bilir — thin shell).
