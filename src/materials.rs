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
}
