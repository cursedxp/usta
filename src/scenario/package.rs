//! Scenario package on disk: the Terminal-Bench / Harbor task shape, reduced
//! (`task.toml`, `instruction.md`, `environment/`, `solution/`, `tests/`),
//! plus Usta's `skills`, `story` and `tone/adventure.md`.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::scenario::env::{CONTAINER, CONVERSATION, FOLDER};

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
/// `check.timeout_secs` when the package does not say, and for a task with a workspace
/// (a cold build runs at every check, at 0.5 CPU).
pub(crate) const DEFAULT_CHECK_SECS: u32 = 30;
pub(crate) const WORKSPACE_CHECK_SECS: u32 = 300;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct EnvSpec {
    /// Which environment runs the scenario: "container", "folder" or "conversation".
    /// Checked at load.
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
    /// Container tasks only: absolute path INSIDE the container where the learner's
    /// task folder is mounted, so the learner edits real files on the host.
    #[serde(default)]
    pub workspace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct Criterion {
    pub id: String,
    pub text: String,
    /// Files the criterion rests on: starting files or deliverables.
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct CheckSpec {
    /// "script" or "rubric".
    pub kind: String,
    /// File name under `tests/`. Script checks only.
    #[serde(default)]
    pub script: String,
    /// 0 until `Package::load` fills in the default.
    #[serde(default)]
    pub timeout_secs: u32,
    /// Workspace tasks only: the check needs the running server, so it runs in it, in
    /// place. Otherwise it runs in a fresh server with the workspace read-only.
    #[serde(default)]
    pub live: bool,
    #[serde(default)]
    pub rubric: Vec<Criterion>,
    /// Files the learner must write in a folder task; what the judge reads.
    #[serde(default)]
    pub deliverables: Vec<String>,
    /// Why this task cannot have a script check. Required on generated rubric packages.
    pub why_rubric: Option<String>,
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
    pub fn is_rubric(&self) -> bool {
        self.check.kind == "rubric"
    }

    /// Container tasks, and folder tasks whose script check runs in a throwaway container.
    pub fn needs_runtime(&self) -> bool {
        self.environment.kind == CONTAINER || self.check.kind == "script"
    }

    pub fn load(dir: &Path) -> Result<Package> {
        let file = dir.join("task.toml");
        let text = std::fs::read_to_string(&file)
            .with_context(|| format!("cannot read {}", file.display()))?;
        let mut p: Package =
            toml::from_str(&text).with_context(|| format!("bad {}", file.display()))?;
        if ![CONTAINER, FOLDER, CONVERSATION].contains(&p.environment.kind.as_str()) {
            bail!(
                "{}: environment kind '{}' is not supported (container, folder or conversation)",
                file.display(),
                p.environment.kind
            );
        }
        if !["script", "rubric"].contains(&p.check.kind.as_str()) {
            bail!(
                "{}: check kind '{}' is not supported (script or rubric)",
                file.display(),
                p.check.kind
            );
        }
        if p.check.timeout_secs == 0 {
            p.check.timeout_secs = if p.environment.workspace.is_some() {
                WORKSPACE_CHECK_SECS
            } else {
                DEFAULT_CHECK_SECS
            };
        }
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
    fn load_rejects_unsupported_environment_and_check_kinds() {
        for (from, to, needle) in [
            ("kind = \"container\"", "kind = \"vm\"", "environment kind"),
            ("kind = \"script\"", "kind = \"manual\"", "check kind"),
        ] {
            let root = tmp("kinds");
            let d = write_pkg(&root);
            std::fs::write(d.join("task.toml"), TASK.replace(from, to)).unwrap();
            let err = format!("{:#}", Package::load(&d).unwrap_err());
            assert!(
                err.contains(needle) && err.contains("not supported"),
                "{err}"
            );
            let _ = std::fs::remove_dir_all(root);
        }
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
}
