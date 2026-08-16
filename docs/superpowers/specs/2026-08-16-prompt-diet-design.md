# Tasarım — Prompt Diyeti: Kabuk Çözüyorsa LLM'e Gitmez (v0.19.0)

**Tarih:** 2026-08-16
**Kapsam:** Prompt denetiminin onaylı 4 bulgusu: (1) Gamification kuralları koşullu yükleme, (2) vadeli-soru seçimi kabuğa, (3) ölü KEEP cümlesi kaldırma, (4) Course Material + Prediction Protocol koşullu yükleme. Yaygın senaryoda system prompt ~%14 küçülür; drill filtresi deterministikleşir.
**Durum:** Onaylandı (Anil, 2026-08-16) → writing-plans
**İlke (bağlayıcı):** Kabukla deterministik çözülebilen hiçbir şey prompt'a yazılmaz; her oturumda taşınan hiçbir bölüm, kabuğun bildiği bir koşula bağlıysa koşulsuz yüklenmez. Referans desenler: GOAL.md koşullu yüklemesi (brain.rs), materials digest enjeksiyonu.

## 1. Gamification → koşullu brain dosyası

- TEACHING.md'deki `## Gamification` bölümü (~2KB) **yeni embedded dosya** `GAMIFICATION.md`'ye taşınır (Code-owned, `defaults.rs` listesine eklenir; TEACHING'den bölüm silinir).
- `load_system_prompt`: USER.md içeriği zaten okunuyor — `- gamification: on` satırı varsa `GAMIFICATION.md` yüklenir (GOAL.md deseni), yoksa hiç taşınmaz.
- **Oturum-ortası toggle boşluğu:** `/game on` bilgi turu artık "apply the rules from TEACHING.md" DİYEMEZ (kurallar prompt'ta olmayabilir). Çözüm: `/game on` enjeksiyon turu, kabuk tarafından global'den okunan GAMIFICATION.md içeriğini TURUN İÇİNE gömer ("[GAME MODE ON]\n<kurallar>\n"). `/game off` turu değişmez ("stop all game narration" yeterli). Sonraki oturum system prompt'tan alır.
- TEACHING.md'de tek satır işaretçi bırakılabilir ("Gamification rules load only when the profile enables it") — zorunlu değil, karar implementasyona.

## 2. Vadeli soru seçimi → kabuk

- Bugün: opening_prompt modele "due ≤ bugün olanları seç, en eski 3, yoksa drill atla" talimatı veriyor — saf tarih/sıralama işi, model bazen yanlış yapıyor.
- Yeni: kabuk seçer. `welcome.rs`'teki due mantığı genişletilir: `pub fn due_questions(progress: &str, today: &str) -> Vec<String>` — `## Geri çağırma soruları` maddelerinden due ≤ today olanlar (kuyruksuz eski madde = due), **en eski due önce**, en fazla 3, madde satırı olduğu gibi döner. `due_count` bu fonksiyonun `.len()`'i olur (tek kaynak — mevcut testler kırılmadan).
- `opening_prompt` yeniden şekillenir: filtreleme talimatı GİDER; imza `due: &[String]` (veya `Option<&str>` birleştirilmiş blok) alır:
  - Boş değil → "Ask me these due recall questions, one at a time, don't answer them yourself:\n<maddeler>" + drill-sonrası harita cümlesi mevcut haliyle.
  - Boş + progress'te soru VAR → "say exactly one sentence: 'no reviews due today', skip the drill and move straight to today's work."
  - (Hiç soru yokken drill zaten farklı akış — mevcut "come up with 2 small recall questions" kuralı yalnız bu durumda kalır; kabuk `drill_count`==0 bilgisini de geçirebilir — detay implementasyona, davranış spec'teki üç dal.)
- Çağrı yerleri (TUI + plain): progress dosyasını zaten okuyorlar — `due_questions` çağrısı + parametre geçişi.
- Kapanış tarafındaki `due:/ivl:` üretim kuralları DEĞİŞMEZ (aritmetik hâlâ modelde — bulgu 5 bilinçli ertelendi).

## 3. Ölü talimat kaldırma

- closing_prompt'taki "KEEP the '## Tercihler' section ... shell-managed" cümlesi SİLİNİR — kabuk `restore_game_pref` ile zaten garanti ediyor. İlgili test (`closing_prompt_protects_tercihler_section`) kaldırılır/negatife çevrilmez, sadece silinir; restore testleri zaten var.

## 4. Course Material + Prediction Protocol → koşullu

- TEACHING.md'den iki bölüm ayrılır, iki yeni Code-owned embedded dosya: `MATERIAL.md` (`## Course Material` içeriği), `PREDICTION.md` (`## Prediction Protocol` içeriği). defaults.rs listesine girer, TEACHING küçülür.
- Yükleme koşulları (`load_system_prompt`):
  - `MATERIAL.md`: `materials_present(project_root)` — `materials/` altında en az bir `.md/.txt/.pdf` dosya (HAFİF varlık kontrolü — digest üretme YOK; `materials.rs`'e küçük saf fonksiyon). Demirlenmiş müfredatla sonraki oturumlar da materials/ hâlâ duruyorsa kuralları alır — koşul bunu kapsar.
  - `PREDICTION.md`: proje kökünde `Cargo.toml` varsa (check.rs'in cargo tespitiyle AYNI koşul — oradaki mantık neyse yeniden kullan/paylaş).
- Koşul false iken ilgili dosya prompt'ta HİÇ yok.

## Sürüm + docs

- Cargo `0.19.0` (+ sürüm testi) + tag. SPEC yeni §: koşullu brain yükleme tablosu (dosya | koşul) + due-seçimi kabukta + prompt-diyet ilkesi. README "thin shell, thick brain" ağacına yeni brain dosyaları (GAMIFICATION/MATERIAL/PREDICTION + koşulları tek satır).
- Global brain sync: üç yeni dosya Code-owned → mevcut `write_global_defaults` senkronu otomatik dağıtır; TEACHING.md değişikliği de aynı yolla gider.

## Test

- Koşullu yükleme ×3: koşul true → bölüm prompt'ta, false → içerik prompt'ta HİÇ yok (tmpdir, GOAL testi deseni). USER.md'de `- gamification: on` satırıyla gate.
- `/game on` turu GAMIFICATION.md içeriğini gömer (global'den okunmuş metin turda).
- `due_questions`: geçmiş/bugün/kuyruksuz seçilir, gelecek elenir, en eski önce, 3 tavanı, başka bölüm maddesi sayılmaz; `due_count == due_questions().len()` tutarlılığı.
- `opening_prompt` üç dal: dolu liste → maddeler turda + filtreleme talimatı YOK; boş+soru var → "no reviews due today" atlaması; mevcut soru-hiç-yok kuralı korunur.
- closing_prompt: Tercihler cümlesi artık YOK (negatif assert), restore testleri yeşil.
- `materials_present`: md/txt/pdf → true; boş/`.gitkeep`-only/klasör yok → false.
- defaults: `returns_all_nonempty_files` üç yeni dosyayı kapsar.

## Kapsam dışı

- Bulgu 5 (spaced-rep aritmetiğinin kabuğa taşınması — verdict kanalı refactor'ı, ayrı iş).
- `_default.md`/`learner/index.md` koşullu yüklemesi (düşük öncelik, ayrı değerlendirme).
- Brain dosyalarının içerik/pedagoji değişikliği (yalnız taşıma + koşul).

## Açık sorular

Yok.
