//! Anthropic Messages API — request/response types + non-streaming client.
//! Server-side `web_search` tool: the research runs on Anthropic's side, the
//! result comes back in the same response. `pause_turn` → re-send the message (server-tool loop).

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const DEFAULT_MODEL: &str = "claude-opus-4-8";
const API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MAX_TOKENS: u32 = 8000;
const MAX_CONTINUATIONS: usize = 6;

/// A conversation message. `content` can be a string (user input) or a raw
/// content array (when sending the assistant response back for `pause_turn`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Value,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Message { role: "user".into(), content: Value::String(text.into()) }
    }
    pub fn assistant_raw(content: Value) -> Self {
        Message { role: "assistant".into(), content }
    }
}

#[derive(Debug, Serialize)]
struct Thinking {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Debug, Serialize)]
struct OutputConfig {
    effort: &'static str,
}

#[derive(Debug, Serialize)]
struct Tool {
    #[serde(rename = "type")]
    kind: &'static str,
    name: &'static str,
}

/// The serialized request body.
#[derive(Debug, Serialize)]
pub struct MessageRequest {
    pub model: String,
    pub max_tokens: u32,
    pub system: String,
    pub messages: Vec<Message>,
    thinking: Thinking,
    output_config: OutputConfig,
    tools: Vec<Tool>,
}

impl MessageRequest {
    pub fn new(model: String, system: String, messages: Vec<Message>) -> Self {
        MessageRequest {
            model,
            max_tokens: MAX_TOKENS,
            system,
            messages,
            thinking: Thinking { kind: "adaptive" },
            output_config: OutputConfig { effort: "high" },
            tools: vec![Tool { kind: "web_search_20260209", name: "web_search" }],
        }
    }
}

#[derive(Debug, Deserialize)]
struct MessageResponse {
    content: Vec<Value>,
    stop_reason: Option<String>,
    usage: Option<Value>,
}

/// Extract the visible text from the response content array (type == "text").
pub fn extract_text(content: &[Value]) -> String {
    content
        .iter()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("")
}

/// Did the response include a web search? (for the UI hint)
pub fn used_web_search(content: &[Value]) -> bool {
    content.iter().any(|b| {
        matches!(
            b.get("type").and_then(Value::as_str),
            Some("web_search_tool_result") | Some("server_tool_use")
        )
    })
}

/// Total context tokens from the usage block: input + cache read + cache write.
/// None if `input_tokens` is missing — the indicator is silently skipped.
pub fn sum_context_tokens(usage: &Value) -> Option<u64> {
    let get = |k: &str| usage.get(k).and_then(Value::as_u64);
    Some(
        get("input_tokens")?
            + get("cache_read_input_tokens").unwrap_or(0)
            + get("cache_creation_input_tokens").unwrap_or(0),
    )
}

pub struct Client {
    http: reqwest::Client,
    api_key: String,
}

impl Client {
    pub fn new(api_key: String) -> Self {
        Client { http: reqwest::Client::new(), api_key }
    }

    /// Complete the request. The request body is built from model/system/history.
    /// If `pause_turn` comes back, append the assistant content and continue.
    /// The raw content juggling for `pause_turn` stays here — it doesn't leak into the session.
    /// Returns the final (text, used_web_search, context_tokens) triple.
    pub async fn complete(
        &self,
        model: &str,
        system: &str,
        history: &[Message],
    ) -> Result<(String, bool, Option<u64>)> {
        let mut req = MessageRequest::new(model.to_string(), system.to_string(), history.to_vec());
        let mut web = false;
        for _ in 0..MAX_CONTINUATIONS {
            let resp = self
                .http
                .post(API_URL)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("content-type", "application/json")
                .json(&req)
                .send()
                .await
                .context("Anthropic API'ye bağlanılamadı")?;

            let status = resp.status();
            let body = resp.text().await.context("yanıt gövdesi okunamadı")?;
            if !status.is_success() {
                bail!("Anthropic API hatası ({}): {}", status, body);
            }

            let parsed: MessageResponse =
                serde_json::from_str(&body).context("yanıt JSON ayrıştırılamadı")?;
            web |= used_web_search(&parsed.content);

            if parsed.stop_reason.as_deref() == Some("pause_turn") {
                // Hit the server-tool loop limit — send the assistant content
                // back so the server can continue where it left off.
                req.messages
                    .push(Message::assistant_raw(json!(parsed.content)));
                continue;
            }

            let tokens = parsed.usage.as_ref().and_then(sum_context_tokens);
            return Ok((extract_text(&parsed.content), web, tokens));
        }
        bail!("çok fazla pause_turn devamı — döngü kesildi");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serializes_expected_shape() {
        let req = MessageRequest::new(
            DEFAULT_MODEL.into(),
            "sistem".into(),
            vec![Message::user("merhaba")],
        );
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["model"], "claude-opus-4-8");
        assert_eq!(v["thinking"]["type"], "adaptive");
        assert_eq!(v["output_config"]["effort"], "high");
        assert_eq!(v["tools"][0]["type"], "web_search_20260209");
        assert_eq!(v["tools"][0]["name"], "web_search");
        assert_eq!(v["messages"][0]["role"], "user");
        assert_eq!(v["messages"][0]["content"], "merhaba");
    }

    #[test]
    fn extract_text_joins_text_blocks_only() {
        let content = vec![
            json!({"type": "thinking", "thinking": "gizli"}),
            json!({"type": "text", "text": "Merhaba "}),
            json!({"type": "text", "text": "dünya"}),
            json!({"type": "server_tool_use", "name": "web_search"}),
        ];
        assert_eq!(extract_text(&content), "Merhaba dünya");
    }

    #[test]
    fn used_web_search_detects_tool_blocks() {
        assert!(used_web_search(&[json!({"type": "web_search_tool_result"})]));
        assert!(!used_web_search(&[json!({"type": "text", "text": "x"})]));
    }

    #[test]
    fn sum_context_tokens_adds_all_categories() {
        let usage = json!({
            "input_tokens": 1000,
            "cache_read_input_tokens": 30000,
            "cache_creation_input_tokens": 500
        });
        assert_eq!(sum_context_tokens(&usage), Some(31500));
    }

    #[test]
    fn sum_context_tokens_works_with_only_input() {
        assert_eq!(sum_context_tokens(&json!({"input_tokens": 42})), Some(42));
    }

    #[test]
    fn sum_context_tokens_none_without_input_tokens() {
        assert_eq!(sum_context_tokens(&json!({"output_tokens": 5})), None);
    }
}
