//! The scenario library, `<global>/scenarios/library/`. Usta ships no tasks: every
//! package is generated, and lands here only after it passed its proof. Older
//! versions copied three hand-written `linux-*` packages here; they are never
//! written again and `hide_retired` stops offering them.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::scenario::ledger::Ledger;
use crate::scenario::package::Package;

pub(crate) fn library_root(global: &Path) -> PathBuf {
    global.join("scenarios/library")
}

pub(crate) fn staging_root(global: &Path) -> PathBuf {
    global.join("scenarios/staging")
}

/// Move a proven package into the library. The rename is the install: anything in
/// the library has passed.
pub(crate) fn install(global: &Path, staged: &Path, id: &str) -> Result<Package> {
    let dest = library_root(global).join(id);
    std::fs::create_dir_all(library_root(global))?;
    std::fs::rename(staged, &dest).with_context(|| format!("cannot add {id} to the library"))?;
    Package::load(&dest)
}

/// A generated task speaks of the company of the world it was written for.
pub(crate) fn fits_world(p: &Package, ledger: &Ledger) -> bool {
    match (&p.story.company, &ledger.company) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
}

/// Subjects whose hyphenated names are real sub-areas. A bare prefix rule would
/// give `c-sharp` the `c` packages and `go-to-market` the `go` ones.
const BROAD_TOPICS: &[&str] = &["linux"];

/// A session on `topic` gets packages of that topic or of the broader one it
/// extends: `linux-security` finds `linux`. Run records carry the package's
/// topic, so they match the same way.
pub(crate) fn topic_matches(pkg_topic: &str, topic: &str) -> bool {
    topic == pkg_topic
        || (BROAD_TOPICS.contains(&pkg_topic)
            && topic
                .strip_prefix(pkg_topic)
                .is_some_and(|rest| rest.starts_with('-')))
}

/// A package directory that did not load or failed lint. Its topic may be unknown, so
/// `bad` covers the whole library, whatever the session's topic.
#[derive(Debug)]
pub(crate) struct Bad {
    pub dir: String,
    pub problems: Vec<String>,
}

#[derive(Debug, Default)]
pub(crate) struct Loaded {
    pub pkgs: Vec<Package>,
    pub bad: Vec<Bad>,
}

/// Packages of `topic`, sorted by id, plus everything in the library that failed.
pub(crate) fn load_all(global: &Path, topic: &str) -> Result<Loaded> {
    let mut out = Loaded::default();
    let Ok(dir) = std::fs::read_dir(library_root(global)) else {
        return Ok(out);
    };
    for path in dir.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match Package::load(&path) {
            Err(e) => out.bad.push(Bad {
                dir: name,
                problems: vec![format!("{e:#}")],
            }),
            Ok(p) => {
                let problems = p.lint();
                if !problems.is_empty() {
                    out.bad.push(Bad {
                        dir: name,
                        problems,
                    });
                } else if topic_matches(&p.topic, topic) {
                    out.pkgs.push(p);
                }
            }
        }
    }
    out.pkgs.sort_by(|a, b| a.id.cmp(&b.id));
    out.bad.sort_by(|a, b| a.dir.cmp(&b.dir));
    Ok(out)
}

/// The hand-written packages older versions copied into the library.
const RETIRED: &[&str] = &[
    "linux-backup-permissions",
    "linux-config-typo",
    "linux-disk-hog",
];

/// Stop offering retired copies, in the list and in `bad`. The copies stay on disk
/// (the library is shared by projects), and one that a run in `in_use` still
/// names is kept so that run can be resumed.
pub(crate) fn hide_retired(loaded: &mut Loaded, in_use: &[String]) {
    let gone = |id: &str| RETIRED.contains(&id) && !in_use.iter().any(|u| u == id);
    loaded.pkgs.retain(|p| !gone(&p.id));
    loaded.bad.retain(|b| !gone(&b.dir));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::package::tests::{tmp, write_pkg};

    /// A lint-clean package in the library under `id` (a retired id, say).
    fn put(global: &Path, id: &str) {
        let d = write_pkg(&library_root(global));
        let moved = library_root(global).join(id);
        std::fs::rename(&d, &moved).unwrap();
        let t = std::fs::read_to_string(moved.join("task.toml")).unwrap();
        std::fs::write(moved.join("task.toml"), t.replace("demo-pkg", id)).unwrap();
    }

    #[test]
    fn load_all_filters_by_topic_and_sorts_by_id() {
        let g = tmp("lib_load");
        put(&g, "linux-b");
        put(&g, "linux-a");
        let linux = load_all(&g, "linux").unwrap().pkgs;
        let ids: Vec<&str> = linux.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["linux-a", "linux-b"]);
        assert!(load_all(&g, "rust").unwrap().pkgs.is_empty());
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn a_narrower_topic_finds_the_broader_packages_and_only_those() {
        let g = tmp("lib_prefix");
        put(&g, "linux-a");
        assert_eq!(load_all(&g, "linux-security").unwrap().pkgs.len(), 1);
        assert!(load_all(&g, "linuxish").unwrap().pkgs.is_empty());
        assert!(load_all(&g, "lin").unwrap().pkgs.is_empty());
        assert!(!topic_matches("linux-security", "linux"));
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn only_broad_topics_extend_to_narrower_ones() {
        assert!(topic_matches("linux", "linux-security"));
        assert!(topic_matches("c", "c"));
        assert!(!topic_matches("c", "c-sharp"));
        assert!(!topic_matches("go", "go-to-market"));
    }

    #[test]
    fn a_package_that_fails_lint_is_reported_not_skipped() {
        let g = tmp("lib_bad");
        let root = library_root(&g);
        let d = write_pkg(&root);
        std::fs::write(
            d.join("task.toml"),
            std::fs::read_to_string(d.join("task.toml"))
                .unwrap()
                .replace("script = \"check.sh\"", "script = \"../../evil.sh\""),
        )
        .unwrap();
        std::fs::create_dir_all(root.join("broken")).unwrap();
        std::fs::write(root.join("broken/task.toml"), "not toml [").unwrap();
        let l = load_all(&g, "linux").unwrap();
        assert!(l.pkgs.iter().all(|p| p.id != "demo-pkg"));
        let dirs: Vec<&str> = l.bad.iter().map(|b| b.dir.as_str()).collect();
        assert!(
            dirs.contains(&"demo-pkg") && dirs.contains(&"broken"),
            "{dirs:?}"
        );
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn load_all_with_no_library_is_empty_not_an_error() {
        let g = tmp("lib_empty");
        assert!(load_all(&g, "linux").unwrap().pkgs.is_empty());
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn another_worlds_task_is_hidden() {
        let root = tmp("lib_world");
        let mut p = Package::load(&write_pkg(&root)).unwrap();
        let mine = Ledger {
            company: Some("Shopwell".into()),
            ..Ledger::default()
        };
        p.story.company = Some("Other Inc".into());
        assert!(!fits_world(&p, &mine));
        assert!(fits_world(&p, &Ledger::default()));
        p.story.company = None;
        assert!(fits_world(&p, &mine));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn retired_copies_are_not_offered_unless_a_run_still_uses_them() {
        let g = tmp("lib_retired");
        put(&g, "linux-config-typo");
        put(&g, "linux-fresh");
        let mut l = load_all(&g, "linux").unwrap();
        hide_retired(&mut l, &[]);
        let ids: Vec<&str> = l.pkgs.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["linux-fresh"]);
        let mut l = load_all(&g, "linux").unwrap();
        hide_retired(&mut l, &["linux-config-typo".to_string()]);
        assert_eq!(l.pkgs.len(), 2, "the one a run names stays");
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn a_damaged_retired_copy_is_not_reported_when_nothing_uses_it() {
        let g = tmp("lib_retired_bad");
        let bad = library_root(&g).join("linux-disk-hog");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(bad.join("task.toml"), "not toml [").unwrap();
        let mut l = load_all(&g, "linux").unwrap();
        assert_eq!(l.bad.len(), 1);
        hide_retired(&mut l, &[]);
        assert!(l.bad.is_empty());
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn install_moves_the_staged_package_into_the_library() {
        let g = tmp("lib_install");
        let staged = staging_root(&g).join("demo-pkg");
        let src = write_pkg(&g.join("src"));
        std::fs::create_dir_all(staged.parent().unwrap()).unwrap();
        std::fs::rename(&src, &staged).unwrap();
        let p = install(&g, &staged, "demo-pkg").unwrap();
        assert_eq!(p.id, "demo-pkg");
        assert!(!staged.exists() && library_root(&g).join("demo-pkg/task.toml").exists());
        let _ = std::fs::remove_dir_all(g);
    }
}
