//! Anthropic Messages API — istek/yanıt tipleri + non-streaming client.
//! Server-side `web_search` tool: araştırma Anthropic tarafında koşar, sonuç
//! aynı yanıtta gelir. `pause_turn` → mesajı re-send et (server-tool döngüsü).

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const DEFAULT_MODEL: &str = "claude-opus-4-8";
const API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MAX_TOKENS: u32 = 8000;
const MAX_CONTINUATIONS: usize = 6;

/// Bir konuşma mesajı. `content` string (user girdisi) veya ham content dizisi
/// (assistant yanıtını `pause_turn` için geri yollarken) olabilir.
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

/// Serialize edilen istek gövdesi.
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
}

/// Yanıt content dizisinden görünür metni çıkar (type == "text").
pub fn extract_text(content: &[Value]) -> String {
    content
        .iter()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("")
}

/// Yanıt web araştırması içerdi mi? (UI ipucu için)
pub fn used_web_search(content: &[Value]) -> bool {
    content.iter().any(|b| {
        matches!(
            b.get("type").and_then(Value::as_str),
            Some("web_search_tool_result") | Some("server_tool_use")
        )
    })
}

pub struct Client {
    http: reqwest::Client,
    api_key: String,
}

impl Client {
    pub fn new(api_key: String) -> Self {
        Client { http: reqwest::Client::new(), api_key }
    }

    /// İsteği tamamla. İstek gövdesi model/system/history'den kurulur.
    /// `pause_turn` dönerse assistant içeriğini geri ekleyip devam et.
    /// `pause_turn`'ün ham content juggling'i burada kalır — session'a sızmaz.
    /// Nihai yanıtın (metin, web_arandı_mı) çiftini döndür.
    pub async fn complete(
        &self,
        model: &str,
        system: &str,
        history: &[Message],
    ) -> Result<(String, bool)> {
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
                // Server-tool döngüsü sınırına ulaştı — assistant içeriğini geri
                // yolla, server kaldığı yerden devam etsin.
                req.messages
                    .push(Message::assistant_raw(json!(parsed.content)));
                continue;
            }

            return Ok((extract_text(&parsed.content), web));
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
}
