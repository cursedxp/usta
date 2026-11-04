//! Static package checks, run when the library loads. A package that fails is
//! reported to the learner and left out, never run.

use std::path::{Path, PathBuf};

use crate::scenario::package::{Package, Tone};

const MAX_FILES: usize = 200;

/// True for a plain relative path that stays inside its root.
pub(crate) fn safe_rel(p: &str) -> bool {
    use std::path::Component;
    !p.is_empty()
        && !p.contains('\\')
        && !p.contains('\0')
        && !p.contains(':')
        && Path::new(p)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

/// Every regular file under `dir`. Links and special files are refused, not followed.
pub(crate) fn walk(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        let entries = std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        for e in entries.flatten() {
            let path = e.path();
            let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if meta.is_dir() {
                todo.push(path);
            } else if meta.is_file() {
                out.push(path);
            } else {
                return Err(format!("{} is not a regular file", path.display()));
            }
            if out.len() > MAX_FILES {
                return Err(format!("more than {MAX_FILES} files"));
            }
        }
    }
    out.sort();
    Ok(out)
}

pub(crate) fn reads_history(script: &str) -> bool {
    let l = script.to_ascii_lowercase();
    if ["bash_history", "zsh_history", "histfile"]
        .iter()
        .any(|k| l.contains(k))
    {
        return true;
    }
    l.lines()
        .filter(|x| !x.trim_start().starts_with('#'))
        .any(|line| {
            line.split(['|', ';', '(', '&', '`']).any(|part| {
                part.split_whitespace()
                    .next()
                    .is_some_and(|w| w.trim_end_matches(')') == "history")
            })
        })
}

impl Package {
    /// Static package problems (empty = fine). The model-driven validation
    /// (fresh check fails, reference solution passes) is step 2 plus the
    /// `#[ignore]`d Docker certification in `docker_tests.rs`.
    pub fn lint(&self) -> Vec<String> {
        let mut p = Vec::new();
        if let Err(e) = walk(&self.dir) {
            p.push(e);
        }
        let context_ok = safe_rel(&self.environment.context);
        if !context_ok {
            p.push(format!(
                "environment.context '{}' must stay inside the package",
                self.environment.context
            ));
        }
        let script_ok = !self.check.script.contains('/') && safe_rel(&self.check.script);
        if !script_ok {
            p.push(format!(
                "check.script '{}' must be a file name under tests/",
                self.check.script
            ));
        }
        let dir_name = self.dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if self.id != dir_name {
            p.push(format!(
                "id '{}' does not match directory '{dir_name}'",
                self.id
            ));
        }
        if self.id.is_empty()
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            p.push("id must be lowercase letters, digits and '-'".into());
        }
        if self.topic.trim().is_empty() {
            p.push("topic is empty".into());
        }
        if self.skills.is_empty() || self.skills.iter().any(|s| s.trim().is_empty()) {
            p.push("skills must be a non-empty list of non-empty keywords".into());
        }
        for f in ["instruction.md", "solution/solve.sh", "tone/adventure.md"] {
            if !self.dir.join(f).is_file() {
                p.push(format!("missing {f}"));
            }
        }
        if self.environment.kind == "container"
            && context_ok
            && !self
                .dir
                .join(&self.environment.context)
                .join("Dockerfile")
                .is_file()
        {
            p.push(format!("missing {}/Dockerfile", self.environment.context));
        }
        for (label, path) in [
            ("setup", &self.environment.setup),
            ("start", &self.environment.start),
        ] {
            if let Some(s) = path {
                if !s.starts_with('/') {
                    p.push(format!(
                        "environment.{label} must be an absolute path inside the container"
                    ));
                }
            }
        }
        if script_ok {
            match std::fs::read_to_string(self.dir.join("tests").join(&self.check.script)) {
                Err(_) => p.push(format!("missing tests/{}", self.check.script)),
                Ok(t) => {
                    if reads_history(&t) {
                        p.push(
                            "check script reads shell history; check system state, not commands typed"
                                .into(),
                        );
                    }
                }
            }
        }
        let thread_ids = self
            .story
            .opens
            .iter()
            .map(|t| &t.id)
            .chain(self.story.closes.iter())
            .chain(self.story.needs.iter());
        for id in thread_ids {
            if id.is_empty()
                || !id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            {
                p.push(format!(
                    "thread id '{id}' must be lowercase letters, digits and '-'"
                ));
            }
        }
        for part in ["open", "close"] {
            if self.tone_text(Tone::Adventure, part).is_err() {
                p.push(format!(
                    "tone/adventure.md needs a non-empty '## {part}' section"
                ));
            }
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::package::tests::{tmp, write_pkg};
    use crate::scenario::package::ThreadSpec;

    #[test]
    fn lint_is_clean_for_a_complete_package() {
        let root = tmp("lint_ok");
        let p = Package::load(&write_pkg(&root)).unwrap();
        assert_eq!(p.lint(), Vec::<String>::new());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn lint_flags_bad_story_thread_ids() {
        let root = tmp("lint_threads");
        let mut p = Package::load(&write_pkg(&root)).unwrap();
        p.story.needs = vec!["Bad Id".into()];
        p.story.opens = vec![ThreadSpec {
            id: "ok-id".into(),
            text: "t".into(),
        }];
        p.story.closes = vec!["ok-id".into(), "also bad".into()];
        let problems = p.lint().join("\n");
        assert!(problems.contains("thread id 'Bad Id'"), "{problems}");
        assert!(problems.contains("thread id 'also bad'"), "{problems}");
        assert!(!problems.contains("thread id 'ok-id'"), "{problems}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn lint_flags_history_reads_missing_files_and_relative_setup() {
        let root = tmp("lint_bad");
        let d = write_pkg(&root);
        std::fs::write(d.join("tests/check.sh"), "grep rm ~/.bash_history\n").unwrap();
        std::fs::remove_file(d.join("solution/solve.sh")).unwrap();
        std::fs::write(d.join("tone/adventure.md"), "## open\nonly open\n").unwrap();
        let mut p = Package::load(&d).unwrap();
        p.environment.setup = Some("setup.sh".into());
        let problems = p.lint().join("\n");
        assert!(problems.contains("shell history"), "{problems}");
        assert!(problems.contains("missing solution/solve.sh"), "{problems}");
        assert!(problems.contains("'## close'"), "{problems}");
        assert!(problems.contains("absolute path"), "{problems}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn paths_cannot_leave_the_package() {
        for bad in [
            "../x",
            "a/../../x",
            "/etc/passwd",
            "",
            "a\\b",
            "C:\\x",
            "./",
        ] {
            assert!(!safe_rel(bad), "{bad}");
        }
        assert!(safe_rel("environment") && safe_rel("files/brief.md"));
    }

    #[test]
    fn lint_flags_escaping_paths_and_symlinks() {
        let root = tmp("lint_escape");
        let d = write_pkg(&root);
        let mut p = Package::load(&d).unwrap();
        p.check.script = "../x.sh".into();
        p.environment.context = "../../somewhere".into();
        let problems = p.lint().join("\n");
        assert!(
            problems.contains("check.script") && problems.contains("environment.context"),
            "{problems}"
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/passwd", d.join("tests/link")).unwrap();
            let p = Package::load(&d).unwrap();
            assert!(p.lint().join("\n").contains("not a regular file"));
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_word_that_merely_contains_history_is_not_flagged() {
        assert!(!reads_history("grep -c ok /task/price-history.csv\n"));
        assert!(reads_history("grep rm ~/.bash_history\n"));
        assert!(reads_history("history | grep rm\n"));
        assert!(reads_history("x=$(history)\n"));
        assert!(!reads_history("# history is not read here\n"));
    }

    #[test]
    fn an_id_a_shell_could_read_is_refused() {
        let root = tmp("lint_id");
        let mut p = Package::load(&write_pkg(&root)).unwrap();
        p.id = "x; rm -rf ~".into();
        assert!(p.lint().join("\n").contains("lowercase"));
        let _ = std::fs::remove_dir_all(root);
    }
}
