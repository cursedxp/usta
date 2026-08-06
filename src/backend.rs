//! Takılabilir LLM backend'i: yerel `claude` CLI (default) veya Anthropic API.
//!
//! - **CLI (default):** `claude -p ...` — Claude Code'un mevcut auth'unu kullanır,
//!   API key / token faturası yok. `--allowedTools WebSearch` hem araştırmayı açar
//!   hem de "Usta dosya düzenlemez"i araç seviyesinde zorlar.
//! - **API (opsiyonel):** mevcut `anthropic::Client` reqwest yolu.
//!
//! Seçim: `USTA_BACKEND` env (`cli`/`api`) öncelikli; yoksa PATH'te `claude` varsa
//! CLI, yoksa `ANTHROPIC_API_KEY` varsa API, ikisi de yoksa net hata.

use anyhow::{bail, Context, Result};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::anthropic::{self, Message};

/// CLI backend'i için default model. `claude -p --model opus`.
pub const DEFAULT_CLI_MODEL: &str = "opus";

/// Kullanılabilir LLM backend'leri.
pub enum Backend {
    /// Yerel `claude` CLI'a shell'ler — Claude Code auth'u, key yok.
    Cli { model: String },
    /// Anthropic Messages API — reqwest, key gerektirir.
    Api {
        client: anthropic::Client,
        model: String,
    },
}

/// Ortam sinyallerine göre backend seç.
pub fn select() -> Result<Backend> {
    match std::env::var("USTA_BACKEND").ok().as_deref() {
        Some("cli") => Ok(cli_backend()),
        Some("api") => api_backend(),
        Some(other) => bail!(
            "USTA_BACKEND geçersiz: '{other}'. Geçerli değerler: 'cli' veya 'api'."
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
                    "LLM backend bulunamadı. İki seçenekten biri gerekli:\n  \
                     1) `claude` CLI'ı PATH'e ekle (Claude Code auth'u kullanılır, key gerekmez), veya\n  \
                     2) export ANTHROPIC_API_KEY=sk-ant-... (Anthropic API yolu).\n  \
                     Backend'i zorlamak için: export USTA_BACKEND=cli|api"
                )
            }
        }
    }
}

fn cli_backend() -> Backend {
    Backend::Cli {
        model: DEFAULT_CLI_MODEL.to_string(),
    }
}

fn api_backend() -> Result<Backend> {
    let key = crate::config::resolve_key(std::env::var("ANTHROPIC_API_KEY").ok())?;
    Ok(Backend::Api {
        client: anthropic::Client::new(key),
        model: anthropic::DEFAULT_MODEL.to_string(),
    })
}

/// `claude` çalıştırılabiliri PATH'te mi?
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
    /// Seçilen backend'e göre tamamlama iste. `(metin, web_arandı_mı)` döner.
    /// CLI modunda web kullanımı metinden tespit edilemez → `false`.
    pub async fn complete(&self, system: &str, history: &[Message]) -> Result<(String, bool)> {
        match self {
            Backend::Api { client, model } => client.complete(model, system, history).await,
            Backend::Cli { model } => {
                let transcript = render_transcript(history);
                let text = run_claude_cli(model, system, &transcript).await?;
                Ok((text, false))
            }
        }
    }
}

/// Konuşma geçmişini `claude` CLI'ın stdin'ine yazılacak düz-metin transcript'e
/// dönüştür. Kullanıcı = `[SEN]`, asistan = `[USTA]`.
fn render_transcript(history: &[Message]) -> String {
    let mut out = String::new();
    for msg in history {
        let label = if msg.role == "assistant" {
            "[USTA]"
        } else {
            "[SEN]"
        };
        // Saklanan turn'ler Value::String; değilse ham JSON'a düş.
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

/// `claude -p` alt sürecini çalıştır: transcript stdin'e yazılır, stdout okunur.
async fn run_claude_cli(model: &str, system: &str, transcript: &str) -> Result<String> {
    let mut child = Command::new("claude")
        .arg("-p")
        .arg("--append-system-prompt")
        .arg(system)
        .arg("--model")
        .arg(model)
        .arg("--allowedTools")
        .arg("WebSearch")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("`claude` CLI başlatılamadı — PATH'te mi?")?;

    // Transcript'i stdin'e yaz, sonra kapat (EOF).
    {
        let mut stdin = child
            .stdin
            .take()
            .context("claude CLI stdin'i alınamadı")?;
        stdin
            .write_all(transcript.as_bytes())
            .await
            .context("claude CLI stdin'ine yazılamadı")?;
        stdin.shutdown().await.ok();
    }

    let output = child
        .wait_with_output()
        .await
        .context("claude CLI çıktısı beklenirken hata")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("claude CLI hata döndü ({}): {}", output.status, stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
