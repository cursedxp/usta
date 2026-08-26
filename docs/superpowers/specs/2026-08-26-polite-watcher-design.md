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

## v0.24.1 Düzeltmeler (2026-08-26 — Anil'in canlı oturumu + final review bulguları)

Canlı doğrulama (stagit oturumu, 0.24.0): mentorun iki sorusu açıkken `cargo new` → feedback anında bastı. Kök neden aşağıdaki F1. Ek olarak dizin event'i gürültüsü görüldü (`file feedback skipped: .../src: Is a directory (os error 21)`).

- **F1 (I1) — Backstop penceresi kuyruğun kurulma anına çapalanır.** `PoliteQueue` boş kuyruğa ilk push'ta `armed_at: Option<Instant>` damgalar (`drain` temizler). `backstop_deadline(armed_at: Option<Instant>, last_key: Instant) -> Option<Instant>` = `armed_at.map(|a| a.max(last_key) + POLITE_BACKSTOP)` — pencere HİÇBİR zaman kuyruklama anından kısa olamaz. Bu, `db0b47b`'nin ateşleme-sonrası elle `last_key` bump'ını kapsar → o iki satır kaldırılır (drain → armed_at sıfırlanır, sonraki push taze pencere açar).
- **F2 (I2) — `/watch off` bekleyen kuyruğu da susturur.** Watch kapatılırken kuyruk işlenmeden boşaltılır; boşaltılan path'ler `files.observe` ile senkronlanır (mevcut `!watching` dalıyla aynı baseline davranışı). Backstop select guard'ına `watching &&` eklenir.
- **F3 (I3) — Dokümantasyon.** `help.rs` yardım metnine `/watch polite [on|off]` satırı; README'ye polite mode bölümü (default davranış + `watch: live` approach anahtarı + `/watch polite off`). İngilizce.
- **F4 — Dizin event'leri sessiz.** Watcher dizin path'lerini kaynağında eler (`path.is_dir()` → gönderme); yarış artığına karşı `is_silent_skip` `ErrorKind::IsADirectory`'yi de sessiz sınıflar (NotFound/InvalidData deseninin aynısı; rustc 1.98, IsADirectory 1.83'te stabil).
- SPEC.md §4.21'deki "her backstop penceresi 180 sn" cümlesi F1 ile doğru hale gelir — değişiklik gerekmez; yine de kontrol edilir.

## v0.24.2 Düzeltmeler (2026-08-26 — v0.24.1 final review bulguları; Anil kararı: polite-off kuyruğu HEMEN İŞLENİR)

- **G0 (ön-koşul) — Routing çıkarımı.** Watcher dalındaki 4-yollu karar saf fonksiyona çıkarılır: `pub(crate) enum Route { Bulk, ObserveOnly, Queue, Feedback }` + `pub(crate) fn route(batch_len: usize, max_batch: usize, watching: bool, polite: bool, question_open: bool) -> Route` (`polite.rs`'te; sıra: bulk → !watching → polite&&question_open → feedback). Saf refactor — davranış birebir; run.rs `match` ile küçülür (600/600 kilidini açar) ve F2'nin `watching` kapısı birim-test edilebilir olur.
- **G1 — `/watch polite off` bekleyen kuyruğu HEMEN işler.** Anil kararı: polite'ı kapatmak "anında feedback istiyorum" demek — slash dalında polite false'a düşerken kuyruk doluysa `process_paths(pq.drain())` o anda koşar (`.await` mevcut Submit dalında zaten kullanılıyor). Notice: `polite mode off — delivering queued feedback` (kuyruk boşsa mevcut mesaj).
- **G2 — Mesaj/dokümantasyon düzeltmeleri.** `/watch off` kuyruk doluyken mesaja `(pending feedback dropped)` eklenir · README "never lost" ifadesi düzeltilir (`/watch off` düşürür; polite off teslim eder) · README/SPEC "180s of inactivity" netleşir: pencereyi yalnız TUI tuş vuruşları uzatır, editör kayıtları uzatmaz · SPEC.md:249 "exactly the pre-v0.24 behavior" cümlesi F4 istisnasıyla düzeltilir (dizin eleme her modda geçerli).
- **G3 — Küçük teknik borç.** `Cargo.toml`'a `rust-version = "1.83"` (F4 `ErrorKind::IsADirectory` MSRV'yi yükseltti) · watcher temp-dir testlerine `std::process::id()` soneki (polite.rs testleriyle tutarlı).
- Kapsam dışı (değişmedi): `FileFeedback::{Sessiz,Bildirim,Yanit}` legacy rename ayrı iş.

## Kapsam dışı

- Plain yol (`plain.rs` — pipe/CI): bugünkü davranışında kalır · sınav modu zaten ayrı bir akış, dokunulmaz · mod değişikliğinin approach dosyasına otomatik yazımı yok · LLM'e "cevap bekliyorum" protokol bayrağı yok (heuristik yeter, prompt diet korunur) · kuyruktaki diff'i kullanıcının cevabına iliştirme (ayrı tur olarak işlenir — ileride istenirse ayrı tasarım).
