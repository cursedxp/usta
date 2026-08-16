//! Protocol tokens — single source of truth for every marker the shell
//! parses or writes. User-facing language stays free (SOUL.md language
//! lock); these internal tokens are the protocol. Values flip to English
//! in the migration release; legacy forms then move to src/migrate.rs.

pub const STATE_NOT_SEEN: &str = "görülmedi";
pub const STATE_SEEN: &str = "görüldü";
pub const STATE_SETTLED: &str = "oturdu";
pub const STATE_DEEPENED: &str = "derinleşildi";
/// Order matters: index 0 is the "unseen" state, 1.. are the seen states.
pub const STATES: [&str; 4] = [STATE_NOT_SEEN, STATE_SEEN, STATE_SETTLED, STATE_DEEPENED];

// Bare section names (used with the `section()` helpers, no `## ` prefix).
pub const S_LEVEL: &str = "Seviye";
pub const S_RECALL: &str = "Geri çağırma soruları";
pub const S_RETIRED: &str = "Kapatılanlar";
pub const S_OPEN_EXERCISE: &str = "Açık egzersiz";
pub const S_GAPS: &str = "Gap'ler";
pub const S_ERROR_LOG: &str = "Hata günlüğü";
pub const S_HINT_LADDER: &str = "İpucu merdiveni";

// Full line-start headers.
pub const H_RECORDS: &str = "## Kayıtlar";
pub const H_GOAL: &str = "## Hedef";
pub const H_GOAL_STATUS: &str = "## Hedef Durumu";
pub const H_PREFERENCES: &str = "## Tercihler";

// File / flow markers.
pub const FILE_DIVIDER: &str = "===DOSYA:";
pub const CHECKPOINT: &str = "[ARA KAYIT]";
pub const SOURCE_DASH: &str = "— kaynak:";
pub const SOURCE_HYPHEN: &str = "- kaynak:";
pub const HISTORY_HEADER: &str = "# Oturum Geçmişi\n\n";
/// Progress file heading suffix: `# <topic> — İlerleme`.
pub const PROGRESS_HEADING_SUFFIX: &str = "— İlerleme";
pub const DEFAULT_TOPIC: &str = "genel";
