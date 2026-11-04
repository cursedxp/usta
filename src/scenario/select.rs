//! Which scenario next. Skills are keywords matched against curriculum-map
//! lines (map items are free text written by the model). A finished skill is
//! skipped; a package already finished is not offered again while a new one
//! exists; a package whose story prerequisites (`story.needs`) are not open
//! yet is `Later`. When nothing suitable is left the answer is to generate one,
//! never "no tasks". Due-return is not built here (the map carries no `due:`).

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
    } else if finished.contains(&pkg.id) {
        Tag::Done
    } else if pkg.story.needs.iter().any(|n| !threads.contains(n)) {
        Tag::Later
    } else {
        Tag::New
    }
}

pub(crate) enum Pick<'a> {
    Package(&'a Package),
    /// Nothing suitable is left, or the learner asked for something new.
    Generate {
        request: Option<String>,
    },
}

/// `arg` = 1-based number, id, or free text; `None` = the first `New` one that can run
/// here. Explicit picks are honoured even for done or skipped tasks, and even when they
/// need a runtime that is missing (the install guidance is the answer then), but not
/// for a task that is later in the story.
pub(crate) fn resolve<'a>(
    pkgs: &'a [Package],
    arg: Option<&str>,
    map: &str,
    finished: &[String],
    open: &[String],
    runtime: bool,
) -> Result<Pick<'a>, String> {
    let threads = threads_available(pkgs, map, open);
    let Some(a) = arg else {
        let next = pkgs.iter().find(|p| {
            tag(p, map, finished, &threads) == Tag::New && (runtime || !p.needs_runtime())
        });
        return Ok(next.map_or(Pick::Generate { request: None }, Pick::Package));
    };
    let found = match a.parse::<usize>() {
        Ok(n) => Some(
            n.checked_sub(1)
                .and_then(|i| pkgs.get(i))
                .ok_or_else(|| format!("no task '{a}'; see /scenario"))?,
        ),
        Err(_) => pkgs.iter().find(|p| p.id == a),
    };
    let Some(p) = found else {
        return Ok(Pick::Generate {
            request: Some(a.to_string()),
        });
    };
    if tag(p, map, finished, &threads) == Tag::Later {
        return Err("that task comes later in the story; finish the earlier ones first".into());
    }
    Ok(Pick::Package(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::package::tests::{tmp, write_pkg};
    use crate::scenario::package::ThreadSpec;

    fn pkg(id: &str, skills: &[&str]) -> Package {
        // Unique dir per call: tests reuse ids and run in parallel.
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = tmp(&format!("sel_{id}_{n}"));
        let mut p = Package::load(&write_pkg(&root)).unwrap();
        p.id = id.into();
        p.skills = skills.iter().map(|s| s.to_string()).collect();
        p
    }

    fn picked<'a>(r: Result<Pick<'a>, String>) -> &'a str {
        match r.unwrap() {
            Pick::Package(p) => &p.id,
            Pick::Generate { .. } => panic!("expected a package"),
        }
    }

    fn opens(p: &mut Package, id: &str) {
        p.story.opens.push(ThreadSpec {
            id: id.into(),
            text: "t".into(),
        });
    }

    const MAP: &str = "# linux\n\
        - File permissions and ownership: settled\n\
        - Disk usage and finding files: seen\n\
        - Processes and signals: not seen\n\
        - Shell quoting: deepened\n";

    #[test]
    fn skipped_only_when_every_skill_matches_a_settled_line() {
        assert_eq!(
            tag(&pkg("a", &["permission", "ownership"]), MAP, &[], &[]),
            Tag::Skipped
        );
        assert_eq!(
            tag(&pkg("b", &["permission", "processes"]), MAP, &[], &[]),
            Tag::New
        );
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
        assert_eq!(picked(resolve(&pkgs, None, "", &fin, &[], true)), "b");
    }

    #[test]
    fn a_package_is_later_until_its_needed_thread_is_open() {
        let mut a = pkg("a", &["x1"]);
        opens(&mut a, "t1");
        let mut b = pkg("b", &["x2"]);
        b.story.needs = vec!["t1".into()];
        let pkgs = vec![a, b];
        assert_eq!(tag(&pkgs[1], "", &[], &[]), Tag::Later);
        assert_eq!(picked(resolve(&pkgs, None, "", &[], &[], true)), "a");
        let open = vec!["t1".to_string()];
        assert_eq!(
            tag(&pkgs[1], "", &[], &threads_available(&pkgs, "", &open)),
            Tag::New
        );
    }

    #[test]
    fn a_skipped_package_opens_its_threads_for_the_ones_that_need_them() {
        let mut a = pkg("a", &["permission"]);
        opens(&mut a, "t1");
        let mut b = pkg("b", &["x2"]);
        b.story.needs = vec!["t1".into()];
        let pkgs = vec![a, b];
        assert_eq!(threads_available(&pkgs, MAP, &[]), vec!["t1".to_string()]);
        assert_eq!(picked(resolve(&pkgs, None, MAP, &[], &[], true)), "b");
    }

    #[test]
    fn resolve_by_number_and_id_even_for_done_or_skipped() {
        let pkgs = vec![pkg("a", &["permission"]), pkg("b", &["x2"])];
        assert_eq!(picked(resolve(&pkgs, Some("1"), MAP, &[], &[], true)), "a");
        assert_eq!(picked(resolve(&pkgs, Some("b"), MAP, &[], &[], true)), "b");
    }

    #[test]
    fn an_explicit_pick_of_a_later_task_is_refused() {
        let mut b = pkg("b", &["x2"]);
        b.story.needs = vec!["t1".into()];
        let pkgs = [b];
        let err = resolve(&pkgs, Some("b"), "", &[], &[], true).err().unwrap();
        assert!(err.contains("later in the story"), "{err}");
    }

    #[test]
    fn nothing_left_means_generate_never_an_error() {
        assert!(matches!(
            resolve(&[], None, "", &[], &[], true).unwrap(),
            Pick::Generate { request: None }
        ));
        let pkgs = vec![pkg("a", &["x1"])];
        let fin = vec!["a".to_string()];
        assert!(matches!(
            resolve(&pkgs, None, "", &fin, &[], true).unwrap(),
            Pick::Generate { request: None }
        ));
    }

    #[test]
    fn free_text_is_a_request_and_a_bad_number_stays_an_error() {
        let pkgs = vec![pkg("a", &["x1"])];
        let Pick::Generate { request } =
            resolve(&pkgs, Some("price anchoring"), "", &[], &[], true).unwrap()
        else {
            panic!()
        };
        assert_eq!(request.as_deref(), Some("price anchoring"));
        assert!(resolve(&pkgs, Some("9"), "", &[], &[], true).is_err());
    }

    #[test]
    fn without_a_runtime_a_bare_start_passes_over_what_needs_one() {
        let pkgs = vec![pkg("a", &["x1"])];
        assert!(matches!(
            resolve(&pkgs, None, "", &[], &[], false).unwrap(),
            Pick::Generate { .. }
        ));
        assert_eq!(picked(resolve(&pkgs, Some("a"), "", &[], &[], false)), "a");
    }
}
