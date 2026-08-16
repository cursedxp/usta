//! One-shot deterministic migration: legacy Turkish protocol tokens → English.
//! Context-locked — free prose is never touched. Idempotent. The ONLY place
//! legacy Turkish tokens are allowed to appear in src/.
//!
//! Directory scope (verified against progress.rs/brain.rs/transcript.rs/config.rs,
//! spec §4/§7): global root `~/.config/usta/` holds USER.md/SOUL.md/RULES.md/
//! TEACHING.md/GOAL.md/MATERIAL.md/PREDICTION.md/GAMIFICATION.md/USTA.md directly
//! (root scan) + `approaches/*.md` + `learner/*.md` (index.md, history.md — no
//! nested subdirs globally, curriculum/progress moved project-local in v0.6).
//! Project `.usta/` holds `approaches/*.md` + `learner/progress/<topic>.md` +
//! `learner/curriculum/<topic>.md` + `sessions/<topic>-<stamp>.jsonl` (raw
//! transcripts — `.jsonl`, not `.md`; each line is one escaped JSON record, so
//! only the context-free MARKERS substitutions can ever match inside them —
//! header/state-line matching requires a bare line and never fires there).
//! `mentor/PROJECT.md` / `mentor/PROGRESS.md` are OUT OF SCOPE here: they live
//! at the project root, sibling to (not inside) `.usta/`, outside this
//! function's `project_usta: &Path` parameter.

use std::fs;
use std::path::{Path, PathBuf};

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

/// `.bak` / `.tmp` sibling path — appends to the FULL filename rather than
/// swapping the last extension (`with_extension` would mangle multi-dot names
/// like `x.done.jsonl`). `file.md` → `file.md.bak`, `x.jsonl` → `x.jsonl.bak`.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

fn migrate_file(path: &Path) -> Result<bool> {
    let Ok(content) = fs::read_to_string(path) else { return Ok(false) };
    let Some(new) = migrate_content(&content) else { return Ok(false) };
    let bak = sibling(path, ".bak");
    if !bak.exists() { fs::copy(path, &bak)?; } // ilk hal korunur, asla ezilmez
    let tmp = sibling(path, ".tmp");
    fs::write(&tmp, &new)?;
    fs::rename(&tmp, path)?; // atomik
    Ok(true)
}

/// Walk both trees; returns the number of migrated files.
pub fn run(global: &Path, project_usta: Option<&Path>) -> Result<usize> {
    let mut n = 0;
    // (root, subdir relative to root, file extension to touch)
    let mut targets: Vec<(PathBuf, &str, &str)> = vec![
        (global.to_path_buf(), "", "md"),
        (global.to_path_buf(), "approaches", "md"),
        (global.to_path_buf(), "learner", "md"),
    ];
    if let Some(p) = project_usta {
        targets.push((p.to_path_buf(), "approaches", "md"));
        targets.push((p.to_path_buf(), "learner/progress", "md"));
        targets.push((p.to_path_buf(), "learner/curriculum", "md"));
        targets.push((p.to_path_buf(), "sessions", "jsonl"));
    }
    for (root, sub, ext) in targets {
        let dir = if sub.is_empty() { root } else { root.join(sub) };
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == ext) && migrate_file(&p)? { n += 1; }
        }
    }
    Ok(n)
}

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

    /// Lets each file-level test set up its own isolated global/project pair.
    fn temp_pair(name: &str) -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("usta_migrate_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let global = base.join("global");
        let project_usta = base.join("project/.usta");
        fs::create_dir_all(global.join("learner")).unwrap();
        fs::create_dir_all(project_usta.join("sessions")).unwrap();
        (global, project_usta)
    }

    #[test]
    fn bak_preserved_across_second_migration() {
        let (global, project_usta) = temp_pair("bak");
        let user_md = global.join("USER.md");
        fs::write(&user_md, "## Tercihler\n- gamification: on\n").unwrap();

        let first = run(&global, Some(&project_usta)).unwrap();
        assert_eq!(first, 1);
        let bak = super::sibling(&user_md, ".bak");
        assert!(bak.exists());
        let bak_content = fs::read_to_string(&bak).unwrap();
        assert_eq!(bak_content, "## Tercihler\n- gamification: on\n");

        // Mutate the .bak to prove a second migration never overwrites it —
        // first-captured state is preserved forever.
        fs::write(&bak, "SENTINEL — never overwritten").unwrap();

        let second = run(&global, Some(&project_usta)).unwrap();
        assert_eq!(second, 0); // idempotent: already-migrated file, nothing left to change
        assert_eq!(fs::read_to_string(&bak).unwrap(), "SENTINEL — never overwritten");
    }

    #[test]
    fn run_second_pass_returns_zero() {
        let (global, project_usta) = temp_pair("second_pass");
        fs::write(global.join("USER.md"), "## Tercihler\n- gamification: on\n").unwrap();
        fs::write(
            project_usta.join("sessions").join("rust-1.jsonl"),
            "{\"role\":\"assistant\",\"text\":\"===DOSYA: rust.md===\\n## Seviye\\northa\"}\n",
        )
        .unwrap();

        assert_eq!(run(&global, Some(&project_usta)).unwrap(), 2);
        assert_eq!(run(&global, Some(&project_usta)).unwrap(), 0);
    }
}
