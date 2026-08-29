//! Background file watcher (`notify` v6). Forwards saved files to the REPL
//! for proactive Socratic feedback.
//!
//! `notify` runs synchronously — we don't force it into async. Background
//! thread + std mpsc is the right pattern: the watcher is kept alive inside
//! the thread, changed paths flow through the channel.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use notify::event::ModifyKind;
use notify::{EventKind, RecursiveMode, Watcher};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
use tokio::time::Instant;

/// Watch recursively under `root`; send file paths from modify events.
/// The returned receiver is consumed in the select-loop with `recv().await`
/// (or `try_recv`).
pub fn spawn(root: &Path) -> Result<UnboundedReceiver<PathBuf>> {
    let (out_tx, out_rx) = unbounded_channel::<PathBuf>();
    let (ev_tx, ev_rx) = mpsc::channel::<notify::Result<notify::Event>>();

    let mut watcher = notify::recommended_watcher(move |res| {
        // If the receiver has been dropped, swallow silently — the thread will close soon.
        let _ = ev_tx.send(res);
    })
    .context("failed to create file watcher")?;

    watcher
        .watch(root, RecursiveMode::Recursive)
        .with_context(|| format!("failed to watch: {}", root.display()))?;

    thread::spawn(move || {
        // Keep the watcher alive inside the thread — watching stops if it's dropped.
        let _watcher = watcher;
        for res in ev_rx {
            let Ok(event) = res else { continue };
            if matches!(
                event.kind,
                EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
            ) {
                for path in event.paths {
                    if !should_forward(&path, &event.kind) {
                        continue;
                    }
                    // End the thread if the REPL receiver has closed.
                    if out_tx.send(path).is_err() {
                        return;
                    }
                }
            }
        }
    });

    Ok(out_rx)
}

/// Filter out build/VCS/hidden-dir noise — ignore if a path component is
/// `target`, `node_modules`, or starts with `.` (e.g. `.git`, `.venv`).
/// Language-agnostic: we don't filter by extension-of-content, Usta works
/// across domains. Tool-transient artifacts (temp files a library writes and
/// then renames/deletes, e.g. libgit2's `_git2_<hex>`, editor swap/backup
/// files) are a different category — not domain content, just noise from the
/// tools around it — and ARE filtered by basename pattern.
pub fn is_ignored(path: &Path) -> bool {
    path.components().any(|c| match c {
        std::path::Component::Normal(s) => {
            let s = s.to_string_lossy().to_ascii_lowercase();
            s == "target"
                || s == "node_modules"
                // Machine-written lockfile (`cargo run` side effect) — never
                // the user's work, never worth a turn (spec K6).
                || s == "cargo.lock"
                || s.starts_with('.')
                // Secret files never go to the LLM.
                || s.ends_with(".pem")
                || s.ends_with(".key")
                || s.contains("secret")
                || s.contains("credential")
                // Tool-transient artifacts: never real content, just noise.
                || s.starts_with("_git2_")
                || s.ends_with('~')
                || s.ends_with(".swp")
                || s.ends_with(".tmp")
                || (s.starts_with('#') && s.ends_with('#'))
        }
        _ => false,
    })
}

/// Decide whether a changed path is worth forwarding. Ignored paths never
/// are. A LIVE directory only matters when it appears, disappears, or is
/// renamed — a structure signal (spec D1) — so it forwards on
/// Create/Remove and on a rename (`Modify(Name(_))`, the destination side
/// of a directory rename lands here: the path exists as a directory with a
/// `Name` modify kind, e.g. `notify`'s FSEvents backend on rename produces
/// `Modify(Name(Any))` for the new path). Ordinary Modify on a directory
/// (`Data`, `Metadata`, `Any`, `Other`) is contents noise — a directory's
/// mtime changes whenever a child changes, and the file inside gets its own
/// event — so it is NOT forwarded, or every save would flood the channel
/// with the parent directory. Files, and paths that no longer exist
/// (deletions, rename sources), forward on every kind: classification
/// happens at flush time (polite::classify_flush), where existence is
/// probed exactly once.
pub fn should_forward(path: &Path, kind: &EventKind) -> bool {
    if is_ignored(path) {
        return false;
    }
    if path.is_dir() {
        return matches!(
            kind,
            EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
        );
    }
    true
}

/// Session-scoped directory inventory (spec D1): seeded from the project
/// tree at session start so an event on a PRE-EXISTING directory is never
/// misreported as "new". Deterministic shell state; classification asks it
/// two questions and never reads contents (directory contents are never
/// sent — the v0.24-era decision stands, only the EVENT is no longer
/// dropped).
pub struct StructureTracker {
    dirs: std::collections::BTreeSet<PathBuf>,
}

impl StructureTracker {
    /// Walk `root` and record every non-ignored directory that exists now.
    pub fn seed(root: &Path) -> Self {
        let mut tracker = StructureTracker {
            dirs: std::collections::BTreeSet::new(),
        };
        tracker.walk(root);
        tracker
    }

    fn walk(&mut self, dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && !is_ignored(&path) {
                self.dirs.insert(path.clone());
                self.walk(&path);
            }
        }
    }

    /// Record a directory sighting; true when it was previously unknown —
    /// the "new directory" signal.
    pub fn note_new_dir(&mut self, path: &Path) -> bool {
        self.dirs.insert(path.to_path_buf())
    }

    /// Record a disappearance; true when the path was a known directory —
    /// the "directory removed" signal. Also prunes every tracked descendant
    /// of `path`: a directory removed in one shot (e.g. `rm -rf`, no
    /// per-child event for each nested directory) must not leave stale
    /// children behind in the tracker — a stale child left in `dirs` would
    /// make a LATER re-creation of that same child report as "already
    /// known" instead of "new" (finding 10).
    pub fn note_removed(&mut self, path: &Path) -> bool {
        let was_known = self.dirs.remove(path);
        self.dirs.retain(|d| !d.starts_with(path));
        was_known
    }
}

/// Pure debounce state that smooths out a save storm. Editors produce
/// multiple modify events for a single save; each event is `push`ed, the
/// `deadline` expires `window` after the last event, and the select-loop
/// processes them all at once with `flush`.
pub struct Debouncer {
    pending: Vec<PathBuf>,
    deadline: Option<Instant>,
    window: Duration,
}

impl Debouncer {
    pub fn new(window: Duration) -> Self {
        Debouncer {
            pending: Vec::new(),
            deadline: None,
            window,
        }
    }

    /// Accumulate the path (dedupe while preserving first-seen order) and
    /// push the deadline forward.
    pub fn push(&mut self, path: PathBuf, now: Instant) {
        if !self.pending.contains(&path) {
            self.pending.push(path);
        }
        self.deadline = Some(now + self.window);
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Drain the accumulator, reset the deadline.
    pub fn flush(&mut self) -> Vec<PathBuf> {
        self.deadline = None;
        std::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};
    use std::time::Duration;
    use tokio::time::Instant;

    #[test]
    fn debouncer_push_dedups_and_preserves_order() {
        let mut d = Debouncer::new(Duration::from_millis(1000));
        let now = Instant::now();
        d.push(PathBuf::from("a.rs"), now);
        d.push(PathBuf::from("b.rs"), now);
        d.push(PathBuf::from("a.rs"), now);
        assert_eq!(
            d.flush(),
            vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")]
        );
    }

    #[test]
    fn debouncer_push_extends_deadline() {
        let mut d = Debouncer::new(Duration::from_millis(1000));
        let t0 = Instant::now();
        d.push(PathBuf::from("a.rs"), t0);
        let t1 = t0 + Duration::from_millis(500);
        d.push(PathBuf::from("b.rs"), t1);
        assert_eq!(d.deadline(), Some(t1 + Duration::from_millis(1000)));
    }

    #[test]
    fn debouncer_flush_clears_pending_and_deadline() {
        let mut d = Debouncer::new(Duration::from_millis(1000));
        d.push(PathBuf::from("a.rs"), Instant::now());
        let _ = d.flush();
        assert!(d.deadline().is_none());
        assert!(d.flush().is_empty());
    }

    #[test]
    fn debouncer_empty_has_no_deadline() {
        let d = Debouncer::new(Duration::from_millis(1000));
        assert!(d.deadline().is_none());
    }

    #[test]
    fn is_ignored_flags_target_dir() {
        assert!(is_ignored(Path::new("target/debug/x.rs")));
    }

    #[test]
    fn is_ignored_flags_hidden_dir() {
        assert!(is_ignored(Path::new(".git/HEAD")));
    }

    #[test]
    fn is_ignored_flags_node_modules() {
        assert!(is_ignored(Path::new("node_modules/foo/index.js")));
    }

    #[test]
    fn is_ignored_allows_src_file() {
        assert!(!is_ignored(Path::new("src/main.rs")));
    }

    #[test]
    fn is_ignored_allows_arbitrary_extension() {
        assert!(!is_ignored(Path::new("foo.py")));
    }

    #[test]
    fn is_ignored_blocks_secret_files() {
        assert!(is_ignored(Path::new("config/server.pem")));
        assert!(is_ignored(Path::new("keys/deploy.key")));
        assert!(is_ignored(Path::new("config/client_secrets.yaml")));
        assert!(is_ignored(Path::new("aws/CREDENTIALS.json")));
    }

    #[test]
    fn is_ignored_allows_normal_config() {
        assert!(!is_ignored(Path::new("config/settings.yaml")));
    }

    #[test]
    fn is_ignored_flags_git2_temp_file() {
        assert!(is_ignored(Path::new("_git2_4fec0a8edace5")));
    }

    #[test]
    fn is_ignored_flags_tilde_backup() {
        assert!(is_ignored(Path::new("main.rs~")));
    }

    #[test]
    fn is_ignored_flags_vim_swap() {
        assert!(is_ignored(Path::new("main.rs.swp")));
    }

    #[test]
    fn is_ignored_flags_tmp_suffix() {
        assert!(is_ignored(Path::new("upload.tmp")));
    }

    #[test]
    fn is_ignored_flags_emacs_lock_file() {
        assert!(is_ignored(Path::new(".#main.rs")));
    }

    #[test]
    fn is_ignored_flags_emacs_autosave_file() {
        assert!(is_ignored(Path::new("#main.rs#")));
    }

    #[test]
    fn is_ignored_flags_cargo_lock() {
        // Machine-written lockfile — a `cargo run` side effect, never the
        // user's own work; it bought an LLM turn in the 2026-08-28 live
        // session (spec K6).
        assert!(is_ignored(Path::new("Cargo.lock")));
        assert!(is_ignored(Path::new("sub/crate/Cargo.lock")));
        // The source file next to it must stay watchable.
        assert!(!is_ignored(Path::new("src/main.rs")));
    }

    #[test]
    fn should_forward_live_directory_only_on_structure_kinds() {
        // A directory APPEARING is a structure signal (spec D1: "an empty
        // directory produces no file event" was the invisible half of the
        // brands/marka-a assignment); a generic Modify on a live directory
        // is contents noise — the file inside gets its own event. The
        // Remove case is NOT exercised here: by the time a real Remove
        // event fires, the directory is gone from disk, so `is_dir()` is
        // false — see `should_forward_vanished_directory_on_remove`.
        let dir =
            std::env::temp_dir().join(format!("usta_watcher_forward_kinds_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(should_forward(&dir, &EventKind::Create(CreateKind::Folder)));
        assert!(!should_forward(&dir, &EventKind::Modify(ModifyKind::Any)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn should_forward_vanished_directory_on_remove() {
        // In real life a directory-removal event fires AFTER the directory
        // is gone: `path.is_dir()` is false at classification time, so this
        // takes the "vanished path" branch, not the "live directory"
        // branch. The two happen to agree here, but only this test proves
        // it — asserting on a still-existing directory would not.
        let dir = std::env::temp_dir().join(format!(
            "usta_watcher_forward_removed_dir_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(should_forward(&dir, &EventKind::Remove(RemoveKind::Folder)));
    }

    #[test]
    fn should_forward_directory_rename_destination() {
        // A directory rename's destination side: the new path exists as a
        // directory and the observed kind is `Modify(Name(_))` (confirmed
        // empirically against `recommended_watcher` — see task report). Must
        // pass, or the mentor never learns a directory reappeared under a
        // new name (only that the old one vanished).
        let dir = std::env::temp_dir().join(format!(
            "usta_watcher_forward_rename_dest_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(should_forward(
            &dir,
            &EventKind::Modify(ModifyKind::Name(RenameMode::Any))
        ));
        assert!(should_forward(
            &dir,
            &EventKind::Modify(ModifyKind::Name(RenameMode::To))
        ));
        // Ordinary directory Modify kinds must still be filtered — a
        // directory's mtime/metadata changes whenever a child changes.
        assert!(!should_forward(
            &dir,
            &EventKind::Modify(ModifyKind::Metadata(notify::event::MetadataKind::Any))
        ));
        assert!(!should_forward(
            &dir,
            &EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Any))
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn should_forward_files_and_vanished_paths_on_every_kind() {
        let file = std::env::temp_dir().join(format!(
            "usta_watcher_forward_file_{}.rs",
            std::process::id()
        ));
        std::fs::write(&file, b"fn main() {}").unwrap();
        assert!(should_forward(&file, &EventKind::Modify(ModifyKind::Any)));
        assert!(should_forward(&file, &EventKind::Create(CreateKind::File)));
        std::fs::remove_file(&file).unwrap();
        // A vanished path (deletion / rename source): is_dir() is false —
        // forward; flush-time classification decides what it means.
        assert!(should_forward(&file, &EventKind::Remove(RemoveKind::File)));
        assert!(should_forward(&file, &EventKind::Modify(ModifyKind::Any)));
        // Ignored stays ignored on every kind.
        assert!(!should_forward(
            Path::new("target/debug/x.rs"),
            &EventKind::Modify(ModifyKind::Any)
        ));
        assert!(!should_forward(
            Path::new(".git/HEAD"),
            &EventKind::Remove(RemoveKind::File)
        ));
    }

    #[test]
    fn should_forward_pins_ignore_check_before_directory_gate() {
        // Precedence pin: `is_ignored` must run BEFORE the directory gate.
        // A real, existing ignored directory (e.g. under `target/`) hitting
        // a structure-kind event (Create) must still be filtered. If the
        // directory gate were checked first, an ignored directory would
        // pass it (Create matches) and never reach the ignore filter —
        // verified locally by swapping the order in `should_forward` and
        // confirming this test fails (see task report for the transcript).
        let base = std::env::temp_dir().join(format!(
            "usta_watcher_ignore_precedence_{}",
            std::process::id()
        ));
        let ignored_dir = base.join("target").join("debug_stuff");
        std::fs::create_dir_all(&ignored_dir).unwrap();
        assert!(!should_forward(
            &ignored_dir,
            &EventKind::Create(CreateKind::Folder)
        ));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn structure_tracker_seeds_existing_dirs_and_flags_changes() {
        let base =
            std::env::temp_dir().join(format!("usta_watcher_tracker_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("src")).unwrap();
        let mut t = StructureTracker::seed(&base);
        // Pre-existing dir was seeded: sighting it is NOT "new".
        assert!(!t.note_new_dir(&base.join("src")));
        // A brand-new dir: first sighting is new, second is not.
        let fresh = base.join("brands");
        assert!(t.note_new_dir(&fresh));
        assert!(!t.note_new_dir(&fresh));
        // Removal: known dir → true exactly once; unknown → false.
        assert!(t.note_removed(&fresh));
        assert!(!t.note_removed(&fresh));
        assert!(!t.note_removed(&base.join("never-seen")));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn note_removed_prunes_tracked_descendants_so_recreation_is_reported_as_new() {
        // Finding 10: a directory removed in one shot (e.g. `rm -rf`, no
        // per-child Remove event for every nested directory) must not leave
        // stale children behind — a later re-creation of such a child must
        // still be reported as NEW, not silently swallowed as "already
        // known".
        let base = std::env::temp_dir().join(format!(
            "usta_watcher_prune_descendants_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let mut t = StructureTracker::seed(&base);

        let parent = base.join("brands");
        let child = parent.join("marka-a");
        // Both directories become known — as if the watcher had seen both
        // Create events (the ordinary two-event case).
        assert!(t.note_new_dir(&parent));
        assert!(t.note_new_dir(&child));

        // Only the PARENT's Remove event arrives — the single-shot `rm -rf`
        // case where no per-child event fires for the nested directory.
        assert!(t.note_removed(&parent));

        // Without pruning, `child` would still be "known" here, so its
        // re-creation would wrongly report as NOT new.
        assert!(
            t.note_new_dir(&child),
            "a child of a removed directory must be forgotten too, so its recreation reports as new"
        );

        let _ = std::fs::remove_dir_all(&base);
    }
}
