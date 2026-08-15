# Egzersiz/Artefakt Döngüsü Implementation Plan (Roadmap #2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** Çakışan bekleyen plan yok. Spec: `docs/superpowers/specs/2026-08-15-exercise-loop-design.md` — önce oku.

**Goal:** `exercises/` altına kaydedilen dosyalar egzersiz-teslimatı olarak çerçevelenir (assignment'a karşı Socratic değerlendirme, `cargo check` atlanır); açık egzersiz progress'e yazılır, açılışta hatırlatılır; scaffold `exercises/` kurar; pedagoji kuralı TEACHING.md'de.

**Architecture:** Yeni izleme mekanizması YOK — watcher zaten uzantı-agnostik. `handle_file_change`'in prompt kurulumu saf `feedback_frame` fonksiyonuna çekilir (test edilebilirlik), path-tanıma saf `is_exercise_path` ile. Kalıcılık mevcut closing/opening prompt kural metinleriyle.

**Tech Stack:** Rust (mevcut crate, yeni bağımlılık YOK). Test: in-module `#[cfg(test)]`.

## Global Constraints

- Kod yorumları ve prompt metinleri İngilizce; progress bölüm başlığı Türkçe: `## Açık egzersiz` (mevcut Türkçe başlık konvansiyonu).
- `exercises/` dışı dosyaların feedback çerçevesi ve `cargo check` davranışı BİREBİR korunur (regresyon kırmızı çizgi).
- Her task sonunda commit (Türkçe mesaj) + push + `cargo test` yeşil.

---

### Task 1: `is_exercise_path` saf yardımcı (`src/main.rs`)

**Files:**
- Modify: `src/main.rs` (`handle_file_change`'in üstü, ~satır 1260)
- Test: `src/main.rs` in-module tests

**Interfaces:**
- Produces: `pub(crate) fn is_exercise_path(project_root: &Path, path: &Path) -> bool`. Task 2 kullanır.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn is_exercise_path_detects_exercises_dir() {
    let root = Path::new("/tmp/proj");
    assert!(is_exercise_path(root, Path::new("/tmp/proj/exercises/a.md")));
    assert!(is_exercise_path(root, Path::new("/tmp/proj/exercises/gtm/brief.md")));
    assert!(!is_exercise_path(root, Path::new("/tmp/proj/src/exercises.rs")));
    assert!(!is_exercise_path(root, Path::new("/tmp/proj/mentor/PROJECT.md")));
    // watcher may hand a path the root-strip doesn't cover — component scan fallback
    assert!(is_exercise_path(root, Path::new("/other/place/exercises/x.md")));
    assert!(!is_exercise_path(root, Path::new("/other/place/src/lib.rs")));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test is_exercise_path`
Expected: derleme hatası — fonksiyon yok

- [ ] **Step 3: Write minimal implementation**

```rust
/// Is this saved file an exercise deliverable? True when a path component is
/// the `exercises/` dir (project-root-relative when possible; the watcher
/// hands absolute paths, so we fall back to scanning the path as-is).
pub(crate) fn is_exercise_path(project_root: &Path, path: &Path) -> bool {
    let rel = path.strip_prefix(project_root).unwrap_or(path);
    rel.components().any(|c| c.as_os_str() == "exercises")
}
```

Not: `src/exercises.rs` bir DOSYA bileşeni (`exercises.rs` ≠ `exercises`), any-component eşleşmez — test bunu kilitler.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test is_exercise_path`
Expected: PASS

- [ ] **Step 5: Commit + push**

```bash
git add src/main.rs
git commit -m "egzersiz: is_exercise_path — exercises/ altını path bileşeninden tanır"
git push
```

---

### Task 2: `feedback_frame` + `handle_file_change` entegrasyonu

**Files:**
- Modify: `src/main.rs:1266-1308` (`handle_file_change`)
- Test: `src/main.rs` in-module tests

**Interfaces:**
- Consumes: Task 1 `is_exercise_path`.
- Produces: `pub(crate) fn feedback_frame(is_exercise: bool, path_display: &str, body: &str, is_diff: bool) -> String` — FirstSight (`is_diff=false`) ve Diff (`is_diff=true`) çerçeveleri.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn feedback_frame_regular_paths_keep_existing_wording() {
    let s = feedback_frame(false, "src/main.rs", "fn main() {}", false);
    assert!(s.contains("[File saved: src/main.rs]"));
    assert!(s.contains("Give project-grounded, Socratic feedback on this change."));
    let d = feedback_frame(false, "src/main.rs", "-a\n+b", true);
    assert!(d.contains("[File changed: src/main.rs]"));
    assert!(d.contains("focus on what changed"));
}

#[test]
fn feedback_frame_exercise_paths_review_as_exercise() {
    let s = feedback_frame(true, "exercises/gtm/brief.md", "draft", false);
    assert!(s.contains("[Exercise submission saved: exercises/gtm/brief.md]"));
    assert!(s.contains("AS AN EXERCISE"));
    assert!(s.contains("hint ladder"));
    assert!(s.contains("do NOT rewrite"));
    let d = feedback_frame(true, "exercises/gtm/brief.md", "-a\n+b", true);
    assert!(d.contains("[Exercise submission changed: exercises/gtm/brief.md]"));
    assert!(d.contains("previous feedback"));
    assert!(d.contains("never hand over the solution"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test feedback_frame`
Expected: derleme hatası — fonksiyon yok

- [ ] **Step 3: Implement `feedback_frame`**

```rust
/// Build the injected user-turn for a watched-file change. Exercise files get
/// an exercise-review frame (assignment comparison, hint ladder, no solutions);
/// everything else keeps the original project-feedback wording VERBATIM.
pub(crate) fn feedback_frame(is_exercise: bool, path_display: &str, body: &str, is_diff: bool) -> String {
    match (is_exercise, is_diff) {
        (false, false) => format!(
            "[File saved: {path_display}]\n{body}\n\nGive project-grounded, Socratic feedback on this change."
        ),
        (false, true) => format!(
            "[File changed: {path_display}]\nChange (unified diff):\n{body}\n\nGive project-grounded, Socratic feedback on this change — focus on what changed."
        ),
        (true, false) => format!(
            "[Exercise submission saved: {path_display}]\n{body}\n\nThis is the user's deliverable for the exercise you assigned. Review it AS AN EXERCISE: compare against the assignment, apply the hint ladder (start high), point at what to reconsider — do NOT rewrite or complete it for them. If no exercise was assigned this session, treat it as spontaneous practice work and review it the same way."
        ),
        (true, true) => format!(
            "[Exercise submission changed: {path_display}]\nChange (unified diff):\n{body}\n\nReview the revision AS AN EXERCISE iteration: did it address your previous feedback? Move one rung down the hint ladder only if they're stuck — never hand over the solution."
        ),
    }
}
```

- [ ] **Step 4: Wire into `handle_file_change`**

`handle_file_change` içinde:

```rust
    let exercise = is_exercise_path(project_root, path);
    let mut injected = match files.observe(path, contents) {
        feedback::ChangePayload::Skip => return Ok(FileFeedback::Sessiz),
        feedback::ChangePayload::TooLarge(len) => { /* mevcut hali aynen */ }
        feedback::ChangePayload::FirstSight(full) => {
            feedback_frame(exercise, &path.display().to_string(), &full, false)
        }
        feedback::ChangePayload::Diff(diff) => {
            feedback_frame(exercise, &path.display().to_string(), &diff, true)
        }
    };
    if !exercise {
        if let Some(check_result) = check::run_check(project_root).await {
            /* mevcut push_str bloğu aynen */
        }
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test`
Expected: tümü PASS

- [ ] **Step 6: Commit + push**

```bash
git add src/main.rs
git commit -m "egzersiz: feedback_frame — exercises/ teslimatı egzersiz olarak değerlendirilir, check atlanır"
git push
```

---

### Task 3: Açık egzersiz kalıcılığı (`src/progress.rs`)

**Files:**
- Modify: `src/progress.rs` (`closing_prompt` progress kuralı; `opening_prompt`)
- Test: `src/progress.rs` in-module tests

**Interfaces:**
- Consumes: yok (yalnız kural metni).
- Produces: closing'de `## Açık egzersiz` üretim kuralı; opening'de hatırlatma cümlesi. İmza değişikliği YOK.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn closing_prompt_defines_open_exercise_section() {
    let s = closing_prompt("rust", None, None, None, None, None, None);
    assert!(s.contains("## Açık egzersiz"));
    assert!(s.contains("assigned"));
}

#[test]
fn opening_prompt_reminds_open_exercise() {
    let s = opening_prompt("rust", false, false);
    assert!(s.contains("open exercise"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test open_exercise && cargo test opening_prompt_reminds`
Expected: FAIL (assert)

- [ ] **Step 3: Implement**

- `closing_prompt` içindeki `progress` kuralına (mevcut `## Hedef Durumu` cümlesinden sonra, aynı madde içinde) ekle:

```text
 / `## Açık egzersiz` — ONLY if an exercise was assigned this session (or an \
earlier one is still open) and not completed: `- <file> | <one-line assignment> \
| assigned YYYY-MM-DD`. A completed exercise moves to `Kapatılanlar` as a \
normal item and leaves this section.
```

- `opening_prompt` format string'ine (drill talimatının sonuna) ekle:

```text
 If your progress file has an `## Açık egzersiz` section, remind me in ONE \
sentence after the drill: open exercise: <file> — continue or discuss it.
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: tümü PASS

- [ ] **Step 5: Commit + push**

```bash
git add src/progress.rs
git commit -m "egzersiz: açık egzersiz progress'e yazılır, açılışta hatırlatılır"
git push
```

---

### Task 4: Scaffold `exercises/` kurar (`src/main.rs`)

**Files:**
- Modify: `src/main.rs` `write_project_scaffold` (mentor bloğunun yanı)
- Test: `src/main.rs` in-module tests

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn write_project_scaffold_creates_visible_exercises_dir() {
    let base = std::env::temp_dir().join(format!(
        "usta_main_test_exercises_scaffold_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();

    write_project_scaffold(&base).unwrap();
    assert!(base.join("exercises").is_dir());
    assert!(base.join("exercises/.gitkeep").is_file());

    let _ = std::fs::remove_dir_all(&base);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test exercises_scaffold`
Expected: FAIL

- [ ] **Step 3: Implement**

`write_project_scaffold` içinde mentor bloğunun altına aynı desenle:

```rust
    // Visible exercise deliverables dir (spec: exercise loop). The watcher is
    // extension-agnostic, so anything saved here is already watched — this dir
    // only gives assignments a conventional, visible home.
    let exercises_dir = cwd.join("exercises");
    let exercises_existed = exercises_dir.is_dir();
    std::fs::create_dir_all(&exercises_dir)
        .with_context(|| format!("could not create directory: {}", exercises_dir.display()))?;
    results.push((exercises_dir.clone(), !exercises_existed));
    let ex_gitkeep = exercises_dir.join(".gitkeep");
    if config::should_write(&ex_gitkeep) {
        std::fs::write(&ex_gitkeep, "")
            .with_context(|| format!("could not write: {}", ex_gitkeep.display()))?;
    }
```

Mevcut scaffold sayaç testi (`results.len()` assert'i) varsa +1 güncelle.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: tümü PASS

- [ ] **Step 5: Commit + push**

```bash
git add src/main.rs
git commit -m "egzersiz: scaffold görünür exercises/ klasörünü kurar"
git push
```

---

### Task 5: TEACHING.md + SPEC.md + README + ROADMAP

**Files:**
- Modify: `TEACHING.md` (sona yeni bölüm), `SPEC.md`, `README.md`, `docs/ROADMAP.md`

- [ ] **Step 1: TEACHING.md — `## Exercise Loop` bölümü ekle**

```markdown
## Exercise Loop

Exercises turn the file-feedback loop into deliberate practice — in ANY domain, not just code.

- **When to assign:** when a map item reaches `seen` and needs consolidation; when the user asks for practice; when the next map step requires doing rather than discussing. One exercise at a time.
- **How to assign (in chat — you never create the file):** one clear deliverable + a suggested path (`exercises/<topic>/<name>.md`) + a one-sentence success criterion ("a good answer includes ..."). The user writes the file; saving it triggers your review automatically.
- **How to review:** compare against the assignment, not against perfection. Hint ladder applies — start high, descend only on stuck. Hard Rule 2 applies to exercises too: never write the solution or a completable skeleton.
- **On completion:** short verdict + what it unlocked; consider promoting the related map item (`seen → settled`). Completed exercises leave `## Açık egzersiz` and land in `Kapatılanlar`.
- **Domains:** code (snippet file), writing (brief/essay), terminal work (user pastes command output into the file) — the file IS the deliverable.
```

- [ ] **Step 2: SPEC.md — egzersiz döngüsü bölümü ekle**

Uygun bölüme (watcher/feedback maddelerinin yanı) kısa madde: `exercises/` konvansiyonu, path-tanıma (`is_exercise_path`), egzersiz çerçevesi, check-atlama, `## Açık egzersiz` kalıcılığı, scaffold.

- [ ] **Step 3: README.md (İngilizce) güncelle**

Highlights tablosuna:

```markdown
| 🏋️ **Exercise loop** | Usta assigns a deliverable, you write it under `exercises/` — saving triggers the same Socratic review loop, in any domain (a GTM brief, a German essay, a Rust snippet). Open exercises survive sessions and get reminded at opening. |
```

Usage bölümüne (session örneğinin altına) tek cümle: `Practice: Usta assigns exercises into `exercises/` — write, save, get reviewed. No solutions handed over.`

- [ ] **Step 4: ROADMAP güncelle**

`docs/ROADMAP.md` #2 satırı: `✅ tamamlandı (2026-08-15)` + Tamamlananlar'a satır.

- [ ] **Step 5: Verify + commit + push**

Run: `cargo test`
Expected: PASS (defaults `returns_all_nonempty_files` TEACHING.md'yi kapsar)

```bash
git add TEACHING.md SPEC.md README.md docs/ROADMAP.md
git commit -m "egzersiz: TEACHING protokolü + SPEC + README + roadmap #2 kapandı"
git push
```

---

### Task 6: Final doğrulama

- [ ] **Step 1:** `cargo test` → tümü PASS, 0 failed
- [ ] **Step 2:** `cargo clippy --all-targets` → baseline'a (5 pre-existing) ek YENİ uyarı yok
- [ ] **Step 3:** `cargo install --path .` → başarılı
- [ ] **Step 4 (elle doğrulama — ATLA, Anil koşacak):** kod-dışı bir konuda Usta'dan egzersiz iste → `exercises/` altına yaz, kaydet → "AS AN EXERCISE" çerçeveli değerlendirme gelmeli, `cargo check` koşmamalı; `/quit` → progress'te `## Açık egzersiz` (tamamlanmadıysa); yeni oturum → açılışta hatırlatma.
