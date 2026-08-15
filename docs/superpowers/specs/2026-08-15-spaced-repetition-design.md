# Tasarım — Spaced Repetition (Roadmap #3)

**Tarih:** 2026-08-15
**Kapsam:** Geri-çağırma sorularına vade (due-date) + basitleştirilmiş SM-2 aralık merdiveni. Açılış drilli yalnız vadesi gelenleri sorar; welcome kutusu "bugün X madde vadesi geldi" gösterir.
**Durum:** Onaylandı (roadmap zinciri) → writing-plans
**Bağımlılık:** Yok. Mevcut altyapı: progress `## Geri çağırma soruları` bölümü, `welcome.rs drill_count` (satır 84), system prompt TODAY bölümü (brain.rs:76 — model bugünü biliyor).

## Amaç

Bugün drill her açılışta aynı havuzdan 2-3 soru seçiyor — zamanlama yok. Öğrenme bilimi: geri çağırma aralıkları büyüyerek tekrarlanırsa kalıcılık katlanır (spacing effect). Roadmap #3: sorulara vade ver, vadesi gelmeyeni sorma, geleni açılışta görünür kıl.

Kararlar:
- **Format (makine-okur kuyruk):** `## Geri çağırma soruları` maddeleri: `- <soru> — <tek satır cevap> | due: YYYY-MM-DD | ivl: <gün>`. `due`/`ivl` kuyruğu olmayan eski madde = bugün vadeli sayılır (migrasyon: ilk kapanışta model kuyruk ekler).
- **Basitleştirilmiş SM-2 (ease factor YOK):** aralık merdiveni gün cinsinden `1 → 3 → 7 → 16 → 35 → 90`. Drill'de rahat hatırlandı → bir üst basamak, `due = bugün + yeni ivl`. Zorlandı/yanlış → basamak 1'e döner (`ivl: 1`, yarın vadeli). Yeni soru → `ivl: 1`, yarın vadeli.
- **Emeklilik:** 90 günü rahat geçen soru progress şişirmesin — `Kapatılanlar`a tek satır özetle taşınır, soru listesinden düşer.
- **Drill = yalnız vadesi gelenler** (due ≤ bugün), en fazla 3. Vadeli soru yoksa drill atlanır — tek cümle "no reviews due today" + doğrudan işe geçilir.
- **Welcome:** "Drill: N question(s) ready" satırı vade-farkında olur: `Reviews due today: N` (N>0) / `No reviews due today` (soru var ama vadeli değil) / satır yok (hiç soru yok). Yeni saf `due_count(progress, today)`.
- **Hesap sahibi = model** (aralık seçimi, kuyruk yazımı — kapanış flush'ı zaten dosyayı model yazıyor); **kabuk yalnız sayar** (`due_count` — welcome göstergesi). "İnce kabuk" korunur.

## Davranış

### 1. Kapanış (`src/progress.rs` `closing_prompt` — progress kuralı)

`Geri çağırma soruları` kural cümlesi genişler:
- Madde formatı: `- <question> — <one-line answer> | due: YYYY-MM-DD | ivl: <days>`.
- Çizelgeleme: bugün TODAY bölümünden; drill'de rahat hatırlanan → merdivende bir üst basamak (`1,3,7,16,35,90`), zorlanılan/yanlışlanan → `ivl: 1` (yarın); bu oturumda eklenen yeni soru → `ivl: 1` (yarın); drill'e girmeyen soru → kuyruk DEĞİŞMEZ.
- Kuyruksuz eski maddeye kuyruk eklenir (`ivl: 1`, yarın).
- `ivl: 90` basamağını rahat geçen soru `Kapatılanlar`a tek satırla taşınır, listeden düşer.

### 2. Açılış (`src/progress.rs` `opening_prompt`)

Drill talimatı değişir: "Pick 2-3 questions" → "Pick ONLY questions whose `due:` date is today or earlier (at most 3; oldest due first). If NO question is due, say one sentence — 'no reviews due today' — skip the drill and move straight to today's work." (Mevcut "progress'te soru yoksa uydur" kuralı: yalnız hiç soru yokken geçerli kalır.)

### 3. Welcome (`src/tui/welcome.rs` + çağrı yeri)

- Yeni saf fonksiyon: `pub fn due_count(progress: &str, today: &str) -> usize` — `## Geri çağırma soruları` bölümünün maddelerinde `due: YYYY-MM-DD` ara; `due ≤ today` (ISO string karşılaştırması yeterli) VEYA kuyruk yok → say.
- `WelcomeData`'ya `due_count: usize` alanı; kurulum fonksiyonu `today: &str` parametresi alır (çağrı yerlerinde `today` zaten mevcut — `build_session`/`run` akışı).
- Render: `drill_count > 0` satırı yerine: `due_count > 0` → `Reviews due today: {due_count}`; `due_count == 0 && drill_count > 0` → `No reviews due today`; `drill_count == 0` → satır yok (mevcut davranış).

## Test

- `due_count`: kuyruklu geçmiş/bugün tarihli → sayılır; gelecek tarihli → sayılmaz; kuyruksuz madde → sayılır; başka bölümdeki `due:` → sayılmaz; boş içerik → 0.
- `closing_prompt`: `due:`/`ivl:` formatını, merdiveni (`1,3,7,16,35,90`), reset ve emeklilik kurallarını içerir (string assert).
- `opening_prompt`: "due" seçim talimatı + "no reviews due today" atlama cümlesi.
- Welcome render: üç durum (`Reviews due today: N` / `No reviews due today` / satır yok).
- Mevcut `drill_count` testleri bozulmaz.

## Kapsam dışı

- Gerçek SM-2 (ease factor, kalite notu 0-5) — YAGNI, merdiven yeter.
- Plain/pipe yolunda vade göstergesi (banner) — TUI'ye özel.
- Konu-arası (cross-topic) vade toplama; `usta topics`'e vade sütunu.
- Kabuğun drill sonucunu puanlaması (model değerlendirir — gamification #8'in işi).

## Açık sorular

Yok.
