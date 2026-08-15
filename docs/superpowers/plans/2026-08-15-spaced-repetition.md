# Spaced Repetition Implementation Plan (Roadmap #3)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** Çakışan bekleyen plan yok. Spec: `docs/superpowers/specs/2026-08-15-spaced-repetition-design.md` — önce oku.

**Goal:** Geri-çağırma soruları `| due: YYYY-MM-DD | ivl: <gün>` kuyruğu taşır (basitleştirilmiş SM-2 merdiveni `1→3→7→16→35→90`); drill yalnız vadesi gelenleri sorar; welcome "Reviews due today: N" gösterir.

**Architecture:** Çizelgeleme tamamen model tarafında (closing/opening prompt kuralları — kabuk hesap yapmaz). Kabuk yalnız sayar: `welcome.rs`'e saf `due_count(progress, today)`, `WelcomeData`'ya alan, `gather`'a `today` parametresi. ISO tarih string karşılaştırması yeterli (`"2026-08-14" <= "2026-08-15"` leksikografik doğru).

**Tech Stack:** Rust (mevcut crate, yeni bağımlılık YOK). Test: in-module `#[cfg(test)]`.

## Global Constraints

- Prompt/UI metinleri İngilizce; progress bölüm başlıkları Türkçe (mevcut konvansiyon — `## Geri çağırma soruları`).
- Merdiven sabit: `1, 3, 7, 16, 35, 90` gün. Ease factor YOK.
- Kuyruksuz (eski format) madde = bugün vadeli sayılır — hem `due_count` hem prompt kuralı aynı davranır.
- Her task sonunda commit (Türkçe mesaj) + push + `cargo test` yeşil; imza kıran task tüm call site'ları aynı task'ta günceller.

---

### Task 1: `due_count` saf sayaç (`src/tui/welcome.rs`)

**Files:**
- Modify: `src/tui/welcome.rs` (`drill_count`'un yanı, ~satır 84)
- Test: `src/tui/welcome.rs` in-module tests

**Interfaces:**
- Produces: `pub fn due_count(progress: &str, today: &str) -> usize`. Task 2 kullanır.
- Consumes: mevcut bölüm-ayrıştırma yaklaşımı (`drill_count` nasıl `## Geri çağırma soruları` maddelerini sayıyorsa aynı bölüm sınırlama mantığı — koda bak, birebir aynı bölüm tespiti kullanılsın ki iki sayaç tutarlı kalsın).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn due_count_counts_due_and_untagged_skips_future() {
    let p = "\
# rust — İlerleme

## Geri çağırma soruları
- Borrow checker ne yapar? — sahipliği derlemede doğrular | due: 2026-08-14 | ivl: 3
- Trait nedir? — davranış sözleşmesi | due: 2026-08-15 | ivl: 1
- Lifetime nedir? — referans ömrü | due: 2026-09-01 | ivl: 35
- Eski format soru — cevap

## Hata günlüğü
- typo | 1 | due: 2026-08-01 gibi görünen ama başka bölümde
";
    // past + today + untagged = 3; future (09-01) and other-section lines don't count
    assert_eq!(due_count(p, "2026-08-15"), 3);
    assert_eq!(due_count(p, "2026-08-13"), 1); // only untagged counts as due
    assert_eq!(due_count("# bos", "2026-08-15"), 0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test due_count`
Expected: derleme hatası — fonksiyon yok

- [ ] **Step 3: Write minimal implementation**

`drill_count`'un bölüm-sınırlama mantığını aynen izleyen implementasyon (aynı başlık sabiti / aynı bölüm-bitiş koşulu — `drill_count` kodunu oku, kopyala, madde işleme kısmını değiştir):

```rust
/// Count recall questions due today or earlier. A bullet without a
/// `| due: YYYY-MM-DD` tail is legacy format and counts as due (it gets its
/// tail at the next closing flush). ISO date strings compare lexicographically.
pub fn due_count(progress: &str, today: &str) -> usize {
    let mut in_section = false;
    let mut n = 0;
    for line in progress.lines() {
        let t = line.trim();
        if t.starts_with("## ") {
            in_section = t.starts_with("## Geri çağırma soruları");
            continue;
        }
        if !in_section || !t.starts_with("- ") {
            continue;
        }
        match t.find("due: ") {
            None => n += 1, // legacy, no schedule tail → due now
            Some(i) => {
                let date = t[i + 5..].chars().take(10).collect::<String>();
                if date.as_str() <= today {
                    n += 1;
                }
            }
        }
    }
    n
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test due_count`
Expected: PASS

- [ ] **Step 5: Commit + push**

```bash
git add src/tui/welcome.rs
git commit -m "tekrar: due_count — vadesi gelen geri-çağırma sorularını sayar"
git push
```

---

### Task 2: `WelcomeData.due_count` + `gather(today)` + render

**Files:**
- Modify: `src/tui/welcome.rs` (`WelcomeData` ~21, `gather` ~91, render `Drill:` satırı ~165) + welcome testlerindeki `gather` çağrıları (~338, 342, 350, 365)
- Modify: `src/tui/run.rs:638` (`gather` çağrısı — `today` bu scope'ta var; yoksa `run` parametrelerinden geçir)
- Test: `src/tui/welcome.rs` in-module tests

**Interfaces:**
- Consumes: Task 1 `due_count`.
- Produces: `WelcomeData { due_count: usize, ... }`; `gather(profile, progress, curriculum, topic, model, dir, today)` (yeni son parametre `today: &str`).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn welcome_shows_due_line_three_states() {
    // state 1: due questions exist → "Reviews due today: N"
    let p_due = "## Geri çağırma soruları\n- q — a | due: 2026-01-01 | ivl: 1\n";
    let d = gather(None, Some(p_due), None, "rust", "opus · cli", "~/x", "2026-08-15");
    let joined = render_lines_joined(&d); // mevcut testlerin render-join yardımcısı neyse onu kullan
    assert!(joined.contains("Reviews due today: 1"));

    // state 2: questions exist, none due → "No reviews due today"
    let p_future = "## Geri çağırma soruları\n- q — a | due: 2099-01-01 | ivl: 90\n";
    let d = gather(None, Some(p_future), None, "rust", "opus · cli", "~/x", "2026-08-15");
    let joined = render_lines_joined(&d);
    assert!(joined.contains("No reviews due today"));
    assert!(!joined.contains("Reviews due today:"));

    // state 3: no questions at all → neither line
    let d = gather(None, Some("# bos"), None, "rust", "opus · cli", "~/x", "2026-08-15");
    let joined = render_lines_joined(&d);
    assert!(!joined.contains("Reviews due"));
    assert!(!joined.contains("No reviews due"));
}
```

(`render_lines_joined`: mevcut welcome testleri `render_welcome` çıktısını nasıl string'e çeviriyorsa aynı yardımcı/desen — koda bak, varsa kullan, yoksa aynı inline deseni tekrar et.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib welcome`
Expected: derleme hatası (gather parametre sayısı)

- [ ] **Step 3: Implement**

- `WelcomeData`'ya alan: `pub due_count: usize,`
- `gather` imzasına son parametre `today: &str`; gövdeye: `due_count: progress.map(|p| due_count(p, today)).unwrap_or(0),`
- Render (~165) — eski:
```rust
        if d.drill_count > 0 { right.push((format!("Drill: {} question(s) ready", d.drill_count), Style::default())); }
```
yeni:
```rust
        if d.due_count > 0 {
            right.push((format!("Reviews due today: {}", d.due_count), Style::default()));
        } else if d.drill_count > 0 {
            right.push(("No reviews due today".to_string(), Style::default()));
        }
```
- `src/tui/run.rs:638` `gather` çağrısına `today` argümanı ekle (scope'ta yoksa `run`'ın çağrı zincirinden geçir — `build_session` çağrısında `today` üretiliyor, aynı değeri kullan).
- Welcome testlerindeki mevcut 4 `gather` çağrısına `"2026-08-15"` gibi sabit `today` ekle; `Drill:` string'ini assert eden mevcut test varsa yeni metne uyarla.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: tümü PASS

- [ ] **Step 5: Commit + push**

```bash
git add src/tui/welcome.rs src/tui/run.rs
git commit -m "tekrar: welcome vade-farkında — Reviews due today: N / No reviews due today"
git push
```

---

### Task 3: Kapanış çizelgeleme kuralı (`src/progress.rs` `closing_prompt`)

**Files:**
- Modify: `src/progress.rs` (`closing_prompt` progress kuralındaki `Geri çağırma soruları` cümlesi)
- Test: `src/progress.rs` in-module tests

**Interfaces:** imza değişikliği YOK — yalnız kural metni.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn closing_prompt_defines_spaced_repetition_schedule() {
    let s = closing_prompt("rust", None, None, None, None, None, None);
    assert!(s.contains("due: YYYY-MM-DD"));
    assert!(s.contains("ivl:"));
    assert!(s.contains("1, 3, 7, 16, 35, 90"));
    assert!(s.contains("Kapatılanlar")); // retirement target already exists; schedule rule must mention retire path
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test closing_prompt_defines_spaced`
Expected: FAIL (assert)

- [ ] **Step 3: Implement**

`closing_prompt` içindeki mevcut `## Geri çağırma soruları` kural parçasını genişlet — eski çekirdek: "(3-5 questions + one-line answer each; drop settled old ones, add new ones from this session)" → yeni:

```text
(each bullet: `- <question> — <one-line answer> | due: YYYY-MM-DD | ivl: <days>`. \
Simplified spaced repetition, interval ladder in days: 1, 3, 7, 16, 35, 90. \
Compute dates from the TODAY section. A question recalled comfortably in this \
session's drill moves one rung up and gets `due = today + new ivl`; a question \
answered wrong or with struggle resets to `ivl: 1` (due tomorrow); a question \
not drilled this session keeps its tail UNCHANGED; a new question starts at \
`ivl: 1` (due tomorrow); a legacy bullet without a tail gets `ivl: 1` (due \
tomorrow). A question passed comfortably at `ivl: 90` retires: move it to \
`Kapatılanlar` as a one-line summary and remove it from this list.)
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: tümü PASS (mevcut closing_prompt testleri kırılırsa metin uyarlaması yap — "drop settled old ones" assert'i varsa yeni metinle güncelle)

- [ ] **Step 5: Commit + push**

```bash
git add src/progress.rs
git commit -m "tekrar: kapanış çizelgeleme kuralı — due/ivl kuyruğu + 1-3-7-16-35-90 merdiveni"
git push
```

---

### Task 4: Açılış drilli vade-farkında (`src/progress.rs` `opening_prompt`)

**Files:**
- Modify: `src/progress.rs` (`opening_prompt`)
- Test: `src/progress.rs` in-module tests

**Interfaces:** imza değişikliği YOK.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn opening_prompt_drills_only_due_questions() {
    let s = opening_prompt("rust", false, false);
    assert!(s.contains("due"));
    assert!(s.contains("no reviews due today"));
    assert!(s.contains("oldest due first"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test opening_prompt_drills`
Expected: FAIL

- [ ] **Step 3: Implement**

`opening_prompt` format string'inde drill talimatını güncelle — eski: "Pick 2-3 questions from the 'Recall questions' section of your progress file and ASK me" civarı → yeni (aynı yerinde):

```text
Pick ONLY questions from your progress file whose `due:` date is today or \
earlier (TODAY is in your system prompt; a bullet without a `due:` tail counts \
as due) — at most 3, oldest due first — and ASK me; don't answer them yourself. \
If NO question is due, say exactly one sentence: 'no reviews due today', skip \
the drill and move straight to today's work.
```

Mevcut "If progress has no questions, come up with 2 small recall questions" cümlesi KALIR (hiç soru yokken geçerli).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: tümü PASS (eski "Pick 2-3 questions" assert'i varsa güncelle)

- [ ] **Step 5: Commit + push**

```bash
git add src/progress.rs
git commit -m "tekrar: drill yalnız vadesi gelen soruları sorar — vadeli yoksa atlanır"
git push
```

---

### Task 5: Dokümantasyon + final doğrulama

**Files:**
- Modify: `SPEC.md` (drill/progress maddeleri), `README.md` (Highlights pedagoji satırı), `docs/ROADMAP.md` (#3 → ✅)

- [ ] **Step 1: SPEC.md** — drill maddesine vade kuralı + progress soru formatı (`due/ivl` kuyruğu, merdiven, emeklilik) eklenir.

- [ ] **Step 2: README.md (İngilizce)** — Pedagogy satırına ek: `..., spaced repetition (recall questions carry due dates on a 1→3→7→16→35→90-day ladder — the opening drill asks only what's due; the welcome box shows "Reviews due today: N")`.

- [ ] **Step 3: ROADMAP** — #3 satırı `✅ tamamlandı (2026-08-15)` + Tamamlananlar'a satır.

- [ ] **Step 4: Verify**

Run: `cargo test` → tümü PASS · `cargo clippy --all-targets` → yeni uyarı 0 · `cargo install --path .` → başarılı

- [ ] **Step 5: Commit + push**

```bash
git add SPEC.md README.md docs/ROADMAP.md
git commit -m "tekrar: SPEC + README + roadmap #3 kapandı"
git push
```

- [ ] **Step 6 (elle doğrulama — ATLA, Anil koşacak):** vadeli soru içeren konu aç → drill yalnız vadesi gelenleri sormalı; welcome "Reviews due today: N" göstermeli; kapanışta kuyruklar güncellenmiş olmalı.
