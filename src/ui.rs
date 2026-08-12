//! Presentation layer: who's speaking should be obvious at a glance. ● (orange) = Usta,
//! ■ = user prompt, dim `·` = system notice. Usta's replies are
//! rendered as real markdown via termimad. The `is_plain` gate: if there's no TTY
//! or NO_COLOR is set, plain output with no ANSI — doesn't break pipes/tests.
//! Behavior doesn't live here — only appearance.

use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use termimad::MadSkin;

pub const ORANGE: &str = "\x1b[38;5;208m";
pub const DIM: &str = "\x1b[2m";
pub const YELLOW: &str = "\x1b[33m";
pub const RESET: &str = "\x1b[0m";

/// Is the TUI active? While true, notice/warn/Spinner don't print raw ANSI — they're
/// buffered/no-op so they don't collide with ratatui's inline viewport.
static TUI_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Notices accumulated while the TUI is active — run.rs flushes these to the
/// scrollback via page_notice.
static TUI_NOTICES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Set the flag when entering/leaving the TUI loop (main.rs is responsible for this).
pub fn set_tui_active(on: bool) {
    TUI_ACTIVE.store(on, Ordering::SeqCst);
}

fn tui_active() -> bool {
    TUI_ACTIVE.load(Ordering::SeqCst)
}

/// Take the accumulated TUI notices and empty the buffer.
pub fn drain_tui_notices() -> Vec<String> {
    match TUI_NOTICES.lock() {
        Ok(mut guard) => std::mem::take(&mut *guard),
        // No panic on the cleanup path even if poisoned — recover the lock.
        Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
    }
}

fn push_tui_notice(msg: String) {
    match TUI_NOTICES.lock() {
        Ok(mut guard) => guard.push(msg),
        Err(poisoned) => poisoned.into_inner().push(msg),
    }
}

/// Plain mode: stdout isn't a TTY, or the user requested NO_COLOR.
pub fn is_plain() -> bool {
    !std::io::stdout().is_terminal() || std::env::var_os("NO_COLOR").is_some()
}

/// Usta reply block: blank line + orange ● + markdown render + blank line.
pub fn print_usta_reply(reply: &str, web: bool) {
    if is_plain() {
        println!("Usta> {reply}");
        if web {
            println!("(🔎 web researched)");
        }
        return;
    }
    println!("\n{ORANGE}●{RESET}");
    let width = termimad::terminal_size().0.max(40) as usize;
    let skin = skin();
    let text = skin.text(reply, Some(width.saturating_sub(4)));
    // termimad adds a blank line before/after when it starts with a header/bold —
    // drop the leading/trailing blank lines so there's no gap right after `●`
    // (interior blank lines stay as paragraph separators).
    let block = format!("{text}");
    let lines: Vec<&str> = block.lines().collect();
    let start = lines.iter().position(|l| !l.trim().is_empty()).unwrap_or(0);
    let end = lines
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map(|i| i + 1)
        .unwrap_or(0);
    for line in &lines[start..end] {
        println!("  {line}");
    }
    if web {
        println!("{DIM}  🔎 web researched{RESET}");
    }
    println!();
}

/// Dim info line (stdout) — visually set apart from the main flow.
pub fn notice(msg: &str) {
    if tui_active() {
        // The TUI is printing its live viewport — buffer instead so raw ANSI doesn't collide.
        push_tui_notice(msg.to_string());
        return;
    }
    if is_plain() {
        println!("({msg})");
    } else {
        println!("{DIM}· {msg}{RESET}");
    }
}

/// Dim warning line (stderr).
pub fn warn(msg: &str) {
    if tui_active() {
        push_tui_notice(format!("⚠ {msg}"));
        return;
    }
    if is_plain() {
        eprintln!("({msg})");
    } else {
        eprintln!("{DIM}! {msg}{RESET}");
    }
}

/// Session opening line — topic + model + exit hint.
pub fn banner(topic: &str, model: &str) {
    if is_plain() {
        println!("Usta ready — topic: {topic} · model: {model}. (/quit to exit)");
        return;
    }
    println!("{ORANGE}● Usta{RESET} {DIM}— topic: {topic} · model: {model} · /quit to exit{RESET}");
}

/// Context fullness indicator — an 8-cell bar, yellow warning at ≥70%.
/// Not drawn at all if there's no token info or in plain mode (no noise).
pub fn context_gauge(tokens: Option<u64>, window: u64) {
    let Some(t) = tokens else { return };
    if is_plain() {
        return;
    }
    let ratio = (t as f64 / window as f64).min(1.0);
    let filled = ((ratio * 8.0).round() as usize).min(8);
    let bar = format!("{}{}", "▓".repeat(filled), "░".repeat(8 - filled));
    let color = if ratio >= 0.7 { YELLOW } else { DIM };
    println!("{color}  {bar} context {}k/{}k{RESET}", t / 1000, window / 1000);
}

/// One-line animation while waiting for the LLM. Draws nothing in plain mode.
/// When `stop` is called, the line is erased — the reply prints onto a clean slate.
pub struct Spinner {
    stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl Spinner {
    pub fn start(msg: &'static str) -> Spinner {
        // Also a no-op while the TUI is active: it has its own spinner (Status::Thinking),
        // if this one started too, the background print! task would clobber the viewport.
        if is_plain() || tui_active() {
            return Spinner { stop_tx: None, handle: None };
        }
        let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            const FRAMES: [&str; 4] = ["⠋", "⠙", "⠸", "⠴"];
            let mut i = 0usize;
            loop {
                print!("\r{DIM}{} {msg}{RESET}", FRAMES[i % FRAMES.len()]);
                let _ = std::io::Write::flush(&mut std::io::stdout());
                i += 1;
                tokio::select! {
                    _ = &mut rx => break,
                    _ = tokio::time::sleep(std::time::Duration::from_millis(120)) => {}
                }
            }
            // Erase the line — let the reply print onto a clean slate.
            print!("\r\x1b[2K");
            let _ = std::io::Write::flush(&mut std::io::stdout());
        });
        Spinner { stop_tx: Some(tx), handle: Some(handle) }
    }

    pub async fn stop(mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(h) = self.handle.take() {
            let _ = h.await;
        }
    }
}

/// Render Usta's reply into an ANSI-formatted String — the TUI path converts this
/// into a ratatui Text (tui::convert). The 2-space indent per line is the same
/// visual language as print_usta_reply.
pub fn render_markdown(md: &str, width: usize) -> String {
    let skin = skin();
    let text = skin.text(md, Some(width.saturating_sub(4)));
    format!("{text}")
        .lines()
        .map(|l| format!("  {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Markdown skin for Usta's replies: headers+bold orange, inline code green.
/// (The termimad API can change between versions — the target colors are in
/// Global Constraints; use the equivalent call.)
fn skin() -> MadSkin {
    use termimad::crossterm::style::Color;
    let mut skin = MadSkin::default();
    skin.set_headers_fg(Color::AnsiValue(208));
    skin.bold.set_fg(Color::AnsiValue(208));
    skin.inline_code.set_fg(Color::AnsiValue(114));
    skin
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_markdown_indents_and_keeps_content() {
        let out = render_markdown("**kalın** metin", 60);
        assert!(out.contains("kalın"));
        assert!(out.lines().all(|l| l.is_empty() || l.starts_with("  ")));
    }
}
