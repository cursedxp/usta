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

fn d_env() -> String {
    crate::scenario::env::CONTAINER.into()
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
    /// Which environment runs it. Older records have no such field and are all container runs.
    #[serde(default = "d_env")]
    pub env: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_check: Option<String>,
}

impl RunRecord {
    pub fn handle(&self) -> EnvHandle {
        EnvHandle {
            reference: self.reference.clone(),
            access: self.access.clone(),
            kind: self.env.clone(),
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

    /// True when a run file exists for `id`, readable or not.
    pub fn has_file(&self, id: &str) -> bool {
        self.run_path(id).is_file()
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

/// The project's identity: labels every container so one project's cleanup can
/// never touch another project's. `.usta/scenarios/tag` holds the tag and the
/// folder it belongs to (an older file holds only the tag). A new project gets a
/// unique tag, never one derived from its path alone: a later project at the same
/// path must not inherit a moved project's containers. A lost or damaged file is
/// rebuilt from an unfinished record, so its tasks still match. When the folder is
/// somewhere else, the old location decides: gone means it moved, so the tag
/// stays and records and containers still match; still there means this is a
/// copy, which gets a fresh tag so neither folder's cleanup can touch the other's
/// work. Its copied records then count as foreign, and foreign records never
/// lead to a removal.
pub(crate) fn project_tag(project_root: &Path) -> String {
    let file = project_root.join(".usta/scenarios/tag");
    let here = canon(project_root);
    let tag = match read_tag(&file) {
        Some((tag, Some(path))) if path == here => return tag,
        Some((tag, Some(path))) if is_original(&path, &here, &tag) => fresh_tag(&here, &tag),
        Some((tag, _)) => tag,
        None => recovered_tag(project_root).unwrap_or_else(|| fresh_tag(&here, "")),
    };
    let _ = write_atomic(&file, &format!("{tag}\n{}\n", here.display()));
    tag
}

/// The tag inside a `usta-<tag>-<run>` reference.
fn tag_in(reference: &str) -> Option<&str> {
    let (tag, _) = reference.strip_prefix("usta-")?.split_once('-')?;
    valid_tag(tag).then_some(tag)
}

/// Only unfinished records: a copy's dropped ones carry the original's tag.
fn recovered_tag(project_root: &Path) -> Option<String> {
    Store::new(project_root)
        .all()
        .iter()
        .rev()
        .filter(|r| {
            matches!(
                r.state,
                RunState::Preparing | RunState::Active | RunState::Paused
            )
        })
        .find_map(|r| tag_in(&r.reference).map(String::from))
}

fn valid_tag(tag: &str) -> bool {
    tag.len() == 8 && tag.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
}

fn canon(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The tag and, when the file has one, the folder it was written for. None when
/// the file is missing or its tag is damaged.
fn read_tag(file: &Path) -> Option<(String, Option<PathBuf>)> {
    let text = std::fs::read_to_string(file).ok()?;
    let mut lines = text.lines();
    let tag = lines.next()?.trim();
    if !valid_tag(tag) {
        return None;
    }
    let path = lines.next().filter(|p| !p.is_empty()).map(PathBuf::from);
    Some((tag.to_string(), path))
}

/// True when `stored` is the live folder this one was copied from: it still holds
/// this very tag. Another name for the same folder is not, and neither is an
/// unrelated project that took over the path after a move, with a tag of its own
/// or none yet (`/scenario tone` makes the folder without one).
fn is_original(stored: &Path, here: &Path, tag: &str) -> bool {
    let scenarios = stored.join(".usta/scenarios");
    scenarios.is_dir()
        && canon(stored) != here
        && read_tag(&scenarios.join("tag")).is_some_and(|(t, _)| t == tag)
}

/// Unique per call: the clock goes into the hash, so the same path never yields
/// the same tag twice.
fn fresh_tag(here: &Path, old: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    path_tag(&here.join(format!("{old}-{nanos}")))
}

/// 8-hex hash of a path (FNV-1a).
fn path_tag(project_root: &Path) -> String {
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
            env: "container".into(),
            last_check: None,
        }
    }

    #[test]
    fn an_old_record_without_an_environment_is_a_container_run() {
        let p = tmp("store_old_env");
        let s = Store::new(&p);
        std::fs::create_dir_all(p.join(".usta/scenarios/runs")).unwrap();
        std::fs::write(
            p.join(".usta/scenarios/runs/a.toml"),
            "run_id = \"a\"\npackage = \"p\"\ntopic = \"t\"\ntone = \"plain\"\nstate = \"paused\"\nowner_pid = 0\nstarted = \"d\"\nupdated = \"d\"\n",
        )
        .unwrap();
        assert_eq!(s.load("a").unwrap().env, "container");
        assert_eq!(s.load("a").unwrap().handle().kind, "container");
        let _ = std::fs::remove_dir_all(p);
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
    fn path_tag_is_stable_eight_hex_and_differs_per_path() {
        let a = path_tag(Path::new("/work/a"));
        assert_eq!(a, path_tag(Path::new("/work/a")));
        assert_eq!(a.len(), 8);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, path_tag(Path::new("/work/b")));
    }

    #[test]
    fn the_tag_is_persisted_once_and_survives_a_moved_folder() {
        let base = tmp("store_tag_move");
        let (old, new) = (base.join("old"), base.join("new"));
        std::fs::create_dir_all(&old).unwrap();
        let tag = project_tag(&old);
        assert_eq!(
            stored(&old),
            (tag.clone(), canon(&old).display().to_string())
        );
        std::fs::rename(&old, &new).unwrap();
        assert_eq!(
            project_tag(&new),
            tag,
            "the moved folder keeps its identity"
        );
        assert_eq!(
            stored(&new).1,
            canon(&new).display().to_string(),
            "path updated"
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_damaged_tag_file_is_replaced_never_used() {
        let root = tmp("store_tag_bad");
        for bad in ["", "not-hex!", "aaaa1111 --filter x", "AAAA11112"] {
            std::fs::create_dir_all(root.join(".usta/scenarios")).unwrap();
            std::fs::write(root.join(".usta/scenarios/tag"), bad).unwrap();
            let tag = project_tag(&root);
            assert!(tag.len() == 8 && tag != bad, "{bad:?}: {tag}");
            assert_eq!(project_tag(&root), tag, "{bad:?}: stable once replaced");
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_copied_folder_gets_a_fresh_tag_and_the_original_keeps_its_own() {
        let base = tmp("store_tag_copy");
        let (orig, copy) = (base.join("orig"), base.join("copy"));
        std::fs::create_dir_all(&orig).unwrap();
        std::fs::create_dir_all(&copy).unwrap();
        let tag = project_tag(&orig);
        copy_tag_file(&orig, &copy);
        let fresh = project_tag(&copy);
        assert_ne!(fresh, tag);
        assert_eq!(
            stored(&copy),
            (fresh.clone(), canon(&copy).display().to_string())
        );
        assert_eq!(project_tag(&copy), fresh, "stable after the first read");
        assert_eq!(project_tag(&orig), tag);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_copy_made_at_the_path_the_tag_was_hashed_from_still_differs() {
        let base = tmp("store_tag_copy_back");
        let (a, b) = (base.join("a"), base.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        let tag = project_tag(&a);
        std::fs::rename(&a, &b).unwrap();
        assert_eq!(project_tag(&b), tag);
        std::fs::create_dir_all(&a).unwrap();
        copy_tag_file(&b, &a);
        assert_ne!(project_tag(&a), tag);
        assert_eq!(project_tag(&b), tag);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn the_old_tag_only_format_keeps_its_tag_and_gains_the_path() {
        let root = tmp("store_tag_old");
        std::fs::create_dir_all(root.join(".usta/scenarios")).unwrap();
        std::fs::write(root.join(".usta/scenarios/tag"), "abcd1234\n").unwrap();
        assert_eq!(project_tag(&root), "abcd1234");
        assert_eq!(
            stored(&root),
            ("abcd1234".to_string(), canon(&root).display().to_string())
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn the_same_folder_through_a_symlink_is_not_a_copy() {
        let base = tmp("store_tag_link");
        let (real, link) = (base.join("real"), base.join("link"));
        std::fs::create_dir_all(&real).unwrap();
        let tag = project_tag(&real);
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(project_tag(&link), tag);
        assert_eq!(project_tag(&real), tag);
        let _ = std::fs::remove_dir_all(base);
    }

    /// The two lines of the tag file.
    fn stored(root: &Path) -> (String, String) {
        let text = std::fs::read_to_string(root.join(".usta/scenarios/tag")).unwrap();
        let mut l = text.lines();
        (l.next().unwrap().into(), l.next().unwrap_or("").into())
    }

    fn copy_tag_file(from: &Path, to: &Path) {
        std::fs::create_dir_all(to.join(".usta/scenarios")).unwrap();
        std::fs::copy(
            from.join(".usta/scenarios/tag"),
            to.join(".usta/scenarios/tag"),
        )
        .unwrap();
    }

    fn run_with_reference(root: &Path, id: &str, state: RunState, reference: &str) {
        let mut r = rec(id, state, "2026-11-04");
        r.reference = reference.into();
        Store::new(root).save(&r).unwrap();
    }

    #[test]
    fn the_first_tag_is_not_a_function_of_the_path() {
        let root = tmp("store_tag_unique");
        std::fs::create_dir_all(&root).unwrap();
        let first = project_tag(&root);
        assert_ne!(first, path_tag(&root));
        std::fs::remove_dir_all(&root).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        assert_ne!(project_tag(&root), first, "a new project at the same path");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_lost_or_damaged_tag_file_is_recovered_from_an_unfinished_record() {
        let root = tmp("store_tag_recover");
        std::fs::create_dir_all(&root).unwrap();
        let tag = project_tag(&root);
        let file = root.join(".usta/scenarios/tag");
        run_with_reference(&root, "a", RunState::Paused, &format!("usta-{tag}-a"));
        std::fs::remove_file(&file).unwrap();
        assert_eq!(project_tag(&root), tag, "lost");
        std::fs::write(&file, "broken").unwrap();
        assert_eq!(project_tag(&root), tag, "damaged");
        assert_eq!(stored(&root).0, tag, "and written back");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn only_a_well_formed_reference_of_an_unfinished_run_is_trusted() {
        let root = tmp("store_tag_no_recover");
        run_with_reference(&root, "d", RunState::Finished, "usta-abcd1234-d");
        run_with_reference(&root, "x", RunState::Dropped, "usta-abcd1234-x");
        run_with_reference(&root, "k", RunState::Paused, "kiln-db");
        run_with_reference(&root, "s", RunState::Paused, "usta-short-s");
        run_with_reference(&root, "e", RunState::Paused, "");
        assert_ne!(project_tag(&root), "abcd1234");
        let _ = std::fs::remove_dir_all(root);
    }
}
