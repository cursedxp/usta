//! Course-material ingestion (spec: material ingest). The shell produces
//! deterministic digests — heading skeleton + short excerpts — that get
//! injected into the NEW-TOPIC introduction turn. Usta anchors the curriculum
//! to the material; the USER does the reading. No LLM here, no persistence.

use std::path::{Path, PathBuf};

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

/// Lightweight existence check: is there at least one md/txt/pdf file under
/// `materials/`? No digesting — just a presence probe, used to gate whether
/// the Course Material teaching rules are worth loading into the prompt at
/// all (an empty/absent `materials/` dir means those rules are dead weight).
pub fn materials_present(project_root: &Path) -> bool {
    let root = project_root.join("materials");
    let mut files: Vec<PathBuf> = Vec::new();
    collect_files(&root, &mut files);
    files
        .iter()
        .any(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("md" | "txt" | "pdf")))
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

#[cfg(test)]
mod tests {
    use super::*;

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
            md.push_str(&format!("## Bölüm {i} çğüşöı\nçğüşöı içerik satırı burada uzayıp gider\n"));
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
            Material { name: "a.md".into(), digest: "x".repeat(10_000) },
            Material { name: "b.md".into(), digest: "y".repeat(10_000) },
        ];
        let c = combined_digests(&mats).unwrap();
        assert!(c.contains("=== a.md ==="));
        assert!(c.chars().count() <= TOTAL_CAP + 50); // marker payı
        assert!(combined_digests(&[]).is_none());
    }

    #[test]
    fn convert_pdfs_missing_tool_reports_notice_and_no_txt() {
        // Deterministic only while pdftotext is absent from PATH (true in CI here).
        // If a CI image adds poppler-utils, this exercises the real-conversion path instead.
        let base = std::env::temp_dir().join(format!("usta_materials_pdftotext_missing_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("materials");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("kitap.pdf"), "%PDF").unwrap();

        let notes = convert_pdfs(&base);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("pdftotext missing"));
        assert!(!dir.join("kitap.txt").exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn convert_pdfs_no_pdfs_returns_empty() {
        let base = std::env::temp_dir().join(format!("usta_materials_pdftotext_none_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("materials");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.md"), "# x").unwrap();
        assert!(convert_pdfs(&base).is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn materials_present_true_for_md() {
        let base = std::env::temp_dir().join(format!("usta_materials_present_md_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("materials");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.md"), "# x").unwrap();
        assert!(materials_present(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn materials_present_true_for_txt() {
        let base = std::env::temp_dir().join(format!("usta_materials_present_txt_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("materials");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        assert!(materials_present(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn materials_present_true_for_pdf() {
        let base = std::env::temp_dir().join(format!("usta_materials_present_pdf_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("materials");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("kitap.pdf"), "%PDF").unwrap();
        assert!(materials_present(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn materials_present_false_for_empty_dir() {
        let base = std::env::temp_dir().join(format!("usta_materials_present_empty_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("materials");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!materials_present(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn materials_present_false_for_only_gitkeep() {
        let base = std::env::temp_dir().join(format!("usta_materials_present_gitkeep_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("materials");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".gitkeep"), "").unwrap();
        assert!(!materials_present(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn materials_present_false_without_materials_dir() {
        let base = std::env::temp_dir().join(format!("usta_materials_present_none_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        assert!(!materials_present(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn materials_present_false_for_other_extension_only() {
        let base = std::env::temp_dir().join(format!("usta_materials_present_png_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("materials");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("resim.png"), "x").unwrap();
        assert!(!materials_present(&base));
        let _ = std::fs::remove_dir_all(&base);
    }
}
