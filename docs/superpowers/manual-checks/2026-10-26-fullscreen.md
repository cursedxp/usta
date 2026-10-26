# Fullscreen TUI — manual checks (v0.33.0)

- Branch: `fullscreen`
- Base head when written: 9d4e400 (feature commits 8cf8852..9d4e400)
- Run: `cargo run` in iTerm2, then again in Ghostty. Record a result per item per terminal.

| # | Check | iTerm2 | Ghostty |
|---|---|---|---|
| 1 | `cargo run`: window clears, welcome panel at top of the pane, editor + status at the bottom. | | |
| 2 | Chat long enough to overflow: mouse wheel scrolls 3 lines per notch; PageUp/PageDown move a page. | | |
| 3 | Scrolled up, trigger a reply (or `/help`): "↓ new message" appears bottom-right of the pane; scrolling to the bottom clears it; view did not jump. | | |
| 4 | Submit a message while scrolled up: pane jumps to the bottom. | | |
| 5 | Shift+drag selects text and Cmd+C copies it. | | |
| 6 | Resize the window (wider, narrower, very short): chat re-wraps, no ghost lines, welcome panel clipped not garbled. | | |
| 7 | Type 8+ lines in the editor: box grows, chat pane shrinks, nothing overlaps. | | |
| 8 | Ctrl+C / Ctrl+D exit: previous terminal content is back, cursor visible, no stray blank line, shell usable (`echo ok`). | | |
| 9 | Force a panic (temporary `panic!()` behind an env var, then remove it): terminal restored, panic message readable on the normal screen. | | |
| 10 | Pipe mode: `echo hi \| cargo run` falls back to plain mode (no escape codes). | non-interactive: see task-5 report | |

Result notes:
