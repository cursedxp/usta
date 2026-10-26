# Full-screen TUI (opencode-style) — Design

Date: 2026-10-26 · Status: approved · Branch: `fullscreen` (cut from `main` after `multiline-input` merges)

## Goal
usta owns the whole terminal window (alternate screen) instead of an inline viewport at the bottom. Chat history is rendered by usta in a scrollable pane above the editor.

## Decisions (user-approved)
1. **Mouse capture on.** Wheel scrolls usta's chat pane. Native selection via Shift+drag (iTerm2/Ghostty/WezTerm).
2. **Exit clears.** Leaving the alternate screen restores the previous terminal content; no transcript is printed. Chat persists in usta's own history store.

## Layout
```
┌ chat pane (scrollable, fills remaining height) ┐
│ ...                                            │
│                         ↓ new message (if any) │
├ editor (existing multiline box, variable h)    ┤
└ status bar                                     ┘
```

## Architecture
- **`term.rs`:** `Viewport::Fullscreen`; `setup()` adds `EnterAlternateScreen` + `EnableMouseCapture`; `restore()` adds `DisableMouseCapture` + `LeaveAlternateScreen` (panic hook already calls `restore()`). Inline-specific code (CPR seed query, `set_height` rebuild, `TrackedBackend` seed) is removed; editor height becomes a layout constraint, not a viewport rebuild. `Drop` no longer prints a trailing newline.
- **Chat buffer (new, e.g. `tui/chat.rs`):** `Vec<Entry>` storing un-wrapped source (`Text<'static>` or the raw reply/echo + kind) so it re-wraps on width change. Holds `scroll_offset` (lines from bottom) and `follow: bool`.
- **`page.rs`:** `page()` and the `page_*` wrappers push into the chat buffer instead of `insert_before`. Callers unchanged.
- **`draw()`:** splits the frame: chat pane (`Paragraph` with wrap + scroll computed from offset), editor, status bar.
- **Events (`run.rs` loops):** `MouseEventKind::ScrollUp/Down` (3 lines), `PageUp/PageDown` (pane height − 1). Scrolling to bottom re-enables `follow`. New content while `follow == false` sets an unread flag → "↓ new message" indicator; cleared on reaching bottom.
- **Resize:** re-wrap from source; keep bottom-anchored when following, otherwise clamp offset.

## Unchanged
Editor behavior, slash commands, backend, welcome/intro content (rendered into the chat pane), plain (non-TTY) mode in `plain.rs`/`ui.rs`.

## Out of scope
Loading past sessions into the pane, in-app selection/copy, search in history.

## Testing
- Unit: chat buffer wrap/scroll math (offset clamp, follow, unread flag, re-wrap on width change).
- Render: ratatui `TestBackend` snapshot of layout at 80×24 and narrow width.
- Source guards (existing style in `page.rs` tests): every event loop handles Resize + Mouse; `restore()` leaves alternate screen and disables mouse capture.
- Manual: iTerm2/Ghostty — wheel scroll, Shift+drag select, resize, Ctrl+C exit restores screen, panic restores screen.
