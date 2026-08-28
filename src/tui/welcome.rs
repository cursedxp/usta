//! Welcome box render: the two-column box (logo/identity left, status right),
//! its three modes — full welcome, compact resume continuation, and the
//! entry-point dispatch between them — plus the wrap/pad/fit text helpers
//! they share. Spec §5. Data gathering moved to `welcome_data.rs` (cleanup
//! round, Task 2); this module only draws from the `WelcomeData` it's given.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use unicode_width::UnicodeWidthStr;

use crate::tui::theme;
use crate::tui::welcome_data::WelcomeData;

// 5 rows — a block-letter S needs top, left, middle, right AND bottom bars;
// the old 4-row version had no bottom bar, so the S looked cut in half.
const LOGO: [&str; 5] = [
    "██  ██ ██████ ██████ ██████",
    "██  ██ ██       ██   ██  ██",
    "██  ██ ██████   ██   ██████",
    "██  ██     ██   ██   ██  ██",
    "██████ ██████   ██   ██  ██",
];

/// Truncate to visible width, add `…` if it overflows. Padding calculations
/// also use unicode-width — byte counting misaligns Turkish characters.
pub fn fit(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0usize;
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > max.saturating_sub(1) {
            break;
        }
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
        let extra = if current.is_empty() {
            word_w
        } else {
            current_w + 1 + word_w
        };
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
        Some(format!(
            "This week: {sessions} session(s) · streak {streak} day(s)"
        ))
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
    let inner = total - 2; // borders
    let left_w = 34usize;
    let right_w = inner - left_w - 3; // " │ " separator

    let greet = match &d.name {
        Some(n) => format!("Welcome back, {n}!"),
        None => "Welcome back!".to_string(),
    };
    let mut left: Vec<(String, bool)> = vec![(String::new(), false)];
    for l in LOGO {
        left.push((format!("  {l}"), true));
    }
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
        if let Some(p) = d.map_percent {
            right.push((format!("Map: {p}%"), Style::default()));
        }
        right.push(("─".repeat(right_w), Style::default()));
        right.push(("Up next".to_string(), Style::default()));
        if let Some(n) = &d.next_item {
            for l in wrap(n, right_w) {
                right.push((l, Style::default()));
            }
        }
        if d.due_count > 0 {
            right.push((
                format!("Reviews due today: {}", d.due_count),
                Style::default(),
            ));
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
/// (Claude-style: welcome on top, question below). Wired up in entry.rs's
/// topic entry (`ask_topic`).
///
/// `local`: topics recorded in this project — if not empty, shows an
/// `Enter → resume <first>` line and a numbered list (≤6). `other`: topics
/// recorded in other projects — informational only, not selectable, summarized
/// in a dim line.
///
/// `introduction`: `true` when this box is printed ahead of the pre-lock
/// introduction conversation (feature 13a) rather than the topic-entry prompt
/// (`ask_topic`). No topic is being asked for in that flow — the model opens
/// the conversation instead — so the right column must not offer topic
/// selection or an Enter shortcut; it gets the same "First session — let's
/// start with an introduction." wording `render_welcome`'s `first_session`
/// branch uses, and `local`/`other`/`project_known` are ignored. `false` keeps
/// today's topic-selection column exactly as it was.
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
    introduction: bool,
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
    for l in LOGO {
        left.push((format!("  {l}"), true));
    }
    left.push((String::new(), false));
    left.push((format!("  {}", fit(&greet, left_w - 2)), false));
    left.push((format!("  {}", fit(model, left_w - 2)), false));
    left.push((format!("  {}", fit(dir, left_w - 2)), false));

    // Topics in other projects are informational only — shown dim (DIM).
    let dim = Style::default().add_modifier(Modifier::DIM);
    let mut right: Vec<(String, Style)> = if introduction {
        // Pre-lock introduction (13a): no topic prompt, no Enter shortcut —
        // the one-shot suggestion machinery those hints describe was deleted.
        // Same wording/style as render_welcome's first_session branch.
        let mut r = vec![
            ("Learning Status".to_string(), Style::default()),
            (String::new(), Style::default()),
        ];
        for l in wrap("First session — let's start with an introduction.", right_w) {
            r.push((l, Style::default()));
        }
        r
    } else {
        let mut r: Vec<(String, Style)> = vec![
            ("What do you want to learn?".to_string(), Style::default()),
            (String::new(), Style::default()),
        ];
        if let Some(first) = local.first() {
            for l in wrap(&format!("Enter → resume {first}"), right_w) {
                r.push((l, Style::default()));
            }
            // List items stay on `fit`, not `wrap` — wrapping a topic name would
            // continue onto an unnumbered line, breaking the numbered list's
            // visual alignment (see render_box's per-row rendering).
            for (i, t) in local.iter().take(6).enumerate() {
                r.push((fit(&format!("{}) {t}", i + 1), right_w), Style::default()));
            }
            r.push((String::new(), Style::default()));
            for l in wrap("Type to start a new topic.", right_w) {
                r.push((l, Style::default()));
            }
            if !other.is_empty() {
                for l in wrap(&format!("In other projects: {}", other.join(", ")), right_w) {
                    r.push((l, dim));
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
                r.push((l, Style::default()));
            }
            // the previous "Registered:" line is REMOVED — replaced by the other-projects info line (if any).
            if !other.is_empty() {
                r.push((String::new(), Style::default()));
                for l in wrap(&format!("In other projects: {}", other.join(", ")), right_w) {
                    r.push((l, dim));
                }
            }
        }
        r
    };
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
    // Mirror of the low-end guard below: rounding alone maps 96-99% to a full
    // 12/12 bar (round(96 * 12 / 100) == 12), which reads as "done" next to a
    // number that isn't 100. A full bar means 100% and nothing else. (No
    // `.min(12)` needed first — `pct <= 100` already bounds `filled` at 12.)
    let filled = if pct < 100 && filled >= 12 {
        11
    } else {
        filled
    };
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

    // Cap the title so it can never desync the top border from the panel
    // body. The dash-run formula below (`inner.saturating_sub(4 +
    // title.width())`) floors at 0 once the title is too wide, but nothing
    // capped the title ITSELF — an uncapped title made the printed top-border
    // line longer than every other line in the panel, breaking the
    // equal-width invariant every bordered frame in this file relies on.
    // `fit` truncates to a display width and appends `…`, so
    // `title.width()` after this is always <= inner.saturating_sub(4),
    // which keeps the dash count >= 0 and the top line's total width exactly
    // `inner + 2`, matching every other line, for any title length at any
    // width in the clamp range.
    //
    // NOTE: `title` is intentionally NOT trimmed here (unlike `render_box`,
    // which calls `.trim()` on its title before measuring/printing it).
    // `render_resume` bakes a trailing space into its title string
    // (`"Continuing · {topic} "`) so the dash run starts one column later,
    // matching the design mock (`Continuing · topic ─────`). Since
    // `title.width()` is used unmodified for the dash count, trimming here
    // would silently swallow that space and shift the dash run one column
    // left with no test failure — see
    // `solo_box_preserves_title_trailing_space_no_trim`, which pins the
    // exact spacing.
    let title = fit(title, inner.saturating_sub(4));

    // Same fixed-offset formula as render_box's top border (see that
    // function's comment): "╭─── " (5) + "╮" (1) = 6 fixed chars, and
    // inner already excludes the 2 side borders, so 6-2=4 remains.
    let top = Line::from(vec![
        Span::raw("╭─── "),
        Span::styled(title.clone(), theme::brand().add_modifier(Modifier::BOLD)),
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
///
/// Wired in (v0.21.0) via `render_for_entry`'s dispatch: on the `resumed`
/// branch it calls this instead of `render_welcome`, replacing the second
/// identity-carrying frame that used to print right after the one
/// `ask_topic` already showed.
///
/// Returns `None` when every row would drop — i.e. there's genuinely nothing
/// to show (a topic upserted at OPEN but never reaching its first CLOSING
/// flush: no `last_session`, `level`, `map_percent`, `next_item` or due
/// count). Rendering `solo_box` anyway would print an empty two-line frame
/// (top+bottom border, no content) — dead weight on screen, since the
/// `resuming: <topic>` notice printed moments earlier by the caller already
/// says everything true at this point. Callers must skip printing the panel
/// entirely on `None` rather than substitute a placeholder box.
pub fn render_resume(d: &WelcomeData, width: u16) -> Option<Text<'static>> {
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
    // neither is present. `d.level` is free-form curriculum text with no
    // upper bound, so this wraps the same way "Up next" (row 4 below) does —
    // never truncated/ellipsized — with continuation lines aligned to the
    // 15-column value start. When the combined text fits on one line (the
    // common case) the original mixed plain/DIM styling is kept; a genuine
    // wrap falls back to a single plain style per line, since a word-level
    // wrap can't cleanly preserve which fragment (rel vs level) a given
    // wrapped word came from.
    if d.last_session.is_some() || d.level.is_some() {
        let (label, value_text, mixed): (&str, String, Option<Vec<Span<'static>>>) =
            match (&d.last_session, &d.level) {
                (Some(rel), Some(level)) => (
                    "Last session",
                    format!("{rel} · Level {level}"),
                    Some(vec![
                        Span::styled(rel.clone(), plain),
                        Span::styled(" · ".to_string(), dim),
                        Span::styled("Level ".to_string(), dim),
                        Span::styled(level.clone(), plain),
                    ]),
                ),
                (Some(rel), None) => ("Last session", rel.clone(), None),
                (None, Some(level)) => ("Level", level.clone(), None),
                (None, None) => unreachable!("guarded by the outer if"),
            };
        let wrapped = wrap(&value_text, value_w);
        // `wrapped` is computed from `wrap`, which collapses whitespace runs
        // (it splits on `split_whitespace`). But when this row IS single-line,
        // it's rendered from `mixed`'s RAW spans (below), not from `wrapped` —
        // so the fit decision must be based on the width that is ACTUALLY
        // rendered (`value_text`, which preserves whatever whitespace `level`
        // came with), not on `wrapped.len()`. Otherwise a raw string with
        // doubled internal spaces (e.g. from an LLM-written bullet —
        // `extract_level` only trims the ends) can collapse to something that
        // fits while the raw text that's actually printed does not, and the
        // row silently overflows `value_w`.
        let single_line = value_text.width() <= value_w;
        for (i, line) in wrapped.into_iter().enumerate() {
            if i == 0 {
                let mut row = vec![
                    Span::raw("  "),
                    Span::styled(pad(label, 12), dim),
                    Span::raw(" "),
                ];
                match (single_line, &mixed) {
                    (true, Some(spans)) => row.extend(spans.clone()),
                    _ => row.push(Span::styled(line, plain)),
                }
                rows.push(row);
            } else {
                rows.push(vec![Span::raw(" ".repeat(15)), Span::styled(line, plain)]);
            }
        }
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

    // Finding 1: nothing to show — skip the panel entirely rather than print
    // an empty top+bottom-border frame. See the doc comment above for when
    // this is reachable (topic upserted at OPEN, never reaching CLOSING; or
    // any closing flush that failed).
    if rows.is_empty() {
        return None;
    }

    // Trailing space is deliberate — see solo_box's "NOTE: title is
    // intentionally NOT trimmed" comment for why it must survive.
    let title = format!("Continuing · {} ", d.topic);
    Some(solo_box(&title, rows, width))
}

/// Pick the welcome render for the topic-entry point (run.rs, `had_topic_arg
/// || resumed` branch). The two entry paths render differently because they
/// arrive on screen in different states: `usta start <topic>` (`had_topic_arg
/// = true`) never printed an identity frame on its way here, so it has
/// nothing to duplicate — the full-mode box, carrying identity plus Learning
/// Status, is the only frame shown. A resume (`had_topic_arg = false`)
/// already has the identity welcome on screen from `ask_topic` moments
/// earlier, so repeating logo/greeting/model/dir/week-streak here would print
/// them twice within a few rows — that duplicate-box bug is exactly what this
/// dispatch exists to prevent, so the resume path instead gets the compact,
/// identity-free continuation panel.
///
/// Returns `None` only on the resume path when `render_resume` has nothing to
/// show (see its doc comment) — the full-mode box (`had_topic_arg = true`)
/// always has identity content, so that arm always returns `Some`. Callers
/// must skip printing on `None` rather than substitute a placeholder.
pub fn render_for_entry(had_topic_arg: bool, d: &WelcomeData, width: u16) -> Option<Text<'static>> {
    if had_topic_arg {
        Some(render_welcome(d, width))
    } else {
        render_resume(d, width)
    }
}

/// Append the `/help` discovery hint as a separate dim line after the bordered
/// box — NOT inside the box, so the box's equal-width line logic stays intact.
fn with_help_hint(mut t: Text<'static>) -> Text<'static> {
    let dim = Style::default().add_modifier(Modifier::DIM);
    t.lines
        .push(Line::from(Span::styled(crate::help::HELP_HINT, dim)));
    t
}

/// Draw the two-column box — border + " │ " separator + equal-width padding.
/// `left`: (text, is-logo). `right`: (text, style) — row 0 is also automatically
/// wrapped in a bold+orange title style (even if the row's own style is empty),
/// other rows are printed with whatever style they carry (e.g. DIM).
fn render_box(
    version: &str,
    left: Vec<(String, bool)>,
    right: Vec<(String, Style)>,
    width: u16,
) -> Text<'static> {
    let total = (width as usize).clamp(60, 100);
    let inner = total - 2; // borders
    let left_w = 34usize;
    let right_w = inner - left_w - 3; // " │ " separator

    let rows = left.len().max(right.len());
    let title = format!(" Usta v{version} ");
    // NOTE: dashes = inner - (4 + title_width) should hold — the "╭─── " prefix is
    // 5 chars, the closing "╮" is 1 char, 6 total fixed; since inner = total-2,
    // 6-2=4 remains. The "5 +" formula from the briefing left the line 1 char
    // short (breaking the equal-width test).
    let top = format!(
        "╭─── {}{}╮",
        title.trim(),
        "─".repeat(inner.saturating_sub(4 + title.trim().width()))
    );
    let bottom = format!("╰{}╯", "─".repeat(inner));

    let mut lines: Vec<Line> = vec![Line::from(top)];
    for i in 0..rows {
        let (ltxt, is_logo) = left.get(i).cloned().unwrap_or_default();
        let (rtxt, rtxt_style) = right.get(i).cloned().unwrap_or_default();
        let lspan = Span::styled(
            pad(&ltxt, left_w),
            if is_logo {
                theme::brand()
            } else {
                Style::default()
            },
        );
        let rstyle = if i == 0 && !rtxt.is_empty() {
            theme::brand().add_modifier(Modifier::BOLD)
        } else {
            rtxt_style
        };
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
#[path = "welcome_tests.rs"]
mod tests;
