# Tasarım — Polite Watcher: Soru Açıkken Dosya Feedback'i Kuyruğa (v0.24.0)

**Tarih:** 2026-08-26
**Kapsam:** Watch mode'a `polite` bayrağı: mentorun açık (cevaplanmamış) sorusu varken dosya-değişikliği feedback'i araya girmez — kuyruğa alınır, kullanıcının cevabından sonra (veya ~3 dk hareketsizlik backstop'unda) normal yoldan işlenir. Bildiğin konularda bugünkü anında-feedback davranışı approach dosyasındaki `watch: live` satırıyla veya `/watch polite off` ile korunur.
**Durum:** Onaylandı → implement (Anil onayı 2026-08-26)

## Gerekçe (bug)

- Sokratik akışta mentor soru sorup aynı mesajda "şunu paralelde dene" diyebiliyor. Kullanıcı editöre geçip deniyor; cevabını daha yazmadan watcher tetikleniyor ve feedback turu açılıyor — açık soru havada kalıyor, öğrenme bölünüyor.
- Mekanizma: `run.rs` watcher dalı (debounce → `handle_file_change`) diyalog durumuna hiç bakmıyor; tek kapı `watching` bayrağı + bulk-batch limiti. Session'da "cevap bekleyen soru" durumu yok (`Status` yalnız Idle/Thinking).
- SPEC §11 "Enter beklemeden feedback" kararı bilinçli — proaktiflik ölmez, yalnız soru-açık penceresinde ertelenir. Bilinen konuda (örn. JS) kesinti istenir → mod konu bazlı seçilebilir.

## İsimlendirme (bağlayıcı — tüm yeni kod İngilizce)

Repo english-base (2026-08-12 migration): yeni kod, tip/fonksiyon adları, kullanıcıya dönük metinler, kod yorumları ve commit mesajları İNGİLİZCE. Bu tasarımın kod adları:

- Modül: `src/tui/polite.rs` · Bayrak: `polite: bool` · Kuyruk tipi: `PoliteQueue` (`push` / `is_empty` / `drain`)
- Soru-açık durumu: `question_open: bool`, tespit yardımcısı `question_open(text: &str) -> bool`
- Backstop sabiti: `POLITE_BACKSTOP` (Duration, 180 sn) · deadline yardımcısı `backstop_deadline(...)`
- Approach parse: `live_from_approach(text: &str) -> bool` · approach anahtarı: `watch: live`
- Slash: `WatchCmd::{PoliteOn, PoliteOff, PoliteToggle}` + `apply_polite(...)`
- (Mevcut `FileFeedback::Sessiz/Bildirim/Yanit` Türkçe varyantları legacy — bu işin kapsamında rename YOK, yenisi eklenmez.)

## Davranış

- **İki ortogonal bayrak:** `watching: bool` aynen kalır (kapalı = hiç feedback). Yanına `polite: bool` gelir:
  - `watching && !polite` → bugünkü davranış BİREBİR (anında feedback).
  - `watching && polite` → aşağıdaki soru-açık kapısı devrede. **Default: polite = true.**
- **Soru-açık tespiti (heuristik):** son asistan mesajı `?` içeriyorsa VE kullanıcı o mesajdan sonra hiç mesaj göndermediyse → soru açık. Asistan yanıtı ekrana düşerken bayrak kurulur (`?` var mı), kullanıcı bir mesaj gönderince temizlenir. Dosya-feedback yanıtları da asistan mesajıdır — onların sorusu da bayrağı kurar (tutarlı).
- **Kuyruk:** soru açıkken debounce batch'i LLM'e gitmez; path'ler sıra-korumalı dedup'lu kuyruğa eklenir (diff İŞLEME ANINDA FileMemory'den üretilir — mevcut `handle_file_change` zaten path alıp diff'i kendisi çıkarır, kuyrukta içerik bayatlamaz). Kuyruğa İLK ekleme anında tek dim notis: `change noticed — feedback after your answer`. Sonraki eklemeler sessiz. Bulk-batch limiti (`max_feedback_batch`) kuyruk boşalırken de aynen uygulanır.
- **Kuyruk boşalma (flush) anları:**
  1. Kullanıcı mesajını gönderip mentorun o tura yanıtı tamamlanınca — KOŞULSUZ (yeni yanıt yine soru içerse bile bekleyenler işlenir; yeni gelen değişiklikler yeniden kapıya tabidir).
  2. **Backstop:** kuyruk doluyken son tuş basımından ~180 sn geçtiyse — soruyu unutup kodlamaya dalınca Usta sessiz kalmaz. Süre sabiti tek yerde (`defaults.rs`'e bak, mevcut sabitler nasıl duruyorsa öyle).
  - Flush = bekleyen path'ler mevcut feedback yolundan (aynı kod: `handle_file_change` çevrimi) sırayla işlenir.
- **Mod seçimi:**
  - Konu default'u: konunun approach dosyasında (`.usta/approaches/<konu>.md`, proje override dahil — `brain.rs` çözümleme sırası) satır başında `watch: live` satırı varsa oturum `polite=false` açılır; yoksa `polite=true`. Yalnız `live` değeri tanınır; tanınmayan değer sessizce default.
  - Oturum içi: `/watch polite` (toggle), `/watch polite on`, `/watch polite off` — mevcut `/watch` ailesinin (slash.rs `WatchCmd`) genişlemesi. Oturumluk override approach dosyasına YAZILMAZ.
- **Görünürlük:** status satırındaki watch göstergesi polite durumunu da yansıtır (koda bak — mevcut gösterge neyse minimal ek, örn. `watch·polite`).
- **Yerleşim:** `run.rs` 595 satır (600 bütçesi) — kuyruk + soru-açık + backstop mantığı saf-test-edilebilir yeni modülde yaşar (örn. `src/tui/polite.rs`); `run.rs`'e yalnız ince bağlantı (select dalında kapı kontrolü + flush çağrıları + deadline dalı).

## Test

- Soru-açık heuristiği: `?` içeren/içermeyen asistan mesajı; kullanıcı mesajı sonrası temizlenme.
- Kuyruk: sıra-korumalı dedup (aynı path ikinci kez eklenmez, sıra bozulmaz); ilk eklemede notis / sonrakinde sessiz.
- Backstop deadline hesabı: kuyruk boş → deadline yok; dolu → son tuş + 180 sn; tuş basımı deadline'ı iter.
- Flush kararı tablosu: (polite, soru-açık, kuyruk-dolu) kombinasyonları → anında mı / kuyruk mu / flush mu.
- Slash: `parse_watch_command` polite varyantları (+ mevcut varyantlar kırılmaz), apply geçişleri.
- Approach parse: `watch: live` satırı var/yok/tanınmayan değer.
- Uçtan uca elle doğrulama: Anil (soru açıkken kaydet → notis + cevap sonrası feedback; `/watch polite off` → anında feedback; JS approach'ına `watch: live` → oturum live açılır).

## Kapsam dışı

- Plain yol (`plain.rs` — pipe/CI): bugünkü davranışında kalır · sınav modu zaten ayrı bir akış, dokunulmaz · mod değişikliğinin approach dosyasına otomatik yazımı yok · LLM'e "cevap bekliyorum" protokol bayrağı yok (heuristik yeter, prompt diet korunur) · kuyruktaki diff'i kullanıcının cevabına iliştirme (ayrı tur olarak işlenir — ileride istenirse ayrı tasarım).
