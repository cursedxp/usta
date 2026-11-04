//! The scenario library. Built-in packages are embedded in the binary and
//! synced to `<global>/scenarios/library/` on first `/scenario` use (code-owned,
//! like the brain files); packages generated in step 2 will land in the same tree.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::needs_sync;
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

/// Packages of `topic`, sorted by id. A directory that does not load is skipped.
pub(crate) fn load_all(global: &Path, topic: &str) -> Result<Vec<Package>> {
    let Ok(dir) = std::fs::read_dir(library_root(global)) else {
        return Ok(Vec::new());
    };
    let mut pkgs: Vec<Package> = dir
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| Package::load(&e.path()).ok())
        .filter(|p| p.topic == topic)
        .collect();
    pkgs.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(pkgs)
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
        let linux = load_all(&g, "linux").unwrap();
        assert!(!linux.is_empty());
        let ids: Vec<&str> = linux.iter().map(|p| p.id.as_str()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
        assert!(load_all(&g, "rust").unwrap().is_empty());
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn load_all_before_any_sync_is_empty_not_an_error() {
        let g = tmp("lib_empty");
        assert!(load_all(&g, "linux").unwrap().is_empty());
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn the_story_chain_is_resolvable_and_acyclic() {
        let g = tmp("lib_story");
        sync(&g).unwrap();
        let pkgs = load_all(&g, "linux").unwrap();
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
        // container.rs reads 125-127 and 137 as infrastructure failures, so a check
        // script must never exit with anything but 0 or 1.
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
}
