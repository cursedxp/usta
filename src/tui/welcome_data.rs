//! Welcome box data gathering (pure) — no ratatui dependency. Spec §5 —
//! extracted from `welcome.rs` (cleanup round, Task 2).
//! All parsing is best-effort — malformed/missing input skips the field, never panics.

use crate::tokens;

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
    if name.is_empty() || name.contains("Profil") || name.starts_with('#') {
        return None;
    }
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
            Some(_) => {
                total += 1;
                seen += 1;
            }
            None => {}
        }
    }
    if total == 0 {
        return None;
    }
    Some(((seen * 100) / total) as u8)
}

/// Text of the first `tokens::STATE_NOT_SEEN` item — list marker and status suffix stripped.
pub fn next_unseen(curriculum: &str) -> Option<String> {
    let line = curriculum
        .lines()
        .find(|l| tokens::map_state_of(l) == Some(tokens::STATE_NOT_SEEN))?;
    let head = line.trim().split(" | ").next().unwrap_or(line.trim());
    let text = head
        .rsplit_once(':')?
        .0
        .trim()
        .trim_start_matches(['-', '*', ' '])
        .trim_end_matches([':', '—', '-', '·', '|', ' ']);
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
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
    let Some(s) = section(progress, tokens::S_RECALL) else {
        return Vec::new();
    };
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
    due_items(progress, today)
        .into_iter()
        .take(3)
        .map(|(_, line)| line)
        .collect()
}

/// Relative phrasing for the newest history entry of `topic`, EXCLUDING the
/// session being opened right now (its line is appended at close, not at open).
/// `0` → `today`, `1` → `yesterday`, `n` → `n days ago`. A future-dated entry
/// (clock skew) collapses to `today` rather than printing a negative count.
/// ADHD-safe: the phrasing is a neutral timestamp at every distance — no
/// streak-zero, no "it has been a while" (SPEC §"ADHD-safe rules").
pub fn last_session_ago(
    entries: &[crate::history::Entry],
    topic: &str,
    today: &str,
) -> Option<String> {
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
    profile: Option<&str>,
    progress: Option<&str>,
    curriculum: Option<&str>,
    topic: &str,
    model: &str,
    dir: &str,
    today: &str,
    history: Option<&str>,
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

#[cfg(test)]
#[path = "welcome_data_tests.rs"]
mod tests;
