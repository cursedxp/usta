# Materyal Yutma Implementation Plan (Roadmap #5)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** Çakışan bekleyen plan yok. Spec: `docs/superpowers/specs/2026-08-15-material-ingest-design.md` — önce oku.

**Goal:** `materials/` klasöründeki md/txt (+ pdftotext'le çevrilen PDF) materyaller tanışma turuna digest olarak enjekte edilir; müfredat bölüm referanslarıyla materyale demirlenir; scaffold klasörü kurar; v0.14.0.

**Architecture:** Yeni `src/materials.rs` modülü: saf digest fonksiyonları + fs tarama + opsiyonel pdftotext dönüşümü. `onboarding_prompt`'a `materials: Option<&str>` parametresi; iki çağrı yeri (TUI + plain) yeni-konu yolunda scan/convert koşar. Kalıcılık müfredat haritasındaki `— kaynak:` referanslarıyla (closing kural eki) — digest persist edilmez.

**Tech Stack:** Rust (mevcut crate, YENİ BAĞIMLILIK YOK — pdftotext harici opsiyonel araç, `std::process::Command`).

## Global Constraints

- Prompt metinleri İngilizce; `— kaynak:` referans etiketi Türkçe (dosya-içi konvansiyon).
- Sınırlar: dosya başına 8_000, toplam 16_000 karakter; kırpma UTF-8 güvenli (`char` sınırında), sessiz değil (`[truncated]`).
- Mevcut konu (resume) yolunda materyal enjeksiyonu YOK — yalnız yeni-konu tanışması.
- Her task sonunda commit (Türkçe mesaj) + push + `cargo test` yeşil; imza kıran task tüm call site'ları aynı task'ta günceller.

---

### Task 1: Digest fonksiyonları (`src/materials.rs` — yeni modül)

**Files:**
- Create: `src/materials.rs`
- Modify: `src/main.rs` (`mod materials;` satırı — mevcut mod bloğuna)
- Test: `src/materials.rs` in-module tests

**Interfaces:**
- Produces: `pub fn digest_md(content: &str, cap: usize) -> String`; `pub fn digest_txt(content: &str, cap: usize) -> String`; `pub const PER_FILE_CAP: usize = 8_000;`; `pub const TOTAL_CAP: usize = 16_000;`. Task 2-3 kullanır.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn digest_md_lists_headings_with_excerpts() {
    let md = "# Kitap\ngiriş metni burada uzar gider\n## Bölüm 1: Sahiplik\nownership anlatımı çok uzun bir paragraf halinde devam eder\n## Bölüm 2: Borrowing\nborrow açıklaması\n";
    let d = digest_md(md, 8_000);
    assert!(d.contains("# Kitap"));
    assert!(d.contains("## Bölüm 1: Sahiplik"));
    assert!(d.contains("ownership anlatımı"));
    assert!(d.contains("## Bölüm 2: Borrowing"));
    // excerpt tek satıra iner (içindeki \n yok)
    assert!(!d.contains("paragraf halinde\ndevam"));
}

#[test]
fn digest_md_caps_with_marker_on_char_boundary() {
    let md = format!("# T\n{}", "çğüşöı ".repeat(3000)); // Türkçe çok-baytlı içerik
    let d = digest_md(&md, 500);
    assert!(d.chars().count() <= 500 + "\n[truncated]".chars().count());
    assert!(d.ends_with("[truncated]"));
}

#[test]
fn digest_txt_head_plus_stats() {
    let txt = format!("{}\n", "satır içeriği\n".repeat(500));
    let d = digest_txt(&txt, 8_000);
    assert!(d.starts_with("satır içeriği"));
    assert!(d.contains("lines"));
    assert!(d.contains("KB"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib materials`
Expected: derleme hatası — modül yok

- [ ] **Step 3: Write minimal implementation**

`src/materials.rs`:

```rust
//! Course-material ingestion (spec: material ingest). The shell produces
//! deterministic digests — heading skeleton + short excerpts — that get
//! injected into the NEW-TOPIC introduction turn. Usta anchors the curriculum
//! to the material; the USER does the reading. No LLM here, no persistence.

pub const PER_FILE_CAP: usize = 8_000;
pub const TOTAL_CAP: usize = 16_000;

/// Truncate on a char boundary and append a visible marker — silent clipping
/// would read as "that's the whole material".
fn cap_str(s: &str, cap: usize) -> String {
    if s.chars().count() <= cap {
        return s.to_string();
    }
    let cut: String = s.chars().take(cap).collect();
    format!("{cut}\n[truncated]")
}

/// One-line excerpt: first `n` chars of the section body, newlines flattened.
fn excerpt(body: &str, n: usize) -> String {
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars().take(n).collect()
}

/// Markdown digest: every heading line kept as-is, followed by a ~200-char
/// excerpt of the text under it.
pub fn digest_md(content: &str, cap: usize) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut body = String::new();
    let mut flush = |out: &mut Vec<String>, body: &mut String| {
        if !body.trim().is_empty() {
            out.push(format!("  {}", excerpt(body, 200)));
        }
        body.clear();
    };
    for line in content.lines() {
        if line.trim_start().starts_with('#') {
            flush(&mut out, &mut body);
            out.push(line.trim_end().to_string());
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    flush(&mut out, &mut body);
    cap_str(&out.join("\n"), cap)
}

/// Plain-text digest: head excerpt + size stats (no structure to mine).
pub fn digest_txt(content: &str, cap: usize) -> String {
    let head: String = content.chars().take(1_000).collect();
    let lines = content.lines().count();
    let kb = content.len() / 1024;
    cap_str(&format!("{head}\n[... {lines} lines, {kb} KB total]"), cap)
}
```

`src/main.rs`: mod bloğuna `mod materials;` ekle.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib materials`
Expected: PASS

- [ ] **Step 5: Commit + push**

```bash
git add src/materials.rs src/main.rs
git commit -m "materyal: digest_md/digest_txt — başlık iskeleti + alıntı, UTF-8 güvenli cap"
git push
```

---

### Task 2: Tarama + PDF dönüşümü (`src/materials.rs`)

**Files:**
- Modify: `src/materials.rs`
- Test: `src/materials.rs` in-module tests (tmpdir — mevcut `std::env::temp_dir()` + `process::id()` deseni)

**Interfaces:**
- Consumes: Task 1 digest'leri + cap sabitleri.
- Produces: `pub struct Material { pub name: String, pub digest: String }`; `pub fn scan(project_root: &Path) -> Vec<Material>`; `pub fn convert_pdfs(dir: &Path) -> Vec<String>`; `pub fn combined_digests(mats: &[Material]) -> Option<String>`. Task 3 kullanır.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn scan_finds_md_txt_skips_hidden_sorts() {
    let base = std::env::temp_dir().join(format!("usta_materials_scan_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let dir = base.join("materials");
    std::fs::create_dir_all(dir.join("alt")).unwrap();
    std::fs::write(dir.join("b-kitap.md"), "# K\nicerik").unwrap();
    std::fs::write(dir.join("a-notlar.txt"), "notlar").unwrap();
    std::fs::write(dir.join(".gitkeep"), "").unwrap();
    std::fs::write(dir.join("alt/ek.md"), "# Ek\nx").unwrap();
    std::fs::write(dir.join("resim.png"), "x").unwrap();

    let mats = scan(&base);
    let names: Vec<&str> = mats.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, vec!["a-notlar.txt", "alt/ek.md", "b-kitap.md"]);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn scan_without_materials_dir_is_empty() {
    let base = std::env::temp_dir().join(format!("usta_materials_none_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    assert!(scan(&base).is_empty());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn scan_skips_pdf_when_sibling_txt_exists() {
    let base = std::env::temp_dir().join(format!("usta_materials_pdf_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let dir = base.join("materials");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("kitap.pdf"), "%PDF").unwrap();
    std::fs::write(dir.join("kitap.txt"), "cevrilmis metin").unwrap();
    let mats = scan(&base);
    assert_eq!(mats.len(), 1);
    assert_eq!(mats[0].name, "kitap.txt");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn combined_digests_caps_total_and_labels_files() {
    let mats = vec![
        Material { name: "a.md".into(), digest: "x".repeat(10_000) },
        Material { name: "b.md".into(), digest: "y".repeat(10_000) },
    ];
    let c = combined_digests(&mats).unwrap();
    assert!(c.contains("=== a.md ==="));
    assert!(c.chars().count() <= TOTAL_CAP + 50); // marker payı
    assert!(combined_digests(&[]).is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib materials`
Expected: derleme hatası

- [ ] **Step 3: Write minimal implementation**

```rust
pub struct Material {
    pub name: String,   // materials/ altına göreli yol
    pub digest: String, // PER_FILE_CAP'li digest
}

/// Recursively collect .md/.txt under `materials/`, digest each. A .pdf with a
/// sibling .txt is represented by the .txt alone (no double counting).
/// Deterministic: sorted by relative name.
pub fn scan(project_root: &Path) -> Vec<Material> {
    let root = project_root.join("materials");
    let mut files: Vec<PathBuf> = Vec::new();
    collect_files(&root, &mut files);
    files.sort();
    files
        .iter()
        .filter_map(|p| {
            let name = p.strip_prefix(&root).ok()?.to_string_lossy().to_string();
            let ext = p.extension()?.to_str()?;
            let content = std::fs::read_to_string(p).ok()?;
            let digest = match ext {
                "md" => digest_md(&content, PER_FILE_CAP),
                "txt" => digest_txt(&content, PER_FILE_CAP),
                _ => return None,
            };
            Some(Material { name, digest })
        })
        .collect()
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if p.is_dir() {
            collect_files(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// Join per-file digests under `=== name ===` banners, capped at TOTAL_CAP.
pub fn combined_digests(mats: &[Material]) -> Option<String> {
    if mats.is_empty() {
        return None;
    }
    let joined = mats
        .iter()
        .map(|m| format!("=== {} ===\n{}", m.name, m.digest))
        .collect::<Vec<_>>()
        .join("\n\n");
    Some(cap_str(&joined, TOTAL_CAP))
}

/// Convert each materials/*.pdf to a sibling .txt via pdftotext when available.
/// Returns user-facing notice lines; never fails the session.
pub fn convert_pdfs(project_root: &Path) -> Vec<String> {
    let root = project_root.join("materials");
    let mut files: Vec<PathBuf> = Vec::new();
    collect_files(&root, &mut files);
    let pdfs: Vec<&PathBuf> = files.iter().filter(|p| p.extension().is_some_and(|e| e == "pdf")).collect();
    if pdfs.is_empty() {
        return Vec::new();
    }
    let have_tool = std::process::Command::new("pdftotext")
        .arg("-v")
        .output()
        .is_ok();
    let mut notes = Vec::new();
    for pdf in pdfs {
        let txt = pdf.with_extension("txt");
        let fresh = match (std::fs::metadata(&txt), std::fs::metadata(pdf)) {
            (Ok(t), Ok(p)) => t.modified().ok() >= p.modified().ok(),
            _ => false,
        };
        if fresh {
            continue; // cached conversion is current
        }
        if !have_tool {
            notes.push(format!(
                "PDF found but pdftotext missing — convert {} to text yourself, or `brew install poppler`",
                pdf.display()
            ));
            continue;
        }
        let ok = std::process::Command::new("pdftotext")
            .arg("-layout")
            .arg(pdf)
            .arg(&txt)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        notes.push(if ok {
            format!("converted: {} → {}", pdf.display(), txt.display())
        } else {
            format!("pdftotext failed on {} — convert it to text yourself", pdf.display())
        });
    }
    notes
}
```

(`scan_skips_pdf_when_sibling_txt_exists` zaten geçer: `.pdf` uzantısı digest match'inde `None`.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib materials`
Expected: PASS

- [ ] **Step 5: Commit + push**

```bash
git add src/materials.rs
git commit -m "materyal: scan + combined_digests + opsiyonel pdftotext dönüşümü"
git push
```

---

### Task 3: Tanışma entegrasyonu (`onboarding_prompt` + iki çağrı yeri)

**Files:**
- Modify: `src/progress.rs` (`onboarding_prompt` — yeni parametre `materials: Option<&str>`)
- Modify: `src/tui/run.rs` (yeni-konu onboarding çağrısı) + `src/main.rs` (`run_plain_loop` onboarding çağrısı)
- Test: `src/progress.rs` in-module tests

**Interfaces:**
- Consumes: Task 2 `materials::{scan, combined_digests, convert_pdfs}`.
- Produces: `onboarding_prompt(topic, intro, profile_generic, project_known, materials: Option<&str>)`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn onboarding_prompt_injects_material_block() {
    let s = onboarding_prompt("rust", None, false, false, Some("=== kitap.md ===\n# K"));
    assert!(s.contains("COURSE MATERIAL FOUND"));
    assert!(s.contains("=== kitap.md ==="));
    assert!(s.contains("ASK whether to anchor"));
    assert!(s.contains("kaynak:"));
    let s = onboarding_prompt("rust", None, false, false, None);
    assert!(!s.contains("COURSE MATERIAL FOUND"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test onboarding_prompt`
Expected: derleme hatası (parametre sayısı)

- [ ] **Step 3: Implement**

`onboarding_prompt` imzasına `materials: Option<&str>` ekle; gövdeye blok:

```rust
    let material_block = match materials {
        Some(d) => format!(
            "\n[COURSE MATERIAL FOUND]\nThe user has material under materials/ — \
             outline digests below. ASK whether to anchor this topic's curriculum \
             to this material (it may belong to another topic). If yes: build the \
             curriculum map FROM its chapters/sections — each map item carries a \
             source ref (`— kaynak: <file> §<section>`); assign reading from it \
             (the USER reads — you don't summarize the material into the chat); \
             still add critical items the material lacks, from web research \
             (scope guarding). If no: proceed normally.\n---\n{d}\n---\n"
        ),
        None => String::new(),
    };
```

ve `format!` sonuna `{material_block}` ekle.

Çağrı yerleri — yeni-konu yolunda (her ikisinde de `project_root` scope'ta):

```rust
        for note in crate::materials::convert_pdfs(project_root) {
            // TUI: page_notice(&mut tui, &note)?;  ·  plain: ui::notice(&note);
        }
        let mats = crate::materials::scan(project_root);
        let material_digest = crate::materials::combined_digests(&mats);
        // onboarding_prompt(..., material_digest.as_deref())
```

- TUI (`src/tui/run.rs`): `progress::onboarding_prompt(...)` çağrılan dala (opening/onboarding seçimi) — yalnız onboarding dalında scan koş (opening dalında koşma, gereksiz IO).
- Plain (`src/main.rs` `run_plain_loop`): aynı desen, `ui::notice` ile.
- `src/progress.rs` içindeki mevcut `onboarding_prompt` test çağrılarına `None` ekle.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: tümü PASS

- [ ] **Step 5: Commit + push**

```bash
git add src/progress.rs src/tui/run.rs src/main.rs
git commit -m "materyal: tanışma turuna digest enjeksiyonu — müfredat materyale demirlenir"
git push
```

---

### Task 4: Kapanış kural eki + scaffold

**Files:**
- Modify: `src/progress.rs` (`closing_prompt` curriculum kuralı), `src/main.rs` (`write_project_scaffold`)
- Test: her ikisinin in-module testleri

- [ ] **Step 1: Write the failing tests**

```rust
// progress.rs:
#[test]
fn closing_prompt_preserves_material_source_refs() {
    let s = closing_prompt("rust", None, None, None, None, None, None);
    assert!(s.contains("kaynak:"));
    assert!(s.contains("— kaynak: web"));
}

// main.rs:
#[test]
fn write_project_scaffold_creates_visible_materials_dir() {
    let base = std::env::temp_dir().join(format!("usta_main_test_materials_scaffold_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    write_project_scaffold(&base).unwrap();
    assert!(base.join("materials").is_dir());
    assert!(base.join("materials/.gitkeep").is_file());
    let _ = std::fs::remove_dir_all(&base);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test material`
Expected: FAIL

- [ ] **Step 3: Implement**

- `closing_prompt` curriculum kural cümlesine ek: `If the map was anchored to course material, KEEP the source refs (`— kaynak: <file> §<section>`) on every item; items added from web research are marked `— kaynak: web`.`
- `write_project_scaffold`: mentor/exercises bloklarıyla aynı desende `materials/` + `.gitkeep`; sayaç assert'i +1 (5→6).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: tümü PASS

- [ ] **Step 5: Commit + push**

```bash
git add src/progress.rs src/main.rs
git commit -m "materyal: kaynak referansları kapanışta korunur + scaffold materials/ kurar"
git push
```

---

### Task 5: TEACHING.md + docs + v0.14.0

**Files:**
- Modify: `TEACHING.md`, `SPEC.md`, `README.md`, `docs/ROADMAP.md`, `Cargo.toml` (+`Cargo.lock`), sürüm testi

- [ ] **Step 1: TEACHING.md — `## Course Material` bölümü**

```markdown
## Course Material

When the curriculum is anchored to material under `materials/`:

- The material is the spine, web research the supplement. Map items carry their source ref (`— kaynak: <file> §<section>`).
- The USER reads. Assign a section, then test it with recall questions and anchor exercises to it (exercise loop: "read §3, then write `exercises/<topic>/ch3-notes.md`").
- Never summarize the material into the chat — your job is anchoring, questioning, and gap-filling, not replacing the book.
- Scope guarding still applies: critical items the material lacks enter the map from web research, marked `— kaynak: web`.
```

- [ ] **Step 2: SPEC.md** — yeni § (v0.14): materials/ konvansiyonu, digest enjeksiyonu (yalnız yeni-konu tanışması), pdftotext opsiyonelliği, kaynak-ref kalıcılığı, cap'ler.

- [ ] **Step 3: README.md (İngilizce) Highlights satırı:**

```markdown
| 📚 **Bring your own material** | Drop your book/course notes (md/txt — PDFs auto-convert if `pdftotext` is installed) into `materials/` — Usta anchors the curriculum to its chapters, assigns reading, and quizzes you on it. You read; it never summarizes the book at you. |
```

- [ ] **Step 4: ROADMAP** — #5 `✅ tamamlandı (2026-08-15)` + Tamamlananlar satırı.

- [ ] **Step 5: Sürüm** — Cargo.toml `0.14.0`; `version_aligned_with_spec` testi `"0.14.0"` olarak güncellenir; `cargo build` (lock).

- [ ] **Step 6: Verify + commit + push + tag**

Run: `cargo test` → PASS · `cargo clippy --all-targets` → yeni uyarı 0 · `cargo install --path .` → başarılı

```bash
git add TEACHING.md SPEC.md README.md docs/ROADMAP.md Cargo.toml Cargo.lock src/
git commit -m "materyal: TEACHING + SPEC + README + roadmap #5 kapandı — v0.14.0"
git push
git tag v0.14.0 && git push --tags
```

- [ ] **Step 7 (elle doğrulama — ATLA, Anil koşacak):** `materials/` içine bir md kitap koy → yeni konu aç → Usta "demirleyeyim mi?" diye sormalı → evet → müfredat bölüm referanslı kurulmalı; PDF koy (pdftotext'siz) → "convert to text / brew install poppler" mesajı gelmeli.
