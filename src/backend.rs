//! Pluggable LLM backend: local `claude` CLI (default) or the Anthropic API.
//!
//! - **CLI (default):** `claude -p ...` — uses Claude Code's existing auth,
//!   no API key / token billing. `--allowedTools WebSearch,WebFetch` enables
//!   research and reading a linked page, and enforces "Usta doesn't edit files"
//!   at the tool level.
//! - **API (optional):** the existing `anthropic::Client` reqwest path.
//!
//! Selection: `USTA_BACKEND` env (`cli`/`api`) takes priority; otherwise CLI if
//! `claude` is found (PATH or its install folders, see `claude_bin`),
//! otherwise API if `ANTHROPIC_API_KEY` is set, else a clear error.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
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
    /// The model hit the output budget — `text` is cut off (API `stop_reason:
    /// max_tokens`). Always false on the CLI: its output-limit hit exits 1 → Err.
    pub truncated: bool,
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
        /// Resolved `claude` executable (`claude_bin::find_claude`), or the
        /// bare name `claude` when `USTA_BACKEND=cli` forced it unfound.
        bin: PathBuf,
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
        Some("cli") => Ok(cli_backend(
            crate::claude_bin::find_claude().unwrap_or_else(|| PathBuf::from("claude")),
        )),
        Some("api") => api_backend(),
        Some(other) => bail!("USTA_BACKEND invalid: '{other}'. Valid values: 'cli' or 'api'."),
        None => {
            if let Some(bin) = crate::claude_bin::find_claude() {
                Ok(cli_backend(bin))
            } else if std::env::var("ANTHROPIC_API_KEY")
                .ok()
                .is_some_and(|k| !k.trim().is_empty())
            {
                api_backend()
            } else {
                bail!(
                    "No LLM backend found. One of two options is required:\n  \
                     1) Install Claude Code (usta finds `claude` on PATH or in its install folders), or\n  \
                     2) export ANTHROPIC_API_KEY=sk-ant-... (Anthropic API path).\n  \
                     To force a backend: export USTA_BACKEND=cli|api"
                )
            }
        }
    }
}

fn cli_backend(bin: PathBuf) -> Backend {
    Backend::Cli {
        model: DEFAULT_CLI_MODEL.to_string(),
        session_id: None,
        bin,
    }
}

fn api_backend() -> Result<Backend> {
    let key = crate::config::resolve_key(std::env::var("ANTHROPIC_API_KEY").ok())?;
    Ok(Backend::Api {
        client: anthropic::Client::new(key),
        model: anthropic::DEFAULT_MODEL.to_string(),
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
        self.complete_with_limit(system, history, anthropic::MAX_TOKENS)
            .await
    }

    /// `complete` with an explicit output budget — the closing/compaction flush
    /// needs more room than a chat turn. The CLI backend ignores it.
    pub async fn complete_with_limit(
        &mut self,
        system: &str,
        history: &[Message],
        max_tokens: u32,
    ) -> Result<Reply> {
        match self {
            Backend::Api { client, model } => {
                let (text, web, tokens, truncated) =
                    client.complete(model, system, history, max_tokens).await?;
                Ok(Reply {
                    text,
                    web,
                    context_tokens: tokens,
                    truncated,
                })
            }
            Backend::Cli {
                model,
                session_id,
                bin,
            } => {
                let resume = session_id.clone();
                let (input, images) = match &resume {
                    Some(_) => (last_user_text(history), last_user_images(history)),
                    None => (render_transcript(history), history_images(history)),
                };
                let attempt =
                    run_claude_cli(bin, model, system, &input, &images, resume.as_deref()).await;
                let (text, new_sid, tokens) = match attempt {
                    Ok(v) => v,
                    // Stale/deleted session — retry once from scratch with the full transcript
                    // and every budgeted image (also recovers a session overgrown with images).
                    Err(_) if resume.is_some() => {
                        *session_id = None;
                        let transcript = render_transcript(history);
                        let images = history_images(history);
                        run_claude_cli(bin, model, system, &transcript, &images, None).await?
                    }
                    Err(e) => return Err(e),
                };
                if new_sid.is_some() {
                    *session_id = new_sid;
                }
                Ok(Reply {
                    text,
                    web: false,
                    context_tokens: tokens,
                    truncated: false,
                })
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
        .map(Message::text)
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

/// Image blocks of the LAST user message, for a resumed call (the server-side
/// session already holds earlier images).
fn last_user_images(history: &[Message]) -> Vec<serde_json::Value> {
    history
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(Message::image_blocks)
        .unwrap_or_default()
}

/// Every image still in (budgeted) history, for a no-resume call: per user message k
/// (1-based among user messages) that has images, one label text block then its images.
fn history_images(history: &[Message]) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for (k, m) in history.iter().filter(|m| m.role == "user").enumerate() {
        let imgs = m.image_blocks();
        if imgs.is_empty() {
            continue;
        }
        out.push(serde_json::json!({"type": "text", "text": format!("images attached to [SEN] message {}:", k + 1)}));
        out.extend(imgs);
    }
    out
}

/// The single stdin line for `--input-format stream-json`: images first, then the text.
fn stream_input(text: &str, images: &[serde_json::Value]) -> String {
    let mut content = images.to_vec();
    content.push(serde_json::json!({"type": "text", "text": text}));
    let mut line = serde_json::json!({
        "type": "user",
        "message": {"role": "user", "content": content}
    })
    .to_string();
    line.push('\n');
    line
}

/// `--output-format stream-json` prints NDJSON; the answer is the last `"type":"result"`
/// line, which carries the same `result` / `session_id` / `usage` as json mode.
pub fn parse_stream_output(stdout: &str) -> Result<(String, Option<String>, Option<u64>)> {
    let v = stdout
        .lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["type"] == "result")
        .context("claude CLI returned no result")?;
    if v["is_error"] == true {
        let why = v["result"]
            .as_str()
            .or(v["subtype"].as_str())
            .unwrap_or("unknown error");
        bail!("claude CLI error: {why}");
    }
    Ok(parse_cli_output(&v.to_string()))
}

/// On a non-zero exit in stream mode the useful message is the `is_error` result line on
/// stdout (stderr is empty). `None` -> fall back to the stderr-based exit-status error.
fn stream_failure(stream: bool, stdout: &str) -> Option<String> {
    if !stream {
        return None;
    }
    parse_stream_output(stdout)
        .err()
        .map(|e| e.to_string())
        .filter(|m| m.starts_with("claude CLI error:"))
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
        let body = msg.text();
        out.push_str(label);
        out.push('\n');
        out.push_str(&body);
        out.push_str("\n\n");
    }
    out.trim_end().to_string()
}

/// Tools the `claude -p` subprocess may use: research the web and read a
/// linked page. Nothing that touches files — "Usta doesn't edit" is enforced
/// here, at the tool level.
const CLI_ALLOWED_TOOLS: &str = "WebSearch,WebFetch";

/// Deletes the system-prompt file when the call ends (success, error or spawn failure).
struct PromptFile(PathBuf);

impl Drop for PromptFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Run the `claude -p` subprocess: input is written to stdin, JSON output is read back
/// (stream-json when `images` is non-empty).
/// If `resume` is given, the server-side session is continued via `--resume <id>`.
async fn run_claude_cli(
    bin: &Path,
    model: &str,
    system: &str,
    input: &str,
    images: &[serde_json::Value],
    resume: Option<&str>,
) -> Result<(String, Option<String>, Option<u64>)> {
    let mut cmd = Command::new(bin);
    cmd.arg("-p");
    // Image turns need stream-json in (verified spike: requires stream-json out + --verbose);
    // text-only turns keep the plain json path untouched.
    let stream = !images.is_empty();
    if stream {
        cmd.args([
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
        ]);
    } else {
        cmd.arg("--output-format").arg("json");
    }
    // On Windows every spawn (exe included) uses the file form — the command
    // line is capped at 32,767 UTF-16 chars and usta's system prompt is
    // already ~25K and growing. `.cmd`/`.bat` shims need it on any OS too:
    // cmd.exe can't take a multi-line argument at all.
    let _prompt_file = if crate::claude_bin::prompt_via_file(bin, cfg!(windows)) {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("usta-system-{}-{n}.md", std::process::id()));
        std::fs::write(&path, system).context("failed to write the system prompt file")?;
        cmd.arg("--append-system-prompt-file").arg(&path);
        Some(PromptFile(path))
    } else {
        cmd.arg("--append-system-prompt").arg(system);
        None
    };
    cmd.arg("--model")
        .arg(model)
        .arg("--allowedTools")
        .arg(CLI_ALLOWED_TOOLS)
        // Usta is standalone: never load the user's Claude Code settings,
        // plugins, skills or hooks into its calls — they cost ~25k tokens per
        // turn and have nothing to do with mentoring (measured: ~51k → ~26k).
        .arg("--setting-sources")
        .arg("");
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
        .with_context(|| format!("`{}` failed to start", bin.display()))?;

    // Write the input to stdin, then close it (EOF).
    {
        let mut stdin = child
            .stdin
            .take()
            .context("failed to get claude CLI stdin")?;
        let payload = if stream {
            stream_input(input, images)
        } else {
            input.to_string()
        };
        stdin
            .write_all(payload.as_bytes())
            .await
            .context("failed to write to claude CLI stdin")?;
        stdin.shutdown().await.ok();
    }

    let output = child
        .wait_with_output()
        .await
        .context("error waiting for claude CLI output")?;

    if !output.status.success() {
        if let Some(msg) = stream_failure(stream, &String::from_utf8_lossy(&output.stdout)) {
            bail!("{msg}");
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "claude CLI returned an error ({}): {}",
            output.status,
            stderr.trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stream {
        parse_stream_output(&stdout)
    } else {
        Ok(parse_cli_output(&stdout))
    }
}

/// First-run wizard: what the user typed at the prompt (spec: onboarding-lite).
#[derive(Debug)]
pub enum WizardAction {
    /// Empty line — try `select()` again (they installed Claude Code meanwhile).
    Recheck,
    /// `q` / `quit` — leave with a clean message.
    Quit,
    /// A pasted API key (`sk-ant-...`), already trimmed. NEVER printed or persisted.
    Key(String),
    /// Anything else — re-prompt.
    Invalid,
}

pub fn wizard_action(input: &str) -> WizardAction {
    let t = input.trim();
    if t.is_empty() {
        return WizardAction::Recheck;
    }
    if t.eq_ignore_ascii_case("q") || t.eq_ignore_ascii_case("quit") {
        return WizardAction::Quit;
    }
    if t.starts_with("sk-ant-") {
        return WizardAction::Key(t.to_string());
    }
    WizardAction::Invalid
}

pub fn wizard_guidance() -> &'static str {
    "No LLM backend found. Usta needs one of these:\n\n\
     \x20 1. Claude Code CLI (recommended — uses your subscription, no API key)\n\
     \x20    Install: https://claude.com/claude-code   (then just press Enter here)\n\n\
     \x20 2. Anthropic API key\n\
     \x20    Paste it below (starts with sk-ant-...), or add to your shell first:\n\
     \x20    export ANTHROPIC_API_KEY=sk-ant-...\n\n\
     Press Enter to re-check · paste your API key · or type q to quit"
}

/// Interactive fallback when `select()` finds no backend and we're on a TTY.
/// Loops: Enter = re-check, pasted key = set process env + re-check, q = quit.
/// The key is only written to this process's environment — never to disk, and
/// never echoed back.
pub fn run_backend_wizard() -> Result<Backend> {
    use std::io::{BufRead, Write};

    println!("\n{}", wizard_guidance());
    let stdin = std::io::stdin();
    loop {
        print!("> ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            bail!("no backend configured (EOF)");
        }
        match wizard_action(&line) {
            WizardAction::Quit => {
                bail!("no backend configured — run usta again once one is set up")
            }
            WizardAction::Key(k) => {
                std::env::set_var("ANTHROPIC_API_KEY", k);
                match select() {
                    Ok(b) => {
                        println!("backend found: {}", b.label());
                        println!("tip: add `export ANTHROPIC_API_KEY=...` to your shell profile to skip this next time");
                        return Ok(b);
                    }
                    Err(e) => println!("still no backend: {e}"),
                }
            }
            WizardAction::Recheck => match select() {
                Ok(b) => {
                    println!("backend found: {}", b.label());
                    return Ok(b);
                }
                Err(e) => println!("still no backend: {e}"),
            },
            WizardAction::Invalid => {
                println!(
                    "didn't catch that — Enter to re-check, paste an sk-ant-... key, or q to quit"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_attach::PastedImage;

    #[test]
    fn cli_call_isolates_usta_from_claude_code_settings() {
        // Usta is a standalone system: the user's Claude Code plugins, skills,
        // hooks and settings must never load into its calls. Without
        // `--setting-sources ""` the CLI baseline measured ~51k tokens per
        // turn (plugin/skill catalogs included); with it, ~26k (v0.25.1).
        // Source pin — the arg lives inside an async spawn fn.
        let src = include_str!("backend.rs");
        let production = src.split("#[cfg(test)]").next().unwrap();
        assert!(
            production.contains("--setting-sources"),
            "CLI call no longer isolates usta from Claude Code settings"
        );
    }

    #[test]
    fn context_window_is_1m_for_opus_200k_for_haiku() {
        let opus = Backend::Cli {
            model: "opus".into(),
            session_id: None,
            bin: PathBuf::from("claude"),
        };
        assert_eq!(opus.context_window(), 1_000_000);
        let haiku = Backend::Cli {
            model: "claude-haiku-4-5".into(),
            session_id: None,
            bin: PathBuf::from("claude"),
        };
        assert_eq!(haiku.context_window(), 200_000);
    }

    #[test]
    fn reset_session_clears_cli_session_id() {
        // reset_session() should set session_id to None — the learning session
        // after the slug mini-session must NOT resume it (spec B1).
        let mut b = Backend::Cli {
            model: "opus".into(),
            session_id: Some("sid-123".into()),
            bin: PathBuf::from("claude"),
        };
        b.reset_session();
        let Backend::Cli { session_id, .. } = &b else {
            panic!("Cli bekleniyordu")
        };
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

    #[test]
    fn wizard_action_interprets_inputs() {
        assert!(matches!(wizard_action(""), WizardAction::Recheck));
        assert!(matches!(wizard_action("   "), WizardAction::Recheck));
        assert!(matches!(wizard_action("q"), WizardAction::Quit));
        assert!(matches!(wizard_action("Q"), WizardAction::Quit));
        assert!(matches!(wizard_action(" quit "), WizardAction::Quit));
        match wizard_action("  sk-ant-abc123  ") {
            WizardAction::Key(k) => assert_eq!(k, "sk-ant-abc123"),
            other => panic!("expected Key, got {other:?}"),
        }
        assert!(matches!(wizard_action("hello"), WizardAction::Invalid));
    }

    #[test]
    fn wizard_guidance_names_both_paths() {
        let g = wizard_guidance();
        assert!(g.contains("claude.com/claude-code"));
        assert!(g.contains("ANTHROPIC_API_KEY"));
        assert!(g.contains("sk-ant-"));
        assert!(g.contains("q to quit"));
    }

    #[test]
    fn cli_call_uses_a_prompt_file_on_windows_and_batch_shims() {
        // Source pin (the args live inside an async spawn fn): the file
        // branch must cover every Windows spawn (command-line cap) plus
        // `.cmd`/`.bat` shims on any OS; the default branch keeps passing
        // the prompt inline.
        let src = include_str!("backend.rs");
        let production = src.split("#[cfg(test)]").next().unwrap();
        assert!(production.contains("prompt_via_file(bin, cfg!(windows))"));
        assert!(production.contains("--append-system-prompt-file"));
        assert!(production.contains("\"--append-system-prompt\""));
        assert!(
            !production.contains("fn claude_on_path"),
            "replaced by claude_bin::find_claude"
        );
    }

    #[test]
    fn prompt_file_deletes_on_drop() {
        let path =
            std::env::temp_dir().join(format!("usta-prompt-file-drop-test-{}", std::process::id()));
        std::fs::write(&path, "system prompt").unwrap();
        {
            let _pf = PromptFile(path.clone());
        }
        assert!(!path.exists());
    }

    #[test]
    fn cli_allowed_tools_can_research_and_read_links_but_not_touch_files() {
        let tools: Vec<&str> = CLI_ALLOWED_TOOLS.split(',').collect();
        assert!(tools.contains(&"WebSearch"), "{tools:?}");
        assert!(tools.contains(&"WebFetch"), "{tools:?}");
        for forbidden in ["Bash", "Edit", "Write"] {
            assert!(
                !tools.contains(&forbidden),
                "{forbidden} must stay off: {tools:?}"
            );
        }
    }

    fn pasted(bytes: Vec<u8>) -> PastedImage {
        PastedImage {
            media_type: "image/png",
            data: bytes,
            width: 1,
            height: 1,
        }
    }

    #[test]
    fn last_user_images_come_from_the_last_user_message_only() {
        let history = vec![
            Message::user_with_images("first [Image #1]", &[pasted(vec![1])]),
            Message::assistant_raw(serde_json::Value::String("ok".into())),
            Message::user_with_images("second [Image #1]", &[pasted(vec![2]), pasted(vec![3])]),
        ];
        assert_eq!(last_user_images(&history).len(), 2);
        let plain_last = vec![
            Message::user_with_images("a", &[pasted(vec![1])]),
            Message::user("b"),
        ];
        assert!(last_user_images(&plain_last).is_empty());
    }

    #[test]
    fn history_images_labels_each_user_message_with_images() {
        let mut history = vec![
            Message::user_with_images("one [Image #1]", &[pasted(vec![1])]),
            Message::assistant_raw(serde_json::Value::String("ok".into())),
            Message::user("two"),
            Message::assistant_raw(serde_json::Value::String("ok".into())),
            Message::user_with_images("three", &[pasted(vec![2]), pasted(vec![3])]),
        ];
        let v = history_images(&history);
        let kinds: Vec<&str> = v.iter().map(|b| b["type"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["text", "image", "text", "image", "image"]);
        assert_eq!(v[0]["text"], "images attached to [SEN] message 1:");
        assert_eq!(v[2]["text"], "images attached to [SEN] message 3:");
        // pruned placeholders are not images
        crate::anthropic::prune_images(&mut history, 2, usize::MAX);
        let v = history_images(&history);
        assert_eq!(v.iter().filter(|b| b["type"] == "image").count(), 2);
        assert_eq!(v[0]["text"], "images attached to [SEN] message 3:");
    }

    #[test]
    fn history_images_is_empty_for_text_only_history() {
        let history = vec![Message::user("a"), Message::user("b")];
        assert!(history_images(&history).is_empty());
    }

    #[test]
    fn no_resume_calls_send_history_images() {
        let src = include_str!("backend.rs");
        let production = src.split("#[cfg(test)]").next().unwrap();
        assert!(
            production.contains("None => (render_transcript(history), history_images(history)),")
        );
        assert!(
            production.contains("let images = history_images(history);"),
            "stale retry re-sends the bounded set"
        );
    }

    #[test]
    fn stream_input_is_one_json_line_with_images_before_text() {
        let blocks = vec![pasted(vec![1, 2, 3]).to_block()];
        let line = stream_input("describe [Image #1]", &blocks);
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1);
        let v: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(v["type"], "user");
        assert_eq!(v["message"]["role"], "user");
        assert_eq!(v["message"]["content"][0]["type"], "image");
        assert_eq!(v["message"]["content"][0]["source"]["data"], "AQID");
        assert_eq!(v["message"]["content"][1]["type"], "text");
        assert_eq!(v["message"]["content"][1]["text"], "describe [Image #1]");
    }

    #[test]
    fn parse_stream_output_takes_the_result_line_among_noise() {
        let out = concat!(
            "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"s0\"}\n",
            "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"draft\"}]}}\n",
            "{\"type\":\"rate_limit_event\"}\n",
            "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"Top red, bottom blue.\",\"session_id\":\"abc\",\"usage\":{\"input_tokens\":10,\"cache_read_input_tokens\":5}}\n",
        );
        let (text, sid, tokens) = parse_stream_output(out).unwrap();
        assert_eq!(text, "Top red, bottom blue.");
        assert_eq!(sid.as_deref(), Some("abc"));
        assert_eq!(tokens, Some(15));
    }

    #[test]
    fn parse_stream_output_errors_on_is_error_result() {
        let out = "{\"type\":\"result\",\"subtype\":\"error_during_execution\",\"is_error\":true,\"session_id\":\"x\"}\n";
        let err = parse_stream_output(out).unwrap_err().to_string();
        assert!(err.contains("error_during_execution"), "{err}");
    }

    #[test]
    fn stream_failure_surfaces_the_result_line_error() {
        let out = "{\"type\":\"system\"}\n{\"type\":\"result\",\"is_error\":true,\"subtype\":\"error_during_execution\"}\n";
        let msg = stream_failure(true, out).unwrap();
        assert!(msg.contains("error_during_execution"), "{msg}");
        // Non-stream mode and stdout without a result line keep the stderr bail.
        assert!(stream_failure(false, out).is_none());
        assert!(stream_failure(true, "").is_none());
        assert!(stream_failure(true, "{\"type\":\"system\"}\n").is_none());
    }

    #[test]
    fn parse_stream_output_errors_without_a_result_line() {
        assert!(parse_stream_output("{\"type\":\"system\"}\nnot json\n").is_err());
        assert!(parse_stream_output("").is_err());
    }

    #[test]
    fn cli_call_uses_stream_json_only_for_image_turns() {
        // Source pin - the flags live inside an async spawn fn.
        let src = include_str!("backend.rs");
        let production = src.split("#[cfg(test)]").next().unwrap();
        for flag in ["--input-format", "stream-json", "--verbose"] {
            assert!(production.contains(flag), "missing {flag}");
        }
        assert!(
            production.contains("let stream = !images.is_empty();"),
            "stream-json must stay opt-in per turn"
        );
    }

    /// Live end-to-end check against the real `claude` CLI (costs ~$0.005, needs login).
    /// Run: cargo test live_cli_describes_an_image -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn live_cli_describes_an_image() {
        let mut img = image::RgbaImage::from_pixel(120, 80, image::Rgba([220, 20, 20, 255]));
        for y in 40..80 {
            for x in 0..120 {
                img.put_pixel(x, y, image::Rgba([20, 20, 220, 255]));
            }
        }
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let block = pasted(png).to_block();
        let (text, sid, _) = run_claude_cli(
            Path::new("claude"),
            "haiku",
            "You are terse.",
            "Name the two colors in [Image #1], top then bottom, two words.",
            &[block],
            None,
        )
        .await
        .unwrap();
        let t = text.to_lowercase();
        assert!(t.contains("red") && t.contains("blue"), "{text}");
        assert!(sid.is_some());
    }
}
