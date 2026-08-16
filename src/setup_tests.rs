//! Test module for setup.rs, split out for file size; still a child module via #[path], private access preserved.

use super::*;

#[test]
fn factory_targets_includes_uncatalogued_cwd_project() {
    let idx = "## Records\n- rust | /p/a | 2026-08-01\n";
    // cwd project not in the catalog → still targeted
    let t = factory_targets(idx, Some(Path::new("/p/orphan")));
    assert!(t.contains(&PathBuf::from("/p/a/.usta")));
    assert!(t.contains(&PathBuf::from("/p/orphan/.usta")));
    // cwd project already catalogued → no duplicate
    let t2 = factory_targets(idx, Some(Path::new("/p/a")));
    assert_eq!(
        t2.iter()
            .filter(|p| p.as_path() == Path::new("/p/a/.usta"))
            .count(),
        1
    );
    // no cwd root → catalog only
    let t3 = factory_targets(idx, None);
    assert_eq!(t3, vec![PathBuf::from("/p/a/.usta")]);
}

#[test]
fn recover_choice_defaults_yes_only_explicit_no_deletes() {
    // default / lossless side → recover (true)
    assert!(recover_choice(""));
    assert!(recover_choice("   \n"));
    assert!(recover_choice("y"));
    assert!(recover_choice("evet"));
    assert!(recover_choice("garbage"));
    // explicit no → delete (false)
    assert!(!recover_choice("n"));
    assert!(!recover_choice("N"));
    assert!(!recover_choice("no"));
    assert!(!recover_choice("h"));
    assert!(!recover_choice("hayır"));
    assert!(!recover_choice("hayir")); // ASCII fallback
    assert!(!recover_choice("HAYIR")); // uppercase Turkish: lowercases to "hayir"
    assert!(!recover_choice("Hayır"));
}

#[test]
fn write_project_scaffold_creates_visible_mentor_dir() {
    let base = std::env::temp_dir().join(format!(
        "usta_main_test_mentor_scaffold_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();

    write_project_scaffold(&base).unwrap();
    assert!(base.join("mentor").is_dir());
    assert!(base.join("mentor/.gitkeep").is_file());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn write_project_scaffold_creates_visible_exercises_dir() {
    let base = std::env::temp_dir().join(format!(
        "usta_main_test_exercises_scaffold_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();

    write_project_scaffold(&base).unwrap();
    assert!(base.join("exercises").is_dir());
    assert!(base.join("exercises/.gitkeep").is_file());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn reset_topic_leaves_mentor_dir_untouched() {
    // reset deletes under `.usta/` only — mentor/ is the user's project doc,
    // possibly committed to their repo. Guard that contract with the same
    // path logic run_reset_topic uses (progress_path is under .usta).
    let root = Path::new("/tmp/proj");
    let p = progress::progress_path(root, "rust");
    assert!(p.starts_with(root.join(".usta")));
    assert!(!progress::project_md_path(root).starts_with(root.join(".usta")));
}

#[test]
fn render_stats_full_quiet_and_empty() {
    let mk = |d: &str, t: &str| history::Entry {
        date: d.into(),
        topic: t.into(),
        map: Some(40),
        settled: Some(4),
    };
    let full = render_stats(
        &[mk("2026-08-14", "rust"), mk("2026-08-15", "rust")],
        "2026-08-15",
    );
    assert!(full.contains("rust"));
    assert!(full.contains("2 session(s)"));
    assert!(full.contains("current streak: 2 day(s)"));

    // kırık seri: current 0 → yazılmaz, longest pozitif çerçeve
    let broken = render_stats(&[mk("2026-08-01", "rust")], "2026-08-15");
    assert!(!broken.contains("current streak"));
    assert!(broken.contains("longest streak"));
    assert!(broken.contains("quiet week"));

    let empty = render_stats(&[], "2026-08-15");
    assert!(empty.contains("no sessions recorded yet"));
}

#[test]
fn render_stats_omits_missing_settled_segment() {
    // Entry has a map percentage but no settled count (e.g. curriculum exists
    // but has no items in "settled"/"deepened" state yet) — the "settled X → Y"
    // segment must be omitted entirely, not rendered as "None" or a dangling arrow,
    // while the "map X% → Y%" segment still renders normally.
    let entry = history::Entry {
        date: "2026-08-15".into(),
        topic: "rust".into(),
        map: Some(40),
        settled: None,
    };
    let out = render_stats(&[entry], "2026-08-15");
    assert!(out.contains("rust"));
    assert!(out.contains("1 session(s)"));
    assert!(out.contains("map 40% → 40%"));
    assert!(!out.contains("settled"));
    assert!(!out.contains("None"));

    // Both missing (e.g. topic has no curriculum yet at all) — neither segment renders.
    let both_none = history::Entry {
        date: "2026-08-15".into(),
        topic: "gtm".into(),
        map: None,
        settled: None,
    };
    let out2 = render_stats(&[both_none], "2026-08-15");
    assert!(out2.contains("gtm"));
    assert!(!out2.contains("map"));
    assert!(!out2.contains("settled"));
    assert!(!out2.contains("None"));
    assert!(!out2.contains("→"));
}

#[test]
fn render_topics_table_aligns_columns_with_header_rule() {
    use std::path::PathBuf;
    let entries = vec![
        index::IndexEntry {
            topic: "rust".into(),
            project: PathBuf::from("~/projects/tokio-lab"),
            date: "2026-08-14".into(),
        },
        index::IndexEntry {
            topic: "kaynak-ingest".into(),
            project: PathBuf::from("~/work/ingest"),
            date: "2026-08-11".into(),
        },
    ];
    let out = render_topics_table(&entries);
    let lines: Vec<&str> = out.lines().collect();
    // Header, a dim `─` rule, then the rows — content preserved.
    assert!(lines[0].starts_with("Topic"));
    assert!(
        lines[1].chars().all(|c| c == '─'),
        "header rule line: {:?}",
        lines[1]
    );
    assert!(out.contains("rust"));
    assert!(out.contains("~/projects/tokio-lab"));
    assert!(out.contains("2026-08-11"));
    // Columns align: every data row's project column (starts with ~) begins
    // at the same character offset.
    assert_eq!(
        lines[2].find('~'),
        lines[3].find('~'),
        "project column misaligned: {lines:#?}"
    );
}

#[test]
fn render_stats_aligns_topic_column() {
    let mk = |d: &str, t: &str| history::Entry {
        date: d.into(),
        topic: t.into(),
        map: None,
        settled: None,
    };
    // Two topics of different widths → the "session(s)" column must line up.
    let out = render_stats(
        &[mk("2026-08-15", "rust"), mk("2026-08-15", "kaynak-ingest")],
        "2026-08-15",
    );
    // Only the per-topic rows (2-space indent) — NOT the "total:" footer line.
    let sess_lines: Vec<&str> = out
        .lines()
        .filter(|l| l.starts_with("  ") && l.contains("session(s)"))
        .collect();
    assert!(sess_lines.len() >= 2);
    use unicode_width::UnicodeWidthStr;
    let offsets: Vec<usize> = sess_lines
        .iter()
        .map(|l| l.split_once("session(s)").map(|(a, _)| a.width()).unwrap())
        .collect();
    assert!(
        offsets.windows(2).all(|w| w[0] == w[1]),
        "session(s) column misaligned: {sess_lines:#?}"
    );
}

#[test]
fn profile_is_generic_matches_embedded_template_only() {
    let sablon = defaults::global_defaults()
        .into_iter()
        .find(|(rel, _, _)| *rel == "USER.md")
        .map(|(_, c, _)| c)
        .unwrap();
    assert!(profile_is_generic(sablon));
    assert!(profile_is_generic(&format!("{sablon}\n"))); // line-ending tolerance
    assert!(!profile_is_generic("# Öğrenci Profili — Anil\nkişisel"));
}

#[test]
fn reset_profile_files_backs_up_and_writes_generic_template() {
    let base = std::env::temp_dir().join(format!("usta_reset_profile_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(
        base.join("USER.md"),
        "# Öğrenci Profili — Anil\nkişisel notlar",
    )
    .unwrap();

    reset_profile_files(&base).unwrap();

    let yeni = std::fs::read_to_string(base.join("USER.md")).unwrap();
    let sablon = defaults::global_defaults()
        .into_iter()
        .find(|(rel, _, _)| *rel == "USER.md")
        .map(|(_, c, _)| c)
        .unwrap();
    assert_eq!(yeni, sablon); // equal to the generic template
    assert_eq!(
        std::fs::read_to_string(base.join("USER.md.bak")).unwrap(),
        "# Öğrenci Profili — Anil\nkişisel notlar"
    ); // old content is in the backup
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn reset_profile_files_works_without_existing_profile() {
    let base = std::env::temp_dir().join(format!("usta_reset_profile_yok_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    reset_profile_files(&base).unwrap(); // also works with no existing file: directory is created, template is written, no .bak
    assert!(base.join("USER.md").exists());
    assert!(!base.join("USER.md.bak").exists());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn migrate_moves_old_profile_once() {
    let base = std::env::temp_dir().join(format!("usta_migrate_moves_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("learner")).unwrap();
    std::fs::write(base.join("learner/profile.md"), "KIŞISEL").unwrap();

    let moved = migrate_profile_to_user_md(&base).unwrap();
    assert!(moved);
    assert_eq!(
        std::fs::read_to_string(base.join("USER.md")).unwrap(),
        "KIŞISEL"
    );
    assert!(!base.join("learner/profile.md").exists());

    // Second call: the old path no longer exists → no-op.
    let moved_again = migrate_profile_to_user_md(&base).unwrap();
    assert!(!moved_again);
    assert_eq!(
        std::fs::read_to_string(base.join("USER.md")).unwrap(),
        "KIŞISEL"
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn migrate_never_overwrites_existing_user_md() {
    let base =
        std::env::temp_dir().join(format!("usta_migrate_no_overwrite_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("learner")).unwrap();
    std::fs::write(base.join("learner/profile.md"), "ESKİ").unwrap();
    std::fs::write(base.join("USER.md"), "YENİ").unwrap();

    let moved = migrate_profile_to_user_md(&base).unwrap();
    assert!(!moved);
    assert_eq!(
        std::fs::read_to_string(base.join("USER.md")).unwrap(),
        "YENİ"
    );
    // No risk of data loss is taken — the old file is also left in place.
    assert_eq!(
        std::fs::read_to_string(base.join("learner/profile.md")).unwrap(),
        "ESKİ"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// `write_project_scaffold` sets up the `.usta/` scaffold in a temp directory —
/// without touching `global_root()` at all (doesn't affect the real `~/.config`).
#[test]
fn write_global_defaults_syncs_code_owned_preserves_user_owned() {
    let base =
        std::env::temp_dir().join(format!("usta_main_test_global_sync_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    // First write: everything is written.
    let first = write_global_defaults(&base).unwrap();
    assert!(first.iter().all(|(_, wrote)| *wrote));

    // Dirty it: make code-owned USTA.md stale, edit the user-owned profile.
    std::fs::write(base.join("USTA.md"), "eski sürüm").unwrap();
    std::fs::write(base.join("USER.md"), "kullanıcı düzenlemesi").unwrap();

    write_global_defaults(&base).unwrap();

    // Code-owned file was synced — the embedded up-to-date content came back.
    // Note: USTA.md turned into a behavior-free index via Task 1's brain-split
    // ("Hard Rules" now lives in RULES.md) — the assertion was updated to match
    // the current embedded content.
    let usta = std::fs::read_to_string(base.join("USTA.md")).unwrap();
    assert!(usta.contains("Intervention Map"));
    // User-owned file was preserved.
    assert_eq!(
        std::fs::read_to_string(base.join("USER.md")).unwrap(),
        "kullanıcı düzenlemesi"
    );

    // Nothing gets rewritten when there's no change.
    let third = write_global_defaults(&base).unwrap();
    assert!(third.iter().all(|(_, wrote)| !*wrote));

    let _ = std::fs::remove_dir_all(&base);
}

/// Regression test for the start-path ordering fix: `run_migration` must
/// execute BEFORE `ensure_scaffold` (→ `write_global_defaults` →
/// `config::needs_sync`, which READS `Ownership::Code` files like
/// `approaches/software.md` and may resync them from the embedded English
/// template). Those files are also migration-in-scope
/// (`migrate::run` sweeps global `approaches/*.md`), so a legacy
/// Turkish-token install needs migration to see the file BEFORE the
/// scaffold can silently overwrite it — otherwise the original content is
/// lost with no `.bak`.
///
/// This exercises the two steps in the FIXED order (`migrate::run` then
/// `write_global_defaults`, mirroring main()'s new ordering) and asserts
/// the `.bak` captures the ORIGINAL Turkish content — proof migration ran
/// first. It then asserts the scaffold still resynced the file afterward
/// (to `Ownership::Code`'s embedded content), showing the overwrite did
/// happen, just AFTER migration had already captured the legacy state.
#[test]
fn migration_before_scaffold_preserves_legacy_approaches_bak() {
    let base = std::env::temp_dir().join(format!(
        "usta_main_test_migration_before_scaffold_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("approaches")).unwrap();

    let legacy = "## Tercihler\n- gamification: on\n";
    let software_md = base.join("approaches/software.md");
    std::fs::write(&software_md, legacy).unwrap();

    // Sanity: the legacy content must differ from the embedded template,
    // otherwise write_global_defaults wouldn't touch it and this test
    // wouldn't prove anything about ordering.
    let embedded = defaults::global_defaults()
        .into_iter()
        .find(|(rel, _, _)| *rel == "approaches/software.md")
        .unwrap()
        .1;
    assert_ne!(legacy, embedded);

    // Fixed order: migration first, THEN scaffold sync — matches main()'s
    // new sequencing.
    migrate::run(&base, None).unwrap();
    write_global_defaults(&base).unwrap();

    // `.bak` sibling path — mirrors migrate::sibling()'s append-not-swap
    // semantics (`software.md` -> `software.md.bak`).
    let mut bak_os = software_md.clone().into_os_string();
    bak_os.push(".bak");
    let bak = PathBuf::from(bak_os);

    assert!(
            bak.exists(),
            ".bak must exist — migration must have run before the scaffold could overwrite the legacy file"
        );
    assert_eq!(
        std::fs::read_to_string(&bak).unwrap(),
        legacy,
        ".bak must hold the ORIGINAL Turkish content, captured before any scaffold resync"
    );

    // The scaffold DID resync the file afterward (Ownership::Code) — this
    // is expected and fine, it just had to happen after migration.
    let after = std::fs::read_to_string(&software_md).unwrap();
    assert_eq!(after, embedded);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn write_project_scaffold_creates_dirs_and_gitkeeps() {
    let base = std::env::temp_dir().join(format!(
        "usta_main_test_project_scaffold_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();

    let results = write_project_scaffold(&base).unwrap();
    assert_eq!(results.len(), 6);
    assert!(results.iter().all(|(_, wrote)| *wrote));
    assert!(base.join(".usta/learner/progress").is_dir());
    assert!(base.join(".usta/approaches").is_dir());
    assert!(base.join(".usta/learner/progress/.gitkeep").is_file());
    assert!(base.join(".usta/approaches/.gitkeep").is_file());

    // Second call: directories already exist → `wrote` should be false, no panic.
    let results2 = write_project_scaffold(&base).unwrap();
    assert!(results2.iter().all(|(_, wrote)| !*wrote));

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn write_project_scaffold_creates_visible_materials_dir() {
    let base = std::env::temp_dir().join(format!(
        "usta_main_test_materials_scaffold_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    write_project_scaffold(&base).unwrap();
    assert!(base.join("materials").is_dir());
    assert!(base.join("materials/.gitkeep").is_file());
    let _ = std::fs::remove_dir_all(&base);
}

/// Görev 5: scaffold writes `.usta/visuals/.gitignore` (`*`) — generated
/// visual HTML never leaks into the user's git repo, while the files
/// themselves stay on disk.
#[test]
fn write_project_scaffold_writes_visuals_gitignore() {
    let base = std::env::temp_dir().join(format!(
        "usta_main_test_visuals_gitignore_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();

    write_project_scaffold(&base).unwrap();

    let gitignore = base.join(".usta/visuals/.gitignore");
    assert!(gitignore.is_file());
    assert_eq!(std::fs::read_to_string(&gitignore).unwrap(), "*\n");

    let _ = std::fs::remove_dir_all(&base);
}

/// Görev 5: `usta reset <topic>` also removes `.usta/visuals/<topic>/`.
/// `run_reset_topic` itself reads stdin (confirm) and `cwd`, so it isn't
/// unit-testable directly — this tests the extracted deletion step,
/// following the same temp-dir pattern as the scaffold tests above.
#[test]
fn remove_topic_visuals_deletes_a_populated_dir() {
    let base = std::env::temp_dir().join(format!(
        "usta_main_test_reset_visuals_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    let topic_dir = base.join(".usta/visuals/rust");
    std::fs::create_dir_all(&topic_dir).unwrap();
    std::fs::write(topic_dir.join("2026-01-01-000000-ownership.html"), "x").unwrap();
    let sibling_dir = base.join(".usta/visuals/dns");
    std::fs::create_dir_all(&sibling_dir).unwrap();
    std::fs::write(sibling_dir.join("2026-01-01-000000-records.html"), "x").unwrap();

    remove_topic_visuals(&base, "rust").unwrap();

    assert!(
        !topic_dir.exists(),
        "topic visuals dir must be gone after reset"
    );
    // Sibling topics are untouched — reset is scoped to the one topic.
    assert!(sibling_dir.is_dir());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn remove_topic_visuals_missing_dir_is_not_an_error() {
    let base = std::env::temp_dir().join(format!(
        "usta_main_test_reset_visuals_missing_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();

    // No `.usta/visuals/rust` was ever created (topic never ran `/show`).
    assert!(remove_topic_visuals(&base, "rust").is_ok());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn factory_reset_prompt_advertises_only_english_word() {
    // Display is English-only (Claude Code model: shell text = English base);
    // `evet` stays silently accepted in the `confirm(&["evet", "yes"])` call
    // but is never advertised — the display and the acceptance list are
    // deliberately different surfaces.
    assert!(FACTORY_RESET_PROMPT.contains("yes"));
    assert!(!FACTORY_RESET_PROMPT.contains("evet"));
}
