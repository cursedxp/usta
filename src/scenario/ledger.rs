//! World ledger: the continuing story of one topic. Shell-owned TOML under
//! `<project>/.usta/scenarios/ledger/<topic>.toml`; the model never writes it.
//! Packages declare what they add and need through their `story` field.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::scenario::library::topic_matches;
use crate::scenario::package::{Package, StorySpec};
use crate::scenario::store::{write_atomic, RunRecord, RunState};
use crate::ui;

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

fn corrupt_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".corrupt");
    PathBuf::from(name)
}

/// Copy an unreadable ledger to `<file>.corrupt`, once, with one warning.
fn set_aside(path: &Path) {
    let aside = corrupt_path(path);
    if aside.exists() {
        return;
    }
    ui::warn(&match std::fs::copy(path, &aside) {
        Ok(_) => format!(
            "the story notes could not be read; a copy is in {}, and the story starts fresh",
            aside.display()
        ),
        Err(_) => "the story notes could not be read or copied; the story starts fresh".into(),
    });
}

impl Ledger {
    /// A missing file is a fresh story. An unreadable one starts empty too, but a
    /// save would overwrite it, so it is set aside first.
    pub fn load(project_root: &Path, topic: &str) -> Ledger {
        let path = ledger_path(project_root, topic);
        let text = match std::fs::read_to_string(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ledger::default(),
            other => other,
        };
        match text.ok().and_then(|t| toml::from_str(&t).ok()) {
            Some(ledger) => ledger,
            None => {
                set_aside(&path);
                Ledger::default()
            }
        }
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
                self.characters.push(Character {
                    name: c.name.clone(),
                    role: c.role.clone(),
                });
            }
        }
        for o in &story.opens {
            match self.threads.iter_mut().find(|t| t.id == o.id) {
                Some(t) => {
                    t.open = true;
                    t.text = o.text.clone();
                }
                None => self.threads.push(Thread {
                    id: o.id.clone(),
                    text: o.text.clone(),
                    open: true,
                }),
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
        self.threads
            .iter()
            .filter(|t| t.open)
            .map(|t| t.id.clone())
            .collect()
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
            let cs: Vec<String> = self
                .characters
                .iter()
                .map(|c| format!("{} ({})", c.name, c.role))
                .collect();
            out.push_str(&format!("people: {}. ", cs.join(", ")));
        }
        if !events.is_empty() {
            let skip = events.len().saturating_sub(SUMMARY_EVENTS);
            let es: Vec<String> = events
                .iter()
                .skip(skip)
                .map(|e| format!("[{}] {};", e.date, e.text))
                .collect();
            out.push_str(&format!("past: {} ", es.join(" ")));
        }
        let open: Vec<&str> = self
            .threads
            .iter()
            .filter(|t| t.open)
            .map(|t| t.text.as_str())
            .collect();
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
    for r in records
        .iter()
        .filter(|r| r.state == RunState::Finished && topic_matches(&r.topic, topic))
    {
        if let Some(p) = pkgs.iter().find(|p| p.id == r.package) {
            changed |= ledger.apply_finished(&p.story, &r.run_id, today);
        }
    }
    if changed {
        ledger.save(project_root, topic)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::package::tests::tmp;
    use crate::scenario::package::{Character as PChar, StorySpec, ThreadSpec};

    fn story() -> StorySpec {
        StorySpec {
            company: Some("Shopwell".into()),
            characters: vec![PChar {
                name: "Deniz".into(),
                role: "runs the servers at night".into(),
            }],
            event: Some("The nightly backup was repaired.".into()),
            opens: vec![ThreadSpec {
                id: "backups-ok".into(),
                text: "Backups run every night again.".into(),
            }],
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
        let close = StorySpec {
            closes: vec!["backups-ok".into()],
            ..StorySpec::default()
        };
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
        for want in [
            "Shopwell",
            "Deniz",
            "runs the servers at night",
            "2026-11-04",
            "The nightly backup was repaired.",
            "Backups run every night again.",
        ] {
            assert!(s.contains(want), "{want} not in {s}");
        }
    }

    #[test]
    fn summary_keeps_only_the_latest_events() {
        let mut l = Ledger::default();
        for i in 0..12 {
            let s = StorySpec {
                event: Some(format!("event-{i}")),
                ..StorySpec::default()
            };
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

    #[test]
    fn a_missing_ledger_is_empty_and_leaves_no_copy() {
        let root = tmp("ledger_missing");
        assert_eq!(Ledger::load(&root, "linux"), Ledger::default());
        assert!(!corrupt_path(&ledger_path(&root, "linux")).exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_unreadable_ledger_is_copied_aside_once_before_it_is_overwritten() {
        let root = tmp("ledger_corrupt");
        let path = ledger_path(&root, "linux");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "company = [broken").unwrap();
        let aside = corrupt_path(&path);
        assert_eq!(aside.file_name().unwrap(), "linux.toml.corrupt");

        let mut ledger = Ledger::load(&root, "linux");
        assert_eq!(ledger, Ledger::default());
        assert_eq!(
            std::fs::read_to_string(&aside).unwrap(),
            "company = [broken"
        );

        // A second load does not copy again: what was saved first stays.
        std::fs::write(&path, "company = [other").unwrap();
        Ledger::load(&root, "linux");
        assert_eq!(
            std::fs::read_to_string(&aside).unwrap(),
            "company = [broken"
        );

        ledger.apply_finished(&story(), "r1", "2026-11-04");
        ledger.save(&root, "linux").unwrap();
        assert_eq!(Ledger::load(&root, "linux"), ledger);
        assert_eq!(
            std::fs::read_to_string(&aside).unwrap(),
            "company = [broken"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_ledger_that_is_not_text_counts_as_unreadable() {
        let root = tmp("ledger_binary");
        let path = ledger_path(&root, "linux");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(Ledger::load(&root, "linux"), Ledger::default());
        assert_eq!(
            std::fs::read(corrupt_path(&path)).unwrap(),
            [0xff, 0xfe, 0x00]
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
