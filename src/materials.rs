//! Course-material ingestion (spec: material ingest). The shell produces
//! deterministic digests — heading skeleton + short excerpts — that get
//! injected into the NEW-TOPIC introduction turn. Usta anchors the curriculum
//! to the material; the USER does the reading. No LLM here, no persistence.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

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

/// One-line excerpt: the section body with internal newlines flattened to
/// spaces, capped at `n` chars. The per-section cap keeps a single oversized
/// section from eating the whole digest budget, so later headings in the
/// skeleton survive even when an early section is huge (spec: "heading
/// skeleton, every heading listed, ~200 chars per section").
fn excerpt(body: &str, n: usize) -> String {
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars().take(n).collect()
}

/// Markdown digest: every heading line kept as-is, followed by a flattened
/// excerpt of the text under it (overall digest is capped, see `excerpt`).
pub fn digest_md(content: &str, cap: usize) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut body = String::new();
    let flush = |out: &mut Vec<String>, body: &mut String| {
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

pub struct Material {
    pub name: String,   // materials/ altına göreli yol
    pub digest: String, // PER_FILE_CAP'li digest
}

/// Recursively collect .md/.txt under `materials/`, digest each, plus the PDF
/// conversions in `materials/.converted/` (named after their PDF). A .pdf with a
/// sibling .txt is represented by the .txt alone (no double counting); a blank
/// conversion (scanned PDF) adds nothing. Deterministic: sorted by name.
pub fn scan(project_root: &Path) -> Vec<Material> {
    let root = project_root.join("materials");
    let mut files: Vec<PathBuf> = Vec::new();
    collect_files(&root, &mut files);
    let mut mats: Vec<Material> = files
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
        .collect();
    let conv = root.join(".converted");
    let mut converted: Vec<PathBuf> = Vec::new();
    collect_files(&conv, &mut converted);
    for p in converted {
        let Ok(rel) = p.strip_prefix(&conv) else {
            continue;
        };
        if root.join(rel).exists() {
            continue; // a sibling .txt now speaks for this PDF
        }
        if !source_pdf_exists(&root, rel) {
            continue; // the PDF was deleted: its cached text is stale
        }
        let Ok(content) = std::fs::read_to_string(&p) else {
            continue;
        };
        if content.trim().is_empty() {
            continue;
        }
        mats.push(Material {
            name: rel.with_extension("pdf").to_string_lossy().to_string(),
            digest: digest_txt(&content, PER_FILE_CAP),
        });
    }
    mats.sort_by(|a, b| a.name.cmp(&b.name));
    mats
}

/// Is there a PDF (any extension case) in `materials/` whose path matches `rel` (a
/// `.converted` entry) apart from the extension?
fn source_pdf_exists(materials: &Path, rel: &Path) -> bool {
    let dir = rel
        .parent()
        .map_or_else(|| materials.to_path_buf(), |d| materials.join(d));
    let Some(stem) = rel.file_stem() else {
        return false;
    };
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| is_pdf(&e.path()) && e.path().file_stem() == Some(stem))
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
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

/// Lightweight existence check: is there at least one md/txt/pdf file under
/// `materials/`? No digesting — just a presence probe, used to gate whether
/// the Course Material teaching rules are worth loading into the prompt at
/// all (an empty/absent `materials/` dir means those rules are dead weight).
pub fn materials_present(project_root: &Path) -> bool {
    let root = project_root.join("materials");
    let mut files: Vec<PathBuf> = Vec::new();
    collect_files(&root, &mut files);
    files.iter().any(|p| {
        matches!(
            p.extension().and_then(|e| e.to_str()),
            Some("md" | "txt" | "pdf")
        )
    })
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

/// Per-PDF time cap: a huge or hung PDF can't freeze the session.
const PDF_TIMEOUT: Duration = Duration::from_secs(30);

/// Convert each materials/**/*.pdf (any case) to text in `materials/.converted/`
/// via pdftotext when available. Returns user-facing notice lines; never fails
/// the session.
pub fn convert_pdfs(project_root: &Path) -> Vec<String> {
    convert_pdfs_with(project_root, "pdftotext", PDF_TIMEOUT)
}

/// `convert_pdfs` with the tool and time cap injectable (tests: a missing or fake tool).
fn convert_pdfs_with(project_root: &Path, tool: &str, timeout: Duration) -> Vec<String> {
    let root = project_root.join("materials");
    let mut files: Vec<PathBuf> = Vec::new();
    collect_files(&root, &mut files);
    let pdfs: Vec<&PathBuf> = files.iter().filter(|p| is_pdf(p)).collect();
    if pdfs.is_empty() {
        return Vec::new();
    }
    let have_tool = std::process::Command::new(tool).arg("-v").output().is_ok();
    let mut notes = Vec::new();
    for pdf in pdfs {
        if pdf.with_extension("txt").exists() {
            continue; // a hand-written (or older usta's) text wins — never overwritten
        }
        let Some(txt) = converted_path(&root, pdf) else {
            continue;
        };
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
        notes.push(convert_one(tool, pdf, &txt, timeout));
    }
    notes
}

/// Run the tool on one PDF under a time cap. On failure or timeout no partial
/// output is left (it would read as a fresh conversion next time).
fn convert_one(tool: &str, pdf: &Path, txt: &Path, timeout: Duration) -> String {
    if let Some(dir) = txt.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let spawned = std::process::Command::new(tool)
        .arg("-layout")
        .arg(pdf)
        .arg(txt)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => return failed_note(pdf, &format!(" ({e})")),
    };
    // Drain stderr concurrently: a chatty child would otherwise fill the pipe and block.
    let drain = child.stderr.take().map(|mut s| {
        std::thread::spawn(move || {
            let mut err = String::new();
            let _ = s.read_to_string(&mut err);
            err
        })
    });
    let collect = move || drain.and_then(|h| h.join().ok()).unwrap_or_default();
    let start = Instant::now();
    let exited = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(50)),
            _ => break None,
        }
    };
    let Some(exit) = exited else {
        let _ = child.kill();
        let _ = child.wait();
        let _ = collect();
        let _ = std::fs::remove_file(txt);
        return format!("pdftotext timed out on {}", pdf.display());
    };
    if !exit.success() {
        let err = collect();
        let _ = std::fs::remove_file(txt);
        let why = err
            .lines()
            .next()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| format!(" ({l})"))
            .unwrap_or_default();
        return failed_note(pdf, &why);
    }
    match std::fs::read_to_string(txt) {
        Ok(t) if t.trim().is_empty() => {
            format!("no text found in {} (scanned PDF?)", pdf.display())
        }
        _ => format!("converted: {} → {}", pdf.display(), txt.display()),
    }
}

fn failed_note(pdf: &Path, why: &str) -> String {
    format!(
        "pdftotext failed on {}{why} — convert it to text yourself",
        pdf.display()
    )
}

/// `materials/<rel>.pdf` → `materials/.converted/<rel>.txt` (hidden: the watcher
/// and `collect_files` skip it, so usta's own output is never fed back).
fn converted_path(materials: &Path, pdf: &Path) -> Option<PathBuf> {
    let rel = pdf.strip_prefix(materials).ok()?;
    Some(materials.join(".converted").join(rel).with_extension("txt"))
}

fn is_pdf(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_pdfs_captures_pdftotext_output() {
        let src = include_str!("materials.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap();
        let body = prod
            .split("pub fn convert_pdfs(")
            .nth(1)
            .expect("convert_pdfs");
        assert!(
            !body.contains(".status()"),
            "pdftotext must not inherit the TUI's stdout/stderr"
        );
        assert!(body.contains("Stdio::null()") && body.contains("Stdio::piped()"));
    }

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
        // Many headings so the joined skeleton exceeds the overall cap and cap_str bites.
        let mut md = String::from("# Kitap çğüşöı\n");
        for i in 0..60 {
            md.push_str(&format!(
                "## Bölüm {i} çğüşöı\nçğüşöı içerik satırı burada uzayıp gider\n"
            ));
        }
        let d = digest_md(&md, 500);
        assert!(d.chars().count() <= 500 + "\n[truncated]".chars().count());
        assert!(d.ends_with("[truncated]"));
    }

    #[test]
    fn digest_md_bounds_each_section_so_later_headings_survive() {
        // A huge early section must NOT consume the whole budget — later headings survive.
        let md = format!("# H\n{}\n## H2\nkısa", "a".repeat(5000));
        let d = digest_md(&md, 300);
        assert!(d.contains("## H2"));
    }

    #[test]
    fn digest_txt_head_plus_stats() {
        let txt = format!("{}\n", "satır içeriği\n".repeat(500));
        let d = digest_txt(&txt, 8_000);
        assert!(d.starts_with("satır içeriği"));
        assert!(d.contains("lines"));
        assert!(d.contains("KB"));
    }

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
            Material {
                name: "a.md".into(),
                digest: "x".repeat(10_000),
            },
            Material {
                name: "b.md".into(),
                digest: "y".repeat(10_000),
            },
        ];
        let c = combined_digests(&mats).unwrap();
        assert!(c.contains("=== a.md ==="));
        assert!(c.chars().count() <= TOTAL_CAP + 50); // marker payı
        assert!(combined_digests(&[]).is_none());
    }

    #[test]
    fn convert_pdfs_missing_tool_reports_notice_and_no_txt() {
        let base = std::env::temp_dir().join(format!(
            "usta_materials_pdftotext_missing_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("materials");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("kitap.pdf"), "%PDF").unwrap();

        let notes = convert_pdfs_with(&base, "usta-no-such-pdftotext", PDF_TIMEOUT);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("pdftotext missing"));
        assert!(!dir.join("kitap.txt").exists());
        assert!(!dir.join(".converted").exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    fn pdf_project(tag: &str) -> PathBuf {
        let base =
            std::env::temp_dir().join(format!("usta_materials_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("materials")).unwrap();
        base
    }

    /// A stand-in for pdftotext: answers `-v`, otherwise runs `body` with
    /// `$1=-layout $2=<pdf> $3=<out>`. Lives outside materials/.
    #[cfg(unix)]
    fn fake_tool(base: &Path, body: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let p = base.join("fake-pdftotext");
        std::fs::write(
            &p,
            format!("#!/bin/sh\n[ \"$1\" = \"-v\" ] && exit 0\n{body}\n"),
        )
        .unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn convert_pdfs_never_touches_a_handwritten_sibling_txt() {
        let base = pdf_project("pdf_sibling");
        let dir = base.join("materials");
        std::fs::write(dir.join("kitap.txt"), "my own notes").unwrap();
        std::fs::write(dir.join("kitap.pdf"), "%PDF").unwrap();
        let notes = convert_pdfs_with(&base, "usta-no-such-pdftotext", PDF_TIMEOUT);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(
            std::fs::read_to_string(dir.join("kitap.txt")).unwrap(),
            "my own notes"
        );
        assert!(!dir.join(".converted").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn convert_pdfs_times_out_and_leaves_no_partial_output() {
        let base = pdf_project("pdf_timeout");
        let dir = base.join("materials");
        std::fs::write(dir.join("kitap.pdf"), "%PDF").unwrap();
        let tool = fake_tool(&base, "printf partial > \"$3\"\nsleep 5");
        let notes = convert_pdfs_with(&base, &tool, Duration::from_secs(1));
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("timed out"), "{notes:?}");
        assert!(!dir.join(".converted/kitap.txt").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn convert_pdfs_survives_a_noisy_stderr() {
        let base = pdf_project("pdf_noisy");
        let dir = base.join("materials");
        std::fs::write(dir.join("kitap.pdf"), "%PDF").unwrap();
        let tool = fake_tool(
            &base,
            "head -c 200000 /dev/zero | tr '\\0' 'e' >&2\nprintf 'hello' > \"$3\"",
        );
        let notes = convert_pdfs_with(&base, &tool, Duration::from_secs(3));
        assert_eq!(notes.len(), 1);
        assert!(notes[0].starts_with("converted:"), "{notes:?}");
        assert!(dir.join(".converted/kitap.txt").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn scan_skips_a_conversion_whose_pdf_is_gone() {
        let base = pdf_project("scan_orphan");
        let dir = base.join("materials");
        std::fs::create_dir_all(dir.join(".converted")).unwrap();
        std::fs::write(dir.join(".converted/gone.txt"), "stale text").unwrap();
        std::fs::write(dir.join(".converted/kept.txt"), "live text").unwrap();
        std::fs::write(dir.join("kept.PDF"), "%PDF").unwrap();
        let names: Vec<String> = scan(&base).into_iter().map(|m| m.name).collect();
        assert_eq!(names, vec!["kept.pdf".to_string()]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn convert_pdfs_notes_blank_output_once_as_scanned() {
        let base = pdf_project("pdf_blank");
        let dir = base.join("materials");
        std::fs::write(dir.join("kitap.pdf"), "%PDF").unwrap();
        let tool = fake_tool(&base, "printf '\\f\\n' > \"$3\"");
        let notes = convert_pdfs_with(&base, &tool, PDF_TIMEOUT);
        assert_eq!(notes.len(), 1);
        assert!(
            notes[0].contains("no text found") && notes[0].contains("scanned PDF?"),
            "{notes:?}"
        );
        assert!(
            dir.join(".converted/kitap.txt").exists(),
            "freshness marker stays"
        );
        assert!(
            convert_pdfs_with(&base, &tool, PDF_TIMEOUT).is_empty(),
            "reported once"
        );
        assert!(
            scan(&base).is_empty(),
            "a blank conversion adds no material"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn convert_pdfs_matches_extension_case_insensitively_and_scan_reads_it() {
        let base = pdf_project("pdf_case");
        let dir = base.join("materials");
        std::fs::write(dir.join("KITAP.PDF"), "%PDF").unwrap();
        let tool = fake_tool(&base, "printf 'hello world\\n' > \"$3\"");
        let notes = convert_pdfs_with(&base, &tool, PDF_TIMEOUT);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].starts_with("converted:"), "{notes:?}");
        assert!(dir.join(".converted/KITAP.txt").is_file());
        assert!(
            !dir.join("KITAP.txt").exists(),
            "nothing written next to the PDF"
        );
        let mats = scan(&base);
        assert_eq!(mats.len(), 1);
        assert_eq!(mats[0].name, "KITAP.pdf");
        assert!(mats[0].digest.contains("hello world"));
        assert!(
            convert_pdfs_with(&base, &tool, PDF_TIMEOUT).is_empty(),
            "fresh → skipped"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn convert_pdfs_no_pdfs_returns_empty() {
        let base = std::env::temp_dir().join(format!(
            "usta_materials_pdftotext_none_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("materials");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.md"), "# x").unwrap();
        assert!(convert_pdfs(&base).is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn materials_present_true_for_md() {
        let base =
            std::env::temp_dir().join(format!("usta_materials_present_md_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("materials");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.md"), "# x").unwrap();
        assert!(materials_present(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn materials_present_true_for_txt() {
        let base =
            std::env::temp_dir().join(format!("usta_materials_present_txt_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("materials");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        assert!(materials_present(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn materials_present_true_for_pdf() {
        let base =
            std::env::temp_dir().join(format!("usta_materials_present_pdf_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("materials");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("kitap.pdf"), "%PDF").unwrap();
        assert!(materials_present(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn materials_present_false_for_empty_dir() {
        let base = std::env::temp_dir().join(format!(
            "usta_materials_present_empty_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("materials");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!materials_present(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn materials_present_false_for_only_gitkeep() {
        let base = std::env::temp_dir().join(format!(
            "usta_materials_present_gitkeep_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("materials");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".gitkeep"), "").unwrap();
        assert!(!materials_present(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn materials_present_false_without_materials_dir() {
        let base = std::env::temp_dir().join(format!(
            "usta_materials_present_none_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        assert!(!materials_present(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn materials_present_false_for_other_extension_only() {
        let base =
            std::env::temp_dir().join(format!("usta_materials_present_png_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("materials");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("resim.png"), "x").unwrap();
        assert!(!materials_present(&base));
        let _ = std::fs::remove_dir_all(&base);
    }
}
