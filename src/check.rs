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

/// One verification verdict, parsed from raw checker output. The names are
/// deliberately NEUTRAL (spec C1): the concept is "the project's own
/// verification signal, when it has one" — the Cargo check is merely
/// today's only implementation, and adding another later is a new arm, not
/// a redesign. No verifier registry, no per-language detection (YAGNI).
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Pass,
    Fail { summary: String },
}

/// Classify raw `run_check` output. The clean case is pinned to
/// run_check's own sentence; anything else fails, with a one-line summary.
pub fn verdict_of(raw: &str) -> Verdict {
    if raw.starts_with("CLEAN") {
        Verdict::Pass
    } else {
        Verdict::Fail {
            summary: error_summary(raw),
        }
    }
}

/// First error-carrying line of a failing check, capped for a one-line note.
pub fn error_summary(raw: &str) -> String {
    let line = raw
        .lines()
        .find(|l| l.contains("error"))
        .or_else(|| raw.lines().find(|l| !l.trim().is_empty()))
        .unwrap_or("(no output)")
        .trim();
    truncate_output(line, 200)
}

/// Shell memory of the project's verification signal (spec C2): the check
/// keeps running exactly where it runs today — at deliveries that carry a
/// non-exercise file, and on the live path — and this REMEMBERS the verdict
/// so the mentor can no longer forget a red project between deliveries
/// (finding C: turns 14–15 declared a step finished while the project did
/// not compile). Level-triggered where v0.28.0 was edge-triggered; zero new
/// check executions. For a project with no verifier the monitor is disabled
/// and every method is a silent no-op — zero behavior change (spec C1).
/// Never opens a turn (K1): everything it produces is a status-line pixel
/// or a line attached to a turn the user already opened.
pub struct VerifyMonitor {
    enabled: bool,
    verdict: Option<Verdict>,
}

impl VerifyMonitor {
    pub fn new(project_root: &Path) -> Self {
        VerifyMonitor {
            enabled: is_cargo_project(project_root),
            verdict: None,
        }
    }

    /// Remember the verdict of a check that actually ran. Disabled → no-op
    /// (belt: run_check already returns None for non-Cargo roots).
    pub fn record(&mut self, raw: &str) {
        if self.enabled {
            self.verdict = Some(verdict_of(raw));
        }
    }

    /// Last KNOWN state is failing — drives the dim status marker.
    pub fn is_failing(&self) -> bool {
        matches!(self.verdict, Some(Verdict::Fail { .. }))
    }

    /// One-line state note while the last known verdict is red; None
    /// otherwise. Carried on EVERY delivered turn until a later check comes
    /// back clean. The instruction rides INSIDE the note, so flow_frame
    /// needs no extra rule (prompt diet). Honest about staleness: "as of
    /// the last delivered change".
    pub fn note(&self) -> Option<String> {
        match &self.verdict {
            Some(Verdict::Fail { summary }) if self.enabled => Some(format!(
                "[build state: the last cargo check was still failing — first error: {summary}. \
The project did not compile as of the last delivered change; do not treat the \
current step as complete until a later check comes back clean.]"
            )),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_cargo_project_true_when_manifest_exists() {
        let base =
            std::env::temp_dir().join(format!("usta_check_test_manifest_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("Cargo.toml"), "[package]").unwrap();
        assert!(is_cargo_project(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn is_cargo_project_false_without_manifest() {
        let base =
            std::env::temp_dir().join(format!("usta_check_test_nomanifest_{}", std::process::id()));
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

    #[test]
    fn verdict_of_classifies_clean_and_failing_output() {
        // run_check's own clean sentence is the Pass pin.
        assert_eq!(
            verdict_of("CLEAN — cargo check passed with no errors."),
            Verdict::Pass
        );
        let raw = "src/main.rs:3:18: error[E0308]: mismatched types: expected `i32`, found `()`\nwarning: unused";
        match verdict_of(raw) {
            Verdict::Fail { summary } => {
                assert!(summary.contains("error[E0308]"));
                assert!(summary.contains("mismatched types"));
            }
            Verdict::Pass => panic!("failing output must classify as Fail"),
        }
    }

    #[test]
    fn error_summary_picks_first_error_line_and_caps_it() {
        let raw = "warning: something\nsrc/main.rs:3:18: error[E0308]: mismatched types\nsrc/x.rs:9:1: error[E0425]: not found";
        let s = error_summary(raw);
        assert!(s.contains("error[E0308]"));
        assert!(!s.contains("E0425"), "one line, the FIRST error");
        // No error-looking line at all → first non-empty line, still capped.
        let long = format!("prelude {}", "x".repeat(400));
        let capped = error_summary(&long);
        assert!(capped.len() < 250);
        assert!(capped.contains("truncated"));
        assert_eq!(error_summary(""), "(no output)");
    }

    /// Scratch dirs for monitor tests — with/without a Cargo manifest.
    fn monitor_scratch(tag: &str, cargo: bool) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("usta_verify_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if cargo {
            std::fs::write(dir.join("Cargo.toml"), "[package]").unwrap();
        }
        dir
    }

    #[test]
    fn verify_monitor_remembers_the_last_verdict() {
        // Finding C's fix in one test: the check already produces the truth;
        // the bug was forgetting it the moment a delivery ended. Level-
        // triggered memory, zero new executions.
        let dir = monitor_scratch("remember", true);
        let mut m = VerifyMonitor::new(&dir);
        assert!(!m.is_failing());
        assert!(m.note().is_none(), "no verdict yet — silence");
        m.record("src/main.rs:3:18: error[E0308]: mismatched types");
        assert!(m.is_failing());
        let note = m.note().expect("red state must produce the note");
        assert!(note.starts_with("[build state:"));
        assert!(note.contains("error[E0308]"));
        assert!(note.contains("do not treat the current step as complete"));
        // Superseded only by the next REAL check (spec C2).
        m.record("CLEAN — cargo check passed with no errors.");
        assert!(!m.is_failing());
        assert!(m.note().is_none(), "green state never nags");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_monitor_is_a_silent_no_op_without_a_verifier() {
        // Spec C1 lock: Usta is domain-agnostic — for any non-Cargo project
        // (and every non-software domain) the WHOLE feature must be a
        // silent no-op: no marker, no note, zero behavior change.
        let dir = monitor_scratch("noop", false);
        let mut m = VerifyMonitor::new(&dir);
        m.record("src/main.rs:1:1: error[E0308]: mismatched types");
        assert!(!m.is_failing());
        assert!(m.note().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
