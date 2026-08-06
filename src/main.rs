//! Usta — terminal Socratic öğrenim mentoru. İnce kabuk: CLI + LLM client +
//! dosya izleyici + markdown brain yükleyici. Zekâ markdown'da yaşar.

mod anthropic;
mod backend;
mod brain;
mod config;
mod defaults;
mod session;
mod watcher;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

use crate::backend::Backend;
use crate::session::Session;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if is_init(&args) {
        return run_init();
    }
    let topic = parse_topic(&args);

    // Backend seçimi (CLI default, API opsiyonel) — net hata mesajıyla.
    let backend = backend::select()?;

    // Global brain + proje kökü çöz (hibrit model — bkz. brain.rs).
    let global = config::global_root()?;
    let current_dir = std::env::current_dir()?;
    let project = config::find_project_root(&current_dir);

    let (system, watch_root): (String, PathBuf) = match &project {
        Some(root) => (
            brain::load_system_prompt(&global, Some(root), &topic),
            root.clone(),
        ),
        None => {
            eprintln!(
                "(uyarı: .usta/ bulunamadı — ilerleme kaydı olmadan çalışıyorum. `usta init` ile proje kökü kur.)"
            );
            (
                brain::load_system_prompt(&global, None, &topic),
                current_dir.clone(),
            )
        }
    };

    let mut session = Session::new(topic.clone(), system);

    // Dosya izleyiciyi proje kökünde başlat (proaktif feedback).
    let watch_rx = watcher::spawn(&watch_root)?;

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

/// `usta start <topic>` — konu yoksa default `rust`. Çıplak `usta` da `start rust`
/// gibi davranır (`init` bu fonksiyona hiç gelmez, `main` içinde önce elenir).
fn parse_topic(args: &[String]) -> String {
    let mut rest = args.iter().skip(1);
    match rest.next().map(String::as_str) {
        Some("start") => rest.next().cloned().unwrap_or_else(|| "rust".to_string()),
        _ => "rust".to_string(),
    }
}

/// `args[1] == "init"` mi? — `main`'in en başında `start` akışından ayırmak için.
fn is_init(args: &[String]) -> bool {
    args.get(1).map(String::as_str) == Some("init")
}

/// `usta init` — global brain'i (`~/.config/usta`) ilk-kez varsayılanlarla
/// doldurur (var olan dosyaların üstüne YAZMAZ) ve CWD'de proje `.usta/`
/// iskeletini kurar. Global brain "bir kere kurulur, tüm projelerde paylaşılır";
/// proje `.usta/` her projede ayrı, override + ilerleme kaydı için.
fn run_init() -> Result<()> {
    let global = config::global_root()?;
    std::fs::create_dir_all(&global)
        .with_context(|| format!("global kök oluşturulamadı: {}", global.display()))?;

    for (rel, content) in defaults::global_defaults() {
        let path = global.join(rel);
        if config::should_write(&path) {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("dizin oluşturulamadı: {}", parent.display()))?;
            }
            std::fs::write(&path, content)
                .with_context(|| format!("yazılamadı: {}", path.display()))?;
            println!("oluşturuldu: {}", path.display());
        } else {
            println!("zaten var, atlandı: {}", path.display());
        }
    }

    let cwd = std::env::current_dir()?;
    let usta_dir = cwd.join(".usta");
    for sub in ["learner/progress", "approaches"] {
        let dir = usta_dir.join(sub);
        let dir_existed = dir.is_dir();
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("dizin oluşturulamadı: {}", dir.display()))?;
        if dir_existed {
            println!("zaten var, atlandı: {}", dir.display());
        } else {
            println!("oluşturuldu: {}", dir.display());
        }

        // .gitkeep — boş dizin de commit edilebilsin.
        let gitkeep = dir.join(".gitkeep");
        if config::should_write(&gitkeep) {
            std::fs::write(&gitkeep, "")
                .with_context(|| format!("yazılamadı: {}", gitkeep.display()))?;
        }
    }

    println!("Hazır. 'usta start <konu>' ile başla.");
    Ok(())
}

/// Kaydedilen dosyayı sentetik user turn olarak enjekte et → Socratic feedback.
async fn handle_file_change(backend: &Backend, session: &mut Session, path: &Path) -> Result<()> {
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
