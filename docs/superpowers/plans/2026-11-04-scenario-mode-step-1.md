# Scenario Mode, Build Step 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give Usta a scenario engine, a five-operation environment contract, a container environment with its full lifecycle, an on-disk scenario package format, a minimal world ledger, and three handwritten Linux scenarios (plain and adventure tone, one continuing story) that a learner starts, resumes, finishes or drops with `/scenario` from inside a session, without ever seeing or typing a Docker command.

**Architecture:** A new `src/scenario/` module tree. The engine (`engine.rs`) only talks to the `Environment` trait (`prepare`/`pause`/`resume`/`check`/`clean`); `container.rs` implements it over the `docker` CLI through a small `Docker` trait so every command line is unit-tested with a fake and no Docker is needed in CI. Run records live in `<project>/.usta/scenarios/runs/*.toml` (shell-owned, never rewritten by the model). Built-in packages are embedded in the binary, synced to `~/.config/usta/scenarios/library/`, and loaded from there. `/scenario ...` lines are intercepted in both session loops (TUI and plain) like `/game`: notices are shell-only, and one synthesized `[SCENARIO ...]` turn per start/resume/pass goes through the normal ask flow so the mentor and the closing flush know about it. The learner does no setup work: Usta starts the runtime if it is installed but stopped, and opens a new terminal window already inside the task (platform seam with a fake launcher in tests; the command is shown only if no window can be opened). A shell-owned world ledger per topic (company, characters, past events, open threads) is updated when a scenario finishes and read when the next one opens; packages declare what they add and need in their `story` field.

**Tech Stack:** Rust 2021 (rust-version 1.88), `serde` (already present), `toml` (new, see Open questions 1), `serde_json` (already present), the `docker` CLI. No new async code: Docker calls are blocking and run through `tokio::task::spawn_blocking`.

---

## Decisions taken and stated constraints

Nothing here blocks execution. These were open questions in the first draft of this plan; the coordinator resolved them.

**Resolved (built as the plan describes):**
- `toml = "0.8"` is added for `task.toml`, run records and the ledger.
- Adventure tone is handwritten per package in step 1 (`tone/adventure.md`); plain tone is `instruction.md`.
- A running scenario of another Usta project makes start/resume refuse with a plain message; Usta never stops another project's task.
- Provisional numbers: stale-task question after 14 days (`sweep::STALE_DAYS`); runtime start wait of 90 seconds (`docker::START_TIMEOUT`); no idle-timeout stop in step 1 (tasks stop when Usta closes, and the startup sweep stops crash survivors).
- The learner never types a Docker command: Usta opens a terminal window inside the task (Task 8). Spec section "No work for the learner".

**Stated constraints (carried, not open):**
1. **`skills` are keywords, not map-item ids.** The curriculum map is written by the model (`progress::closing_prompt`) and its items are free text; the shell only reads them via `tokens::map_state_of`. A skill with no matching line counts as not settled, so a task is offered, never wrongly skipped.
2. **`due:` does not exist on map items.** In the code `due:` tails live only on Recall questions in the progress file. Step 1 builds skip and variety only; due-return needs a later decision (derive from recall questions, or add `due:` to map items).
3. **The shell never writes the curriculum map.** A passing check injects a `[SCENARIO DONE]` turn and the closing flush records the evidence, so "skip finished skills" works only after that flush ran. The ledger is different: it is shell-owned and the shell writes it.
4. **runc fixed versions are unverified.** `docker::runc_is_old` encodes "fixed in runc 1.2.8, 1.3.3, 1.4.0-rc.3" from memory of the November 2025 advisories. Task 6 Step 0 blocks until the three CVE advisories are checked; if they cannot be verified, ship the warning off (return `None`).
5. **Not built in step 1:** project-folder mount (the three Linux scenarios need none), generating/validating with the model (step 2), other environments, the embedded terminal pane inside Usta (a later step in the spec), removing the Docker dependency, installing Docker for the learner (step 3's tool setup; here only guidance).
6. **Unverified on real systems (checked in Task 14's manual step):** the macOS `.command` window method, the Linux terminal launchers, the Windows `cmd /c start` launcher and Docker Desktop's install path, and how each platform starts a stopped runtime. Each has a fallback that shows the command or a plain message.
7. **Docker is named in exactly three kinds of learner text,** because the learner cannot act without the name: the install guidance, the "could not start / did not start" messages, and the fallback command when no window can open. Everything else says task or server. A test enforces this for every other string (Task 12).
8. **macOS permission prompt:** the chosen method (write a `.command` file and `open` it) uses LaunchServices, not Apple Events, so it should not trigger the Automation prompt ("Usta wants to control Terminal") that `osascript` would. This is expected, not verified; if a prompt appears or the window is blocked, the fallback command is shown.

## Ground truth from the code (what this plan integrates with)

- **Slash commands** are intercepted per line in two loops: TUI `src/tui/run.rs` (`Action::Submit(line)` arm, lines ~268-345) and plain `src/plain.rs` (`InputEvent::Line`, lines ~265-345). Pure parsers live in `src/slash.rs` (`parse_game_command`, `GameCmd`). `/game on|off` and `/exam` set the pattern this plan copies: a notice via `page_notice`/`ui::notice`, then an `outgoing` turn text swapped in and sent through the normal ask flow. Before a topic exists, `topic_flow.rs:168`, `intro.rs:220`, `plain.rs:88` answer "that command works inside a session"; `/scenario` joins those conditions.
- **Sessions:** `lifecycle::build_session(global, project_root, topic, today)`; topic lock via `lock::LockGuard`; `lock::pid_alive(pid)` is already `pub(crate)`; `lifecycle::today()` returns `YYYY-MM-DD`.
- **Curriculum map:** `progress::curriculum_path(project_root, topic)` = `<project>/.usta/learner/curriculum/<topic>.md`; line state via `tokens::map_state_of(line) -> Option<&'static str>` (`tokens::STATE_SETTLED`, `STATE_DEEPENED`). Written only by the model (`flush_core`).
- **Progress files** are model-written markdown, atomic via `progress::write_atomic` (which makes a `.md.bak`; wrong for `.toml`, so the run store has its own tmp+rename write).
- **Game mode:** `GameCmd`, `game_pref` (USER.md `- gamification:` line), `restore_game_pref` in `lifecycle.rs:196`. The closing flush rewrites USER.md, which is why tone is **not** stored there (a model rewrite would need its own restore guarantee). Tone lives in `.usta/scenarios/tone`, a shell-owned file.
- **Global defaults** (`defaults.rs`) are flat `(path, include_str!, Ownership)` triples synced at startup via `config::needs_sync`. Packages have ~8 files each, so the scenario library uses its own embedded list and syncs lazily on first `/scenario` use (no startup cost, no `defaults.rs` growth).
- **Watcher** ignores hidden directories (`watcher::is_ignored`), so `.usta/scenarios/` writes never trigger feedback turns.
- **Subprocess style:** `std::process::Command` is used for blocking tools elsewhere; no `tokio::process` needed.
- **Tests:** unit tests live in `#[cfg(test)] mod tests` inside the file; there is no `tests/` directory (and AGENTS.md reserves it). Docker-dependent tests use `#[ignore]` and run with `cargo test -- --ignored`.

## Line budget (SPEC.md §11: 600 production lines per file, test module excluded)

Production lines measured with `awk '/#\[cfg\(test\)\]/{exit} {n++} END{print n}' <file>`:

| Existing file | Prod lines now | This plan adds | After | Verdict |
|---|---|---|---|---|
| `src/plain.rs` | 503 | ~22 | ~525 | OK |
| `src/tui/run.rs` | 422 | ~26 | ~448 | OK |
| `src/main.rs` | 295 | ~4 | ~299 | OK |
| `src/help.rs` | 43 | ~2 | ~45 | OK |
| `src/tui/topic_flow.rs`, `src/tui/intro.rs` | n/a | 1 line each | n/a | OK |

Files near the limit that this plan **does not touch**: `src/setup.rs` (560), `src/backend.rs` (560), `src/progress.rs` (547), `src/file_feedback.rs` (539). New files are each planned well under 600 (largest: `command.rs` ~430, `container.rs` ~260, `ledger.rs` ~170). Task 14 re-measures everything.

## Global Constraints

- 600-line production budget per file; test modules do not count (spec/SPEC.md §11, CLAUDE.md §3).
- Surgical changes in existing files; `cargo fmt` scoped to files touched; `cargo fmt --check` stays crate-wide clean.
- Container rules (spec "Container lifecycle"): no `--rm`; one task one container; `--memory`, `--cpus`, `--pids-limit` on every container; network off unless the package declares it; names prefixed `usta-` and labelled `usta=1`; Usta touches only containers carrying its label **and** this project's tag; never remove an unfinished task without asking.
- Check rules (spec "Check scripts"): check system state, never commands typed; accept every valid solution; check scripts stay outside anything the learner can write and enter the container only at check time.
- Safety: a container is protection against mistakes, never presented as a defence against malicious code.
- Learner-facing text never says container, image or Docker; it says task or server (exceptions: install guidance, runtime-start failures, and the fallback command). Tool output is never shown raw: internal errors go to `.usta/scenarios/last-error.log`.
- The learner does no setup: no Docker command to type, runtime started by Usta, a terminal window opened inside the task.
- Continuity: the ledger is shell-owned TOML under `.usta/scenarios/ledger/<topic>.toml`; the model never writes it.
- Do not touch Docker containers named `kiln-*`. The label filter (`usta=1`) guarantees it; no code path may use `docker rm`/`stop` on a name that is not `usta-`-prefixed.
- All learner-visible strings are English (project rule: technology text in English).
- Version bumps to `0.40.0` in `Cargo.toml` and the version-pinning test in `src/tui/welcome_tests.rs`.

## File structure

```
Cargo.toml                                   modify  (+toml, version 0.40.0)
scenarios/<id>/...                           create  (3 handwritten packages, embedded in the binary)
src/main.rs                                  modify  (mod scenario; startup sweep call)
src/help.rs                                  modify  (+/scenario lines)
src/plain.rs                                 modify  (Slot + /scenario branch + pre-topic pointer)
src/tui/run.rs                               modify  (Slot + /scenario branch)
src/tui/topic_flow.rs, src/tui/intro.rs      modify  (pre-topic pointer condition)
src/tui/welcome_tests.rs                     modify  (version pin)
SPEC.md, README.md                           modify  (section 4.24, command list)
src/scenario/mod.rs                          create  module root
src/scenario/package.rs                      create  Package, Tone, task.toml loader, lint
src/scenario/env.rs                          create  Environment trait + EnvHandle + CheckOutcome
src/scenario/store.rs                        create  RunRecord, RunState, Store, project_tag
src/scenario/engine.rs                       create  start/resume/pause/check/finish
src/scenario/select.rs                       create  skill matching, skip, story prerequisites, choose
src/scenario/ledger.rs                       create  world ledger (company, characters, events, threads)
src/scenario/terminal.rs                     create  terminal-window launcher seam
src/scenario/docker.rs                       create  Docker trait, runtime start, runc check
src/scenario/container.rs                    create  ContainerEnv (the five operations)
src/scenario/library.rs                      create  embedded packages, sync, load_all
src/scenario/sweep.rs                        create  startup cleanup plan + executor
src/scenario/command.rs                      create  /scenario parser, execute, turns, Slot
src/scenario/docker_tests.rs                 create  #[ignore]d real-Docker certification
```

---

### Task 1: Dependency, module skeleton, package format

**Files:**
- Modify: `Cargo.toml` (dependencies block)
- Modify: `src/main.rs:5` (module list)
- Create: `src/scenario/mod.rs`
- Create: `src/scenario/package.rs`

**Interfaces:**
- Produces:
  - `pub(crate) enum Tone { Plain, Adventure }` with `Tone::parse(&str) -> Option<Tone>`, `Tone::as_str(self) -> &'static str`
  - `pub(crate) struct Package { id, topic, title: String, version: u32, skills: Vec<String>, environment: EnvSpec, check: CheckSpec, story: StorySpec, dir: PathBuf }`
  - `pub(crate) struct EnvSpec { kind: String, context: String, setup: Option<String>, start: Option<String>, network: bool, memory: String, cpus: String, pids: u32 }`
  - `pub(crate) struct CheckSpec { kind: String, script: String, timeout_secs: u32 }`
  - `pub(crate) struct StorySpec { company: Option<String>, characters: Vec<Character>, event: Option<String>, opens: Vec<ThreadSpec>, closes: Vec<String>, needs: Vec<String> }` (`Default`), `pub(crate) struct Character { name, role: String }`, `pub(crate) struct ThreadSpec { id, text: String }`
  - `Package::load(dir: &Path) -> Result<Package>`, `Package::instruction(&self) -> Result<String>`, `Package::tone_text(&self, Tone, part: &str) -> Result<String>`, `Package::lint(&self) -> Vec<String>`
  - `pub(crate) fn section(md: &str, name: &str) -> Option<String>`

- [ ] **Step 1: Add the dependency and the module**

In `Cargo.toml`, after the `base64 = "0.22"` line add:

```toml
toml = "0.8"
```

In `src/main.rs`, add `mod scenario;` between `mod reset;` and `mod session;` (alphabetical).

Create `src/scenario/mod.rs`:

```rust
//! Scenario mode: Usta brings the work. A scenario is a package (role,
//! situation, task, check) run in a pluggable environment; the engine knows
//! only the five-operation `Environment` contract. Spec:
//! docs/superpowers/specs/2026-11-04-scenario-mode-design.md
#![allow(dead_code)] // removed in Task 13, once the commands are wired

pub(crate) mod package;
```

- [ ] **Step 2: Write the failing tests**

Create `src/scenario/package.rs` containing only the tests first:

```rust
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
        p.story.opens = vec![ThreadSpec { id: "ok-id".into(), text: "t".into() }];
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
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test scenario::package 2>&1 | tail -5`
Expected: FAIL to compile (`Package`, `Tone`, `section` not found).

- [ ] **Step 4: Write the implementation**

Prepend to `src/scenario/package.rs` (above the `#[cfg(test)]` block):

```rust
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
            p.push(format!("id '{}' does not match directory '{dir_name}'", self.id));
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
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test scenario::package 2>&1 | tail -5`
Expected: `test result: ok. 5 passed`

- [ ] **Step 6: Format and commit**

```bash
rustfmt --edition 2021 src/scenario/package.rs src/scenario/mod.rs
git add Cargo.toml Cargo.lock src/main.rs src/scenario/mod.rs src/scenario/package.rs
git commit -m "feat(scenario): package format, tone, lint"
```

---

### Task 2: Environment contract and run store

**Files:**
- Create: `src/scenario/env.rs`
- Create: `src/scenario/store.rs`
- Modify: `src/scenario/mod.rs` (add `pub(crate) mod env;` and `pub(crate) mod store;`)

**Interfaces:**
- Consumes: `Package`, `Tone` (Task 1)
- Produces:
  - `pub(crate) struct EnvHandle { pub reference: String, pub access: String }` (`access` = command line that opens a shell inside the task; the terminal launcher runs it)
  - `pub(crate) struct Internal(pub String)` and `pub(crate) fn internal(msg: impl Into<String>) -> anyhow::Error`: an error whose text is for the log, never for the learner (tool output, exit codes)
  - `pub(crate) enum CheckOutcome { Pass, Fail { output: String } }`
  - `pub(crate) trait Environment { fn prepare(&mut self, pkg: &Package, run_id: &str) -> Result<EnvHandle>; fn pause(&mut self, h: &EnvHandle) -> Result<()>; fn resume(&mut self, pkg: &Package, h: &EnvHandle) -> Result<()>; fn check(&mut self, pkg: &Package, h: &EnvHandle) -> Result<CheckOutcome>; fn clean(&mut self, h: &EnvHandle) -> Result<()>; }`
  - `pub(crate) enum RunState { Preparing, Active, Paused, Finished, Dropped }` (`Dropped` = the learner abandoned it; the record stays as history)
  - `pub(crate) struct RunRecord { run_id, package, topic, tone: String, state: RunState, owner_pid: u32, started, updated, reference, access: String, last_check: Option<String> }`; `impl RunRecord { pub fn handle(&self) -> EnvHandle }`
  - `#[derive(Clone)] pub(crate) struct Store`; `Store::new(project_root: &Path) -> Store`, `exists(&self) -> bool`, `save(&self, &RunRecord) -> Result<()>`, `load(&self, &str) -> Result<RunRecord>`, `remove(&self, &str) -> Result<()>`, `all(&self) -> Vec<RunRecord>`, `tone(&self) -> Tone`, `set_tone(&self, Tone) -> Result<()>`
  - `pub(crate) fn project_tag(project_root: &Path) -> String` (8 hex chars)
  - `pub(crate) fn write_atomic(path: &Path, text: &str) -> Result<()>` (tmp file + rename; also used by the ledger)

- [ ] **Step 1: Write the failing tests (store)**

Create `src/scenario/store.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::package::tests::tmp;

    fn rec(id: &str, state: RunState, started: &str) -> RunRecord {
        RunRecord {
            run_id: id.into(),
            package: "demo-pkg".into(),
            topic: "linux".into(),
            tone: "plain".into(),
            state,
            owner_pid: 0,
            started: started.into(),
            updated: started.into(),
            reference: "usta-x-demo".into(),
            access: "docker exec -it usta-x-demo bash".into(),
            last_check: None,
        }
    }

    #[test]
    fn save_load_roundtrip_and_none_is_omitted() {
        let root = tmp("store_rt");
        let s = Store::new(&root);
        assert!(!s.exists());
        let mut r = rec("demo-1", RunState::Active, "2026-11-04");
        s.save(&r).unwrap();
        assert!(s.exists());
        assert_eq!(s.load("demo-1").unwrap(), r);
        r.last_check = Some("fail".into());
        r.state = RunState::Paused;
        s.save(&r).unwrap();
        assert_eq!(s.load("demo-1").unwrap(), r);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn all_sorts_by_started_and_skips_garbage_and_tmp_files() {
        let root = tmp("store_all");
        let s = Store::new(&root);
        s.save(&rec("b", RunState::Paused, "2026-11-05")).unwrap();
        s.save(&rec("a", RunState::Finished, "2026-11-03")).unwrap();
        let runs = root.join(".usta/scenarios/runs");
        std::fs::write(runs.join("junk.toml"), "not = [valid").unwrap();
        std::fs::write(runs.join("half.toml.tmp"), "x").unwrap();
        let ids: Vec<String> = s.all().into_iter().map(|r| r.run_id).collect();
        assert_eq!(ids, vec!["a", "b"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn dropped_state_roundtrips() {
        let root = tmp("store_dropped");
        let s = Store::new(&root);
        let r = rec("d", RunState::Dropped, "2026-11-04");
        s.save(&r).unwrap();
        assert_eq!(s.load("d").unwrap().state, RunState::Dropped);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn remove_is_idempotent() {
        let root = tmp("store_rm");
        let s = Store::new(&root);
        s.save(&rec("a", RunState::Paused, "2026-11-03")).unwrap();
        s.remove("a").unwrap();
        s.remove("a").unwrap();
        assert!(s.all().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tone_defaults_to_plain_and_persists() {
        let root = tmp("store_tone");
        let s = Store::new(&root);
        assert_eq!(s.tone(), Tone::Plain);
        s.set_tone(Tone::Adventure).unwrap();
        assert_eq!(s.tone(), Tone::Adventure);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn project_tag_is_stable_eight_hex_and_differs_per_path() {
        let a = project_tag(Path::new("/work/a"));
        assert_eq!(a, project_tag(Path::new("/work/a")));
        assert_eq!(a.len(), 8);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, project_tag(Path::new("/work/b")));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test scenario::store 2>&1 | tail -5` (after adding the two `mod` lines)
Expected: FAIL to compile (`Store` not found).

- [ ] **Step 3: Write `env.rs`**

```rust
//! The environment contract. The scenario engine never knows which environment
//! it runs in; a new environment (browser, VM, account) is a new implementation
//! of these five operations and the engine does not change.

use anyhow::Result;

use crate::scenario::package::Package;

/// What `prepare` returns and every later operation receives. Opaque to the engine.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EnvHandle {
    /// Environment-specific reference (container name, folder path, ...).
    pub reference: String,
    /// Command line that opens a shell inside the task. The terminal launcher
    /// runs it; it is shown to the learner only if no window can be opened.
    pub access: String,
}

/// An error whose text is meant for the log, not the learner (tool output,
/// exit codes). The command layer logs it and shows a plain sentence instead.
#[derive(Debug)]
pub(crate) struct Internal(pub String);

impl std::fmt::Display for Internal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Internal {}

pub(crate) fn internal(msg: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(Internal(msg.into()))
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CheckOutcome {
    Pass,
    /// `output` is shown to the learner, so check scripts must only print lines safe to show.
    Fail { output: String },
}

pub(crate) trait Environment {
    /// Build the starting state. On error nothing may be left behind.
    fn prepare(&mut self, pkg: &Package, run_id: &str) -> Result<EnvHandle>;
    /// Release resources, keep the work.
    fn pause(&mut self, h: &EnvHandle) -> Result<()>;
    /// Return to the paused state.
    fn resume(&mut self, pkg: &Package, h: &EnvHandle) -> Result<()>;
    /// Decide whether the task is done. Must not change what the learner built.
    fn check(&mut self, pkg: &Package, h: &EnvHandle) -> Result<CheckOutcome>;
    /// Remove what is left after a finished task. Missing is success.
    fn clean(&mut self, h: &EnvHandle) -> Result<()>;
}
```

- [ ] **Step 4: Write `store.rs` implementation** (prepend above the test block)

```rust
//! Run records: one TOML file per scenario run under
//! `<project>/.usta/scenarios/runs/`, plus the shell-owned `tone` file.
//! Shell-owned on purpose: the closing flush rewrites USER.md and the map,
//! never these files.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::scenario::env::EnvHandle;
use crate::scenario::package::Tone;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RunState {
    /// Record written before any resource exists; a dead owner here means an interrupted prepare.
    Preparing,
    Active,
    Paused,
    Finished,
    /// The learner abandoned the task; the environment was cleaned, the record stays.
    Dropped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct RunRecord {
    pub run_id: String,
    pub package: String,
    pub topic: String,
    pub tone: String,
    pub state: RunState,
    /// Pid of the Usta process running it; 0 when nobody does.
    pub owner_pid: u32,
    pub started: String,
    pub updated: String,
    #[serde(default)]
    pub reference: String,
    #[serde(default)]
    pub access: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_check: Option<String>,
}

impl RunRecord {
    pub fn handle(&self) -> EnvHandle {
        EnvHandle {
            reference: self.reference.clone(),
            access: self.access.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(project_root: &Path) -> Store {
        Store {
            root: project_root.join(".usta/scenarios"),
        }
    }

    /// True once any run was ever recorded in this project (gates all Docker work at startup).
    pub fn exists(&self) -> bool {
        self.root.is_dir()
    }

    fn run_path(&self, id: &str) -> PathBuf {
        self.root.join("runs").join(format!("{id}.toml"))
    }

    pub fn save(&self, rec: &RunRecord) -> Result<()> {
        write_atomic(&self.run_path(&rec.run_id), &toml::to_string(rec)?)
    }

    pub fn load(&self, id: &str) -> Result<RunRecord> {
        let path = self.run_path(id);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("no such run: {id}"))?;
        toml::from_str(&text).with_context(|| format!("bad run record {}", path.display()))
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        match std::fs::remove_file(self.run_path(id)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    /// Every readable record, oldest first. Unreadable files are skipped, never fatal.
    pub fn all(&self) -> Vec<RunRecord> {
        let Ok(dir) = std::fs::read_dir(self.root.join("runs")) else {
            return Vec::new();
        };
        let mut recs: Vec<RunRecord> = dir
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("toml"))
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .filter_map(|t| toml::from_str(&t).ok())
            .collect();
        recs.sort_by(|a, b| (&a.started, &a.run_id).cmp(&(&b.started, &b.run_id)));
        recs
    }

    /// Remembered tone for the next scenario. Plain unless the learner chose otherwise.
    pub fn tone(&self) -> Tone {
        std::fs::read_to_string(self.root.join("tone"))
            .ok()
            .and_then(|s| Tone::parse(&s))
            .unwrap_or(Tone::Plain)
    }

    pub fn set_tone(&self, t: Tone) -> Result<()> {
        std::fs::create_dir_all(&self.root)?;
        std::fs::write(self.root.join("tone"), t.as_str())?;
        Ok(())
    }
}

/// Stable 8-hex tag of the project root (FNV-1a). Labels every container so
/// one project's cleanup can never touch another project's containers.
pub(crate) fn project_tag(project_root: &Path) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in project_root.to_string_lossy().as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:08x}", (h ^ (h >> 32)) as u32)
}

/// Write `text` to `path` through a sibling tmp file and a rename, creating parent directories.
pub(crate) fn write_atomic(path: &Path, text: &str) -> Result<()> {
    std::fs::create_dir_all(path.parent().expect("path has a parent"))
        .with_context(|| format!("cannot create the folder for {}", path.display()))?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).with_context(|| format!("cannot write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("cannot write {}", path.display()))
}
```

- [ ] **Step 5: Run to verify pass**

Run: `cargo test scenario::store 2>&1 | tail -5`
Expected: `test result: ok. 5 passed`

- [ ] **Step 6: Format and commit**

```bash
rustfmt --edition 2021 src/scenario/env.rs src/scenario/store.rs src/scenario/mod.rs
git add src/scenario/env.rs src/scenario/store.rs src/scenario/mod.rs
git commit -m "feat(scenario): environment contract and run store"
```

---

### Task 3: Engine lifecycle over a fake environment

**Files:**
- Create: `src/scenario/engine.rs`
- Modify: `src/scenario/mod.rs` (add `pub(crate) mod engine;`)

**Interfaces:**
- Consumes: `Environment`, `EnvHandle`, `CheckOutcome` (Task 2); `Store`, `RunRecord`, `RunState` (Task 2); `Package`, `Tone` (Task 1)
- Produces:
  - `pub(crate) struct Tick<'a> { pub pid: u32, pub alive: &'a dyn Fn(u32) -> bool, pub stamp: &'a str, pub today: &'a str }` (`stamp` = `YYYYMMDDHHMMSS`, `today` = `YYYY-MM-DD`)
  - `pub(crate) struct CheckReport { pub outcome: CheckOutcome, pub clean_warning: Option<String> }`
  - `pub(crate) fn start(env: &mut dyn Environment, store: &Store, pkg: &Package, tone: Tone, t: &Tick) -> Result<RunRecord>`
  - `pub(crate) fn resume(env: &mut dyn Environment, store: &Store, pkg: &Package, rec: &mut RunRecord, t: &Tick) -> Result<()>`
  - `pub(crate) fn pause(env: &mut dyn Environment, store: &Store, rec: &mut RunRecord, t: &Tick) -> Result<()>`
  - `pub(crate) fn check(env: &mut dyn Environment, store: &Store, pkg: &Package, rec: &mut RunRecord, t: &Tick) -> Result<CheckReport>`
  - `pub(crate) fn drop_run(env: &mut dyn Environment, store: &Store, rec: &mut RunRecord, t: &Tick) -> Result<Option<String>>` (abandon an unfinished run; the record is written as `Dropped` first, then the environment is cleaned; `Some(text)` = clean warning)

- [ ] **Step 1: Write the failing tests**

Create `src/scenario/engine.rs` with only the tests:

```rust
#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::scenario::package::tests::{tmp, write_pkg};

    /// Test double for the contract: no Docker, records every call.
    struct FakeEnv {
        calls: Vec<String>,
        checks: VecDeque<CheckOutcome>,
        fail_prepare: bool,
        fail_clean: bool,
        store: Option<Store>,
        state_seen_at_clean: Option<RunState>,
    }

    impl FakeEnv {
        fn new() -> FakeEnv {
            FakeEnv {
                calls: vec![],
                checks: VecDeque::new(),
                fail_prepare: false,
                fail_clean: false,
                store: None,
                state_seen_at_clean: None,
            }
        }
    }

    impl Environment for FakeEnv {
        fn prepare(&mut self, _p: &Package, run_id: &str) -> Result<EnvHandle> {
            self.calls.push(format!("prepare {run_id}"));
            if self.fail_prepare {
                anyhow::bail!("boom");
            }
            Ok(EnvHandle {
                reference: format!("fake-{run_id}"),
                access: "fake access".into(),
            })
        }
        fn pause(&mut self, h: &EnvHandle) -> Result<()> {
            self.calls.push(format!("pause {}", h.reference));
            Ok(())
        }
        fn resume(&mut self, _p: &Package, h: &EnvHandle) -> Result<()> {
            self.calls.push(format!("resume {}", h.reference));
            Ok(())
        }
        fn check(&mut self, _p: &Package, h: &EnvHandle) -> Result<CheckOutcome> {
            self.calls.push(format!("check {}", h.reference));
            Ok(self.checks.pop_front().unwrap_or(CheckOutcome::Pass))
        }
        fn clean(&mut self, h: &EnvHandle) -> Result<()> {
            self.calls.push(format!("clean {}", h.reference));
            if let Some(s) = &self.store {
                self.state_seen_at_clean = s.all().first().map(|r| r.state);
            }
            if self.fail_clean {
                anyhow::bail!("rm failed");
            }
            Ok(())
        }
    }

    fn tick<'a>(pid: u32, stamp: &'a str, alive: &'a dyn Fn(u32) -> bool) -> Tick<'a> {
        Tick {
            pid,
            alive,
            stamp,
            today: "2026-11-04",
        }
    }

    fn setup(name: &str) -> (PathBuf, Store, Package) {
        let root = tmp(name);
        let pkg = Package::load(&write_pkg(&root)).unwrap();
        (root.clone(), Store::new(&root), pkg)
    }

    const NOBODY: &dyn Fn(u32) -> bool = &|_| false;

    #[test]
    fn start_writes_an_active_record_owned_by_us() {
        let (root, store, pkg) = setup("eng_start");
        let mut env = FakeEnv::new();
        let rec = start(&mut env, &store, &pkg, Tone::Adventure, &tick(10, "20261104090000", NOBODY)).unwrap();
        assert_eq!(rec.run_id, "demo-pkg-20261104090000");
        assert_eq!(rec.state, RunState::Active);
        assert_eq!(rec.owner_pid, 10);
        assert_eq!(rec.tone, "adventure");
        assert_eq!(rec.reference, "fake-demo-pkg-20261104090000");
        assert_eq!(store.load(&rec.run_id).unwrap(), rec);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_prepare_leaves_no_record() {
        let (root, store, pkg) = setup("eng_failprep");
        let mut env = FakeEnv::new();
        env.fail_prepare = true;
        assert!(start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).is_err());
        assert!(store.all().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn second_start_pauses_the_first_so_one_runs_at_a_time() {
        let (root, store, pkg) = setup("eng_two");
        let mut env = FakeEnv::new();
        let a = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        let b = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "2", NOBODY)).unwrap();
        assert_eq!(store.load(&a.run_id).unwrap().state, RunState::Paused);
        assert_eq!(store.load(&b.run_id).unwrap().state, RunState::Active);
        assert_eq!(
            env.calls,
            vec![
                "prepare demo-pkg-1".to_string(),
                "pause fake-demo-pkg-1".into(),
                "prepare demo-pkg-2".into()
            ]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_run_owned_by_another_live_usta_blocks_start() {
        let (root, store, pkg) = setup("eng_live");
        let mut env = FakeEnv::new();
        start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        let alive: &dyn Fn(u32) -> bool = &|p| p == 10;
        let err = start(&mut env, &store, &pkg, Tone::Plain, &tick(99, "2", alive)).unwrap_err();
        assert!(err.to_string().contains("another usta (pid 10)"), "{err}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pause_is_idempotent_and_clears_the_owner() {
        let (root, store, pkg) = setup("eng_pause");
        let mut env = FakeEnv::new();
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        pause(&mut env, &store, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        pause(&mut env, &store, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        assert_eq!(env.calls.iter().filter(|c| c.starts_with("pause")).count(), 1);
        let saved = store.load(&rec.run_id).unwrap();
        assert_eq!((saved.state, saved.owner_pid), (RunState::Paused, 0));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resume_reactivates_and_takes_ownership() {
        let (root, store, pkg) = setup("eng_resume");
        let mut env = FakeEnv::new();
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        pause(&mut env, &store, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        resume(&mut env, &store, &pkg, &mut rec, &tick(20, "x", NOBODY)).unwrap();
        let saved = store.load(&rec.run_id).unwrap();
        assert_eq!((saved.state, saved.owner_pid), (RunState::Active, 20));
        assert!(env.calls.contains(&"resume fake-demo-pkg-1".to_string()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resume_of_a_finished_run_is_refused() {
        let (root, store, pkg) = setup("eng_resume_fin");
        let mut env = FakeEnv::new();
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        check(&mut env, &store, &pkg, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        assert!(resume(&mut env, &store, &pkg, &mut rec, &tick(10, "x", NOBODY)).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failing_check_keeps_the_run_active() {
        let (root, store, pkg) = setup("eng_checkfail");
        let mut env = FakeEnv::new();
        env.checks.push_back(CheckOutcome::Fail { output: "nope".into() });
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        let r = check(&mut env, &store, &pkg, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        assert_eq!(r.outcome, CheckOutcome::Fail { output: "nope".into() });
        let saved = store.load(&rec.run_id).unwrap();
        assert_eq!(saved.state, RunState::Active);
        assert_eq!(saved.last_check.as_deref(), Some("fail"));
        assert!(!env.calls.iter().any(|c| c.starts_with("clean")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn passing_check_writes_finished_before_clean() {
        let (root, store, pkg) = setup("eng_checkpass");
        let mut env = FakeEnv::new();
        env.store = Some(store.clone());
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        let r = check(&mut env, &store, &pkg, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        assert_eq!(r.outcome, CheckOutcome::Pass);
        assert_eq!(env.state_seen_at_clean, Some(RunState::Finished));
        let saved = store.load(&rec.run_id).unwrap();
        assert_eq!((saved.state, saved.owner_pid), (RunState::Finished, 0));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn clean_failure_is_a_warning_not_an_error() {
        let (root, store, pkg) = setup("eng_cleanfail");
        let mut env = FakeEnv::new();
        env.fail_clean = true;
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        let r = check(&mut env, &store, &pkg, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        assert_eq!(r.outcome, CheckOutcome::Pass);
        assert!(r.clean_warning.unwrap().contains("rm failed"));
        assert_eq!(store.load(&rec.run_id).unwrap().state, RunState::Finished);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn drop_writes_dropped_before_cleaning_and_keeps_the_record() {
        let (root, store, pkg) = setup("eng_drop");
        let mut env = FakeEnv::new();
        env.store = Some(store.clone());
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        let warn = drop_run(&mut env, &store, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        assert!(warn.is_none());
        assert_eq!(env.state_seen_at_clean, Some(RunState::Dropped));
        let saved = store.load(&rec.run_id).unwrap();
        assert_eq!((saved.state, saved.owner_pid), (RunState::Dropped, 0));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn drop_works_on_a_paused_run_and_reports_a_failed_clean_as_a_warning() {
        let (root, store, pkg) = setup("eng_drop_paused");
        let mut env = FakeEnv::new();
        env.fail_clean = true;
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        pause(&mut env, &store, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        let warn = drop_run(&mut env, &store, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        assert!(warn.unwrap().contains("rm failed"));
        assert_eq!(store.load(&rec.run_id).unwrap().state, RunState::Dropped);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn drop_of_a_finished_or_dropped_run_is_refused_and_so_is_resuming_it() {
        let (root, store, pkg) = setup("eng_drop_over");
        let mut env = FakeEnv::new();
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        drop_run(&mut env, &store, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        assert!(drop_run(&mut env, &store, &mut rec, &tick(10, "x", NOBODY)).is_err());
        assert!(resume(&mut env, &store, &pkg, &mut rec, &tick(10, "x", NOBODY)).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn drop_by_another_live_usta_owner_is_refused() {
        let (root, store, pkg) = setup("eng_drop_live");
        let mut env = FakeEnv::new();
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        let alive: &dyn Fn(u32) -> bool = &|p| p == 10;
        assert!(drop_run(&mut env, &store, &mut rec, &tick(99, "x", alive)).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn check_on_a_paused_run_is_refused() {
        let (root, store, pkg) = setup("eng_check_paused");
        let mut env = FakeEnv::new();
        let mut rec = start(&mut env, &store, &pkg, Tone::Plain, &tick(10, "1", NOBODY)).unwrap();
        pause(&mut env, &store, &mut rec, &tick(10, "x", NOBODY)).unwrap();
        assert!(check(&mut env, &store, &pkg, &mut rec, &tick(10, "x", NOBODY)).is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test scenario::engine 2>&1 | tail -5`
Expected: FAIL to compile (`start`, `Tick` not found).

- [ ] **Step 3: Write the implementation** (above the test block)

```rust
//! Scenario engine: drives one run through the `Environment` contract and
//! keeps the run record honest. Knows nothing about Docker.

use anyhow::{bail, Result};

use crate::scenario::env::{CheckOutcome, Environment};
use crate::scenario::package::{Package, Tone};
use crate::scenario::store::{RunRecord, RunState, Store};

/// Clock and process facts, injected so the engine is deterministic in tests.
pub(crate) struct Tick<'a> {
    pub pid: u32,
    pub alive: &'a dyn Fn(u32) -> bool,
    /// `YYYYMMDDHHMMSS`, makes run ids unique.
    pub stamp: &'a str,
    /// `YYYY-MM-DD`.
    pub today: &'a str,
}

pub(crate) struct CheckReport {
    pub outcome: CheckOutcome,
    /// Set when the run finished but the environment could not be cleaned; the startup sweep retries.
    pub clean_warning: Option<String>,
}

/// At most one run is active per project: pause the others, but never one a
/// different live Usta owns.
fn release_others(
    env: &mut dyn Environment,
    store: &Store,
    t: &Tick,
    except: Option<&str>,
) -> Result<()> {
    for mut rec in store.all() {
        if rec.state != RunState::Active || Some(rec.run_id.as_str()) == except {
            continue;
        }
        if rec.owner_pid != 0 && rec.owner_pid != t.pid && (t.alive)(rec.owner_pid) {
            bail!(
                "another usta (pid {}) is running scenario '{}'; close it first",
                rec.owner_pid,
                rec.package
            );
        }
        pause(env, store, &mut rec, t)?;
    }
    Ok(())
}

pub(crate) fn start(
    env: &mut dyn Environment,
    store: &Store,
    pkg: &Package,
    tone: Tone,
    t: &Tick,
) -> Result<RunRecord> {
    release_others(env, store, t, None)?;
    let mut rec = RunRecord {
        run_id: format!("{}-{}", pkg.id, t.stamp),
        package: pkg.id.clone(),
        topic: pkg.topic.clone(),
        tone: tone.as_str().into(),
        state: RunState::Preparing,
        owner_pid: t.pid,
        started: t.today.into(),
        updated: t.today.into(),
        reference: String::new(),
        access: String::new(),
        last_check: None,
    };
    // Record first: a crash mid-prepare leaves a record the sweep can resolve,
    // never an unrecorded container.
    store.save(&rec)?;
    match env.prepare(pkg, &rec.run_id) {
        Ok(h) => {
            rec.reference = h.reference;
            rec.access = h.access;
            rec.state = RunState::Active;
            store.save(&rec)?;
            Ok(rec)
        }
        Err(e) => {
            let _ = store.remove(&rec.run_id);
            Err(e)
        }
    }
}

pub(crate) fn resume(
    env: &mut dyn Environment,
    store: &Store,
    pkg: &Package,
    rec: &mut RunRecord,
    t: &Tick,
) -> Result<()> {
    if matches!(
        rec.state,
        RunState::Finished | RunState::Preparing | RunState::Dropped
    ) {
        bail!("run '{}' cannot be resumed ({:?})", rec.run_id, rec.state);
    }
    release_others(env, store, t, Some(&rec.run_id))?;
    if rec.state == RunState::Active
        && rec.owner_pid != 0
        && rec.owner_pid != t.pid
        && (t.alive)(rec.owner_pid)
    {
        bail!("another usta (pid {}) is running this scenario", rec.owner_pid);
    }
    env.resume(pkg, &rec.handle())?;
    rec.state = RunState::Active;
    rec.owner_pid = t.pid;
    rec.updated = t.today.into();
    store.save(rec)
}

pub(crate) fn pause(
    env: &mut dyn Environment,
    store: &Store,
    rec: &mut RunRecord,
    t: &Tick,
) -> Result<()> {
    if rec.state == RunState::Paused {
        return Ok(());
    }
    env.pause(&rec.handle())?;
    rec.state = RunState::Paused;
    rec.owner_pid = 0;
    rec.updated = t.today.into();
    store.save(rec)
}

/// Run the check. On a pass the record is written as finished FIRST, then the
/// environment is cleaned, so a crash in between leaves a finished record and a
/// leftover the sweep removes, never an unfinished-looking task.
pub(crate) fn check(
    env: &mut dyn Environment,
    store: &Store,
    pkg: &Package,
    rec: &mut RunRecord,
    t: &Tick,
) -> Result<CheckReport> {
    if rec.state != RunState::Active {
        bail!("run '{}' is not active; use /scenario resume", rec.run_id);
    }
    let outcome = env.check(pkg, &rec.handle())?;
    rec.updated = t.today.into();
    let mut clean_warning = None;
    match &outcome {
        CheckOutcome::Pass => {
            rec.last_check = Some("pass".into());
            rec.state = RunState::Finished;
            rec.owner_pid = 0;
            store.save(rec)?;
            clean_warning = env.clean(&rec.handle()).err().map(|e| e.to_string());
        }
        CheckOutcome::Fail { .. } => {
            rec.last_check = Some("fail".into());
            store.save(rec)?;
        }
    }
    Ok(CheckReport {
        outcome,
        clean_warning,
    })
}

/// Abandon an unfinished run. Like finishing, the record is written FIRST
/// (as `Dropped`), then the environment is cleaned, so a crash in between
/// leaves a leftover the startup sweep removes, never a half-dropped task.
/// Returns a warning when the clean failed.
pub(crate) fn drop_run(
    env: &mut dyn Environment,
    store: &Store,
    rec: &mut RunRecord,
    t: &Tick,
) -> Result<Option<String>> {
    if matches!(rec.state, RunState::Finished | RunState::Dropped) {
        bail!("'{}' is already over", rec.package);
    }
    if rec.state == RunState::Active
        && rec.owner_pid != 0
        && rec.owner_pid != t.pid
        && (t.alive)(rec.owner_pid)
    {
        bail!("another usta (pid {}) is running this scenario", rec.owner_pid);
    }
    rec.state = RunState::Dropped;
    rec.owner_pid = 0;
    rec.updated = t.today.into();
    store.save(rec)?;
    if rec.reference.is_empty() {
        return Ok(None); // interrupted before anything existed
    }
    Ok(env.clean(&rec.handle()).err().map(|e| e.to_string()))
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test scenario::engine 2>&1 | tail -5`
Expected: `test result: ok. 11 passed`

- [ ] **Step 5: Format and commit**

```bash
rustfmt --edition 2021 src/scenario/engine.rs src/scenario/mod.rs
git add src/scenario/engine.rs src/scenario/mod.rs
git commit -m "feat(scenario): engine lifecycle over the environment contract"
```

---

### Task 4: Skill matching, story prerequisites, skip and choice

**Files:**
- Create: `src/scenario/select.rs`
- Modify: `src/scenario/mod.rs` (add `pub(crate) mod select;`)

**Interfaces:**
- Consumes: `Package`, `ThreadSpec` (Task 1); `crate::tokens::{map_state_of, STATE_SETTLED, STATE_DEEPENED}`
- Produces:
  - `pub(crate) enum Tag { New, Done, Skipped, Later }` (`Later` = story prerequisites not met)
  - `pub(crate) fn threads_available(pkgs: &[Package], map: &str, open: &[String]) -> Vec<String>` (open ledger threads plus the threads opened by packages that are skipped as already settled, so a skipped scenario never locks the story)
  - `pub(crate) fn tag(pkg: &Package, map: &str, finished: &[String], threads: &[String]) -> Tag`
  - `pub(crate) fn resolve<'a>(pkgs: &'a [Package], arg: Option<&str>, map: &str, finished: &[String], open: &[String]) -> Result<&'a Package, String>` (`pkgs` sorted by id; `arg` is a 1-based number or an id)

- [ ] **Step 1: Write the failing tests**

Create `src/scenario/select.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::package::tests::{tmp, write_pkg};
    use crate::scenario::package::ThreadSpec;

    fn pkg(id: &str, skills: &[&str]) -> Package {
        let root = tmp(&format!("sel_{id}"));
        let mut p = Package::load(&write_pkg(&root)).unwrap();
        p.id = id.into();
        p.skills = skills.iter().map(|s| s.to_string()).collect();
        p
    }

    fn opens(p: &mut Package, id: &str) {
        p.story.opens.push(ThreadSpec { id: id.into(), text: "t".into() });
    }

    const MAP: &str = "# linux\n\
        - File permissions and ownership: settled\n\
        - Disk usage and finding files: seen\n\
        - Processes and signals: not seen\n\
        - Shell quoting: deepened\n";

    #[test]
    fn skipped_only_when_every_skill_matches_a_settled_line() {
        assert_eq!(tag(&pkg("a", &["permission", "ownership"]), MAP, &[], &[]), Tag::Skipped);
        assert_eq!(tag(&pkg("b", &["permission", "processes"]), MAP, &[], &[]), Tag::New);
        assert_eq!(tag(&pkg("c", &["quoting"]), MAP, &[], &[]), Tag::Skipped);
    }

    #[test]
    fn an_unmatched_skill_is_never_skipped() {
        assert_eq!(tag(&pkg("a", &["cron"]), MAP, &[], &[]), Tag::New);
        assert_eq!(tag(&pkg("a", &["permission"]), "", &[], &[]), Tag::New);
    }

    #[test]
    fn matching_is_case_insensitive_and_ignores_non_map_lines() {
        let map = "## Disk\nPERMISSION notes without a state\n- Permission bits: seen\n";
        assert_eq!(tag(&pkg("a", &["permission"]), map, &[], &[]), Tag::New);
    }

    #[test]
    fn one_unsettled_match_blocks_the_skip() {
        let map = "- Permission basics: settled\n- Permission edge cases: seen\n";
        assert_eq!(tag(&pkg("a", &["permission"]), map, &[], &[]), Tag::New);
    }

    #[test]
    fn finished_packages_are_done_and_not_chosen_while_a_new_one_exists() {
        let pkgs = vec![pkg("a", &["x1"]), pkg("b", &["x2"])];
        let fin = vec!["a".to_string()];
        assert_eq!(tag(&pkgs[0], "", &fin, &[]), Tag::Done);
        assert_eq!(resolve(&pkgs, None, "", &fin, &[]).unwrap().id, "b");
    }

    #[test]
    fn a_package_is_later_until_its_needed_thread_is_open() {
        let mut a = pkg("a", &["x1"]);
        opens(&mut a, "t1");
        let mut b = pkg("b", &["x2"]);
        b.story.needs = vec!["t1".into()];
        let pkgs = vec![a, b];
        assert_eq!(tag(&pkgs[1], "", &[], &[]), Tag::Later);
        assert_eq!(resolve(&pkgs, None, "", &[], &[]).unwrap().id, "a");
        let open = vec!["t1".to_string()];
        assert_eq!(tag(&pkgs[1], "", &[], &threads_available(&pkgs, "", &open)), Tag::New);
    }

    #[test]
    fn a_skipped_package_opens_its_threads_for_the_ones_that_need_them() {
        let mut a = pkg("a", &["permission"]);
        opens(&mut a, "t1");
        let mut b = pkg("b", &["x2"]);
        b.story.needs = vec!["t1".into()];
        let pkgs = vec![a, b];
        assert_eq!(threads_available(&pkgs, MAP, &[]), vec!["t1".to_string()]);
        assert_eq!(resolve(&pkgs, None, MAP, &[], &[]).unwrap().id, "b");
    }

    #[test]
    fn resolve_by_number_and_id_even_for_done_or_skipped() {
        let pkgs = vec![pkg("a", &["permission"]), pkg("b", &["x2"])];
        assert_eq!(resolve(&pkgs, Some("1"), MAP, &[], &[]).unwrap().id, "a");
        assert_eq!(resolve(&pkgs, Some("b"), MAP, &[], &[]).unwrap().id, "b");
        assert!(resolve(&pkgs, Some("9"), MAP, &[], &[]).is_err());
        assert!(resolve(&pkgs, Some("zzz"), MAP, &[], &[]).is_err());
    }

    #[test]
    fn an_explicit_pick_of_a_later_task_is_refused() {
        let mut b = pkg("b", &["x2"]);
        b.story.needs = vec!["t1".into()];
        let err = resolve(&[b], Some("b"), "", &[], &[]).unwrap_err();
        assert!(err.contains("later in the story"), "{err}");
    }

    #[test]
    fn nothing_new_gives_a_clear_message() {
        let pkgs = vec![pkg("a", &["x1"])];
        let err = resolve(&pkgs, None, "", &["a".to_string()], &[]).unwrap_err();
        assert!(err.contains("no new task"), "{err}");
        assert!(resolve(&[], None, "", &[], &[]).unwrap_err().contains("no tasks"));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test scenario::select 2>&1 | tail -5`
Expected: FAIL to compile.

- [ ] **Step 3: Write the implementation** (above the tests)

```rust
//! Which scenario next. Skills are keywords matched against curriculum-map
//! lines (map items are free text written by the model). A finished skill is
//! skipped; a package already finished is not offered again while a new one
//! exists; a package whose story prerequisites (`story.needs`) are not open
//! yet is `Later`. Due-return is not built here (the map carries no `due:`).

use crate::scenario::package::Package;
use crate::tokens;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tag {
    New,
    Done,
    Skipped,
    Later,
}

/// Map states of every map line mentioning `skill` (case-insensitive substring).
fn states_for(map: &str, skill: &str) -> Vec<&'static str> {
    let needle = skill.trim().to_ascii_lowercase();
    map.lines()
        .filter_map(|l| {
            let state = tokens::map_state_of(l)?;
            l.to_ascii_lowercase().contains(&needle).then_some(state)
        })
        .collect()
}

/// True when every skill matches at least one map line and all matches are settled/deepened.
fn settled(pkg: &Package, map: &str) -> bool {
    pkg.skills.iter().all(|s| {
        let st = states_for(map, s);
        !st.is_empty()
            && st
                .iter()
                .all(|x| *x == tokens::STATE_SETTLED || *x == tokens::STATE_DEEPENED)
    })
}

/// Open ledger threads plus the threads opened by packages skipped as settled.
pub(crate) fn threads_available(pkgs: &[Package], map: &str, open: &[String]) -> Vec<String> {
    let mut out: Vec<String> = open.to_vec();
    for p in pkgs.iter().filter(|p| settled(p, map)) {
        for t in &p.story.opens {
            if !out.contains(&t.id) {
                out.push(t.id.clone());
            }
        }
    }
    out
}

pub(crate) fn tag(pkg: &Package, map: &str, finished: &[String], threads: &[String]) -> Tag {
    if settled(pkg, map) {
        Tag::Skipped
    } else if finished.iter().any(|f| *f == pkg.id) {
        Tag::Done
    } else if pkg.story.needs.iter().any(|n| !threads.contains(n)) {
        Tag::Later
    } else {
        Tag::New
    }
}

/// Pick a package. `arg` = 1-based number or id (explicit picks are honoured
/// even for done/skipped, but not for a task that is later in the story);
/// `None` = the first `New` one.
pub(crate) fn resolve<'a>(
    pkgs: &'a [Package],
    arg: Option<&str>,
    map: &str,
    finished: &[String],
    open: &[String],
) -> Result<&'a Package, String> {
    if pkgs.is_empty() {
        return Err("no tasks for this topic yet".into());
    }
    let threads = threads_available(pkgs, map, open);
    let picked = match arg {
        Some(a) => {
            let found = match a.parse::<usize>() {
                Ok(n) => n.checked_sub(1).and_then(|i| pkgs.get(i)),
                Err(_) => pkgs.iter().find(|p| p.id == a),
            };
            let p = found.ok_or_else(|| format!("no task '{a}'; see /scenario"))?;
            if tag(p, map, finished, &threads) == Tag::Later {
                return Err("that task comes later in the story; finish the earlier ones first".into());
            }
            p
        }
        None => pkgs
            .iter()
            .find(|p| tag(p, map, finished, &threads) == Tag::New)
            .ok_or_else(|| "no new task for this topic: all are done, settled, or later in the story".to_string())?,
    };
    Ok(picked)
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test scenario::select 2>&1 | tail -5`
Expected: `test result: ok. 10 passed`

- [ ] **Step 5: Format and commit**

```bash
rustfmt --edition 2021 src/scenario/select.rs src/scenario/mod.rs
git add src/scenario/select.rs src/scenario/mod.rs
git commit -m "feat(scenario): skill matching, story prerequisites, skip and choice"
```

---

### Task 5: World ledger

A shell-owned file per topic holding the company, characters, past events and open threads. Updated when a scenario finishes, read when the next one opens, so the first Linux scenarios form one continuing story. Packages declare what they add (`story.company`, `characters`, `event`, `opens`, `closes`) and need (`needs`) through their `story` field (Task 1).

**Files:**
- Create: `src/scenario/ledger.rs`
- Modify: `src/scenario/mod.rs` (add `pub(crate) mod ledger;`)

**Interfaces:**
- Consumes: `StorySpec`, `Package` (Task 1); `RunRecord`, `RunState`, `write_atomic` (Task 2)
- Produces:
  - `pub(crate) struct Ledger { company: Option<String>, characters: Vec<Character>, events: Vec<Event>, threads: Vec<Thread> }` (`Default`, `Clone`, `PartialEq`, `Serialize`, `Deserialize`); `Character { name, role }`, `Event { date, run_id, text }`, `Thread { id, text, open: bool }`
  - `pub(crate) fn ledger_path(project_root: &Path, topic: &str) -> PathBuf` (`<project>/.usta/scenarios/ledger/<topic>.toml`)
  - `Ledger::load(project_root: &Path, topic: &str) -> Ledger` (missing or unreadable file gives the default)
  - `Ledger::save(&self, project_root: &Path, topic: &str) -> Result<()>`
  - `Ledger::apply_finished(&mut self, story: &StorySpec, run_id: &str, today: &str) -> bool` (idempotent per `run_id`; true when it changed anything)
  - `Ledger::open_ids(&self) -> Vec<String>`
  - `Ledger::summary(&self) -> String` (empty when there is nothing to say)
  - `pub(crate) fn reconcile(project_root: &Path, topic: &str, records: &[RunRecord], pkgs: &[Package], today: &str) -> Result<()>` (applies finished runs the ledger has not seen, so a crash between the finished record and the ledger write loses nothing)

- [ ] **Step 1: Write the failing tests**

Create `src/scenario/ledger.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::package::tests::tmp;
    use crate::scenario::package::{Character as PChar, StorySpec, ThreadSpec};

    fn story() -> StorySpec {
        StorySpec {
            company: Some("Shopwell".into()),
            characters: vec![PChar { name: "Deniz".into(), role: "runs the servers at night".into() }],
            event: Some("The nightly backup was repaired.".into()),
            opens: vec![ThreadSpec { id: "backups-ok".into(), text: "Backups run every night again.".into() }],
            closes: vec![],
            needs: vec![],
        }
    }

    #[test]
    fn apply_finished_records_the_story_once_per_run() {
        let mut l = Ledger::default();
        assert!(l.apply_finished(&story(), "run-1", "2026-11-04"));
        assert!(!l.apply_finished(&story(), "run-1", "2026-11-04"));
        assert_eq!(l.company.as_deref(), Some("Shopwell"));
        assert_eq!(l.characters.len(), 1);
        assert_eq!(l.events.len(), 1);
        assert_eq!(l.open_ids(), vec!["backups-ok".to_string()]);
    }

    #[test]
    fn characters_are_not_duplicated_and_company_is_not_overwritten() {
        let mut l = Ledger::default();
        l.apply_finished(&story(), "run-1", "2026-11-04");
        let mut s2 = story();
        s2.company = Some("Other Inc".into());
        l.apply_finished(&s2, "run-2", "2026-11-05");
        assert_eq!(l.company.as_deref(), Some("Shopwell"));
        assert_eq!(l.characters.len(), 1);
        assert_eq!(l.events.len(), 2);
    }

    #[test]
    fn closes_marks_a_thread_closed() {
        let mut l = Ledger::default();
        l.apply_finished(&story(), "run-1", "2026-11-04");
        let close = StorySpec { closes: vec!["backups-ok".into()], ..StorySpec::default() };
        l.apply_finished(&close, "run-2", "2026-11-05");
        assert!(l.open_ids().is_empty());
        assert_eq!(l.threads.len(), 1);
    }

    #[test]
    fn summary_names_company_characters_events_and_open_threads() {
        let mut l = Ledger::default();
        assert_eq!(l.summary(), "");
        l.apply_finished(&story(), "run-1", "2026-11-04");
        let s = l.summary();
        for want in ["Shopwell", "Deniz", "runs the servers at night", "2026-11-04", "The nightly backup was repaired.", "Backups run every night again."] {
            assert!(s.contains(want), "{want} not in {s}");
        }
    }

    #[test]
    fn summary_keeps_only_the_latest_events() {
        let mut l = Ledger::default();
        for i in 0..12 {
            let s = StorySpec { event: Some(format!("event-{i}")), ..StorySpec::default() };
            l.apply_finished(&s, &format!("r{i}"), "2026-11-04");
        }
        let s = l.summary();
        assert!(s.contains("event-11") && !s.contains("event-0;"), "{s}");
    }

    #[test]
    fn save_load_roundtrip_and_bad_or_missing_file_is_the_default() {
        let root = tmp("ledger_rt");
        assert_eq!(Ledger::load(&root, "linux"), Ledger::default());
        let mut l = Ledger::default();
        l.apply_finished(&story(), "run-1", "2026-11-04");
        l.save(&root, "linux").unwrap();
        assert_eq!(Ledger::load(&root, "linux"), l);
        std::fs::write(ledger_path(&root, "linux"), "garbage = [").unwrap();
        assert_eq!(Ledger::load(&root, "linux"), Ledger::default());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reconcile_applies_finished_runs_in_order_and_skips_unknown_packages() {
        use crate::scenario::package::tests::write_pkg;
        use crate::scenario::package::Package;
        use crate::scenario::store::{RunRecord, RunState};
        let root = tmp("ledger_rec");
        let mut p = Package::load(&write_pkg(&root)).unwrap();
        p.story = story();
        let rec = |id: &str, pkg: &str, st: RunState, started: &str| RunRecord {
            run_id: id.into(),
            package: pkg.into(),
            topic: "linux".into(),
            tone: "plain".into(),
            state: st,
            owner_pid: 0,
            started: started.into(),
            updated: started.into(),
            reference: String::new(),
            access: String::new(),
            last_check: None,
        };
        let recs = vec![
            rec("demo-pkg-1", "demo-pkg", RunState::Finished, "2026-11-04"),
            rec("gone-1", "gone", RunState::Finished, "2026-11-04"),
            rec("demo-pkg-2", "demo-pkg", RunState::Dropped, "2026-11-05"),
        ];
        reconcile(&root, "linux", &recs, &[p], "2026-11-06").unwrap();
        let l = Ledger::load(&root, "linux");
        assert_eq!(l.events.len(), 1);
        assert_eq!(l.events[0].run_id, "demo-pkg-1");
        let _ = std::fs::remove_dir_all(root);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test scenario::ledger 2>&1 | tail -5`
Expected: FAIL to compile.

- [ ] **Step 3: Write the implementation** (above the tests)

```rust
//! World ledger: the continuing story of one topic. Shell-owned TOML under
//! `<project>/.usta/scenarios/ledger/<topic>.toml`; the model never writes it.
//! Packages declare what they add and need through their `story` field.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::scenario::package::{Package, StorySpec};
use crate::scenario::store::{write_atomic, RunRecord, RunState};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Character {
    pub name: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Event {
    pub date: String,
    pub run_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Thread {
    pub id: String,
    pub text: String,
    pub open: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Ledger {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub company: Option<String>,
    #[serde(default)]
    pub characters: Vec<Character>,
    #[serde(default)]
    pub events: Vec<Event>,
    #[serde(default)]
    pub threads: Vec<Thread>,
}

/// How many past events the mentor turn carries.
const SUMMARY_EVENTS: usize = 8;

pub(crate) fn ledger_path(project_root: &Path, topic: &str) -> PathBuf {
    project_root
        .join(".usta/scenarios/ledger")
        .join(format!("{topic}.toml"))
}

impl Ledger {
    pub fn load(project_root: &Path, topic: &str) -> Ledger {
        std::fs::read_to_string(ledger_path(project_root, topic))
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, project_root: &Path, topic: &str) -> Result<()> {
        write_atomic(&ledger_path(project_root, topic), &toml::to_string(self)?)
    }

    /// Fold a finished scenario's story in. Idempotent per `run_id`.
    pub fn apply_finished(&mut self, story: &StorySpec, run_id: &str, today: &str) -> bool {
        if self.events.iter().any(|e| e.run_id == run_id) {
            return false;
        }
        if self.company.is_none() {
            self.company = story.company.clone();
        }
        for c in &story.characters {
            if !self.characters.iter().any(|x| x.name == c.name) {
                self.characters.push(Character { name: c.name.clone(), role: c.role.clone() });
            }
        }
        for o in &story.opens {
            match self.threads.iter_mut().find(|t| t.id == o.id) {
                Some(t) => {
                    t.open = true;
                    t.text = o.text.clone();
                }
                None => self.threads.push(Thread { id: o.id.clone(), text: o.text.clone(), open: true }),
            }
        }
        for id in &story.closes {
            if let Some(t) = self.threads.iter_mut().find(|t| t.id == *id) {
                t.open = false;
            }
        }
        // Always recorded, even without text: it is the idempotency marker for this run.
        self.events.push(Event {
            date: today.into(),
            run_id: run_id.into(),
            text: story.event.clone().unwrap_or_default(),
        });
        true
    }

    pub fn open_ids(&self) -> Vec<String> {
        self.threads.iter().filter(|t| t.open).map(|t| t.id.clone()).collect()
    }

    /// Compact text for the mentor turn when a task opens. Empty when there is nothing yet.
    pub fn summary(&self) -> String {
        let events: Vec<&Event> = self.events.iter().filter(|e| !e.text.is_empty()).collect();
        if self.company.is_none() && self.characters.is_empty() && events.is_empty() {
            return String::new();
        }
        let mut out = String::from("World so far");
        if let Some(c) = &self.company {
            out.push_str(&format!(" ({c})"));
        }
        out.push_str(": ");
        if !self.characters.is_empty() {
            let cs: Vec<String> = self.characters.iter().map(|c| format!("{} ({})", c.name, c.role)).collect();
            out.push_str(&format!("people: {}. ", cs.join(", ")));
        }
        if !events.is_empty() {
            let skip = events.len().saturating_sub(SUMMARY_EVENTS);
            let es: Vec<String> = events.iter().skip(skip).map(|e| format!("[{}] {};", e.date, e.text)).collect();
            out.push_str(&format!("past: {} ", es.join(" ")));
        }
        let open: Vec<&str> = self.threads.iter().filter(|t| t.open).map(|t| t.text.as_str()).collect();
        if !open.is_empty() {
            out.push_str(&format!("open threads: {}", open.join("; ")));
        }
        out.trim_end().to_string()
    }
}

/// Apply finished runs the ledger has not seen yet (oldest first). A crash
/// between the finished record and the ledger write is repaired at the next start.
pub(crate) fn reconcile(
    project_root: &Path,
    topic: &str,
    records: &[RunRecord],
    pkgs: &[Package],
    today: &str,
) -> Result<()> {
    let mut ledger = Ledger::load(project_root, topic);
    let mut changed = false;
    for r in records.iter().filter(|r| r.state == RunState::Finished && r.topic == topic) {
        if let Some(p) = pkgs.iter().find(|p| p.id == r.package) {
            changed |= ledger.apply_finished(&p.story, &r.run_id, today);
        }
    }
    if changed {
        ledger.save(project_root, topic)?;
    }
    Ok(())
}
```

Note: `summary_keeps_only_the_latest_events` asserts `!s.contains("event-0;")`; the summary writes each event as `[date] text;` so `event-0;` is distinct from `event-10;`/`event-11;`. With 12 events and a window of 8, events 4 to 11 appear.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test scenario::ledger 2>&1 | tail -5`
Expected: `test result: ok. 7 passed`

- [ ] **Step 5: Format and commit**

```bash
rustfmt --edition 2021 src/scenario/ledger.rs src/scenario/mod.rs
git add src/scenario/ledger.rs src/scenario/mod.rs
git commit -m "feat(scenario): world ledger for a continuing story"
```

---

### Task 6: Docker seam, runtime start and runtime warning

**Files:**
- Create: `src/scenario/docker.rs`
- Modify: `src/scenario/mod.rs` (add `pub(crate) mod docker;`)

**Interfaces:**
- Produces:
  - `pub(crate) struct Out { pub code: i32, pub stdout: String, pub stderr: String }`; `Out::ok(&self) -> bool`; `Out::text(&self) -> String`
  - `pub(crate) trait Docker { fn run(&self, args: &[&str]) -> Result<Out>; }`
  - `pub(crate) struct RealDocker;` (implements `Docker`)
  - `pub(crate) enum Runtime { Ready, Stopped, NotInstalled }`, `pub(crate) fn probe(d: &dyn Docker) -> Runtime`
  - `pub(crate) fn available(d: &dyn Docker) -> Result<()>` (runtime must already be running; plain message otherwise)
  - `pub(crate) trait RuntimeStarter { fn start(&self) -> bool; }`, `pub(crate) struct SystemStarter;`
  - `pub(crate) const START_TIMEOUT: Duration` (90 s), `pub(crate) const START_POLL: Duration` (2 s)
  - `pub(crate) fn ensure_ready(d: &dyn Docker, s: &dyn RuntimeStarter, timeout: Duration, poll: Duration, sleep: &dyn Fn(Duration)) -> Result<()>` (starts the runtime if installed but stopped, waits with a timeout, plain messages on failure; not installed gives install guidance)
  - test helpers in `docker::fake`: `FakeDocker::new`, `FakeDocker::missing()` (every call is an error, as when the CLI is absent), `ok`, `fail`
  - `pub(crate) fn runc_is_old(version: &str) -> Option<bool>`
  - `pub(crate) fn runc_warning(d: &dyn Docker) -> Option<String>`

- [ ] **Step 0: Verify the runc fixed versions (blocking)**

Before writing `runc_is_old`, open the advisories for CVE-2025-31133, CVE-2025-52565 and CVE-2025-52881 (runc GitHub security advisories) and write down the first fixed release on each branch. This plan assumes 1.2.8, 1.3.3 and 1.4.0-rc.3. If the real numbers differ, change the match arms and the tests below to the verified numbers. If they cannot be verified, make `runc_is_old` return `None` for every input and drop the first three assertions in the test (the warning then never fires; Open question 9).

- [ ] **Step 1: Write the failing tests**

Create `src/scenario/docker.rs` with only:

```rust
#[cfg(test)]
pub(crate) mod fake {
    use std::cell::RefCell;

    use super::*;

    /// Records every call as one joined line and answers from a closure.
    pub(crate) struct FakeDocker {
        pub calls: RefCell<Vec<String>>,
        respond: Box<dyn Fn(&str) -> Out>,
        missing: bool,
    }

    impl FakeDocker {
        pub fn new(respond: impl Fn(&str) -> Out + 'static) -> FakeDocker {
            FakeDocker {
                calls: RefCell::new(vec![]),
                respond: Box::new(respond),
                missing: false,
            }
        }
        /// The docker CLI is not installed: every call errors.
        pub fn missing() -> FakeDocker {
            FakeDocker {
                calls: RefCell::new(vec![]),
                respond: Box::new(|_| Out { code: 0, stdout: String::new(), stderr: String::new() }),
                missing: true,
            }
        }
        pub fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl Docker for FakeDocker {
        fn run(&self, args: &[&str]) -> Result<Out> {
            let line = args.join(" ");
            self.calls.borrow_mut().push(line.clone());
            if self.missing {
                bail!("docker: command not found");
            }
            Ok((self.respond)(&line))
        }
    }

    pub(crate) fn ok(stdout: &str) -> Out {
        Out { code: 0, stdout: stdout.into(), stderr: String::new() }
    }

    pub(crate) fn fail(code: i32, stderr: &str) -> Out {
        Out { code, stdout: String::new(), stderr: stderr.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;

    #[test]
    fn runc_versions_before_the_fixes_are_old() {
        assert_eq!(runc_is_old("1.1.12"), Some(true));
        assert_eq!(runc_is_old("1.2.7"), Some(true));
        assert_eq!(runc_is_old("1.3.2"), Some(true));
        assert_eq!(runc_is_old("1.4.0-rc.2"), Some(true));
    }

    #[test]
    fn runc_versions_with_the_fixes_are_not_old() {
        assert_eq!(runc_is_old("1.2.8"), Some(false));
        assert_eq!(runc_is_old("v1.3.3"), Some(false));
        assert_eq!(runc_is_old("1.4.0-rc.3"), Some(false));
        assert_eq!(runc_is_old("1.4.0"), Some(false));
        assert_eq!(runc_is_old("1.5.1"), Some(false));
    }

    #[test]
    fn unparseable_runc_version_is_unknown() {
        assert_eq!(runc_is_old("weird"), None);
        assert_eq!(runc_is_old(""), None);
    }

    #[test]
    fn probe_tells_ready_stopped_and_not_installed_apart() {
        assert!(matches!(probe(&FakeDocker::new(|_| ok("27"))), Runtime::Ready));
        assert!(matches!(probe(&FakeDocker::new(|_| fail(1, "Cannot connect"))), Runtime::Stopped));
        assert!(matches!(probe(&FakeDocker::missing()), Runtime::NotInstalled));
    }

    #[test]
    fn available_has_plain_messages_for_both_failures() {
        assert!(available(&FakeDocker::new(|_| ok("27"))).is_ok());
        let stopped = available(&FakeDocker::new(|_| fail(1, "x"))).unwrap_err().to_string();
        assert!(stopped.contains("not running") && stopped.contains("/scenario start"), "{stopped}");
        let missing = available(&FakeDocker::missing()).unwrap_err().to_string();
        assert!(missing.contains("not installed"), "{missing}");
    }

    struct FakeStarter {
        started: std::cell::Cell<bool>,
        can: bool,
    }
    impl RuntimeStarter for FakeStarter {
        fn start(&self) -> bool {
            self.started.set(true);
            self.can
        }
    }

    const POLL: Duration = Duration::from_secs(2);
    const LIMIT: Duration = Duration::from_secs(10);

    #[test]
    fn a_ready_runtime_is_not_started_again() {
        let st = FakeStarter { started: Default::default(), can: true };
        ensure_ready(&FakeDocker::new(|_| ok("27")), &st, LIMIT, POLL, &|_| {}).unwrap();
        assert!(!st.started.get());
    }

    #[test]
    fn a_stopped_runtime_is_started_and_waited_for() {
        use std::cell::Cell;
        use std::rc::Rc;
        // First probe fails (stopped), then two more fail while it boots, then it answers.
        let n = Rc::new(Cell::new(0));
        let n2 = n.clone();
        let d = FakeDocker::new(move |_| {
            n2.set(n2.get() + 1);
            if n2.get() <= 3 { fail(1, "starting") } else { ok("27") }
        });
        let st = FakeStarter { started: Default::default(), can: true };
        let slept = Cell::new(0u32);
        ensure_ready(&d, &st, LIMIT, POLL, &|_| slept.set(slept.get() + 1)).unwrap();
        assert!(st.started.get());
        assert_eq!(slept.get(), 3);
    }

    #[test]
    fn a_runtime_that_never_comes_up_times_out_with_a_plain_message() {
        let st = FakeStarter { started: Default::default(), can: true };
        let slept = std::cell::Cell::new(0u32);
        let e = ensure_ready(&FakeDocker::new(|_| fail(1, "x")), &st, LIMIT, POLL, &|_| slept.set(slept.get() + 1))
            .unwrap_err()
            .to_string();
        assert!(e.contains("did not finish starting") && e.contains("try /scenario start again"), "{e}");
        assert_eq!(slept.get(), 5); // LIMIT / POLL, no real waiting in the test
    }

    #[test]
    fn a_runtime_that_cannot_be_launched_says_so_without_waiting() {
        let st = FakeStarter { started: Default::default(), can: false };
        let slept = std::cell::Cell::new(0u32);
        let e = ensure_ready(&FakeDocker::new(|_| fail(1, "x")), &st, LIMIT, POLL, &|_| slept.set(1))
            .unwrap_err()
            .to_string();
        assert!(e.contains("could not start it"), "{e}");
        assert_eq!(slept.get(), 0);
    }

    #[test]
    fn a_missing_runtime_gets_install_guidance_and_is_never_started() {
        let st = FakeStarter { started: Default::default(), can: true };
        let e = ensure_ready(&FakeDocker::missing(), &st, LIMIT, POLL, &|_| {}).unwrap_err().to_string();
        assert!(e.contains("not installed") && e.contains("https://docs.docker.com/get-docker/"), "{e}");
        assert!(!st.started.get());
    }

    #[test]
    fn runc_warning_fires_only_for_an_old_runc() {
        let old = FakeDocker::new(|_| ok(r#"[{"Name":"Engine","Version":"27"},{"Name":"runc","Version":"1.2.5"}]"#));
        assert!(runc_warning(&old).unwrap().contains("CVE-2025-31133"));
        let new = FakeDocker::new(|_| ok(r#"[{"Name":"runc","Version":"1.3.3"}]"#));
        assert!(runc_warning(&new).is_none());
        let none = FakeDocker::new(|_| ok(r#"[{"Name":"Engine","Version":"27"}]"#));
        assert!(runc_warning(&none).is_none());
        let broken = FakeDocker::new(|_| fail(1, "x"));
        assert!(runc_warning(&broken).is_none());
    }

    #[test]
    fn out_text_joins_both_streams() {
        let o = Out { code: 1, stdout: "a\n".into(), stderr: "b\n".into() };
        assert_eq!(o.text(), "a\nb");
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test scenario::docker 2>&1 | tail -5`
Expected: FAIL to compile.

- [ ] **Step 3: Write the implementation** (above the test modules)

```rust
//! Thin seam over the `docker` CLI so every command line is testable without Docker.

use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{bail, Context, Result};

pub(crate) struct Out {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Out {
    pub fn ok(&self) -> bool {
        self.code == 0
    }

    /// stdout then stderr, trimmed; for error messages and check output.
    pub fn text(&self) -> String {
        format!("{}{}", self.stdout, self.stderr).trim().to_string()
    }
}

pub(crate) trait Docker {
    fn run(&self, args: &[&str]) -> Result<Out>;
}

pub(crate) struct RealDocker;

impl Docker for RealDocker {
    fn run(&self, args: &[&str]) -> Result<Out> {
        let o = Command::new("docker")
            .args(args)
            .output()
            .context("could not run the docker command")?;
        Ok(Out {
            code: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        })
    }
}

pub(crate) enum Runtime {
    Ready,
    /// The CLI exists but the engine does not answer.
    Stopped,
    /// No docker CLI at all.
    NotInstalled,
}

pub(crate) fn probe(d: &dyn Docker) -> Runtime {
    match d.run(&["version", "--format", "{{.Server.Version}}"]) {
        Err(_) => Runtime::NotInstalled,
        Ok(o) if o.ok() => Runtime::Ready,
        Ok(_) => Runtime::Stopped,
    }
}

// Docker is named below on purpose: the learner cannot install or open it
// without the name. Everywhere else learner text says "task" or "server".
const NOT_INSTALLED: &str = "Practice tasks run on a small server that Usta starts for you, and that needs Docker. Docker is not installed on this computer. Install Docker Desktop from https://docs.docker.com/get-docker/ (on Linux, install your distribution's docker package), open it once, then run /scenario start again.";
const CANNOT_START: &str = "Usta needs Docker and it is installed, but Usta could not start it. Open Docker yourself, wait until it says it is running, then try /scenario start again.";
const START_TIMEOUT_MSG: &str = "Docker did not finish starting within 90 seconds. Wait until it says it is running, then try /scenario start again.";

/// The runtime must already be running (used by check, pause and the startup sweep).
pub(crate) fn available(d: &dyn Docker) -> Result<()> {
    match probe(d) {
        Runtime::Ready => Ok(()),
        Runtime::Stopped => bail!("Docker is not running. Run /scenario start, which starts it for you, or open Docker yourself."),
        Runtime::NotInstalled => bail!(NOT_INSTALLED),
    }
}

/// Tries to launch the stopped runtime. True = a launch was attempted.
pub(crate) trait RuntimeStarter {
    fn start(&self) -> bool;
}

pub(crate) const START_TIMEOUT: Duration = Duration::from_secs(90);
pub(crate) const START_POLL: Duration = Duration::from_secs(2);

pub(crate) struct SystemStarter;

/// Per-platform launch command. Unverified on real systems (plan, constraint 6);
/// a failure here only produces the plain "could not start it" message.
fn start_command() -> (&'static str, Vec<&'static str>) {
    if cfg!(target_os = "macos") {
        ("open", vec!["-a", "Docker"])
    } else if cfg!(target_os = "windows") {
        (
            "cmd",
            vec!["/c", "start", "", "C:\\Program Files\\Docker\\Docker\\Docker Desktop.exe"],
        )
    } else {
        ("systemctl", vec!["start", "docker"])
    }
}

impl RuntimeStarter for SystemStarter {
    fn start(&self) -> bool {
        let (program, args) = start_command();
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
    }
}

/// Make the runtime ready: nothing to do if it runs, start it and wait (up to
/// `timeout`, polling every `poll`) if it is installed but stopped, install
/// guidance if it is missing. `sleep` is injected so tests do not wait.
pub(crate) fn ensure_ready(
    d: &dyn Docker,
    s: &dyn RuntimeStarter,
    timeout: Duration,
    poll: Duration,
    sleep: &dyn Fn(Duration),
) -> Result<()> {
    match probe(d) {
        Runtime::Ready => return Ok(()),
        Runtime::NotInstalled => bail!(NOT_INSTALLED),
        Runtime::Stopped => {}
    }
    if !s.start() {
        bail!(CANNOT_START);
    }
    let mut waited = Duration::ZERO;
    while waited < timeout {
        sleep(poll);
        waited += poll;
        if matches!(probe(d), Runtime::Ready) {
            return Ok(());
        }
    }
    bail!(START_TIMEOUT_MSG)
}

/// Whether a runc version predates the November 2025 container-escape fixes
/// (CVE-2025-31133, CVE-2025-52565, CVE-2025-52881). None = unparseable.
/// Fixed releases per branch were verified in Task 6 Step 0.
pub(crate) fn runc_is_old(version: &str) -> Option<bool> {
    let v = version.trim().trim_start_matches('v');
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (v, None),
    };
    let mut it = core.split('.').map(|n| n.parse::<u32>().ok());
    let (maj, min, pat) = (it.next()??, it.next()??, it.next()??);
    Some(match (maj, min) {
        (0, _) | (1, 0) | (1, 1) => true,
        (1, 2) => pat < 8,
        (1, 3) => pat < 3,
        (1, 4) => {
            pat == 0
                && pre.is_some_and(|p| {
                    p.strip_prefix("rc.")
                        .and_then(|n| n.parse::<u32>().ok())
                        .is_none_or(|n| n < 3)
                })
        }
        _ => false,
    })
}

/// Warning text when the engine's runc is old; None when fine or unknown.
pub(crate) fn runc_warning(d: &dyn Docker) -> Option<String> {
    let o = d
        .run(&["version", "--format", "{{json .Server.Components}}"])
        .ok()
        .filter(Out::ok)?;
    let comps: serde_json::Value = serde_json::from_str(o.stdout.trim()).ok()?;
    let v = comps
        .as_array()?
        .iter()
        .find(|c| c["Name"] == "runc")?["Version"]
        .as_str()?;
    runc_is_old(v)?.then(|| {
        format!(
            "The software that runs your practice tasks (Docker, runc {v}) is older than the November 2025 security fixes (CVE-2025-31133, CVE-2025-52565, CVE-2025-52881). Update Docker before you continue."
        )
    })
}
```

(`Option::is_none_or` needs Rust 1.82; the crate floor is 1.88.)

- [ ] **Step 4: Run to verify pass**

Run: `cargo test scenario::docker 2>&1 | tail -5`
Expected: `test result: ok. 6 passed`

- [ ] **Step 5: Format and commit**

```bash
rustfmt --edition 2021 src/scenario/docker.rs src/scenario/mod.rs
git add src/scenario/docker.rs src/scenario/mod.rs
git commit -m "feat(scenario): docker seam and runc version warning"
```

---

### Task 7: Container environment (the five operations)

**Files:**
- Create: `src/scenario/container.rs`
- Modify: `src/scenario/mod.rs` (add `pub(crate) mod container;`)

**Interfaces:**
- Consumes: `Docker`, `Out` (Task 6, plus `fake` test helpers); `Environment`, `EnvHandle`, `CheckOutcome` (Task 2); `Package` (Task 1)
- Produces:
  - `pub(crate) struct ContainerEnv<'a> { pub docker: &'a dyn Docker, pub project_tag: String }` implementing `Environment`
  - `pub(crate) const CHECK_DIR: &str = "/opt/.usta-check"`, `SOLUTION_DIR: &str = "/opt/.usta-solution"`
  - `ContainerEnv::image(pkg: &Package) -> String`
  - `ContainerEnv::apply_solution(&self, pkg: &Package, h: &EnvHandle) -> Result<()>` (certification only, deliberately NOT part of the five operations)
  - `pub(crate) struct Found { pub name: String, pub state: String, pub run_id: String }`
  - `pub(crate) fn list_labeled(d: &dyn Docker, project_tag: &str) -> Result<Vec<Found>>`
  - `pub(crate) fn tail(text: &str, lines: usize, max_chars: usize) -> String`

- [ ] **Step 1: Write the failing tests**

Create `src/scenario/container.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::docker::fake::{fail, ok, FakeDocker};
    use crate::scenario::package::{CheckSpec, EnvSpec, StorySpec};
    use std::path::PathBuf;

    fn pkg() -> Package {
        Package {
            id: "demo-pkg".into(),
            topic: "linux".into(),
            title: "Demo".into(),
            version: 3,
            skills: vec!["permission".into()],
            environment: EnvSpec {
                kind: "container".into(),
                context: "environment".into(),
                setup: Some("/usr/local/lib/usta/setup.sh".into()),
                start: Some("/usr/local/lib/usta/start.sh".into()),
                network: false,
                memory: "256m".into(),
                cpus: "0.5".into(),
                pids: 128,
            },
            check: CheckSpec { kind: "script".into(), script: "check.sh".into(), timeout_secs: 30 },
            story: StorySpec::default(),
            dir: PathBuf::from("/lib/demo-pkg"),
        }
    }

    fn handle() -> EnvHandle {
        EnvHandle { reference: "usta-aaaa1111-demo-1".into(), access: "x".into() }
    }

    #[test]
    fn prepare_builds_a_missing_image_creates_with_limits_then_starts_and_runs_scripts() {
        let d = FakeDocker::new(|l| if l.starts_with("image inspect") { fail(1, "none") } else { ok("") });
        let mut env = ContainerEnv { docker: &d, project_tag: "aaaa1111".into() };
        let h = env.prepare(&pkg(), "demo-1").unwrap();
        assert_eq!(h.reference, "usta-aaaa1111-demo-1");
        assert_eq!(h.access, "docker exec -it usta-aaaa1111-demo-1 bash");
        let calls = d.calls();
        let idx = |needle: &str| calls.iter().position(|c| c.contains(needle)).unwrap_or_else(|| panic!("missing {needle} in {calls:?}"));
        assert!(calls[idx("build")].starts_with("build -t usta-scn-demo-pkg:v3 /lib/demo-pkg/environment"));
        let create = &calls[idx("create")];
        for want in ["--name usta-aaaa1111-demo-1", "--label usta=1", "--label usta.project=aaaa1111", "--label usta.run=demo-1", "--memory 256m", "--cpus 0.5", "--pids-limit 128", "--network none", "usta-scn-demo-pkg:v3 sleep infinity"] {
            assert!(create.contains(want), "{want} not in {create}");
        }
        assert!(idx("create") < idx("start usta-aaaa1111-demo-1"));
        assert!(idx("start usta-aaaa1111-demo-1") < idx("sh /usr/local/lib/usta/setup.sh"));
        assert!(idx("sh /usr/local/lib/usta/setup.sh") < idx("sh /usr/local/lib/usta/start.sh"));
    }

    #[test]
    fn containers_are_never_created_with_rm() {
        let d = FakeDocker::new(|_| ok(""));
        let mut env = ContainerEnv { docker: &d, project_tag: "aaaa1111".into() };
        env.prepare(&pkg(), "demo-1").unwrap();
        assert!(d.calls().iter().all(|c| !c.contains("--rm")));
    }

    #[test]
    fn existing_image_skips_the_build_and_network_true_omits_none() {
        let d = FakeDocker::new(|_| ok(""));
        let mut env = ContainerEnv { docker: &d, project_tag: "aaaa1111".into() };
        let mut p = pkg();
        p.environment.network = true;
        env.prepare(&p, "demo-1").unwrap();
        let calls = d.calls();
        assert!(!calls.iter().any(|c| c.starts_with("build")));
        assert!(!calls.iter().any(|c| c.contains("--network")));
    }

    #[test]
    fn prepare_refuses_while_another_project_runs_a_scenario() {
        let d = FakeDocker::new(|l| if l.starts_with("ps ") { ok("usta-zzzz9999-other-1\n") } else { ok("") });
        let mut env = ContainerEnv { docker: &d, project_tag: "aaaa1111".into() };
        let e = env.prepare(&pkg(), "demo-1").unwrap_err().to_string();
        assert!(e.contains("another Usta project") && !e.contains("usta-"), "{e}");
        assert!(!d.calls().iter().any(|c| c.starts_with("create")));
    }

    #[test]
    fn our_own_running_container_does_not_block() {
        let d = FakeDocker::new(|l| if l.starts_with("ps ") { ok("usta-aaaa1111-old-1\n") } else { ok("") });
        let mut env = ContainerEnv { docker: &d, project_tag: "aaaa1111".into() };
        assert!(env.prepare(&pkg(), "demo-1").is_ok());
    }

    #[test]
    fn a_failing_setup_removes_the_half_built_container() {
        let d = FakeDocker::new(|l| if l.contains("setup.sh") { fail(1, "bad") } else { ok("") });
        let mut env = ContainerEnv { docker: &d, project_tag: "aaaa1111".into() };
        assert!(env.prepare(&pkg(), "demo-1").is_err());
        assert!(d.calls().last().unwrap().starts_with("rm -f usta-aaaa1111-demo-1"));
    }

    #[test]
    fn pause_stops_and_resume_starts_then_reruns_start_script_only() {
        let d = FakeDocker::new(|_| ok(""));
        let mut env = ContainerEnv { docker: &d, project_tag: "aaaa1111".into() };
        env.pause(&handle()).unwrap();
        env.resume(&pkg(), &handle()).unwrap();
        let calls = d.calls();
        assert_eq!(calls[0], "stop -t 2 usta-aaaa1111-demo-1");
        assert!(calls.contains(&"start usta-aaaa1111-demo-1".to_string()));
        assert!(calls.iter().any(|c| c.ends_with("sh /usr/local/lib/usta/start.sh")));
        assert!(!calls.iter().any(|c| c.ends_with("setup.sh")));
    }

    fn running(l: &str) -> Option<Out> {
        l.starts_with("inspect").then(|| ok("true\n"))
    }

    #[test]
    fn check_copies_tests_in_runs_with_timeout_and_always_removes_them() {
        let d = FakeDocker::new(|l| running(l).unwrap_or_else(|| ok("")));
        let mut env = ContainerEnv { docker: &d, project_tag: "aaaa1111".into() };
        let out = env.check(&pkg(), &handle()).unwrap();
        assert_eq!(out, CheckOutcome::Pass);
        let calls = d.calls();
        assert_eq!(calls[1], "cp /lib/demo-pkg/tests/. usta-aaaa1111-demo-1:/opt/.usta-check");
        assert_eq!(calls[2], "exec usta-aaaa1111-demo-1 timeout 30 sh /opt/.usta-check/check.sh");
        assert_eq!(calls[3], "exec usta-aaaa1111-demo-1 rm -rf /opt/.usta-check");
    }

    #[test]
    fn check_maps_exit_codes_and_still_cleans_up() {
        let d = FakeDocker::new(|l| {
            if let Some(o) = running(l) { return o }
            if l.contains("check.sh") { return Out { code: 1, stdout: "script still fails\n".into(), stderr: String::new() } }
            ok("")
        });
        let mut env = ContainerEnv { docker: &d, project_tag: "aaaa1111".into() };
        assert_eq!(env.check(&pkg(), &handle()).unwrap(), CheckOutcome::Fail { output: "script still fails".into() });
        assert!(d.calls().last().unwrap().ends_with("rm -rf /opt/.usta-check"));

        let t = FakeDocker::new(|l| {
            if let Some(o) = running(l) { return o }
            if l.contains("check.sh") { return fail(124, "") }
            ok("")
        });
        let mut env = ContainerEnv { docker: &t, project_tag: "aaaa1111".into() };
        match env.check(&pkg(), &handle()).unwrap() {
            CheckOutcome::Fail { output } => assert!(output.contains("timed out"), "{output}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn check_requires_a_running_container() {
        let d = FakeDocker::new(|l| if l.starts_with("inspect") { ok("false\n") } else { ok("") });
        let mut env = ContainerEnv { docker: &d, project_tag: "aaaa1111".into() };
        let e = env.check(&pkg(), &handle()).unwrap_err().to_string();
        assert!(e.contains("/scenario resume"), "{e}");
        assert!(!d.calls().iter().any(|c| c.starts_with("cp")));
    }

    #[test]
    fn clean_removes_and_treats_missing_as_success() {
        let d = FakeDocker::new(|_| fail(1, "Error: No such container: x"));
        let mut env = ContainerEnv { docker: &d, project_tag: "aaaa1111".into() };
        assert!(env.clean(&handle()).is_ok());
        let d2 = FakeDocker::new(|_| fail(1, "permission denied"));
        let mut env2 = ContainerEnv { docker: &d2, project_tag: "aaaa1111".into() };
        assert!(env2.clean(&handle()).is_err());
        assert_eq!(d.calls()[0], "rm -f usta-aaaa1111-demo-1");
    }

    #[test]
    fn list_labeled_filters_by_project_label_only() {
        let d = FakeDocker::new(|_| ok("usta-aaaa1111-a-1|running|a-1\nusta-aaaa1111-b-2|exited|b-2\n"));
        let found = list_labeled(&d, "aaaa1111").unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[1], Found { name: "usta-aaaa1111-b-2".into(), state: "exited".into(), run_id: "b-2".into() });
        let call = &d.calls()[0];
        assert!(call.contains("label=usta=1") && call.contains("label=usta.project=aaaa1111"), "{call}");
    }

    #[test]
    fn tail_keeps_last_lines_and_caps_characters() {
        assert_eq!(tail("a\nb\nc\nd", 2, 100), "c\nd");
        assert_eq!(tail("abcdefghij", 5, 4), "ghij");
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test scenario::container 2>&1 | tail -5`
Expected: FAIL to compile.

- [ ] **Step 3: Write the implementation** (above the tests)

`Found` needs `#[derive(Debug, PartialEq)]`; `CheckOutcome` already derives `PartialEq`/`Debug`.

```rust
//! The container environment: the five operations over the `docker` CLI.
//! Lifecycle (spec): never `--rm`; one task one container; memory/cpu/pids
//! limits; network off unless declared; `usta-` names plus `usta=1` and
//! `usta.project=<tag>` labels so Usta only ever touches its own containers.

use anyhow::{bail, Result};

use crate::scenario::docker::{Docker, Out};
use crate::scenario::env::{internal, CheckOutcome, EnvHandle, Environment};
use crate::scenario::package::Package;

/// Check scripts enter the container only here, at check time, and are removed right after.
pub(crate) const CHECK_DIR: &str = "/opt/.usta-check";
/// Reference solution location, certification only.
pub(crate) const SOLUTION_DIR: &str = "/opt/.usta-solution";

pub(crate) struct ContainerEnv<'a> {
    pub docker: &'a dyn Docker,
    pub project_tag: String,
}

impl ContainerEnv<'_> {
    fn name(&self, run_id: &str) -> String {
        format!("usta-{}-{run_id}", self.project_tag)
    }

    pub fn image(pkg: &Package) -> String {
        format!("usta-scn-{}:v{}", pkg.id, pkg.version)
    }

    fn must(&self, args: &[&str]) -> Result<Out> {
        let o = self.docker.run(args)?;
        if o.ok() {
            Ok(o)
        } else {
            Err(internal(format!(
                "docker {} failed: {}",
                args.first().unwrap_or(&""),
                o.text()
            )))
        }
    }

    /// Refuse while ANOTHER Usta project has a scenario container running.
    /// Our own project's running containers are paused by the engine first.
    fn guard_single(&self) -> Result<()> {
        let o = self.must(&[
            "ps",
            "--filter",
            "label=usta=1",
            "--filter",
            "status=running",
            "--format",
            "{{.Names}}",
        ])?;
        let mine = format!("usta-{}-", self.project_tag);
        if o.stdout
            .lines()
            .map(str::trim)
            .any(|n| !n.is_empty() && !n.starts_with(&mine))
        {
            bail!("another Usta project already has a practice task running; pause it there first");
        }
        Ok(())
    }

    fn run_script(&self, name: &str, path: &str) -> Result<()> {
        self.must(&["exec", name, "sh", path]).map(|_| ())
    }

    fn is_running(&self, name: &str) -> bool {
        self.docker
            .run(&["inspect", "-f", "{{.State.Running}}", name])
            .map(|o| o.ok() && o.stdout.trim() == "true")
            .unwrap_or(false)
    }

    /// Certification only (not one of the five operations): run the reference solution inside.
    pub fn apply_solution(&self, pkg: &Package, h: &EnvHandle) -> Result<()> {
        let src = format!("{}/.", pkg.dir.join("solution").display());
        let dest = format!("{}:{SOLUTION_DIR}", h.reference);
        self.must(&["cp", &src, &dest])?;
        let run = self.run_script(&h.reference, &format!("{SOLUTION_DIR}/solve.sh"));
        let _ = self.docker.run(&["exec", &h.reference, "rm", "-rf", SOLUTION_DIR]);
        run
    }
}

impl Environment for ContainerEnv<'_> {
    fn prepare(&mut self, pkg: &Package, run_id: &str) -> Result<EnvHandle> {
        self.guard_single()?;
        let image = Self::image(pkg);
        if !self.docker.run(&["image", "inspect", &image])?.ok() {
            let ctx = pkg.dir.join(&pkg.environment.context);
            self.must(&["build", "-t", &image, &ctx.to_string_lossy()])?;
        }
        let name = self.name(run_id);
        let project_label = format!("usta.project={}", self.project_tag);
        let run_label = format!("usta.run={run_id}");
        let pids = pkg.environment.pids.to_string();
        let mut args: Vec<&str> = vec![
            "create",
            "--name",
            name.as_str(),
            "--label",
            "usta=1",
            "--label",
            project_label.as_str(),
            "--label",
            run_label.as_str(),
            "--memory",
            pkg.environment.memory.as_str(),
            "--cpus",
            pkg.environment.cpus.as_str(),
            "--pids-limit",
            pids.as_str(),
        ];
        if !pkg.environment.network {
            args.extend(["--network", "none"]);
        }
        args.extend([image.as_str(), "sleep", "infinity"]);
        self.must(&args)?;
        let up = (|| -> Result<()> {
            self.must(&["start", &name])?;
            if let Some(s) = &pkg.environment.setup {
                self.run_script(&name, s)?;
            }
            if let Some(s) = &pkg.environment.start {
                self.run_script(&name, s)?;
            }
            Ok(())
        })();
        if let Err(e) = up {
            let _ = self.docker.run(&["rm", "-f", &name]);
            return Err(e);
        }
        Ok(EnvHandle {
            access: format!("docker exec -it {name} bash"),
            reference: name,
        })
    }

    fn pause(&mut self, h: &EnvHandle) -> Result<()> {
        self.must(&["stop", "-t", "2", &h.reference]).map(|_| ())
    }

    fn resume(&mut self, pkg: &Package, h: &EnvHandle) -> Result<()> {
        self.guard_single()?;
        self.must(&["start", &h.reference])?;
        if let Some(s) = &pkg.environment.start {
            self.run_script(&h.reference, s)?;
        }
        Ok(())
    }

    fn check(&mut self, pkg: &Package, h: &EnvHandle) -> Result<CheckOutcome> {
        if !self.is_running(&h.reference) {
            bail!("the task is not running; use /scenario resume");
        }
        let src = format!("{}/.", pkg.dir.join("tests").display());
        let dest = format!("{}:{CHECK_DIR}", h.reference);
        self.must(&["cp", &src, &dest])?;
        let script = format!("{CHECK_DIR}/{}", pkg.check.script);
        let secs = pkg.check.timeout_secs.to_string();
        let run = self
            .docker
            .run(&["exec", &h.reference, "timeout", &secs, "sh", &script]);
        let _ = self.docker.run(&["exec", &h.reference, "rm", "-rf", CHECK_DIR]);
        let o = run?;
        Ok(match o.code {
            0 => CheckOutcome::Pass,
            124 => CheckOutcome::Fail {
                output: format!("the check timed out after {secs}s"),
            },
            _ => CheckOutcome::Fail {
                output: tail(&o.text(), 20, 1500),
            },
        })
    }

    fn clean(&mut self, h: &EnvHandle) -> Result<()> {
        let o = self.docker.run(&["rm", "-f", &h.reference])?;
        if o.ok() || o.text().contains("No such container") {
            Ok(())
        } else {
            Err(internal(format!("docker rm failed: {}", o.text())))
        }
    }
}

/// A container carrying this project's label.
#[derive(Debug, PartialEq)]
pub(crate) struct Found {
    pub name: String,
    pub state: String,
    pub run_id: String,
}

/// Containers labelled `usta=1` AND this project's tag, running or not. Never anything else.
pub(crate) fn list_labeled(d: &dyn Docker, project_tag: &str) -> Result<Vec<Found>> {
    let project_filter = format!("label=usta.project={project_tag}");
    let o = d.run(&[
        "ps",
        "-a",
        "--filter",
        "label=usta=1",
        "--filter",
        &project_filter,
        "--format",
        "{{.Names}}|{{.State}}|{{.Label \"usta.run\"}}",
    ])?;
    if !o.ok() {
        return Err(internal(format!("docker ps failed: {}", o.text())));
    }
    Ok(o.stdout
        .lines()
        .filter_map(|l| {
            let mut p = l.trim().splitn(3, '|');
            Some(Found {
                name: p.next()?.to_string(),
                state: p.next()?.to_string(),
                run_id: p.next()?.to_string(),
            })
        })
        .collect())
}

/// Last `lines` lines of `text`, then at most `max_chars` trailing characters.
pub(crate) fn tail(text: &str, lines: usize, max_chars: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    let joined = all[all.len().saturating_sub(lines)..].join("\n");
    let n = joined.chars().count();
    joined.chars().skip(n.saturating_sub(max_chars)).collect()
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test scenario::container 2>&1 | tail -5`
Expected: `test result: ok. 13 passed`

- [ ] **Step 5: Format and commit**

```bash
rustfmt --edition 2021 src/scenario/container.rs src/scenario/mod.rs
git add src/scenario/container.rs src/scenario/mod.rs
git commit -m "feat(scenario): container environment with full lifecycle and check rules"
```

---

### Task 8: Terminal window inside the task

The learner never sees or types a Docker command. On start and resume Usta opens a new terminal window already inside the task. This is a platform seam with a fake launcher in tests; if no window can be opened, the command is shown (spec: "No work for the learner").

**Chosen methods (all unverified on real systems until Task 14's manual step):**
- **macOS:** write an executable `window-<run>.command` file into `<project>/.usta/scenarios/` and `open` it (LaunchServices runs it in the default terminal app). This does **not** use Apple Events, so it should not trigger the macOS Automation permission prompt ("Usta wants to control Terminal") that `osascript -e 'tell application "Terminal" ...'` would trigger on first use. If a prompt or a block appears anyway, the fallback command is shown.
- **Linux:** needs `DISPLAY` or `WAYLAND_DISPLAY`, then the first of `x-terminal-emulator`, `gnome-terminal`, `konsole`, `xfce4-terminal`, `xterm` found on `PATH`.
- **Windows:** `cmd /c start "Usta task" cmd /k <command>`.
- **Anything else, or the launch failed:** fallback text with the command.

**Files:**
- Create: `src/scenario/terminal.rs`
- Modify: `src/scenario/mod.rs` (add `pub(crate) mod terminal;`)

**Interfaces:**
- Consumes: nothing from earlier tasks (`EnvHandle::access` is passed in as a string)
- Produces:
  - `pub(crate) struct WindowReq<'a> { pub command_line: &'a str, pub run_id: &'a str, pub scratch: &'a Path }`
  - `pub(crate) trait Launcher { fn open(&self, req: &WindowReq) -> bool; }` (true = a window opened)
  - `pub(crate) struct SystemLauncher;` (implements `Launcher`)
  - `pub(crate) enum Plan { CommandFile, Spawn { program: String, args: Vec<String> } }` and `pub(crate) fn plan(os: &str, on_path: &dyn Fn(&str) -> bool, display: bool, command_line: &str) -> Option<Plan>`
  - `pub(crate) fn command_file_text(command_line: &str) -> String`
  - `pub(crate) fn window_notice(l: &dyn Launcher, req: &WindowReq) -> String`

- [ ] **Step 1: Write the failing tests**

Create `src/scenario/terminal.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = "docker exec -it usta-aaaa1111-demo-1 bash";

    fn none(_: &str) -> bool {
        false
    }

    #[test]
    fn macos_uses_a_command_file() {
        assert_eq!(plan("macos", &none, false, LINE), Some(Plan::CommandFile));
    }

    #[test]
    fn linux_needs_a_display_and_a_known_terminal() {
        assert_eq!(plan("linux", &|_| true, false, LINE), None);
        assert_eq!(plan("linux", &none, true, LINE), None);
    }

    #[test]
    fn linux_picks_the_first_known_terminal_in_order() {
        let only_konsole_and_xterm = |p: &str| p == "konsole" || p == "xterm";
        match plan("linux", &only_konsole_and_xterm, true, LINE).unwrap() {
            Plan::Spawn { program, args } => {
                assert_eq!(program, "konsole");
                assert_eq!(args, vec!["-e", "sh", "-c", LINE]);
            }
            other => panic!("{other:?}"),
        }
        match plan("linux", &|p| p == "gnome-terminal", true, LINE).unwrap() {
            Plan::Spawn { args, .. } => assert_eq!(args[0], "--"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn windows_starts_a_new_cmd_window() {
        match plan("windows", &none, false, LINE).unwrap() {
            Plan::Spawn { program, args } => {
                assert_eq!(program, "cmd");
                assert_eq!(&args[..5], ["/c", "start", "Usta task", "cmd", "/k"]);
                assert_eq!(args[5..].join(" "), LINE);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn unknown_systems_have_no_plan() {
        assert_eq!(plan("plan9", &|_| true, true, LINE), None);
    }

    #[test]
    fn the_command_file_runs_the_command_in_place_of_the_shell() {
        let t = command_file_text(LINE);
        assert!(t.starts_with("#!/bin/sh\n"));
        assert!(t.contains(&format!("exec {LINE}")));
    }

    #[cfg(unix)]
    #[test]
    fn the_command_file_is_written_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("usta_scn_term_{}", std::process::id()));
        let req = WindowReq { command_line: LINE, run_id: "demo-1", scratch: &dir };
        let path = write_command_file(&req).unwrap();
        assert_eq!(path.file_name().unwrap(), "window-demo-1.command");
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o755, 0o755);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), command_file_text(LINE));
        let _ = std::fs::remove_dir_all(dir);
    }

    struct FakeLauncher {
        opens: bool,
        seen: std::cell::RefCell<Vec<String>>,
    }
    impl Launcher for FakeLauncher {
        fn open(&self, req: &WindowReq) -> bool {
            self.seen.borrow_mut().push(req.command_line.to_string());
            self.opens
        }
    }

    fn req() -> WindowReq<'static> {
        WindowReq { command_line: LINE, run_id: "demo-1", scratch: Path::new("/tmp/x") }
    }

    #[test]
    fn success_notice_never_shows_the_command_or_infrastructure_words() {
        let l = FakeLauncher { opens: true, seen: Default::default() };
        let n = window_notice(&l, &req());
        assert_eq!(l.seen.borrow().as_slice(), [LINE]);
        let low = n.to_lowercase();
        assert!(n.contains("terminal window") && !low.contains("docker") && !low.contains("container") && !low.contains("image"), "{n}");
    }

    #[test]
    fn fallback_notice_shows_the_command() {
        let l = FakeLauncher { opens: false, seen: Default::default() };
        let n = window_notice(&l, &req());
        assert!(n.contains("could not open a terminal window") && n.contains(LINE), "{n}");
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test scenario::terminal 2>&1 | tail -5`
Expected: FAIL to compile.

- [ ] **Step 3: Write the implementation** (above the tests)

```rust
//! Opens a new terminal window already inside the task, so the learner never
//! types a Docker command. Platform seam: `plan` is pure and tested for every
//! system; `SystemLauncher` executes the plan; tests use a fake `Launcher`.

use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) struct WindowReq<'a> {
    /// Command line that opens a shell inside the task (`EnvHandle::access`).
    pub command_line: &'a str,
    pub run_id: &'a str,
    /// Folder for the macOS launcher script (`<project>/.usta/scenarios`).
    pub scratch: &'a Path,
}

pub(crate) trait Launcher {
    /// True when a window was opened.
    fn open(&self, req: &WindowReq) -> bool;
}

#[derive(Debug, PartialEq)]
pub(crate) enum Plan {
    /// macOS: write a `.command` file and `open` it. No Apple Events, so no Automation prompt.
    CommandFile,
    Spawn { program: String, args: Vec<String> },
}

/// Known Linux terminals in preference order, with the arguments that precede the command.
const LINUX_TERMINALS: [(&str, &[&str]); 5] = [
    ("x-terminal-emulator", &["-e"]),
    ("gnome-terminal", &["--"]),
    ("konsole", &["-e"]),
    ("xfce4-terminal", &["-x"]),
    ("xterm", &["-e"]),
];

pub(crate) fn plan(
    os: &str,
    on_path: &dyn Fn(&str) -> bool,
    display: bool,
    command_line: &str,
) -> Option<Plan> {
    match os {
        "macos" => Some(Plan::CommandFile),
        "windows" => {
            let mut args: Vec<String> = ["/c", "start", "Usta task", "cmd", "/k"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            args.extend(command_line.split_whitespace().map(String::from));
            Some(Plan::Spawn { program: "cmd".into(), args })
        }
        "linux" if display => LINUX_TERMINALS.iter().find(|(p, _)| on_path(p)).map(|(p, pre)| {
            let mut args: Vec<String> = pre.iter().map(|s| s.to_string()).collect();
            args.extend(["sh".into(), "-c".into(), command_line.to_string()]);
            Plan::Spawn { program: p.to_string(), args }
        }),
        _ => None,
    }
}

pub(crate) fn command_file_text(command_line: &str) -> String {
    format!("#!/bin/sh\nclear\nexec {command_line}\n")
}

#[cfg(unix)]
fn write_command_file(req: &WindowReq) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = req.scratch.join(format!("window-{}.command", req.run_id));
    std::fs::create_dir_all(req.scratch).ok()?;
    std::fs::write(&path, command_file_text(req.command_line)).ok()?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).ok()?;
    Some(path)
}

#[cfg(not(unix))]
fn write_command_file(_req: &WindowReq) -> Option<PathBuf> {
    None
}

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

pub(crate) struct SystemLauncher;

impl Launcher for SystemLauncher {
    fn open(&self, req: &WindowReq) -> bool {
        let display =
            std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
        match plan(std::env::consts::OS, &on_path, display, req.command_line) {
            Some(Plan::CommandFile) => write_command_file(req).is_some_and(|f| {
                Command::new("open")
                    .arg(f)
                    .status()
                    .is_ok_and(|s| s.success())
            }),
            Some(Plan::Spawn { program, args }) => Command::new(program).args(args).spawn().is_ok(),
            None => false,
        }
    }
}

/// What the learner reads after start or resume. Only the fallback names the
/// command (spec: "falls back to showing the command").
pub(crate) fn window_notice(l: &dyn Launcher, req: &WindowReq) -> String {
    if l.open(req) {
        "A terminal window opened inside your task. Work there.".to_string()
    } else {
        format!(
            "Usta could not open a terminal window on this system. Open a terminal yourself and run: {}",
            req.command_line
        )
    }
}
```

The test `the_command_file_is_written_executable` calls `write_command_file`, which is private; the test module is a child so it can see it.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test scenario::terminal 2>&1 | tail -5`
Expected: `test result: ok. 10 passed` (9 on non-unix).

- [ ] **Step 5: Format and commit**

```bash
rustfmt --edition 2021 src/scenario/terminal.rs src/scenario/mod.rs
git add src/scenario/terminal.rs src/scenario/mod.rs
git commit -m "feat(scenario): open a terminal window inside the task, with fallback"
```

---

### Task 9: Library mechanism and the first scenario package

**Files:**
- Create: `src/scenario/library.rs`
- Create: `scenarios/linux-backup-permissions/{task.toml,instruction.md,tone/adventure.md,environment/Dockerfile,environment/setup.sh,solution/solve.sh,tests/check.sh}`
- Modify: `src/scenario/mod.rs` (add `pub(crate) mod library;`)

**Interfaces:**
- Consumes: `Package` (Task 1); `crate::config::needs_sync(path, embedded) -> bool`
- Produces:
  - `pub(crate) fn library_root(global: &Path) -> PathBuf` (`<global>/scenarios/library`)
  - `pub(crate) fn sync(global: &Path) -> Result<PathBuf>` (writes embedded packages that are missing or differ; returns the library root)
  - `pub(crate) fn load_all(global: &Path, topic: &str) -> Result<Vec<Package>>` (packages whose `topic` matches, sorted by id; a package that fails to load is skipped with no panic)
  - `pub(crate) fn embedded_ids() -> Vec<&'static str>`

- [ ] **Step 1: Create the package files**

`scenarios/linux-backup-permissions/task.toml`:

```toml
id = "linux-backup-permissions"
topic = "linux"
title = "The backup that cannot write"
version = 1
skills = ["permission", "ownership"]

[environment]
kind = "container"
setup = "/usr/local/lib/usta/setup.sh"

[check]
kind = "script"
script = "check.sh"
timeout_secs = 30

[story]
company = "Shopwell"
event = "The nightly backup was repaired: the backup user can write to the backup folder again."

[[story.characters]]
name = "Deniz"
role = "runs the servers at night"

[[story.opens]]
id = "backups-ok"
text = "Backups run every night again."
```

`scenarios/linux-backup-permissions/instruction.md`:

```markdown
The nightly backup script `/opt/app/backup.sh` runs as the user `deniz` and fails with a permission error.

Make it work: when `deniz` runs it, an archive containing `customers.txt` must appear in `/srv/backups`.

Do not edit the script and do not make `/srv/backups` writable for everyone.
```

`scenarios/linux-backup-permissions/tone/adventure.md`:

```markdown
## open
03:12. Deniz writes: "The nightly backup died again, and the client wants last night's data before nine."

Your task: make `/opt/app/backup.sh` work when `deniz` runs it, so an archive containing `customers.txt` lands in `/srv/backups`. Leave the script alone and do not open the folder to everyone.

## close
03:41. Deniz: "The archive is there. I can sleep." Tomorrow night the backup runs on its own, and you know why it failed.
```

`scenarios/linux-backup-permissions/environment/Dockerfile`:

```dockerfile
FROM debian:12-slim
RUN useradd -m -s /bin/bash deniz
COPY setup.sh /usr/local/lib/usta/
WORKDIR /root
```

`scenarios/linux-backup-permissions/environment/setup.sh`:

```sh
#!/bin/sh
set -e
mkdir -p /opt/app/data /srv/backups
echo "customer list" > /opt/app/data/customers.txt
cat > /opt/app/backup.sh <<'EOF'
#!/bin/sh
if tar -czf "/srv/backups/app-$(date +%F).tgz" -C /opt/app data 2>/tmp/backup.err; then
  echo "backup ok"
else
  echo "backup failed:"
  cat /tmp/backup.err
  exit 1
fi
EOF
chmod 755 /opt/app/backup.sh
chmod -R a+rX /opt/app
chown root:root /srv/backups
chmod 755 /srv/backups
```

`scenarios/linux-backup-permissions/solution/solve.sh`:

```sh
#!/bin/sh
chown deniz:deniz /srv/backups
```

`scenarios/linux-backup-permissions/tests/check.sh`:

```sh
#!/bin/sh
# Runs inside the container only at check time. Checks system state, not commands typed.
rm -f /srv/backups/app-*.tgz
if ! runuser -u deniz -- /opt/app/backup.sh >/dev/null 2>&1; then
  echo "the backup script still fails when deniz runs it"
  exit 1
fi
if ! tar -tzf /srv/backups/app-*.tgz 2>/dev/null | grep -q 'customers.txt'; then
  echo "no archive containing customers.txt in /srv/backups"
  exit 1
fi
mode=$(stat -c %a /srv/backups)
if [ $(( 0$mode & 2 )) -ne 0 ]; then
  echo "/srv/backups is writable for everyone"
  exit 1
fi
echo "ok"
```

- [ ] **Step 2: Write the failing tests**

Create `src/scenario/library.rs` with only:

```rust
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
                    pkgs.iter().any(|o| o.id != p.id && o.story.opens.iter().any(|t| &t.id == n)),
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
        assert_eq!(done.len(), pkgs.len(), "story chain is stuck after {done:?}");
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
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test scenario::library 2>&1 | tail -5`
Expected: FAIL to compile.

- [ ] **Step 4: Write the implementation** (above the tests)

```rust
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

const PACKAGES: &[Embedded] = &[pkg!(
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
)];

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
```

- [ ] **Step 5: Run to verify pass**

Run: `cargo test scenario::library 2>&1 | tail -5`
Expected: `test result: ok. 5 passed`

- [ ] **Step 6: Format and commit**

```bash
rustfmt --edition 2021 src/scenario/library.rs src/scenario/mod.rs
git add scenarios/linux-backup-permissions src/scenario/library.rs src/scenario/mod.rs
git commit -m "feat(scenario): library sync and first Linux scenario (permissions)"
```

---

### Task 10: Two more scenarios and the Docker-gated certification

**Files:**
- Create: `scenarios/linux-disk-hog/{task.toml,instruction.md,tone/adventure.md,environment/Dockerfile,environment/setup.sh,solution/solve.sh,tests/check.sh}`
- Create: `scenarios/linux-config-typo/{task.toml,instruction.md,tone/adventure.md,environment/Dockerfile,environment/setup.sh,environment/start.sh,solution/solve.sh,tests/check.sh}`
- Modify: `src/scenario/library.rs` (add the two `pkg!` entries to `PACKAGES`)
- Create: `src/scenario/docker_tests.rs`
- Modify: `src/scenario/mod.rs` (add `#[cfg(test)] mod docker_tests;`)

**Interfaces:**
- Consumes: `ContainerEnv`, `ContainerEnv::apply_solution`, `Environment`, `CheckOutcome`, `RealDocker`, `library::{sync, load_all}`

- [ ] **Step 1: Create `linux-disk-hog`**

`task.toml`:

```toml
id = "linux-disk-hog"
topic = "linux"
title = "The log that ate the disk"
version = 1
skills = ["disk usage", "finding files"]

[environment]
kind = "container"
setup = "/usr/local/lib/usta/setup.sh"

[check]
kind = "script"
script = "check.sh"
timeout_secs = 30

[story]
event = "Cleared the oversized debug log so the disk stopped filling."
needs = ["backups-ok"]

[[story.opens]]
id = "logs-unrotated"
text = "Logs are still not rotated, so the disk can fill again."
```

`instruction.md`:

```markdown
The disk alert says `/var/log` is almost full.

Find the single largest file under `/var/log` and free its space. Keep the file in place (the service has it open) and leave every other log untouched.
```

`tone/adventure.md`:

```markdown
## open
06:40. Deniz again. The backups you repaired are running, but now monitoring says the disk is nearly full: "The shop cannot take orders when it hits the top."

Your task: find the single biggest file under `/var/log`, free its space, and keep the file where it is, because the service holds it open. Do not touch the other logs.

## close
06:58. Deniz: "Alert cleared." The orders flow again, and you now have a habit worth keeping: look for the biggest file first.
```

`environment/Dockerfile`:

```dockerfile
FROM debian:12-slim
COPY setup.sh /usr/local/lib/usta/
WORKDIR /root
```

`environment/setup.sh`:

```sh
#!/bin/sh
set -e
mkdir -p /var/log/shopd/worker
dd if=/dev/zero of=/var/log/shopd/access.log bs=1k count=1024 2>/dev/null
dd if=/dev/zero of=/var/log/shopd/worker/debug.log bs=1k count=20480 2>/dev/null
dd if=/dev/zero of=/var/log/shopd/error.log bs=1 count=2048 2>/dev/null
```

`solution/solve.sh`:

```sh
#!/bin/sh
: > /var/log/shopd/worker/debug.log
```

`tests/check.sh`:

```sh
#!/bin/sh
d=/var/log/shopd
if [ ! -f "$d/worker/debug.log" ]; then
  echo "the largest log was deleted; it has to stay in place"
  exit 1
fi
if [ "$(stat -c %s "$d/worker/debug.log")" -ge 1048576 ]; then
  echo "the largest log still takes its space"
  exit 1
fi
if [ "$(stat -c %s "$d/access.log")" -ne 1048576 ]; then
  echo "another log was changed"
  exit 1
fi
if [ "$(stat -c %s "$d/error.log")" -ne 2048 ]; then
  echo "another log was changed"
  exit 1
fi
echo "ok"
```

- [ ] **Step 2: Create `linux-config-typo`** (has a `start` script so resume has something observable)

`task.toml`:

```toml
id = "linux-config-typo"
topic = "linux"
title = "The port that is not a number"
version = 1
skills = ["config files", "grep"]

[environment]
kind = "container"
setup = "/usr/local/lib/usta/setup.sh"
start = "/usr/local/lib/usta/start.sh"

[check]
kind = "script"
script = "check.sh"
timeout_secs = 30

[story]
event = "Found and fixed a mistyped port number in the shop service's settings."
needs = ["logs-unrotated"]

[[story.opens]]
id = "config-review"
text = "Settings changes need a second look before they go live."
```

`instruction.md`:

```markdown
`/opt/shopd/shopd --check` fails.

Find out why and fix the configuration so the check passes. Do not edit `/opt/shopd/shopd`.
```

`tone/adventure.md`:

```markdown
## open
09:05. Deniz, whispering: "After the disk scare I moved a few settings around, and now shopd will not start. I am in a meeting. Please."

Your task: find out why `/opt/shopd/shopd --check` fails and fix the configuration. The program itself stays as it is.

## close
09:21. Deniz: "Check passes. What was it?" You tell him, and he writes it on the whiteboard so nobody does it again.
```

`environment/Dockerfile`:

```dockerfile
FROM debian:12-slim
COPY setup.sh start.sh /usr/local/lib/usta/
WORKDIR /root
```

`environment/setup.sh`:

```sh
#!/bin/sh
set -e
mkdir -p /opt/shopd /etc/shopd/conf.d /var/log/shopd
cat > /opt/shopd/shopd <<'EOF'
#!/bin/sh
conf=/etc/shopd/conf.d
file=$(grep -l '^listen_port' $conf/*.conf | tail -1)
port=$(grep -h '^listen_port' $conf/*.conf | tail -1 | cut -d= -f2 | tr -d ' ')
case "$port" in
  ''|*[!0-9]*)
    echo "shopd: invalid configuration, see /var/log/shopd/shopd.log" >&2
    echo "$(date '+%F %T') ERROR invalid listen_port '$port' in $file" >> /var/log/shopd/shopd.log
    exit 1 ;;
esac
echo "shopd: configuration ok (port $port)"
EOF
chmod 755 /opt/shopd/shopd
printf 'listen_port = 8080\nworkers = 4\n' > /etc/shopd/conf.d/10-base.conf
printf 'name = shop\nlisten_port = 80g0\n' > /etc/shopd/conf.d/20-site.conf
```

`environment/start.sh`:

```sh
#!/bin/sh
mkdir -p /var/log/shopd
echo "$(date '+%F %T') INFO services up" >> /var/log/shopd/starts.log
```

`solution/solve.sh`:

```sh
#!/bin/sh
sed -i 's/80g0/8081/' /etc/shopd/conf.d/20-site.conf
```

`tests/check.sh`:

```sh
#!/bin/sh
# Re-reads the configuration itself; does not trust the learner-writable program.
port=$(grep -h '^listen_port' /etc/shopd/conf.d/*.conf | tail -1 | cut -d= -f2 | tr -d ' ')
case "$port" in
  ''|*[!0-9]*)
    echo "listen_port is not a number: '$port'"
    exit 1 ;;
esac
if [ "$port" -lt 1024 ] || [ "$port" -gt 65535 ]; then
  echo "listen_port is out of range"
  exit 1
fi
echo "ok"
```

- [ ] **Step 3: Register both in the library**

In `src/scenario/library.rs`, replace the `PACKAGES` constant with:

```rust
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
```

- [ ] **Step 4: Run the existing library tests (they now cover three packages)**

Run: `cargo test scenario::library 2>&1 | tail -5`
Expected: `test result: ok. 5 passed` (the lint test now loops over all three packages).

- [ ] **Step 5: Write the Docker-gated certification and lifecycle tests**

Create `src/scenario/docker_tests.rs`:

```rust
//! Real-Docker tests. `#[ignore]`d so the default `cargo test` (and CI) never
//! needs Docker. Run on a machine with Docker:
//!   cargo test scenario::docker_tests -- --ignored --test-threads=1
//! They touch only containers named `usta-cert0000-*`.

use crate::scenario::container::ContainerEnv;
use crate::scenario::docker::{Docker, RealDocker};
use crate::scenario::env::{CheckOutcome, Environment};
use crate::scenario::library;
use crate::scenario::package::tests::tmp;

const TAG: &str = "cert0000";

/// Removes the test container even when an assertion panics.
struct Cleanup(String);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = RealDocker.run(&["rm", "-f", &self.0]);
    }
}

fn is_fail(o: &CheckOutcome) -> bool {
    matches!(o, CheckOutcome::Fail { .. })
}

fn certify(id: &str) {
    let global = tmp(&format!("cert_{id}"));
    library::sync(&global).unwrap();
    let pkg = library::load_all(&global, "linux")
        .unwrap()
        .into_iter()
        .find(|p| p.id == id)
        .expect("package present");
    let d = RealDocker;
    let mut env = ContainerEnv { docker: &d, project_tag: TAG.into() };
    let _guard = Cleanup(format!("usta-{TAG}-cert-{id}"));
    let h = env.prepare(&pkg, &format!("cert-{id}")).unwrap();
    // Condition 1: on the fresh environment (also the "do nothing" attempt) the check fails.
    assert!(is_fail(&env.check(&pkg, &h).unwrap()), "{id}: fresh check must fail");
    // Condition 2: after the reference solution, the check passes.
    env.apply_solution(&pkg, &h).unwrap();
    assert_eq!(env.check(&pkg, &h).unwrap(), CheckOutcome::Pass, "{id}: solved check must pass");
    env.clean(&h).unwrap();
    let _ = std::fs::remove_dir_all(global);
}

#[test]
#[ignore = "needs Docker"]
fn backup_permissions_is_certified() {
    certify("linux-backup-permissions");
}

#[test]
#[ignore = "needs Docker"]
fn disk_hog_is_certified() {
    certify("linux-disk-hog");
}

#[test]
#[ignore = "needs Docker"]
fn config_typo_is_certified() {
    certify("linux-config-typo");
}

#[test]
#[ignore = "needs Docker"]
fn pause_keeps_files_and_resume_reruns_the_start_script() {
    let global = tmp("cert_lifecycle");
    library::sync(&global).unwrap();
    let pkg = library::load_all(&global, "linux")
        .unwrap()
        .into_iter()
        .find(|p| p.id == "linux-config-typo")
        .unwrap();
    let d = RealDocker;
    let mut env = ContainerEnv { docker: &d, project_tag: TAG.into() };
    let _guard = Cleanup(format!("usta-{TAG}-cert-life"));
    let h = env.prepare(&pkg, "cert-life").unwrap();
    let name = h.reference.clone();
    assert!(d.run(&["exec", &name, "touch", "/root/marker"]).unwrap().ok());
    env.pause(&h).unwrap();
    let st = d.run(&["inspect", "-f", "{{.State.Running}}", &name]).unwrap();
    assert_eq!(st.stdout.trim(), "false", "paused container must not run");
    env.resume(&pkg, &h).unwrap();
    assert!(d.run(&["exec", &name, "test", "-f", "/root/marker"]).unwrap().ok(), "files survive a stop");
    let starts = d.run(&["exec", &name, "wc", "-l", "/var/log/shopd/starts.log"]).unwrap();
    assert!(starts.stdout.trim_start().starts_with('2'), "start script ran at prepare and resume: {}", starts.stdout);
    env.clean(&h).unwrap();
    let gone = d.run(&["inspect", &name]).unwrap();
    assert!(!gone.ok(), "clean must remove the container");
    let _ = std::fs::remove_dir_all(global);
}
```

In `src/scenario/mod.rs` add:

```rust
#[cfg(test)]
mod docker_tests;
```

- [ ] **Step 6: Verify the default run skips them and everything compiles**

Run: `cargo test scenario:: 2>&1 | tail -6`
Expected: all non-ignored scenario tests pass; the four Docker tests are reported as `ignored`.

- [ ] **Step 7: Certify on a Docker machine (executing session only; never against `kiln-*`)**

Run: `cargo test scenario::docker_tests -- --ignored --test-threads=1 2>&1 | tail -10`
Expected: `4 passed`. If a scenario fails condition 1 or 2, fix the package scripts, not the test. Afterwards `docker ps -a --filter label=usta=1` must list nothing from the test run.

- [ ] **Step 8: Format and commit**

```bash
rustfmt --edition 2021 src/scenario/library.rs src/scenario/docker_tests.rs src/scenario/mod.rs
git add scenarios src/scenario/library.rs src/scenario/docker_tests.rs src/scenario/mod.rs
git commit -m "feat(scenario): disk and config scenarios, Docker certification tests"
```

---

### Task 11: Startup sweep

**Files:**
- Create: `src/scenario/sweep.rs`
- Modify: `src/scenario/mod.rs` (add `pub(crate) mod sweep;`)
- Modify: `src/main.rs` (one call after `ensure_scaffold`, around line 99)

**Interfaces:**
- Consumes: `Store`, `RunRecord`, `RunState`, `project_tag` (Task 2); `Found`, `list_labeled` (Task 7); `RealDocker`, `available` (Task 6); `crate::setup::confirm(prompt, yes) -> Result<bool>`
- Produces:
  - `pub(crate) const STALE_DAYS: i64 = 14`
  - `pub(crate) enum Action { RemoveContainer(String), PauseRecord { run_id: String, container: String }, DeleteRecord(String), AskStale { run_id: String, package: String, days: i64 } }`
  - `pub(crate) fn plan(records: &[RunRecord], containers: &[Found], alive: &dyn Fn(u32) -> bool, today: &str) -> Vec<Action>`
  - `pub(crate) fn startup(project_root: &Path, today: &str)`

Rules encoded in `plan` (spec "Container lifecycle"): a container with no record is removed; a finished or dropped run's leftover container is removed (the record stays as history); a `preparing` record whose owner is dead is deleted together with any container; an `active` record whose owner is dead is paused (crash recovery); a `paused` record untouched for `STALE_DAYS` or more produces a question, never a deletion; an unfinished task is otherwise never touched.

- [ ] **Step 1: Write the failing tests**

Create `src/scenario/sweep.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str, state: RunState, owner: u32, updated: &str) -> RunRecord {
        RunRecord {
            run_id: id.into(),
            package: "demo".into(),
            topic: "linux".into(),
            tone: "plain".into(),
            state,
            owner_pid: owner,
            started: "2026-10-01".into(),
            updated: updated.into(),
            reference: format!("usta-aaaa1111-{id}"),
            access: String::new(),
            last_check: None,
        }
    }

    fn found(id: &str, state: &str) -> Found {
        Found { name: format!("usta-aaaa1111-{id}"), state: state.into(), run_id: id.into() }
    }

    const DEAD: &dyn Fn(u32) -> bool = &|_| false;
    const TODAY: &str = "2026-11-04";

    #[test]
    fn unfinished_tasks_survive_startup_cleanup() {
        let recs = vec![rec("p", RunState::Paused, 0, "2026-11-01")];
        assert_eq!(plan(&recs, &[found("p", "exited")], DEAD, TODAY), vec![]);
    }

    #[test]
    fn a_container_without_a_record_is_removed() {
        assert_eq!(plan(&[], &[found("ghost", "exited")], DEAD, TODAY), vec![Action::RemoveContainer("usta-aaaa1111-ghost".into())]);
    }

    #[test]
    fn a_finished_leftover_is_removed_and_its_record_kept() {
        let recs = vec![rec("f", RunState::Finished, 0, "2026-11-03")];
        assert_eq!(plan(&recs, &[found("f", "exited")], DEAD, TODAY), vec![Action::RemoveContainer("usta-aaaa1111-f".into())]);
        assert_eq!(plan(&recs, &[], DEAD, TODAY), vec![]);
    }

    #[test]
    fn a_dropped_leftover_is_removed_and_its_record_kept() {
        let recs = vec![rec("d", RunState::Dropped, 0, "2026-11-03")];
        assert_eq!(plan(&recs, &[found("d", "exited")], DEAD, TODAY), vec![Action::RemoveContainer("usta-aaaa1111-d".into())]);
        assert_eq!(plan(&recs, &[], DEAD, TODAY), vec![]);
    }

    #[test]
    fn a_crash_leaving_a_container_running_pauses_it() {
        let recs = vec![rec("c", RunState::Active, 4242, "2026-11-04")];
        assert_eq!(
            plan(&recs, &[found("c", "running")], DEAD, TODAY),
            vec![Action::PauseRecord { run_id: "c".into(), container: "usta-aaaa1111-c".into() }]
        );
    }

    #[test]
    fn a_live_owner_is_left_alone() {
        let recs = vec![rec("c", RunState::Active, 4242, "2026-11-04")];
        let alive: &dyn Fn(u32) -> bool = &|p| p == 4242;
        assert_eq!(plan(&recs, &[found("c", "running")], alive, TODAY), vec![]);
    }

    #[test]
    fn an_interrupted_prepare_is_deleted_with_its_container() {
        let recs = vec![rec("x", RunState::Preparing, 4242, "2026-11-04")];
        assert_eq!(
            plan(&recs, &[found("x", "created")], DEAD, TODAY),
            vec![Action::RemoveContainer("usta-aaaa1111-x".into()), Action::DeleteRecord("x".into())]
        );
        assert_eq!(plan(&recs, &[], DEAD, TODAY), vec![Action::DeleteRecord("x".into())]);
    }

    #[test]
    fn a_long_untouched_paused_task_asks_never_deletes() {
        let recs = vec![rec("old", RunState::Paused, 0, "2026-10-01")];
        let acts = plan(&recs, &[found("old", "exited")], DEAD, TODAY);
        assert_eq!(acts, vec![Action::AskStale { run_id: "old".into(), package: "demo".into(), days: 34 }]);
        let fresh = vec![rec("n", RunState::Paused, 0, "2026-10-22")];
        assert_eq!(plan(&fresh, &[], DEAD, TODAY), vec![]);
    }

    #[test]
    fn a_bad_date_never_asks() {
        let recs = vec![rec("d", RunState::Paused, 0, "not-a-date")];
        assert_eq!(plan(&recs, &[], DEAD, TODAY), vec![]);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test scenario::sweep 2>&1 | tail -5`
Expected: FAIL to compile.

- [ ] **Step 3: Write the implementation** (above the tests)

```rust
//! Startup cleanup. Touches only containers carrying `usta=1` AND this
//! project's tag. Pure `plan` decides; `startup` executes. An unfinished task
//! is never removed without asking.

use std::path::Path;

use chrono::NaiveDate;

use crate::scenario::container::{list_labeled, ContainerEnv, Found};
use crate::scenario::docker::{available, Docker, RealDocker};
use crate::scenario::engine::{self, Tick};
use crate::scenario::store::{project_tag, RunRecord, RunState, Store};
use crate::ui;

/// Provisional: the spec lists the idle age as an open measurement.
pub(crate) const STALE_DAYS: i64 = 14;

#[derive(Debug, PartialEq)]
pub(crate) enum Action {
    RemoveContainer(String),
    PauseRecord { run_id: String, container: String },
    DeleteRecord(String),
    AskStale { run_id: String, package: String, days: i64 },
}

fn age_days(updated: &str, today: &str) -> Option<i64> {
    let u = NaiveDate::parse_from_str(updated, "%Y-%m-%d").ok()?;
    let t = NaiveDate::parse_from_str(today, "%Y-%m-%d").ok()?;
    Some((t - u).num_days())
}

pub(crate) fn plan(
    records: &[RunRecord],
    containers: &[Found],
    alive: &dyn Fn(u32) -> bool,
    today: &str,
) -> Vec<Action> {
    let mut acts = Vec::new();
    let find = |run_id: &str| containers.iter().find(|c| c.run_id == run_id);
    for c in containers {
        match records.iter().find(|r| r.run_id == c.run_id) {
            None => acts.push(Action::RemoveContainer(c.name.clone())),
            Some(r) if matches!(r.state, RunState::Finished | RunState::Dropped) => {
                acts.push(Action::RemoveContainer(c.name.clone()))
            }
            Some(_) => {}
        }
    }
    for r in records {
        let owner_dead = r.owner_pid == 0 || !alive(r.owner_pid);
        match r.state {
            RunState::Preparing if owner_dead => {
                if let Some(c) = find(&r.run_id) {
                    acts.push(Action::RemoveContainer(c.name.clone()));
                }
                acts.push(Action::DeleteRecord(r.run_id.clone()));
            }
            RunState::Active if owner_dead => acts.push(Action::PauseRecord {
                run_id: r.run_id.clone(),
                container: r.reference.clone(),
            }),
            RunState::Paused => {
                if let Some(days) = age_days(&r.updated, today).filter(|d| *d >= STALE_DAYS) {
                    acts.push(Action::AskStale {
                        run_id: r.run_id.clone(),
                        package: r.package.clone(),
                        days,
                    });
                }
            }
            _ => {}
        }
    }
    acts
}

/// Run once at startup, before the session UI. No Docker call unless this
/// project ever recorded a run. Failures are warnings, never fatal.
pub(crate) fn startup(project_root: &Path, today: &str) {
    let store = Store::new(project_root);
    if !store.exists() {
        return;
    }
    let docker = RealDocker;
    if available(&docker).is_err() {
        return;
    }
    let tag = project_tag(project_root);
    let Ok(containers) = list_labeled(&docker, &tag) else {
        return;
    };
    let records = store.all();
    let alive: &dyn Fn(u32) -> bool = &crate::lock::pid_alive;
    let actions = plan(&records, &containers, alive, today);
    let interactive = std::io::IsTerminal::is_terminal(&std::io::stdin())
        && std::io::IsTerminal::is_terminal(&std::io::stdout());
    let mut env = ContainerEnv {
        docker: &docker,
        project_tag: tag,
    };
    for a in actions {
        match a {
            Action::RemoveContainer(name) => {
                let _ = docker.run(&["rm", "-f", &name]);
            }
            Action::DeleteRecord(id) => {
                let _ = store.remove(&id);
            }
            Action::PauseRecord { run_id, .. } => {
                if let Ok(mut rec) = store.load(&run_id) {
                    let t = Tick { pid: std::process::id(), alive, stamp: "", today };
                    match engine::pause(&mut env, &store, &mut rec, &t) {
                        Ok(()) => ui::notice(&format!("scenario '{}' was left running; paused it", rec.package)),
                        Err(e) => ui::warn(&format!("could not pause scenario {run_id}: {e}")),
                    }
                }
            }
            Action::AskStale { run_id, package, days } => {
                if !interactive {
                    continue; // never decide silently
                }
                let prompt = format!(
                    "scenario '{package}' has been untouched for {days} days. Remove it? [y/N] "
                );
                let yes = crate::setup::confirm(&prompt, &["y", "yes"]).unwrap_or(false);
                if let Ok(mut rec) = store.load(&run_id) {
                    if yes {
                        let _ = docker.run(&["rm", "-f", &rec.reference]);
                        rec.state = RunState::Dropped; // the record notes it was dropped
                        rec.owner_pid = 0;
                        rec.updated = today.to_string();
                        let _ = store.save(&rec);
                    } else {
                        rec.updated = today.to_string(); // ask again only after another STALE_DAYS
                        let _ = store.save(&rec);
                    }
                }
            }
        }
    }
}
```

Note for the executor: `engine::pause` calls `env.pause` which runs `docker stop`; stopping an already-stopped container exits 0, so the crash-recovery path is safe.

- [ ] **Step 4: Wire the call into `main.rs`**

In `src/main.rs`, directly after the `if !had_project_root { ui::notice(".usta/ set up"); }` block (before `let mut watch_rx = watcher::spawn(...)`), add:

```rust
    // Resolve scenario leftovers (finished tasks' containers, crash survivors) before any UI.
    scenario::sweep::startup(&project_root, &today());
```

- [ ] **Step 5: Run to verify pass**

Run: `cargo test scenario::sweep 2>&1 | tail -5`
Expected: `test result: ok. 8 passed`. Then `cargo build 2>&1 | tail -3` must succeed.

- [ ] **Step 6: Format and commit**

```bash
rustfmt --edition 2021 src/scenario/sweep.rs src/scenario/mod.rs
git add src/scenario/sweep.rs src/scenario/mod.rs src/main.rs
git commit -m "feat(scenario): startup sweep that never removes unfinished work silently"
```

---

### Task 12: The `/scenario` command layer

**Files:**
- Create: `src/scenario/command.rs`
- Modify: `src/scenario/mod.rs` (add `pub(crate) mod command;`)

**Interfaces:**
- Consumes: Tasks 1-11 (`Package`, `Tone`, `Store`, `RunRecord`, `RunState`, `engine::*`, `select::*`, `ledger::{Ledger, reconcile}`, `docker::{ensure_ready, available, runc_warning, RealDocker, SystemStarter, START_TIMEOUT, START_POLL}`, `container::ContainerEnv`, `terminal::{window_notice, SystemLauncher, WindowReq}`, `env::Internal`); `crate::progress::curriculum_path`, `crate::lifecycle::today`, `crate::lock::pid_alive`
- Produces:
  - `pub(crate) enum ScenarioCmd { List, Start(Option<String>), Resume(Option<String>), Check, Pause, Drop { id: Option<String>, confirmed: bool }, Tone(Option<String>), Usage }`
  - `pub(crate) fn parse(line: &str) -> Option<ScenarioCmd>`
  - `pub(crate) fn busy_note(cmd: &ScenarioCmd) -> Option<&'static str>`
  - `pub(crate) struct Reply { pub notices: Vec<String>, pub turn: Option<String> }`
  - `#[derive(Clone)] pub(crate) struct Ctx { pub global: PathBuf, pub project_root: PathBuf, pub topic: String, pub pid: u32 }`
  - `pub(crate) fn execute(ctx: &Ctx, cmd: ScenarioCmd) -> Reply` (blocking; never panics; internal errors are logged to `.usta/scenarios/last-error.log` and shown as one plain sentence)
  - `pub(crate) struct Slot` with `Slot::new(global: &Path, project_root: &Path, topic: &str) -> Slot`, `async fn handle(&self, cmd: ScenarioCmd) -> Reply`, and `impl Drop` that pauses runs this process owns
  - pure helpers `render_list(...)`, `start_turn(...)`, `resume_turn(...)`, `done_turn(...)`, `failure(...)`, `pick_unfinished(...)`

Behaviour in short: `start` and `resume` first make the runtime ready (start it if stopped, wait up to 90 s), then build or wake the task and open the terminal window; a passing `check` shows the closing text, updates the ledger and finishes the run; `drop` asks once (`/scenario drop <id>` shows what will be lost, `... confirm` does it) and then removes the task and records it as dropped.

- [ ] **Step 1: Write the failing tests**

Create `src/scenario/command.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::env::internal;
    use crate::scenario::package::tests::{tmp, write_pkg};
    use crate::scenario::store::{RunRecord, RunState};

    fn pkg() -> Package {
        let root = tmp("cmd_pkg");
        Package::load(&write_pkg(&root)).unwrap()
    }

    fn rec(id: &str, state: RunState, owner: u32) -> RunRecord {
        RunRecord {
            run_id: id.into(),
            package: "linux-b".into(),
            topic: "linux".into(),
            tone: "plain".into(),
            state,
            owner_pid: owner,
            started: "2026-11-04".into(),
            updated: "2026-11-04".into(),
            reference: "r".into(),
            access: "a".into(),
            last_check: Some("fail".into()),
        }
    }

    fn bad_words(s: &str) -> bool {
        let l = s.to_lowercase();
        l.contains("container") || l.contains("docker") || l.contains("image")
    }

    #[test]
    fn parse_variants() {
        assert_eq!(parse("/scenario"), Some(ScenarioCmd::List));
        assert_eq!(parse("  /SCENARIO  "), Some(ScenarioCmd::List));
        assert_eq!(parse("/scenario list"), Some(ScenarioCmd::List));
        assert_eq!(parse("/scenario start"), Some(ScenarioCmd::Start(None)));
        assert_eq!(parse("/scenario start 2"), Some(ScenarioCmd::Start(Some("2".into()))));
        assert_eq!(parse("/scenario resume linux-x"), Some(ScenarioCmd::Resume(Some("linux-x".into()))));
        assert_eq!(parse("/scenario check"), Some(ScenarioCmd::Check));
        assert_eq!(parse("/scenario pause"), Some(ScenarioCmd::Pause));
        assert_eq!(parse("/scenario tone adventure"), Some(ScenarioCmd::Tone(Some("adventure".into()))));
        assert_eq!(parse("/scenario tone"), Some(ScenarioCmd::Tone(None)));
        assert_eq!(parse("/scenario wat"), Some(ScenarioCmd::Usage));
    }

    #[test]
    fn parse_drop_with_and_without_id_and_confirmation() {
        assert_eq!(parse("/scenario drop"), Some(ScenarioCmd::Drop { id: None, confirmed: false }));
        assert_eq!(parse("/scenario drop linux-x"), Some(ScenarioCmd::Drop { id: Some("linux-x".into()), confirmed: false }));
        assert_eq!(parse("/scenario drop linux-x confirm"), Some(ScenarioCmd::Drop { id: Some("linux-x".into()), confirmed: true }));
        assert_eq!(parse("/scenario drop confirm"), Some(ScenarioCmd::Drop { id: None, confirmed: true }));
    }

    #[test]
    fn parse_ignores_lookalikes_and_ordinary_text() {
        assert_eq!(parse("/scenarios"), None);
        assert_eq!(parse("scenario start"), None);
        assert_eq!(parse("tell me about /scenario"), None);
        assert_eq!(parse("/game on"), None);
    }

    #[test]
    fn only_slow_commands_get_a_busy_note() {
        assert!(busy_note(&ScenarioCmd::Start(None)).unwrap().contains("first start"));
        assert!(busy_note(&ScenarioCmd::Resume(None)).is_some());
        assert!(busy_note(&ScenarioCmd::Check).is_some());
        assert!(busy_note(&ScenarioCmd::List).is_none());
        assert!(busy_note(&ScenarioCmd::Tone(None)).is_none());
        assert!(busy_note(&ScenarioCmd::Drop { id: None, confirmed: true }).is_none());
    }

    #[test]
    fn list_shows_tags_unfinished_runs_drop_hint_and_tone() {
        let mut a = pkg();
        a.id = "linux-a".into();
        a.title = "A title".into();
        let mut b = pkg();
        b.id = "linux-b".into();
        b.title = "B title".into();
        let mut c = pkg();
        c.id = "linux-c".into();
        c.title = "C title".into();
        c.story.needs = vec!["not-open".into()];
        let paused = rec("linux-b-2026", RunState::Paused, 0);
        let out = render_list("linux", &[a, b, c], "", &["linux-b".to_string()], &[], &[paused], Tone::Plain);
        assert!(out.contains("1. linux-a"), "{out}");
        assert!(out.contains("[new]") && out.contains("[done]") && out.contains("[later in the story]"), "{out}");
        assert!(out.contains("/scenario resume linux-b-2026"), "{out}");
        assert!(out.contains("last check: fail"), "{out}");
        assert!(out.contains("/scenario drop"), "{out}");
        assert!(out.contains("Tone: plain"), "{out}");
    }

    #[test]
    fn list_without_packages_says_so() {
        let out = render_list("rust", &[], "", &[], &[], &[], Tone::Plain);
        assert!(out.contains("No tasks for 'rust' yet"), "{out}");
    }

    #[test]
    fn turns_carry_task_facts_and_the_world_but_never_the_solution() {
        let root = tmp("cmd_turn");
        let p = Package::load(&write_pkg(&root)).unwrap();
        std::fs::write(p.dir.join("solution/solve.sh"), "SECRET-FIX-LINE\n").unwrap();
        let s = start_turn(&p, "Fix it.", Tone::Adventure, "World so far (Shopwell): people: Deniz.");
        assert!(s.starts_with("[SCENARIO START]"));
        assert!(s.contains("demo-pkg") && s.contains("Fix it.") && s.contains("adventure"));
        assert!(s.contains("World so far (Shopwell)"));
        assert!(s.contains("task or server") && !s.to_lowercase().contains("container terminal"));
        assert!(!s.contains("SECRET-FIX-LINE"));
        assert!(!start_turn(&p, "Fix it.", Tone::Plain, "").contains("World so far"));
        let r = resume_turn(&p, "Fix it.", Some("fail"));
        assert!(r.starts_with("[SCENARIO RESUME]") && r.contains("Their last check: fail"));
        let d = done_turn(&p);
        assert!(d.starts_with("[SCENARIO DONE]") && d.contains("permission"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn learner_text_avoids_infrastructure_words() {
        for c in [ScenarioCmd::Start(None), ScenarioCmd::Resume(None), ScenarioCmd::Check] {
            assert!(!bad_words(busy_note(&c).unwrap()));
        }
        assert!(!bad_words(USAGE));
        let mut a = pkg();
        a.id = "linux-a".into();
        let out = render_list("linux", &[a], "", &[], &[], &[rec("r1", RunState::Paused, 0)], Tone::Plain);
        assert!(!bad_words(&out), "{out}");
    }

    #[test]
    fn internal_errors_are_logged_and_shown_as_one_plain_sentence() {
        let g = tmp("cmd_fail_g");
        let p = tmp("cmd_fail_p");
        let ctx = Ctx { global: g.clone(), project_root: p.clone(), topic: "linux".into(), pid: 1 };
        let r = failure(&ctx, internal("docker rm failed: Error response from daemon: boom"));
        let shown = r.notices.concat();
        assert!(shown.contains("last-error.log") && !bad_words(&shown) && !shown.contains("boom"), "{shown}");
        let log = std::fs::read_to_string(p.join(".usta/scenarios/last-error.log")).unwrap();
        assert!(log.contains("boom"));
        let plain = failure(&ctx, anyhow::anyhow!("no such task"));
        assert_eq!(plain.notices, vec!["scenario: no such task".to_string()]);
        let _ = std::fs::remove_dir_all(g);
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn pick_unfinished_takes_the_only_one_by_default_and_names_choices_otherwise() {
        let p = tmp("cmd_pick");
        let store = Store::new(&p);
        store.save(&rec("a-1", RunState::Paused, 0)).unwrap();
        assert_eq!(pick_unfinished(&store, None, "linux", 7).unwrap().run_id, "a-1");
        store.save(&rec("a-2", RunState::Active, 7)).unwrap();
        let e = pick_unfinished(&store, None, "linux", 7).unwrap_err().to_string();
        assert!(e.contains("/scenario drop a-1") && e.contains("/scenario drop a-2"), "{e}");
        assert_eq!(pick_unfinished(&store, Some("a-2"), "linux", 7).unwrap().run_id, "a-2");
        assert!(pick_unfinished(&store, Some("nope"), "linux", 7).is_err());
        // finished, dropped, other-topic and other-owner runs are never offered
        store.save(&rec("a-3", RunState::Finished, 0)).unwrap();
        store.save(&rec("a-4", RunState::Dropped, 0)).unwrap();
        store.save(&rec("a-5", RunState::Active, 99)).unwrap();
        assert!(pick_unfinished(&store, Some("a-3"), "linux", 7).is_err());
        assert!(pick_unfinished(&store, Some("a-5"), "linux", 7).is_err());
        assert!(pick_unfinished(&store, Some("a-1"), "rust", 7).is_err());
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn execute_handles_tone_usage_and_dropping_nothing_without_the_runtime() {
        let g = tmp("cmd_exec_g");
        let p = tmp("cmd_exec_p");
        let ctx = Ctx { global: g.clone(), project_root: p.clone(), topic: "rust".into(), pid: 1 };
        let t = execute(&ctx, ScenarioCmd::Tone(Some("adventure".into())));
        assert!(t.notices.concat().contains("adventure"));
        let bad = execute(&ctx, ScenarioCmd::Tone(Some("loud".into())));
        assert!(bad.notices.concat().contains("plain or adventure"));
        assert!(execute(&ctx, ScenarioCmd::Usage).notices.concat().contains("drop"));
        let d = execute(&ctx, ScenarioCmd::Drop { id: None, confirmed: false });
        assert!(d.notices.concat().contains("no unfinished task"), "{:?}", d.notices);
        assert!(d.turn.is_none());
        let _ = std::fs::remove_dir_all(g);
        let _ = std::fs::remove_dir_all(p);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test scenario::command 2>&1 | tail -5`
Expected: FAIL to compile.

- [ ] **Step 3: Write the implementation** (above the tests)

```rust
//! `/scenario ...`: parsing, execution (blocking, run off the UI task via
//! `spawn_blocking`), the synthesized mentor turns, and the `Slot` that pauses
//! this process's runs when the session ends.
//!
//! Conventions copied from `/game`: slash lines never reach the model; notices
//! are shell-only; at most one synthesized `[SCENARIO ...]` turn per command
//! goes through the normal ask flow. Learner-facing text says task or server,
//! never container, image or Docker (install guidance and the no-window
//! fallback are the sanctioned exceptions, in `docker.rs` and `terminal.rs`).

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};

use crate::progress;
use crate::scenario::container::ContainerEnv;
use crate::scenario::docker::{
    available, ensure_ready, runc_warning, RealDocker, SystemStarter, START_POLL, START_TIMEOUT,
};
use crate::scenario::engine::{self, Tick};
use crate::scenario::env::{CheckOutcome, Internal};
use crate::scenario::ledger::{self, Ledger};
use crate::scenario::library;
use crate::scenario::package::{Package, Tone};
use crate::scenario::select::{self, Tag};
use crate::scenario::store::{project_tag, RunRecord, RunState, Store};
use crate::scenario::terminal::{window_notice, SystemLauncher, WindowReq};

const USAGE: &str = "scenario commands: /scenario (list) · start [number|id] · resume [id] · check · pause · drop [id] · tone plain|adventure";

#[derive(Debug, PartialEq)]
pub(crate) enum ScenarioCmd {
    List,
    Start(Option<String>),
    Resume(Option<String>),
    Check,
    Pause,
    Drop { id: Option<String>, confirmed: bool },
    Tone(Option<String>),
    Usage,
}

/// `/scenario ...` only; lookalikes (`/scenarios`) and ordinary text return None.
pub(crate) fn parse(line: &str) -> Option<ScenarioCmd> {
    let lower = line.trim().to_ascii_lowercase();
    let rest = if lower == "/scenario" {
        ""
    } else {
        lower.strip_prefix("/scenario ")?
    };
    let mut it = rest.split_whitespace();
    Some(match it.next() {
        None | Some("list") => ScenarioCmd::List,
        Some("start") => ScenarioCmd::Start(it.next().map(String::from)),
        Some("resume") => ScenarioCmd::Resume(it.next().map(String::from)),
        Some("check") => ScenarioCmd::Check,
        Some("pause") => ScenarioCmd::Pause,
        Some("drop") => {
            let (mut id, mut confirmed) = (None, false);
            for a in it {
                if a == "confirm" {
                    confirmed = true;
                } else if id.is_none() {
                    id = Some(a.to_string());
                }
            }
            ScenarioCmd::Drop { id, confirmed }
        }
        Some("tone") => ScenarioCmd::Tone(it.next().map(String::from)),
        Some(_) => ScenarioCmd::Usage,
    })
}

/// Shown before a slow command (the screen does not redraw while it works).
pub(crate) fn busy_note(cmd: &ScenarioCmd) -> Option<&'static str> {
    match cmd {
        ScenarioCmd::Start(_) => Some("preparing your task… the first start can take a few minutes while its server is set up"),
        ScenarioCmd::Resume(_) => Some("resuming your task…"),
        ScenarioCmd::Check => Some("running the check…"),
        _ => None,
    }
}

#[derive(Debug, Default)]
pub(crate) struct Reply {
    pub notices: Vec<String>,
    /// Synthesized user turn for the normal ask flow (start, resume, pass).
    pub turn: Option<String>,
}

impl Reply {
    fn note(s: impl Into<String>) -> Reply {
        Reply { notices: vec![s.into()], turn: None }
    }
}

#[derive(Clone)]
pub(crate) struct Ctx {
    pub global: PathBuf,
    pub project_root: PathBuf,
    pub topic: String,
    pub pid: u32,
}

pub(crate) fn render_list(
    topic: &str,
    pkgs: &[Package],
    map: &str,
    finished: &[String],
    threads: &[String],
    unfinished: &[RunRecord],
    tone: Tone,
) -> String {
    if pkgs.is_empty() {
        return format!("No tasks for '{topic}' yet.\nTone: {}", tone.as_str());
    }
    let mut out = format!("Tasks for '{topic}':\n");
    for (i, p) in pkgs.iter().enumerate() {
        let t = match select::tag(p, map, finished, threads) {
            Tag::New => "new",
            Tag::Done => "done",
            Tag::Skipped => "skipped, already settled",
            Tag::Later => "later in the story",
        };
        out.push_str(&format!("  {}. {} — {} [{t}]\n", i + 1, p.id, p.title));
    }
    if !unfinished.is_empty() {
        out.push_str("Unfinished:\n");
        for r in unfinished {
            out.push_str(&format!(
                "  {} (last check: {}) — /scenario resume {}\n",
                r.package,
                r.last_check.as_deref().unwrap_or("none yet"),
                r.run_id
            ));
        }
        out.push_str("Give one up with /scenario drop [id].\n");
    }
    out.push_str(&format!(
        "Start one with /scenario start [number|id]. Tone: {} (change: /scenario tone plain|adventure)",
        tone.as_str()
    ));
    out
}

// The three turn builders below are for the mentor (the model), not the
// learner, but they tell it to keep the infrastructure words out of its replies.

pub(crate) fn start_turn(pkg: &Package, instruction: &str, tone: Tone, world: &str) -> String {
    let world = if world.is_empty() { String::new() } else { format!("{world} ") };
    format!(
        "[SCENARIO START] The learner just started the task \"{}\" ({}). Skills it exercises: {}. \
         The shell already showed them the opening in {} tone; do not repeat or paraphrase it. \
         {world}Task: {instruction} \
         They work in a terminal on the task's server that you cannot see and will run /scenario check themselves; the shell reports the result. \
         Say task or server to the learner; never container, image or Docker. \
         Mentor Socratically as usual: never give the fix, ask what they see, hint by the ladder. \
         Reply with ONE short sentence that invites the first step.",
        pkg.title,
        pkg.id,
        pkg.skills.join(", "),
        tone.as_str()
    )
}

pub(crate) fn resume_turn(pkg: &Package, instruction: &str, last_check: Option<&str>) -> String {
    format!(
        "[SCENARIO RESUME] The learner came back to the unfinished task \"{}\" ({}). Task: {instruction} \
         Their last check: {}. Their work on the server survived; its services were restarted. \
         Say task or server to the learner; never container, image or Docker. \
         In two sentences, say where they left off if this conversation shows it, otherwise restate the task. \
         Do not give the fix.",
        pkg.title,
        pkg.id,
        last_check.unwrap_or("none yet")
    )
}

pub(crate) fn done_turn(pkg: &Package) -> String {
    format!(
        "[SCENARIO DONE] The check passed for \"{}\" ({}). Skills exercised: {}. The shell already showed the closing. \
         Treat this as evidence for these skills and reflect it in the curriculum map at the closing flush under the usual evidence rules. \
         Reply with one sentence of acknowledgement, no recap.",
        pkg.title,
        pkg.id,
        pkg.skills.join(", ")
    )
}

fn log_error(ctx: &Ctx, text: &str) {
    let p = ctx.project_root.join(".usta/scenarios/last-error.log");
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(p, format!("{text}\n"));
}

/// Internal errors (tool output) go to the log; the learner reads one plain sentence.
fn failure(ctx: &Ctx, e: anyhow::Error) -> Reply {
    if e.downcast_ref::<Internal>().is_some() {
        log_error(ctx, &format!("{e:#}"));
        Reply::note("scenario: something went wrong with the task's server; details are in .usta/scenarios/last-error.log")
    } else {
        Reply::note(format!("scenario: {e}"))
    }
}

pub(crate) fn execute(ctx: &Ctx, cmd: ScenarioCmd) -> Reply {
    match run(ctx, cmd) {
        Ok(r) => r,
        Err(e) => failure(ctx, e),
    }
}

fn finished_ids(store: &Store) -> Vec<String> {
    store
        .all()
        .into_iter()
        .filter(|r| r.state == RunState::Finished)
        .map(|r| r.package)
        .collect()
}

fn map_text(ctx: &Ctx) -> String {
    std::fs::read_to_string(progress::curriculum_path(&ctx.project_root, &ctx.topic))
        .unwrap_or_default()
}

/// Apply finished runs the ledger has not seen (repairs a crash between the
/// finished record and the ledger write). Never fatal.
fn sync_ledger(ctx: &Ctx, store: &Store, pkgs: &[Package], today: &str) {
    let _ = ledger::reconcile(&ctx.project_root, &ctx.topic, &store.all(), pkgs, today);
}

fn run(ctx: &Ctx, cmd: ScenarioCmd) -> Result<Reply> {
    let store = Store::new(&ctx.project_root);
    let today = crate::lifecycle::today();
    match &cmd {
        ScenarioCmd::Usage => return Ok(Reply::note(USAGE)),
        ScenarioCmd::Tone(arg) => {
            let Some(t) = arg.as_deref().and_then(Tone::parse) else {
                return Ok(Reply::note(format!(
                    "tone is {}; choose plain or adventure: /scenario tone adventure",
                    store.tone().as_str()
                )));
            };
            store.set_tone(t)?;
            return Ok(Reply::note(format!("tone set to {}; it applies from the next task", t.as_str())));
        }
        ScenarioCmd::List => {
            library::sync(&ctx.global)?;
            let pkgs = library::load_all(&ctx.global, &ctx.topic)?;
            sync_ledger(ctx, &store, &pkgs, &today);
            let map = map_text(ctx);
            let threads = select::threads_available(&pkgs, &map, &Ledger::load(&ctx.project_root, &ctx.topic).open_ids());
            let unfinished: Vec<RunRecord> = store
                .all()
                .into_iter()
                .filter(|r| r.topic == ctx.topic && matches!(r.state, RunState::Paused | RunState::Active))
                .collect();
            return Ok(Reply::note(render_list(&ctx.topic, &pkgs, &map, &finished_ids(&store), &threads, &unfinished, store.tone())));
        }
        _ => {}
    }

    let docker = RealDocker;
    match &cmd {
        ScenarioCmd::Start(_) | ScenarioCmd::Resume(_) => {
            ensure_ready(&docker, &SystemStarter, START_TIMEOUT, START_POLL, &|d| std::thread::sleep(d))?
        }
        ScenarioCmd::Check | ScenarioCmd::Pause => available(&docker)?,
        // Drop works without the runtime: a clean that cannot run is retried at the next start.
        _ => {}
    }
    library::sync(&ctx.global)?;
    let mut env = ContainerEnv { docker: &docker, project_tag: project_tag(&ctx.project_root) };
    let stamp = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();
    let alive: &dyn Fn(u32) -> bool = &crate::lock::pid_alive;
    let tick = Tick { pid: ctx.pid, alive, stamp: &stamp, today: &today };
    let pkgs = library::load_all(&ctx.global, &ctx.topic)?;
    let find_pkg = |id: &str| -> Result<&Package> {
        pkgs.iter().find(|p| p.id == id).ok_or_else(|| anyhow!("task '{id}' is not in the library"))
    };
    let scratch = ctx.project_root.join(".usta/scenarios");

    match cmd {
        ScenarioCmd::Start(sel) => {
            sync_ledger(ctx, &store, &pkgs, &today);
            let ledger = Ledger::load(&ctx.project_root, &ctx.topic);
            let finished = finished_ids(&store);
            let pkg = select::resolve(&pkgs, sel.as_deref(), &map_text(ctx), &finished, &ledger.open_ids()).map_err(|m| anyhow!(m))?;
            let tone = store.tone();
            let mut notices = Vec::new();
            if let Some(w) = runc_warning(&docker) {
                notices.push(w);
            }
            let rec = engine::start(&mut env, &store, pkg, tone, &tick)?;
            notices.push(pkg.tone_text(tone, "open")?);
            let req = WindowReq { command_line: &rec.access, run_id: &rec.run_id, scratch: &scratch };
            notices.push(window_notice(&SystemLauncher, &req));
            notices.push("When you think it is solved: /scenario check".into());
            Ok(Reply { notices, turn: Some(start_turn(pkg, &pkg.instruction()?, tone, &ledger.summary())) })
        }
        ScenarioCmd::Resume(sel) => {
            let mut rec = pick_paused(&store, sel.as_deref(), &ctx.topic)?;
            let pkg = find_pkg(&rec.package)?;
            engine::resume(&mut env, &store, pkg, &mut rec, &tick)?;
            let req = WindowReq { command_line: &rec.access, run_id: &rec.run_id, scratch: &scratch };
            let notices = vec![
                format!("Resumed: {}. Your files are as you left them; its programs were restarted.", pkg.title),
                window_notice(&SystemLauncher, &req),
            ];
            Ok(Reply { notices, turn: Some(resume_turn(pkg, &pkg.instruction()?, rec.last_check.as_deref())) })
        }
        ScenarioCmd::Check => {
            let mut rec = own_active(&store, ctx.pid)?;
            let pkg = find_pkg(&rec.package)?;
            let report = engine::check(&mut env, &store, pkg, &mut rec, &tick)?;
            match report.outcome {
                CheckOutcome::Pass => {
                    let tone = Tone::parse(&rec.tone).unwrap_or(Tone::Plain);
                    let mut notices = vec![pkg.tone_text(tone, "close")?, "check: script (it looked at the system state, not at the commands you typed)".into()];
                    let mut ledger = Ledger::load(&ctx.project_root, &ctx.topic);
                    if ledger.apply_finished(&pkg.story, &rec.run_id, &today) && ledger.save(&ctx.project_root, &ctx.topic).is_err() {
                        notices.push("the story notes could not be saved this time; they are added at the next start".into());
                    }
                    if let Some(w) = report.clean_warning {
                        log_error(ctx, &w);
                        notices.push("the task's server could not be removed yet; Usta will try again at the next start".into());
                    }
                    Ok(Reply { notices, turn: Some(done_turn(pkg)) })
                }
                CheckOutcome::Fail { output } => Ok(Reply::note(format!("Not yet. check: script\n{output}"))),
            }
        }
        ScenarioCmd::Pause => {
            let mut rec = own_active(&store, ctx.pid)?;
            engine::pause(&mut env, &store, &mut rec, &tick)?;
            Ok(Reply::note(format!("Paused. Continue later with /scenario resume {}", rec.run_id)))
        }
        ScenarioCmd::Drop { id, confirmed } => {
            let mut rec = pick_unfinished(&store, id.as_deref(), &ctx.topic, ctx.pid)?;
            let title = pkgs
                .iter()
                .find(|p| p.id == rec.package)
                .map(|p| p.title.clone())
                .unwrap_or_else(|| rec.package.clone());
            if !confirmed {
                return Ok(Reply::note(format!(
                    "This removes your work on '{title}' for good, and nothing from it counts toward your progress. To go ahead: /scenario drop {} confirm",
                    rec.run_id
                )));
            }
            let mut notices = vec![format!("Dropped '{title}'. Its server was removed.")];
            if let Some(w) = engine::drop_run(&mut env, &store, &mut rec, &tick)? {
                log_error(ctx, &w);
                notices = vec![format!("Dropped '{title}'. Its server could not be removed right now; Usta will try again at the next start.")];
            }
            Ok(Reply { notices, turn: None })
        }
        ScenarioCmd::Usage | ScenarioCmd::Tone(_) | ScenarioCmd::List => unreachable!("handled above"),
    }
}

fn own_active(store: &Store, pid: u32) -> Result<RunRecord> {
    store
        .all()
        .into_iter()
        .find(|r| r.state == RunState::Active && r.owner_pid == pid)
        .ok_or_else(|| anyhow!("no active task; use /scenario start or /scenario resume"))
}

fn pick_paused(store: &Store, sel: Option<&str>, topic: &str) -> Result<RunRecord> {
    let paused: Vec<RunRecord> = store
        .all()
        .into_iter()
        .filter(|r| r.state == RunState::Paused && r.topic == topic)
        .collect();
    match sel {
        Some(id) => paused.into_iter().find(|r| r.run_id == id).ok_or_else(|| anyhow!("no paused task '{id}'; see /scenario")),
        None => paused.into_iter().last().ok_or_else(|| anyhow!("nothing to resume; see /scenario")),
    }
}

/// Unfinished = paused, or active and owned by this process. Never finished, dropped, another topic's or another Usta's.
pub(crate) fn pick_unfinished(store: &Store, sel: Option<&str>, topic: &str, pid: u32) -> Result<RunRecord> {
    let open: Vec<RunRecord> = store
        .all()
        .into_iter()
        .filter(|r| {
            r.topic == topic
                && (r.state == RunState::Paused || (r.state == RunState::Active && r.owner_pid == pid))
        })
        .collect();
    match sel {
        Some(id) => open.into_iter().find(|r| r.run_id == id).ok_or_else(|| anyhow!("no unfinished task '{id}'; see /scenario")),
        None => match open.len() {
            0 => bail!("no unfinished task; see /scenario"),
            1 => Ok(open.into_iter().next().expect("one element")),
            _ => bail!(
                "several unfinished tasks; name one: {}",
                open.iter().map(|r| format!("/scenario drop {}", r.run_id)).collect::<Vec<_>>().join(" · ")
            ),
        },
    }
}

/// Holds the session's identity; pauses runs this process owns when dropped
/// (normal close, `?` error, unwind). A hard kill is handled by the startup sweep.
pub(crate) struct Slot {
    ctx: Ctx,
}

impl Slot {
    pub fn new(global: &Path, project_root: &Path, topic: &str) -> Slot {
        Slot {
            ctx: Ctx {
                global: global.to_path_buf(),
                project_root: project_root.to_path_buf(),
                topic: topic.to_string(),
                pid: std::process::id(),
            },
        }
    }

    pub async fn handle(&self, cmd: ScenarioCmd) -> Reply {
        let ctx = self.ctx.clone();
        tokio::task::spawn_blocking(move || execute(&ctx, cmd))
            .await
            .unwrap_or_else(|e| Reply::note(format!("scenario: internal error: {e}")))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let store = Store::new(&self.ctx.project_root);
        if !store.exists() {
            return;
        }
        let docker = RealDocker;
        let mut env = ContainerEnv { docker: &docker, project_tag: project_tag(&self.ctx.project_root) };
        let today = crate::lifecycle::today();
        let alive: &dyn Fn(u32) -> bool = &crate::lock::pid_alive;
        let tick = Tick { pid: self.ctx.pid, alive, stamp: "", today: &today };
        for mut rec in store.all() {
            if rec.state == RunState::Active && rec.owner_pid == self.ctx.pid {
                let _ = engine::pause(&mut env, &store, &mut rec, &tick);
            }
        }
    }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test scenario::command 2>&1 | tail -5`
Expected: `test result: ok. 11 passed`. None of these tests needs Docker: tone, usage and an empty drop return before any Docker call.

- [ ] **Step 5: Format and commit**

```bash
rustfmt --edition 2021 src/scenario/command.rs src/scenario/mod.rs
git add src/scenario/command.rs src/scenario/mod.rs
git commit -m "feat(scenario): /scenario commands, drop, turns and session slot"
```

---

### Task 13: Wire into the session loops, help, and pre-topic pointers

**Files:**
- Modify: `src/tui/run.rs` (Slot after session setup; branch before the `/game` block, ~line 301; `images`/`outgoing` chain at ~331-338)
- Modify: `src/plain.rs` (Slot near the loop start ~line 262; branch before the `/game` block ~line 299; swap chain ~line 328)
- Modify: `src/tui/topic_flow.rs:168-171`, `src/tui/intro.rs:220-223`, `src/plain.rs:88` (pre-topic pointer)
- Modify: `src/help.rs` (in-session command list + test needles)
- Modify: `src/scenario/mod.rs` (remove `#![allow(dead_code)]`)

**Interfaces:**
- Consumes: `scenario::command::{parse, busy_note, Slot, ScenarioCmd}` (Task 12)

- [ ] **Step 1: Write the failing test (help)**

In `src/help.rs` test `help_text_lists_shortcuts_commands_and_cli`, append `"/scenario",` to the `needle` array after `"/game on|off",`. Run `cargo test help 2>&1 | tail -4`. Expected: FAIL (`help_text missing: /scenario`).

- [ ] **Step 2: Add the help line**

In `help_text()` add, after the `/game on|off` line, one line in the same escaped style:

```
     \x20\x20/scenario        practice tasks: start | resume | check | pause | drop | tone
```

Run `cargo test help 2>&1 | tail -4`. Expected: PASS.

- [ ] **Step 3: Pre-topic pointers (three one-line edits)**

Append `|| crate::scenario::command::parse(&raw).is_some()` to the condition in `src/tui/topic_flow.rs` (the `if crate::visual::parse_show_command(&raw).is_some() || ...` block at ~line 168) and in `src/tui/intro.rs` (~line 220). In `src/plain.rs:88` change the condition to `if visual::parse_show_command(raw).is_some() || parse_watch_command(raw).is_some() || crate::scenario::command::parse(raw).is_some() {`.

- [ ] **Step 4: TUI loop wiring (`src/tui/run.rs`)**

Right after the `lock`/`build_session` section, where `session`, `recorder`, `topic`, `global`, `project_root` are all bound and before the main `loop`, add:

```rust
    let scn = crate::scenario::command::Slot::new(global, project_root, &topic);
```

In the `Action::Submit(line)` arm, directly BEFORE the line `// /game: toggle persists ...` (`let game_cmd = ...`), insert:

```rust
                            // /scenario: shell-only notices; start, resume and a passing check also yield one turn for the normal ask flow below.
                            let scenario_turn = match crate::scenario::command::parse(&line) {
                                None => None,
                                Some(cmd) => {
                                    crate::tui::page::page_user_echo(&mut tui, &line)?;
                                    if let Some(b) = crate::scenario::command::busy_note(&cmd) { crate::tui::page::page_notice(&mut tui, b)?; }
                                    let reply = scn.handle(cmd).await;
                                    for n in &reply.notices { crate::tui::page::page_notice(&mut tui, n)?; }
                                    match reply.turn { Some(t) => Some(t), None => continue }
                                }
                            };
```

Change the next two statements:

```rust
let images = if crate::slash::is_exam_command(&line) || game_cmd.is_some() || scenario_turn.is_some() { Vec::new() } else { editor.take_images() };
let outgoing = if let Some(t) = scenario_turn {
    t
} else if crate::slash::is_exam_command(&line) {
```

(the rest of the chain is unchanged; the `/scenario` literal is already echoed above).

- [ ] **Step 5: Plain loop wiring (`src/plain.rs`)**

At the start of `run_plain_loop`'s `loop` setup, next to `let mut watching = true;` add:

```rust
    let scn = crate::scenario::command::Slot::new(ctx.global, ctx.project_root, ctx.topic);
```

Directly BEFORE `let game_cmd = parse_game_command(&line);` insert:

```rust
                    let scenario_turn = match crate::scenario::command::parse(&line) {
                        None => None,
                        Some(cmd) => {
                            if let Some(b) = crate::scenario::command::busy_note(&cmd) { ui::notice(b); }
                            let reply = scn.handle(cmd).await;
                            for n in &reply.notices { ui::notice(n); }
                            match reply.turn {
                                Some(t) => Some(t),
                                None => { let _ = ready_tx.send(()); continue; }
                            }
                        }
                    };
```

Change the swap chain head `let line = if is_exam_command(&line) {` to:

```rust
                    let line = if let Some(t) = scenario_turn {
                        t
                    } else if is_exam_command(&line) {
```

- [ ] **Step 6: Remove the temporary allow and build**

Delete `#![allow(dead_code)] // removed in Task 13 ...` from `src/scenario/mod.rs`. Run:

`cargo build 2>&1 | tail -20`

Expected: builds. Any remaining `dead_code` warning names an item built but never used; delete that item if it is truly unused (for example a helper added speculatively) or, if it is used only by tests, mark it `#[cfg(test)]`. Do not re-add the blanket allow.

- [ ] **Step 7: Full gate**

Run:

```bash
cargo fmt --check 2>&1 | tail -3
cargo clippy --all-targets -- -D warnings 2>&1 | tail -15
cargo test 2>&1 | tail -6
```

Expected: fmt clean (if a touched existing file shows diffs, run `rustfmt --edition 2021 <that file>` and re-check that the diff is only your lines), clippy clean, all tests pass with the Docker tests `ignored`.

- [ ] **Step 8: Commit**

```bash
git add src/tui/run.rs src/plain.rs src/tui/topic_flow.rs src/tui/intro.rs src/help.rs src/scenario/mod.rs
git commit -m "feat(scenario): wire /scenario into the TUI and plain loops"
```

---

### Task 14: Version, docs, budget audit, manual check

**Files:**
- Modify: `Cargo.toml` (`version = "0.40.0"`), `Cargo.lock` (via cargo)
- Modify: `src/tui/welcome_tests.rs:8` (pin `"0.40.0"`)
- Modify: `SPEC.md` (new section before `## 5. Flow`, and one Decisions line)
- Modify: `README.md` (in-session commands line ~93)

- [ ] **Step 1: Bump the version**

Set `version = "0.40.0"` in `Cargo.toml`, change the assertion in `src/tui/welcome_tests.rs:8` to `"0.40.0"`, run `cargo build` (updates `Cargo.lock`).

- [ ] **Step 2: SPEC.md section**

Insert before the `## 5. Flow (one learning session)` heading (after §4.23):

```markdown
## 4.24 Scenario Mode, step 1 (v0.40)

Usta brings the work: `/scenario` starts a packaged practice task inside the current topic's session. Design: `docs/superpowers/specs/2026-11-04-scenario-mode-design.md`; plan: `docs/superpowers/plans/2026-11-04-scenario-mode-step-1.md`.

- **Engine and contract:** `src/scenario/engine.rs` drives a run through the five-operation `Environment` trait (`prepare`, `pause`, `resume`, `check`, `clean`); it never knows which environment it runs in. Only `container` exists (`container.rs`, over the `docker` CLI via a `Docker` trait so command lines are unit-tested without Docker).
- **Package format:** `task.toml`, `instruction.md`, `environment/`, `solution/`, `tests/`, plus `tone/adventure.md` (Terminal-Bench/Harbor shape, reduced). `skills` are lowercase keywords matched against free-text curriculum-map lines. Built-in packages (`scenarios/`) are embedded and synced to `~/.config/usta/scenarios/library/` on first use.
- **No work for the learner:** Usta starts the runtime if it is installed but stopped (waits up to 90 s, plain messages on failure; install guidance if missing) and opens a new terminal window already inside the task (`terminal.rs`: macOS `.command` file via `open`, Linux known terminals, Windows `cmd /c start`; the command is shown only if no window can open). Learner text says task or server; tool output goes to `.usta/scenarios/last-error.log`.
- **World ledger:** `.usta/scenarios/ledger/<topic>.toml`, shell-owned (company, characters, events, threads); updated when a scenario finishes, read when the next opens; packages declare `story.{company,characters,event,opens,closes,needs}`; a task whose `needs` are not open is `later in the story`.
- **Drop:** `/scenario drop <id>` shows what is lost, `... confirm` cleans the environment and records the run as `dropped`.
- **Lifecycle:** no `--rm`; one task one container; `--memory/--cpus/--pids-limit`; network off unless declared; `usta-<project-tag>-<run>` names with `usta=1` and `usta.project=<tag>` labels, so cleanup only ever touches this project's containers. One container runs per project; another project's running container makes `prepare`/`resume` refuse. Closing Usta pauses the run (`Slot` drop); the startup sweep stops crash survivors, removes leftovers of finished tasks and unrecorded containers, and asks (never deletes) about a paused task untouched for 14 days.
- **Checks:** run inside the container only at check time (tests copied in, executed under `timeout`, removed right after); they read system state, never shell history (lint-enforced). Handwritten packages are certified by the `#[ignore]`d Docker tests: the check fails on a fresh environment and passes after the reference solution.
- **Records and tone:** run records are shell-owned TOML under `<project>/.usta/scenarios/runs/`; tone (`plain`/`adventure`) lives in `.usta/scenarios/tone`, not USER.md (the closing flush rewrites USER.md). The shell never writes the curriculum map: a passing check injects a `[SCENARIO DONE]` turn and the closing flush records the evidence.
- **Not in step 1:** model generation/validation (step 2), other environments, project-folder mount, due-return (the map carries no `due:`), the embedded terminal pane inside Usta (later step), removing the Docker dependency.
```

Add to §11 Decisions Made one line: `- **Scenario mode (v0.40.0):** see §4.24; containers are scoped by a per-project label, tone is shell-owned, the closing flush owns the curriculum map. Version: \`0.40.0\`.`

- [ ] **Step 3: README line**

In the in-session commands sentence (`README.md` ~line 93) add `· /scenario (practice tasks: start, resume, check, pause, drop, tone)` in the same style as its neighbours.

- [ ] **Step 4: Budget audit**

Run:

```bash
for f in src/plain.rs src/tui/run.rs src/main.rs src/help.rs src/scenario/*.rs; do printf '%s %s\n' "$f" "$(awk '/#\[cfg\(test\)\]/{exit} {n++} END{print n}' "$f")"; done
```

Expected: every line is below 600 (plain.rs about 525, tui/run.rs about 448, command.rs about 430). If any file reaches 600, split before finishing (for example move `start_turn`/`resume_turn`/`done_turn`/`render_list` from `command.rs` to a `turns.rs`, with their tests).

- [ ] **Step 5: Safety audit (no `kiln-*` exposure)**

Run: `grep -rn 'kiln' src/scenario scenarios || echo none` (expected `none`), and `grep -rn '"rm"\|"stop"' src/scenario/*.rs | grep -v tests` and confirm each call receives a `usta-`-prefixed reference taken from a record or from `list_labeled` (label-filtered).

Learner-text audit: `grep -n -i 'container\|docker\|image' src/scenario/command.rs src/scenario/engine.rs src/scenario/select.rs src/scenario/ledger.rs src/scenario/sweep.rs src/scenario/package.rs | grep -v '^.*://'` must show only comments and model-facing prompts that forbid the words; the only learner-visible occurrences of "Docker" are in `docker.rs` (install/start messages, runc warning) and `terminal.rs` (the fallback command).

- [ ] **Step 6: Full gate and manual check**

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | tail -4
```

Manual check on a machine with Docker (the executing session decides; never against `kiln-*`): `cd` into a scratch project, `usta start linux`, then `/scenario`, `/scenario start` (expect: the opening text, a new terminal window already inside the task, no Docker command anywhere on screen; on macOS note whether any permission prompt appeared and that the `.command` window opens in your terminal app), work in that window, `/scenario check` (expect "Not yet"), fix per the task, `/scenario check` (expect the close text, then `docker ps -a --filter label=usta=1` shows no container, and `.usta/scenarios/ledger/linux.toml` holds the company, Deniz, the event and the open thread), `/scenario` (the next task is now `new`, the one needing a later thread shows `later in the story`), `/scenario start`, `/quit`, `docker ps -a` shows the task Exited, restart `usta start linux`, `/scenario resume` (window reopens, files intact), `/scenario drop` then `/scenario drop <id> confirm` (task removed, record says dropped). Also quit Docker Desktop and run `/scenario start`: expect Usta to start it and continue, or a plain message after 90 s. Check which Linux/Windows launchers work if you have those systems; any that do not must fall back to the shown command, not fail. Record the outcome in the PR description; also note disk size of the three images and stopped containers (`docker system df`), which the spec lists as open measurements.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src/tui/welcome_tests.rs SPEC.md README.md
git commit -m "chore: v0.40.0 scenario mode step 1"
```

---

## Self-review

**1. Spec coverage (step 1 scope plus the "No work for the learner" section).**
- Scenario engine: Task 3. Five-operation contract: Task 2 (`Environment`), implemented once in Task 7, fake in Task 3 tests.
- Container environment, full lifecycle: Task 7 (no `--rm`, limits, network off, labels, single-run guard, stop/start, cleanup) and Task 11 (startup sweep: orphan removal, finished and dropped leftovers, stale question, crash recovery); stop/start resume with start-script rerun is certified in Task 10.
- Check-script rules: Task 7 (copy in at check time, remove after, timeout), Task 1 lint (no history), Tasks 9 and 10 scripts check state and accept any valid solution; runc warning: Task 6 (verification step gated).
- Package format on disk: Task 1 (now including the `story` fields); library and sync: Task 9.
- First Linux scenarios, plain and adventure, forming one continuing story: Tasks 9 and 10 (backup, then disk, then config, chained by `story.needs`/`opens`).
- Start, resume, finish, drop from inside Usta: Tasks 12 and 13. Drop: engine Task 3, sweep Task 11, command Task 12.
- World ledger: Task 5 (store, idempotent apply, reconcile after a crash), prerequisites in Task 4 (`Later`, skipped packages open their threads so the story never locks), read at task open in `start_turn` (Task 12), updated on pass (Task 12).
- No work for the learner: runtime start with timeout and install guidance (Task 6), terminal window with per-platform seam, fake launcher and fallback (Task 8), no Docker words in learner text enforced by a test and a grep audit (Tasks 12 and 14), tool output kept out of sight via `Internal` errors (Tasks 2, 7, 12).
- Skip finished skills and variety: Task 4. Due-return: not built (constraint 2).
- Spec tests: contract via fake environment (Task 3), lifecycle tests for resume after stop, crash leftover and survival of unfinished tasks (Tasks 10, 11), Docker tests separated by `#[ignore]` (Task 10). Validation of generated packages is step 2; step 1 ships the two-condition certification for handwritten packages only.
- Recorded gaps (stated constraints, not hidden): project-folder mount, idle-timeout stop, embedded terminal pane, other environments, Docker removal.

**2. Placeholder scan.** No TBD/TODO text; every code step contains code. One deliberate conditional: Task 6 Step 0 requires verifying the runc numbers before committing and says what to do if they differ or cannot be verified. Platform launch methods (Tasks 6 and 8) are explicitly marked unverified with a defined fallback and a manual check in Task 14.

**3. Type consistency.** `Tick { pid, alive, stamp, today }` (Task 3) is used with the same fields in Tasks 11 and 12. `EnvHandle { reference, access }`, `CheckOutcome::{Pass, Fail { output }}`, `Internal`/`internal()` (Task 2, used in Tasks 7 and 12), `RunRecord::handle()`, `RunState::{Preparing, Active, Paused, Finished, Dropped}`, `Store::{new, exists, save, load, remove, all, tone, set_tone}`, `write_atomic` (Task 2, used by Task 5), `project_tag`, `Found`, `list_labeled`, `ContainerEnv { docker, project_tag }`, `select::{tag(pkg, map, finished, threads), threads_available(pkgs, map, open), resolve(pkgs, arg, map, finished, open)}` (Task 4, used in Task 12 with the same argument order), `Ledger::{load, save, apply_finished, open_ids, summary}` and `reconcile` (Task 5, used in Task 12), `StorySpec`/`Character`/`ThreadSpec` (Task 1, used in Tasks 4, 5), `ensure_ready`, `available`, `SystemStarter`, `START_TIMEOUT`, `START_POLL` (Task 6, used in Task 12), `WindowReq`, `Launcher`, `SystemLauncher`, `window_notice` (Task 8, used in Task 12), `engine::{start, resume, pause, check, drop_run}` (Task 3), `ScenarioCmd::Drop { id, confirmed }`, `Reply`, `Ctx`, `Slot` (Task 12, used in Task 13) are named identically wherever used. Test helpers `package::tests::{tmp, write_pkg}` are `pub(crate)` inside a `pub(crate) mod tests`; `docker::fake::{FakeDocker, ok, fail}` is used by Tasks 6 and 7.
