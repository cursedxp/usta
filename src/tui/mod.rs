//! Claude Code-style TUI: a self-drawn relative-addressed bottom region.
//! In plain mode (ui::is_plain) this module is never used.

pub mod ask;
pub mod convert;
pub mod editor;
pub mod entry;
pub mod intro;
pub mod page;
pub mod paint;
pub mod polite;
pub mod run;
pub(crate) mod screen;
pub mod status;
pub mod term;
pub mod theme;
pub mod welcome;
pub mod welcome_data;
