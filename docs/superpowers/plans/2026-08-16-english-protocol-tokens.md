# English Protocol Tokens Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Kabuğun parse ettiği/yazdığı tüm iç protokol token'ları İngilizce'ye geçer (spec: `docs/superpowers/specs/2026-08-16-english-protocol-tokens-design.md`, token haritası T1–T21 oradadır ve **bağlayıcıdır**); mevcut kullanıcı verisi tek seferlik deterministik migration ile dönüşür.

**Architecture:** Önce tüm token literalleri yeni `src/tokens.rs` modülünde toplanır (değerler henüz Türkçe — davranış-nötr refactor), harita-durumu sayımı exact-match'e sertleştirilir, sonra değerler tek atomik commit'te İngilizce'ye çevrilir (kod + embedded md + test fixture'ları birlikte). Migration ayrı modül (`src/migrate.rs`), her komut girişinde okumalardan önce koşar, idempotent, bağlam-kilitli.

**Tech Stack:** Rust (mevcut bağımlılıklar yeterli — yeni crate YOK; regex crate'i eklenmez, hepsi string ops).

## Global Constraints

- Token değerlerinin tek kaynağı spec'teki T1–T21 tablosu — plan ile spec çelişirse spec kazanır.
- `## Hedef Durumu` her yerde `## Hedef`ten önce eşlenir (prefix çakışması).
- Harita durumu eşleşmesi HER ZAMAN tam-segment (spec Not 2: `"seen"` ⊂ `"not seen"`); `contains(state)` ile durum sayımı yasak.
- Parser'lar migration sonrası YALNIZ İngilizce token bilir; Türkçe sabitler sadece `src/migrate.rs` içinde yaşar.
- Kullanıcı girdisi kabulü (`evet/hayır/e/h`) ve `Çırak→Usta` seviye adları DEĞİŞMEZ.
- Her task sonunda `cargo test` yeşil (319+ test) ve `cargo clippy` yeni uyarı üretmez.
- Sürüm: v0.20.0 (Task 7'de).

---

### Task 1: `src/tokens.rs` — tek kaynak modülü (değerler henüz Türkçe)

**Files:**
- Create: `src/tokens.rs`
- Modify: `src/main.rs` (mod bildirimi, diğer `mod` satırlarının yanına)

**Interfaces:**
- Produces: `tokens::STATES: [&str; 4]`, `tokens::STATE_NOT_SEEN/SEEN/SETTLED/DEEPENED`, `tokens::S_LEVEL/S_RECALL/S_RETIRED/S_OPEN_EXERCISE/S_GAPS/S_ERROR_LOG/S_HINT_LADDER` (bare bölüm adları, `section()` helper'larıyla kullanılır), `tokens::H_RECORDS/H_GOAL/H_GOAL_STATUS/H_PREFERENCES` (satır-başı tam başlıklar), `tokens::FILE_DIVIDER/CHECKPOINT/SOURCE_DASH/SOURCE_HYPHEN/HISTORY_HEADER/PROGRESS_HEADING_SUFFIX/DEFAULT_TOPIC` — sonraki TÜM task'lar bu adları kullanır.

- [ ] **Step 1: Modülü yaz** — değerler bugünkü Türkçe literallerin birebir aynısı (bu task davranış değiştirmez):

```rust
//! Protocol tokens — single source of truth for every marker the shell
//! parses or writes. User-facing language stays free (SOUL.md language
//! lock); these internal tokens are the protocol. Values flip to English
//! in the migration release; legacy forms then move to src/migrate.rs.

pub const STATE_NOT_SEEN: &str = "görülmedi";
pub const STATE_SEEN: &str = "görüldü";
pub const STATE_SETTLED: &str = "oturdu";
pub const STATE_DEEPENED: &str = "derinleşildi";
/// Order matters: index 0 is the "unseen" state, 1.. are the seen states.
pub const STATES: [&str; 4] = [STATE_NOT_SEEN, STATE_SEEN, STATE_SETTLED, STATE_DEEPENED];

// Bare section names (used with the `section()` helpers, no `## ` prefix).
pub const S_LEVEL: &str = "Seviye";
pub const S_RECALL: &str = "Geri çağırma soruları";
pub const S_RETIRED: &str = "Kapatılanlar";
pub const S_OPEN_EXERCISE: &str = "Açık egzersiz";
pub const S_GAPS: &str = "Gap'ler";
pub const S_ERROR_LOG: &str = "Hata günlüğü";
pub const S_HINT_LADDER: &str = "İpucu merdiveni";

// Full line-start headers.
pub const H_RECORDS: &str = "## Kayıtlar";
pub const H_GOAL: &str = "## Hedef";
pub const H_GOAL_STATUS: &str = "## Hedef Durumu";
pub const H_PREFERENCES: &str = "## Tercihler";

// File / flow markers.
pub const FILE_DIVIDER: &str = "===DOSYA:";
pub const CHECKPOINT: &str = "[ARA KAYIT]";
pub const SOURCE_DASH: &str = "— kaynak:";
pub const SOURCE_HYPHEN: &str = "- kaynak:";
pub const HISTORY_HEADER: &str = "# Oturum Geçmişi\n\n";
/// Progress file heading suffix: `# <topic> — İlerleme`.
pub const PROGRESS_HEADING_SUFFIX: &str = "— İlerleme";
pub const DEFAULT_TOPIC: &str = "genel";
```

- [ ] **Step 2: `src/main.rs`'e `mod tokens;` ekle** (mevcut `mod` bloğuna, alfabetik sıraya uyarak).

- [ ] **Step 3: Derle** — Run: `cargo build 2>&1 | tail -3`. Expected: yeşil; `dead_code` uyarıları normal (henüz kullanılmıyor) — geçici `#[allow(dead_code)]` modül başına eklenebilir, Task 2'de kalkar.

- [ ] **Step 4: Commit** — `git add src/tokens.rs src/main.rs && git commit -m "refactor: src/tokens.rs — protokol token'ları tek modülde (değerler henüz TR)"`

---

### Task 2: Literal süpürme — parser/writer'lar `tokens::` sabitlerinden okur

**Files:**
- Modify: `src/index.rs` (SECTION sabiti → `tokens::H_RECORDS`), `src/history.rs` (HEADER, settled_count durumları), `src/brain.rs` (`"## Hedef"` :104,114; `"## Tercihler"` gamification koşulu), `src/main.rs` (`"## Tercihler"` :991; `"## Hedef"` :1042; `"genel"` :793,810,827 → `tokens::DEFAULT_TOPIC`; :1074 kabul satırı OLDUĞU GİBİ kalır — iki dilli kabul bilinçli), `src/progress.rs` (kapanış prompt'undaki bölüm adları, `===DOSYA:`, kaynak önekleri, `Kapatılanlar` kuralları — format! içine `{}` ile sabitler girer), `src/session.rs` (`[ARA KAYIT]` :95–106), `src/tui/welcome.rs` (STATUSES dizisi → `tokens::STATES`, `"Seviye"`, `"Geri çağırma soruları"`)

**Interfaces:**
- Consumes: Task 1'in tüm sabitleri.
- Produces: davranış birebir aynı; Türkçe protokol literali `src/` altında yalnız `tokens.rs` ve `#[cfg(test)]` bloklarında kalır.

- [ ] **Step 1: Dosya dosya süpür.** Bulma komutu (her dosyada test-dışı eşleşme sıfırlanana kadar):

```bash
grep -n 'görülmedi\|görüldü\|oturdu\|derinleşildi\|Kayıtlar\|## Hedef\|Tercihler\|Kapatılanlar\|Geri çağırma\|Açık egzersiz\|Seviye\|Gap.ler\|Hata günlüğü\|İpucu merdiveni\|===DOSYA\|ARA KAYIT\|kaynak:\|Oturum Geçmişi\|— İlerleme\|"genel"' src/*.rs src/tui/*.rs | grep -v 'tokens.rs\|#\[cfg(test)\]'
```

Örnek edit deseni (index.rs): `const SECTION: &str = "## Kayıtlar";` → `use crate::tokens; … const SECTION: &str = tokens::H_RECORDS;`. format!/string sabitlerinde: `"…`## Seviye`…"` gibi prompt metinleri `format!("…`## {lvl}`…", lvl = tokens::S_LEVEL)` biçimine döner. history.rs:176'daki test-locked senkron yorumu güncellenir: artık "duplicate of welcome::STATUSES" değil, ikisi de `tokens::STATES`ten okur — yorum silinir.

- [ ] **Step 2: Test** — Run: `cargo test --quiet 2>&1 | tail -3`. Expected: 319 passed (fixture'lar hâlâ Türkçe, değerler de Türkçe — hiçbir test değişmedi).

- [ ] **Step 3: Süpürme doğrulaması** — Step 1'deki grep test-dışı SIFIR dönmeli (tokens.rs hariç).

- [ ] **Step 4: Commit** — `git commit -am "refactor: protokol literalleri tokens:: sabitlerine süpürüldü — davranış değişmedi"`

---

### Task 3: Durum eşleşmesini exact-match'e sertleştir

**Files:**
- Modify: `src/tui/welcome.rs` (`curriculum_percent`, `next_unseen`), `src/history.rs` (`settled_count`)
- Test: aynı dosyaların `#[cfg(test)]` blokları

**Interfaces:**
- Consumes: `tokens::STATES`.
- Produces: `map_state_of(line: &str) -> Option<&'static str>` — `src/tokens.rs`'e eklenir; harita satırından durumu tam-segment çıkarır. Task 5 migration'ı da aynı segment mantığını kullanır.

- [ ] **Step 1: Önce başarısız testleri yaz** (welcome.rs testlerine; Türkçe değerlerle — İngilizce'ye Task 4'te fixture güncellemesiyle döner):

```rust
#[test]
fn state_matching_is_exact_segment_not_substring() {
    // Madde METNİNDE durum kelimesi geçiyor — sayılmamalı/karışmamalı.
    let c = "- oturdu kelimesi konulu makale: görülmedi\n- borrow: oturdu\n";
    assert_eq!(curriculum_percent(c), Some(50)); // 1/2 seen
    assert_eq!(next_unseen(c).as_deref(), Some("oturdu kelimesi konulu makale"));
}
```

Ve history.rs'e:

```rust
#[test]
fn settled_count_ignores_state_words_in_item_text() {
    let c = "- oturdu üstüne makale: görülmedi\n- b: oturdu\n- c: derinleşildi | due: 2026-01-01\n";
    assert_eq!(settled_count(c), Some(2));
}
```

- [ ] **Step 2: Koş, kırmızıyı gör** — Run: `cargo test state_matching settled_count_ignores -q`. Expected: FAIL (mevcut `contains` ilk maddeyi yanlış sayar).

- [ ] **Step 3: `tokens::map_state_of` yaz ve üç fonksiyonu ona geçir:**

```rust
/// Extract the map state of a `- <item>: <state>` line (optional `| due: …` tail).
/// Exact segment match — never a substring scan ("seen" ⊂ "not seen").
pub fn map_state_of(line: &str) -> Option<&'static str> {
    let line = line.trim();
    if !line.starts_with("- ") { return None; }
    let head = line.split(" | ").next().unwrap_or(line); // drop `| due:` tail
    let state = head.rsplit(':').next()?.trim();
    STATES.iter().find(|s| **s == state).copied()
}
```

`curriculum_percent`: `match tokens::map_state_of(line) { Some(s) if s == tokens::STATE_NOT_SEEN => total += 1, Some(_) => { total += 1; seen += 1 } , None => {} }`. `next_unseen`: `find(|l| tokens::map_state_of(l) == Some(tokens::STATE_NOT_SEEN))`, madde metni = son `:`e kadar olan kısım (`head.rsplitn(2, ':').nth(1)`) — mevcut trim zinciri korunur. `settled_count`: `filter(|l| matches!(tokens::map_state_of(l), Some(s) if s == tokens::STATE_SETTLED || s == tokens::STATE_DEEPENED))`.

- [ ] **Step 4: Test** — Run: `cargo test --quiet 2>&1 | tail -3`. Expected: hepsi yeşil (yeni 2 dahil).

- [ ] **Step 5: Commit** — `git commit -am "fix: harita durumu eşleşmesi exact-segment — substring tuzağı kapandı"`

---

### Task 4: Değerleri İngilizce'ye çevir — kod + embedded md + fixture'lar tek atomik commit

**Files:**
- Modify: `src/tokens.rs` (tüm değerler → spec T1–T21 İngilizce sütunu; `DEFAULT_TOPIC = "general"`), `TEACHING.md`, `GOAL.md`, `USTA.md`, `SOUL.md`, `learner/index.md`, `approaches/software.md`, `approaches/_default.md` (Türkçe token geçen her yer), `src/progress.rs` kapanış prompt'unun serbest-metin kısımları (`görülmedi/görüldü/oturdu/derinleşildi` listesi, `— kaynak:` örnekleri), tüm `#[cfg(test)]` fixture'ları (progress.rs, brain.rs, index.rs, history.rs, main.rs, tui/welcome.rs)

**Interfaces:**
- Consumes: Task 1–3'ün sabit adları (adlar değişmez, yalnız değerler).
- Produces: çalışan binary'nin tüm protokolü İngilizce. Task 5 migration'ı bu İngilizce değerleri hedef alır.

- [ ] **Step 1: `tokens.rs` değerlerini çevir** (spec tablosu birebir): `STATE_NOT_SEEN = "not seen"`, `STATE_SEEN = "seen"`, `STATE_SETTLED = "settled"`, `STATE_DEEPENED = "deepened"`, `S_LEVEL = "Level"`, `S_RECALL = "Recall questions"`, `S_RETIRED = "Retired"`, `S_OPEN_EXERCISE = "Open exercise"`, `S_GAPS = "Gaps"`, `S_ERROR_LOG = "Error log"`, `S_HINT_LADDER = "Hint ladder"`, `H_RECORDS = "## Records"`, `H_GOAL = "## Goal"`, `H_GOAL_STATUS = "## Goal Status"`, `H_PREFERENCES = "## Preferences"`, `FILE_DIVIDER = "===FILE:"`, `CHECKPOINT = "[CHECKPOINT]"`, `SOURCE_DASH = "— source:"`, `SOURCE_HYPHEN = "- source:"`, `HISTORY_HEADER = "# Session History\n\n"`, `PROGRESS_HEADING_SUFFIX = "— Progress"`, `DEFAULT_TOPIC = "general"`.

- [ ] **Step 2: Embedded md dosyalarını güncelle.** Bulma: `grep -n 'görülmedi\|görüldü\|oturdu\|derinleşildi\|Hedef\|Tercihler\|Kapatılanlar\|Kayıtlar\|Seviye\|Geri çağırma\|Açık egzersiz\|kaynak:' TEACHING.md GOAL.md USTA.md SOUL.md learner/index.md approaches/*.md`. Her eşleşme spec tablosundaki İngilizce karşılığına döner (kural cümleleri İngilizce zaten — yalnız token değişir). `Çırak→Usta` (GAMIFICATION.md) DOKUNULMAZ.

- [ ] **Step 3: Test fixture'larını çevir.** `grep -rn 'görülmedi\|görüldü\|oturdu\|derinleşildi\|Tercihler\|Hedef\|Kayıtlar\|Kapatılanlar\|ARA KAYIT\|DOSYA\|Oturum Geçmişi\|Seviye' src/ --include='*.rs'` → kalan her eşleşme test bloklarındadır; İngilizce değere çevir (Task 3'te eklenen exact-match testleri dahil: `"- makale hakkında settled: not seen\n- borrow: settled\n"` gibi — kolizyon senaryosu İngilizce'de anlamını korumalı). `main.rs:1074` (`s == "general" || s == "genel"`) olduğu gibi kalır.

- [ ] **Step 4: Test + süpürme doğrulaması** — Run: `cargo test --quiet 2>&1 | tail -3`. Expected: hepsi yeşil. Ardından: `grep -rn 'görülmedi\|oturdu\|derinleşildi\|Tercihler\|Kayıtlar\|ARA KAYIT\|===DOSYA' src/ *.md learner/ approaches/ | grep -v 'migrate.rs\|SPEC.md\|README\|docs/'` → sıfır (GAMIFICATION.md `Çırak/Usta` hariç — onlar bu grep'e girmez).

- [ ] **Step 5: Commit** — `git commit -am "feat!: protokol token'ları İngilizce — kod + brain + fixture'lar atomik (spec T1–T21)"`

---

### Task 5: `src/migrate.rs` — tek seferlik deterministik migration + unit testler

**Files:**
- Create: `src/migrate.rs`
- Modify: `src/main.rs` (`mod migrate;`)
- Test: `src/migrate.rs` içinde `#[cfg(test)]`

**Interfaces:**
- Consumes: `tokens::STATES` mantığı (segment kuralı) — ama Türkçe→İngilizce çiftleri BURADA sabitlenir, tokens.rs'te Türkçe kalmaz.
- Produces: `migrate::run(global: &Path, project_usta: Option<&Path>) -> anyhow::Result<usize>` (dönüş: değişen dosya sayısı). Task 6 bunu çağırır.

- [ ] **Step 1: Önce testleri yaz** (spec §5 a–f):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const MIXED: &str = "# rust — İlerleme\n\n## Seviye\n- orta\n\n## Hedef Durumu\nx\n\n## Hedef\ny\n\n\
## Geri çağırma soruları\n- soru — cevap | due: 2026-09-01 | ivl: 7\n\n## Kapatılanlar\n- a: oturdu\n\
- oturdu kelimesi cümle içinde geçen madde: görülmedi\n- b: derinleşildi | due: 2026-01-01\n— kaynak: web\n";

    #[test]
    fn full_conversion_and_prose_preserved() {
        let out = migrate_content(MIXED).unwrap();
        assert!(out.contains("# rust — Progress"));
        assert!(out.contains("## Level"));
        assert!(out.contains("## Goal Status"));
        assert!(out.contains("## Goal\n"));
        assert!(out.contains("## Recall questions"));
        assert!(out.contains("## Retired"));
        assert!(out.contains("- a: settled"));
        assert!(out.contains("- b: deepened | due: 2026-01-01"));
        assert!(out.contains("— source: web"));
        // Serbest metindeki "oturdu" kelimesi DOKUNULMADI, yalnız durum segmenti döndü:
        assert!(out.contains("- oturdu kelimesi cümle içinde geçen madde: not seen"));
    }

    #[test]
    fn idempotent_second_pass_is_none() {
        let once = migrate_content(MIXED).unwrap();
        assert!(migrate_content(&once).is_none()); // değişiklik yok → None
    }

    #[test]
    fn goal_status_maps_before_goal_prefix() {
        let out = migrate_content("## Hedef Durumu\n## Hedef\n").unwrap();
        assert_eq!(out, "## Goal Status\n## Goal\n");
    }

    #[test]
    fn english_file_untouched() {
        assert!(migrate_content("## Goal\n- a: settled\n").is_none());
    }
}
```

Dosya-düzeyi testler (tempfile ile — dev-dependency zaten var mı bak; yoksa `std::env::temp_dir` + benzersiz alt klasör kullan, YENİ CRATE EKLEME): `.bak` ilk-halde korunur (ikinci migrate `.bak`'ı ezmez), `run` ikinci koşuda 0 döner.

- [ ] **Step 2: Koş, kırmızı** — Run: `cargo test migrate -q`. Expected: FAIL (modül yok).

- [ ] **Step 3: Implementasyon:**

```rust
//! One-shot deterministic migration: legacy Turkish protocol tokens → English.
//! Context-locked — free prose is never touched. Idempotent. The ONLY place
//! legacy Turkish tokens are allowed to appear in src/.

use std::fs;
use std::path::Path;
use anyhow::Result;

/// Full-line (line-start) header mappings. ORDER MATTERS: longest prefix first.
const HEADERS: [(&str, &str); 12] = [
    ("## Hedef Durumu", "## Goal Status"),
    ("## Hedef", "## Goal"),
    ("## Tercihler", "## Preferences"),
    ("## Kayıtlar", "## Records"),
    ("## Seviye", "## Level"),
    ("## Kapatılanlar", "## Retired"),
    ("## Gap'ler", "## Gaps"),
    ("## Hata günlüğü", "## Error log"),
    ("## İpucu merdiveni", "## Hint ladder"),
    ("## Geri çağırma soruları", "## Recall questions"),
    ("## Açık egzersiz", "## Open exercise"),
    ("# Oturum Geçmişi", "# Session History"),
];

const STATES: [(&str, &str); 4] = [
    ("görülmedi", "not seen"),
    ("görüldü", "seen"),
    ("oturdu", "settled"),
    ("derinleşildi", "deepened"),
];

/// Substring markers — patterns unique enough to be context-free.
const MARKERS: [(&str, &str); 4] = [
    ("===DOSYA:", "===FILE:"),
    ("[ARA KAYIT]", "[CHECKPOINT]"),
    ("— kaynak:", "— source:"),
    ("- kaynak:", "- source:"),
];

/// Migrate one file's content. `None` = nothing to change (idempotence signal).
pub fn migrate_content(content: &str) -> Option<String> {
    let mut changed = false;
    let mut out = String::with_capacity(content.len());
    for line in content.split_inclusive('\n') {
        let (body, nl) = match line.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (line, ""),
        };
        let mut new = body.to_string();
        // 1) Full-line headers (exact line match, trailing space tolerated).
        for (old, newh) in HEADERS {
            if new.trim_end() == old { new = newh.to_string(); break; }
        }
        // 2) `# <topic> — İlerleme` heading.
        if new.starts_with("# ") && new.trim_end().ends_with("— İlerleme") {
            new = format!("{}— Progress", new.trim_end().strip_suffix("— İlerleme").unwrap());
        }
        // 3) Map-state segment on `- item: state [| due: …]` lines.
        if let Some(stripped) = new.strip_prefix("- ") {
            let (head, tail) = match stripped.find(" | ") {
                Some(i) => (&stripped[..i], &stripped[i..]),
                None => (stripped, ""),
            };
            if let Some(ci) = head.rfind(':') {
                let seg = head[ci + 1..].trim();
                if let Some((_, en)) = STATES.iter().find(|(tr, _)| *tr == seg) {
                    new = format!("- {}: {}{}", &head[..ci], en, tail);
                }
            }
        }
        // 4) Context-free markers.
        for (old, newm) in MARKERS {
            if new.contains(old) { new = new.replace(old, newm); }
        }
        if new != body { changed = true; }
        out.push_str(&new);
        out.push_str(nl);
    }
    changed.then_some(out)
}

fn migrate_file(path: &Path) -> Result<bool> {
    let Ok(content) = fs::read_to_string(path) else { return Ok(false) };
    let Some(new) = migrate_content(&content) else { return Ok(false) };
    let bak = path.with_extension("md.bak");
    if !bak.exists() { fs::copy(path, &bak)?; } // ilk hal korunur, asla ezilmez
    let tmp = path.with_extension("md.tmp");
    fs::write(&tmp, &new)?;
    fs::rename(&tmp, path)?; // atomik
    Ok(true)
}

/// Walk both trees; returns the number of migrated files.
pub fn run(global: &Path, project_usta: Option<&Path>) -> Result<usize> {
    let mut n = 0;
    let mut roots = vec![global.to_path_buf()];
    if let Some(p) = project_usta { roots.push(p.to_path_buf()); }
    for root in roots {
        for sub in ["", "learner", "learner/progress", "learner/curriculum",
                    "approaches", "sessions"] {
            let dir = if sub.is_empty() { root.clone() } else { root.join(sub) };
            let Ok(rd) = fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "md") && migrate_file(&p)? { n += 1; }
            }
        }
    }
    Ok(n)
}
```

NOT: `learner/` altındaki gerçek alt-dizin adlarını koda almadan önce doğrula: `ls ~/.config/usta/learner/` ve `src/progress.rs`/`src/brain.rs`'te yazım yolları grep'le (`join("learner")`, `join("progress")`, `join("curriculum")`). Liste gerçek yapıya göre düzeltilir — spec §4 "nereye bakar" bağlayıcı. `sessions/` kayıtlarının uzantısını `src/session.rs`'ten doğrula (md değilse o uzantıyı işle).

- [ ] **Step 4: Test** — Run: `cargo test migrate -q`. Expected: hepsi PASS. Sonra tam paket: `cargo test --quiet 2>&1 | tail -3` → yeşil.

- [ ] **Step 5: Commit** — `git commit -am "feat: migrate.rs — TR→EN token migration, bağlam-kilitli + idempotent + .bak"`

---

### Task 6: Migration'ı giriş noktalarına bağla + siyah-kutu test

**Files:**
- Modify: `src/main.rs` (komut dispatch — start/topics/stats/reset yollarının OKUMALARDAN ÖNCE ortak noktası; `grep -n 'global_root()\|find_project' src/main.rs` ile dispatch sonrası ilk ortak yeri bul)
- Test: `src/main.rs` veya `src/migrate.rs` entegrasyon testi (izole tempdir-HOME)

**Interfaces:**
- Consumes: `migrate::run(&global, project_usta.as_deref())`.
- Produces: her komutta sessiz migration; değişen dosya olduysa tek bilgi satırı.

- [ ] **Step 1: Bağla.** Dispatch'te global root çözüldükten ve proje `.usta` arandıktan hemen sonra, HERHANGİ bir dosya okumasından önce:

```rust
match migrate::run(&global, project_usta.as_deref()) {
    Ok(0) => {}
    Ok(n) => println!("· migrated {n} file(s) to English protocol tokens (backup: .bak)"),
    Err(e) => eprintln!("⚠ token migration skipped: {e}"), // migration hatası oturumu DÜŞÜRMEZ
}
```

(Bilgi satırı stili: mevcut `·` notice tier'ı — tui/theme kullanılıyorsa oradaki helper ile bas.)

- [ ] **Step 2: Entegrasyon testi yaz** — tempdir'de sahte global root kur (Türkçe token'lı USER.md + learner/progress/rust.md + learner/index.md), `migrate::run` çağır: dönüş ≥3, dosyalar İngilizce, `.bak`'lar ilk halde, ikinci koşu 0.

- [ ] **Step 3: Test** — Run: `cargo test --quiet 2>&1 | tail -3`. Expected: yeşil.

- [ ] **Step 4: Siyah-kutu tur (elle, izole HOME):**

```bash
export USTA_TEST_HOME=$(mktemp -d) && mkdir -p "$USTA_TEST_HOME/.config/usta/learner/progress"
printf '# rust — İlerleme\n## Seviye\n- orta\n- a: oturdu\n' > "$USTA_TEST_HOME/.config/usta/learner/progress/rust.md"
HOME="$USTA_TEST_HOME" cargo run -q -- topics
grep -c 'settled\|## Level' "$USTA_TEST_HOME/.config/usta/learner/progress/rust.md"   # beklenen: 2
ls "$USTA_TEST_HOME/.config/usta/learner/progress/"                                   # rust.md + rust.md.bak
HOME="$USTA_TEST_HOME" cargo run -q -- topics                                          # ikinci koşu: migration satırı YOK
```

(XDG_CONFIG_HOME kullanılıyorsa onu override et — config.rs:25'e bak.)

- [ ] **Step 5: Commit** — `git commit -am "feat: migration giriş noktalarına bağlandı — sessiz, idempotent, oturumu düşürmez"`

---

### Task 7: Doküman + sürüm + kapanış

**Files:**
- Modify: `SPEC.md` (Türkçe-kaldı glos/parantezleri temizle — `grep -n 'Tercihler\|Hedef\|Kapatılanlar\|DOSYA\|ARA KAYIT\|oturdu' SPEC.md` ile bul, İngilizce token'a çevir), `README.md` (varsa `Kapatılanlar` vb. anımları), `docs/ROADMAP.md` (tabloya satır: "English protocol tokens ✅ done (tarih) — spec linki"; Completed'e özet paragraf), `Cargo.toml` (`version = "0.20.0"`), `Cargo.lock` (`cargo build` günceller)

- [ ] **Step 1: SPEC/README temizliği** — spec dokümantasyonu artık runtime ile aynı token'ları anlatır; "(Preferences)" tarzı gloslar ve "kept Turkish" notları silinir.

- [ ] **Step 2: Roadmap + sürüm** — `Cargo.toml` 0.20.0; roadmap satırı + Completed özeti (tarih: gerçek gün).

- [ ] **Step 3: Final doğrulama:**

```bash
cargo test --quiet 2>&1 | tail -3          # yeşil, 325+ test
cargo clippy --quiet 2>&1 | tail -3        # yeni uyarı yok
grep -rn 'görülmedi\|oturdu\|Tercihler\|ARA KAYIT\|===DOSYA\|Kayıtlar' src/ *.md learner/ approaches/ | grep -v 'migrate.rs\|docs/'   # sıfır
```

- [ ] **Step 4: Commit + tag + kur** — `git commit -am "release: v0.20.0 — English protocol tokens" && git tag v0.20.0 && cargo install --path . && git push && git push --tags`

- [ ] **Step 5: Canlı veri doğrulaması** — gerçek HOME'da `usta topics` koş: migration satırı bir kez görünür, `~/.config/usta/learner/` dosyaları İngilizce + `.bak`'lı; ikinci koşu sessiz.
