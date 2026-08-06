//! Usta — terminal Socratic öğrenim mentoru. İnce kabuk: CLI + LLM client +
//! dosya izleyici + markdown brain yükleyici. Zekâ markdown'da yaşar.

mod anthropic;
mod backend;
mod brain;
mod config;
mod session;
mod watcher;

use std::path::{Path, PathBuf};

use anyhow::Result;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

use crate::backend::Backend;
use crate::session::Session;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let topic = parse_topic(&args);

    // Backend seçimi (CLI default, API opsiyonel) — net hata mesajıyla.
    let backend = backend::select()?;

    // Brain'i CWD'den yükle, oturumu kur.
    let root = std::env::current_dir()?;
    let system = brain::load_system_prompt(&root, &topic);
    let mut session = Session::new(topic.clone(), system);

    // Dosya izleyiciyi başlat (proaktif feedback).
    let watch_rx = watcher::spawn(Path::new("."))?;

    println!("Usta hazır — konu: {topic}. Kod yaz, kaydet; ben izlerim. (/quit ile çık)");

    let mut rl = DefaultEditor::new()?;
    loop {
        // 1) İzleyici kanalını boşalt → değişen dosyalara proaktif feedback.
        let mut changed: Vec<PathBuf> = Vec::new();
        while let Ok(p) = watch_rx.try_recv() {
            changed.push(p);
        }
        for path in watcher::dedup_paths(changed) {
            if let Err(e) = handle_file_change(&backend, &mut session, &path).await {
                // Binary/silinmiş dosya vb. — sessizce geç, REPL yaşar.
                eprintln!("(dosya feedback atlandı: {}: {e})", path.display());
            }
        }

        // 2) Kullanıcı girdisi.
        match rl.readline("sen> ") {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if line == "/quit" {
                    break;
                }
                let _ = rl.add_history_entry(line);
                session.push_user(line);
                match backend.complete(&session.system, session.history()).await {
                    Ok((reply, web)) => {
                        print_reply(&reply, web);
                        session.push_assistant(reply);
                    }
                    Err(e) => eprintln!("(hata: {e})"),
                }
            }
            // Ctrl-D / Ctrl-C → temiz çıkış.
            Err(ReadlineError::Eof) | Err(ReadlineError::Interrupted) => break,
            Err(e) => {
                eprintln!("(girdi hatası: {e})");
                break;
            }
        }
    }

    println!("Görüşürüz — suya girmeye devam et.");
    Ok(())
}

/// `usta start <topic>` — konu yoksa default `rust`.
fn parse_topic(args: &[String]) -> String {
    let mut rest = args.iter().skip(1);
    match rest.next().map(String::as_str) {
        Some("start") => rest.next().cloned().unwrap_or_else(|| "rust".to_string()),
        _ => "rust".to_string(),
    }
}

/// Kaydedilen dosyayı sentetik user turn olarak enjekte et → Socratic feedback.
async fn handle_file_change(
    backend: &Backend,
    session: &mut Session,
    path: &Path,
) -> Result<()> {
    let contents = std::fs::read_to_string(path)?;
    let injected = format!(
        "[Dosya kaydedildi: {}]\n{contents}\n\nBu değişikliğe proje-temelli, Socratic geri bildirim ver.",
        path.display()
    );
    session.push_user(&injected);
    let (reply, web) = backend.complete(&session.system, session.history()).await?;
    print_reply(&reply, web);
    session.push_assistant(reply);
    Ok(())
}

/// Usta yanıtını yazdır; web araştırıldıysa küçük not ekle.
fn print_reply(reply: &str, web: bool) {
    println!("Usta> {reply}");
    if web {
        println!("(🔎 web araştırıldı)");
    }
}
