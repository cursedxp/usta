//! Slash-command cluster: watch, game, and exam handling — extracted from
//! `main.rs` (module split, Task 4). Pure helpers, independent of the session loop.

use std::path::Path;

use anyhow::Result;

use crate::history;
use crate::progress;
use crate::tokens;

/// Companion (file-watch feedback) slash command. Slash lines never reach the LLM.
#[derive(Debug, PartialEq)]
pub(crate) enum WatchCmd {
    On,
    Off,
    Toggle,
}

pub(crate) fn parse_watch_command(line: &str) -> Option<WatchCmd> {
    // Case-insensitive: /WATCH OFF, /Watch also work (forgiving slash commands).
    match line.trim().to_ascii_lowercase().as_str() {
        "/watch" => Some(WatchCmd::Toggle),
        "/watch on" => Some(WatchCmd::On),
        "/watch off" => Some(WatchCmd::Off),
        _ => None,
    }
}

pub(crate) fn apply_watch(cmd: WatchCmd, cur: bool) -> (bool, &'static str) {
    let next = match cmd {
        WatchCmd::On => true,
        WatchCmd::Off => false,
        WatchCmd::Toggle => !cur,
    };
    let msg = if next {
        "companion on — watching your files"
    } else {
        "companion paused — file feedback off"
    };
    (next, msg)
}

/// Gamification slash command (`/game`). Slash lines never reach the LLM.
#[derive(Debug)]
pub(crate) enum GameCmd {
    On,
    Off,
    Status,
}

pub(crate) fn parse_game_command(line: &str) -> Option<GameCmd> {
    let t = line.trim();
    if t == "/game" {
        return Some(GameCmd::Status);
    }
    let rest = t.strip_prefix("/game ")?;
    match rest.trim().to_ascii_lowercase().as_str() {
        "on" => Some(GameCmd::On),
        "off" => Some(GameCmd::Off),
        _ => None,
    }
}

/// Shell-managed preference line in USER.md (`tokens::H_PREFERENCES` section).
/// The closing flush is told to keep this section as-is.
pub(crate) fn game_pref(global: &Path) -> bool {
    std::fs::read_to_string(global.join("USER.md"))
        .map(|c| c.lines().any(|l| l.trim() == "- gamification: on"))
        .unwrap_or(false)
}

/// Opening `[GAME]` streak line, shell-computed from the global history log — only
/// when gamification is on. ADHD-safe: a broken streak (current 0) shows the LONGEST,
/// never `streak: 0`; no history at all yields None (no game line).
pub(crate) fn game_streak_line(global: &Path, today: &str) -> Option<String> {
    if !game_pref(global) {
        return None;
    }
    let content = std::fs::read_to_string(global.join("learner/history.md")).ok()?;
    let es = history::entries(&content);
    if es.is_empty() {
        return None;
    }
    let cur = history::current_streak(&es, today);
    let longest = history::longest_streak(&es);
    if cur > 0 {
        Some(format!("streak: {cur} day(s) (longest {longest})"))
    } else if longest > 0 {
        Some(format!("longest streak: {longest} day(s)"))
    } else {
        None
    }
}

/// Turn text for `/game on`. Embeds the GAMIFICATION.md rules directly so the
/// model doesn't have to recall them from a TEACHING.md reference — pure so it's
/// unit-testable without touching disk; callers read GAMIFICATION.md themselves.
/// Empty/whitespace-only rules (file missing/unreadable) fall back to the old
/// short instruction so a broken read never breaks the `/game on` turn.
pub(crate) fn game_on_turn(rules: &str) -> String {
    let rules = rules.trim();
    if rules.is_empty() {
        "[GAME MODE ON] Gamification is now ON — apply the Gamification rules from this point on."
            .to_string()
    } else {
        format!("[GAME MODE ON] Gamification is now ON — apply these rules from this point on:\n{rules}")
    }
}

pub(crate) fn set_game_pref(global: &Path, on: bool) -> Result<()> {
    let path = global.join("USER.md");
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let value = if on {
        "- gamification: on"
    } else {
        "- gamification: off"
    };
    let new = if content
        .lines()
        .any(|l| l.trim().starts_with("- gamification:"))
    {
        let had_trailing_newline = content.ends_with('\n');
        let mut rebuilt = content
            .lines()
            .map(|l| {
                if l.trim().starts_with("- gamification:") {
                    value
                } else {
                    l
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        if had_trailing_newline && !rebuilt.ends_with('\n') {
            rebuilt.push('\n');
        }
        rebuilt
    } else if content.contains(tokens::H_PREFERENCES) {
        content.replace(
            tokens::H_PREFERENCES,
            &format!("{}\n{value}", tokens::H_PREFERENCES),
        )
    } else {
        format!(
            "{}\n\n{}\n{value}\n",
            content.trim_end(),
            tokens::H_PREFERENCES
        )
    };
    progress::write_atomic(&path, &new)
}

/// Raw on-disk state of the shell-managed `- gamification:` line in USER.md.
/// `None` = no such line (user never toggled `/game`); `Some(true/false)` = line
/// present with value. Unlike `game_pref`, this distinguishes "line absent" from
/// "line present and off" — needed to know whether a restore should touch the file.
pub(crate) fn read_game_pref(global: &Path) -> Option<bool> {
    let content = std::fs::read_to_string(global.join("USER.md")).ok()?;
    content.lines().find_map(|l| {
        let v = l.trim().strip_prefix("- gamification:")?;
        Some(v.trim() == "on")
    })
}

/// Shell restore guarantee for the `/game` preference. `before` is the raw state
/// captured (via `read_game_pref`) BEFORE the closing flush rewrote USER.md. If the
/// model dropped the line or flipped its value, the captured value is written back.
/// `before == None` (user never toggled) is left strictly untouched — no line is added.
/// Returns whether a restore was performed. Pure: does exactly one conditional write.
pub(crate) fn restore_game_pref(global: &Path, before: Option<bool>) -> Result<bool> {
    let Some(want) = before else {
        return Ok(false);
    };
    if read_game_pref(global) == Some(want) {
        return Ok(false); // model honored the KEEP rule — nothing to do
    }
    set_game_pref(global, want)?;
    Ok(true)
}

/// Mock-exam slash command (`/exam`) — same exact-match pattern as `help::is_help_command`.
pub(crate) fn is_exam_command(line: &str) -> bool {
    line.trim() == "/exam"
}

/// Does this topic have a goal (tokens::H_GOAL)? Same approach-file priority as
/// brain.rs GOAL loading: project override wins over global — keep in sync.
pub(crate) fn topic_has_goal(project_root: &Path, global: &Path, topic: &str) -> bool {
    let override_path = progress::approach_path(project_root, topic);
    let path = if override_path.exists() {
        override_path
    } else {
        global.join("approaches").join(format!("{topic}.md"))
    };
    std::fs::read_to_string(path)
        .map(|c| c.contains(tokens::H_GOAL))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_exam_command_exact_only() {
        assert!(is_exam_command("/exam"));
        assert!(is_exam_command("  /exam  "));
        assert!(!is_exam_command("/exam now"));
        assert!(!is_exam_command("exam"));
        assert!(!is_exam_command("/examx"));
    }

    #[test]
    fn topic_has_goal_override_priority() {
        let base = std::env::temp_dir().join(format!("usta_exam_goal_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let project = base.join("proj");
        let global = base.join("global");
        std::fs::create_dir_all(project.join(".usta/approaches")).unwrap();
        std::fs::create_dir_all(global.join("approaches")).unwrap();

        // yalnız global hedefli → true
        std::fs::write(
            global.join("approaches/rust.md"),
            "yaklaşım\n## Goal\nsınav",
        )
        .unwrap();
        assert!(topic_has_goal(&project, &global, "rust"));

        // override VAR ama hedefsiz → override kazanır → false
        std::fs::write(
            project.join(".usta/approaches/rust.md"),
            "yaklaşım hedefsiz",
        )
        .unwrap();
        assert!(!topic_has_goal(&project, &global, "rust"));

        // override hedefli → true
        std::fs::write(project.join(".usta/approaches/rust.md"), "## Goal\nCEFR B2").unwrap();
        assert!(topic_has_goal(&project, &global, "rust"));

        // hiç dosya yok → false
        assert!(!topic_has_goal(&project, &global, "linux"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn parse_watch_command_variants() {
        assert_eq!(parse_watch_command("/watch"), Some(WatchCmd::Toggle));
        assert_eq!(parse_watch_command("/watch on"), Some(WatchCmd::On));
        assert_eq!(parse_watch_command("/watch off"), Some(WatchCmd::Off));
        assert_eq!(parse_watch_command("  /watch off  "), Some(WatchCmd::Off));
        assert_eq!(parse_watch_command("/WATCH OFF"), Some(WatchCmd::Off));
        assert_eq!(parse_watch_command("/Watch"), Some(WatchCmd::Toggle));
        assert_eq!(parse_watch_command("hello"), None);
        assert_eq!(parse_watch_command("/quit"), None);
    }

    #[test]
    fn apply_watch_transitions() {
        assert!(!apply_watch(WatchCmd::Off, true).0);
        assert!(apply_watch(WatchCmd::On, false).0);
        assert!(!apply_watch(WatchCmd::Toggle, true).0);
        assert!(apply_watch(WatchCmd::Toggle, false).0);
        assert!(apply_watch(WatchCmd::On, false).1.contains("on"));
        assert!(apply_watch(WatchCmd::Off, true).1.contains("off"));
    }

    #[test]
    fn parse_game_command_variants() {
        assert!(matches!(parse_game_command("/game on"), Some(GameCmd::On)));
        assert!(matches!(
            parse_game_command(" /game OFF "),
            Some(GameCmd::Off)
        ));
        assert!(matches!(parse_game_command("/game"), Some(GameCmd::Status)));
        assert!(parse_game_command("/game x").is_none());
        assert!(parse_game_command("/gamer").is_none());
        assert!(parse_game_command("game on").is_none());
    }

    #[test]
    fn game_on_turn_embeds_rules_or_falls_back_when_empty() {
        let turn = game_on_turn("- **XP** ... · **Levels** ...");
        assert!(turn.contains("- **XP** ... · **Levels** ..."));

        let fallback = game_on_turn("");
        assert_eq!(
            fallback,
            "[GAME MODE ON] Gamification is now ON — apply the Gamification rules from this point on."
        );
        let fallback_whitespace = game_on_turn("   \n  ");
        assert_eq!(fallback_whitespace, fallback);
    }

    #[test]
    fn game_pref_roundtrip_idempotent_preserves_user_md() {
        let base = std::env::temp_dir().join(format!("usta_game_pref_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(
            base.join("USER.md"),
            "# Öğrenci Profili\n\n## Kim\n- Anil\n",
        )
        .unwrap();

        assert!(!game_pref(&base)); // default off
        set_game_pref(&base, true).unwrap();
        assert!(game_pref(&base));
        set_game_pref(&base, true).unwrap(); // idempotent
        let c = std::fs::read_to_string(base.join("USER.md")).unwrap();
        assert_eq!(c.matches("- gamification:").count(), 1);
        assert!(c.contains("## Kim")); // diğer içerik korunur
        assert!(c.contains("## Preferences"));
        assert!(c.ends_with('\n')); // trailing newline korunur (line-replace path)
        set_game_pref(&base, false).unwrap();
        assert!(!game_pref(&base));
        let c2 = std::fs::read_to_string(base.join("USER.md")).unwrap();
        assert!(c2.ends_with('\n')); // ikinci toggle'da da korunur

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn restore_game_pref_readds_dropped_line_and_keeps_other_content() {
        let base = std::env::temp_dir().join(format!("usta_restore_drop_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        // Before flush: preference is on.
        std::fs::write(
            base.join("USER.md"),
            "# Öğrenci Profili\n\n## Kim\n- Anil\n\n## Preferences\n- gamification: on\n",
        )
        .unwrap();
        let before = read_game_pref(&base);
        assert_eq!(before, Some(true));

        // Simulate the model rewriting the profile and DROPPING the preference line.
        std::fs::write(
            base.join("USER.md"),
            "# Öğrenci Profili\n\n## Kim\n- Anil (güncel)\n",
        )
        .unwrap();

        let restored = restore_game_pref(&base, before).unwrap();
        assert!(restored); // restore happened
        let c = std::fs::read_to_string(base.join("USER.md")).unwrap();
        assert!(c.contains("- gamification: on")); // line is back
        assert!(c.contains("## Kim")); // other (rewritten) profile content preserved
        assert!(c.contains("Anil (güncel)"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn restore_game_pref_reverts_flipped_value() {
        let base = std::env::temp_dir().join(format!("usta_restore_flip_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(
            base.join("USER.md"),
            "# Öğrenci Profili\n\n## Preferences\n- gamification: on\n",
        )
        .unwrap();
        let before = read_game_pref(&base);
        assert_eq!(before, Some(true));

        // Model flipped the value to off.
        std::fs::write(
            base.join("USER.md"),
            "# Öğrenci Profili\n\n## Preferences\n- gamification: off\n",
        )
        .unwrap();

        let restored = restore_game_pref(&base, before).unwrap();
        assert!(restored);
        assert_eq!(read_game_pref(&base), Some(true)); // back to old value
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn restore_game_pref_none_before_is_untouched() {
        let base = std::env::temp_dir().join(format!("usta_restore_none_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        // User never toggled — no preference line at all.
        let body = "# Öğrenci Profili\n\n## Kim\n- Anil\n";
        std::fs::write(base.join("USER.md"), body).unwrap();
        let before = read_game_pref(&base);
        assert_eq!(before, None);

        let restored = restore_game_pref(&base, before).unwrap();
        assert!(!restored); // nothing done
        let c = std::fs::read_to_string(base.join("USER.md")).unwrap();
        assert_eq!(c, body); // byte-for-byte untouched, no line added
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn game_streak_line_never_shows_streak_zero() {
        let base = std::env::temp_dir().join(format!("usta_game_streak_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("learner")).unwrap();

        // game OFF (no USER.md at all) → None regardless of history.
        let hist = format!(
            "{}\n{}\n{}\n",
            history::record_line("2026-08-14", "rust", None, None),
            history::record_line("2026-08-15", "rust", None, None),
            history::record_line("2026-08-16", "rust", None, None),
        );
        std::fs::write(base.join("learner/history.md"), &hist).unwrap();
        assert_eq!(game_streak_line(&base, "2026-08-16"), None);

        // game ON, but no history file (or empty) → None.
        std::fs::write(
            base.join("USER.md"),
            "# Öğrenci Profili\n\n## Preferences\n- gamification: on\n",
        )
        .unwrap();
        std::fs::remove_file(base.join("learner/history.md")).unwrap();
        assert_eq!(game_streak_line(&base, "2026-08-16"), None);

        // game ON + current streak > 0 → Some("streak: N day(s) (longest M)"), never "streak: 0".
        std::fs::write(base.join("learner/history.md"), &hist).unwrap();
        let s = game_streak_line(&base, "2026-08-16").unwrap();
        assert_eq!(s, "streak: 3 day(s) (longest 3)");
        assert!(!s.contains("streak: 0"));

        // game ON + broken streak (gap before today, longest > 0) → Some("longest streak: M day(s)").
        let broken = game_streak_line(&base, "2026-08-19").unwrap();
        assert_eq!(broken, "longest streak: 3 day(s)");
        assert!(!broken.contains("streak: 0"));
        assert!(broken.starts_with("longest"));

        let _ = std::fs::remove_dir_all(&base);
    }
}
