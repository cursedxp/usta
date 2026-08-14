# Mentor Context Layer (`mentor/`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** Çakışan bekleyen plan yok (2026-08-14 itibarıyla tüm önceki planlar implement edilmiş durumda). Spec: `docs/superpowers/specs/2026-08-14-mentor-context-layer-design.md` — önce oku.

**Goal:** Proje root'unda kullanıcıya görünür `mentor/` klasörü: `PROJECT.md` (proje tanımı, tanışmadan Usta yazar) + `PROGRESS.md` (proje durumu + append-only karar günlüğü, kapanış flush'ı günceller); ikisi de system prompt'a yüklenir.

**Architecture:** Mevcut kapanış-flush mekanizması (`===DOSYA:` delimiter + `split_files` + `flush_target`) iki yeni dosya adıyla (`project`, `project-progress`) genişletilir. `brain.rs` system prompt'a mentor dosyalarını ekler. Açılış/onboarding promptları `project_known` bayrağıyla proje-farkında hale gelir. Scaffold `mentor/` klasörünü kurar. `.usta/` yerleşimi değişmez; reset `mentor/`'a dokunmaz.

**Tech Stack:** Rust (mevcut crate, yeni bağımlılık YOK). Test: mevcut in-module `#[cfg(test)]` deseni (temp dir + `std::process::id()`).

## Global Constraints

- Kod yorumları ve prompt metinleri İngilizce (2026-08-12 english-base migration); dosya ŞABLON başlıkları Türkçe: `## Ne` / `## Neden` / `## Ölçek` / `## Stack` / `## Kapsam Dışı` ve `## Bitti` / `## Yapılıyor` / `## Sırada` / `## Kararlar`.
- Mentor dosyaları **proje root'u** altında: `<root>/mentor/PROJECT.md`, `<root>/mentor/PROGRESS.md` — `.usta/` altında DEĞİL.
- `closing_prompt` dosya adı listesi: `progress | approach | curriculum | profile | project | project-progress`.
- Her task sonunda commit; commit mesajları Türkçe kısa özet (repo geleneği: `git log --oneline`).
- Her task sonunda `cargo test` yeşil olmalı — signature değişikliği yapan task, kırdığı tüm call site'ları aynı task içinde günceller.

---

### Task 1: Mentor path helpers (`progress.rs`)

**Files:**
- Modify: `src/progress.rs` (path fonksiyonları bölümü, ~satır 11-28)
- Test: `src/progress.rs` in-module tests

**Interfaces:**
- Produces: `pub fn project_md_path(project_root: &Path) -> PathBuf` → `<root>/mentor/PROJECT.md`; `pub fn project_progress_path(project_root: &Path) -> PathBuf` → `<root>/mentor/PROGRESS.md`. Task 2, 3, 5, 6 bunları kullanır.

- [ ] **Step 1: Write the failing test**

`src/progress.rs` test modülüne (mevcut `paths_build_expected_layout` testinin yanına) ekle:

```rust
#[test]
fn mentor_paths_build_expected_layout() {
    let root = Path::new("/tmp/proj");
    assert_eq!(
        project_md_path(root),
        Path::new("/tmp/proj/mentor/PROJECT.md")
    );
    assert_eq!(
        project_progress_path(root),
        Path::new("/tmp/proj/mentor/PROGRESS.md")
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test mentor_paths_build_expected_layout`
Expected: derleme hatası — "cannot find function `project_md_path`"

- [ ] **Step 3: Write minimal implementation**

`src/progress.rs`, `curriculum_path`'in altına:

```rust
/// User-facing project definition: `<project>/mentor/PROJECT.md`.
/// Lives OUTSIDE `.usta/` on purpose — visible and hand-editable (spec: mentor layer).
pub fn project_md_path(project_root: &Path) -> PathBuf {
    project_root.join("mentor/PROJECT.md")
}

/// User-facing project status + decision log: `<project>/mentor/PROGRESS.md`.
pub fn project_progress_path(project_root: &Path) -> PathBuf {
    project_root.join("mentor/PROGRESS.md")
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test mentor_paths_build_expected_layout`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/progress.rs
git commit -m "mentor: proje dosyası path helper'ları — mentor/PROJECT.md + PROGRESS.md"
```

---

### Task 2: `flush_target` routing (`main.rs`)

**Files:**
- Modify: `src/main.rs:378-386` (`flush_target`)
- Test: `src/main.rs` in-module tests (mevcut `flush_target` testlerinin yanı, ~satır 1315-1327)

**Interfaces:**
- Consumes: Task 1'in `progress::project_md_path` / `progress::project_progress_path`.
- Produces: `flush_target("project", ...)` → `Some(<root>/mentor/PROJECT.md)`; `flush_target("project-progress", ...)` → `Some(<root>/mentor/PROGRESS.md)`. Task 3 bu isimleri yazma döngüsünde kullanır.

- [ ] **Step 1: Write the failing test**

`src/main.rs` test modülüne, mevcut `flush_target` testlerinin yanına:

```rust
#[test]
fn flush_target_routes_mentor_files_to_project_root() {
    let project = Path::new("/tmp/proj");
    let global = Path::new("/tmp/global");
    assert_eq!(
        flush_target("project", project, global, "rust"),
        Some(PathBuf::from("/tmp/proj/mentor/PROJECT.md"))
    );
    assert_eq!(
        flush_target("project-progress", project, global, "rust"),
        Some(PathBuf::from("/tmp/proj/mentor/PROGRESS.md"))
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test flush_target_routes_mentor_files_to_project_root`
Expected: FAIL — `None` döner (assertion failed)

- [ ] **Step 3: Write minimal implementation**

`src/main.rs` `flush_target` match'ine iki kol ekle (`"profile"` kolundan önce):

```rust
        "project" => Some(progress::project_md_path(project_root)),
        "project-progress" => Some(progress::project_progress_path(project_root)),
```

Fonksiyonun doc yorumuna bir cümle ekle: `` `project`/`project-progress` go to the visible `mentor/` dir under the project root (user-facing, spec: mentor layer). ``

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test flush_target`
Expected: PASS (yeni + mevcut testler)

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "mentor: flush_target project/project-progress isimlerini mentor/ altına yönlendirir"
```

---

### Task 3: Closing flush — `closing_prompt` slotları + `flush_progress` kablolama

**Files:**
- Modify: `src/progress.rs:62-119` (`closing_prompt`)
- Modify: `src/main.rs:388-466` (`flush_progress`)
- Test: `src/progress.rs` in-module tests

**Interfaces:**
- Consumes: Task 1 path helper'ları, Task 2 routing'i.
- Produces: yeni imza — `pub fn closing_prompt(topic: &str, progress: Option<&str>, approach: Option<&str>, curriculum: Option<&str>, profile: Option<&str>, project: Option<&str>, project_progress: Option<&str>) -> String`. Bu task TÜM call site'ları günceller (main.rs:419 + progress.rs testlerindeki 7 çağrı) — repo her adım sonunda derlenir.

- [ ] **Step 1: Write the failing tests**

`src/progress.rs` test modülüne:

```rust
#[test]
fn closing_prompt_includes_mentor_file_rules() {
    let s = closing_prompt("rust", None, None, None, None, None, None);
    assert!(s.contains("project-progress"));
    assert!(s.contains("mentor/PROJECT.md"));
    assert!(s.contains("mentor/PROGRESS.md"));
    // append-only decision log rule must be spelled out
    assert!(s.contains("NEVER delete"));
    assert!(s.contains("## Kararlar"));
}

#[test]
fn closing_prompt_embeds_current_mentor_files() {
    let s = closing_prompt(
        "rust",
        None,
        None,
        None,
        None,
        Some("PRJMEVCUT"),
        Some("PPGMEVCUT"),
    );
    assert!(s.contains("PRJMEVCUT"));
    assert!(s.contains("PPGMEVCUT"));
}

#[test]
fn split_files_carries_mentor_names() {
    let reply = "===DOSYA: project===\nP\n===DOSYA: project-progress===\nQ";
    let files = split_files(reply);
    assert_eq!(files[0], ("project".to_string(), "P".to_string()));
    assert_eq!(files[1], ("project-progress".to_string(), "Q".to_string()));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test closing_prompt`
Expected: derleme hatası — mevcut imza 5 parametre alıyor (7 verildi)

- [ ] **Step 3: Implement — `closing_prompt`**

`src/progress.rs` `closing_prompt`'u güncelle:

1. İmzaya iki parametre ekle: `project: Option<&str>, project_progress: Option<&str>` (en sona).
2. Gövde başına iki satır ekle:
```rust
    let prj = project.unwrap_or("(dosya henüz yok)");
    let ppg = project_progress.unwrap_or("(dosya henüz yok)");
```
3. `format!` içinde ad listesi satırını güncelle — eski:
```text
(name: progress | approach | curriculum | profile — e.g. if generating the profile, `===DOSYA: profile===`).
```
yeni:
```text
(name: progress | approach | curriculum | profile | project | project-progress — e.g. if generating the profile, `===DOSYA: profile===`).
```
4. `Current profile:` bloğundan sonra iki blok ekle:
```text
Current project definition (mentor/PROJECT.md):\n---\n{prj}\n---\n\n\
Current project status (mentor/PROGRESS.md):\n---\n{ppg}\n---\n\n\
```
5. `Rules:` listesine, `profile` kuralından sonra iki madde ekle (tam metin):
```text
- `project` is the USER-FACING project definition, written to `mentor/PROJECT.md` \
at the project root. Generate it ONLY when (a) the file doesn't exist yet and a \
concrete project was discussed this session, or (b) the project definition \
materially changed this session. Structure: `# <Project name> — Proje Tanımı` \
heading + `## Ne` (1-2 sentences: what is being built) / `## Neden` \
(goal/motivation, tie to the learning goal) / `## Ölçek` (solo-scale vs \
1000-user scale — architecture advice anchors to this) / `## Stack` (language, \
tools, WHY chosen) / `## Kapsam Dışı` (deliberate non-goals). For non-software \
domains keep the same skeleton but adapt content (e.g. channels/tools instead \
of stack). KEEP the user's hand-edits. If no project was discussed, do NOT \
generate this file.\n\
- `project-progress` is the USER-FACING project status, written to \
`mentor/PROGRESS.md`. Generate it in every session where work happened ON THE \
PROJECT (not for pure concept-learning sessions). Structure: `# <Project name> \
— Durum` heading + `## Bitti` / `## Yapılıyor` / `## Sırada` (rewrite these \
three with the CURRENT state — they are a pointer, not a journal) + \
`## Kararlar` (append-only decision log: `- YYYY-MM-DD | decision | one-line \
why`; append ONLY decisions taken this session; NEVER delete or rewrite \
existing lines). This tracks the PROJECT's state — the learner's knowledge \
belongs in `progress`, not here.\n\
```

- [ ] **Step 4: Update all `closing_prompt` call sites**

- `src/progress.rs` testlerindeki 7 mevcut çağrıya `, None, None` ekle (satır ~225, 232, 238, 244, 289, 299, 307).
- `src/main.rs` `flush_progress` (satır ~410-425) güncelle:

```rust
    let prj_path = flush_target("project", project_root, global_for_paths, &session.topic).unwrap();
    let ppg_path =
        flush_target("project-progress", project_root, global_for_paths, &session.topic).unwrap();
```
(`p_path`/`a_path`/`c_path` satırlarının yanına) ve çağrıyı:
```rust
    history.push(Message::user(progress::closing_prompt(
        &session.topic,
        read(&p_path).as_deref(),
        read(&a_path).as_deref(),
        read(&c_path).as_deref(),
        pr_path.as_deref().and_then(read).as_deref(),
        read(&prj_path).as_deref(),
        read(&ppg_path).as_deref(),
    )));
```
- Yazma döngüsündeki match'e (satır ~432-445, `"profile"` kolundan önce) iki kol:
```rust
            "project" => prj_path.clone(),
            "project-progress" => ppg_path.clone(),
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test`
Expected: tümü PASS (yeni 2 test + güncellenen mevcutlar)

- [ ] **Step 6: Commit**

```bash
git add src/progress.rs src/main.rs
git commit -m "mentor: kapanış flush'ı project + project-progress dosyalarını üretir/yazar"
```

---

### Task 4: System prompt yüklemesi (`brain.rs`)

**Files:**
- Modify: `src/brain.rs:104-118` (`load_system_prompt`)
- Test: `src/brain.rs` in-module tests

**Interfaces:**
- Consumes: yok (dosya yollarını inline kurar — `read_section` mevcut).
- Produces: system prompt'ta `===== mentor/PROJECT.md =====` ve `===== mentor/PROGRESS.md =====` etiketli bölümler (etiket formatı `read_section`'ın mevcut formatı neyse o — koddaki `label` parametresine `"mentor/PROJECT.md"` / `"mentor/PROGRESS.md"` geçilir).

- [ ] **Step 1: Write the failing test**

`src/brain.rs` test modülüne (mevcut `temp_pair` helper'ını kullanarak; `project_progress_included_when_present` testini örnek al):

```rust
#[test]
fn mentor_files_included_when_present_skipped_when_absent() {
    let (global, project) = temp_pair("mentor");
    fs::create_dir_all(global.join("approaches")).unwrap();
    fs::write(global.join("SOUL.md"), "SOUL").unwrap();

    // absent → no mentor section at all
    let sys = load_system_prompt(&global, Some(&project), "rust", "2026-08-14");
    assert!(!sys.contains("mentor/PROJECT.md"));

    // present → both labeled sections appear
    let mentor = project.join("mentor");
    fs::create_dir_all(&mentor).unwrap();
    fs::write(mentor.join("PROJECT.md"), "PRJICERIK").unwrap();
    fs::write(mentor.join("PROGRESS.md"), "PPGICERIK").unwrap();
    let sys = load_system_prompt(&global, Some(&project), "rust", "2026-08-14");
    assert!(sys.contains("mentor/PROJECT.md"));
    assert!(sys.contains("PRJICERIK"));
    assert!(sys.contains("mentor/PROGRESS.md"));
    assert!(sys.contains("PPGICERIK"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test mentor_files_included`
Expected: FAIL — "present" assert'leri düşer

- [ ] **Step 3: Write minimal implementation**

`src/brain.rs` `load_system_prompt` içinde, `read_section(&global.join("USER.md"), ...)` satırından hemen sonra (learner/index.md'den ÖNCE):

```rust
    // User-facing project context: definition + status live in the VISIBLE
    // `mentor/` dir at the project root (not under `.usta/`) so the user can
    // read and hand-edit them (spec: mentor layer). Loaded right after the
    // profile: who first, then which project, then how to teach.
    if let Some(p) = project {
        read_section(&p.join("mentor/PROJECT.md"), "mentor/PROJECT.md", &mut parts);
        read_section(&p.join("mentor/PROGRESS.md"), "mentor/PROGRESS.md", &mut parts);
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib brain`
Expected: PASS (yeni + mevcut brain testleri)

- [ ] **Step 5: Commit**

```bash
git add src/brain.rs
git commit -m "mentor: PROJECT.md + PROGRESS.md system prompt'a yüklenir"
```

---

### Task 5: Açılış/onboarding proje farkındalığı

**Files:**
- Modify: `src/progress.rs:124-175` (`opening_prompt`, `onboarding_prompt`)
- Modify: `src/main.rs:197-214` (plain path açılışı)
- Modify: `src/tui/run.rs:592-600` (TUI açılışı)
- Test: `src/progress.rs` in-module tests

**Interfaces:**
- Consumes: Task 1 `progress::project_md_path`.
- Produces: yeni imzalar — `opening_prompt(topic: &str, profile_generic: bool, project_known: bool)`, `onboarding_prompt(topic: &str, intro: Option<&str>, profile_generic: bool, project_known: bool)`. Bu task iki call site'ı da günceller.

- [ ] **Step 1: Write the failing tests**

`src/progress.rs` test modülüne:

```rust
#[test]
fn opening_prompt_mentions_project_pointer_when_known() {
    let s = opening_prompt("rust", false, true);
    assert!(s.contains("mentor/PROGRESS.md"));
    assert!(s.contains("Sırada"));
    let s = opening_prompt("rust", false, false);
    assert!(!s.contains("mentor/PROGRESS.md"));
}

#[test]
fn onboarding_prompt_asks_project_basics_only_when_unknown() {
    let s = onboarding_prompt("rust", None, false, false);
    assert!(s.contains("mentor/PROJECT.md"));
    assert!(s.contains("what they're building"));
    let s = onboarding_prompt("rust", None, false, true);
    assert!(!s.contains("what they're building"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib prompt`
Expected: derleme hatası (parametre sayısı)

- [ ] **Step 3: Implement prompt changes**

`src/progress.rs`:

`opening_prompt` — imzaya `project_known: bool` ekle; gövdeye blok:
```rust
    let project_block = if project_known {
        "\nThe project files mentor/PROJECT.md and mentor/PROGRESS.md are in your \
         system prompt — do NOT re-ask project basics. After the drill, add one \
         sentence on where the PROJECT left off, taken from the `## Sırada` section \
         of mentor/PROGRESS.md.\n"
    } else {
        ""
    };
```
ve `format!` string'inin sonuna `{project_block}` ekle.

`onboarding_prompt` — imzaya `project_known: bool` ekle; gövdeye blok:
```rust
    let project_block = if project_known {
        "\nThe project files mentor/PROJECT.md and mentor/PROGRESS.md are in your \
         system prompt — do NOT re-ask project basics; connect this new topic to \
         the existing project context.\n"
    } else {
        "\nThere is no mentor/PROJECT.md for this project yet. During the \
         introduction also find out, naturally (not as a form): what they're \
         building, why, rough scale, stack/tools and why. At session close you'll \
         be asked for a `project` file — the shell writes it; don't write files \
         yourself during the session.\n"
    };
```
ve `format!` string'inin sonuna `{project_block}` ekle.

- [ ] **Step 4: Update both call sites**

Her ikisinde de çağrıdan önce:
```rust
    let project_known = progress::project_md_path(project_root).exists();
```
- `src/main.rs` (~197-214, `run_plain_loop` içi — imzasında `project_root: &Path` zaten var): `opening_prompt(topic, profile_generic, project_known)` / `onboarding_prompt(topic, intro, profile_generic, project_known)`.
- `src/tui/run.rs` (~592-600): aynı şekilde (`project_root` scope'ta var, satır 537 civarı kullanılıyor).

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test`
Expected: tümü PASS

- [ ] **Step 6: Commit**

```bash
git add src/progress.rs src/main.rs src/tui/run.rs
git commit -m "mentor: açılış/onboarding proje-farkında — PROJECT.md varsa sormaz, yoksa tanışmada sorar"
```

---

### Task 6: Scaffold `mentor/` kurar + reset dokunmaz

**Files:**
- Modify: `src/main.rs:1135-1165` (`write_project_scaffold`)
- Test: `src/main.rs` in-module tests

**Interfaces:**
- Consumes: yok.
- Produces: `write_project_scaffold` proje root'unda `mentor/` klasörü + `.gitkeep` oluşturur (results vektörüne eklenir — `usta init` çıktısında görünür).

- [ ] **Step 1: Write the failing tests**

`src/main.rs` test modülüne (mevcut `write_project_scaffold_creates_dirs_and_gitkeeps` desenini örnek al):

```rust
#[test]
fn write_project_scaffold_creates_visible_mentor_dir() {
    let base = std::env::temp_dir().join(format!(
        "usta_main_test_mentor_scaffold_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();

    write_project_scaffold(&base).unwrap();
    assert!(base.join("mentor").is_dir());
    assert!(base.join("mentor/.gitkeep").is_file());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn reset_topic_leaves_mentor_dir_untouched() {
    // reset deletes under `.usta/` only — mentor/ is the user's project doc,
    // possibly committed to their repo. Guard that contract with the same
    // path logic run_reset_topic uses (progress_path is under .usta).
    let root = Path::new("/tmp/proj");
    let p = progress::progress_path(root, "rust");
    assert!(p.starts_with(root.join(".usta")));
    assert!(!progress::project_md_path(root).starts_with(root.join(".usta")));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test mentor`
Expected: `write_project_scaffold_creates_visible_mentor_dir` FAIL (`mentor` yok); ikinci test PASS olabilir — o bir kontrat testi, kalsın.

- [ ] **Step 3: Write minimal implementation**

`src/main.rs` `write_project_scaffold` içinde, `visuals_dir` bloğundan önce:

```rust
    // Visible, user-facing project docs (mentor/PROJECT.md + PROGRESS.md are
    // written by the closing flush; the dir is scaffolded so it's visible from
    // day one). Deliberately OUTSIDE `.usta/` — reset must never touch it.
    let mentor_dir = cwd.join("mentor");
    let mentor_existed = mentor_dir.is_dir();
    std::fs::create_dir_all(&mentor_dir)
        .with_context(|| format!("could not create directory: {}", mentor_dir.display()))?;
    results.push((mentor_dir.clone(), !mentor_existed));
    let mentor_gitkeep = mentor_dir.join(".gitkeep");
    if config::should_write(&mentor_gitkeep) {
        std::fs::write(&mentor_gitkeep, "")
            .with_context(|| format!("could not write: {}", mentor_gitkeep.display()))?;
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: tümü PASS

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "mentor: scaffold görünür mentor/ klasörünü kurar; reset kontratı testle sabit"
```

---

### Task 7: Brain kuralları + dokümantasyon

**Files:**
- Modify: `approaches/software.md` (Mini-spec bölümü, satır 5-12)
- Modify: `SPEC.md` (madde 55 + dosya yerleşimi bölümü ~satır 150-157)
- Modify: `README.md` (Öne çıkanlar tablosu)

**Interfaces:**
- Consumes: kavramsal — Task 1-6'nın davranışını belgeler. Kod değişikliği yok.
- Not: `approaches/software.md` `defaults.rs` ile embed ediliyor (`include_str!`) — md değişince binary'ye otomatik girer, `write_global_defaults` sync'i Code-owned dosyayı günceller. Ek iş yok.

- [ ] **Step 1: `approaches/software.md` Mini-spec kuralını güncelle**

Mini-spec bölümüne (mevcut "The user writes it..." cümlesinden önce) şu satırı ekle:

```markdown
Before asking for a spec, check `mentor/PROJECT.md` in your system prompt — if the
project definition is already there, don't re-ask it piece by piece; only ask for
the mini-spec of the CURRENT piece.
```

- [ ] **Step 2: `SPEC.md` güncelle**

- Madde 55'teki cümleyi güncelle — eski: "**Ne inşa ettiğin** (proje/hedef) klasör + sohbette söylenir — Usta parça-başı 'spek'in ne?' diye sorar." yeni: "**Ne inşa ettiğin** `mentor/PROJECT.md`'de yaşar (Usta tanışmadan yazar, kullanıcı elle düzenleyebilir); Usta önce oraya bakar, orada yoksa sorar. Projenin durumu `mentor/PROGRESS.md`'de (Bitti/Yapılıyor/Sırada + append-only Kararlar)."
- Dosya yerleşimi bölümüne (`projects/` civarı, ~satır 157) `mentor/PROJECT.md` + `mentor/PROGRESS.md` girdilerini ekle: proje root'unda, görünür, kapanış flush'ı yazar, reset dokunmaz.

- [ ] **Step 3: `README.md` güncelle**

Öne çıkanlar tablosuna satır ekle:

```markdown
| 📋 **Proje bağlamı** | Proje tanımı + durumu görünür `mentor/` klasöründe yaşar (`PROJECT.md` + `PROGRESS.md`). Usta tanışmadan doldurur, her oturum günceller — "spek'in ne?" diye baştan sormaz. |
```

- [ ] **Step 4: Verify**

Run: `cargo test`
Expected: PASS (defaults.rs `returns_all_nonempty_files` testi md'lerin boş olmadığını doğrular)

- [ ] **Step 5: Commit**

```bash
git add approaches/software.md SPEC.md README.md
git commit -m "mentor: brain kuralı + SPEC madde 55 + README — proje bağlamı mentor/'dan okunur"
```

---

### Task 8: Final doğrulama

**Files:** yok (salt doğrulama)

- [ ] **Step 1: Tüm testler**

Run: `cargo test`
Expected: tümü PASS, 0 failed

- [ ] **Step 2: Lint + format**

Run: `cargo fmt --check && cargo clippy -- -D warnings`
Expected: temiz (fmt farkı varsa `cargo fmt` + amend)

- [ ] **Step 3: Elle doğrulama (Anil ile)**

```bash
cargo run -- start deneme   # boş bir temp klasörde
```
Beklenen: `mentor/` klasörü oluşur; tanışmada proje soruları gelir; oturum kapatınca `mentor/PROJECT.md` + `mentor/PROGRESS.md` yazılır; ikinci açılışta Usta proje temellerini yeniden sormaz, "projede kaldığımız yer" işaretçisi verir.

- [ ] **Step 4: Kurulum**

```bash
cargo install --path .
```
