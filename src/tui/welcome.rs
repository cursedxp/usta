//! Welcome box: data gathering (pure) + render. Spec §5.
//! All parsing is best-effort — malformed/missing input skips the field, never panics.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use unicode_width::UnicodeWidthStr;

const ORANGE: Color = Color::Indexed(208);

// 5 rows — a block-letter S needs top, left, middle, right AND bottom bars;
// the old 4-row version had no bottom bar, so the S looked cut in half.
const LOGO: [&str; 5] = [
    "██  ██ ██████ ██████ ██████",
    "██  ██ ██       ██   ██  ██",
    "██  ██ ██████   ██   ██████",
    "██  ██     ██   ██   ██  ██",
    "██████ ██████   ██   ██  ██",
];

/// All data for the welcome box — render draws from this struct, does no IO.
pub struct WelcomeData {
    pub version: &'static str,
    pub name: Option<String>,
    pub model: String,
    pub dir: String,
    pub topic: String,
    pub level: Option<String>,
    pub map_percent: Option<u8>,
    pub next_item: Option<String>,
    pub drill_count: usize,
    pub due_count: usize,
    pub first_session: bool,
}

/// Body from a `## {header}` heading up to the next `## `.
fn section<'a>(md: &'a str, header: &str) -> Option<&'a str> {
    let needle = format!("## {header}");
    let start = md.find(&needle)? + needle.len();
    let rest = &md[start..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    Some(&rest[..end])
}

/// `# Öğrenci Profili — Ada` → `Ada` (after an em-dash or hyphen).
pub fn extract_name(profile: &str) -> Option<String> {
    let h1 = profile.lines().find(|l| l.starts_with("# "))?;
    let name = h1.rsplit(['—', '-']).next()?.trim();
    if name.is_empty() || name.contains("Profil") || name.starts_with('#') { return None; }
    Some(name.to_string())
}

/// First non-empty line of the `## Seviye` section, with the list marker stripped.
pub fn extract_level(progress: &str) -> Option<String> {
    section(progress, "Seviye")?
        .lines()
        .map(|l| l.trim().trim_start_matches(['-', '*', ' ']).trim())
        .find(|l| !l.is_empty())
        .map(String::from)
}

const STATUSES: [&str; 4] = ["görülmedi", "görüldü", "oturdu", "derinleşildi"];

/// Map percentage from the count of status-bearing lines: non-`görülmedi` / total.
pub fn curriculum_percent(curriculum: &str) -> Option<u8> {
    let (mut total, mut seen) = (0u32, 0u32);
    for line in curriculum.lines() {
        if line.contains("görülmedi") { total += 1; }
        else if STATUSES[1..].iter().any(|s| line.contains(s)) { total += 1; seen += 1; }
    }
    if total == 0 { return None; }
    Some(((seen * 100) / total) as u8)
}

/// Text of the first `görülmedi` item — list marker and status suffix stripped.
pub fn next_unseen(curriculum: &str) -> Option<String> {
    let line = curriculum.lines().find(|l| l.contains("görülmedi"))?;
    let text = line.split("görülmedi").next()?
        .trim()
        .trim_start_matches(['-', '*', ' '])
        .trim_end_matches([':', '—', '-', '·', '|', ' ']);
    if text.is_empty() { None } else { Some(text.to_string()) }
}

/// Number of items in the `## Geri çağırma soruları` section.
pub fn drill_count(progress: &str) -> usize {
    section(progress, "Geri çağırma soruları")
        .map(|s| s.lines().filter(|l| l.trim().starts_with('-')).count())
        .unwrap_or(0)
}

/// Count recall questions due today or earlier. A bullet without a
/// `| due: YYYY-MM-DD` tail is legacy format and counts as due (it gets its
/// tail at the next closing flush). ISO date strings compare lexicographically.
pub fn due_count(progress: &str, today: &str) -> usize {
    let Some(s) = section(progress, "Geri çağırma soruları") else { return 0 };
    s.lines()
        .filter(|l| l.trim().starts_with('-'))
        .filter(|l| match l.find("due: ") {
            None => true, // legacy, no schedule tail → due now
            Some(i) => {
                let date: String = l[i + 5..].chars().take(10).collect();
                date.as_str() <= today
            }
        })
        .count()
}

/// Build WelcomeData from file contents — everything is Option, missing = field skipped.
pub fn gather(
    profile: Option<&str>, progress: Option<&str>, curriculum: Option<&str>,
    topic: &str, model: &str, dir: &str, today: &str,
) -> WelcomeData {
    WelcomeData {
        version: env!("CARGO_PKG_VERSION"),
        name: profile.and_then(extract_name),
        model: model.to_string(),
        dir: dir.to_string(),
        topic: topic.to_string(),
        level: progress.and_then(extract_level),
        map_percent: curriculum.and_then(curriculum_percent),
        next_item: curriculum.and_then(next_unseen),
        drill_count: progress.map(drill_count).unwrap_or(0),
        due_count: progress.map(|p| due_count(p, today)).unwrap_or(0),
        first_session: progress.is_none(),
    }
}

/// Truncate to visible width, add `…` if it overflows. Padding calculations
/// also use unicode-width — byte counting misaligns Turkish characters.
pub fn fit(s: &str, max: usize) -> String {
    if s.width() <= max { return s.to_string(); }
    let mut out = String::new();
    let mut w = 0usize;
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > max.saturating_sub(1) { break; }
        out.push(ch);
        w += cw;
    }
    out.push('…');
    out
}

/// Pad to visible width — adds spaces on the right (per unicode-width).
fn pad(s: &str, w: usize) -> String {
    format!("{s}{}", " ".repeat(w.saturating_sub(s.width())))
}

/// Two-column welcome box. Width is `min(width, 100)`; left column is logo +
/// greeting + model + directory, right column is Learning Status (spec §5).
pub fn render_welcome(d: &WelcomeData, width: u16) -> Text<'static> {
    let total = (width as usize).clamp(60, 100);
    let inner = total - 2;                      // borders
    let left_w = 34usize;
    let right_w = inner - left_w - 3;           // " │ " separator

    let greet = match &d.name {
        Some(n) => format!("Welcome back, {n}!"),
        None => "Welcome back!".to_string(),
    };
    let mut left: Vec<(String, bool)> = vec![(String::new(), false)];
    for l in LOGO { left.push((format!("  {l}"), true)); }
    left.push((String::new(), false));
    left.push((format!("  {}", fit(&greet, left_w - 2)), false));
    left.push((format!("  {}", fit(&d.model, left_w - 2)), false));
    left.push((format!("  {}", fit(&d.dir, left_w - 2)), false));

    let mut right: Vec<(String, Style)> = Vec::new();
    if d.first_session {
        right.push(("Learning Status".to_string(), Style::default()));
        right.push((String::new(), Style::default()));
        right.push((fit("First session — let's start with an introduction.", right_w), Style::default()));
    } else {
        right.push(("Learning Status".to_string(), Style::default()));
        let konu = match &d.level {
            Some(l) => format!("Topic: {} · {}", d.topic, l),
            None => format!("Topic: {}", d.topic),
        };
        right.push((fit(&konu, right_w), Style::default()));
        if let Some(p) = d.map_percent { right.push((format!("Map: {p}%"), Style::default())); }
        right.push(("─".repeat(right_w), Style::default()));
        right.push(("Up next".to_string(), Style::default()));
        if let Some(n) = &d.next_item { right.push((fit(n, right_w), Style::default())); }
        if d.due_count > 0 {
            right.push((format!("Reviews due today: {}", d.due_count), Style::default()));
        } else if d.drill_count > 0 {
            right.push(("No reviews due today".to_string(), Style::default()));
        }
    }

    with_help_hint(render_box(d.version, left, right, width))
}

/// Identity mode: NO topic. Left column is logo + greeting + model + directory;
/// right column is "What do you want to learn?" + local topics that can be
/// resumed (or the first-session message). Shown before a topic is chosen
/// (Claude-style: welcome on top, question below). Wired up in run.rs's
/// topic entry (`ask_topic`).
///
/// `local`: topics recorded in this project — if not empty, shows an
/// `Enter → resume <first>` line and a numbered list (≤6). `other`: topics
/// recorded in other projects — informational only, not selectable, summarized
/// in a dim line.
pub fn render_welcome_identity(
    name: Option<&str>,
    model: &str,
    dir: &str,
    local: &[String],
    other: &[String],
    project_known: bool,
    width: u16,
) -> Text<'static> {
    let total = (width as usize).clamp(60, 100);
    let inner = total - 2;
    let left_w = 34usize;
    let right_w = inner - left_w - 3;

    let greet = match name {
        Some(n) => format!("Hello, {n}!"),
        None => "Hello!".to_string(),
    };
    let mut left: Vec<(String, bool)> = vec![(String::new(), false)];
    for l in LOGO { left.push((format!("  {l}"), true)); }
    left.push((String::new(), false));
    left.push((format!("  {}", fit(&greet, left_w - 2)), false));
    left.push((format!("  {}", fit(model, left_w - 2)), false));
    left.push((format!("  {}", fit(dir, left_w - 2)), false));

    // Topics in other projects are informational only — shown dim (DIM).
    let dim = Style::default().add_modifier(Modifier::DIM);
    let mut right: Vec<(String, Style)> = vec![
        ("What do you want to learn?".to_string(), Style::default()),
        (String::new(), Style::default()),
    ];
    if let Some(first) = local.first() {
        right.push((fit(&format!("Enter → resume {first}"), right_w), Style::default()));
        for (i, t) in local.iter().take(6).enumerate() {
            right.push((fit(&format!("{}) {t}", i + 1), right_w), Style::default()));
        }
        right.push((String::new(), Style::default()));
        right.push((fit("Type to start a new topic.", right_w), Style::default()));
        if !other.is_empty() {
            right.push((fit(&format!("In other projects: {}", other.join(", ")), right_w), dim));
        }
    } else {
        // Spec §3: the first-session message is kept EXACTLY as-is when there are no local topics,
        // UNLESS a project is known — then the empty-Enter sentinel can trigger a suggestion.
        let first_line = if project_known {
            "PROJECT.md found — press Enter, Usta suggests where to start."
        } else {
            "First session — type a topic."
        };
        right.push((fit(first_line, right_w), Style::default()));
        // the previous "Registered:" line is REMOVED — replaced by the other-projects info line (if any).
        if !other.is_empty() {
            right.push((String::new(), Style::default()));
            right.push((fit(&format!("In other projects: {}", other.join(", ")), right_w), dim));
        }
    }

    with_help_hint(render_box(env!("CARGO_PKG_VERSION"), left, right, width))
}

/// Append the `/help` discovery hint as a separate dim line after the bordered
/// box — NOT inside the box, so the box's equal-width line logic stays intact.
fn with_help_hint(mut t: Text<'static>) -> Text<'static> {
    let dim = Style::default().add_modifier(Modifier::DIM);
    t.lines.push(Line::from(Span::styled(crate::help::HELP_HINT, dim)));
    t
}

/// Draw the two-column box — border + " │ " separator + equal-width padding.
/// `left`: (text, is-logo). `right`: (text, style) — row 0 is also automatically
/// wrapped in a bold+orange title style (even if the row's own style is empty),
/// other rows are printed with whatever style they carry (e.g. DIM).
fn render_box(version: &str, left: Vec<(String, bool)>, right: Vec<(String, Style)>, width: u16) -> Text<'static> {
    let total = (width as usize).clamp(60, 100);
    let inner = total - 2;                      // borders
    let left_w = 34usize;
    let right_w = inner - left_w - 3;           // " │ " separator

    let rows = left.len().max(right.len());
    let title = format!(" Usta v{version} ");
    // NOTE: dashes = inner - (4 + title_width) should hold — the "╭─── " prefix is
    // 5 chars, the closing "╮" is 1 char, 6 total fixed; since inner = total-2,
    // 6-2=4 remains. The "5 +" formula from the briefing left the line 1 char
    // short (breaking the equal-width test).
    let top = format!("╭─── {}{}╮", title.trim(), "─".repeat(inner.saturating_sub(4 + title.trim().width())));
    let bottom = format!("╰{}╯", "─".repeat(inner));

    let mut lines: Vec<Line> = vec![Line::from(top)];
    for i in 0..rows {
        let (ltxt, is_logo) = left.get(i).cloned().unwrap_or_default();
        let (rtxt, rtxt_style) = right.get(i).cloned().unwrap_or_default();
        let lspan = Span::styled(
            pad(&ltxt, left_w),
            if is_logo { Style::default().fg(ORANGE) } else { Style::default() },
        );
        let rstyle = if i == 0 && !rtxt.is_empty() {
            Style::default().add_modifier(Modifier::BOLD).fg(ORANGE)
        } else { rtxt_style };
        lines.push(Line::from(vec![
            Span::raw("│"),
            lspan,
            Span::raw(" │ "),
            Span::styled(pad(&rtxt, right_w), rstyle),
            Span::raw("│"),
        ]));
    }
    lines.push(Line::from(bottom));
    Text::from(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Text;

    #[test]
    fn version_aligned_with_spec() {
        assert_eq!(env!("CARGO_PKG_VERSION"), "0.13.0");
    }

    fn plain_lines(t: &Text) -> Vec<String> {
        t.lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect()
    }

    const PROFILE: &str = "# Öğrenci Profili — Ada\n\n## Kim\n- test";
    const PROGRESS: &str = "# rust — İlerleme\n## Seviye\n- Orta: ownership oturdu\n## Geri çağırma soruları\n- Soru 1? — cevap\n- Soru 2? — cevap\n- Soru 3? — cevap\n";
    const CURRICULUM: &str = "# rust haritası\n- Ownership: oturdu\n- Borrowing: görüldü\n- Lifetimes: görülmedi\n- Traits: görülmedi\n";

    #[test]
    fn extract_name_reads_h1_after_dash() {
        assert_eq!(extract_name(PROFILE), Some("Ada".to_string()));
        assert_eq!(extract_name("# Başlıksız"), None);
        assert_eq!(extract_name(""), None);
    }

    #[test]
    fn extract_level_reads_first_line_of_section() {
        assert_eq!(extract_level(PROGRESS), Some("Orta: ownership oturdu".to_string()));
        assert_eq!(extract_level("# boş"), None);
    }

    #[test]
    fn curriculum_percent_counts_non_unseen() {
        // 4 items have a status, 2 are `görülmedi` → 50%
        assert_eq!(curriculum_percent(CURRICULUM), Some(50));
        assert_eq!(curriculum_percent("# durum yok"), None);
    }

    #[test]
    fn next_unseen_returns_first_unseen_item_text() {
        assert_eq!(next_unseen(CURRICULUM), Some("Lifetimes".to_string()));
        assert_eq!(next_unseen("- Hepsi: oturdu"), None);
    }

    #[test]
    fn drill_count_counts_section_bullets() {
        assert_eq!(drill_count(PROGRESS), 3);
        assert_eq!(drill_count("# soru yok"), 0);
    }

    #[test]
    fn due_count_counts_due_and_untagged_skips_future() {
        let p = "\
# rust — İlerleme

## Geri çağırma soruları
- Borrow checker ne yapar? — sahipliği derlemede doğrular | due: 2026-08-14 | ivl: 3
- Trait nedir? — davranış sözleşmesi | due: 2026-08-15 | ivl: 1
- Lifetime nedir? — referans ömrü | due: 2026-09-01 | ivl: 35
- Eski format soru — cevap

## Hata günlüğü
- typo | 1 | due: 2026-08-01 gibi görünen ama başka bölümde
";
        // past + today + untagged = 3; future (09-01) and other-section lines don't count
        assert_eq!(due_count(p, "2026-08-15"), 3);
        assert_eq!(due_count(p, "2026-08-13"), 1); // only untagged counts as due
        assert_eq!(due_count("# bos", "2026-08-15"), 0);
    }

    #[test]
    fn gather_full_and_first_session() {
        let d = gather(Some(PROFILE), Some(PROGRESS), Some(CURRICULUM), "rust", "opus · cli", "~/x", "2026-08-15");
        assert!(!d.first_session);
        assert_eq!(d.name.as_deref(), Some("Ada"));
        assert_eq!(d.map_percent, Some(50));
        let d2 = gather(None, None, None, "gtm", "opus · cli", "~/x", "2026-08-15");
        assert!(d2.first_session);
        assert_eq!(d2.drill_count, 0);
    }

    #[test]
    fn welcome_shows_due_line_three_states() {
        // state 1: due questions exist → "Reviews due today: N"
        let p_due = "## Geri çağırma soruları\n- q — a | due: 2026-01-01 | ivl: 1\n";
        let d = gather(None, Some(p_due), None, "rust", "opus · cli", "~/x", "2026-08-15");
        let joined = plain_lines(&render_welcome(&d, 80)).join("\n");
        assert!(joined.contains("Reviews due today: 1"));

        // state 2: questions exist, none due → "No reviews due today"
        let p_future = "## Geri çağırma soruları\n- q — a | due: 2099-01-01 | ivl: 90\n";
        let d = gather(None, Some(p_future), None, "rust", "opus · cli", "~/x", "2026-08-15");
        let joined = plain_lines(&render_welcome(&d, 80)).join("\n");
        assert!(joined.contains("No reviews due today"));
        assert!(!joined.contains("Reviews due today:"));

        // state 3: no questions at all → neither line
        let d = gather(None, Some("# bos"), None, "rust", "opus · cli", "~/x", "2026-08-15");
        let joined = plain_lines(&render_welcome(&d, 80)).join("\n");
        assert!(!joined.contains("Reviews due"));
        assert!(!joined.contains("No reviews due"));
    }

    #[test]
    fn render_welcome_lines_have_equal_display_width() {
        use unicode_width::UnicodeWidthStr;
        let d = gather(Some(PROFILE), Some(PROGRESS), Some(CURRICULUM), "rust", "opus · cli", "~/proje", "2026-08-15");
        let t = render_welcome(&d, 80);
        let lines = plain_lines(&t);
        assert!(lines.len() >= 8);
        // Last line is the appended help hint — NOT part of the bordered box,
        // so it's excluded from the equal-width check (spec: separate Line, box intact).
        let box_lines = &lines[..lines.len() - 1];
        let w = box_lines[0].width();
        assert!(box_lines.iter().all(|l| l.width() == w), "hizasız satır: {lines:#?}");
        assert!(box_lines[0].starts_with('╭') && box_lines.last().unwrap().starts_with('╰'));
        assert_eq!(lines.last().unwrap(), crate::help::HELP_HINT);
    }

    #[test]
    fn render_welcome_first_session_shows_intro_message() {
        let d = gather(None, None, None, "gtm", "opus · cli", "~/p", "2026-08-15");
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
    fn render_identity_with_topics_lists_them_and_equal_width() {
        use unicode_width::UnicodeWidthStr;
        let local = vec!["rust".to_string(), "gtm".to_string()];
        let t = render_welcome_identity(Some("Ada"), "opus · cli", "~/p", &local, &[], false, 80);
        let lines = plain_lines(&t);
        // Last line is the appended help hint — NOT part of the bordered box.
        let box_lines = &lines[..lines.len() - 1];
        let w = box_lines[0].width();
        assert!(box_lines.iter().all(|l| l.width() == w), "hizasız: {lines:#?}");
        let joined = lines.join("\n");
        assert!(joined.contains("What do you want to learn?"));
        assert!(joined.contains("rust"));
        assert!(joined.contains("Hello, Ada!"));
        assert!(box_lines[0].starts_with('╭') && box_lines.last().unwrap().starts_with('╰'));
        assert_eq!(lines.last().unwrap(), crate::help::HELP_HINT);
    }

    #[test]
    fn render_identity_no_topics_shows_first_session_and_no_name() {
        let t = render_welcome_identity(None, "opus · cli", "~/p", &[], &[], false, 80);
        let joined = plain_lines(&t).join("\n");
        assert!(joined.contains("What do you want to learn?"));
        assert!(joined.contains("Hello!"));       // no name → generic
        assert!(!joined.contains("Hello,"));      // no "Hello, X!" form
        assert!(!joined.contains("Enter →"));       // no topic → no continuation line
    }

    #[test]
    fn identity_welcome_lists_local_topics_with_enter_hint() {
        let local = vec!["brainstorm-ilk-adim".to_string(), "linux-guvenlik".to_string()];
        let other = vec!["rust".to_string()];
        let t = render_welcome_identity(Some("Anil"), "opus · cli", "~/x", &local, &other, false, 80);
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
        assert!(box_lines.iter().all(|l| l.width() == w), "hizasız: {lines:#?}");
        assert_eq!(lines.last().unwrap(), crate::help::HELP_HINT);
    }

    #[test]
    fn identity_welcome_without_local_topics_keeps_first_run_look() {
        let t = render_welcome_identity(None, "opus · cli", "~/x", &[], &[], false, 80);
        let joined = plain_lines(&t).join("\n");
        assert!(joined.contains("What do you want to learn"));
        assert!(joined.contains("First session"));
        assert!(!joined.contains("Enter →"));
    }

    #[test]
    fn first_session_hint_becomes_suggest_hint_when_project_known() {
        // Call render_welcome_identity twice with empty `local`, flipping only
        // project_known.
        let not_known = render_welcome_identity(None, "opus · cli", "~/p", &[], &[], false, 80);
        let known = render_welcome_identity(None, "opus · cli", "~/p", &[], &[], true, 80);
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
        let t = render_welcome_identity(Some("Ada"), "opus · cli", "~/p", &local, &other, false, 80);
        let span = t
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content.contains("In other projects"))
            .expect("In other projects satırı bulunamalı");
        assert!(span.style.add_modifier.contains(Modifier::DIM), "stil DIM içermiyor: {:?}", span.style);
    }
}
