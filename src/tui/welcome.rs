//! Welcome box: data gathering (pure) + render. Spec §5.
//! All parsing is best-effort — malformed/missing input skips the field, never panics.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use unicode_width::UnicodeWidthStr;

use crate::tokens;
use crate::tui::theme;

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
    pub week_sessions: u32,
    pub streak: u32,
    pub last_session: Option<String>,
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

/// First non-empty line of the `tokens::S_LEVEL` section, with the list marker stripped.
pub fn extract_level(progress: &str) -> Option<String> {
    section(progress, tokens::S_LEVEL)?
        .lines()
        .map(|l| l.trim().trim_start_matches(['-', '*', ' ']).trim())
        .find(|l| !l.is_empty())
        .map(String::from)
}

/// Map percentage from the count of status-bearing lines: non-`tokens::STATE_NOT_SEEN` / total.
pub fn curriculum_percent(curriculum: &str) -> Option<u8> {
    let (mut total, mut seen) = (0u32, 0u32);
    for line in curriculum.lines() {
        match tokens::map_state_of(line) {
            Some(s) if s == tokens::STATE_NOT_SEEN => total += 1,
            Some(_) => { total += 1; seen += 1; }
            None => {}
        }
    }
    if total == 0 { return None; }
    Some(((seen * 100) / total) as u8)
}

/// Text of the first `tokens::STATE_NOT_SEEN` item — list marker and status suffix stripped.
pub fn next_unseen(curriculum: &str) -> Option<String> {
    let line = curriculum.lines().find(|l| tokens::map_state_of(l) == Some(tokens::STATE_NOT_SEEN))?;
    let head = line.trim().split(" | ").next().unwrap_or(line.trim());
    let text = head.rsplit_once(':')?.0
        .trim()
        .trim_start_matches(['-', '*', ' '])
        .trim_end_matches([':', '—', '-', '·', '|', ' ']);
    if text.is_empty() { None } else { Some(text.to_string()) }
}

/// Number of items in the `tokens::S_RECALL` section.
pub fn drill_count(progress: &str) -> usize {
    section(progress, tokens::S_RECALL)
        .map(|s| s.lines().filter(|l| l.trim().starts_with('-')).count())
        .unwrap_or(0)
}

/// All due bullets from the `tokens::S_RECALL` section, paired with a
/// sort key (the `due:` date, or `""` for a legacy/untagged bullet so it sorts
/// first — a bullet with no tail is treated as due now). No cap here — callers
/// that need the count (`due_count`) or a capped preview (`due_questions`)
/// derive from this single scan. Sort is stable: equal dates keep file order.
fn due_items(progress: &str, today: &str) -> Vec<(String, String)> {
    let Some(s) = section(progress, tokens::S_RECALL) else { return Vec::new() };
    let mut items: Vec<(String, String)> = Vec::new();
    for l in s.lines().map(str::trim).filter(|l| l.starts_with('-')) {
        match l.find("due: ") {
            None => items.push((String::new(), l.to_string())), // legacy → due now
            Some(i) => {
                let date: String = l[i + 5..].chars().take(10).collect();
                if date.as_str() <= today {
                    items.push((date, l.to_string()));
                }
            }
        }
    }
    items.sort_by(|a, b| a.0.cmp(&b.0));
    items
}

/// Count recall questions due today or earlier. A bullet without a
/// `| due: YYYY-MM-DD` tail is legacy format and counts as due (it gets its
/// tail at the next closing flush). ISO date strings compare lexicographically.
/// NOT capped — the welcome box shows the real count ("Reviews due today: N").
pub fn due_count(progress: &str, today: &str) -> usize {
    due_items(progress, today).len()
}

/// The due bullets themselves (see `due_count`), oldest due first, capped at 3 —
/// the shell-selected drill list handed to `progress::opening_prompt` so the
/// model no longer has to filter/sort `due:` dates itself.
pub fn due_questions(progress: &str, today: &str) -> Vec<String> {
    due_items(progress, today).into_iter().take(3).map(|(_, line)| line).collect()
}

/// Relative phrasing for the newest history entry of `topic`, EXCLUDING the
/// session being opened right now (its line is appended at close, not at open).
/// `0` → `today`, `1` → `yesterday`, `n` → `n days ago`. A future-dated entry
/// (clock skew) collapses to `today` rather than printing a negative count.
/// ADHD-safe: the phrasing is a neutral timestamp at every distance — no
/// streak-zero, no "it has been a while" (SPEC §"ADHD-safe rules").
pub fn last_session_ago(entries: &[crate::history::Entry], topic: &str, today: &str) -> Option<String> {
    let today = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d").ok()?;
    let newest = entries
        .iter()
        .filter(|e| e.topic == topic)
        .filter_map(|e| chrono::NaiveDate::parse_from_str(&e.date, "%Y-%m-%d").ok())
        .max()?;
    let days = (today - newest).num_days();
    Some(if days <= 0 {
        "today".to_string()
    } else if days == 1 {
        "yesterday".to_string()
    } else {
        format!("{days} days ago")
    })
}

/// Build WelcomeData from file contents — everything is Option, missing = field skipped.
/// `history`: raw `learner/history.md` content (global, not topic-scoped) — `None`
/// means no history file exists yet, which renders as 0 sessions / 0 streak
/// (never a "This week" line — see `week_line`).
#[allow(clippy::too_many_arguments)]
pub fn gather(
    profile: Option<&str>, progress: Option<&str>, curriculum: Option<&str>,
    topic: &str, model: &str, dir: &str, today: &str, history: Option<&str>,
) -> WelcomeData {
    let (week_sessions, streak, last_session) = match history {
        Some(h) => {
            let es = crate::history::entries(h);
            (
                crate::history::week_summary(&es, today).sessions,
                crate::history::current_streak(&es, today),
                last_session_ago(&es, topic, today),
            )
        }
        None => (0, 0, None),
    };
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
        week_sessions,
        streak,
        last_session,
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

/// Greedy word-boundary wrap to visible width — unicode-width aware (same
/// concern as `fit`'s comment: byte counting misaligns Turkish characters).
/// Words are packed onto a line while they fit; a word wider than `max` on
/// its own falls back to char-level splitting so no returned line ever
/// exceeds `max` (guarantees termination — every char/word always advances
/// the cursor). Empty input returns no lines (callers push one right-column
/// row per returned line, so "no lines" naturally means "no rows added").
pub fn wrap(s: &str, max: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_w = 0usize;

    for word in s.split_whitespace() {
        let word_w = word.width();
        if word_w > max {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                current_w = 0;
            }
            for ch in word.chars() {
                let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if current_w + cw > max && !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                    current_w = 0;
                }
                current.push(ch);
                current_w += cw;
            }
            continue;
        }
        let extra = if current.is_empty() { word_w } else { current_w + 1 + word_w };
        if extra > max && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
            current_w = word_w;
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
            current_w = extra;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// `This week: N session(s) · streak M day(s)` — `None` when there were no
/// sessions this week. A streak of 0 is NEVER rendered (ADHD-safe: a broken
/// streak reads as demotivating noise, not useful information) — when
/// `sessions > 0` but `streak == 0` the line drops the " · streak" clause
/// entirely rather than showing "streak 0".
fn week_line(sessions: u32, streak: u32) -> Option<String> {
    if sessions == 0 {
        return None;
    }
    if streak > 0 {
        Some(format!("This week: {sessions} session(s) · streak {streak} day(s)"))
    } else {
        Some(format!("This week: {sessions} session(s)"))
    }
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
        for l in wrap("First session — let's start with an introduction.", right_w) {
            right.push((l, Style::default()));
        }
    } else {
        right.push(("Learning Status".to_string(), Style::default()));
        let konu = match &d.level {
            Some(l) => format!("Topic: {} · {}", d.topic, l),
            None => format!("Topic: {}", d.topic),
        };
        for l in wrap(&konu, right_w) {
            right.push((l, Style::default()));
        }
        if let Some(p) = d.map_percent { right.push((format!("Map: {p}%"), Style::default())); }
        right.push(("─".repeat(right_w), Style::default()));
        right.push(("Up next".to_string(), Style::default()));
        if let Some(n) = &d.next_item {
            for l in wrap(n, right_w) {
                right.push((l, Style::default()));
            }
        }
        if d.due_count > 0 {
            right.push((format!("Reviews due today: {}", d.due_count), Style::default()));
        } else if d.drill_count > 0 {
            right.push(("No reviews due today".to_string(), Style::default()));
        }
    }
    if let Some(line) = week_line(d.week_sessions, d.streak) {
        for l in wrap(&line, right_w) {
            right.push((l, Style::default()));
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
#[allow(clippy::too_many_arguments)]
pub fn render_welcome_identity(
    name: Option<&str>,
    model: &str,
    dir: &str,
    local: &[String],
    other: &[String],
    project_known: bool,
    width: u16,
    week_sessions: u32,
    streak: u32,
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
        for l in wrap(&format!("Enter → resume {first}"), right_w) {
            right.push((l, Style::default()));
        }
        // List items stay on `fit`, not `wrap` — wrapping a topic name would
        // continue onto an unnumbered line, breaking the numbered list's
        // visual alignment (see render_box's per-row rendering).
        for (i, t) in local.iter().take(6).enumerate() {
            right.push((fit(&format!("{}) {t}", i + 1), right_w), Style::default()));
        }
        right.push((String::new(), Style::default()));
        for l in wrap("Type to start a new topic.", right_w) {
            right.push((l, Style::default()));
        }
        if !other.is_empty() {
            for l in wrap(&format!("In other projects: {}", other.join(", ")), right_w) {
                right.push((l, dim));
            }
        }
    } else {
        // Spec §3: the first-session message is kept EXACTLY as-is when there are no local topics,
        // UNLESS a project is known — then the empty-Enter sentinel can trigger a suggestion.
        let first_line = if project_known {
            "PROJECT.md found — press Enter, Usta suggests where to start."
        } else {
            "First session — type a topic."
        };
        for l in wrap(first_line, right_w) {
            right.push((l, Style::default()));
        }
        // the previous "Registered:" line is REMOVED — replaced by the other-projects info line (if any).
        if !other.is_empty() {
            right.push((String::new(), Style::default()));
            for l in wrap(&format!("In other projects: {}", other.join(", ")), right_w) {
                right.push((l, dim));
            }
        }
    }
    if let Some(line) = week_line(week_sessions, streak) {
        for l in wrap(&line, right_w) {
            right.push((l, Style::default()));
        }
    }

    with_help_hint(render_box(env!("CARGO_PKG_VERSION"), left, right, width))
}

/// 12-cell progress bar: `▓` filled, `░` empty. `pct` is clamped to 0..=100.
/// Filled count rounds to nearest cell (not floor/ceil) so 25% reads as a
/// visually-proportional 3/12, but a non-zero percent always shows at least
/// one filled cell — otherwise 1% would render as an all-empty bar, which
/// reads as "0% / not started" and is a lie.
fn map_bar(pct: u8) -> String {
    let pct = pct.min(100);
    let filled = ((pct as f32) * 12.0 / 100.0).round() as usize;
    let filled = filled.min(12);
    let filled = if pct > 0 && filled == 0 { 1 } else { filled };
    format!("{}{}", "▓".repeat(filled), "░".repeat(12 - filled))
}

/// Draw a single-column bordered panel. `title` goes in the top border in
/// brand+bold; each row is a span list padded to the inner width. Same width
/// clamp as `render_box`, so the panel's edges line up with the welcome box
/// printed above it.
fn solo_box(title: &str, rows: Vec<Vec<Span<'static>>>, width: u16) -> Text<'static> {
    let total = (width as usize).clamp(60, 100);
    let inner = total - 2; // borders

    // Same fixed-offset formula as render_box's top border (see that
    // function's comment): "╭─── " (5) + "╮" (1) = 6 fixed chars, and
    // inner already excludes the 2 side borders, so 6-2=4 remains.
    let top = Line::from(vec![
        Span::raw("╭─── "),
        Span::styled(title.to_string(), theme::brand().add_modifier(Modifier::BOLD)),
        Span::raw("─".repeat(inner.saturating_sub(4 + title.width()))),
        Span::raw("╮"),
    ]);

    let mut lines: Vec<Line> = vec![top];
    for row in rows {
        let row_w: usize = row.iter().map(|s| s.content.width()).sum();
        let mut spans = vec![Span::raw("│")];
        spans.extend(row);
        spans.push(Span::raw(" ".repeat(inner.saturating_sub(row_w))));
        spans.push(Span::raw("│"));
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(format!("╰{}╯", "─".repeat(inner))));
    Text::from(lines)
}

/// Resume mode: printed after the identity welcome when a saved topic is
/// picked. Deliberately carries NO identity — no logo, greeting, model, cwd
/// or week/streak line; all of those are already on screen in the identity
/// box above, and repeating them was the bug this panel replaces. Its job is
/// continuity: what you are picking up, when you were last here, how far
/// along the map you are. Design: Claude Design f8cc2dc7 page 06, variant A.
// Not wired into run.rs yet — Task 3 branches `resumed` to call this instead
// of `render_welcome`. Until then it's only reachable from tests.
#[allow(dead_code)]
pub fn render_resume(d: &WelcomeData, width: u16) -> Text<'static> {
    let total = (width as usize).clamp(60, 100);
    let inner = total - 2;
    // Label column is "  " (2) + pad(label,12) (12) + " " (1) = 15 chars;
    // the value — and any wrapped continuation line — gets what's left.
    let value_w = inner.saturating_sub(15).max(1);

    let dim = Style::default().add_modifier(Modifier::DIM);
    let plain = Style::default();

    let mut rows: Vec<Vec<Span<'static>>> = Vec::new();

    // Row 1: "Last session {rel}" (+ " · Level {level}" if both present), or
    // just "Level {level}" if there's no last-session data. Dropped only when
    // neither is present.
    if d.last_session.is_some() || d.level.is_some() {
        let (label, value): (&str, Vec<Span<'static>>) = match (&d.last_session, &d.level) {
            (Some(rel), Some(level)) => (
                "Last session",
                vec![
                    Span::styled(rel.clone(), plain),
                    Span::styled(" · ".to_string(), dim),
                    Span::styled("Level ".to_string(), dim),
                    Span::styled(level.clone(), plain),
                ],
            ),
            (Some(rel), None) => ("Last session", vec![Span::styled(rel.clone(), plain)]),
            (None, Some(level)) => ("Level", vec![Span::styled(level.clone(), plain)]),
            (None, None) => unreachable!("guarded by the outer if"),
        };
        let mut row = vec![Span::raw("  "), Span::styled(pad(label, 12), dim), Span::raw(" ")];
        row.extend(value);
        rows.push(row);
    }

    // Row 2: "Map          {bar} {p}%" — dropped when there's no curriculum data.
    if let Some(p) = d.map_percent {
        rows.push(vec![
            Span::raw("  "),
            Span::styled(pad("Map", 12), dim),
            Span::raw(" "),
            Span::styled(map_bar(p), dim),
            Span::raw(" "),
            Span::styled(format!("{p}%"), dim),
        ]);
    }

    // Row 3: blank separator — only when there's a row 4 or 5 to separate from
    // rows 1/2. No separator when the panel would otherwise end right after them.
    if d.next_item.is_some() || d.due_count > 0 {
        rows.push(Vec::new());
    }

    // Row 4: "Up next      {next_item}", wrapped (never truncated/ellipsized);
    // continuation lines align under the value column (15-space prefix).
    if let Some(next) = &d.next_item {
        for (i, line) in wrap(next, value_w).into_iter().enumerate() {
            if i == 0 {
                rows.push(vec![
                    Span::raw("  "),
                    Span::styled(pad("Up next", 12), dim),
                    Span::raw(" "),
                    Span::styled(line, plain),
                ]);
            } else {
                rows.push(vec![Span::raw(" ".repeat(15)), Span::styled(line, plain)]);
            }
        }
    }

    // Row 5: "Reviews      {n} due today" — dropped when nothing is due. The
    // count is the panel's one other orange element (with the title).
    if d.due_count > 0 {
        rows.push(vec![
            Span::raw("  "),
            Span::styled(pad("Reviews", 12), dim),
            Span::raw(" "),
            Span::styled(d.due_count.to_string(), theme::brand()),
            Span::styled(" due today".to_string(), plain),
        ]);
    }

    let title = format!("Continuing · {} ", d.topic);
    solo_box(&title, rows, width)
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
            if is_logo { theme::brand() } else { Style::default() },
        );
        let rstyle = if i == 0 && !rtxt.is_empty() {
            theme::brand().add_modifier(Modifier::BOLD)
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
        assert_eq!(env!("CARGO_PKG_VERSION"), "0.20.4");
    }

    fn plain_lines(t: &Text) -> Vec<String> {
        t.lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect()
    }

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
        assert_eq!(extract_level(PROGRESS), Some("Orta: ownership settled".to_string()));
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
        assert_eq!(due_count(p, "2026-08-15"), due_questions(p, "2026-08-15").len());
    }

    #[test]
    fn gather_full_and_first_session() {
        let d = gather(Some(PROFILE), Some(PROGRESS), Some(CURRICULUM), "rust", "opus · cli", "~/x", "2026-08-15", None);
        assert!(!d.first_session);
        assert_eq!(d.name.as_deref(), Some("Ada"));
        assert_eq!(d.map_percent, Some(50));
        let d2 = gather(None, None, None, "gtm", "opus · cli", "~/x", "2026-08-15", None);
        assert!(d2.first_session);
        assert_eq!(d2.drill_count, 0);
    }

    #[test]
    fn welcome_shows_due_line_three_states() {
        // state 1: due questions exist → "Reviews due today: N"
        let p_due = "## Recall questions\n- q — a | due: 2026-01-01 | ivl: 1\n";
        let d = gather(None, Some(p_due), None, "rust", "opus · cli", "~/x", "2026-08-15", None);
        let joined = plain_lines(&render_welcome(&d, 80)).join("\n");
        assert!(joined.contains("Reviews due today: 1"));

        // state 2: questions exist, none due → "No reviews due today"
        let p_future = "## Recall questions\n- q — a | due: 2099-01-01 | ivl: 90\n";
        let d = gather(None, Some(p_future), None, "rust", "opus · cli", "~/x", "2026-08-15", None);
        let joined = plain_lines(&render_welcome(&d, 80)).join("\n");
        assert!(joined.contains("No reviews due today"));
        assert!(!joined.contains("Reviews due today:"));

        // state 3: no questions at all → neither line
        let d = gather(None, Some("# bos"), None, "rust", "opus · cli", "~/x", "2026-08-15", None);
        let joined = plain_lines(&render_welcome(&d, 80)).join("\n");
        assert!(!joined.contains("Reviews due"));
        assert!(!joined.contains("No reviews due"));
    }

    #[test]
    fn render_welcome_lines_have_equal_display_width() {
        use unicode_width::UnicodeWidthStr;
        let d = gather(Some(PROFILE), Some(PROGRESS), Some(CURRICULUM), "rust", "opus · cli", "~/proje", "2026-08-15", None);
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
        let d = gather(None, None, None, "gtm", "opus · cli", "~/p", "2026-08-15", None);
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
        assert!(lines.iter().all(|l| l.width() <= 10), "line exceeds max: {lines:?}");
        // every original word survives, in order, none lost/duplicated
        let rejoined = lines.join(" ");
        assert_eq!(rejoined, s);
    }

    #[test]
    fn wrap_oversized_single_word_falls_back_to_char_split() {
        let s = "supercalifragilisticexpialidocious";
        let lines = wrap(s, 5);
        assert!(lines.len() > 1, "expected multiple lines: {lines:?}");
        assert!(lines.iter().all(|l| l.width() <= 5), "line exceeds max: {lines:?}");
        assert_eq!(lines.concat(), s);
    }

    #[test]
    fn wrap_unicode_width_aware_not_byte_counting() {
        // Turkish chars: byte-counting would misalign vs display width — reuse
        // fit's documented concern for wrap.
        let s = "çğşöü ıİĞÜ test kelimeler burada";
        let lines = wrap(s, 8);
        assert!(lines.iter().all(|l| l.width() <= 8), "line exceeds max: {lines:?}");
    }

    #[test]
    fn render_welcome_long_next_item_wraps_full_text_no_ellipsis() {
        let long_item = "Async trait objects and pinning semantics in tokio task spawning";
        let curriculum = format!("# rust haritası\n- {long_item}: not seen\n");
        let d = gather(Some(PROFILE), Some(PROGRESS), Some(&curriculum), "rust", "opus · cli", "~/x", "2026-08-15", None);
        assert_eq!(d.next_item.as_deref(), Some(long_item));
        let t = render_welcome(&d, 80);
        let lines = plain_lines(&t);
        let joined = lines.join(" ");
        assert!(!joined.contains('…'), "next_item was truncated: {lines:#?}");
        for word in long_item.split(' ') {
            assert!(joined.contains(word), "missing word '{word}' from wrapped next_item: {lines:#?}");
        }
        // last word of the long sentence must show up somewhere — proves the
        // tail wasn't dropped, not just the head before an ellipsis.
        assert!(joined.contains("spawning"));
    }

    #[test]
    fn render_identity_with_topics_lists_them_and_equal_width() {
        use unicode_width::UnicodeWidthStr;
        let local = vec!["rust".to_string(), "gtm".to_string()];
        let t = render_welcome_identity(Some("Ada"), "opus · cli", "~/p", &local, &[], false, 80, 0, 0);
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
        let t = render_welcome_identity(None, "opus · cli", "~/p", &[], &[], false, 80, 0, 0);
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
        let t = render_welcome_identity(Some("Anil"), "opus · cli", "~/x", &local, &other, false, 80, 0, 0);
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
        let t = render_welcome_identity(Some("Ada"), "opus · cli", "~/p", &local, &other, false, 80, 0, 0);
        let span = t
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content.contains("In other projects"))
            .expect("In other projects satırı bulunamalı");
        assert!(span.style.add_modifier.contains(Modifier::DIM), "stil DIM içermiyor: {:?}", span.style);
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
        let ident = render_welcome_identity(Some("Ada"), "opus · cli", "~/p", &local, &[], false, 80, 0, 0);
        assert!(orange_element_count(&ident) <= 2, "identity orange > 2: {ident:#?}");

        // Full-mode welcome at rest: logo block + "Learning Status" title = 2.
        let d = gather(Some(PROFILE), Some(PROGRESS), Some(CURRICULUM), "rust", "opus · cli", "~/p", "2026-08-15", None);
        let full = render_welcome(&d, 80);
        assert!(orange_element_count(&full) <= 2, "full-mode orange > 2: {full:#?}");
    }

    #[test]
    fn welcome_shows_week_line() {
        // state 1: sessions this week + an unbroken streak → full line.
        let h = "# Session History\n- 2026-08-14 | rust | map 40% | settled 4\n- 2026-08-15 | rust | map 55% | settled 7\n";
        let d = gather(None, None, None, "rust", "opus · cli", "~/x", "2026-08-15", Some(h));
        assert_eq!(d.week_sessions, 2);
        assert_eq!(d.streak, 2);
        let joined = plain_lines(&render_welcome(&d, 80)).join("\n");
        assert!(joined.contains("This week: 2 session(s) · streak 2 day(s)"));

        // state 2: sessions this week but streak == 0 (entries are 2+ days before
        // `today`, so `current_streak` sees a broken run — an ADHD-unsafe "streak 0"
        // must never be rendered, only the sessions count survives).
        let h0 = "# Session History\n- 2026-08-10 | rust | map 40% | settled 4\n";
        let d0 = gather(None, None, None, "rust", "opus · cli", "~/x", "2026-08-15", Some(h0));
        assert_eq!(d0.week_sessions, 1);
        assert_eq!(d0.streak, 0);
        let joined0 = plain_lines(&render_welcome(&d0, 80)).join("\n");
        assert!(joined0.contains("This week: 1 session(s)"));
        assert!(!joined0.contains("· streak"));
        assert!(!joined0.contains("streak 0"));

        // state 3: no history at all → no "This week" line.
        let dn = gather(None, None, None, "rust", "opus · cli", "~/x", "2026-08-15", None);
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

    fn mk_entry(date: &str, topic: &str) -> crate::history::Entry {
        crate::history::Entry { date: date.to_string(), topic: topic.to_string(), map: None, settled: None }
    }

    #[test]
    fn last_session_ago_today_yesterday_and_days() {
        let today_e = vec![mk_entry("2026-08-15", "rust")];
        assert_eq!(last_session_ago(&today_e, "rust", "2026-08-15"), Some("today".to_string()));

        let yesterday_e = vec![mk_entry("2026-08-14", "rust")];
        assert_eq!(last_session_ago(&yesterday_e, "rust", "2026-08-15"), Some("yesterday".to_string()));

        let days_e = vec![mk_entry("2026-08-10", "rust")];
        assert_eq!(last_session_ago(&days_e, "rust", "2026-08-15"), Some("5 days ago".to_string()));
    }

    #[test]
    fn last_session_ago_picks_newest_not_last_line() {
        // older date written AFTER the newer one in file/vec order — max must
        // still win, not the last element.
        let es = vec![mk_entry("2026-08-14", "rust"), mk_entry("2026-08-10", "rust")];
        assert_eq!(last_session_ago(&es, "rust", "2026-08-15"), Some("yesterday".to_string()));
    }

    #[test]
    fn last_session_ago_filters_by_topic() {
        let es = vec![mk_entry("2026-08-15", "gtm"), mk_entry("2026-08-10", "rust")];
        assert_eq!(last_session_ago(&es, "rust", "2026-08-15"), Some("5 days ago".to_string()));
    }

    #[test]
    fn last_session_ago_none_without_entry() {
        let es = vec![mk_entry("2026-08-15", "gtm")];
        assert_eq!(last_session_ago(&es, "rust", "2026-08-15"), None);
    }

    #[test]
    fn last_session_ago_future_date_is_today() {
        let es = vec![mk_entry("2026-08-16", "rust")];
        assert_eq!(last_session_ago(&es, "rust", "2026-08-15"), Some("today".to_string()));
    }

    #[test]
    fn gather_fills_last_session() {
        let h = "# Session History\n- 2026-08-14 | rust | map 40% | settled 4\n";
        let d = gather(None, None, None, "rust", "opus · cli", "~/x", "2026-08-15", Some(h));
        assert_eq!(d.last_session.as_deref(), Some("yesterday"));

        let d2 = gather(None, None, None, "rust", "opus · cli", "~/x", "2026-08-15", None);
        assert_eq!(d2.last_session, None);
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
        let t = render_resume(&d, 80);
        let lines = plain_lines(&t);
        assert!(lines.len() >= 3);
        let w = lines[0].width();
        assert!(lines.iter().all(|l| l.width() == w), "hizasız satır: {lines:#?}");
        assert!(lines[0].starts_with('╭') && lines.last().unwrap().starts_with('╰'));
    }

    #[test]
    fn render_resume_orange_discipline() {
        let d = full_resume_data();
        let t = render_resume(&d, 80);
        assert!(orange_element_count(&t) <= 2, "resume orange > 2: {t:#?}");
    }

    #[test]
    fn render_resume_has_no_identity() {
        let d = full_resume_data();
        let joined = plain_lines(&render_resume(&d, 80)).join("\n");
        assert!(!joined.contains("██"));
        assert!(!joined.contains("Welcome back"));
        assert!(!joined.contains("opus · cli"));
        assert!(!joined.contains("This week"));
        assert!(!joined.contains(crate::help::HELP_HINT));
    }

    #[test]
    fn render_resume_title_names_the_topic() {
        let d = full_resume_data();
        let lines = plain_lines(&render_resume(&d, 80));
        assert!(lines[0].contains("Continuing · kaynak-ingest"), "top border: {}", lines[0]);
    }

    #[test]
    fn render_resume_sparse_drops_rows() {
        let mut d = full_resume_data();
        d.map_percent = None;
        d.due_count = 0;
        d.level = None;
        let joined = plain_lines(&render_resume(&d, 80)).join("\n");
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
        let long_item = "Async trait objects and pinning semantics in tokio task spawning and scheduling";
        let mut d = full_resume_data();
        d.next_item = Some(long_item.to_string());
        let lines = plain_lines(&render_resume(&d, 80));
        let joined = lines.join(" ");
        assert!(!joined.contains('…'), "next_item was truncated: {lines:#?}");
        for word in long_item.split(' ') {
            assert!(joined.contains(word), "missing word '{word}' from wrapped next_item: {lines:#?}");
        }
        assert!(joined.contains("scheduling"));
    }
}
