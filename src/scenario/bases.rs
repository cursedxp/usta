//! Base images Usta itself pulled for a generated build (or the folder check image).
//! One line per image in `<global>/scenarios/bases.txt`: `<image> <YYYY-MM-DD>`, the
//! day a build last used it. A base that was already on the machine is never written
//! here, so it is never removed. The startup sweep removes a recorded base nobody has
//! used for `OLD_DAYS`. The file is untrusted input: only allowlisted names are read.

use std::path::{Path, PathBuf};

use crate::scenario::docker::{probe, Docker, Runtime};
use crate::scenario::dockerfile::base_ok;
use crate::scenario::folder::CHECK_IMAGE;
use crate::scenario::sweep::age_days;

/// A base unused this long goes. Provisional, like the idle age of a task.
pub(crate) const OLD_DAYS: i64 = 14;

pub(crate) fn file(global: &Path) -> PathBuf {
    global.join("scenarios/bases.txt")
}

fn allowed(image: &str) -> bool {
    image == CHECK_IMAGE || base_ok(image)
}

/// The lines that are well formed: an allowed image and a real date. The rest are dropped.
fn read(file: &Path) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    text.lines()
        .filter_map(|l| {
            let mut p = l.split_whitespace();
            let (image, date) = (p.next()?, p.next()?);
            let ok = p.next().is_none() && allowed(image) && age_days(date, date).is_some();
            ok.then(|| (image.to_string(), date.to_string()))
        })
        .collect()
}

fn write(file: &Path, lines: &[(String, String)]) {
    let text: String = lines.iter().map(|(i, d)| format!("{i} {d}\n")).collect();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, text);
}

/// Usta just pulled `image`: record it, dated today.
pub(crate) fn pulled(file: &Path, image: &str, today: &str) {
    if allowed(image) {
        set(file, image, today, true);
    }
}

/// A build used `image`: refresh its date when it is recorded; otherwise nothing.
pub(crate) fn used(file: &Path, image: &str, today: &str) {
    set(file, image, today, false);
}

fn set(file: &Path, image: &str, today: &str, add: bool) {
    let mut lines = read(file);
    match lines.iter_mut().find(|(i, _)| i == image) {
        Some((_, d)) if d == today => return,
        Some((_, d)) => *d = today.to_string(),
        None if add => lines.push((image.to_string(), today.to_string())),
        None => return,
    }
    write(file, &lines);
}

/// Removes recorded bases not used for `OLD_DAYS`, without `-f`: a base in use, or one a
/// task image still stands on, is refused and keeps its line. Only with a runtime that
/// already answers; this never starts one.
pub(crate) fn sweep(d: &dyn Docker, file: &Path, today: &str) {
    let before = read(file);
    let old = |date: &str| age_days(date, today).is_some_and(|n| n >= OLD_DAYS);
    // Most launches have nothing due, and they must not wait on the runtime.
    if !before.iter().any(|(_, date)| old(date)) || !matches!(probe(d), Runtime::Ready) {
        return;
    }
    let mut keep = Vec::new();
    for (image, date) in &before {
        let gone = old(date)
            && d.run(&["rmi", image])
                .is_ok_and(|o| o.ok() || o.text().contains("No such image"));
        if !gone {
            keep.push((image.clone(), date.clone()));
        }
    }
    // Also rewrites a file that had bad lines; a missing file stays missing.
    let Ok(raw) = std::fs::read_to_string(file) else {
        return;
    };
    if keep != before || raw.lines().count() != before.len() {
        write(file, &keep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::docker::fake::{fail, ok, FakeDocker};
    use crate::scenario::package::tests::tmp;

    fn rmis(d: &FakeDocker) -> Vec<String> {
        let calls = d.calls();
        calls.into_iter().filter(|c| c.starts_with("rmi")).collect()
    }

    #[test]
    fn pulled_records_and_used_refreshes_only_a_recorded_base() {
        let root = tmp("bases_rec");
        let f = file(&root);
        used(&f, "rust:1.83-slim", "2026-11-01");
        assert!(!f.exists(), "a base Usta never pulled is never recorded");
        pulled(&f, "rust:1.83-slim", "2026-10-01");
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            "rust:1.83-slim 2026-10-01\n"
        );
        used(&f, "rust:1.83-slim", "2026-11-05");
        used(&f, "debian:12-slim", "2026-11-05");
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            "rust:1.83-slim 2026-11-05\n"
        );
        pulled(&f, "-p8080:80", "2026-11-05");
        pulled(&f, "evil/rust:1", "2026-11-05");
        assert_eq!(read(&f).len(), 1, "off the allowlist is not recorded");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_sweep_removes_only_old_recorded_bases() {
        let root = tmp("bases_sweep");
        let f = file(&root);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(
            &f,
            "rust:1.83-slim 2026-10-01\ndebian:12-slim 2026-11-01\nnode:20 2026-10-22\n",
        )
        .unwrap();
        let d = FakeDocker::new(|_| ok("27"));
        sweep(&d, &f, "2026-11-05");
        // 35 days old goes; 4 and 14 days: 14 goes too (14 or more).
        assert_eq!(rmis(&d), vec!["rmi rust:1.83-slim", "rmi node:20"]);
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            "debian:12-slim 2026-11-01\n"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_refused_removal_keeps_its_line_and_a_missing_image_drops_it() {
        let root = tmp("bases_refused");
        let f = file(&root);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, "rust:1.83-slim 2026-09-01\nnode:20 2026-09-01\n").unwrap();
        let d = FakeDocker::new(|l| match l {
            "rmi rust:1.83-slim" => fail(1, "image is being used by stopped container abc"),
            "rmi node:20" => fail(1, "Error: No such image: node:20"),
            _ => ok("27"),
        });
        sweep(&d, &f, "2026-11-05");
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            "rust:1.83-slim 2026-09-01\n"
        );
        assert!(d.calls().iter().all(|c| !c.starts_with("rmi -f")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_sweep_ignores_lines_off_the_allowlist_and_drops_them() {
        let root = tmp("bases_untrusted");
        let f = file(&root);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(
            &f,
            "kiln-db:1 2020-01-01\n-f 2020-01-01\nrust:1.83-slim;rm 2020-01-01\n\
             rust:1.83-slim not-a-date\nrust:1.83-slim 2020-01-01 extra\n\
             python:3.12-slim-bookworm 2020-01-01\n",
        )
        .unwrap();
        let d = FakeDocker::new(|_| ok("27"));
        sweep(&d, &f, "2026-11-05");
        assert_eq!(rmis(&d), vec!["rmi python:3.12-slim-bookworm"]);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_launch_with_nothing_due_never_calls_the_runtime() {
        let root = tmp("bases_quiet");
        let f = file(&root);
        let d = FakeDocker::new(|_| ok(""));
        sweep(&d, &f, "2026-11-05");
        pulled(&f, "rust:1.83-slim", "2026-11-01");
        sweep(&d, &f, "2026-11-05");
        assert!(d.calls().is_empty(), "{:?}", d.calls());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_sweep_does_nothing_when_the_runtime_is_not_ready() {
        let root = tmp("bases_off");
        let f = file(&root);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, "rust:1.83-slim 2020-01-01\n").unwrap();
        let d = FakeDocker::new(|_| fail(1, "Cannot connect to the Docker daemon"));
        sweep(&d, &f, "2026-11-05");
        assert!(rmis(&d).is_empty());
        let missing = FakeDocker::missing();
        sweep(&missing, &f, "2026-11-05");
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            "rust:1.83-slim 2020-01-01\n"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
