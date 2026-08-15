//! Oturum geçmişi: her `usta` kapanışında `learner/history.md`'ye eklenen
//! tek satırlık kayıt. Streak (art arda gün) ve haftalık özet hesapları
//! bu kayıtlar üzerinden türetilir. `usta stats` ve welcome kutusu tüketir.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use chrono::NaiveDate;

const HEADER: &str = "# Oturum Geçmişi\n\n";

/// A single history line: date + topic + optional map percent + optional settled count.
#[derive(Debug, PartialEq)]
pub struct Entry {
    pub date: String,
    pub topic: String,
    pub map: Option<u8>,
    pub settled: Option<usize>,
}

/// One topic's activity within a week window: session count + first/last snapshot.
#[derive(Debug, PartialEq)]
pub struct TopicWeek {
    pub topic: String,
    pub sessions: u32,
    pub map_from: Option<u8>,
    pub map_to: Option<u8>,
    pub settled_from: Option<usize>,
    pub settled_to: Option<usize>,
}

/// Weekly rollup: total sessions + per-topic breakdown, topics in first-seen order.
#[derive(Debug, PartialEq)]
pub struct WeekSummary {
    pub sessions: u32,
    pub per_topic: Vec<TopicWeek>,
}

/// Render one history line: `- {date} | {topic} | map {P}% | settled {N}`.
/// `None` renders as `map -` / `settled -`.
pub fn record_line(date: &str, topic: &str, map_percent: Option<u8>, settled: Option<usize>) -> String {
    let map = match map_percent {
        Some(p) => format!("map {p}%"),
        None => "map -".to_string(),
    };
    let settled = match settled {
        Some(n) => format!("settled {n}"),
        None => "settled -".to_string(),
    };
    format!("- {date} | {topic} | {map} | {settled}")
}

/// Parse `- `-prefixed lines into entries; malformed lines (wrong shape, bad date) are skipped.
pub fn entries(content: &str) -> Vec<Entry> {
    content
        .lines()
        .filter_map(|l| {
            let rest = l.strip_prefix("- ")?;
            let mut parts = rest.split(" | ");
            let date = parts.next()?.trim().to_string();
            let topic = parts.next()?.trim().to_string();
            let map_part = parts.next()?.trim();
            let settled_part = parts.next()?.trim();
            if parts.next().is_some() {
                return None;
            }
            NaiveDate::parse_from_str(&date, "%Y-%m-%d").ok()?;
            let map = map_part.strip_prefix("map ")?;
            let map = if map == "-" { None } else { map.strip_suffix('%')?.parse::<u8>().ok() };
            let settled = settled_part.strip_prefix("settled ")?;
            let settled = if settled == "-" { None } else { settled.parse::<usize>().ok() };
            Some(Entry { date, topic, map, settled })
        })
        .collect()
}

/// Append a line to `learner/history.md`, creating it with the header if missing.
pub fn append(global: &Path, line: &str) -> Result<()> {
    let path = global.join("learner/history.md");
    let mut content = std::fs::read_to_string(&path).unwrap_or_else(|_| HEADER.to_string());
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(line);
    content.push('\n');
    crate::progress::write_atomic(&path, &content)
}

/// Consecutive-day streak ending today (or yesterday, if nothing logged today yet).
pub fn current_streak(entries: &[Entry], today: &str) -> u32 {
    let Ok(today) = NaiveDate::parse_from_str(today, "%Y-%m-%d") else { return 0 };
    let days: BTreeSet<NaiveDate> = entries
        .iter()
        .filter_map(|e| NaiveDate::parse_from_str(&e.date, "%Y-%m-%d").ok())
        .collect();
    let yesterday = today - chrono::Duration::days(1);
    let mut cursor = if days.contains(&today) {
        today
    } else if days.contains(&yesterday) {
        yesterday
    } else {
        return 0;
    };
    let mut count = 0u32;
    while days.contains(&cursor) {
        count += 1;
        cursor -= chrono::Duration::days(1);
    }
    count
}

/// Longest consecutive-day run across all entries.
pub fn longest_streak(entries: &[Entry]) -> u32 {
    let days: BTreeSet<NaiveDate> = entries
        .iter()
        .filter_map(|e| NaiveDate::parse_from_str(&e.date, "%Y-%m-%d").ok())
        .collect();
    let mut longest = 0u32;
    let mut current = 0u32;
    let mut prev: Option<NaiveDate> = None;
    for d in &days {
        match prev {
            Some(p) if *d == p + chrono::Duration::days(1) => current += 1,
            _ => current = 1,
        }
        longest = longest.max(current);
        prev = Some(*d);
    }
    longest
}

/// Sessions in the trailing 7-day window (today - 6 days, inclusive), grouped by topic
/// (first-seen order); each topic's from/to is its first/last in-window entry.
pub fn week_summary(entries: &[Entry], today: &str) -> WeekSummary {
    let Ok(today) = NaiveDate::parse_from_str(today, "%Y-%m-%d") else {
        return WeekSummary { sessions: 0, per_topic: Vec::new() };
    };
    let lower = today - chrono::Duration::days(6);
    let in_window: Vec<&Entry> = entries
        .iter()
        .filter(|e| {
            NaiveDate::parse_from_str(&e.date, "%Y-%m-%d")
                .map(|d| d >= lower && d <= today)
                .unwrap_or(false)
        })
        .collect();
    let mut order: Vec<String> = Vec::new();
    for e in &in_window {
        if !order.contains(&e.topic) {
            order.push(e.topic.clone());
        }
    }
    let per_topic: Vec<TopicWeek> = order
        .into_iter()
        .map(|topic| {
            let group: Vec<&&Entry> = in_window.iter().filter(|e| e.topic == topic).collect();
            let first = group.first().unwrap();
            let last = group.last().unwrap();
            TopicWeek {
                topic,
                sessions: group.len() as u32,
                map_from: first.map,
                map_to: last.map,
                settled_from: first.settled,
                settled_to: last.settled,
            }
        })
        .collect();
    WeekSummary { sessions: in_window.len() as u32, per_topic }
}

/// Count of curriculum items in the two "deepest" states (`oturdu`, `derinleşildi`).
/// Deliberate copy of the two deepest words in `tui::welcome::STATUSES` (private there,
/// so duplicated here) — test-locked, keep in sync if that list ever changes.
pub fn settled_count(curriculum: &str) -> Option<usize> {
    Some(
        curriculum
            .lines()
            .filter(|l| l.starts_with("- ") && (l.contains("oturdu") || l.contains("derinleşildi")))
            .count(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_line_entries_roundtrip_and_skip_malformed() {
        let l1 = record_line("2026-08-15", "rust", Some(55), Some(7));
        let l2 = record_line("2026-08-15", "gtm", None, None);
        let content = format!("# Oturum Geçmişi\n{l1}\n{l2}\nbozuk satır\n");
        let es = entries(&content);
        assert_eq!(es.len(), 2);
        assert_eq!(es[0].topic, "rust");
        assert_eq!(es[0].map, Some(55));
        assert_eq!(es[0].settled, Some(7));
        assert_eq!(es[1].map, None);
    }

    #[test]
    fn streaks_count_consecutive_days() {
        let mk = |d: &str| Entry { date: d.into(), topic: "t".into(), map: None, settled: None };
        let es = vec![mk("2026-08-10"), mk("2026-08-13"), mk("2026-08-14"), mk("2026-08-15"), mk("2026-08-15")];
        assert_eq!(current_streak(&es, "2026-08-15"), 3);
        // bugün oturum yok ama dün biten seri güncel sayılır
        assert_eq!(current_streak(&es, "2026-08-16"), 3);
        // bir günden fazla boşluk → seri bitti
        assert_eq!(current_streak(&es, "2026-08-18"), 0);
        assert_eq!(longest_streak(&es), 3);
        assert_eq!(current_streak(&[], "2026-08-15"), 0);
    }

    #[test]
    fn week_summary_windows_and_groups() {
        let mk = |d: &str, t: &str, m: u8, s: usize| Entry { date: d.into(), topic: t.into(), map: Some(m), settled: Some(s) };
        let es = vec![
            mk("2026-08-07", "rust", 30, 3), // 8 gün önce — pencere dışı
            mk("2026-08-09", "rust", 40, 4),
            mk("2026-08-14", "rust", 55, 7),
            mk("2026-08-15", "gtm", 10, 1),
        ];
        let w = week_summary(&es, "2026-08-15");
        assert_eq!(w.sessions, 3);
        let rust = w.per_topic.iter().find(|t| t.topic == "rust").unwrap();
        assert_eq!((rust.sessions, rust.map_from, rust.map_to), (2, Some(40), Some(55)));
    }

    #[test]
    fn settled_count_counts_settled_states() {
        let c = "- a: oturdu\n- b: görüldü\n- c: derinleşildi\n- d: görülmedi\n";
        assert_eq!(settled_count(c), Some(2));
        assert_eq!(settled_count(""), Some(0));
    }

    #[test]
    fn append_creates_with_header_then_appends() {
        let base = std::env::temp_dir().join(format!("usta_history_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        append(&base, &record_line("2026-08-15", "rust", None, None)).unwrap();
        append(&base, &record_line("2026-08-15", "gtm", None, None)).unwrap();
        let c = std::fs::read_to_string(base.join("learner/history.md")).unwrap();
        assert!(c.starts_with("# Oturum Geçmişi"));
        assert_eq!(entries(&c).len(), 2);
        let _ = std::fs::remove_dir_all(&base);
    }
}
