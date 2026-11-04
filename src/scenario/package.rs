//! Scenario package on disk: the Terminal-Bench / Harbor task shape, reduced
//! (`task.toml`, `instruction.md`, `environment/`, `solution/`, `tests/`),
//! plus Usta's `skills`, `story` and `tone/adventure.md`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// Presentation layer over the core. Task, check and difficulty never change with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    Plain,
    Adventure,
}

impl Tone {
    pub fn parse(s: &str) -> Option<Tone> {
        match s.trim().to_ascii_lowercase().as_str() {
            "plain" => Some(Tone::Plain),
            "adventure" => Some(Tone::Adventure),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Tone::Plain => "plain",
            Tone::Adventure => "adventure",
        }
    }
}

fn d_context() -> String {
    "environment".into()
}
fn d_memory() -> String {
    "256m".into()
}
fn d_cpus() -> String {
    "0.5".into()
}
fn d_pids() -> u32 {
    128
}
fn d_timeout() -> u32 {
    30
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct EnvSpec {
    /// Which environment runs the scenario. Step 1 knows only "container".
    pub kind: String,
    /// Directory (relative to the package) used as the image build context.
    #[serde(default = "d_context")]
    pub context: String,
    /// Absolute path INSIDE the container, run as root once at prepare.
    pub setup: Option<String>,
    /// Absolute path INSIDE the container, run at prepare and at every resume.
    pub start: Option<String>,
    #[serde(default)]
    pub network: bool,
    #[serde(default = "d_memory")]
    pub memory: String,
    #[serde(default = "d_cpus")]
    pub cpus: String,
    #[serde(default = "d_pids")]
    pub pids: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CheckSpec {
    pub kind: String,
    /// File name under the package's `tests/` directory.
    pub script: String,
    #[serde(default = "d_timeout")]
    pub timeout_secs: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct Character {
    pub name: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct ThreadSpec {
    pub id: String,
    pub text: String,
}

/// Tone-independent facts the scenario adds to, and needs from, the world ledger.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct StorySpec {
    /// Set on the first finished scenario; later ones do not overwrite it.
    pub company: Option<String>,
    #[serde(default)]
    pub characters: Vec<Character>,
    /// One sentence recorded in the ledger when the scenario finishes.
    pub event: Option<String>,
    /// Threads this scenario opens when it finishes.
    #[serde(default)]
    pub opens: Vec<ThreadSpec>,
    /// Thread ids this scenario closes when it finishes.
    #[serde(default)]
    pub closes: Vec<String>,
    /// Thread ids that must be open before this scenario is offered.
    #[serde(default)]
    pub needs: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Package {
    pub id: String,
    pub topic: String,
    pub title: String,
    pub version: u32,
    /// Lowercase keywords matched as substrings against curriculum-map lines.
    pub skills: Vec<String>,
    pub environment: EnvSpec,
    pub check: CheckSpec,
    #[serde(default)]
    pub story: StorySpec,
    #[serde(skip)]
    pub dir: PathBuf,
}

/// Body of the `## <name>` section (until the next `## `), trimmed; None if absent or empty.
pub(crate) fn section(md: &str, name: &str) -> Option<String> {
    let mut out: Vec<&str> = Vec::new();
    let mut inside = false;
    for line in md.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            if inside {
                break;
            }
            inside = h.trim().eq_ignore_ascii_case(name);
            continue;
        }
        if inside {
            out.push(line);
        }
    }
    let body = out.join("\n").trim().to_string();
    (!body.is_empty()).then_some(body)
}

impl Package {
    pub fn load(dir: &Path) -> Result<Package> {
        let file = dir.join("task.toml");
        let text = std::fs::read_to_string(&file)
            .with_context(|| format!("cannot read {}", file.display()))?;
        let mut p: Package =
            toml::from_str(&text).with_context(|| format!("bad {}", file.display()))?;
        p.dir = dir.to_path_buf();
        Ok(p)
    }

    /// The plain task statement (`instruction.md`).
    pub fn instruction(&self) -> Result<String> {
        let f = self.dir.join("instruction.md");
        Ok(std::fs::read_to_string(&f)
            .with_context(|| format!("cannot read {}", f.display()))?
            .trim()
            .to_string())
    }

    /// Story text for `part` = "open" | "close" in the given tone. Spoken only at task open and close.
    pub fn tone_text(&self, tone: Tone, part: &str) -> Result<String> {
        match tone {
            Tone::Plain if part == "open" => self.instruction(),
            Tone::Plain => Ok(format!("Done: {}.", self.title)),
            Tone::Adventure => {
                let f = self.dir.join("tone/adventure.md");
                let md = std::fs::read_to_string(&f)
                    .with_context(|| format!("cannot read {}", f.display()))?;
                section(&md, part)
                    .with_context(|| format!("{} has no '## {part}' section", f.display()))
            }
        }
    }

    /// Static package problems (empty = fine). The model-driven validation
    /// (fresh check fails, reference solution passes) is step 2 plus the
    /// `#[ignore]`d Docker certification in `docker_tests.rs`.
    pub fn lint(&self) -> Vec<String> {
        let mut p = Vec::new();
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
        match std::fs::read_to_string(self.dir.join("tests").join(&self.check.script)) {
            Err(_) => p.push(format!("missing tests/{}", self.check.script)),
            Ok(t) => {
                let l = t.to_ascii_lowercase();
                if l.contains("history") || l.contains("histfile") {
                    p.push(
                        "check script reads shell history; check system state, not commands typed"
                            .into(),
                    );
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
pub(crate) mod tests {
    use super::*;

    pub(crate) fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("usta_scn_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const TASK: &str = r#"
id = "demo-pkg"
topic = "linux"
title = "Demo"
version = 1
skills = ["permission"]

[environment]
kind = "container"
setup = "/usr/local/lib/usta/setup.sh"

[check]
kind = "script"
script = "check.sh"
"#;

    /// A complete, lint-clean package under `<root>/demo-pkg`.
    pub(crate) fn write_pkg(root: &Path) -> PathBuf {
        let d = root.join("demo-pkg");
        for f in [
            "task.toml",
            "instruction.md",
            "environment/Dockerfile",
            "solution/solve.sh",
            "tests/check.sh",
            "tone/adventure.md",
        ] {
            std::fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
        }
        std::fs::write(d.join("task.toml"), TASK).unwrap();
        std::fs::write(d.join("instruction.md"), "Fix it.\n").unwrap();
        std::fs::write(d.join("environment/Dockerfile"), "FROM debian:12-slim\n").unwrap();
        std::fs::write(d.join("solution/solve.sh"), "true\n").unwrap();
        std::fs::write(d.join("tests/check.sh"), "exit 0\n").unwrap();
        std::fs::write(
            d.join("tone/adventure.md"),
            "## open\n03:12. It broke.\n\n## close\nFixed.\n",
        )
        .unwrap();
        d
    }

    #[test]
    fn load_applies_defaults() {
        let root = tmp("load");
        let p = Package::load(&write_pkg(&root)).unwrap();
        assert_eq!(p.id, "demo-pkg");
        assert_eq!(p.environment.context, "environment");
        assert_eq!(p.environment.memory, "256m");
        assert_eq!(p.environment.cpus, "0.5");
        assert_eq!(p.environment.pids, 128);
        assert!(!p.environment.network);
        assert_eq!(p.check.timeout_secs, 30);
        assert!(p.story.event.is_none() && p.story.needs.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn story_fields_parse() {
        let toml_text = format!(
            "{TASK}\n[story]\ncompany = \"Shopwell\"\nevent = \"Backup repaired.\"\nneeds = [\"a-thread\"]\ncloses = [\"old-thread\"]\n\n[[story.characters]]\nname = \"Deniz\"\nrole = \"runs the servers\"\n\n[[story.opens]]\nid = \"backups-ok\"\ntext = \"Backups run again.\"\n"
        );
        let p: Package = toml::from_str(&toml_text).unwrap();
        assert_eq!(p.story.company.as_deref(), Some("Shopwell"));
        assert_eq!(p.story.characters[0].name, "Deniz");
        assert_eq!(p.story.opens[0].id, "backups-ok");
        assert_eq!(p.story.needs, vec!["a-thread".to_string()]);
        assert_eq!(p.story.closes, vec!["old-thread".to_string()]);
    }

    #[test]
    fn section_extracts_between_headers() {
        let md = "intro\n## open\nline a\nline b\n\n## close\nbye\n";
        assert_eq!(section(md, "open").unwrap(), "line a\nline b");
        assert_eq!(section(md, "CLOSE").unwrap(), "bye");
        assert!(section(md, "missing").is_none());
        assert!(section("## open\n\n## close\nx", "open").is_none());
    }

    #[test]
    fn tone_text_plain_open_is_instruction_adventure_reads_sections() {
        let root = tmp("tone");
        let p = Package::load(&write_pkg(&root)).unwrap();
        assert_eq!(p.tone_text(Tone::Plain, "open").unwrap(), "Fix it.");
        assert_eq!(p.tone_text(Tone::Plain, "close").unwrap(), "Done: Demo.");
        assert_eq!(
            p.tone_text(Tone::Adventure, "open").unwrap(),
            "03:12. It broke."
        );
        assert_eq!(p.tone_text(Tone::Adventure, "close").unwrap(), "Fixed.");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tone_parse_roundtrip() {
        assert_eq!(Tone::parse(" Adventure "), Some(Tone::Adventure));
        assert_eq!(Tone::parse("plain"), Some(Tone::Plain));
        assert_eq!(Tone::parse("loud"), None);
        assert_eq!(Tone::Adventure.as_str(), "adventure");
    }

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
}
