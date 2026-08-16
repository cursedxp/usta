//! In-session /help: keyboard shortcuts, slash commands, and CLI commands.
//! Single source of truth referenced by both session loops and the welcome box.

/// One-line discovery hint shown under the welcome box.
pub const HELP_HINT: &str = "Type /help for shortcuts and commands.";

/// The full help block (English), printed when the user types `/help`.
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
     \x20\x20/show [topic]    animated visual explainer (opens in browser)\n\
     \x20\x20/exam            goal mode: timed mock exam from your map\n\
     \x20\x20/game on|off     XP, levels, badges (ADHD-safe)\n\
     \x20\x20/help            this help\n\
     \x20\x20/quit            end the session\n\
     \n\
     Terminal commands\n\
     \x20\x20usta                    start — asks for the topic\n\
     \x20\x20usta start <topic>      start a specific topic\n\
     \x20\x20usta topics             list what you're learning where\n\
     \x20\x20usta stats              this week + streaks\n\
     \x20\x20usta reset <topic>      reset a topic's progress in this project\n\
     \x20\x20usta reset --profile    reset only your profile\n\
     \x20\x20usta reset --factory    reset everything"
}

/// True when the input line is exactly the `/help` command (trimmed,
/// case-insensitive — forgiving slash commands: `/HELP`, `/Help` also work).
pub fn is_help_command(line: &str) -> bool {
    line.trim().eq_ignore_ascii_case("/help")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_help_command_matches_only_bare_help() {
        assert!(is_help_command("/help"));
        assert!(is_help_command("  /help  "));
        assert!(is_help_command("/HELP"));
        assert!(is_help_command("/Help"));
        assert!(!is_help_command("/help me"));
        assert!(!is_help_command("help"));
        assert!(!is_help_command("/quit"));
        assert!(!is_help_command(""));
    }

    #[test]
    fn help_text_lists_shortcuts_commands_and_cli() {
        let h = help_text();
        for needle in [
            "Ctrl+J",
            "Esc",
            "↑ / ↓",
            "/watch on|off",
            "/show [topic]",
            "/exam",
            "/game on|off",
            "/help",
            "/quit",
            "usta reset --factory",
            "usta topics",
            "usta stats",
        ] {
            assert!(h.contains(needle), "help_text missing: {needle}");
        }
    }
}
