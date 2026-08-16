//! Test module for progress.rs, split out for file size; still a child module via #[path], private access preserved.

    use super::*;
    use std::path::Path;

    #[test]
    fn exam_prompt_defines_exam_contract() {
        let s = exam_prompt("almanca");
        assert!(s.contains("EXAM MODE"));
        assert!(s.contains("almanca"));
        assert!(s.contains("ONE question at a time"));
        assert!(s.contains("SUSPENDED"));
        assert!(s.contains("recorded at session close"));
        assert!(s.contains("stop the exam"));
    }

    #[test]
    fn progress_path_builds_expected_layout() {
        let p = progress_path(Path::new("/proje"), "rust");
        assert_eq!(
            p,
            Path::new("/proje/.usta/learner/progress/rust.md")
        );
    }

    #[test]
    fn closing_prompt_embeds_topic_and_existing() {
        let s = closing_prompt("rust", Some("- Level: orta"), None, None, None, None, None);
        assert!(s.contains("rust"));
        assert!(s.contains("- Level: orta"));
    }

    #[test]
    fn closing_prompt_marks_missing_file() {
        let s = closing_prompt("rust", None, None, None, None, None, None);
        assert!(s.contains("(dosya henüz yok)"));
    }

    #[test]
    fn closing_prompt_includes_pruning_rule() {
        let s = closing_prompt("rust", None, None, None, None, None, None);
        assert!(s.contains("20 items"));
    }

    #[test]
    fn closing_prompt_requests_rich_sections() {
        let s = closing_prompt("rust", None, None, None, None, None, None);
        assert!(s.contains("Recall questions"));
        assert!(s.contains("Error log"));
        assert!(s.contains("Hint ladder"));
    }

    #[test]
    fn closing_prompt_defines_spaced_repetition_schedule() {
        let s = closing_prompt("rust", None, None, None, None, None, None);
        assert!(s.contains("due: YYYY-MM-DD"));
        assert!(s.contains("ivl:"));
        assert!(s.contains("1, 3, 7, 16, 35, 90"));
        assert!(s.contains("retires: move it to")); // unique to the ivl:90 retirement sentence, unlike "Retired" which also appears in unrelated rules
    }

    #[test]
    fn mentor_paths_build_expected_layout() {
        let root = Path::new("/tmp/proj");
        assert_eq!(
            project_md_path(root),
            Path::new("/tmp/proj/mentor/PROJECT.md")
        );
        assert_eq!(
            project_progress_path(root),
            Path::new("/tmp/proj/mentor/PROGRESS.md")
        );
    }

    #[test]
    fn paths_build_expected_layout() {
        assert_eq!(
            approach_path(Path::new("/proje"), "gtm"),
            Path::new("/proje/.usta/approaches/gtm.md")
        );
        assert_eq!(
            curriculum_path(Path::new("/proje"), "gtm"),
            Path::new("/proje/.usta/learner/curriculum/gtm.md")
        );
    }

    #[test]
    fn split_files_without_delimiter_is_progress() {
        let out = split_files("# Rust — İlerleme\niçerik");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "progress");
        assert!(out[0].1.contains("içerik"));
    }

    #[test]
    fn split_files_separates_three_files() {
        let reply = "===FILE: progress===\nP İÇERİK\n===FILE: approach===\nA İÇERİK\n===FILE: curriculum===\nC İÇERİK\n";
        let out = split_files(reply);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0], ("progress".to_string(), "P İÇERİK".to_string()));
        assert_eq!(out[1], ("approach".to_string(), "A İÇERİK".to_string()));
        assert_eq!(out[2], ("curriculum".to_string(), "C İÇERİK".to_string()));
    }

    #[test]
    fn split_files_cleans_fenced_content() {
        let reply = "===FILE: progress===\n```markdown\n# başlık\n```\n";
        let out = split_files(reply);
        assert_eq!(out[0].1, "# başlık");
    }

    #[test]
    fn closing_prompt_embeds_all_three_currents_and_delimiter() {
        let s = closing_prompt(
            "rust",
            Some("PMEVCUT"),
            Some("AMEVCUT"),
            Some("CMEVCUT"),
            None,
            None,
            None,
        );
        assert!(s.contains("PMEVCUT"));
        assert!(s.contains("AMEVCUT"));
        assert!(s.contains("CMEVCUT"));
        assert!(s.contains("===FILE:"));
        assert!(s.contains("not seen/seen/settled/deepened"));
    }

    #[test]
    fn closing_prompt_records_mock_exam() {
        let s = closing_prompt("rust", None, None, None, None, None, None);
        assert!(s.contains("/exam")); // references the command
        assert!(s.contains("record weak items as gaps")); // the recording rule
    }

    #[test]
    fn closing_prompt_defines_goal_sections() {
        let s = closing_prompt("almanca", None, None, None, None, None, None);
        assert!(s.contains("## Goal Status"));
        assert!(s.contains("## Goal"));
        assert!(s.contains("pace assessment"));
    }

    #[test]
    fn closing_prompt_defines_profile_rules() {
        let s = closing_prompt("rust", None, None, None, Some("CURRENT PROFILE"), None, None);
        assert!(s.contains("===FILE: profile==="));
        assert!(s.contains("CURRENT PROFILE"));
        assert!(s.contains("NO TOPIC KNOWLEDGE"));
        assert!(s.contains("only")); // generated only if new/changed info exists
    }

    #[test]
    fn closing_prompt_includes_mentor_file_rules() {
        let s = closing_prompt("rust", None, None, None, None, None, None);
        assert!(s.contains("project-progress"));
        assert!(s.contains("mentor/PROJECT.md"));
        assert!(s.contains("mentor/PROGRESS.md"));
        // append-only decision log rule must be spelled out
        assert!(s.contains("NEVER delete"));
        assert!(s.contains("## Kararlar"));
    }

    #[test]
    fn closing_prompt_embeds_current_mentor_files() {
        let s = closing_prompt(
            "rust",
            None,
            None,
            None,
            None,
            Some("PRJMEVCUT"),
            Some("PPGMEVCUT"),
        );
        assert!(s.contains("PRJMEVCUT"));
        assert!(s.contains("PPGMEVCUT"));
    }

    #[test]
    fn split_files_carries_mentor_names() {
        let reply = "===FILE: project===\nP\n===FILE: project-progress===\nQ";
        let files = split_files(reply);
        assert_eq!(files[0], ("project".to_string(), "P".to_string()));
        assert_eq!(files[1], ("project-progress".to_string(), "Q".to_string()));
    }

    #[test]
    fn opening_prompt_mentions_project_pointer_when_known() {
        let s = opening_prompt("rust", false, true, None, &[], false);
        assert!(s.contains("mentor/PROGRESS.md"));
        assert!(s.contains("Sırada"));
        let s = opening_prompt("rust", false, false, None, &[], false);
        assert!(!s.contains("mentor/PROGRESS.md"));
    }

    #[test]
    fn onboarding_prompt_asks_project_basics_only_when_unknown() {
        let s = onboarding_prompt("rust", None, false, false, None);
        assert!(s.contains("mentor/PROJECT.md"));
        assert!(s.contains("what they're building"));
        let s = onboarding_prompt("rust", None, false, true, None);
        assert!(!s.contains("what they're building"));
    }

    #[test]
    fn onboarding_prompt_carries_user_intro_and_forbids_reasking() {
        let s = onboarding_prompt(
            "hosting",
            Some("müşterimin hesabına coolify kuracağım, Fedora, temel güvenlik lazım"),
            false,
            false,
            None,
        );
        assert!(s.contains("coolify kuracağım"));
        assert!(s.contains("FIRST ANSWER"));
        assert!(s.contains("don't ask again"));
        // If there's no intro, the block doesn't appear at all.
        let bare = onboarding_prompt("hosting", None, false, false, None);
        assert!(!bare.contains("FIRST ANSWER"));
    }

    #[test]
    fn onboarding_prompt_injects_material_block() {
        let s = onboarding_prompt("rust", None, false, false, Some("=== kitap.md ===\n# K"));
        assert!(s.contains("COURSE MATERIAL FOUND"));
        assert!(s.contains("=== kitap.md ==="));
        assert!(s.contains("ASK whether to anchor"));
        assert!(s.contains("source:"));
        let s = onboarding_prompt("rust", None, false, false, None);
        assert!(!s.contains("COURSE MATERIAL FOUND"));
    }

    #[test]
    fn onboarding_prompt_infers_goal_without_jargon_and_limits_questions() {
        let s = onboarding_prompt("almanca", None, false, false, None);
        // Exploration/goal terms are NOT asked to the user — the model infers them itself.
        assert!(!s.contains("keşif mi"));
        assert!(s.contains("infer it YOURSELF"));
        // Jargon-free fallback question + question limit.
        assert!(s.contains("a deadline or exam"));
        assert!(s.contains("at most two questions"));
        assert!(s.contains("## Goal"));
    }

    #[test]
    fn opening_prompt_embeds_topic_and_asks_to_quiz() {
        let due = vec!["- q — a | due: 2026-08-01 | ivl: 1".to_string()];
        let s = opening_prompt("rust", false, false, None, &due, true);
        assert!(s.contains("rust"));
        assert!(s.contains("RECALL DRILL"));
        assert!(s.contains("ASK"));
    }

    #[test]
    fn onboarding_prompt_embeds_topic_and_open_conversation() {
        let s = onboarding_prompt("linux-guvenlik", None, false, false, None);
        assert!(s.contains("linux-guvenlik"));
        assert!(s.contains("INTRODUCTION"));
        assert!(s.contains("form"));
    }

    #[test]
    fn onboarding_prompt_does_not_tell_model_it_writes_files() {
        // Hard Rule 6: the model has no file-writing tool — it produces the closing
        // content, the shell writes the file. The prompt must not push the model to try writing.
        let s = onboarding_prompt("rust", None, false, false, None);
        assert!(!s.contains("you will write files"));
        assert!(s.contains("shell writes"));
    }

    #[test]
    fn opening_prompt_mentions_curriculum_position() {
        let s = opening_prompt("rust", false, false, None, &[], false);
        assert!(s.contains("map"));
    }

    #[test]
    fn closing_prompt_preserves_material_source_refs() {
        let s = closing_prompt("rust", None, None, None, None, None, None);
        assert!(s.contains("source:"));
        assert!(s.contains("— source: web"));
    }

    #[test]
    fn closing_prompt_defines_open_exercise_section() {
        let s = closing_prompt("rust", None, None, None, None, None, None);
        assert!(s.contains("## Open exercise"));
        assert!(s.contains("assigned"));
    }

    #[test]
    fn opening_prompt_reminds_open_exercise() {
        let s = opening_prompt("rust", false, false, None, &[], false);
        assert!(s.contains("open exercise"));
    }

    #[test]
    fn opening_prompt_asks_due_list_when_due_present() {
        // The shell already selected/sorted/capped these — opening_prompt just phrases
        // the turn around them, no filtering instruction should remain.
        let due = vec![
            "- Borrow checker ne yapar? — sahipliği derlemede doğrular | due: 2026-08-14 | ivl: 3".to_string(),
            "- Trait nedir? — davranış sözleşmesi | due: 2026-08-15 | ivl: 1".to_string(),
        ];
        let s = opening_prompt("rust", false, false, None, &due, true);
        assert!(s.contains("Borrow checker ne yapar?"));
        assert!(s.contains("Trait nedir?"));
        assert!(!s.contains("Pick ONLY"));
        assert!(!s.contains("oldest due first"));
        assert!(!s.contains("today or earlier"));
    }

    #[test]
    fn opening_prompt_says_no_reviews_when_due_empty_but_questions_exist() {
        let s = opening_prompt("rust", false, false, None, &[], true);
        assert!(s.contains("no reviews due today"));
        assert!(!s.contains("come up with 2 small recall questions"));
    }

    #[test]
    fn opening_prompt_invents_questions_when_no_questions_exist() {
        let s = opening_prompt("rust", false, false, None, &[], false);
        assert!(s.contains("come up with 2 small recall questions"));
        assert!(!s.contains("no reviews due today"));
    }

    #[test]
    fn opening_prompts_include_meet_block_only_when_profile_generic() {
        let on = onboarding_prompt("rust", None, true, false, None);
        assert!(on.contains("[PROFILE EMPTY]"));
        assert!(on.contains("1-2 questions"));
        assert!(!onboarding_prompt("rust", None, false, false, None).contains("[PROFILE EMPTY]"));

        let op = opening_prompt("rust", true, false, None, &[], false);
        assert!(op.contains("[PROFILE EMPTY]"));
        assert!(!opening_prompt("rust", false, false, None, &[], false).contains("[PROFILE EMPTY]"));
    }

    #[test]
    fn opening_prompt_carries_game_streak_block() {
        let s = opening_prompt("rust", false, false, Some("streak: 3 day(s) (longest 6)"), &[], false);
        assert!(s.contains("[GAME] streak: 3 day(s) (longest 6)"));
        let s = opening_prompt("rust", false, false, None, &[], false);
        assert!(!s.contains("[GAME]"));
    }

    #[test]
    fn clean_reply_strips_fenced_block() {
        let raw = "```markdown\n# Rust — Progress\n- Level: orta\n```";
        assert_eq!(
            clean_markdown_reply(raw),
            "# Rust — Progress\n- Level: orta"
        );
    }

    #[test]
    fn clean_reply_passes_plain_text_through() {
        assert_eq!(clean_markdown_reply("  # Başlık\niçerik  "), "# Başlık\niçerik");
    }

    #[test]
    fn write_atomic_creates_parents_and_writes() {
        let base = std::env::temp_dir().join(format!(
            "usta_progress_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let target = base.join("derin/dizin/rust.md");
        write_atomic(&target, "içerik").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "içerik");
        // No tmp file should remain.
        assert!(!target.with_extension("md.tmp").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn write_atomic_backs_up_previous_version() {
        let base = std::env::temp_dir().join(format!(
            "usta_progress_bak_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let target = base.join("rust.md");
        write_atomic(&target, "ilk sürüm").unwrap();
        assert!(!target.with_extension("md.bak").exists()); // no backup on the first write
        write_atomic(&target, "ikinci sürüm").unwrap();
        assert_eq!(
            std::fs::read_to_string(target.with_extension("md.bak")).unwrap(),
            "ilk sürüm"
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "ikinci sürüm");
        let _ = std::fs::remove_dir_all(&base);
    }
