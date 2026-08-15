# Tasarım — Gamification Modu (Roadmap #8)

**Tarih:** 2026-08-15
**Kapsam:** `/game on|off` toggle'ı (kalıcı, USER.md `## Tercihler`); açıkken Usta XP/seviye/rozet anlatısı yapar — süreç-puanlı, ADHD-safe. Deneme sınavı = boss fight. Anlatı tamamen brain/prompt katmanında; kabuk yalnız toggle + streak besleme.
**Durum:** Onaylandı (Anil'in fikri, 2026-08-14; "hepsini bitir" ile sıraya girdi) → writing-plans
**Bağımlılık:** #6 (streak verisi `history.rs`) ve #7 (`/exam` = boss fight) MERGE EDİLMİŞ olmalı.

## Amaç

Anil'in fikri: on/off'lu oyunlaştırma. ADHD beyni görünür dopamin döngüsüyle çalışır — puan/seviye/rozet ilerlemeyi hissedilir kılar. İki bilinen tuzak tasarım kuralına çevrildi:
1. **Streak suçluluğu yasak** — kırık seride utandırma yok; yalnız "longest streak" pozitif çerçevesi (habits.md pattern'i).
2. **Overjustification önlemi** — puan PERFORMANSA değil SÜRECE bağlanır (oturum açmak, tahmin etmek, egzersiz teslim etmek puan getirir; doğru bilmek şart değil). Kobe motoru: süreç/kimlik.

Kararlar:
- **Toggle: `/game on|off|durum`** oturum içi komut. Kalıcılık: USER.md'de `## Tercihler` bölümünde `- gamification: on|off` satırı — KABUK yazar (deterministik upsert, `index::record` hijyeni; model USER.md'yi kapanışta yazarken bu bölümü korur — closing_prompt profile kuralına tek cümle eklenir). USER.md system prompt'ta zaten yüklü → model tercihi görür.
- **Anında etki:** toggle sonrası oturuma `[GAME MODE ON/OFF]` bilgi turu enjekte edilir (system prompt yeniden kurulmaz).
- **Anlatı modeli yapar, kabuk saymaz.** XP hesabı prompt kuralı: müfredat durumları (görüldü=10 · oturdu=25 · derinleşildi=50 XP — model haritadan toplar), süreç puanları (oturum +5, tahmin protokolüne katılım +2, egzersiz teslimi +10 — doğruluktan bağımsız), gap kapanışı = rozet, `/exam` = boss fight (skor anlatıya boss sonucu olarak girer). Seviye eşikleri: 0/100/250/500/1000/2000 XP (Çırak → Kalfa yolu → Usta adayı... isimlendirme USTA.md kuralında).
- **Doz kontrolü:** oyun anlatısı HER mesajda değil — yalnız kilometre taşında (durum terfisi, rozet, seviye atlama, oturum kapanışı özeti, boss sonucu) tek kısa satır. Spam = novelty ölümü (ADHD).
- **Streak beslemesi:** oyun açıkken kabuk, açılış turuna tek satır ekler: `[GAME] streak: N day(s) (longest M)` — `history.rs`'ten (#6). Kırık seri: yalnız longest geçilir (`streak yok` yazılmaz).
- **Kural evi: TEACHING.md `## Gamification`** (embedded; USTA.md değil — pedagoji katmanı).
- **Sürüm:** `0.17.0` + tag.

## Davranış

### 1. Komut (`src/main.rs`)

`pub(crate) enum GameCmd { On, Off, Status }` + `pub(crate) fn parse_game_command(line: &str) -> Option<GameCmd>` — `/game on`, `/game off`, `/game` (Status); başka her şey None (`parse_watch_command` deseni — koda bak, birebir aynı yapı).

### 2. Kalıcılık (`src/main.rs` veya uygun modül)

- `pub(crate) fn game_pref(global: &Path) -> bool` — USER.md'de `- gamification: on` satırı var mı (bölümden bağımsız satır araması yeter; default OFF).
- `pub(crate) fn set_game_pref(global: &Path, on: bool) -> Result<()>` — USER.md oku (yoksa oluşturma — sihirbaz değil, USER.md scaffold'da hep var); `## Tercihler` bölümü yoksa dosya sonuna ekle; `- gamification:` satırı varsa değeri güncelle, yoksa bölüme ekle; `write_atomic`.

### 3. Döngü entegrasyonu (TUI + plain — `/watch` deseni)

`/game ...` intercept: `set_game_pref` (On/Off) → notice (`gamification on — XP, levels and badges are live` / `gamification off — back to quiet mode` / Status → `gamification is on|off`) → On/Off'ta oturuma bilgi turu enjekte edilir: `[GAME MODE ON] Gamification is now ON — apply the Gamification rules from TEACHING.md from this point on.` (OFF muadili: `...stop all game narration.`). Status LLM'e gitmez.

### 4. Açılış beslemesi (`src/progress.rs` + çağrı yerleri)

`opening_prompt` yeni parametre: `game_streak: Option<&str>` — Some ise sona `\n[GAME] {s}\n` eklenir. Çağrı yerleri: `game_pref(global)` true ise `history.rs` entries'ten `current_streak`/`longest_streak` hesaplanır, string kurulur: streak>0 → `streak: N day(s) (longest M)`; streak==0 → `longest streak: M day(s)` (mevcut-0 yazılmaz); history boş → None. `onboarding_prompt`'a EKLENMEZ (ilk tanışmada oyun anlatısı erken).

### 5. Kapanış koruması (`closing_prompt` profile kuralına ek)

"KEEP the `## Tercihler` section (e.g. `- gamification: on`) exactly as-is — it is shell-managed."

### 6. TEACHING.md — `## Gamification` bölümü

- Yalnız USER.md `- gamification: on` iken aktif; kapalıyken TEK oyun kelimesi yok.
- XP: durumlar (görüldü 10 · oturdu 25 · derinleşildi 50 — haritadan topla) + süreç puanları (oturum +5, tahmin +2, egzersiz teslimi +10 — doğruluktan BAĞIMSIZ; yanlış tahmin de puan alır, katılım ödüllenir).
- Seviyeler: 0 Çırak · 100 Kalfa Adayı · 250 Kalfa · 500 Usta Çırağı · 1000 Usta Adayı · 2000 Usta. Seviye atlarken tek satır kutlama.
- Rozetler: gap kapanışı, ilk egzersiz, 7-gün streak, ilk boss (exam) — icat serbest ama seyrek.
- Boss fight: `/exam` sonucu oyun dilinde de yorumlanır (geçti = boss düştü; kalan zayıf maddeler = boss'un kaçan minyonları → gap).
- DOZ: kilometre taşında tek kısa satır; her mesajda skor sayma YOK. Streak: yalnız açılıştaki `[GAME]` satırından; kırık seriyi ASLA utandırma — longest çerçevesi.
- Overjustification bekçisi: puan süreçte; "yanlış cevap = puan kaybı" YOK, ceza mekaniği YOK.

### 7. Yardım + docs

`/help`: `/game on|off       XP, levels, badges (ADHD-safe)`. SPEC yeni § (v0.17) · README (İngilizce Highlights satırı) · ROADMAP #8 ✅ · Cargo `0.17.0` + tag.

## Test

- `parse_game_command`: `/game on`→On, `/game off`→Off, `/game`→Status, ` /game ON ` (case) →On, `/game x`→None, `/gamer`→None.
- `game_pref` + `set_game_pref` (tmpdir): default off; on yaz→true; off'a çevir→false; `## Tercihler` yoksa oluşur, varsa satır güncellenir (çift satır oluşmaz — iki kez on üst üste); USER.md'nin diğer içeriği bozulmaz.
- `opening_prompt`: `game_streak=Some` → `[GAME]` içerir; None → içermez.
- `closing_prompt`: `Tercihler` koruma cümlesi.
- help: `/game` satırı.
- TEACHING içerik: defaults `returns_all_nonempty_files` zaten kapsar.

## Kapsam dışı

- Kabuğun XP hesaplaması/saklaması (model anlatır — tutarlılık maliyeti kabul, oyun kozmetik katman).
- Lider tablosu, paylaşım, görsel rozet.
- Oyun verisinin ayrı dosyada persist'i (seviye zaten müfredat durumlarından türetilir — idempotent).
- `/game` dışında ayar UI'ı.

## Açık sorular

Yok.
