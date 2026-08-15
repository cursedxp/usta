# İlerleme Özeti / Motivasyon Implementation Plan (Roadmap #6)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** `2026-08-15-material-ingest` planı MERGE EDİLMİŞ olmalı (aynı dosyalara dokunuyor — main.rs, welcome.rs). Spec: `docs/superpowers/specs/2026-08-15-progress-stats-design.md` — önce oku.

**Goal:** Kapanışta `learner/history.md`'ye oturum satırı; `usta stats` haftalık özeti (LLM'siz, ADHD-safe ton); welcome'da "This week: N session(s) · streak M day(s)"; v0.15.0.

**Architecture:** Yeni `src/history.rs`: satır formatı + append + parser + streak/hafta hesapları (saf, chrono ile). `flush_progress` kayıt düşer; `usta stats` CLI komutu `render_stats` saf fonksiyonunu basar; `gather` history içeriği alır.

**Tech Stack:** Rust; chrono zaten bağımlı — YENİ BAĞIMLILIK YOK.

## Global Constraints

- UI/çıktı metinleri İngilizce; history dosya başlığı Türkçe (`# Oturum Geçmişi`) — dosya-içi konvansiyon.
- ADHD-safe: "current streak: 0" HİÇBİR yüzeyde yazılmaz; kırık seride yalnız `longest streak` pozitif çerçeve.
- Satır formatı TEK yerde (`record_line`) — parser onunla round-trip test edilir.
- Her task sonunda commit (Türkçe mesaj) + push + `cargo test` yeşil.

---

### Task 1: `src/history.rs` — format, parser, hesaplar

**Files:**
- Create: `src/history.rs` · Modify: `src/main.rs` (`mod history;`)
- Test: `src/history.rs` in-module

**Interfaces (Produces):**
```rust
pub struct Entry { pub date: String, pub topic: String, pub map: Option<u8>, pub settled: Option<usize> }
pub struct TopicWeek { pub topic: String, pub sessions: u32, pub map_from: Option<u8>, pub map_to: Option<u8>, pub settled_from: Option<usize>, pub settled_to: Option<usize> }
pub struct WeekSummary { pub sessions: u32, pub per_topic: Vec<TopicWeek> }
pub fn record_line(date: &str, topic: &str, map_percent: Option<u8>, settled: Option<usize>) -> String
pub fn entries(content: &str) -> Vec<Entry>
pub fn append(global: &Path, line: &str) -> Result<()>
pub fn current_streak(entries: &[Entry], today: &str) -> u32
pub fn longest_streak(entries: &[Entry]) -> u32
pub fn week_summary(entries: &[Entry], today: &str) -> WeekSummary
pub fn settled_count(curriculum: &str) -> Option<usize>
```

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn record_line_entries_roundtrip_and_skip_malformed() {
    let l1 = record_line("2026-08-15", "rust", Some(55), Some(7));
    let l2 = record_line("2026-08-15", "gtm", None, None);
    let content = format!("# Oturum Geçmişi\n{l1}\n{l2}\nbozuk satır\n");
    let es = entries(&content);
    assert_eq!(es.len(), 2);
    assert_eq!(es[0].topic, "rust");
    assert_eq!(es[0].map, Some(55));
    assert_eq!(es[0].settled, Some(7));
    assert_eq!(es[1].map, None);
}

#[test]
fn streaks_count_consecutive_days() {
    let mk = |d: &str| Entry { date: d.into(), topic: "t".into(), map: None, settled: None };
    let es = vec![mk("2026-08-10"), mk("2026-08-13"), mk("2026-08-14"), mk("2026-08-15"), mk("2026-08-15")];
    assert_eq!(current_streak(&es, "2026-08-15"), 3);
    // bugün oturum yok ama dün biten seri güncel sayılır
    assert_eq!(current_streak(&es, "2026-08-16"), 3);
    // bir günden fazla boşluk → seri bitti
    assert_eq!(current_streak(&es, "2026-08-18"), 0);
    assert_eq!(longest_streak(&es), 3);
    assert_eq!(current_streak(&[], "2026-08-15"), 0);
}

#[test]
fn week_summary_windows_and_groups() {
    let mk = |d: &str, t: &str, m: u8, s: usize| Entry { date: d.into(), topic: t.into(), map: Some(m), settled: Some(s) };
    let es = vec![
        mk("2026-08-07", "rust", 30, 3), // 8 gün önce — pencere dışı
        mk("2026-08-09", "rust", 40, 4),
        mk("2026-08-14", "rust", 55, 7),
        mk("2026-08-15", "gtm", 10, 1),
    ];
    let w = week_summary(&es, "2026-08-15");
    assert_eq!(w.sessions, 3);
    let rust = w.per_topic.iter().find(|t| t.topic == "rust").unwrap();
    assert_eq!((rust.sessions, rust.map_from, rust.map_to), (2, Some(40), Some(55)));
}

#[test]
fn settled_count_counts_settled_states() {
    let c = "- a: oturdu\n- b: görüldü\n- c: derinleşildi\n- d: görülmedi\n";
    assert_eq!(settled_count(c), Some(2));
    assert_eq!(settled_count(""), Some(0));
}

#[test]
fn append_creates_with_header_then_appends() {
    let base = std::env::temp_dir().join(format!("usta_history_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    append(&base, &record_line("2026-08-15", "rust", None, None)).unwrap();
    append(&base, &record_line("2026-08-15", "gtm", None, None)).unwrap();
    let c = std::fs::read_to_string(base.join("learner/history.md")).unwrap();
    assert!(c.starts_with("# Oturum Geçmişi"));
    assert_eq!(entries(&c).len(), 2);
    let _ = std::fs::remove_dir_all(&base);
}
```

- [ ] **Step 2:** Run: `cargo test --lib history` → derleme hatası (modül yok)

- [ ] **Step 3: Implement**

Format: `- {date} | {topic} | map {P}% | settled {N}` — None ise `map -` / `settled -`. Parser: `- ` ile başlayan satırı ` | ` ile 4 parçaya böl; tarih `NaiveDate::parse_from_str(.., "%Y-%m-%d")` ile doğrulanır (bozuksa atla); `map ` / `settled ` önekleri soyulur, `-` → None. Streak'ler: tarihleri `BTreeSet<NaiveDate>`'e indir; `current_streak`: bugün setteyse bugünden, değilse dünden başla (dün de yoksa 0), geriye ardışık say; `longest_streak`: set üzerinde ardışık koşu taraması. `week_summary`: `today - 6 gün` alt sınır (dahil), konuya grupla (ilk görülen sıra korunur), her konuda pencere-içi İLK ve SON entry'nin map/settled'ı from/to. `settled_count`: satırlarda `oturdu` veya `derinleşildi` içeren `- ` maddelerini say (welcome STATUSES sabitiyle aynı kelimeler — kopyaysa yorumda not düş). `append`: `learner/` klasörünü kur, yoksa `# Oturum Geçmişi\n\n` başlığıyla başlat, satır + `\n` ekle, `progress::write_atomic` ile yaz.

- [ ] **Step 4:** Run: `cargo test --lib history` → PASS

- [ ] **Step 5: Commit + push**

```bash
git add src/history.rs src/main.rs
git commit -m "özet: history modülü — oturum kaydı formatı, streak ve hafta hesapları"
git push
```

---

### Task 2: Kapanışta kayıt (`flush_progress`)

**Files:** Modify: `src/main.rs` (`flush_progress`, `index::record` bloğunun yanı)

- [ ] **Step 1: Implement** — katalog güncellemesinin hemen ardından (global Some ise):

```rust
            // Session history line — powers streaks/weekly stats (spec: progress stats).
            let cur = std::fs::read_to_string(&c_path).ok();
            let map = cur.as_deref().and_then(crate::tui::welcome::curriculum_percent);
            let settled = cur.as_deref().and_then(history::settled_count);
            let line = history::record_line(&today(), &session.topic, map, settled);
            if let Err(e) = history::append(g, &line) {
                ui::warn(&format!("history could not be updated: {e}"));
            }
```

(`curriculum_percent` görünürlüğü: `pub` değilse `pub` yap — koda bak. `c_path` flush'ta zaten var.)

- [ ] **Step 2:** `cargo build && cargo test` → PASS. Commit + push:

```bash
git add src/main.rs
git commit -m "özet: kapanış flush'ı learner/history.md'ye oturum satırı düşer"
git push
```

---

### Task 3: `usta stats` komutu

**Files:** Modify: `src/main.rs` (CLI komut ayrıştırma — `topics` deseni; `render_stats` saf fn + testler), `src/help.rs` (Terminal commands listesine `usta stats`)

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn render_stats_full_quiet_and_empty() {
    let mk = |d: &str, t: &str| history::Entry { date: d.into(), topic: t.into(), map: Some(40), settled: Some(4) };
    let full = render_stats(&[mk("2026-08-14", "rust"), mk("2026-08-15", "rust")], "2026-08-15");
    assert!(full.contains("rust"));
    assert!(full.contains("2 session(s)"));
    assert!(full.contains("current streak: 2 day(s)"));

    // kırık seri: current 0 → yazılmaz, longest pozitif çerçeve
    let broken = render_stats(&[mk("2026-08-01", "rust")], "2026-08-15");
    assert!(!broken.contains("current streak"));
    assert!(broken.contains("longest streak"));
    assert!(broken.contains("quiet week"));

    let empty = render_stats(&[], "2026-08-15");
    assert!(empty.contains("no sessions recorded yet"));
}
```

- [ ] **Step 2:** Run → FAIL (fn yok)

- [ ] **Step 3: Implement** — `render_stats(entries: &[history::Entry], today: &str) -> String` spec'teki örnek çıktı formatıyla (hafta bloğu; konu satırları `map X% → Y%` yalnız ikisi de Some ise, değilse o hücre boş; toplam satırı; streak kuralı: current>0 → `current streak: N day(s) · longest: M day(s)`, current==0 → yalnız `longest streak: M day(s)`; boş hafta → `quiet week — your longest streak is still M day(s)`; hiç entry → `no sessions recorded yet — streaks start with the first one.`). CLI: `"stats"` komutu → global root'tan `learner/history.md` oku (yoksa boş), `println!("{}", render_stats(...))`. `help.rs` Terminal commands bloğuna `usta stats             this week + streaks` satırı (+ help testi güncelle).

- [ ] **Step 4:** `cargo test` → PASS. Commit + push:

```bash
git add src/main.rs src/help.rs
git commit -m "özet: usta stats — haftalık özet + streak, ADHD-safe ton"
git push
```

---

### Task 4: Welcome satırı

**Files:** Modify: `src/tui/welcome.rs` (`WelcomeData` + `gather` + iki render), `src/tui/run.rs` (gather çağrıları — history içeriği global'den okunur)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn welcome_shows_week_line() {
    let h = "# Oturum Geçmişi\n- 2026-08-14 | rust | map 40% | settled 4\n- 2026-08-15 | rust | map 55% | settled 7\n";
    let d = gather(None, None, None, "rust", "opus · cli", "~/x", "2026-08-15", Some(h));
    // render join yardımcıyla:
    // "This week: 2 session(s) · streak 2 day(s)" içerir
    // streak 0 senaryosu: yalnız "This week: N session(s)" — " · streak" yok
    // history None → satır yok
}
```

(Gövde mevcut welcome test desenleriyle doldurulur — üç durum assert edilir.)

- [ ] **Step 2:** Run → derleme hatası (gather parametre)

- [ ] **Step 3: Implement** — `gather`'a son parametre `history: Option<&str>`; `week_sessions` = `week_summary(...).sessions`, `streak` = `current_streak(...)` (`crate::history` kullan — welcome, history modülünü görebilir; görünürlük sorunu olursa hesapları çağrı yerinde yapıp `gather`'a sayı geçir — koda bak, hangisi temizse). Render: iki kutuda da (identity + full) `week_sessions > 0` ise satır. Çağrı yerleri (`run.rs` iki gather noktası): `read(global.join("learner/history.md"))` içeriği geçir. Mevcut gather test çağrılarına `None` ekle.

- [ ] **Step 4:** `cargo test` → PASS. Commit + push:

```bash
git add src/tui/welcome.rs src/tui/run.rs
git commit -m "özet: welcome haftalık satır — This week: N session(s) · streak M day(s)"
git push
```

---

### Task 5: Docs + v0.15.0

**Files:** `SPEC.md`, `README.md`, `docs/ROADMAP.md`, `Cargo.toml`(+lock), sürüm testi

- [ ] **Step 1:** SPEC yeni § (v0.15): history formatı, stats komutu, welcome satırı, ADHD-safe kurallar.
- [ ] **Step 2:** README Highlights (İngilizce): `| 📈 **Visible progress** | Every session lands in a lightweight history — `usta stats` shows your week (sessions, map %, settled items) and streaks. Broken streak? No guilt: it shows your longest instead. |`
- [ ] **Step 3:** ROADMAP #6 `✅ tamamlandı (2026-08-15)` + Tamamlananlar satırı.
- [ ] **Step 4:** Cargo.toml `0.15.0`; sürüm testi `"0.15.0"`; `cargo build`.
- [ ] **Step 5:** Verify: `cargo test` PASS · clippy yeni uyarı 0 · `cargo install --path .` başarılı.

```bash
git add SPEC.md README.md docs/ROADMAP.md Cargo.toml Cargo.lock src/
git commit -m "özet: SPEC + README + roadmap #6 kapandı — v0.15.0"
git push
git tag v0.15.0 && git push --tags
```

- [ ] **Step 6 (elle doğrulama — ATLA, Anil koşacak):** birkaç oturum sonrası `usta stats` haftayı göstermeli; welcome'da hafta satırı; kırık seride "current streak 0" görünmemeli.
