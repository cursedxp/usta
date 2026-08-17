//! Test module for welcome_data.rs, split out for file size; still a child module via #[path], private access preserved.

use super::*;

const PROFILE: &str = "# Öğrenci Profili — Ada\n\n## Kim\n- test";
const PROGRESS: &str = "# rust — Progress\n## Level\n- Orta: ownership settled\n## Recall questions\n- Soru 1? — cevap\n- Soru 2? — cevap\n- Soru 3? — cevap\n";
const CURRICULUM: &str = "# rust haritası\n- Ownership: settled\n- Borrowing: seen\n- Lifetimes: not seen\n- Traits: not seen\n";

#[test]
fn extract_name_reads_h1_after_dash() {
    assert_eq!(extract_name(PROFILE), Some("Ada".to_string()));
    assert_eq!(extract_name("# Başlıksız"), None);
    assert_eq!(extract_name(""), None);
}

#[test]
fn extract_level_reads_first_line_of_section() {
    assert_eq!(
        extract_level(PROGRESS),
        Some("Orta: ownership settled".to_string())
    );
    assert_eq!(extract_level("# boş"), None);
}

#[test]
fn curriculum_percent_counts_non_unseen() {
    // 4 items have a status, 2 are `not seen` → 50%
    assert_eq!(curriculum_percent(CURRICULUM), Some(50));
    assert_eq!(curriculum_percent("# durum yok"), None);
}

#[test]
fn next_unseen_returns_first_unseen_item_text() {
    assert_eq!(next_unseen(CURRICULUM), Some("Lifetimes".to_string()));
    assert_eq!(next_unseen("- Hepsi: settled"), None);
}

#[test]
fn state_matching_is_exact_segment_not_substring() {
    // Item TEXT contains a state word — must not be counted/confused with it.
    let c = "- makale hakkında settled: not seen\n- borrow: settled\n";
    assert_eq!(curriculum_percent(c), Some(50)); // 1/2 seen
    assert_eq!(next_unseen(c).as_deref(), Some("makale hakkında settled"));
}

#[test]
fn state_matching_uses_trailing_segment_not_item_text_word() {
    // Item text contains "not seen" but the REAL state is "settled" (seen).
    // The old contains-logic would have miscounted this as not-seen; map_state_of reads the trailing segment.
    let c = "- not seen decision result: settled\n- Lifetimes: not seen\n";
    // First item is seen (settled), second is not-seen → 1/2 = 50%.
    assert_eq!(curriculum_percent(c), Some(50));
    // next_unseen must SKIP the first item, return the truly not-seen second one.
    assert_eq!(next_unseen(c).as_deref(), Some("Lifetimes"));
}

#[test]
fn drill_count_counts_section_bullets() {
    assert_eq!(drill_count(PROGRESS), 3);
    assert_eq!(drill_count("# soru yok"), 0);
}

#[test]
fn due_count_counts_due_and_untagged_skips_future() {
    let p = "\
# rust — Progress

## Recall questions
- Borrow checker ne yapar? — sahipliği derlemede doğrular | due: 2026-08-14 | ivl: 3
- Trait nedir? — davranış sözleşmesi | due: 2026-08-15 | ivl: 1
- Lifetime nedir? — referans ömrü | due: 2026-09-01 | ivl: 35
- Eski format soru — cevap

## Error log
- typo | 1 | due: 2026-08-01 gibi görünen ama başka bölümde
";
    // past + today + untagged = 3; future (09-01) and other-section lines don't count
    assert_eq!(due_count(p, "2026-08-15"), 3);
    assert_eq!(due_count(p, "2026-08-13"), 1); // only untagged counts as due
    assert_eq!(due_count("# bos", "2026-08-15"), 0);
}

#[test]
fn due_questions_selects_and_orders_oldest_due_first() {
    let p = "\
# rust — Progress

## Recall questions
- B sorusu — cevap | due: 2026-08-15 | ivl: 1
- A sorusu — cevap | due: 2026-08-14 | ivl: 3
- Eski format soru — cevap
- Gelecek soru — cevap | due: 2026-09-01 | ivl: 35
";
    let qs = due_questions(p, "2026-08-15");
    // legacy/untagged sorts first (empty key = due now), then 08-14, then 08-15;
    // the future (09-01) bullet is excluded entirely.
    assert_eq!(qs.len(), 3);
    assert!(qs[0].starts_with("- Eski format soru"));
    assert!(qs[1].starts_with("- A sorusu"));
    assert!(qs[2].starts_with("- B sorusu"));
    assert!(qs.iter().all(|q| !q.contains("Gelecek soru")));
}

#[test]
fn due_questions_caps_at_three_but_due_count_stays_uncapped() {
    let p = "\
## Recall questions
- S1 — c | due: 2026-08-10 | ivl: 1
- S2 — c | due: 2026-08-11 | ivl: 1
- S3 — c | due: 2026-08-12 | ivl: 1
- S4 — c | due: 2026-08-13 | ivl: 1
";
    let qs = due_questions(p, "2026-08-15");
    assert_eq!(qs.len(), 3);
    assert!(qs[0].contains("S1"));
    assert!(qs[1].contains("S2"));
    assert!(qs[2].contains("S3"));
    assert_eq!(due_count(p, "2026-08-15"), 4); // count is NOT truncated
}

#[test]
fn due_questions_excludes_other_section_bullets() {
    let p = "\
## Recall questions
- Soru — cevap | due: 2026-08-01 | ivl: 1

## Error log
- typo | 1 | due: 2026-08-01 gibi görünen ama başka bölümde
";
    let qs = due_questions(p, "2026-08-15");
    assert_eq!(qs.len(), 1);
    assert!(qs[0].contains("Soru"));
    assert!(qs.iter().all(|q| !q.contains("typo")));
}

#[test]
fn due_count_matches_due_questions_len_when_three_or_fewer() {
    let p = "## Recall questions\n- q1 — a | due: 2026-08-01 | ivl: 1\n- q2 — a | due: 2026-08-02 | ivl: 1\n";
    assert_eq!(
        due_count(p, "2026-08-15"),
        due_questions(p, "2026-08-15").len()
    );
}

#[test]
fn gather_full_and_first_session() {
    let d = gather(
        Some(PROFILE),
        Some(PROGRESS),
        Some(CURRICULUM),
        "rust",
        "opus · cli",
        "~/x",
        "2026-08-15",
        None,
    );
    assert!(!d.first_session);
    assert_eq!(d.name.as_deref(), Some("Ada"));
    assert_eq!(d.map_percent, Some(50));
    let d2 = gather(
        None,
        None,
        None,
        "gtm",
        "opus · cli",
        "~/x",
        "2026-08-15",
        None,
    );
    assert!(d2.first_session);
    assert_eq!(d2.drill_count, 0);
}

fn mk_entry(date: &str, topic: &str) -> crate::history::Entry {
    crate::history::Entry {
        date: date.to_string(),
        topic: topic.to_string(),
        map: None,
        settled: None,
    }
}

#[test]
fn last_session_ago_today_yesterday_and_days() {
    let today_e = vec![mk_entry("2026-08-15", "rust")];
    assert_eq!(
        last_session_ago(&today_e, "rust", "2026-08-15"),
        Some("today".to_string())
    );

    let yesterday_e = vec![mk_entry("2026-08-14", "rust")];
    assert_eq!(
        last_session_ago(&yesterday_e, "rust", "2026-08-15"),
        Some("yesterday".to_string())
    );

    let days_e = vec![mk_entry("2026-08-10", "rust")];
    assert_eq!(
        last_session_ago(&days_e, "rust", "2026-08-15"),
        Some("5 days ago".to_string())
    );
}

#[test]
fn last_session_ago_picks_newest_not_last_line() {
    // older date written AFTER the newer one in file/vec order — max must
    // still win, not the last element.
    let es = vec![
        mk_entry("2026-08-14", "rust"),
        mk_entry("2026-08-10", "rust"),
    ];
    assert_eq!(
        last_session_ago(&es, "rust", "2026-08-15"),
        Some("yesterday".to_string())
    );
}

#[test]
fn last_session_ago_filters_by_topic() {
    let es = vec![
        mk_entry("2026-08-15", "gtm"),
        mk_entry("2026-08-10", "rust"),
    ];
    assert_eq!(
        last_session_ago(&es, "rust", "2026-08-15"),
        Some("5 days ago".to_string())
    );
}

#[test]
fn last_session_ago_none_without_entry() {
    let es = vec![mk_entry("2026-08-15", "gtm")];
    assert_eq!(last_session_ago(&es, "rust", "2026-08-15"), None);
}

#[test]
fn last_session_ago_future_date_is_today() {
    let es = vec![mk_entry("2026-08-16", "rust")];
    assert_eq!(
        last_session_ago(&es, "rust", "2026-08-15"),
        Some("today".to_string())
    );
}

#[test]
fn gather_fills_last_session() {
    let h = "# Session History\n- 2026-08-14 | rust | map 40% | settled 4\n";
    let d = gather(
        None,
        None,
        None,
        "rust",
        "opus · cli",
        "~/x",
        "2026-08-15",
        Some(h),
    );
    assert_eq!(d.last_session.as_deref(), Some("yesterday"));

    let d2 = gather(
        None,
        None,
        None,
        "rust",
        "opus · cli",
        "~/x",
        "2026-08-15",
        None,
    );
    assert_eq!(d2.last_session, None);
}
