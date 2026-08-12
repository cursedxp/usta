# /help Command + Welcome Hint Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an in-session `/help` command listing keyboard shortcuts, slash commands, and CLI commands, plus a one-line discovery hint in the welcome box.

**Architecture:** A new focused `src/help.rs` owns the single source of truth (`help_text()`, `is_help_command()`, `HELP_HINT`). Both session loops (TUI `run.rs`, plain `main.rs::run_plain_loop`) intercept `/help` before pushing to the LLM, mirroring the existing `/watch` pattern. Both welcome renders append `HELP_HINT`.

**Tech Stack:** Rust 2021, ratatui, crossterm.

## Global Constraints

- `/help` is intercepted in BOTH loops before any `session.push_user` — it is NEVER sent to the LLM (echo/print + show help, then continue the loop), exactly like `/watch`.
- Single source of truth: `help_text()`, `is_help_command()`, `HELP_HINT` all live in `src/help.rs`; welcome.rs and both loops reference them — no duplicated strings.
- English copy (app base language is English).
- `help_text()` content must contain, verbatim, these substrings (tests assert them): `Ctrl+J`, `Esc`, `/watch on|off`, `/help`, `/quit`, `usta reset --factory`.
- Every task ends green: `cargo build -p usta` and `cargo test -p usta`.

---

### Task 1: /help command + welcome hint

**Files:**
- Create: `src/help.rs`
- Modify: `src/main.rs` (add `mod help;`; intercept `/help` in `run_plain_loop`)
- Modify: `src/tui/run.rs` (intercept `/help` in the main loop's Submit arm)
- Modify: `src/tui/welcome.rs` (append `HELP_HINT` to both welcome renders)

**Interfaces:**
- Produces: `pub fn help_text() -> &'static str`, `pub fn is_help_command(line: &str) -> bool`, `pub const HELP_HINT: &str` in `src/help.rs` (module declared `mod help;` in `main.rs`, referenced as `crate::help::…`).
- Consumes: existing `parse_watch_command`/`/quit` intercept sites in both loops; `page`/`page_user_echo`/`page_notice` (run.rs); `println!`/`ui::notice` + `ready_tx` (main.rs); the two welcome render functions (welcome.rs).

- [ ] **Step 1: Write the failing tests** — create `src/help.rs` with only the tests + empty stubs so the module compiles:

```rust
//! In-session /help: keyboard shortcuts, slash commands, and CLI commands.
//! Single source of truth referenced by both session loops and the welcome box.

/// One-line discovery hint shown under the welcome box.
pub const HELP_HINT: &str = "Type /help for shortcuts and commands.";

/// The full help block (English), printed when the user types `/help`.
pub fn help_text() -> &'static str {
    todo!()
}

/// True when the input line is exactly the `/help` command (trimmed).
pub fn is_help_command(line: &str) -> bool {
    let _ = line;
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_help_command_matches_only_bare_help() {
        assert!(is_help_command("/help"));
        assert!(is_help_command("  /help  "));
        assert!(!is_help_command("/help me"));
        assert!(!is_help_command("help"));
        assert!(!is_help_command("/quit"));
        assert!(!is_help_command(""));
    }

    #[test]
    fn help_text_lists_shortcuts_commands_and_cli() {
        let h = help_text();
        for needle in ["Ctrl+J", "Esc", "↑ / ↓", "/watch on|off", "/help", "/quit", "usta reset --factory", "usta topics"] {
            assert!(h.contains(needle), "help_text missing: {needle}");
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p usta --lib help`
Expected: FAIL (panics on `todo!()`). Note: `mod help;` must be added in main.rs (Step 3) before this compiles — if the module isn't wired yet, add `mod help;` first, then run.

- [ ] **Step 3: Implement `help.rs`** — replace the two `todo!()` bodies:

```rust
pub fn help_text() -> &'static str {
    "Usta — shortcuts & commands\n\
     \n\
     Keyboard\n\
     \x20\x20Enter            send message\n\
     \x20\x20Ctrl+J           new line   (also Shift+Enter / Alt+Enter on modern terminals)\n\
     \x20\x20Esc              stop Usta mid-reply\n\
     \x20\x20Ctrl-C / Ctrl-D  quit\n\
     \x20\x20↑ / ↓            previous / next message\n\
     \n\
     In-session commands\n\
     \x20\x20/watch on|off    file-feedback companion (on by default)\n\
     \x20\x20/help            this help\n\
     \x20\x20/quit            end the session\n\
     \n\
     Terminal commands\n\
     \x20\x20usta                    start — asks for the topic\n\
     \x20\x20usta start <topic>      start a specific topic\n\
     \x20\x20usta topics             list what you're learning where\n\
     \x20\x20usta reset <topic>      reset a topic's progress in this project\n\
     \x20\x20usta reset --profile    reset only your profile\n\
     \x20\x20usta reset --factory    reset everything"
}

pub fn is_help_command(line: &str) -> bool {
    line.trim() == "/help"
}
```

(The `\x20\x20` are two leading spaces per row — Rust's `\` line-continuation eats leading whitespace, so encode the indent explicitly.)

- [ ] **Step 4: Declare the module** — in `src/main.rs`, add `mod help;` alongside the other `mod` declarations near the top of the file.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p usta --lib help`
Expected: PASS (both tests).

- [ ] **Step 6: Intercept `/help` in the plain loop** — in `src/main.rs::run_plain_loop`, in the `InputEvent::Line(line)` arm, right next to the existing `parse_watch_command` intercept (before `/quit`), add:

```rust
if help::is_help_command(&line) {
    println!("{}", help::help_text());
    let _ = ready_tx.send(());
    continue;
}
```

- [ ] **Step 7: Intercept `/help` in the TUI loop** — in `src/tui/run.rs`, in the main loop's `Action::Submit(line)` arm, next to the existing `crate::parse_watch_command` intercept (before the `/quit` check), add:

```rust
if crate::help::is_help_command(&line) {
    page_user_echo(&mut tui, &line)?;
    page_notice(&mut tui, crate::help::help_text())?;
    continue;
}
```

(`page_notice` renders the block as a dim system message — consistent with how `/watch` feedback is shown. The multi-line string renders across lines via the existing `ansi_to_text` path.)

- [ ] **Step 8: Add the welcome hint** — in `src/tui/welcome.rs`, in BOTH `render_welcome_identity` and `render_welcome`, append a final dim line containing `crate::help::HELP_HINT` after the existing box content. Match the existing dim/notice styling used elsewhere in the file (e.g. `Style::default().add_modifier(Modifier::DIM)` or the file's established dim color). Keep the box width logic intact — the hint is a separate `Line` appended to the returned `Text`, not inside the bordered box.

- [ ] **Step 9: Build + full test**

Run: `cargo test -p usta && cargo build -p usta`
Expected: PASS, clean build (no warnings).

- [ ] **Step 10: Manual smoke** (interactive — defer to human if no TTY)

`cargo run`: welcome box shows the dim "Type /help for shortcuts and commands." line. Type `/help` → the help block prints; it is NOT answered by Usta (no LLM turn). `/watch` and `/quit` still work.

- [ ] **Step 11: Commit**

```bash
git add src/help.rs src/main.rs src/tui/run.rs src/tui/welcome.rs
git commit -m "usta: /help komutu (klavye/slash/CLI kısa yolları) + welcome ipucu"
```

---

## Self-Review

**Spec coverage:** `help.rs` single source (Step 1,3), `mod help` (Step 4), `/help` intercept in both loops not sent to LLM (Steps 6,7), welcome hint in both renders (Step 8), content + is_help_command tests (Step 1). All spec sections covered. ✓

**Placeholder scan:** No TBD/TODO in shipped code (the `todo!()` are the intentional RED stubs, replaced in Step 3). ✓

**Type consistency:** `help_text() -> &'static str`, `is_help_command(&str) -> bool`, `HELP_HINT: &str` defined in Step 1/3 and referenced as `crate::help::…` in Steps 6–8 consistently. ✓
