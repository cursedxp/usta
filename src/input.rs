//! User input: rustyline runs in a separate thread, lines flow into a tokio
//! channel. This way the main loop can also handle watcher events while
//! waiting for input (real proactive feedback). The `ready` handshake prevents
//! the prompt from being printed in the MIDDLE of Usta's response: the main
//! loop sends `()` when it finishes a turn, only then does the thread draw a
//! new `sen> `.

use std::sync::mpsc::Receiver as ReadyReceiver;
use std::thread;

use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

/// Input event: a line, or a shutdown request (Ctrl-C / Ctrl-D / input error).
pub enum InputEvent {
    Line(String),
    Eof,
}

/// Start the input thread. After each `ready_rx` signal it reads a SINGLE
/// line and sends it to the channel; it doesn't read again until the main
/// loop processes it and sends a new `ready`.
/// If `ready_tx` is dropped (main loop ended), the thread closes silently.
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
                // Ctrl-D / Ctrl-C → shutdown signal, thread ends.
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
