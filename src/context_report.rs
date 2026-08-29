//! `/context` — deterministic context-window breakdown (spec F1). Exact
//! bytes from the LIVE session: the system prompt is parsed back through
//! brain::section_sizes (the assembly's own divider format — single-sourced,
//! never recomputed) and the history is classified by role and injection
//! prefix. Token figures are estimates and say so; the backend's last
//! reported usage is shown alongside because the gap between the two —
//! backend overhead, caching — is itself diagnostic. No LLM call anywhere
//! in this module.

use crate::anthropic::Message;

/// Rough tokens-from-bytes estimate; always labeled as such in the output.
fn est_tokens(bytes: usize) -> usize {
    bytes / 4
}

/// "1 turn" / "N turns" — singular grammar for the one-turn case instead of
/// always pluralizing.
fn pluralize_turns(n: usize) -> String {
    if n == 1 {
        "1 turn".to_string()
    } else {
        format!("{n} turns")
    }
}

/// `brain::section_sizes` deliberately excludes divider header lines and the
/// blank `join("\n\n")` separators from every section (see its doc comment)
/// — so the per-section rows never sum to the system prompt's total on
/// their own. Reported here as its own row instead of leaving that gap
/// unexplained, the way the history breakdown below already reconciles.
const SYSTEM_PROMPT_OVERHEAD_LABEL: &str = "(framing: dividers + blank-line separators)";

fn message_text(m: &Message) -> String {
    match &m.content {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Build the full report — pure string work over the live session state.
pub(crate) fn build(
    system: &str,
    history: &[Message],
    last_reported: Option<u64>,
    window: u64,
) -> String {
    let mut out = String::from(
        "context breakdown — exact bytes; token figures are estimates (bytes / 4)\n\n",
    );
    out.push_str(&format!(
        "system prompt: {} bytes (~{} tokens)\n",
        system.len(),
        est_tokens(system.len())
    ));
    let sizes = crate::brain::section_sizes(system);
    let attributed: usize = sizes.iter().map(|(_, bytes)| bytes).sum();
    for (label, bytes) in &sizes {
        out.push_str(&format!("  {label:<34} {bytes} bytes\n"));
    }
    let overhead = system.len().saturating_sub(attributed);
    out.push_str(&format!(
        "  {SYSTEM_PROMPT_OVERHEAD_LABEL:<34} {overhead} bytes\n"
    ));
    // History buckets: the user's own words, the mentor's replies, injected
    // file deliveries, and other shell-injected directives.
    let mut buckets: [(&str, usize, usize); 4] = [
        ("your messages", 0, 0),
        ("usta's replies", 0, 0),
        ("file deliveries", 0, 0),
        ("injected directives", 0, 0),
    ];
    let mut history_bytes = 0usize;
    for m in history {
        let text = message_text(m);
        history_bytes += text.len();
        let idx = if m.role == "assistant" {
            1
        } else if crate::file_feedback::is_delivery_turn(&text) {
            2
        } else if text.starts_with(crate::check::NOTE_PREFIX) {
            // A bare turn carrying only the remembered-verdict note (see
            // `check::VerifyMonitor::note`) is not a shell-injected
            // directive like `[EXAM MODE]` or a file delivery — its bulk
            // and its meaning are the learner's own words, with a one-line
            // status pixel prepended. Filing it under "injected directives"
            // would bury the learner's own message there instead.
            0
        } else if text.starts_with('[') {
            3
        } else {
            0
        };
        buckets[idx].1 += text.len();
        buckets[idx].2 += 1;
    }
    out.push_str(&format!(
        "history: {} bytes (~{} tokens) across {}\n",
        history_bytes,
        est_tokens(history_bytes),
        pluralize_turns(history.len())
    ));
    for (name, bytes, count) in &buckets {
        out.push_str(&format!(
            "  {name:<34} {bytes} bytes ({})\n",
            pluralize_turns(*count)
        ));
    }
    let total = system.len() + history_bytes;
    out.push_str(&format!(
        "total: {} bytes (~{} tokens) — window: {}k tokens\n",
        total,
        est_tokens(total),
        window / 1000
    ));
    match last_reported {
        Some(t) => out.push_str(&format!(
            "last call reported: {t} tokens (API side — the gap vs the estimate \
includes backend overhead and caching, and is itself diagnostic)"
        )),
        None => {
            out.push_str("last call reported: nothing yet — no usage on record for this session")
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys() -> String {
        "===== TODAY =====\n2026-08-29\n\n===== SOUL.md =====\nCORE RULE".to_string()
    }

    #[test]
    fn report_lists_sections_buckets_and_labels_the_estimate() {
        let history = vec![
            Message::user("hello there"),
            Message::assistant_raw(serde_json::Value::String("hi".into())),
            Message::user("[Files changed]\nFILE: src/main.rs (full contents)\nfn main() {}"),
            Message::user("[EXAM MODE — MOCK EXAM]\nTopic: rust."),
        ];
        let r = build(&sys(), &history, Some(131_072), 200_000);
        assert!(r.contains("estimates"));
        assert!(r.contains("TODAY"));
        assert!(r.contains("SOUL.md"));
        assert!(r.contains("your messages"));
        assert!(r.contains("usta's replies"));
        assert!(r.contains("file deliveries"));
        assert!(r.contains("injected directives"));
        assert!(
            r.contains("(1 turn)"),
            "each bucket counted exactly one turn, singular grammar"
        );
        assert!(r.contains("last call reported: 131072 tokens"));
        assert!(r.contains("across 4 turns"));
        assert!(r.contains("bytes"));
    }

    #[test]
    fn bare_turn_with_build_state_note_classifies_as_the_learners_own_message() {
        // A bare turn (no delivery riding with it) while the last verdict is
        // red starts with `[build state:` — the same leading bracket as a
        // genuine injected directive like `[EXAM MODE]`. Without the
        // dedicated check the leading-bracket rule files the WHOLE turn,
        // including the learner's own words, under "injected directives".
        let text = format!(
            "{} the last cargo check that ran was failing — first error: e. \
Nothing has re-verified the project since; do not treat the current step as \
complete until a later check comes back clean.]\n\nso is this part finished?",
            crate::check::NOTE_PREFIX
        );
        let history = vec![Message::user(&text)];
        let r = build(&sys(), &history, None, 200_000);
        let messages_line = r
            .lines()
            .find(|l| l.contains("your messages"))
            .expect("your messages row must exist");
        assert!(
            messages_line.contains("(1 turn)"),
            "the bare turn belongs to the learner's own message bucket: {messages_line}"
        );
        let injected_line = r
            .lines()
            .find(|l| l.contains("injected directives"))
            .expect("injected directives row must exist");
        assert!(
            injected_line.contains("(0 turns)"),
            "a note-prefixed bare turn must NOT be filed as an injected directive: {injected_line}"
        );
    }

    #[test]
    fn report_says_so_when_no_usage_was_reported() {
        // The introduction path drops context_tokens (known M11 gap) — the
        // report must say "nothing yet" instead of inventing a number.
        let r = build(&sys(), &[], None, 200_000);
        assert!(r.contains("nothing yet"));
    }

    /// Extracts the byte count printed for `label`'s row (the token right
    /// before the trailing "bytes" word), so tests can check reconciliation
    /// against the report's own printed numbers rather than recomputing.
    fn row_bytes(report: &str, label: &str) -> usize {
        let line = report
            .lines()
            .find(|l| l.contains(label))
            .unwrap_or_else(|| panic!("no row for label {label:?} in report:\n{report}"));
        line.split_whitespace()
            .rev()
            .nth(1)
            .and_then(|tok| tok.parse().ok())
            .unwrap_or_else(|| panic!("could not parse byte count from row: {line:?}"))
    }

    #[test]
    fn system_prompt_rows_plus_overhead_row_equal_the_total() {
        // Three real sections (spec: "fixture with at least three sections").
        let sys = "===== TODAY =====\n2026-08-29\n\n===== SOUL.md =====\nCORE\n\n===== USER.md =====\nPROFILE-BODY".to_string();
        let sizes = crate::brain::section_sizes(&sys);
        assert_eq!(sizes.len(), 3, "fixture must have exactly three sections");

        let r = build(&sys, &[], None, 200_000);
        let rows_sum: usize = sizes.iter().map(|(_, bytes)| bytes).sum::<usize>()
            + row_bytes(&r, SYSTEM_PROMPT_OVERHEAD_LABEL);
        assert_eq!(
            rows_sum,
            sys.len(),
            "per-section rows plus the overhead row must reconcile with the stated system prompt total"
        );
    }

    #[test]
    fn divider_less_prompt_rows_plus_overhead_row_equal_the_total() {
        // Regression for the embedded-fallback shape (finding 6): a system
        // prompt with NO divider at all — a single line, no trailing
        // newline, exactly the shape of `brain::FALLBACK_SYSTEM`. Before
        // the fix, the lone "(fallback)" section over-counted its own last
        // line by one byte with no header bytes anywhere to absorb the
        // slack, so the saturating overhead subtraction silently clamped to
        // 0 and the rows summed to ONE MORE than the stated total.
        let sys = "Sen Usta'sın: yaparak-öğrenmeyi yürüten senior bir mühendislik mentorusun."
            .to_string();
        assert!(
            !sys.contains("====="),
            "fixture must carry no divider at all"
        );
        assert!(
            !sys.ends_with('\n'),
            "fixture must have no trailing newline"
        );

        let sizes = crate::brain::section_sizes(&sys);
        assert_eq!(
            sizes.len(),
            1,
            "a divider-less prompt is exactly one (fallback) section"
        );

        let r = build(&sys, &[], None, 200_000);
        let rows_sum: usize = sizes.iter().map(|(_, bytes)| bytes).sum::<usize>()
            + row_bytes(&r, SYSTEM_PROMPT_OVERHEAD_LABEL);
        assert_eq!(
            rows_sum,
            sys.len(),
            "per-section rows plus the overhead row must reconcile exactly, even with zero header bytes to hide a miscount"
        );
    }

    #[test]
    fn divider_shaped_line_inside_a_section_body_is_flagged_and_still_reconciles() {
        // GOAL.md-style hazard: a hand-edited file contains a bare line
        // shaped exactly like a section divider, mid-body.
        let sys = "===== TODAY =====\n2026-08-29\n\n===== GOAL.md =====\nbefore\n===== TODAY =====\nafter".to_string();

        let r = build(&sys, &[], None, 200_000);
        assert!(
            r.contains("ANOMALY"),
            "a divider-shaped line found inside a section body must be visibly flagged, not silently mislabeled:\n{r}"
        );

        let sizes = crate::brain::section_sizes(&sys);
        let rows_sum: usize = sizes.iter().map(|(_, bytes)| bytes).sum::<usize>()
            + row_bytes(&r, SYSTEM_PROMPT_OVERHEAD_LABEL);
        assert_eq!(
            rows_sum,
            sys.len(),
            "bytes from the colliding line must still be counted and still reconcile with the total"
        );
    }
}
