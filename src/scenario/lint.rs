//! Static package checks, run when the library loads. A package that fails is
//! reported to the learner and left out, never run.

use std::path::{Path, PathBuf};

use crate::scenario::env::{CONTAINER, CONVERSATION, FOLDER};
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

const DATA_EXTENSIONS: &[&str] = &[
    "md", "txt", "csv", "tsv", "json", "html", "xml", "yaml", "yml", "toml",
];

fn has(dir: &Path, rel: &str) -> bool {
    dir.join(rel).is_file()
}

/// Data only: an allowed extension, and nothing a shell would treat as a program.
fn data_problems(root: &Path) -> Vec<String> {
    let Ok(files) = walk(root) else { return vec![] }; // `walk` on the package already reported it
    files
        .iter()
        .filter_map(|f| {
            let ext = f
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let shebang = std::fs::read(f).is_ok_and(|b| b.starts_with(b"#!"));
            (!DATA_EXTENSIONS.contains(&ext.as_str()) || shebang)
                .then(|| format!("{} must be a data file", f.display()))
        })
        .collect()
}

fn rubric_problems(p: &Package) -> Vec<String> {
    let mut out = Vec::new();
    if p.check.rubric.len() < 2 {
        out.push("a rubric needs at least 2 criteria".into());
    }
    let mut seen = Vec::new();
    for c in &p.check.rubric {
        if c.text.trim().is_empty() || c.id.trim().is_empty() || seen.contains(&&c.id) {
            out.push(format!("criterion '{}' needs a unique id and text", c.id));
        }
        seen.push(&c.id);
        for e in &c.evidence {
            let known = safe_rel(e)
                && (p.dir.join("environment/files").join(e).is_file()
                    || p.check.deliverables.contains(e));
            if !known {
                out.push(format!(
                    "criterion '{}' refers to '{e}', which is not in the task",
                    c.id
                ));
            }
        }
    }
    if p.is_generated()
        && p.check
            .why_rubric
            .as_deref()
            .is_none_or(|w| w.trim().is_empty())
    {
        out.push("check.why_rubric must say why this task cannot have a script check".into());
    }
    out
}

fn script_problems(p: &Package) -> Vec<String> {
    match std::fs::read_to_string(p.dir.join("tests").join(&p.check.script)) {
        Err(_) => vec![format!("missing tests/{}", p.check.script)],
        Ok(t) if reads_history(&t) => {
            vec!["check script reads shell history; check system state, not commands typed".into()]
        }
        Ok(_) => vec![],
    }
}

fn kind_problems(p: &Package) -> Vec<String> {
    let mut out = Vec::new();
    let (kind, rubric) = (p.environment.kind.as_str(), p.is_rubric());
    match kind {
        CONTAINER => {
            if rubric {
                out.push("container tasks use a script check".to_string());
            }
            if !has(&p.dir, "solution/solve.sh") {
                out.push("missing solution/solve.sh".into());
            }
        }
        FOLDER => {
            for sub in ["environment/files", "solution/files"] {
                if p.dir.join(sub).is_dir() {
                    out.extend(data_problems(&p.dir.join(sub)));
                } else {
                    out.push(format!("missing {sub}/"));
                }
            }
            if rubric && p.check.deliverables.is_empty() {
                out.push("a folder rubric needs check.deliverables".into());
            }
            for d in p.check.deliverables.iter().filter(|d| !safe_rel(d)) {
                out.push(format!(
                    "deliverable '{d}' must stay inside the task folder"
                ));
            }
        }
        CONVERSATION => {
            if !rubric {
                out.push("conversation tasks use a rubric".to_string());
            }
            if !has(&p.dir, "solution/answer.md") {
                out.push("missing solution/answer.md".into());
            }
        }
        _ => {}
    }
    out.extend(if rubric {
        rubric_problems(p)
    } else {
        script_problems(p)
    });
    out
}

const MAX_MEMORY_MB: u64 = 1024;
const MAX_CPUS: f32 = 2.0;
const MAX_PIDS: u32 = 512;
const MAX_CHECK_SECS: u32 = 120;

fn megabytes(s: &str) -> Option<u64> {
    let (n, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit())?);
    let n: u64 = n.parse().ok()?;
    match unit {
        "m" => Some(n),
        "g" => n.checked_mul(1024),
        _ => None,
    }
}

/// Values reach `docker` as arguments, so they must be plain values under the caps.
fn cap_problems(p: &Package) -> Vec<String> {
    let mut out = Vec::new();
    let e = &p.environment;
    if megabytes(&e.memory).is_none_or(|m| m == 0 || m > MAX_MEMORY_MB) {
        out.push(format!(
            "environment.memory '{}' must be at most 1g",
            e.memory
        ));
    }
    if e.cpus
        .parse::<f32>()
        .ok()
        .filter(|c| *c > 0.0 && *c <= MAX_CPUS)
        .is_none()
    {
        out.push(format!("environment.cpus '{}' must be at most 2", e.cpus));
    }
    if e.pids == 0 || e.pids > MAX_PIDS {
        out.push(format!(
            "environment.pids {} must be at most {MAX_PIDS}",
            e.pids
        ));
    }
    if p.check.timeout_secs == 0 || p.check.timeout_secs > MAX_CHECK_SECS {
        out.push(format!(
            "check.timeout_secs {} must be at most {MAX_CHECK_SECS}",
            p.check.timeout_secs
        ));
    }
    out
}

/// `allow` is the topic plus the learner's request: a Docker lesson may say Docker.
#[allow(dead_code)] // Task 11 calls it during generation
pub(crate) fn learner_word_problems(p: &Package, allow: &str) -> Vec<String> {
    let allow = allow.to_ascii_lowercase();
    let mut files = vec![
        p.dir.join("instruction.md"),
        p.dir.join("tone/adventure.md"),
    ];
    if !p.check.script.is_empty() {
        files.push(p.dir.join("tests").join(&p.check.script));
    }
    let mut out = Vec::new();
    // A Docker lesson may also say container.
    let docker = allow.contains("docker");
    for word in ["docker", "container"]
        .iter()
        .filter(|w| !(docker || allow.contains(**w)))
    {
        for f in &files {
            if std::fs::read_to_string(f).is_ok_and(|t| t.to_ascii_lowercase().contains(word)) {
                out.push(format!(
                    "{} must not say '{word}'; say task or server",
                    f.display()
                ));
            }
        }
    }
    out
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
        if !script_ok && !self.is_rubric() {
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
        for f in ["instruction.md", "tone/adventure.md"] {
            if !has(&self.dir, f) {
                p.push(format!("missing {f}"));
            }
        }
        if self.environment.kind == CONTAINER
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
        p.extend(kind_problems(self));
        if self.is_generated() {
            p.extend(cap_problems(self));
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
    use std::path::{Path, PathBuf};

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

    const RUBRIC_TOML: &str = r#"
id = "demo-pkg"
topic = "marketing"
title = "Demo"
version = 1
skills = ["positioning"]

[environment]
kind = "folder"

[check]
kind = "rubric"
deliverables = ["plan.md"]
why_rubric = "the quality of a positioning argument has no fixed answer"

[[check.rubric]]
id = "names-audience"
text = "Names one audience from brief.md"
evidence = ["brief.md"]

[[check.rubric]]
id = "gives-reason"
text = "Gives a reason drawn from the competitor file"
evidence = ["competitors.csv"]
"#;

    fn folder_pkg(root: &Path) -> PathBuf {
        let d = root.join("demo-pkg");
        for (f, c) in [
            ("task.toml", RUBRIC_TOML),
            ("instruction.md", "Write plan.md.\n"),
            ("tone/adventure.md", "## open\nGo.\n\n## close\nDone.\n"),
            ("environment/files/brief.md", "Audience: bakers.\n"),
            ("environment/files/competitors.csv", "name,price\na,1\n"),
            ("solution/files/plan.md", "Bakers, because a is cheap.\n"),
        ] {
            std::fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
            std::fs::write(d.join(f), c).unwrap();
        }
        d
    }

    #[test]
    fn a_complete_folder_rubric_package_is_clean() {
        let root = tmp("lint_folder");
        let p = Package::load(&folder_pkg(&root)).unwrap();
        assert_eq!(p.lint(), Vec::<String>::new());
        assert!(p.is_rubric() && !p.needs_runtime());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn only_the_four_allowed_pairs_pass() {
        let root = tmp("lint_pairs");
        let mut p = Package::load(&folder_pkg(&root)).unwrap();
        p.environment.kind = "container".into();
        assert!(p
            .lint()
            .join("\n")
            .contains("container tasks use a script check"));
        p.environment.kind = "conversation".into();
        p.check.kind = "script".into();
        assert!(p
            .lint()
            .join("\n")
            .contains("conversation tasks use a rubric"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_folder_holds_data_files_only() {
        let root = tmp("lint_data");
        let d = folder_pkg(&root);
        std::fs::write(d.join("environment/files/run.sh"), "#!/bin/sh\nrm -rf ~\n").unwrap();
        std::fs::write(d.join("solution/files/note.txt"), "#!/usr/bin/env python\n").unwrap();
        let problems = Package::load(&d).unwrap().lint().join("\n");
        assert!(
            problems.contains("run.sh") && problems.contains("note.txt"),
            "{problems}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_rubric_must_point_at_material_that_exists() {
        let root = tmp("lint_evidence");
        let d = folder_pkg(&root);
        std::fs::remove_file(d.join("environment/files/competitors.csv")).unwrap();
        assert!(Package::load(&d)
            .unwrap()
            .lint()
            .join("\n")
            .contains("competitors.csv"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn generated_packages_stay_under_the_caps() {
        let root = tmp("lint_caps");
        let mut p = Package::load(&write_pkg(&root)).unwrap();
        p.id = "gen-linux-1".into();
        p.environment.memory = "64g".into();
        p.environment.cpus = "--privileged".into();
        p.environment.pids = 100000;
        p.check.timeout_secs = 3600;
        let problems = p.lint().join("\n");
        for k in [
            "environment.memory",
            "environment.cpus",
            "environment.pids",
            "check.timeout_secs",
        ] {
            assert!(problems.contains(k), "{k}: {problems}");
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn learner_text_may_not_name_the_machinery_unless_the_subject_does() {
        let root = tmp("lint_words");
        let d = write_pkg(&root);
        std::fs::write(d.join("instruction.md"), "The Docker container is down.\n").unwrap();
        let p = Package::load(&d).unwrap();
        assert!(!learner_word_problems(&p, "marketing").is_empty());
        assert!(learner_word_problems(&p, "docker-basics").is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
