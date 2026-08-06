//! Usta — terminal Socratic öğrenim mentoru. İnce kabuk: CLI + LLM client +
//! dosya izleyici + markdown brain yükleyici. Zekâ markdown'da yaşar.

mod anthropic;
mod backend;
mod brain;
mod config;
mod defaults;
mod session;
mod watcher;

use std::io::IsTerminal;
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

    // Backend seçimi (CLI default, API opsiyonel) — net hata mesajıyla.
    let backend = backend::select()?;

    // `.usta/` yoksa sessizce kur — `usta init` artık zorunlu ön-adım değil,
    // `start` kendi kendini bootstrap eder (bkz. ensure_scaffold).
    let cwd = std::env::current_dir()?;
    let had_project_root = config::find_project_root(&cwd).is_some();
    let project_root = ensure_scaffold(&cwd)?;
    if !had_project_root {
        println!("(.usta/ kuruldu)");
    }

    let mut rl = DefaultEditor::new()?;
    let topic = resolve_topic(&args, &mut rl)?;

    // Global brain + proje kökü birleştirilip system prompt üretilir (hibrit
    // model — bkz. brain.rs).
    let global = config::global_root()?;
    let system = brain::load_system_prompt(&global, Some(&project_root), &topic);

    let mut session = Session::new(topic.clone(), system);

    // Dosya izleyiciyi proje kökünde başlat (proaktif feedback).
    let mut watch_rx = watcher::spawn(&project_root)?;

    println!("Usta hazır — konu: {topic}. Kod yaz, kaydet; ben izlerim. (/quit ile çık)");

    loop {
        // 1) İzleyici kanalını boşalt → değişen dosyalara proaktif feedback.
        let mut changed: Vec<PathBuf> = Vec::new();
        while let Ok(p) = watch_rx.try_recv() {
            if !changed.contains(&p) {
                changed.push(p);
            }
        }
        for path in changed {
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

/// Komut satırında açık bir konu verilmiş mi? — sadece `usta start <konu>`
/// biçimini tanır. Bare `usta` veya argümansız `usta start` için `None` döner;
/// o durumda konu `resolve_topic` içinde TTY promptu veya "genel" default'uyla
/// çözülür.
fn explicit_topic(args: &[String]) -> Option<String> {
    let mut rest = args.iter().skip(1);
    match rest.next().map(String::as_str) {
        Some("start") => rest.next().cloned(),
        _ => None,
    }
}

/// Konuyu çöz: açık argüman > TTY promptu > sessiz "genel" default'u.
/// Stdin pipe'lanmışsa (TTY değilse) cevaplanamayacak bir prompt'a takılmadan
/// direkt "genel" döner.
fn resolve_topic(args: &[String], rl: &mut DefaultEditor) -> Result<String> {
    if let Some(raw) = explicit_topic(args) {
        return Ok(slugify_topic(&raw));
    }
    if !std::io::stdin().is_terminal() {
        return Ok("genel".to_string());
    }
    match rl.readline("Ne öğreneceksin/yapacaksın? (ör. rust, javascript): ") {
        Ok(line) => Ok(slugify_topic(&line)),
        // Ctrl-D / Ctrl-C promptta → engellemeden "genel"e düş.
        Err(_) => Ok("genel".to_string()),
    }
}

/// Serbest metni konu slug'ına çevir — saf fonksiyon, test edilebilir.
/// Kural: küçük harfe çevir, İLK boşlukla-ayrılmış token'ı al, sadece ascii
/// alfanümerik karakterleri tut. Sonuç boşsa `"genel"` döner.
pub fn slugify_topic(input: &str) -> String {
    let first_token = input.split_whitespace().next().unwrap_or("");
    let slug: String = first_token
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if slug.is_empty() {
        "genel".to_string()
    } else {
        slug
    }
}

/// `args[1] == "init"` mi? — `main`'in en başında `start` akışından ayırmak için.
fn is_init(args: &[String]) -> bool {
    args.get(1).map(String::as_str) == Some("init")
}

/// `.usta/` iskeletini tembel kurar — `start`'ın kendi kendini bootstrap
/// etmesini sağlar, `usta init`'i opsiyonel yapar. (1) global brain kökünü
/// (`~/.config/usta`) ve eksik varsayılan dosyalarını tamamlar (var olanın
/// üstüne yazmaz). (2) proje kökü yukarı doğru aranıp bulunamazsa `cwd`'de
/// yeni bir proje `.usta/` kurar ve `cwd`'yi döndürür; bulunursa onu olduğu
/// gibi döndürür.
fn ensure_scaffold(cwd: &Path) -> Result<PathBuf> {
    let global = config::global_root()?;
    write_global_defaults(&global)?;

    match config::find_project_root(cwd) {
        Some(root) => Ok(root),
        None => {
            write_project_scaffold(cwd)?;
            Ok(cwd.to_path_buf())
        }
    }
}

/// `usta init` — global brain'i (`~/.config/usta`) ilk-kez varsayılanlarla
/// doldurur (var olan dosyaların üstüne YAZMAZ) ve CWD'de proje `.usta/`
/// iskeletini kurar. Global brain "bir kere kurulur, tüm projelerde paylaşılır";
/// proje `.usta/` her projede ayrı, override + ilerleme kaydı için.
/// Yazma mantığı `ensure_scaffold`'la paylaşılır (`write_global_defaults` /
/// `write_project_scaffold`) — tek fark burada per-dosya durum yazdırılması.
fn run_init() -> Result<()> {
    let global = config::global_root()?;
    for (path, wrote) in write_global_defaults(&global)? {
        print_scaffold_status(&path, wrote);
    }

    let cwd = std::env::current_dir()?;
    for (path, wrote) in write_project_scaffold(&cwd)? {
        print_scaffold_status(&path, wrote);
    }

    println!("Hazır. 'usta start <konu>' ile başla.");
    Ok(())
}

/// `usta init`'in per-dosya/dizin durum satırı.
fn print_scaffold_status(path: &Path, wrote: bool) {
    if wrote {
        println!("oluşturuldu: {}", path.display());
    } else {
        println!("zaten var, atlandı: {}", path.display());
    }
}

/// Global brain kökünü oluşturup eksik varsayılan dosyaları yazar (var olanın
/// üstüne yazmaz — `config::should_write`). Her dosya için `(yol, yazıldı-mı)`
/// döner — `run_init` bunu yazdırır, `ensure_scaffold` sessizce yutar.
fn write_global_defaults(global: &Path) -> Result<Vec<(PathBuf, bool)>> {
    std::fs::create_dir_all(global)
        .with_context(|| format!("global kök oluşturulamadı: {}", global.display()))?;

    let mut results = Vec::new();
    for (rel, content) in defaults::global_defaults() {
        let path = global.join(rel);
        let wrote = if config::should_write(&path) {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("dizin oluşturulamadı: {}", parent.display()))?;
            }
            std::fs::write(&path, content)
                .with_context(|| format!("yazılamadı: {}", path.display()))?;
            true
        } else {
            false
        };
        results.push((path, wrote));
    }
    Ok(results)
}

/// `cwd/.usta/` altında proje iskeletini kurar (`learner/progress`,
/// `approaches` + `.gitkeep`'ler — boş dizin de commit edilebilsin diye).
/// `.gitkeep` yazımı sessizdir (orijinal `run_init` davranışıyla birebir);
/// dönen liste sadece dizinlerin `(yol, yazıldı-mı)` durumunu içerir.
fn write_project_scaffold(cwd: &Path) -> Result<Vec<(PathBuf, bool)>> {
    let usta_dir = cwd.join(".usta");
    let mut results = Vec::new();

    for sub in ["learner/progress", "approaches"] {
        let dir = usta_dir.join(sub);
        let dir_existed = dir.is_dir();
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("dizin oluşturulamadı: {}", dir.display()))?;
        results.push((dir.clone(), !dir_existed));

        // .gitkeep — boş dizin de commit edilebilsin.
        let gitkeep = dir.join(".gitkeep");
        if config::should_write(&gitkeep) {
            std::fs::write(&gitkeep, "")
                .with_context(|| format!("yazılamadı: {}", gitkeep.display()))?;
        }
    }

    Ok(results)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_lowercases_simple_word() {
        assert_eq!(slugify_topic("JavaScript"), "javascript");
    }

    #[test]
    fn slugify_takes_first_token_only() {
        assert_eq!(slugify_topic("Rust öğreniyorum"), "rust");
    }

    #[test]
    fn slugify_drops_non_alnum_chars() {
        assert_eq!(slugify_topic("C++"), "c");
    }

    #[test]
    fn slugify_blank_input_falls_back_to_genel() {
        assert_eq!(slugify_topic("   "), "genel");
    }

    #[test]
    fn slugify_multi_word_takes_first() {
        assert_eq!(slugify_topic("todo app"), "todo");
    }

    #[test]
    fn slugify_empty_string_falls_back_to_genel() {
        assert_eq!(slugify_topic(""), "genel");
    }

    #[test]
    fn explicit_topic_none_for_bare_invocation() {
        let args = vec!["usta".to_string()];
        assert_eq!(explicit_topic(&args), None);
    }

    #[test]
    fn explicit_topic_none_for_start_without_arg() {
        let args = vec!["usta".to_string(), "start".to_string()];
        assert_eq!(explicit_topic(&args), None);
    }

    #[test]
    fn explicit_topic_some_for_start_with_arg() {
        let args = vec![
            "usta".to_string(),
            "start".to_string(),
            "javascript".to_string(),
        ];
        assert_eq!(explicit_topic(&args), Some("javascript".to_string()));
    }

    #[test]
    fn is_init_true_only_for_init_subcommand() {
        assert!(is_init(&["usta".to_string(), "init".to_string()]));
        assert!(!is_init(&["usta".to_string(), "start".to_string()]));
        assert!(!is_init(&["usta".to_string()]));
    }

    /// `write_project_scaffold` bir temp dizinde `.usta/` iskeletini kurar —
    /// `global_root()`'a hiç dokunmadan (gerçek `~/.config`'i etkilemez).
    #[test]
    fn write_project_scaffold_creates_dirs_and_gitkeeps() {
        let base = std::env::temp_dir().join(format!(
            "usta_main_test_project_scaffold_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let results = write_project_scaffold(&base).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|(_, wrote)| *wrote));
        assert!(base.join(".usta/learner/progress").is_dir());
        assert!(base.join(".usta/approaches").is_dir());
        assert!(base.join(".usta/learner/progress/.gitkeep").is_file());
        assert!(base.join(".usta/approaches/.gitkeep").is_file());

        // İkinci çağrı: dizinler zaten var → `wrote` false dönmeli, panic yok.
        let results2 = write_project_scaffold(&base).unwrap();
        assert!(results2.iter().all(|(_, wrote)| !*wrote));

        let _ = std::fs::remove_dir_all(&base);
    }
}
