//! Pluggable LLM backend: local `claude` CLI (default) or the Anthropic API.
//!
//! - **CLI (default):** `claude -p ...` — uses Claude Code's existing auth,
//!   no API key / token billing. `--allowedTools WebSearch` both enables research
//!   and enforces "Usta doesn't edit files" at the tool level.
//! - **API (optional):** the existing `anthropic::Client` reqwest path.
//!
//! Selection: `USTA_BACKEND` env (`cli`/`api`) takes priority; otherwise CLI if
//! `claude` is on PATH, otherwise API if `ANTHROPIC_API_KEY` is set, else a clear error.

use anyhow::{bail, Context, Result};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::anthropic::{self, Message};

/// A single completion result — text + web hint + context fullness.
pub struct Reply {
    pub text: String,
    pub web: bool,
    /// Total context tokens of the last call (input + cache) — for the indicator.
    pub context_tokens: Option<u64>,
}

/// Default model for the CLI backend. `claude -p --model opus`.
pub const DEFAULT_CLI_MODEL: &str = "opus";

/// Available LLM backends.
pub enum Backend {
    /// Shells out to the local `claude` CLI — Claude Code auth, no key.
    /// `session_id`: captured from the first response; subsequent turns are
    /// continued with `--resume` → the full transcript isn't resent each time.
    Cli {
        model: String,
        session_id: Option<String>,
    },
    /// Anthropic Messages API — reqwest, requires a key.
    Api {
        client: anthropic::Client,
        model: String,
    },
}

/// Select a backend based on environment signals.
pub fn select() -> Result<Backend> {
    match std::env::var("USTA_BACKEND").ok().as_deref() {
        Some("cli") => Ok(cli_backend()),
        Some("api") => api_backend(),
        Some(other) => bail!(
            "USTA_BACKEND invalid: '{other}'. Valid values: 'cli' or 'api'."
        ),
        None => {
            if claude_on_path() {
                Ok(cli_backend())
            } else if std::env::var("ANTHROPIC_API_KEY")
                .ok()
                .is_some_and(|k| !k.trim().is_empty())
            {
                api_backend()
            } else {
                bail!(
                    "No LLM backend found. One of two options is required:\n  \
                     1) Add the `claude` CLI to PATH (uses Claude Code auth, no key needed), or\n  \
                     2) export ANTHROPIC_API_KEY=sk-ant-... (Anthropic API path).\n  \
                     To force a backend: export USTA_BACKEND=cli|api"
                )
            }
        }
    }
}

fn cli_backend() -> Backend {
    Backend::Cli {
        model: DEFAULT_CLI_MODEL.to_string(),
        session_id: None,
    }
}

fn api_backend() -> Result<Backend> {
    let key = crate::config::resolve_key(std::env::var("ANTHROPIC_API_KEY").ok())?;
    Ok(Backend::Api {
        client: anthropic::Client::new(key),
        model: anthropic::DEFAULT_MODEL.to_string(),
    })
}

/// Is the `claude` executable on PATH?
fn claude_on_path() -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join("claude");
        candidate.is_file()
    })
}

impl Backend {
    /// Model label to show in the banner.
    pub fn label(&self) -> String {
        match self {
            Backend::Cli { model, .. } => format!("{model} · cli"),
            Backend::Api { model, .. } => format!("{model} · api"),
        }
    }

    /// Reset the CLI server session — after compaction, the next call opens
    /// a NEW session with the compacted history. No-op on the API backend.
    pub fn reset_session(&mut self) {
        if let Backend::Cli { session_id, .. } = self {
            *session_id = None;
        }
    }

    /// The model's context window (tokens). The window varies by model: Haiku
    /// 200k, the others (opus / sonnet / fable) 1M. The indicator and compaction
    /// threshold are scaled to this base — not fixed.
    pub fn context_window(&self) -> u64 {
        let model = match self {
            Backend::Cli { model, .. } => model.as_str(),
            Backend::Api { model, .. } => model.as_str(),
        };
        if model.contains("haiku") {
            200_000
        } else {
            1_000_000
        }
    }

    /// Request a completion from the selected backend, returns `Reply`.
    /// In CLI mode web usage can't be detected from the text → `false`.
    pub async fn complete(&mut self, system: &str, history: &[Message]) -> Result<Reply> {
        match self {
            Backend::Api { client, model } => {
                let (text, web, tokens) = client.complete(model, system, history).await?;
                Ok(Reply { text, web, context_tokens: tokens })
            }
            Backend::Cli { model, session_id } => {
                let resume = session_id.clone();
                let input = match &resume {
                    Some(_) => last_user_text(history),
                    None => render_transcript(history),
                };
                let attempt = run_claude_cli(model, system, &input, resume.as_deref()).await;
                let (text, new_sid, tokens) = match attempt {
                    Ok(v) => v,
                    // Stale/deleted session — retry once from scratch with the full transcript.
                    Err(_) if resume.is_some() => {
                        *session_id = None;
                        run_claude_cli(model, system, &render_transcript(history), None).await?
                    }
                    Err(e) => return Err(e),
                };
                if new_sid.is_some() {
                    *session_id = new_sid;
                }
                Ok(Reply { text, web: false, context_tokens: tokens })
            }
        }
    }
}

/// Return the plain text of the LAST user message in history — on a resume
/// call the server-side session context already exists, only the new turn is sent.
fn last_user_text(history: &[Message]) -> String {
    history
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| match &m.content {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .unwrap_or_default()
}

/// Parse `claude -p --output-format json` output. If it's not JSON (old
/// version / unexpected output) fall back to raw text — continues without a session id.
pub fn parse_cli_output(stdout: &str) -> (String, Option<String>, Option<u64>) {
    #[derive(serde::Deserialize)]
    struct CliJson {
        result: Option<String>,
        session_id: Option<String>,
        usage: Option<serde_json::Value>,
    }
    match serde_json::from_str::<CliJson>(stdout) {
        Ok(j) => {
            let tokens = j.usage.as_ref().and_then(anthropic::sum_context_tokens);
            (j.result.unwrap_or_default(), j.session_id, tokens)
        }
        Err(_) => (stdout.trim().to_string(), None, None),
    }
}

/// Convert the conversation history into a plain-text transcript to be
/// written to the `claude` CLI's stdin. User = `[SEN]`, assistant = `[USTA]`.
fn render_transcript(history: &[Message]) -> String {
    let mut out = String::new();
    for msg in history {
        let label = if msg.role == "assistant" {
            "[USTA]"
        } else {
            "[SEN]"
        };
        // Stored turns are Value::String; otherwise fall back to raw JSON.
        let body = match &msg.content {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        out.push_str(label);
        out.push('\n');
        out.push_str(&body);
        out.push_str("\n\n");
    }
    out.trim_end().to_string()
}

/// Run the `claude -p` subprocess: input is written to stdin, JSON output is read back.
/// If `resume` is given, the server-side session is continued via `--resume <id>`.
async fn run_claude_cli(
    model: &str,
    system: &str,
    input: &str,
    resume: Option<&str>,
) -> Result<(String, Option<String>, Option<u64>)> {
    let mut cmd = Command::new("claude");
    cmd.arg("-p")
        .arg("--output-format")
        .arg("json")
        .arg("--append-system-prompt")
        .arg(system)
        .arg("--model")
        .arg(model)
        .arg("--allowedTools")
        .arg("WebSearch");
    if let Some(id) = resume {
        cmd.arg("--resume").arg(id);
    }
    // If the future is cancelled (double Ctrl-C), don't leave the child process orphaned.
    cmd.kill_on_drop(true);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("`claude` CLI failed to start — is it on PATH?")?;

    // Write the input to stdin, then close it (EOF).
    {
        let mut stdin = child
            .stdin
            .take()
            .context("failed to get claude CLI stdin")?;
        stdin
            .write_all(input.as_bytes())
            .await
            .context("failed to write to claude CLI stdin")?;
        stdin.shutdown().await.ok();
    }

    let output = child
        .wait_with_output()
        .await
        .context("error waiting for claude CLI output")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("claude CLI returned an error ({}): {}", output.status, stderr.trim());
    }

    Ok(parse_cli_output(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_window_is_1m_for_opus_200k_for_haiku() {
        let opus = Backend::Cli { model: "opus".into(), session_id: None };
        assert_eq!(opus.context_window(), 1_000_000);
        let haiku = Backend::Cli { model: "claude-haiku-4-5".into(), session_id: None };
        assert_eq!(haiku.context_window(), 200_000);
    }

    #[test]
    fn reset_session_clears_cli_session_id() {
        // reset_session() should set session_id to None — the learning session
        // after the slug mini-session must NOT resume it (spec B1).
        let mut b = Backend::Cli { model: "opus".into(), session_id: Some("sid-123".into()) };
        b.reset_session();
        let Backend::Cli { session_id, .. } = &b else { panic!("Cli bekleniyordu") };
        assert!(session_id.is_none());
    }

    #[test]
    fn transcript_labels_roles_and_orders() {
        let history = vec![
            Message::user("selam"),
            Message {
                role: "assistant".into(),
                content: serde_json::Value::String("merhaba, spek'in ne?".into()),
            },
            Message::user("bilmiyorum"),
        ];
        let t = render_transcript(&history);
        let expected = "[SEN]\nselam\n\n[USTA]\nmerhaba, spek'in ne?\n\n[SEN]\nbilmiyorum";
        assert_eq!(t, expected);
    }

    #[test]
    fn transcript_empty_history_is_empty() {
        assert_eq!(render_transcript(&[]), "");
    }

    #[test]
    fn parse_cli_output_reads_json_result_and_session() {
        let out = r#"{"type":"result","result":"merhaba","session_id":"abc-123","is_error":false}"#;
        let (text, sid, _) = parse_cli_output(out);
        assert_eq!(text, "merhaba");
        assert_eq!(sid, Some("abc-123".to_string()));
    }

    #[test]
    fn parse_cli_output_falls_back_to_plain_text() {
        let (text, sid, _) = parse_cli_output("  düz metin yanıt  ");
        assert_eq!(text, "düz metin yanıt");
        assert_eq!(sid, None);
    }

    #[test]
    fn parse_cli_output_reads_usage_tokens() {
        let out = r#"{"result":"m","session_id":"s1","usage":{"input_tokens":100,"cache_read_input_tokens":900}}"#;
        let (_, _, tokens) = parse_cli_output(out);
        assert_eq!(tokens, Some(1000));
    }

    #[test]
    fn parse_cli_output_tokens_none_when_usage_missing() {
        let out = r#"{"result":"m","session_id":"s1"}"#;
        let (_, _, tokens) = parse_cli_output(out);
        assert_eq!(tokens, None);
    }

    #[test]
    fn last_user_text_takes_final_user_message() {
        let history = vec![
            Message::user("ilk"),
            Message {
                role: "assistant".into(),
                content: serde_json::Value::String("yanıt".into()),
            },
            Message::user("son soru"),
        ];
        assert_eq!(last_user_text(&history), "son soru");
    }

    #[test]
    fn last_user_text_empty_history_is_empty() {
        assert_eq!(last_user_text(&[]), "");
    }
}
