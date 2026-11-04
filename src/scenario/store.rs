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
        let text = std::fs::read_to_string(&path).with_context(|| format!("no such run: {id}"))?;
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
