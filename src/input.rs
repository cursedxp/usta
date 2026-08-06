//! Kullanıcı girdisi: rustyline ayrı thread'de koşar, satırlar tokio kanalına
//! akar. Böylece ana döngü girdi beklerken watcher olaylarını da işleyebilir
//! (gerçek proaktif feedback). `ready` el-sıkışması, prompt'un Usta yanıtının
//! ORTASINA basılmasını önler: ana döngü bir turn'ü bitirince `()` yollar,
//! thread ancak o zaman yeni `sen> ` çizer.

use std::sync::mpsc::Receiver as ReadyReceiver;
use std::thread;

use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

/// Girdi olayı: bir satır veya kapanış isteği (Ctrl-C / Ctrl-D / girdi hatası).
pub enum InputEvent {
    Line(String),
    Eof,
}

/// Girdi thread'ini başlat. Her `ready_rx` sinyalinden sonra TEK satır okur ve
/// kanala yollar; ana döngü işleyip yeni `ready` gönderene dek tekrar okumaz.
/// `ready_tx` düşerse (ana döngü bitti) thread sessizce kapanır.
pub fn spawn(
    prompt: &'static str,
    ready_rx: ReadyReceiver<()>,
) -> UnboundedReceiver<InputEvent> {
    let (tx, rx) = unbounded_channel();
    thread::spawn(move || {
        let mut rl = match DefaultEditor::new() {
            Ok(rl) => rl,
            Err(_) => {
                let _ = tx.send(InputEvent::Eof);
                return;
            }
        };
        while ready_rx.recv().is_ok() {
            match rl.readline(prompt) {
                Ok(line) => {
                    if !line.trim().is_empty() {
                        let _ = rl.add_history_entry(&line);
                    }
                    if tx.send(InputEvent::Line(line)).is_err() {
                        return;
                    }
                }
                // Ctrl-D / Ctrl-C → kapanış sinyali, thread biter.
                Err(ReadlineError::Eof) | Err(ReadlineError::Interrupted) => {
                    let _ = tx.send(InputEvent::Eof);
                    return;
                }
                Err(_) => {
                    let _ = tx.send(InputEvent::Eof);
                    return;
                }
            }
        }
    });
    rx
}
