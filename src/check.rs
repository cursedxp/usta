//! Post-record `cargo check` — the raw material for the guess protocol. The result
//! goes to the LLM as a "for your eyes only" block; USTA.md rules decide when it
//! gets revealed to the user (by having them guess first).
//! If it's not a Cargo project / check can't run, it's silently skipped — the
//! feedback flow is never blocked.

use std::path::Path;
use std::time::Duration;

use tokio::process::Command;

/// Output cap — so massive error lists don't bloat the context.
pub const MAX_CHECK_BYTES: usize = 4 * 1024;

/// Check time cap — the first check can take a while with a cold cache.
const CHECK_TIMEOUT: Duration = Duration::from_secs(60);

/// Is there a Cargo.toml in the project root?
pub fn is_cargo_project(root: &Path) -> bool {
    root.join("Cargo.toml").is_file()
}

/// Trim the output to the cap — respecting UTF-8 char boundaries; note it if trimmed.
pub fn truncate_output(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut cut = max;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n… (truncated — {} bytes total)", &s[..cut], s.len())
}

/// Run `cargo check --message-format=short`. If it's not a Cargo project,
/// cargo can't run, or it hits a timeout, returns `None` — the guess protocol
/// is skipped for that record, and feedback flows normally.
pub async fn run_check(root: &Path) -> Option<String> {
    if !is_cargo_project(root) {
        return None;
    }
    let fut = Command::new("cargo")
        .arg("check")
        .arg("--message-format=short")
        .current_dir(root)
        .output();
    let output = tokio::time::timeout(CHECK_TIMEOUT, fut).await.ok()?.ok()?;
    if output.status.success() {
        return Some("CLEAN — cargo check passed with no errors.".to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Some(truncate_output(stderr.trim(), MAX_CHECK_BYTES))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_cargo_project_true_when_manifest_exists() {
        let base = std::env::temp_dir().join(format!(
            "usta_check_test_manifest_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("Cargo.toml"), "[package]").unwrap();
        assert!(is_cargo_project(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn is_cargo_project_false_without_manifest() {
        let base = std::env::temp_dir().join(format!(
            "usta_check_test_nomanifest_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        assert!(!is_cargo_project(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn truncate_passes_short_output_through() {
        assert_eq!(truncate_output("kısa", 100), "kısa");
    }

    #[test]
    fn truncate_cuts_long_output_with_note() {
        let long = "a".repeat(200);
        let out = truncate_output(&long, 100);
        assert!(out.len() < 200);
        assert!(out.contains("truncated"));
    }

    #[test]
    fn truncate_respects_utf8_char_boundary() {
        // "ö" is 2 bytes — if the cap lands in the middle of a char, it must not panic.
        let s = "ööööö";
        let out = truncate_output(s, 3);
        assert!(out.contains("truncated"));
    }
}
