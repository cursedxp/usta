//! Claude Code-style TUI: fullscreen alternate screen + scrollable chat pane.
//! In plain mode (ui::is_plain) this module is never used.

pub mod ask;
#[allow(dead_code)] // scroll/unread API wired in Task 4
pub mod chat;
pub mod convert;
pub mod editor;
pub mod entry;
pub mod intro;
pub mod page;
pub mod paint;
pub mod polite;
pub mod run;
pub mod status;
pub mod term;
pub mod theme;
pub mod welcome;
pub mod welcome_data;
