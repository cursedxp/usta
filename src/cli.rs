//! CLI command enum and argument parser — extracted from `main.rs` (module
//! split, Task 3).

use anyhow::Result;

use crate::topic::slugify_topic;

/// Reset scope.
#[derive(Debug, PartialEq)]
pub enum ResetTarget {
    /// Just one topic's progress in the current project.
    Topic(String),
    /// All known project `.usta/`s + global brain — zero point.
    Factory,
    /// Global user profile — reverts to the embedded generic template (with backup).
    Profile,
}

/// Command-line command — argument parsing in one place, pure and testable.
#[derive(Debug, PartialEq)]
pub enum Command {
    /// `usta init` — set up the scaffold, print per-file status.
    Init,
    /// `usta topics` — topic list from the global catalog.
    Topics,
    /// `usta stats` — this week's summary + streaks (ADHD-safe framing).
    Stats,
    /// `usta reset <topic>` — delete progress (with confirmation) + drop from the catalog.
    Reset(ResetTarget),
    /// `usta` / `usta start [topic]` — learning session.
    Start(Option<String>),
}

/// Turn arguments into a command. Unknown command → clear error, no silent surprises.
pub fn parse_command(args: &[String]) -> Result<Command> {
    let mut rest = args.iter().skip(1);
    match rest.next().map(String::as_str) {
        None => Ok(Command::Start(None)),
        Some("start") => Ok(Command::Start(rest.next().cloned())),
        Some("init") => Ok(Command::Init),
        Some("topics") => Ok(Command::Topics),
        Some("stats") => Ok(Command::Stats),
        Some("reset") => match rest.next().map(String::as_str) {
            Some("--factory") => Ok(Command::Reset(ResetTarget::Factory)),
            Some("--profile") | Some("--profil") => Ok(Command::Reset(ResetTarget::Profile)),
            Some(topic) => Ok(Command::Reset(ResetTarget::Topic(slugify_topic(topic)))),
            None => anyhow::bail!("usage: usta reset <topic>  |  --factory  |  --profile"),
        },
        Some(other) => anyhow::bail!(
            "unknown command: '{other}'. Commands: start [topic], init, topics, stats, reset <topic>|--factory|--profile"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bare_is_start_without_topic() {
        let args = vec!["usta".to_string()];
        assert_eq!(parse_command(&args).unwrap(), Command::Start(None));
    }

    #[test]
    fn parse_start_keeps_topic_arg() {
        let args = vec!["usta".into(), "start".into(), "javascript".into()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Start(Some("javascript".to_string()))
        );
    }

    #[test]
    fn parse_start_without_arg_is_start_none() {
        let args = vec!["usta".into(), "start".into()];
        assert_eq!(parse_command(&args).unwrap(), Command::Start(None));
    }

    #[test]
    fn parse_init_and_topics() {
        assert_eq!(
            parse_command(&["usta".into(), "init".into()]).unwrap(),
            Command::Init
        );
        assert_eq!(
            parse_command(&["usta".into(), "topics".into()]).unwrap(),
            Command::Topics
        );
    }

    #[test]
    fn parse_stats() {
        assert_eq!(
            parse_command(&["usta".into(), "stats".into()]).unwrap(),
            Command::Stats
        );
    }

    #[test]
    fn parse_unknown_command_errors() {
        assert!(parse_command(&["usta".into(), "rust".into()]).is_err());
    }

    #[test]
    fn parse_reset_topic_is_slugified() {
        let args = vec!["usta".into(), "reset".into(), "C++".into()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Reset(ResetTarget::Topic("c".to_string()))
        );
    }

    #[test]
    fn parse_reset_without_arg_errors() {
        assert!(parse_command(&["usta".into(), "reset".into()]).is_err());
    }

    #[test]
    fn parse_reset_factory_flag() {
        let args = vec!["usta".into(), "reset".into(), "--factory".into()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Reset(ResetTarget::Factory)
        );
    }

    #[test]
    fn parse_reset_profile_flag_both_spellings() {
        let args = |s: &str| vec!["usta".to_string(), "reset".to_string(), s.to_string()];
        assert_eq!(parse_command(&args("--profile")).unwrap(), Command::Reset(ResetTarget::Profile));
        assert_eq!(parse_command(&args("--profil")).unwrap(), Command::Reset(ResetTarget::Profile));
        // Regression: topic and factory unchanged.
        assert_eq!(parse_command(&args("--factory")).unwrap(), Command::Reset(ResetTarget::Factory));
        assert!(matches!(parse_command(&args("rust")).unwrap(), Command::Reset(ResetTarget::Topic(t)) if t == "rust"));
    }
}
