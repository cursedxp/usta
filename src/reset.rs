//! `usta reset <topic>`: removes every per-topic artifact of the current
//! project — progress, approach, curriculum (and their `.bak`), generated
//! visuals, session transcripts — plus the topic's catalog row. Moved out of
//! `setup.rs` (600-line budget). Not touched: the topic lock (a live session
//! may own it), `learner/history.md` (global stats), `mentor/` (the user's docs).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::setup::{confirm, run_migration};
use crate::{config, index, progress, transcript};

/// `usta reset <topic>` — list what will go, confirm, remove. No LLM needed.
pub(crate) fn run_reset_topic(topic: &str) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let Some(root) = config::find_project_root(&cwd) else {
        anyhow::bail!("no .usta in this directory (or above) — no project found to reset");
    };
    let global = config::global_root()?;
    run_migration(&global, Some(&root.join(".usta")));
    let index_path = global.join("learner/index.md");
    let plan = plan_reset(&root, &index_path, topic);
    if plan.is_empty() {
        println!("nothing to reset for {topic}");
        return Ok(());
    }
    println!("{topic}: will be removed:");
    for p in &plan.artifacts {
        println!("  {}", p.display());
    }
    if plan.catalog.is_some() {
        println!("  catalog row in {}", index_path.display());
    }
    if !confirm("Are you sure? [y/N] ", &["e", "evet", "y", "yes"])? {
        println!("cancelled.");
        return Ok(());
    }
    apply_reset(&plan, &index_path)?;
    println!("reset: {topic}");
    Ok(())
}

/// What a reset would remove: the existing artifacts and, when the catalog has
/// a row for (topic, project), the catalog text without it.
struct ResetPlan {
    artifacts: Vec<PathBuf>,
    catalog: Option<String>,
}

impl ResetPlan {
    fn is_empty(&self) -> bool {
        self.artifacts.is_empty() && self.catalog.is_none()
    }
}

fn plan_reset(root: &Path, index_path: &Path, topic: &str) -> ResetPlan {
    let catalog = std::fs::read_to_string(index_path)
        .ok()
        .filter(|c| {
            index::entries(c)
                .iter()
                .any(|e| e.topic == topic && e.project == root)
        })
        .map(|c| index::remove(&c, topic, root));
    ResetPlan {
        artifacts: topic_artifacts(root, topic),
        catalog,
    }
}

/// Remove the artifacts, then the catalog row — the row goes even when no
/// progress file exists (a cancelled session leaves a row and maybe a curriculum).
fn apply_reset(plan: &ResetPlan, index_path: &Path) -> Result<()> {
    remove_artifacts(&plan.artifacts)?;
    if let Some(updated) = &plan.catalog {
        progress::write_atomic(index_path, updated)?;
    }
    Ok(())
}

/// Every EXISTING per-topic artifact under `root`. Transcripts match the topic
/// exactly (`rust` never takes `rust-async`'s records).
fn topic_artifacts(root: &Path, topic: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for md in [
        progress::progress_path(root, topic),
        progress::approach_path(root, topic),
        progress::curriculum_path(root, topic),
    ] {
        let bak = md.with_extension("md.bak");
        out.push(md);
        out.push(bak);
    }
    out.push(root.join(".usta/visuals").join(topic));
    out.retain(|p| p.exists());
    if let Ok(rd) = std::fs::read_dir(root.join(".usta/sessions")) {
        let mut records: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| record_topic(p).as_deref() == Some(topic))
            .collect();
        records.sort();
        out.extend(records);
    }
    out
}

/// Topic of a transcript, `.done.jsonl` included — `topic_from_record` alone
/// reads the stem `x-<date>-<time>.done` as having no timestamp.
fn record_topic(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let stem = name
        .strip_suffix(".done.jsonl")
        .or_else(|| name.strip_suffix(".jsonl"))?;
    transcript::topic_from_record(Path::new(&format!("{stem}.jsonl")))
}

/// Delete files and directories; one that is already gone is fine (idempotent).
fn remove_artifacts(paths: &[PathBuf]) -> Result<()> {
    for p in paths {
        let res = if p.is_dir() {
            std::fs::remove_dir_all(p)
        } else {
            std::fs::remove_file(p)
        };
        match res {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("could not delete: {}", p.display())),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("usta_reset_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "x").unwrap();
    }

    #[test]
    fn topic_artifacts_lists_exactly_this_topics_files() {
        let root = project("artifacts");
        for t in ["rust", "rust-async"] {
            touch(&progress::progress_path(&root, t));
            touch(&progress::approach_path(&root, t));
            touch(&progress::curriculum_path(&root, t));
            touch(&root.join(".usta/visuals").join(t).join("a.html"));
            touch(&root.join(format!(".usta/sessions/{t}-20261026-101500.jsonl")));
            touch(&root.join(format!(".usta/sessions/{t}-20261025-090000.done.jsonl")));
        }
        touch(&progress::progress_path(&root, "rust").with_extension("md.bak"));
        touch(&crate::lifecycle::lock_path(&root, "rust"));

        let got = topic_artifacts(&root, "rust");
        // progress + its .bak, approach, curriculum, visuals dir, two transcripts
        assert_eq!(got.len(), 7, "{got:?}");
        assert!(got
            .iter()
            .all(|p| !p.to_string_lossy().contains("rust-async")));
        assert!(!got.contains(&crate::lifecycle::lock_path(&root, "rust")));

        remove_artifacts(&got).unwrap();
        assert!(topic_artifacts(&root, "rust").is_empty());
        assert_eq!(topic_artifacts(&root, "rust-async").len(), 6);
        assert!(remove_artifacts(&got).is_ok(), "removal is idempotent");
        assert!(crate::lifecycle::lock_path(&root, "rust").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn reset_without_progress_still_removes_curriculum_and_catalog_row() {
        let root = project("noprogress");
        let global = root.join("global");
        touch(&progress::curriculum_path(&root, "rust"));
        index::record(&global, "rust", &root, "2026-10-26").unwrap();
        index::record(&global, "dns", &root, "2026-10-26").unwrap();
        let index_path = global.join("learner/index.md");

        let plan = plan_reset(&root, &index_path, "rust");
        assert_eq!(
            plan.artifacts,
            vec![progress::curriculum_path(&root, "rust")]
        );
        assert!(plan.catalog.is_some());
        apply_reset(&plan, &index_path).unwrap();

        assert!(!progress::curriculum_path(&root, "rust").exists());
        let cat = std::fs::read_to_string(&index_path).unwrap();
        let topics: Vec<String> = index::entries(&cat).into_iter().map(|e| e.topic).collect();
        assert_eq!(topics, vec!["dns".to_string()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn nothing_to_reset_without_files_or_catalog_row() {
        let root = project("nothing");
        let plan = plan_reset(&root, &root.join("global/learner/index.md"), "rust");
        assert!(plan.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn record_topic_reads_open_and_done_records() {
        let p = |n: &str| PathBuf::from(format!("/p/.usta/sessions/{n}"));
        assert_eq!(
            record_topic(&p("rust-20261026-101500.jsonl")).as_deref(),
            Some("rust")
        );
        assert_eq!(
            record_topic(&p("rust-20261026-101500.done.jsonl")).as_deref(),
            Some("rust")
        );
        assert_eq!(
            record_topic(&p("rust-async-20261026-101500.done.jsonl")).as_deref(),
            Some("rust-async")
        );
        assert_eq!(record_topic(&p("notes.txt")), None);
    }
}
