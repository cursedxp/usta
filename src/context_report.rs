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
    for (label, bytes) in crate::brain::section_sizes(system) {
        out.push_str(&format!("  {label:<34} {bytes} bytes\n"));
    }
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
        } else if text.starts_with('[') {
            3
        } else {
            0
        };
        buckets[idx].1 += text.len();
        buckets[idx].2 += 1;
    }
    out.push_str(&format!(
        "history: {} bytes (~{} tokens) across {} turns\n",
        history_bytes,
        est_tokens(history_bytes),
        history.len()
    ));
    for (name, bytes, count) in &buckets {
        out.push_str(&format!("  {name:<34} {bytes} bytes ({count} turns)\n"));
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
            r.contains("(1 turns)"),
            "each bucket counted exactly one turn"
        );
        assert!(r.contains("last call reported: 131072 tokens"));
        assert!(r.contains("across 4 turns"));
        assert!(r.contains("bytes"));
    }

    #[test]
    fn report_says_so_when_no_usage_was_reported() {
        // The introduction path drops context_tokens (known M11 gap) — the
        // report must say "nothing yet" instead of inventing a number.
        let r = build(&sys(), &[], None, 200_000);
        assert!(r.contains("nothing yet"));
    }
}
