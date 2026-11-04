//! The scenario library. Built-in packages are embedded in the binary and
//! synced to `<global>/scenarios/library/` on first `/scenario` use (code-owned,
//! like the brain files); generated packages (`gen-` ids) land in the same tree, but
//! only after they passed their proof.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::needs_sync;
use crate::scenario::ledger::Ledger;
use crate::scenario::package::Package;

struct Embedded {
    id: &'static str,
    files: &'static [(&'static str, &'static str)],
}

macro_rules! pkg {
    ($id:literal, [$($f:literal),+ $(,)?]) => {
        Embedded {
            id: $id,
            files: &[$(($f, include_str!(concat!("../../scenarios/", $id, "/", $f)))),+],
        }
    };
}

const PACKAGES: &[Embedded] = &[
    pkg!(
        "linux-backup-permissions",
        [
            "task.toml",
            "instruction.md",
            "tone/adventure.md",
            "environment/Dockerfile",
            "environment/setup.sh",
            "solution/solve.sh",
            "tests/check.sh",
        ]
    ),
    pkg!(
        "linux-config-typo",
        [
            "task.toml",
            "instruction.md",
            "tone/adventure.md",
            "environment/Dockerfile",
            "environment/setup.sh",
            "environment/start.sh",
            "solution/solve.sh",
            "tests/check.sh",
        ]
    ),
    pkg!(
        "linux-disk-hog",
        [
            "task.toml",
            "instruction.md",
            "tone/adventure.md",
            "environment/Dockerfile",
            "environment/setup.sh",
            "solution/solve.sh",
            "tests/check.sh",
        ]
    ),
];

#[cfg(test)]
pub(crate) fn embedded_ids() -> Vec<&'static str> {
    PACKAGES.iter().map(|p| p.id).collect()
}

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
#[allow(dead_code)] // reached from the command layer in Task 12
pub(crate) fn fits_world(p: &Package, ledger: &Ledger) -> bool {
    !p.is_generated()
        || match (&p.story.company, &ledger.company) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        }
}

/// Write embedded packages that are missing or differ from the binary's copy.
pub(crate) fn sync(global: &Path) -> Result<PathBuf> {
    let root = library_root(global);
    for pkg in PACKAGES {
        for (rel, content) in pkg.files {
            let path = root.join(pkg.id).join(rel);
            if needs_sync(&path, content) {
                std::fs::create_dir_all(path.parent().expect("package file has a parent"))
                    .with_context(|| format!("cannot create {}", path.display()))?;
                std::fs::write(&path, content)
                    .with_context(|| format!("cannot write {}", path.display()))?;
            }
        }
    }
    Ok(root)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::package::tests::tmp;

    #[test]
    fn sync_writes_every_embedded_package_and_every_one_lints_clean() {
        let g = tmp("lib_sync");
        let root = sync(&g).unwrap();
        assert_eq!(root, g.join("scenarios/library"));
        for id in embedded_ids() {
            let p = Package::load(&root.join(id)).unwrap();
            assert_eq!(p.lint(), Vec::<String>::new(), "{id}");
            assert_eq!(p.id, id);
        }
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn sync_is_code_owned_edits_are_overwritten_and_it_is_idempotent() {
        let g = tmp("lib_owned");
        let root = sync(&g).unwrap();
        let f = root.join("linux-backup-permissions/instruction.md");
        let original = std::fs::read_to_string(&f).unwrap();
        std::fs::write(&f, "tampered").unwrap();
        sync(&g).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), original);
        sync(&g).unwrap();
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn load_all_filters_by_topic_and_sorts_by_id() {
        let g = tmp("lib_load");
        sync(&g).unwrap();
        let linux = load_all(&g, "linux").unwrap().pkgs;
        assert!(!linux.is_empty());
        let ids: Vec<&str> = linux.iter().map(|p| p.id.as_str()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
        assert!(load_all(&g, "rust").unwrap().pkgs.is_empty());
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn a_narrower_topic_finds_the_broader_packages_and_only_those() {
        let g = tmp("lib_prefix");
        sync(&g).unwrap();
        let linux = load_all(&g, "linux").unwrap().pkgs.len();
        assert_eq!(load_all(&g, "linux-security").unwrap().pkgs.len(), linux);
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
        sync(&g).unwrap();
        let root = library_root(&g);
        let d = crate::scenario::package::tests::write_pkg(&root);
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
    fn load_all_before_any_sync_is_empty_not_an_error() {
        let g = tmp("lib_empty");
        assert!(load_all(&g, "linux").unwrap().pkgs.is_empty());
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn the_story_chain_is_resolvable_and_acyclic() {
        let g = tmp("lib_story");
        sync(&g).unwrap();
        let pkgs = load_all(&g, "linux").unwrap().pkgs;
        // Every need is opened by ANOTHER package.
        for p in &pkgs {
            for n in &p.story.needs {
                assert!(
                    pkgs.iter()
                        .any(|o| o.id != p.id && o.story.opens.iter().any(|t| &t.id == n)),
                    "{} needs '{n}' but no other package opens it",
                    p.id
                );
            }
        }
        // Starting from an empty ledger, repeatedly finishing whatever is available reaches every package.
        let mut open: Vec<String> = vec![];
        let mut done: Vec<&str> = vec![];
        loop {
            let next = pkgs.iter().find(|p| {
                !done.contains(&p.id.as_str()) && p.story.needs.iter().all(|n| open.contains(n))
            });
            let Some(p) = next else { break };
            done.push(&p.id);
            open.extend(p.story.opens.iter().map(|t| t.id.clone()));
        }
        assert_eq!(
            done.len(),
            pkgs.len(),
            "story chain is stuck after {done:?}"
        );
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn embedded_ids_are_unique() {
        let ids = embedded_ids();
        let mut u = ids.clone();
        u.sort();
        u.dedup();
        assert_eq!(u.len(), ids.len());
    }

    #[test]
    fn check_scripts_exit_only_0_or_1() {
        // container.rs reads every code but 0, 1 and 124 as an internal failure, so a
        // check script must never exit with anything but 0 or 1.
        let g = tmp("lib_exit_codes");
        let root = sync(&g).unwrap();
        for id in embedded_ids() {
            let text = std::fs::read_to_string(root.join(id).join("tests/check.sh")).unwrap();
            for line in text.lines().filter(|l| !l.trim_start().starts_with('#')) {
                if let Some(rest) = line.trim().strip_prefix("exit ") {
                    let code = rest.split_whitespace().next().unwrap_or("");
                    assert!(
                        matches!(code, "0" | "1"),
                        "{id}: `{}` exits {code}",
                        line.trim()
                    );
                }
            }
            assert!(text.contains("exit 0") && text.contains("exit 1"), "{id}");
        }
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn another_worlds_generated_task_is_hidden_but_handwritten_ones_never_are() {
        let root = tmp("lib_world");
        let mut p = Package::load(&crate::scenario::package::tests::write_pkg(&root)).unwrap();
        let mine = Ledger {
            company: Some("Shopwell".into()),
            ..Ledger::default()
        };
        p.story.company = Some("Other Inc".into());
        assert!(fits_world(&p, &mine), "handwritten");
        p.id = "gen-x-1".into();
        assert!(!fits_world(&p, &mine));
        assert!(fits_world(&p, &Ledger::default()));
        p.story.company = None;
        assert!(fits_world(&p, &mine));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn install_moves_the_staged_package_into_the_library() {
        let g = tmp("lib_install");
        let staged = staging_root(&g).join("demo-pkg");
        let src = crate::scenario::package::tests::write_pkg(&g.join("src"));
        std::fs::create_dir_all(staged.parent().unwrap()).unwrap();
        std::fs::rename(&src, &staged).unwrap();
        let p = install(&g, &staged, "demo-pkg").unwrap();
        assert_eq!(p.id, "demo-pkg");
        assert!(!staged.exists() && library_root(&g).join("demo-pkg/task.toml").exists());
        let _ = std::fs::remove_dir_all(g);
    }
}
