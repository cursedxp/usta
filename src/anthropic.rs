//! Anthropic Messages API — request/response types + non-streaming client.
//! Server-side `web_search` + `web_fetch` tools: the research / page read runs on Anthropic's side, the
//! result comes back in the same response. `pause_turn` → re-send the message (server-tool loop).

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::image_attach::PastedImage;

pub const DEFAULT_MODEL: &str = "claude-opus-4-8";
const API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Output budget for a normal chat turn (adaptive thinking shares it).
pub const MAX_TOKENS: u32 = 8000;
const MAX_CONTINUATIONS: usize = 6;

/// A conversation message. `content` can be a string (user input) or a raw
/// content array (when sending the assistant response back for `pause_turn`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Value,
}

impl Message {
    /// A user turn with images: image blocks first (the docs' recommended order), then
    /// one text block. No images => the usual plain-string message.
    pub fn user_with_images(text: impl Into<String>, images: &[PastedImage]) -> Self {
        if images.is_empty() {
            return Self::user(text);
        }
        let mut blocks: Vec<Value> = images.iter().map(PastedImage::to_block).collect();
        blocks.push(json!({"type": "text", "text": text.into()}));
        Message {
            role: "user".into(),
            content: Value::Array(blocks),
        }
    }

    /// Plain text of the message: a string as is; a block array = its text blocks joined
    /// (image blocks and omitted-image placeholders contribute nothing, so base64 and
    /// pruning notes never leak into transcripts or reports).
    pub fn text(&self) -> String {
        match &self.content {
            Value::String(s) => s.clone(),
            Value::Array(blocks) => {
                let kept: Vec<Value> = blocks
                    .iter()
                    .filter(|b| b.get("text").and_then(Value::as_str) != Some(IMAGE_OMITTED))
                    .cloned()
                    .collect();
                extract_text(&kept)
            }
            other => other.to_string(),
        }
    }

    /// The `image` blocks of an array message (empty for string content).
    pub fn image_blocks(&self) -> Vec<Value> {
        self.content
            .as_array()
            .map(|bs| {
                bs.iter()
                    .filter(|b| b.get("type").and_then(Value::as_str) == Some("image"))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn user(text: impl Into<String>) -> Self {
        Message {
            role: "user".into(),
            content: Value::String(text.into()),
        }
    }
    pub fn assistant_raw(content: Value) -> Self {
        Message {
            role: "assistant".into(),
            content,
        }
    }
}

/// Stands in for an image block that `prune_images` dropped from history.
pub const IMAGE_OMITTED: &str = "[earlier image omitted to stay under the request size limit]";

/// Keep only the newest images within `max_count` blocks and `max_b64` base64 bytes.
/// Walks messages newest to oldest and, inside a message, image blocks last to first;
/// every image block over budget becomes an `IMAGE_OMITTED` text block in place.
/// Returns how many blocks were newly replaced (placeholders are skipped: idempotent).
pub fn prune_images(history: &mut [Message], max_count: usize, max_b64: usize) -> usize {
    let (mut count, mut bytes, mut dropped) = (0usize, 0usize, 0usize);
    for msg in history.iter_mut().rev() {
        let Some(blocks) = msg.content.as_array_mut() else {
            continue;
        };
        for b in blocks.iter_mut().rev() {
            if b.get("type").and_then(Value::as_str) != Some("image") {
                continue;
            }
            let len = b["source"]["data"].as_str().map_or(0, str::len);
            if count < max_count && bytes + len <= max_b64 {
                count += 1;
                bytes += len;
            } else {
                *b = json!({"type": "text", "text": IMAGE_OMITTED});
                dropped += 1;
            }
        }
    }
    dropped
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
    pub fn new(model: String, system: String, messages: Vec<Message>, max_tokens: u32) -> Self {
        MessageRequest {
            model,
            max_tokens,
            system,
            messages,
            thinking: Thinking { kind: "adaptive" },
            output_config: OutputConfig { effort: "high" },
            tools: vec![
                Tool {
                    kind: "web_search_20260209",
                    name: "web_search",
                },
                Tool {
                    kind: "web_fetch_20260209",
                    name: "web_fetch",
                },
            ],
        }
    }
}

#[derive(Debug, Deserialize)]
struct MessageResponse {
    content: Vec<Value>,
    stop_reason: Option<String>,
    usage: Option<Value>,
}

impl MessageResponse {
    /// The reply hit the output budget — its text is cut off.
    fn truncated(&self) -> bool {
        self.stop_reason.as_deref() == Some("max_tokens")
    }
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

/// Did the response use the web (search or page fetch)? (for the UI hint)
pub fn used_web_search(content: &[Value]) -> bool {
    content.iter().any(|b| {
        matches!(
            b.get("type").and_then(Value::as_str),
            Some("web_search_tool_result")
                | Some("web_fetch_tool_result")
                | Some("server_tool_use")
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
        Client {
            http: reqwest::Client::new(),
            api_key,
        }
    }

    /// Complete the request. The request body is built from model/system/history.
    /// If `pause_turn` comes back, append the assistant content and continue.
    /// The raw content juggling for `pause_turn` stays here — it doesn't leak into the session.
    /// Returns (text, used_web_search, context_tokens, truncated) of the final response.
    pub async fn complete(
        &self,
        model: &str,
        system: &str,
        history: &[Message],
        max_tokens: u32,
    ) -> Result<(String, bool, Option<u64>, bool)> {
        let mut req = MessageRequest::new(
            model.to_string(),
            system.to_string(),
            history.to_vec(),
            max_tokens,
        );
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
                .context("failed to connect to Anthropic API")?;

            let status = resp.status();
            let body = resp.text().await.context("failed to read response body")?;
            if !status.is_success() {
                bail!("Anthropic API error ({}): {}", status, body);
            }

            let parsed: MessageResponse =
                serde_json::from_str(&body).context("failed to parse response JSON")?;
            web |= used_web_search(&parsed.content);

            if parsed.stop_reason.as_deref() == Some("pause_turn") {
                // Hit the server-tool loop limit — send the assistant content
                // back so the server can continue where it left off.
                req.messages
                    .push(Message::assistant_raw(json!(parsed.content)));
                continue;
            }

            let tokens = parsed.usage.as_ref().and_then(sum_context_tokens);
            return Ok((
                extract_text(&parsed.content),
                web,
                tokens,
                parsed.truncated(),
            ));
        }
        bail!("too many pause_turn continuations — loop aborted");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(bytes: Vec<u8>) -> PastedImage {
        PastedImage {
            media_type: "image/png",
            data: bytes,
            width: 2,
            height: 3,
        }
    }

    #[test]
    fn user_with_no_images_is_a_plain_string_message() {
        let m = Message::user_with_images("hello", &[]);
        assert_eq!(m.content, Value::String("hello".into()));
    }

    #[test]
    fn user_with_images_puts_image_blocks_before_the_text_block() {
        let m = Message::user_with_images("what is this? [Image #1]", &[img(vec![1, 2, 3])]);
        assert_eq!(m.role, "user");
        let blocks = m.content.as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "image");
        assert_eq!(blocks[0]["source"]["media_type"], "image/png");
        assert_eq!(blocks[0]["source"]["data"], "AQID");
        assert_eq!(blocks[1]["type"], "text");
        assert_eq!(blocks[1]["text"], "what is this? [Image #1]");
    }

    #[test]
    fn request_serializes_an_image_message_as_a_content_array() {
        let req = MessageRequest::new(
            DEFAULT_MODEL.into(),
            "sistem".into(),
            vec![Message::user_with_images("hi", &[img(vec![9])])],
            MAX_TOKENS,
        );
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["messages"][0]["content"][0]["type"], "image");
        assert_eq!(v["messages"][0]["content"][1]["text"], "hi");
    }

    #[test]
    fn text_flattens_blocks_and_ignores_base64() {
        let m = Message::user_with_images("only words", &[img(vec![7; 1000])]);
        assert_eq!(m.text(), "only words");
        assert_eq!(Message::user("plain").text(), "plain");
    }

    #[test]
    fn image_blocks_returns_only_image_blocks() {
        let m = Message::user_with_images("t", &[img(vec![1]), img(vec![2])]);
        assert_eq!(m.image_blocks().len(), 2);
        assert!(Message::user("t").image_blocks().is_empty());
    }

    #[test]
    fn request_serializes_expected_shape() {
        let req = MessageRequest::new(
            DEFAULT_MODEL.into(),
            "sistem".into(),
            vec![Message::user("merhaba")],
            MAX_TOKENS,
        );
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["model"], "claude-opus-4-8");
        assert_eq!(v["max_tokens"], 8000);
        assert_eq!(v["thinking"]["type"], "adaptive");
        assert_eq!(v["output_config"]["effort"], "high");
        assert_eq!(v["tools"][0]["type"], "web_search_20260209");
        assert_eq!(v["tools"][0]["name"], "web_search");
        assert_eq!(v["tools"][1]["type"], "web_fetch_20260209");
        assert_eq!(v["tools"][1]["name"], "web_fetch");
        assert_eq!(v["messages"][0]["role"], "user");
        assert_eq!(v["messages"][0]["content"], "merhaba");
    }

    #[test]
    fn response_with_max_tokens_stop_is_truncated() {
        let cut: MessageResponse = serde_json::from_str(
            r#"{"content":[{"type":"text","text":"===FILE: progress===\n# Ru"}],"stop_reason":"max_tokens"}"#,
        )
        .unwrap();
        assert!(cut.truncated());
        let done: MessageResponse =
            serde_json::from_str(r#"{"content":[],"stop_reason":"end_turn"}"#).unwrap();
        assert!(!done.truncated());
    }

    #[test]
    fn request_carries_the_requested_output_budget() {
        let req = MessageRequest::new(
            DEFAULT_MODEL.into(),
            "s".into(),
            vec![Message::user("x")],
            32000,
        );
        assert_eq!(serde_json::to_value(&req).unwrap()["max_tokens"], 32000);
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
        assert!(used_web_search(&[
            json!({"type": "web_search_tool_result"})
        ]));
        assert!(used_web_search(&[json!({"type": "web_fetch_tool_result"})]));
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

    fn img_msg(n: usize, b64_len: usize) -> Message {
        let blocks: Vec<Value> = (0..n)
            .map(|_| json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":"A".repeat(b64_len)}}))
            .chain(std::iter::once(json!({"type":"text","text":"look"})))
            .collect();
        Message {
            role: "user".into(),
            content: Value::Array(blocks),
        }
    }

    fn count_images(h: &[Message]) -> usize {
        h.iter().map(|m| m.image_blocks().len()).sum()
    }

    #[test]
    fn prune_images_caps_the_count_keeping_the_newest() {
        let mut h: Vec<Message> = (0..5).map(|_| img_msg(1, 10)).collect();
        assert_eq!(prune_images(&mut h, 3, usize::MAX), 2);
        assert_eq!(count_images(&h), 3);
        assert!(h[0].image_blocks().is_empty());
        assert!(h[1].image_blocks().is_empty());
        assert_eq!(h[4].image_blocks().len(), 1);
    }

    #[test]
    fn prune_images_caps_the_base64_bytes() {
        let mut h = vec![img_msg(1, 100), img_msg(1, 100), img_msg(1, 100)];
        assert_eq!(prune_images(&mut h, 10, 250), 1);
        assert!(h[0].image_blocks().is_empty());
        assert_eq!(count_images(&h), 2);
    }

    #[test]
    fn prune_images_drops_a_messages_oldest_images_first() {
        let mut h = vec![img_msg(3, 10)];
        assert_eq!(prune_images(&mut h, 2, usize::MAX), 1);
        let blocks = h[0].content.as_array().unwrap();
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], IMAGE_OMITTED);
        assert_eq!(blocks[1]["type"], "image");
        assert_eq!(blocks[2]["type"], "image");
    }

    #[test]
    fn prune_images_is_idempotent() {
        let mut h: Vec<Message> = (0..4).map(|_| img_msg(1, 10)).collect();
        assert_eq!(prune_images(&mut h, 2, usize::MAX), 2);
        assert_eq!(prune_images(&mut h, 2, usize::MAX), 0);
        assert_eq!(count_images(&h), 2);
    }

    #[test]
    fn placeholder_does_not_leak_into_text() {
        let mut h = vec![img_msg(1, 10), img_msg(1, 10)];
        prune_images(&mut h, 1, usize::MAX);
        assert_eq!(h[0].text(), "look");
    }
}
