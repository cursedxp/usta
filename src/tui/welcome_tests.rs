//! Test module for welcome.rs, split out for file size; still a child module via #[path], private access preserved.

use super::*;
use ratatui::text::Text;

#[test]
fn version_aligned_with_spec() {
    assert_eq!(env!("CARGO_PKG_VERSION"), "0.22.0");
}

fn plain_lines(t: &Text) -> Vec<String> {
    t.lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect()
}

const PROFILE: &str = "# Öğrenci Profili — Ada\n\n## Kim\n- test";
const PROGRESS: &str = "# rust — Progress\n## Level\n- Orta: ownership settled\n## Recall questions\n- Soru 1? — cevap\n- Soru 2? — cevap\n- Soru 3? — cevap\n";
const CURRICULUM: &str = "# rust haritası\n- Ownership: settled\n- Borrowing: seen\n- Lifetimes: not seen\n- Traits: not seen\n";

#[test]
fn welcome_shows_due_line_three_states() {
    // state 1: due questions exist → "Reviews due today: N"
    let p_due = "## Recall questions\n- q — a | due: 2026-01-01 | ivl: 1\n";
    let d = crate::tui::welcome_data::gather(
        None,
        Some(p_due),
        None,
        "rust",
        "opus · cli",
        "~/x",
        "2026-08-15",
        None,
    );
    let joined = plain_lines(&render_welcome(&d, 80)).join("\n");
    assert!(joined.contains("Reviews due today: 1"));

    // state 2: questions exist, none due → "No reviews due today"
    let p_future = "## Recall questions\n- q — a | due: 2099-01-01 | ivl: 90\n";
    let d = crate::tui::welcome_data::gather(
        None,
        Some(p_future),
        None,
        "rust",
        "opus · cli",
        "~/x",
        "2026-08-15",
        None,
    );
    let joined = plain_lines(&render_welcome(&d, 80)).join("\n");
    assert!(joined.contains("No reviews due today"));
    assert!(!joined.contains("Reviews due today:"));

    // state 3: no questions at all → neither line
    let d = crate::tui::welcome_data::gather(
        None,
        Some("# bos"),
        None,
        "rust",
        "opus · cli",
        "~/x",
        "2026-08-15",
        None,
    );
    let joined = plain_lines(&render_welcome(&d, 80)).join("\n");
    assert!(!joined.contains("Reviews due"));
    assert!(!joined.contains("No reviews due"));
}

#[test]
fn render_welcome_lines_have_equal_display_width() {
    use unicode_width::UnicodeWidthStr;
    let d = crate::tui::welcome_data::gather(
        Some(PROFILE),
        Some(PROGRESS),
        Some(CURRICULUM),
        "rust",
        "opus · cli",
        "~/proje",
        "2026-08-15",
        None,
    );
    let t = render_welcome(&d, 80);
    let lines = plain_lines(&t);
    assert!(lines.len() >= 8);
    // Last line is the appended help hint — NOT part of the bordered box,
    // so it's excluded from the equal-width check (spec: separate Line, box intact).
    let box_lines = &lines[..lines.len() - 1];
    let w = box_lines[0].width();
    assert!(
        box_lines.iter().all(|l| l.width() == w),
        "hizasız satır: {lines:#?}"
    );
    assert!(box_lines[0].starts_with('╭') && box_lines.last().unwrap().starts_with('╰'));
    assert_eq!(lines.last().unwrap(), crate::help::HELP_HINT);
}

#[test]
fn render_welcome_first_session_shows_intro_message() {
    let d = crate::tui::welcome_data::gather(
        None,
        None,
        None,
        "gtm",
        "opus · cli",
        "~/p",
        "2026-08-15",
        None,
    );
    let joined = plain_lines(&render_welcome(&d, 80)).join("\n");
    assert!(joined.contains("First session"));
    assert!(joined.contains("Welcome back"));
}

#[test]
fn fit_truncates_by_display_width_with_ellipsis() {
    assert_eq!(fit("çğşöü-uzun-metin", 8), "çğşöü-u…");
    assert_eq!(fit("kısa", 10), "kısa");
}

#[test]
fn wrap_short_string_passes_through_as_one_line() {
    assert_eq!(wrap("hello", 20), vec!["hello".to_string()]);
}

#[test]
fn wrap_empty_input_returns_no_lines() {
    assert_eq!(wrap("", 10), Vec::<String>::new());
}

#[test]
fn wrap_breaks_at_word_boundary_within_width() {
    let s = "The quick brown fox jumps over the lazy dog";
    let lines = wrap(s, 10);
    assert!(lines.len() > 1, "expected multiple lines: {lines:?}");
    assert!(
        lines.iter().all(|l| l.width() <= 10),
        "line exceeds max: {lines:?}"
    );
    // every original word survives, in order, none lost/duplicated
    let rejoined = lines.join(" ");
    assert_eq!(rejoined, s);
}

#[test]
fn wrap_oversized_single_word_falls_back_to_char_split() {
    let s = "supercalifragilisticexpialidocious";
    let lines = wrap(s, 5);
    assert!(lines.len() > 1, "expected multiple lines: {lines:?}");
    assert!(
        lines.iter().all(|l| l.width() <= 5),
        "line exceeds max: {lines:?}"
    );
    assert_eq!(lines.concat(), s);
}

#[test]
fn wrap_unicode_width_aware_not_byte_counting() {
    // Turkish chars: byte-counting would misalign vs display width — reuse
    // fit's documented concern for wrap.
    let s = "çğşöü ıİĞÜ test kelimeler burada";
    let lines = wrap(s, 8);
    assert!(
        lines.iter().all(|l| l.width() <= 8),
        "line exceeds max: {lines:?}"
    );
}

#[test]
fn render_welcome_long_next_item_wraps_full_text_no_ellipsis() {
    let long_item = "Async trait objects and pinning semantics in tokio task spawning";
    let curriculum = format!("# rust haritası\n- {long_item}: not seen\n");
    let d = crate::tui::welcome_data::gather(
        Some(PROFILE),
        Some(PROGRESS),
        Some(&curriculum),
        "rust",
        "opus · cli",
        "~/x",
        "2026-08-15",
        None,
    );
    assert_eq!(d.next_item.as_deref(), Some(long_item));
    let t = render_welcome(&d, 80);
    let lines = plain_lines(&t);
    let joined = lines.join(" ");
    assert!(!joined.contains('…'), "next_item was truncated: {lines:#?}");
    for word in long_item.split(' ') {
        assert!(
            joined.contains(word),
            "missing word '{word}' from wrapped next_item: {lines:#?}"
        );
    }
    // last word of the long sentence must show up somewhere — proves the
    // tail wasn't dropped, not just the head before an ellipsis.
    assert!(joined.contains("spawning"));
}

#[test]
fn render_identity_with_topics_lists_them_and_equal_width() {
    use unicode_width::UnicodeWidthStr;
    let local = vec!["rust".to_string(), "gtm".to_string()];
    let t = render_welcome_identity(
        Some("Ada"),
        "opus · cli",
        "~/p",
        &local,
        &[],
        false,
        80,
        0,
        0,
    );
    let lines = plain_lines(&t);
    // Last line is the appended help hint — NOT part of the bordered box.
    let box_lines = &lines[..lines.len() - 1];
    let w = box_lines[0].width();
    assert!(
        box_lines.iter().all(|l| l.width() == w),
        "hizasız: {lines:#?}"
    );
    let joined = lines.join("\n");
    assert!(joined.contains("What do you want to learn?"));
    assert!(joined.contains("rust"));
    assert!(joined.contains("Hello, Ada!"));
    assert!(box_lines[0].starts_with('╭') && box_lines.last().unwrap().starts_with('╰'));
    assert_eq!(lines.last().unwrap(), crate::help::HELP_HINT);
}

#[test]
fn render_identity_no_topics_shows_first_session_and_no_name() {
    let t = render_welcome_identity(None, "opus · cli", "~/p", &[], &[], false, 80, 0, 0);
    let joined = plain_lines(&t).join("\n");
    assert!(joined.contains("What do you want to learn?"));
    assert!(joined.contains("Hello!")); // no name → generic
    assert!(!joined.contains("Hello,")); // no "Hello, X!" form
    assert!(!joined.contains("Enter →")); // no topic → no continuation line
}

#[test]
fn identity_welcome_lists_local_topics_with_enter_hint() {
    let local = vec![
        "brainstorm-ilk-adim".to_string(),
        "linux-guvenlik".to_string(),
    ];
    let other = vec!["rust".to_string()];
    let t = render_welcome_identity(
        Some("Anil"),
        "opus · cli",
        "~/x",
        &local,
        &other,
        false,
        80,
        0,
        0,
    );
    let joined = plain_lines(&t).join("\n");
    assert!(joined.contains("Enter"));
    assert!(joined.contains("brainstorm-ilk-adim"));
    assert!(joined.contains("1)"));
    assert!(joined.contains("2)"));
    assert!(joined.contains("In other projects"));
    // Hizalama korunur — appended help hint (last line) hariç.
    use unicode_width::UnicodeWidthStr;
    let lines = plain_lines(&t);
    let box_lines = &lines[..lines.len() - 1];
    let w = box_lines[0].width();
    assert!(
        box_lines.iter().all(|l| l.width() == w),
        "hizasız: {lines:#?}"
    );
    assert_eq!(lines.last().unwrap(), crate::help::HELP_HINT);
}

#[test]
fn identity_welcome_without_local_topics_keeps_first_run_look() {
    let t = render_welcome_identity(None, "opus · cli", "~/x", &[], &[], false, 80, 0, 0);
    let joined = plain_lines(&t).join("\n");
    assert!(joined.contains("What do you want to learn"));
    assert!(joined.contains("First session"));
    assert!(!joined.contains("Enter →"));
}

#[test]
fn first_session_hint_becomes_suggest_hint_when_project_known() {
    // Call render_welcome_identity twice with empty `local`, flipping only
    // project_known.
    let not_known = render_welcome_identity(None, "opus · cli", "~/p", &[], &[], false, 80, 0, 0);
    let known = render_welcome_identity(None, "opus · cli", "~/p", &[], &[], true, 80, 0, 0);
    let joined_not_known = plain_lines(&not_known).join("\n");
    let joined_known = plain_lines(&known).join("\n");
    assert!(joined_not_known.contains("First session — type a topic."));
    assert!(joined_known.contains("PROJECT.md found"));
    assert!(joined_known.contains("Enter"));
}

#[test]
fn identity_welcome_other_projects_line_is_dim() {
    let local = vec!["rust".to_string()];
    let other = vec!["gtm".to_string()];
    let t = render_welcome_identity(
        Some("Ada"),
        "opus · cli",
        "~/p",
        &local,
        &other,
        false,
        80,
        0,
        0,
    );
    let span = t
        .lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .find(|s| s.content.contains("In other projects"))
        .expect("In other projects satırı bulunamalı");
    assert!(
        span.style.add_modifier.contains(Modifier::DIM),
        "stil DIM içermiyor: {:?}",
        span.style
    );
}

/// Count distinct orange (BRAND) elements: the whole logo block collapses to
/// one element (all its spans carry █), plus each non-logo brand span with
/// real text. Design tokens 06: ≤ 2 orange elements per screen at rest.
fn orange_element_count(t: &Text) -> usize {
    let mut logo_seen = false;
    let mut others = 0usize;
    for line in &t.lines {
        for s in &line.spans {
            if s.style.fg != Some(theme::BRAND) {
                continue;
            }
            if s.content.contains('█') {
                logo_seen = true;
            } else if !s.content.trim().is_empty() {
                others += 1;
            }
        }
    }
    (logo_seen as usize) + others
}

#[test]
fn welcome_orange_discipline() {
    // Identity welcome at rest: logo block + the single section accent = 2.
    let local = vec!["rust".to_string()];
    let ident = render_welcome_identity(
        Some("Ada"),
        "opus · cli",
        "~/p",
        &local,
        &[],
        false,
        80,
        0,
        0,
    );
    assert!(
        orange_element_count(&ident) <= 2,
        "identity orange > 2: {ident:#?}"
    );

    // Full-mode welcome at rest: logo block + "Learning Status" title = 2.
    let d = crate::tui::welcome_data::gather(
        Some(PROFILE),
        Some(PROGRESS),
        Some(CURRICULUM),
        "rust",
        "opus · cli",
        "~/p",
        "2026-08-15",
        None,
    );
    let full = render_welcome(&d, 80);
    assert!(
        orange_element_count(&full) <= 2,
        "full-mode orange > 2: {full:#?}"
    );
}

#[test]
fn welcome_shows_week_line() {
    // state 1: sessions this week + an unbroken streak → full line.
    let h = "# Session History\n- 2026-08-14 | rust | map 40% | settled 4\n- 2026-08-15 | rust | map 55% | settled 7\n";
    let d = crate::tui::welcome_data::gather(
        None,
        None,
        None,
        "rust",
        "opus · cli",
        "~/x",
        "2026-08-15",
        Some(h),
    );
    assert_eq!(d.week_sessions, 2);
    assert_eq!(d.streak, 2);
    let joined = plain_lines(&render_welcome(&d, 80)).join("\n");
    assert!(joined.contains("This week: 2 session(s) · streak 2 day(s)"));

    // state 2: sessions this week but streak == 0 (entries are 2+ days before
    // `today`, so `current_streak` sees a broken run — an ADHD-unsafe "streak 0"
    // must never be rendered, only the sessions count survives).
    let h0 = "# Session History\n- 2026-08-10 | rust | map 40% | settled 4\n";
    let d0 = crate::tui::welcome_data::gather(
        None,
        None,
        None,
        "rust",
        "opus · cli",
        "~/x",
        "2026-08-15",
        Some(h0),
    );
    assert_eq!(d0.week_sessions, 1);
    assert_eq!(d0.streak, 0);
    let joined0 = plain_lines(&render_welcome(&d0, 80)).join("\n");
    assert!(joined0.contains("This week: 1 session(s)"));
    assert!(!joined0.contains("· streak"));
    assert!(!joined0.contains("streak 0"));

    // state 3: no history at all → no "This week" line.
    let dn = crate::tui::welcome_data::gather(
        None,
        None,
        None,
        "rust",
        "opus · cli",
        "~/x",
        "2026-08-15",
        None,
    );
    assert_eq!(dn.week_sessions, 0);
    let joinedn = plain_lines(&render_welcome(&dn, 80)).join("\n");
    assert!(!joinedn.contains("This week"));
}

#[test]
fn identity_welcome_shows_week_line_when_sessions_present() {
    let t = render_welcome_identity(Some("Ada"), "opus · cli", "~/p", &[], &[], false, 80, 3, 1);
    let joined = plain_lines(&t).join("\n");
    assert!(joined.contains("This week: 3 session(s) · streak 1 day(s)"));

    let t0 = render_welcome_identity(Some("Ada"), "opus · cli", "~/p", &[], &[], false, 80, 0, 0);
    let joined0 = plain_lines(&t0).join("\n");
    assert!(!joined0.contains("This week"));
}

fn full_resume_data() -> WelcomeData {
    WelcomeData {
        version: env!("CARGO_PKG_VERSION"),
        name: None,
        model: "opus · cli".to_string(),
        dir: "~/x".to_string(),
        topic: "kaynak-ingest".to_string(),
        level: Some("Başlangıç — sıfır noktası".to_string()),
        map_percent: Some(25),
        next_item: Some("URL → HTML fetch, then strip to text".to_string()),
        drill_count: 3,
        due_count: 3,
        first_session: false,
        week_sessions: 2,
        streak: 2,
        last_session: Some("2 days ago".to_string()),
    }
}

#[test]
fn render_resume_lines_have_equal_display_width() {
    let d = full_resume_data();
    let t = render_resume(&d, 80).unwrap();
    let lines = plain_lines(&t);
    assert!(lines.len() >= 3);
    let w = lines[0].width();
    assert!(
        lines.iter().all(|l| l.width() == w),
        "hizasız satır: {lines:#?}"
    );
    assert!(lines[0].starts_with('╭') && lines.last().unwrap().starts_with('╰'));
}

#[test]
fn render_resume_orange_discipline() {
    let d = full_resume_data();
    let t = render_resume(&d, 80).unwrap();
    assert!(orange_element_count(&t) <= 2, "resume orange > 2: {t:#?}");
}

#[test]
fn render_resume_has_no_identity() {
    let d = full_resume_data();
    let joined = plain_lines(&render_resume(&d, 80).unwrap()).join("\n");
    assert!(!joined.contains("██"));
    assert!(!joined.contains("Welcome back"));
    assert!(!joined.contains("opus · cli"));
    assert!(!joined.contains("This week"));
    assert!(!joined.contains(crate::help::HELP_HINT));
}

#[test]
fn render_resume_title_names_the_topic() {
    let d = full_resume_data();
    let lines = plain_lines(&render_resume(&d, 80).unwrap());
    assert!(
        lines[0].contains("Continuing · kaynak-ingest"),
        "top border: {}",
        lines[0]
    );
}

#[test]
fn render_resume_sparse_drops_rows() {
    let mut d = full_resume_data();
    d.map_percent = None;
    d.due_count = 0;
    d.level = None;
    let joined = plain_lines(&render_resume(&d, 80).unwrap()).join("\n");
    assert!(joined.contains("Last session"));
    assert!(joined.contains("Up next"));
    assert!(!joined.contains("Map"));
    assert!(!joined.contains("Reviews"));
    assert!(!joined.contains("Level"));
}

#[test]
fn render_resume_bar_reflects_percent() {
    assert_eq!(map_bar(25).matches('▓').count(), 3);
    assert!(map_bar(1).matches('▓').count() >= 1);
    assert_eq!(map_bar(100).matches('▓').count(), 12);
}

#[test]
fn render_resume_long_next_item_wraps_no_ellipsis() {
    let long_item =
        "Async trait objects and pinning semantics in tokio task spawning and scheduling";
    let mut d = full_resume_data();
    d.next_item = Some(long_item.to_string());
    let lines = plain_lines(&render_resume(&d, 80).unwrap());
    let joined = lines.join(" ");
    assert!(!joined.contains('…'), "next_item was truncated: {lines:#?}");
    for word in long_item.split(' ') {
        assert!(
            joined.contains(word),
            "missing word '{word}' from wrapped next_item: {lines:#?}"
        );
    }
    assert!(joined.contains("scheduling"));
}

// Finding 1 (CRITICAL): `solo_box`'s dash-run formula
// (`inner.saturating_sub(4 + title.width())`) floors at 0 for an
// oversized title but never caps the title itself, so the top border
// prints longer than every other line. `render_resume` builds its title
// as `"Continuing · {topic} "` (14 + topic.len()) and `d.topic` is a
// slug from free-typed input with no length cap — a realistic topic like
// this 41-char one overflows at width 60 (the legitimate floor of the
// clamp): title_w = 14 + 41 = 55 > inner(58) - 4 = 54.
#[test]
fn render_resume_long_topic_keeps_top_border_aligned() {
    let mut d = full_resume_data();
    d.topic = "async-trait-objects-and-pinning-semantics".to_string(); // 41 chars
    for width in [60u16, 70, 80, 90, 100] {
        let lines = plain_lines(&render_resume(&d, width).unwrap());
        let w = lines[0].width();
        assert!(
            lines.iter().all(|l| l.width() == w),
            "hizasız satır (width={width}): {lines:#?}"
        );
    }
}

// Finding 2 (IMPORTANT): row 1 (`Last session {rel} · Level {level}`) is
// built directly from spans with no wrap and no length bound. `d.level`
// comes from free-form curriculum text (first non-empty line of a
// markdown section) — a long level string overflows `inner` and hits the
// same floor-to-zero padding in `solo_box`'s row loop.
#[test]
fn render_resume_long_level_row_wraps_and_stays_aligned() {
    let mut d = full_resume_data();
    d.level = Some(
            "Intermediate — deep dive into async trait objects, pinning, and Send/Sync bounds for task schedulers"
                .to_string(),
        );
    let t = render_resume(&d, 60).unwrap();
    let lines = plain_lines(&t);
    let w = lines[0].width();
    assert!(
        lines.iter().all(|l| l.width() == w),
        "hizasız satır: {lines:#?}"
    );
    let joined = lines.join(" ");
    assert!(
        !joined.contains('…'),
        "level was truncated instead of wrapped: {lines:#?}"
    );
    assert!(
        joined.contains("schedulers"),
        "tail of wrapped level missing: {lines:#?}"
    );
}

// Finding 3: `solo_box` deliberately does NOT `.trim()` its title (unlike
// `render_box`) because `render_resume` bakes a trailing space into the
// title so the dash run starts one column later, matching the design
// mock. This pins that exact spacing so a future ".trim()" addition
// fails loudly instead of silently swallowing the space.
#[test]
fn solo_box_preserves_title_trailing_space_no_trim() {
    let t = solo_box("Continuing · topic ", vec![], 80);
    let lines = plain_lines(&t);
    assert!(
        lines[0].contains("topic ─"),
        "trailing space before dash run was trimmed: {}",
        lines[0]
    );
}

// render_for_entry is the dispatcher run.rs uses to pick between the two
// welcome renderers on the `had_topic_arg || resumed` path (v0.21.0 fix).
// Before this extraction, the choice was an inline `if had_topic_arg {
// render_welcome } else { render_resume }` in run.rs's async TUI loop —
// untested, so an inverted or swapped condition would either silently
// reintroduce the duplicate-identity-box bug or show the wrong frame,
// and nothing would catch it. These tests pin the dispatch itself.

#[test]
fn render_for_entry_with_topic_arg_yields_full_box_with_identity() {
    // `had_topic_arg = true` == `usta start <topic>`: no identity frame
    // was printed earlier on this path, so the full-mode box (which
    // carries the logo/greeting/model/dir) must be what's shown.
    let d = full_resume_data();
    let t = render_for_entry(true, &d, 80).expect("full-mode box always has identity content");
    let joined = plain_lines(&t).join("\n");
    assert!(joined.contains("██"), "missing logo block: {joined}");
    assert!(
        joined.contains("Welcome back"),
        "missing greeting: {joined}"
    );
    assert!(
        joined.contains("opus · cli"),
        "missing model line: {joined}"
    );
}

#[test]
fn render_for_entry_without_topic_arg_yields_resume_panel_no_identity() {
    // `had_topic_arg = false` is the resume path: identity was already
    // printed by ask_topic's identity welcome, so this panel must carry
    // the `Continuing · <topic>` title and NONE of the identity content
    // — that absence is the entire point of the fix this dispatcher
    // guards, so it's asserted directly rather than inferred.
    let d = full_resume_data();
    let t = render_for_entry(false, &d, 80).expect("resume data is present, panel must render");
    let lines = plain_lines(&t);
    assert!(
        lines[0].contains("Continuing · kaynak-ingest"),
        "missing continuation title: {}",
        lines[0]
    );
    let joined = lines.join("\n");
    assert!(
        !joined.contains("██"),
        "logo block leaked into resume panel: {joined}"
    );
    assert!(
        !joined.contains("Welcome back"),
        "greeting leaked into resume panel: {joined}"
    );
    assert!(
        !joined.contains("opus · cli"),
        "model line leaked into resume panel: {joined}"
    );
}

// --- TDD probes for the final review's REQUIRED findings ---------------

fn empty_resume_data(topic: &str) -> WelcomeData {
    WelcomeData {
        version: env!("CARGO_PKG_VERSION"),
        name: None,
        model: "opus · cli".to_string(),
        dir: "~/x".to_string(),
        topic: topic.to_string(),
        level: None,
        map_percent: None,
        next_item: None,
        drill_count: 0,
        due_count: 0,
        first_session: true,
        week_sessions: 0,
        streak: 0,
        last_session: None,
    }
}

// Finding 1 (IMPORTANT): a resumed topic with no recorded data (opened once,
// never closed, or a closing flush that failed) must not render an empty
// two-line box (just top+bottom border, nothing between). Before the fix
// `render_resume` had no way to signal "nothing to show" — it always
// returned a `Text`, so `solo_box` was called with an empty `rows` and
// printed exactly that empty frame. Fix: `render_resume` (and the
// `render_for_entry` dispatcher) now return `Option<Text>`, `None` when
// every row would drop — callers skip printing entirely, since the
// `resuming: <topic>` notice printed moments earlier already says
// everything true at this point.
#[test]
fn render_resume_no_data_returns_none_not_an_empty_box() {
    let d = empty_resume_data("rust");
    assert!(
        render_resume(&d, 80).is_none(),
        "expected no panel when there's no data to show, got a frame"
    );
}

#[test]
fn render_for_entry_no_data_resume_returns_none() {
    let d = empty_resume_data("rust");
    assert!(render_for_entry(false, &d, 80).is_none());
}

// Regression guard: `had_topic_arg = true` must ALWAYS return `Some`, even
// with no data — the full-mode box always carries the logo, so there's no
// "empty panel" case to skip on this arm. Pinned with EMPTY data on
// purpose: a fully-populated fixture wouldn't catch someone later hoisting
// the `rows.is_empty()` check up into `render_for_entry` itself, which
// would make `usta start <topic>` go silently frameless on a first-ever
// topic — worse than the empty box the no-data fix above just prevented.
#[test]
fn render_for_entry_with_topic_arg_always_renders_even_with_no_data() {
    assert!(render_for_entry(true, &empty_resume_data("rust"), 80).is_some());
}

// Finding 2 (IMPORTANT): row 1's single-line decision is based on `wrap`'s
// collapsed-whitespace width, but the single-line row is built from `mixed`,
// whose spans carry the RAW (uncollapsed) `level` string. A level string
// with doubled internal spaces (extract_level only trims the ends) can land
// in the band `collapsed <= value_w < raw`, where the row is judged to fit
// but actually overflows by the difference. Reproduces the reviewer's
// repro at width 60 with a doubled-space level string.
#[test]
fn render_resume_row1_fit_decision_matches_actually_rendered_width() {
    let mut d = full_resume_data();
    d.last_session = Some("2 days ago".to_string());
    d.level = Some("aaaa  bbbb  cccc  dddd  x".to_string()); // raw 25, collapsed 22
    let t = render_resume(&d, 60).expect("data present, panel must render");
    let lines = plain_lines(&t);
    let w = lines[0].width();
    assert!(
        lines.iter().all(|l| l.width() == w),
        "hizasız satır (row1 fit/render width mismatch): {lines:#?}"
    );
}

// Finding 4: `map_bar` must reserve a full 12/12 bar for 100% only — the
// doc comment reasons carefully about the low end (1% must show >= 1 filled
// cell) but the high end was unguarded, so `round(96 * 12 / 100) == 12`
// renders a visually-full bar next to "96%".
#[test]
fn render_resume_bar_full_only_at_100_percent() {
    assert_eq!(
        map_bar(96).matches('▓').count(),
        11,
        "96% must not render a full bar"
    );
    assert_eq!(
        map_bar(99).matches('▓').count(),
        11,
        "99% must not render a full bar"
    );
    assert_eq!(map_bar(100).matches('▓').count(), 12);
}

// Finding 5a: constraint 4 says the resume panel's edges line up with the
// frame printed above it (identity welcome or full-mode welcome) — nothing
// pinned that cross-frame invariant. Covers the interesting widths: below
// the clamp floor (20), right at/around the floor (60, 61), a mid-range
// value (79, 80), and at/above the ceiling (100, 140) — the clamp is
// `clamp(60, 100)`, so the out-of-range ones matter most.
#[test]
fn all_three_renderers_agree_on_line_width_for_same_input_width() {
    for width in [20u16, 60, 61, 79, 80, 100, 140] {
        let d_full = crate::tui::welcome_data::gather(
            Some(PROFILE),
            Some(PROGRESS),
            Some(CURRICULUM),
            "rust",
            "opus · cli",
            "~/x",
            "2026-08-15",
            None,
        );
        let welcome_lines = plain_lines(&render_welcome(&d_full, width));
        let welcome_box = &welcome_lines[..welcome_lines.len() - 1]; // drop appended help hint
        let welcome_w = welcome_box[0].width();

        let local = vec!["rust".to_string()];
        let identity_t = render_welcome_identity(
            Some("Ada"),
            "opus · cli",
            "~/p",
            &local,
            &[],
            false,
            width,
            0,
            0,
        );
        let identity_lines = plain_lines(&identity_t);
        let identity_box = &identity_lines[..identity_lines.len() - 1];
        let identity_w = identity_box[0].width();

        let resume_t =
            render_resume(&full_resume_data(), width).expect("full resume data always renders");
        let resume_lines = plain_lines(&resume_t);
        let resume_w = resume_lines[0].width();

        assert_eq!(
            welcome_w, identity_w,
            "welcome vs identity width mismatch at width={width}"
        );
        assert_eq!(
            welcome_w, resume_w,
            "welcome vs resume width mismatch at width={width}"
        );
    }
}

// Finding 5b: the existing orange-discipline test only catches
// over-brightening (too many BRAND spans) — a silent drop of the panel's
// DIM labels to plain style would pass it undetected. Pin the DIM modifier
// directly on each label span.
#[test]
fn render_resume_labels_carry_dim_modifier() {
    let d = full_resume_data();
    let t = render_resume(&d, 80).unwrap();
    for label in ["Last session", "Map", "Up next", "Reviews"] {
        let span = t
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content.trim() == label)
            .unwrap_or_else(|| panic!("label '{label}' not found in resume panel"));
        assert!(
            span.style.add_modifier.contains(Modifier::DIM),
            "label '{label}' missing DIM: {:?}",
            span.style
        );
    }
}
