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

/// Tek tamamlama sonucu — metin + web ipucu + bağlam doluluğu.
pub struct Reply {
    pub text: String,
    pub web: bool,
    /// Son çağrının toplam bağlam token'ı (input + cache) — gösterge için.
    pub context_tokens: Option<u64>,
}

/// CLI backend'i için default model. `claude -p --model opus`.
pub const DEFAULT_CLI_MODEL: &str = "opus";

/// Kullanılabilir LLM backend'leri.
pub enum Backend {
    /// Yerel `claude` CLI'a shell'ler — Claude Code auth'u, key yok.
    /// `session_id`: ilk yanıttan yakalanır, sonraki turn'ler `--resume` ile
    /// sürdürülür → tam transcript her seferinde yeniden gönderilmez.
    Cli {
        model: String,
        session_id: Option<String>,
    },
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
    /// Banner'da gösterilecek model etiketi.
    pub fn label(&self) -> String {
        match self {
            Backend::Cli { model, .. } => format!("{model} · cli"),
            Backend::Api { model, .. } => format!("{model} · api"),
        }
    }

    /// CLI server oturumunu sıfırla — kompaksiyon sonrası sıradaki çağrı
    /// kompakt history ile YENİ oturum açar. API'de no-op.
    pub fn reset_session(&mut self) {
        if let Backend::Cli { session_id, .. } = self {
            *session_id = None;
        }
    }

    /// Seçilen backend'e göre tamamlama iste, `Reply` döner.
    /// CLI modunda web kullanımı metinden tespit edilemez → `false`.
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
                    // Stale/silinmiş oturum — bir kez tam transcript'le baştan dene.
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

/// History'deki SON user mesajının düz metnini döndür — resume çağrısında
/// sunucu taraflı oturum bağlamı zaten var, sadece yeni turn gönderilir.
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

/// `claude -p --output-format json` çıktısını ayrıştır. JSON değilse (eski
/// sürüm / beklenmedik çıktı) ham metne düş — session id'siz devam edilir.
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

/// `claude -p` alt sürecini çalıştır: girdi stdin'e yazılır, JSON çıktı okunur.
/// `resume` verilirse `--resume <id>` ile sunucu taraflı oturum sürdürülür.
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
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("`claude` CLI başlatılamadı — PATH'te mi?")?;

    // Girdiyi stdin'e yaz, sonra kapat (EOF).
    {
        let mut stdin = child
            .stdin
            .take()
            .context("claude CLI stdin'i alınamadı")?;
        stdin
            .write_all(input.as_bytes())
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

    Ok(parse_cli_output(&String::from_utf8_lossy(&output.stdout)))
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
