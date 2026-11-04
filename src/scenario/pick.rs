//! Which run a `/scenario` command acts on.

use anyhow::{anyhow, bail, Result};

use crate::scenario::library;
use crate::scenario::store::{RunRecord, RunState, Store};

pub(crate) fn own_active(store: &Store, pid: u32) -> Result<RunRecord> {
    store
        .all()
        .into_iter()
        .find(|r| r.state == RunState::Active && r.owner_pid == pid)
        .ok_or_else(|| anyhow!("no active task; use /scenario start or /scenario resume"))
}

/// Paused, or active and owned by this process (its server may have stopped
/// mid-session; `engine::resume` handles both). Never finished, dropped, another
/// topic's or another Usta's.
pub(crate) fn open_runs(store: &Store, topic: &str, pid: u32) -> Vec<RunRecord> {
    store
        .all()
        .into_iter()
        .filter(|r| {
            library::topic_matches(&r.topic, topic)
                && (r.state == RunState::Paused
                    || (r.state == RunState::Active && r.owner_pid == pid))
        })
        .collect()
}

pub(crate) fn pick_resumable(
    store: &Store,
    sel: Option<&str>,
    topic: &str,
    pid: u32,
) -> Result<RunRecord> {
    let paused = open_runs(store, topic, pid);
    Ok(match sel {
        Some(id) => paused
            .into_iter()
            .find(|r| r.run_id == id)
            .ok_or_else(|| anyhow!("no task '{id}' to resume; see /scenario"))?,
        None => paused
            .iter()
            .rfind(|r| r.state == RunState::Paused)
            .or(paused.last())
            .cloned()
            .ok_or_else(|| anyhow!("nothing to resume; see /scenario"))?,
    })
}

pub(crate) fn pick_unfinished(
    store: &Store,
    sel: Option<&str>,
    topic: &str,
    pid: u32,
) -> Result<RunRecord> {
    let open = open_runs(store, topic, pid);
    match sel {
        Some(id) => open
            .into_iter()
            .find(|r| r.run_id == id)
            .ok_or_else(|| anyhow!("no unfinished task '{id}'; see /scenario")),
        None => match open.len() {
            0 => bail!("no unfinished task; see /scenario"),
            1 => Ok(open.into_iter().next().expect("one element")),
            _ => bail!(
                "several unfinished tasks; name one: {}",
                open.iter()
                    .map(|r| format!("/scenario drop {}", r.run_id))
                    .collect::<Vec<_>>()
                    .join(" · ")
            ),
        },
    }
}
