//! Usta — terminal Socratic öğrenim mentoru. İnce kabuk: CLI + LLM client +
//! dosya izleyici + markdown brain yükleyici. Zekâ markdown'da yaşar.

mod anthropic;
mod backend;
mod brain;
mod check;
mod config;
mod defaults;
mod feedback;
mod index;
mod input;
mod progress;
mod session;
mod watcher;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rustyline::DefaultEditor;

use crate::anthropic::Message;
use crate::backend::Backend;
use crate::session::Session;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let topic_arg = match parse_command(&args)? {
        Command::Init => return run_init(),
        Command::Topics => return run_topics(),
        Command::Start(t) => t,
    };

    // Backend seçimi (CLI default, API opsiyonel) — net hata mesajıyla.
    let mut backend = backend::select()?;

    // `.usta/` yoksa sessizce kur — `usta init` artık zorunlu ön-adım değil,
    // `start` kendi kendini bootstrap eder (bkz. ensure_scaffold).
    let cwd = std::env::current_dir()?;
    let had_project_root = config::find_project_root(&cwd).is_some();
    let project_root = ensure_scaffold(&cwd)?;
    if !had_project_root {
        println!("(.usta/ kuruldu)");
    }

    let topic = resolve_topic(topic_arg)?;

    // Global brain + proje kökü birleştirilip system prompt üretilir (hibrit
    // model — bkz. brain.rs).
    let global = config::global_root()?;
    let system = brain::load_system_prompt(&global, Some(&project_root), &topic);

    let mut session = Session::new(topic.clone(), system);

    // Dosya izleyici + girdi thread'i + debounce durumu.
    let mut watch_rx = watcher::spawn(&project_root)?;
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
    let mut input_rx = input::spawn("sen> ", ready_rx);
    let mut debouncer = watcher::Debouncer::new(std::time::Duration::from_millis(1000));
    let mut files = feedback::FileMemory::new();

    println!("Usta hazır — konu: {topic}. Kod yaz, kaydet; ben izlerim. (/quit ile çık)");

    // Açılış drilli: önceki oturumlardan progress varsa Usta ilk sözü alır,
    // 2-3 geri çağırma sorusuyla ısındırır (testing effect — USTA.md kuralı).
    let has_progress = std::fs::read_to_string(progress::progress_path(&project_root, &topic))
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    if has_progress {
        session.push_user(&progress::opening_prompt(&topic));
        match backend.complete(&session.system, session.history()).await {
            Ok((reply, web)) => {
                print_reply(&reply, web);
                session.push_assistant(reply);
            }
            // Drill başarısız → oturumu engelleme, sessizce normal akışa düş.
            Err(e) => eprintln!("(açılış drilli atlandı: {e})"),
        }
    }

    let _ = ready_tx.send(()); // ilk prompt

    loop {
        tokio::select! {
            maybe_ev = input_rx.recv() => match maybe_ev {
                Some(input::InputEvent::Line(line)) => {
                    let line = line.trim().to_string();
                    if line == "/quit" {
                        break;
                    }
                    if !line.is_empty() {
                        session.push_user(&line);
                        match backend.complete(&session.system, session.history()).await {
                            Ok((reply, web)) => {
                                print_reply(&reply, web);
                                session.push_assistant(reply);
                            }
                            Err(e) => eprintln!("(hata: {e})"),
                        }
                    }
                    let _ = ready_tx.send(());
                }
                Some(input::InputEvent::Eof) | None => break,
            },
            Some(path) = watch_rx.recv() => {
                debouncer.push(path, tokio::time::Instant::now());
            },
            _ = sleep_until_deadline(debouncer.deadline()), if debouncer.deadline().is_some() => {
                // Kullanıcı prompt'tayken de çalışır — gerçek proaktiflik.
                println!(); // yarım kalan prompt satırını kirletme
                for path in debouncer.flush() {
                    if let Err(e) = handle_file_change(&mut backend, &mut session, &mut files, &project_root, &path).await {
                        // Binary/silinmiş dosya vb. — sessizce geç, REPL yaşar.
                        eprintln!("(dosya feedback atlandı: {}: {e})", path.display());
                    }
                }
            }
        }
    }

    if let Err(e) = flush_progress(&mut backend, &session, &project_root).await {
        eprintln!("(progress güncellenemedi: {e})");
    }

    println!("Görüşürüz — suya girmeye devam et.");
    Ok(())
}

/// Oturum kapanışında progress dosyasını LLM'e tam-içerik yeniden yazdır.
/// Boş oturumda (hiç turn yok) dosyaya dokunma.
async fn flush_progress(backend: &mut Backend, session: &Session, project_root: &Path) -> Result<()> {
    if session.history().is_empty() {
        return Ok(());
    }
    println!("(oturum özetleniyor — progress yazılıyor…)");
    let path = progress::progress_path(project_root, &session.topic);
    let existing = std::fs::read_to_string(&path).ok();
    let mut history = session.history().to_vec();
    history.push(Message::user(progress::closing_prompt(
        &session.topic,
        existing.as_deref(),
    )));
    let (reply, _) = backend.complete(&session.system, &history).await?;
    let content = progress::clean_markdown_reply(&reply);
    if content.is_empty() {
        anyhow::bail!("model boş içerik döndürdü — dosya yazılmadı");
    }
    progress::write_atomic(&path, &content)?;
    println!("(progress güncellendi: {})", path.display());

    // Global kataloğu güncelle — başarısızlık progress yazımını geri almaz,
    // sadece not düşülür (katalog konfor katmanı, hafızanın kendisi değil).
    match config::global_root() {
        Ok(global) => {
            if let Err(e) = index::record(&global, &session.topic, project_root, &today()) {
                eprintln!("(katalog güncellenemedi: {e})");
            }
        }
        Err(e) => eprintln!("(katalog güncellenemedi: {e})"),
    }

    Ok(())
}

/// Bugünün yerel tarihi — katalog satırlarının tarih alanı.
fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Deadline varsa ona kadar uyu; yoksa asla dönmeyen future (select guard'ı
/// zaten bu kolu deadline'sız poll etmez — bu sadece tip güvenliği).
async fn sleep_until_deadline(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(d) => tokio::time::sleep_until(d).await,
        None => std::future::pending().await,
    }
}

/// Komut satırı komutu — argüman ayrıştırma tek yerde, saf ve test edilebilir.
#[derive(Debug, PartialEq)]
pub enum Command {
    /// `usta init` — iskelet kur, per-dosya durum yazdır.
    Init,
    /// `usta topics` — global katalogdan konu listesi.
    Topics,
    /// `usta` / `usta start [konu]` — öğrenme oturumu.
    Start(Option<String>),
}

/// Argümanları komuta çevir. Bilinmeyen komut net hata — sessiz sürpriz yok.
pub fn parse_command(args: &[String]) -> Result<Command> {
    let mut rest = args.iter().skip(1);
    match rest.next().map(String::as_str) {
        None => Ok(Command::Start(None)),
        Some("start") => Ok(Command::Start(rest.next().cloned())),
        Some("init") => Ok(Command::Init),
        Some("topics") => Ok(Command::Topics),
        Some(other) => anyhow::bail!(
            "bilinmeyen komut: '{other}'. Komutlar: start [konu], init, topics"
        ),
    }
}

/// Konuyu çöz: açık argüman > TTY promptu > sessiz "genel" default'u.
/// Stdin pipe'lanmışsa (TTY değilse) cevaplanamayacak bir prompt'a takılmadan
/// direkt "genel" döner.
fn resolve_topic(topic_arg: Option<String>) -> Result<String> {
    if let Some(raw) = topic_arg {
        return Ok(slugify_topic(&raw));
    }
    if !std::io::stdin().is_terminal() {
        return Ok("genel".to_string());
    }
    let mut rl = DefaultEditor::new()?;
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

/// `usta topics` — global katalogdaki kayıtları listele. LLM gerekmez.
fn run_topics() -> Result<()> {
    let global = config::global_root()?;
    let content =
        std::fs::read_to_string(global.join("learner/index.md")).unwrap_or_default();
    let list = index::entries(&content);
    if list.is_empty() {
        println!("Kayıtlı konu yok — 'usta start <konu>' ile başla.");
        return Ok(());
    }
    println!("Konu | Proje | Son oturum");
    for e in list {
        println!("{} | {} | {}", e.topic, e.project.display(), e.date);
    }
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

/// Kaydedilen dosyayı FileMemory'den geçir; ilk görüşte tam içerik, sonrasında
/// diff olarak sentetik user turn'e çevir → Socratic feedback. Cargo projesiyse
/// check sonucu "sadece Usta'nın gözü için" bloğuyla eklenir (tahmin protokolü).
async fn handle_file_change(
    backend: &mut Backend,
    session: &mut Session,
    files: &mut feedback::FileMemory,
    project_root: &Path,
    path: &Path,
) -> Result<()> {
    let contents = std::fs::read_to_string(path)?;
    let mut injected = match files.observe(path, contents) {
        feedback::ChangePayload::Skip => return Ok(()),
        feedback::ChangePayload::TooLarge(len) => {
            println!("(büyük dosya izleme dışı: {} — {len} bayt)", path.display());
            return Ok(());
        }
        feedback::ChangePayload::FirstSight(full) => format!(
            "[Dosya kaydedildi: {}]\n{full}\n\nBu değişikliğe proje-temelli, Socratic geri bildirim ver.",
            path.display()
        ),
        feedback::ChangePayload::Diff(diff) => format!(
            "[Dosya değişti: {}]\nDeğişiklik (unified diff):\n{diff}\n\nBu değişikliğe proje-temelli, Socratic geri bildirim ver — değişen kısma odaklan.",
            path.display()
        ),
    };
    if let Some(check_result) = check::run_check(project_root).await {
        injected.push_str(&format!(
            "\n\n[cargo check sonucu — SADECE SENİN GÖZÜN İÇİN, kullanıcıya doğrudan aktarma; tahmin protokolünü uygula]\n{check_result}"
        ));
    }
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
    fn parse_bare_is_start_without_topic() {
        let args = vec!["usta".to_string()];
        assert_eq!(parse_command(&args).unwrap(), Command::Start(None));
    }

    #[test]
    fn parse_start_keeps_topic_arg() {
        let args = vec!["usta".into(), "start".into(), "javascript".into()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Start(Some("javascript".to_string()))
        );
    }

    #[test]
    fn parse_start_without_arg_is_start_none() {
        let args = vec!["usta".into(), "start".into()];
        assert_eq!(parse_command(&args).unwrap(), Command::Start(None));
    }

    #[test]
    fn parse_init_and_topics() {
        assert_eq!(
            parse_command(&["usta".into(), "init".into()]).unwrap(),
            Command::Init
        );
        assert_eq!(
            parse_command(&["usta".into(), "topics".into()]).unwrap(),
            Command::Topics
        );
    }

    #[test]
    fn parse_unknown_command_errors() {
        assert!(parse_command(&["usta".into(), "rust".into()]).is_err());
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
