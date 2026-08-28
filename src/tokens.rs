//! Protocol tokens — single source of truth for every marker the shell
//! parses or writes. User-facing language stays free (SOUL.md language
//! lock); these internal tokens are the protocol. Values flip to English
//! in the migration release; legacy forms then move to src/migrate.rs.

pub const STATE_NOT_SEEN: &str = "not seen";
pub const STATE_SEEN: &str = "seen";
pub const STATE_SETTLED: &str = "settled";
pub const STATE_DEEPENED: &str = "deepened";
/// Order matters: index 0 is the "unseen" state, 1.. are the seen states.
pub const STATES: [&str; 4] = [STATE_NOT_SEEN, STATE_SEEN, STATE_SETTLED, STATE_DEEPENED];

/// Extract the map state of a `- <item>: <state>` line (optional `| due: …` tail).
/// Exact segment match — never a substring scan ("seen" ⊂ "not seen").
pub fn map_state_of(line: &str) -> Option<&'static str> {
    let line = line.trim();
    if !line.starts_with("- ") {
        return None;
    }
    let head = line.split(" | ").next().unwrap_or(line); // drop `| due:` tail
    let state = head.rsplit(':').next()?.trim();
    STATES.iter().find(|s| **s == state).copied()
}

// Approach-file role field (SPEC §4.22): inferred, never asked as a menu.
// The shell does NOT parse it in 13a (13b adds the reader) — single-sourced
// here because two prompts embed the same strings.
pub const ROLE_PREFIX: &str = "role: ";
pub const ROLE_GUIDED: &str = "guided";
pub const ROLE_PROJECT: &str = "project";
pub const ROLE_OBSERVATION: &str = "observation";

// Bare section names (used with the `section()` helpers, no `## ` prefix).
pub const S_LEVEL: &str = "Level";
pub const S_RECALL: &str = "Recall questions";
pub const S_RETIRED: &str = "Retired";
pub const S_OPEN_EXERCISE: &str = "Open exercise";
pub const S_GAPS: &str = "Gaps";
pub const S_ERROR_LOG: &str = "Error log";
pub const S_HINT_LADDER: &str = "Hint ladder";

// Full line-start headers.
pub const H_RECORDS: &str = "## Records";
pub const H_GOAL: &str = "## Goal";
pub const H_GOAL_STATUS: &str = "## Goal Status";
pub const H_PREFERENCES: &str = "## Preferences";

// File / flow markers.
pub const FILE_DIVIDER: &str = "===FILE:";
pub const CHECKPOINT: &str = "[CHECKPOINT]";
pub const SOURCE_DASH: &str = "— source:";
pub const HISTORY_HEADER: &str = "# Session History\n\n";
/// Progress file heading suffix: `# <topic> — Progress`.
pub const PROGRESS_HEADING_SUFFIX: &str = "— Progress";
/// Pre-lock conversation topic lock: the model's FINAL reply line
/// `TOPIC: <slug>` locks the topic (SPEC §4.22).
#[allow(dead_code)]
pub const TOPIC_MARKER: &str = "TOPIC:";
pub const DEFAULT_TOPIC: &str = "general";
