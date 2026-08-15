# Tasarım — İlerleme Özeti / Motivasyon (Roadmap #6)

**Tarih:** 2026-08-15
**Kapsam:** Oturum geçmişi veri katmanı (`learner/history.md`) + `usta stats` komutu (haftalık özet, LLM'siz) + welcome kutusunda haftalık tek satır. ADHD ilkesi: görünür ilerleme = yakıt; sıfır suçlama.
**Durum:** Onaylandı (Anil: "hepsini bitir") → writing-plans
**Bağımlılık:** Yok (gamification #8 bu veri katmanının ÜSTÜNE kurulacak).

## Amaç

Roadmap #6: "Haftalık özet: harita % değişimi, oturan maddeler, streak." Bugün hiçbir oturum geçmişi tutulmuyor — katalog (`learner/index.md`) yalnız SON oturum tarihini tutar; streak/haftalık delta hesaplanamaz. Veri katmanı + iki yüzey (komut + welcome satırı) eklenir. Tamamen kabuk işi — LLM çağrısı YOK ("kabuk sayar").

Kararlar:
- **Veri:** global `~/.config/usta/learner/history.md`, append-only; kapanış flush'ı başına bir satır: `- YYYY-MM-DD | <konu> | map <P>% | settled <N>` (P = `curriculum_percent`, N = `oturdu`+`derinleşildi` madde sayısı — curriculum dosyası flush SONRASI diskten okunur; curriculum yoksa `map -` / `settled -`). Aynı gün aynı konu birden çok oturum = birden çok satır (oturum sayısı bilgisi).
- **`usta stats`:** son 7 gün: konu başına oturum sayısı + map% ilk→son delta + settled delta; genel: bu hafta toplam oturum, güncel streak (ardışık gün, herhangi bir konu), en uzun streak. LLM gerekmez.
- **ADHD-safe ton:** streak kırıksa suçlama yok — "current streak 0" YAZILMAZ; onun yerine `longest streak: N day(s)` pozitif çerçeve. Boş hafta: "quiet week — your longest streak is still N day(s)". Karşılaştırma/utandırma dili yok.
- **Welcome satırı:** identity + full-mode kutularına tek satır: `This week: N session(s) · streak M day(s)` (N>0 iken; streak 0 ise streak kısmı düşer). Veri `history.md`'den, saf parser.
- **Sürüm:** iş sonunda `0.15.0` + tag (politika).

## Davranış

### 1. Modül: `src/history.rs` (yeni)

- `pub fn record_line(date: &str, topic: &str, map_percent: Option<u8>, settled: Option<usize>) -> String` — satır formatı tek yerde.
- `pub fn append(global: &Path, line: &str) -> Result<()>` — `learner/history.md` yoksa `# Oturum Geçmişi` başlığıyla oluştur, satırı sona ekle (index::record'un dosya-yazım hijyeniyle — `write_atomic`).
- `pub struct Entry { pub date: String, pub topic: String, pub map: Option<u8>, pub settled: Option<usize> }`
- `pub fn entries(content: &str) -> Vec<Entry>` — bozuk satır atlanır (index::entries deseni).
- `pub fn current_streak(entries: &[Entry], today: &str) -> u32` — bugünden (veya dünden — bugün henüz oturum yoksa dün biten seri hâlâ "güncel" sayılır) geriye ardışık gün sayısı. Tarih aritmetiği `chrono::NaiveDate` (crate zaten bağımlı — index tarihleri chrono ile).
- `pub fn longest_streak(entries: &[Entry]) -> u32`
- `pub fn week_summary(entries: &[Entry], today: &str) -> WeekSummary` — son 7 gün penceresi: `pub struct WeekSummary { pub sessions: u32, pub per_topic: Vec<TopicWeek> }`, `TopicWeek { topic, sessions, map_from, map_to, settled_from, settled_to }`.
- `pub fn settled_count(curriculum: &str) -> Option<usize>` — `oturdu`/`derinleşildi` satır sayısı (welcome::curriculum_percent'in durum-sayma yaklaşımıyla tutarlı — aynı STATUSES sabitine dayanmalı; sabit ortak kullanılabilir hale getirilir veya kopya test-kilitli tutulur).

### 2. Kayıt (`src/main.rs` `flush_progress`)

Katalog güncellemesinin (`index::record`) hemen yanında: curriculum dosyası diskten okunur → `curriculum_percent` + `settled_count` → `history::append`. Hata = warn, oturumu düşürmez (katalogla aynı tolerans).

### 3. `usta stats` komutu (`src/main.rs` CLI)

`usta topics` deseninde yeni komut. Çıktı (örnek):

```
This week (last 7 days)

  rust             3 session(s)   map 40% → 55%   settled 4 → 7
  kaynak-ingest    2 session(s)   map 10% → 25%   settled 1 → 2

  total: 5 session(s) · current streak: 3 day(s) · longest: 6 day(s)
```

- Streak 0 ise: `longest streak: N day(s)` yalnız (current yazılmaz).
- Hiç oturum yoksa (7 gün): `quiet week — your longest streak is still N day(s)`; history hiç yoksa: `no sessions recorded yet — streaks start with the first one.`
- `usta help`/`/help` metnine `usta stats` satırı eklenir (help.rs).

### 4. Welcome satırı (`src/tui/welcome.rs`)

`WelcomeData`'ya `week_sessions: u32`, `streak: u32`; `gather`'a `history: Option<&str>` parametresi (çağrı yerleri global'den okur). Render (iki kutu da): `week_sessions > 0` ise `This week: N session(s) · streak M day(s)` (M=0 ise ` · streak...` kısmı yok).

### 5. Dokümantasyon + sürüm

SPEC yeni § (v0.15) · README Highlights satırı (İngilizce, "visible progress" vurgusu) · ROADMAP #6 ✅ · Cargo.toml `0.15.0` + sürüm testi güncelle + tag.

## Test

- `record_line`/`entries` round-trip; bozuk satır atlanır; `map -`/`settled -` (None) yolu.
- `current_streak`: bugün dahil seri; dün biten seri (bugün oturum yok) hâlâ sayılır; boşluk seriyi bitirir; boş giriş → 0.
- `longest_streak`: aralıklı geçmişte doğru maksimum.
- `week_summary`: 7 gün penceresi sınırı (8 gün önceki sayılmaz), konu grupları, ilk→son map/settled.
- `settled_count`: oturdu+derinleşildi sayımı, curriculum'suz None.
- `append`: dosya yoksa başlıkla oluşur, varsa sona eklenir (tmpdir).
- Welcome: satır üç durumu (sessions>0+streak>0 / sessions>0+streak=0 / sessions=0 → satır yok).
- stats çıktı fonksiyonu saf test edilir (`render_stats(entries, today) -> String`): dolu hafta / boş hafta / hiç kayıt üç senaryosu + "current streak 0 yazılmaz" assert'i.
- Sürüm assert: `0.15.0`.

## Kapsam dışı

- LLM'li haftalık yorum/motivasyon mesajı (gamification #8'in alanı).
- history.md'nin system prompt'a yüklenmesi (context şişirir; #8 gerekirse streak'i tek satır geçirir).
- Retention/arşivleme (append-only büyür — yıllar sonra sorun, şimdi değil).
- Grafik/sparkline.

## Açık sorular

Yok.
