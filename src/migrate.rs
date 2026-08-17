//! One-shot deterministic migration: legacy Turkish protocol tokens → English.
//! Context-locked — free prose is never touched. Idempotent. The ONLY place
//! legacy Turkish tokens are allowed to appear in src/.
//!
//! Directory scope (verified against progress.rs/brain.rs/transcript.rs/config.rs,
//! spec §4 "nereye bakar"/§7): global root `~/.config/usta/` — at the root
//! level ONLY `USER.md` (the user-owned profile). The other root files
//! (SOUL.md, RULES.md, TEACHING.md, GOAL.md, MATERIAL.md, PREDICTION.md,
//! GAMIFICATION.md, USTA.md) are code-owned templates resynced from the
//! English embeds — migrating them would only create pointless `.bak`/`.tmp`
//! churn, so they are deliberately out of scope. Subdirs stay in scope:
//! `approaches/*.md` + `learner/*.md` (index.md, history.md — no nested
//! subdirs globally, curriculum/progress moved project-local in v0.6).
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

/// Full-line (line-start) header mappings. Exact full-line match (see below) — list order is inert.
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
            if new.trim_end() == old {
                new = newh.to_string();
                break;
            }
        }
        // 2) `# <topic> — İlerleme` heading.
        if new.starts_with("# ") && new.trim_end().ends_with("— İlerleme") {
            new = format!(
                "{}— Progress",
                new.trim_end().strip_suffix("— İlerleme").unwrap()
            );
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
            if new.contains(old) {
                new = new.replace(old, newm);
            }
        }
        if new != body {
            changed = true;
        }
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
    let Ok(content) = fs::read_to_string(path) else {
        return Ok(false);
    };
    let Some(new) = migrate_content(&content) else {
        return Ok(false);
    };
    let bak = sibling(path, ".bak");
    if !bak.exists() {
        fs::copy(path, &bak)?;
    } // ilk hal korunur, asla ezilmez
    let tmp = sibling(path, ".tmp");
    fs::write(&tmp, &new)?;
    fs::rename(&tmp, path)?; // atomik
    Ok(true)
}

/// Walk both trees; returns the number of migrated files.
pub fn run(global: &Path, project_usta: Option<&Path>) -> Result<usize> {
    let mut n = 0;
    // Global ROOT: exactly USER.md — never a `*.md` glob. The sibling root
    // files are code-owned templates (see module doc); sweeping them in would
    // produce .bak churn for files the scaffold resyncs anyway (spec §4).
    let user_md = global.join("USER.md");
    if user_md.is_file() && migrate_file(&user_md)? {
        n += 1;
    }
    // (root, subdir relative to root, file extension to touch)
    let mut targets: Vec<(PathBuf, &str, &str)> = vec![
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
        let dir = root.join(sub);
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == ext) && migrate_file(&p)? {
                n += 1;
            }
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
        assert_eq!(
            fs::read_to_string(&bak).unwrap(),
            "SENTINEL — never overwritten"
        );
    }

    #[test]
    fn run_second_pass_returns_zero() {
        let (global, project_usta) = temp_pair("second_pass");
        fs::write(global.join("USER.md"), "## Tercihler\n- gamification: on\n").unwrap();
        let session = project_usta.join("sessions").join("rust-1.jsonl");
        fs::write(
            &session,
            "{\"role\":\"assistant\",\"text\":\"===DOSYA: rust.md===\\n## Seviye\\northa\"}\n",
        )
        .unwrap();

        assert_eq!(run(&global, Some(&project_usta)).unwrap(), 2);
        assert_eq!(run(&global, Some(&project_usta)).unwrap(), 0);

        // The migrated session record must still be valid JSON — the marker
        // substitution is plain ASCII and may never corrupt the escaping.
        let migrated = fs::read_to_string(&session).unwrap();
        let line = migrated.lines().next().unwrap();
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(
            v["text"].as_str().unwrap(),
            "===FILE: rust.md===\n## Seviye\northa"
        );
    }

    /// Task 6 integration coverage: a single `run` call sweeping BOTH trees at
    /// once — global root (USER.md + learner/index.md) and a project `.usta`
    /// (learner/progress/<topic>.md) — proving the multi-file/multi-root count
    /// and cross-tree `.bak` behavior in one pass. File-level substitution
    /// rules are already covered above; this test's added value is the
    /// end-to-end fan-out that setup.rs's wiring actually exercises.
    #[test]
    fn run_sweeps_global_and_project_trees_in_one_call() {
        let (global, project_usta) = temp_pair("multi_root");
        fs::create_dir_all(project_usta.join("learner/progress")).unwrap();

        let user_md = global.join("USER.md");
        fs::write(&user_md, "## Tercihler\n- gamification: on\n").unwrap();
        let index_md = global.join("learner/index.md");
        fs::write(&index_md, "## Kayıtlar\n- rust | /proje | 2026-08-01\n").unwrap();
        let progress_md = project_usta.join("learner/progress/rust.md");
        fs::write(
            &progress_md,
            "# rust — İlerleme\n\n## Seviye\n- a: oturdu\n",
        )
        .unwrap();

        let first = run(&global, Some(&project_usta)).unwrap();
        assert!(
            first >= 3,
            "expected at least 3 migrated files, got {first}"
        );

        assert!(fs::read_to_string(&user_md)
            .unwrap()
            .contains("## Preferences"));
        assert!(fs::read_to_string(&index_md)
            .unwrap()
            .contains("## Records"));
        let progress_after = fs::read_to_string(&progress_md).unwrap();
        assert!(progress_after.contains("# rust — Progress"));
        assert!(progress_after.contains("## Level"));
        assert!(progress_after.contains("- a: settled"));

        // .baks hold the first (pre-migration) state, across both trees.
        assert_eq!(
            fs::read_to_string(super::sibling(&user_md, ".bak")).unwrap(),
            "## Tercihler\n- gamification: on\n"
        );
        assert_eq!(
            fs::read_to_string(super::sibling(&index_md, ".bak")).unwrap(),
            "## Kayıtlar\n- rust | /proje | 2026-08-01\n"
        );
        assert_eq!(
            fs::read_to_string(super::sibling(&progress_md, ".bak")).unwrap(),
            "# rust — İlerleme\n\n## Seviye\n- a: oturdu\n"
        );

        // Second run: idempotent across the whole sweep — zero further changes.
        assert_eq!(run(&global, Some(&project_usta)).unwrap(), 0);
    }

    #[test]
    fn code_owned_root_files_out_of_scope_only_user_md_migrated() {
        let (global, project_usta) = temp_pair("root_scope");
        // Code-owned template at the global root with Turkish tokens — must NOT
        // be touched (resynced from English embeds; migrating = .bak churn).
        let soul_tr = "## Tercihler\n- a: oturdu\n";
        fs::write(global.join("SOUL.md"), soul_tr).unwrap();
        // User-owned profile — the only root-level file in migration scope.
        fs::write(global.join("USER.md"), "## Tercihler\n- gamification: on\n").unwrap();

        assert_eq!(run(&global, Some(&project_usta)).unwrap(), 1); // USER.md only
        assert_eq!(fs::read_to_string(global.join("SOUL.md")).unwrap(), soul_tr);
        assert!(!super::sibling(&global.join("SOUL.md"), ".bak").exists());
        let user = fs::read_to_string(global.join("USER.md")).unwrap();
        assert!(user.contains("## Preferences"));
        assert!(super::sibling(&global.join("USER.md"), ".bak").exists());
    }
}
