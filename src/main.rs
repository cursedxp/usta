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
mod transcript;
mod tui;
mod ui;
mod watcher;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rustyline::DefaultEditor;

use crate::anthropic::Message;
use crate::backend::Backend;
use crate::session::Session;
use crate::transcript::Recorder;


/// Bu orana ulaşınca ara-kayıt + kompaksiyon tetiklenir.
const COMPACT_THRESHOLD: f64 = 0.70;
/// Kompaksiyon sonrası history'de bırakılacak son mesaj sayısı.
const COMPACT_KEEP_LAST: usize = 4;
/// Kompaksiyon sonrası history başına eklenen not — modele bağlamın
/// sıkıştırıldığını, özün dosyalarda olduğunu söyler.
const COMPACT_NOTE: &str = "[ARA KAYIT] Bağlam sıkıştırıldı. Önceki konuşmanın özü \
system prompt'taki progress/curriculum/approach dosyalarına yazıldı — güncel durum \
orada. Kaldığımız yerden devam et; kullanıcıya kompaksiyonu anlatma.";
/// Tek debounce penceresinde feedback verilecek azami dosya sayısı — üstü
/// "toplu değişiklik" sayılır (git checkout, format-all): LLM çağrısı yok.
const MAX_FEEDBACK_BATCH: usize = 5;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let topic_arg = match parse_command(&args)? {
        Command::Init => return run_init(),
        Command::Topics => return run_topics(),
        Command::Reset(ResetTarget::Topic(t)) => return run_reset_topic(&t),
        Command::Reset(ResetTarget::Factory) => return run_reset_factory(),
        Command::Reset(ResetTarget::Profile) => return run_reset_profile(),
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
        ui::notice(".usta/ set up");
    }

    // Global brain + proje kökü birleştirilip system prompt üretilir (hibrit
    // model — bkz. brain.rs). build_session bunu kullanır.
    let global = config::global_root()?;

    // Dosya izleyici — TEK kez spawn edilir (thread başlatır), sonra çalışan
    // yola (&mut) geçirilir. Girdi thread'i + debounce durumu yola özgü:
    // plain yol rustyline kullanır, TUI yol crossterm EventStream.
    let mut watch_rx = watcher::spawn(&project_root)?;

    for p in transcript::find_unfinished(&project_root) {
        ui::warn(&format!("half-finished session record found (may not have been flushed): {}", p.display()));
    }

    // İki yol da `(Session, Recorder, PathBuf)` üretir; kapanış paylaşımlı.
    // TUI yolu: konu girişi + slug/onay + build_session hepsi run() içinde —
    // topic_arg ham geçer, `None` dönüşü kullanıcının konu vermeden çıkışıdır.
    // Plain yol (TTY yok / NO_COLOR): resolve_topic + lock-çakışma + build_session
    // + banner + run_plain_loop burada — davranış birebir korunur.
    let (session, recorder, lock) = if !ui::is_plain() {
        // TUI aktifken notice/warn/Spinner ham ANSI basmasın diye bayrağı
        // aç — run() dönünce (hata dahil) mutlaka kapat, sonra hatayı fırlat.
        ui::set_tui_active(true);
        let r = tui::run::run(
            &mut backend,
            &global,
            &project_root,
            &today(),
            topic_arg,
            MAX_FEEDBACK_BATCH,
            &mut watch_rx,
        )
        .await;
        ui::set_tui_active(false);
        match r? {
            Some(artifacts) => artifacts,
            None => {
                // Konu verilmeden çıkıldı — oturum/kilit yok, kapanacak şey yok.
                ui::notice("See you — keep getting in the water.");
                return Ok(());
            }
        }
    } else {
        let (topic, intro) = resolve_topic(&mut backend, topic_arg, &project_root, &global).await?;

        // Lock-çakışması onayı (plain/pipe) — build_session'dan ÖNCE, kendi
        // lock'unu yazmadan. (TUI yolunda bu kontrol run() içinde tui_confirm ile.)
        let lock = lock_path(&project_root, &topic);
        if lock.exists() {
            let pid = std::fs::read_to_string(&lock).unwrap_or_default();
            if std::io::stdin().is_terminal() {
                let msg = format!(
                    "Another session may be open for this topic (pid {}) — progress could clash \
                     if both sessions close at the same time. Continue anyway? [y/N] ",
                    pid.trim()
                );
                if !confirm(&msg, &["e", "evet", "y", "yes"])? {
                    println!("cancelled — close the other session first (or delete the lock if it's stale: {})", lock.display());
                    return Ok(());
                }
            } else {
                ui::warn("stale topic lock found — continuing in pipe mode");
            }
        }

        let (mut session, recorder, lock, has_progress) =
            build_session(&global, &project_root, &topic, &today())?;
        ui::banner(&topic, &backend.label());
        // Profil hâlâ gömülü jenerik şablonsa (veya hiç yoksa) Usta kullanıcıyı
        // tanımıyor demektir — açılış turn'üne kısa tanışma talimatı eklenir (spec Ç3a).
        let profile_generic = std::fs::read_to_string(global.join("USER.md"))
            .ok()
            .as_deref()
            .map(profile_is_generic)
            .unwrap_or(true);
        run_plain_loop(
            &mut backend,
            &mut session,
            &recorder,
            &project_root,
            &topic,
            has_progress,
            intro.as_deref(),
            profile_generic,
            &mut watch_rx,
        )
        .await?;
        (session, recorder, lock)
    };

    if let Err(e) = flush_progress(&mut backend, &session, &project_root).await {
        ui::warn(&format!("progress could not be updated: {e} — raw record left on disk: {}", recorder.path().display()));
    } else if session.history().is_empty() {
        // Boş oturum: dosya hiç oluşmadı, işaretlenecek şey yok.
    } else if let Err(e) = transcript::mark_done(recorder.path()) {
        ui::warn(&format!("session record could not be marked done: {e}"));
    }

    let _ = std::fs::remove_file(&lock);

    ui::notice("See you — keep getting in the water.");
    Ok(())
}

/// Plain (satır tabanlı) REPL döngüsü: rustyline girdi thread'i + watcher +
/// debounce tek select!'te. TTY yoksa / NO_COLOR'da koşar — davranış eski
/// main döngüsüyle birebir (banner main'de basılır, drill + loop burada).
async fn run_plain_loop(
    backend: &mut Backend,
    session: &mut Session,
    recorder: &transcript::Recorder,
    project_root: &Path,
    topic: &str,
    has_progress: bool,
    intro: Option<&str>,
    profile_generic: bool,
    watch_rx: &mut tokio::sync::mpsc::UnboundedReceiver<PathBuf>,
) -> Result<()> {
    // Girdi thread'i + debounce durumu — plain yola özgü (rustyline).
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
    let mut input_rx = input::spawn("❯ ", ready_rx);
    let mut debouncer = watcher::Debouncer::new(std::time::Duration::from_millis(1000));
    let mut files = feedback::FileMemory::new();

    // Açılış drilli: önceki oturumlardan progress varsa Usta ilk sözü alır,
    // 2-3 geri çağırma sorusuyla ısındırır (testing effect — USTA.md kuralı).
    if has_progress {
        let opening = progress::opening_prompt(topic, profile_generic);
        session.push_user(&opening);
        recorder.user(&opening);
        match ask_usta(backend, &session.system, session.history()).await {
            Ok(reply) => {
                print_reply(&reply, backend.context_window());
                recorder.assistant(&reply.text);
                session.push_assistant(reply.text);
            }
            // Drill başarısız → oturumu engelleme, sessizce normal akışa düş.
            Err(e) => ui::warn(&format!("opening drill skipped: {e}")),
        }
    } else {
        // Yeni konu: yaklaşım/harita yok — tanışma turn'ü, Usta ilk sözü alır.
        let onboarding = progress::onboarding_prompt(topic, intro, profile_generic);
        session.push_user(&onboarding);
        recorder.user(&onboarding);
        match ask_usta(backend, &session.system, session.history()).await {
            Ok(reply) => {
                print_reply(&reply, backend.context_window());
                recorder.assistant(&reply.text);
                session.push_assistant(reply.text);
            }
            Err(e) => ui::warn(&format!("introduction turn skipped: {e}")),
        }
    }

    let _ = ready_tx.send(()); // ilk prompt

    let mut watching = true;
    loop {
        tokio::select! {
            maybe_ev = input_rx.recv() => match maybe_ev {
                Some(input::InputEvent::Line(line)) => {
                    let line = line.trim().to_string();
                    if let Some(cmd) = parse_watch_command(&line) {
                        let (next, msg) = apply_watch(cmd, watching);
                        watching = next;
                        ui::notice(msg);
                        let _ = ready_tx.send(());
                        continue;
                    }
                    if line == "/quit" {
                        break;
                    }
                    if !line.is_empty() {
                        session.push_user(&line);
                        recorder.user(&line);
                        match ask_usta(backend, &session.system, session.history()).await {
                            Ok(reply) => {
                                print_reply(&reply, backend.context_window());
                                let tokens = reply.context_tokens;
                                recorder.assistant(&reply.text);
                                session.push_assistant(reply.text);
                                maybe_compact(backend, session, project_root, tokens).await;
                            }
                            Err(e) => ui::warn(&format!("error: {e}")),
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
                let batch = debouncer.flush();
                if batch.len() > MAX_FEEDBACK_BATCH {
                    ui::notice(&format!(
                        "bulk change ({} files) — feedback skipped, still watching",
                        batch.len()
                    ));
                    // FileMemory'yi sessizce senkronla: sonraki tekil kayıt
                    // bu yığına karşı dev diff üretmesin.
                    for path in batch {
                        if let Ok(c) = std::fs::read_to_string(&path) {
                            let _ = files.observe(&path, c);
                        }
                    }
                } else if !watching {
                    // Companion off: keep the diff baseline current, no LLM feedback.
                    for path in batch {
                        if let Ok(c) = std::fs::read_to_string(&path) {
                            let _ = files.observe(&path, c);
                        }
                    }
                } else {
                    for path in batch {
                        match handle_file_change(backend, session, &mut files, project_root, &path, recorder).await {
                            // handle_file_change artık basmaz — plain yol kendi
                            // sunum dilini uygular (print_reply: web + gauge).
                            Ok(FileFeedback::Sessiz) => {}
                            Ok(FileFeedback::Bildirim(m)) => println!("{m}"),
                            Ok(FileFeedback::Yanit { tokens, reply }) => {
                                print_reply(&reply, backend.context_window());
                                maybe_compact(backend, session, project_root, tokens).await;
                            }
                            // Binary/silinmiş dosya vb. — sessizce geç, REPL yaşar.
                            Err(e) => ui::warn(&format!("file feedback skipped: {}: {e}", path.display())),
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// LLM çağrısını spinner ile sar — kullanıcı beklerken sessizlik olmasın.
async fn ask_usta(
    backend: &mut Backend,
    system: &str,
    history: &[Message],
) -> Result<backend::Reply> {
    let spinner = ui::Spinner::start("Usta is thinking…");
    let result = backend.complete(system, history).await;
    spinner.stop().await;
    result
}

/// Konu belli olduktan sonra oturum kurulumu — system prompt + Session + kendi
/// kilidini yaz + recorder + has_progress. Lock-ÇAKIŞMASI onayı burada DEĞİL
/// (çağıran yola göre halleder: plain stdin, TUI tek-tuş). Döner:
/// `(session, recorder, lock_yolu, has_progress)`.
fn build_session(
    global: &Path,
    project_root: &Path,
    topic: &str,
    today: &str,
) -> Result<(Session, Recorder, PathBuf, bool)> {
    let system = brain::load_system_prompt(global, Some(project_root), topic, today);
    let session = Session::new(topic.to_string(), system);

    let lock = lock_path(project_root, topic);
    if let Err(e) = std::fs::write(&lock, std::process::id().to_string()) {
        ui::warn(&format!("topic lock could not be written: {e}"));
    }

    let recorder = Recorder::new(transcript::session_path(project_root, topic, &now_stamp()));

    let has_progress = std::fs::read_to_string(progress::progress_path(project_root, topic))
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);

    Ok((session, recorder, lock, has_progress))
}

/// Oturum kapanışında progress/approach/curriculum dosyalarını LLM'e üretir.
/// Boş oturumda dokunmaz; bilinmeyen dosya adı uyarıyla atlanır (keyfi yola
/// asla yazılmaz).
/// Kapanış dosya adını yazma hedefine çözer — SAF: I/O yok, sadece yol
/// hesabı. `profile` GLOBAL köke (`global`) yazılır (kişi-hakkında, tüm
/// konularda ortak); `progress`/`approach`/`curriculum` PROJE köküne
/// (`project_root`). Bilinmeyen ad → `None` — `flush_progress`'teki
/// "bilinmeyen dosya atlanır" güvenliği bu sayede izole test edilebilir.
fn flush_target(name: &str, project_root: &Path, global: &Path, topic: &str) -> Option<PathBuf> {
    match name {
        "progress" => Some(progress::progress_path(project_root, topic)),
        "approach" => Some(progress::approach_path(project_root, topic)),
        "curriculum" => Some(progress::curriculum_path(project_root, topic)),
        "profile" => Some(global.join("USER.md")),
        _ => None,
    }
}

async fn flush_progress(
    backend: &mut Backend,
    session: &Session,
    project_root: &Path,
) -> Result<()> {
    if session.history().is_empty() {
        return Ok(());
    }
    ui::notice("summarizing session — writing files…");
    // Global kök tek seferde çözülür: hem mevcut profili prompt'a gömmek hem
    // de kapanışta profile yazmak için kullanılır. Çözülemezse profil bu
    // oturum için atlanır — progress/approach/curriculum (proje-yerel) buna
    // bağlı değil, yazımları etkilenmez.
    let global = match config::global_root() {
        Ok(g) => Some(g),
        Err(e) => {
            ui::warn(&format!("global root could not be resolved — profile will be skipped this session: {e}"));
            None
        }
    };
    let dummy_global = PathBuf::new();
    let global_for_paths = global.as_deref().unwrap_or(&dummy_global);
    let p_path = flush_target("progress", project_root, global_for_paths, &session.topic).unwrap();
    let a_path = flush_target("approach", project_root, global_for_paths, &session.topic).unwrap();
    let c_path = flush_target("curriculum", project_root, global_for_paths, &session.topic).unwrap();
    let pr_path = global
        .as_ref()
        .map(|g| flush_target("profile", project_root, g, &session.topic).unwrap());

    let read = |p: &Path| std::fs::read_to_string(p).ok();
    let mut history = session.history().to_vec();
    history.push(Message::user(progress::closing_prompt(
        &session.topic,
        read(&p_path).as_deref(),
        read(&a_path).as_deref(),
        read(&c_path).as_deref(),
        pr_path.as_deref().and_then(read).as_deref(),
    )));
    let reply = ask_usta(backend, &session.system, &history).await?;
    let files = progress::split_files(&reply.text);
    if files.is_empty() {
        anyhow::bail!("model produced no files — nothing was written");
    }
    for (name, content) in files {
        let path = match name.as_str() {
            "progress" => p_path.clone(),
            "approach" => a_path.clone(),
            "curriculum" => c_path.clone(),
            "profile" => match &pr_path {
                Some(p) => p.clone(),
                // global kök yoktu — uyarı zaten yukarıda verildi.
                None => continue,
            },
            other => {
                ui::warn(&format!("unknown closing file skipped: {other}"));
                continue;
            }
        };
        if content.is_empty() {
            ui::warn(&format!("empty content skipped: {name}"));
            continue;
        }
        progress::write_atomic(&path, &content)?;
        ui::notice(&format!("updated: {}", path.display()));
    }

    // Global kataloğu güncelle — başarısızlık progress yazımını geri almaz,
    // sadece not düşülür (katalog konfor katmanı, hafızanın kendisi değil).
    match &global {
        Some(g) => {
            if let Err(e) = index::record(g, &session.topic, project_root, &today()) {
                ui::warn(&format!("catalog could not be updated: {e}"));
            }
        }
        None => ui::warn("catalog could not be updated: no global root"),
    }

    Ok(())
}

/// Eşik aşıldıysa: ara-flush → system prompt'u taze dosyalarla yeniden yükle →
/// history'yi kırp → CLI oturumunu sıfırla. Flush başarısızsa kompaksiyon
/// İPTAL — veri diske inmeden history atılmaz.
pub(crate) async fn maybe_compact(
    backend: &mut Backend,
    session: &mut Session,
    project_root: &Path,
    tokens: Option<u64>,
) {
    let Some(t) = tokens else { return };
    if (t as f64) < COMPACT_THRESHOLD * backend.context_window() as f64 {
        return;
    }
    if session.history().len() <= COMPACT_KEEP_LAST {
        return;
    }
    ui::notice("context filling up — taking an interim checkpoint…");
    if let Err(e) = flush_progress(backend, session, project_root).await {
        ui::warn(&format!("interim checkpoint failed, compaction postponed: {e}"));
        return;
    }
    match config::global_root() {
        Ok(global) => {
            session.system =
                brain::load_system_prompt(&global, Some(project_root), &session.topic, &today());
        }
        Err(e) => ui::warn(&format!("system prompt could not be refreshed: {e}")),
    }
    session.compact(COMPACT_KEEP_LAST, COMPACT_NOTE);
    backend.reset_session();
    ui::notice("context compacted — pick up where you left off");
}

/// Bugünün yerel tarihi — katalog satırlarının tarih alanı.
fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Oturum dosya adı damgası — yerel saat.
fn now_stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

/// Konu kilidi: `.usta/.lock-<konu>` — eşzamanlı iki oturumun aynı progress'i
/// sessizce ezmesini önler. İçerik: pid (teşhis için).
pub(crate) fn lock_path(project_root: &Path, topic: &str) -> PathBuf {
    project_root.join(".usta").join(format!(".lock-{topic}"))
}

/// Deadline varsa ona kadar uyu; yoksa asla dönmeyen future (select guard'ı
/// zaten bu kolu deadline'sız poll etmez — bu sadece tip güvenliği).
pub(crate) async fn sleep_until_deadline(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(d) => tokio::time::sleep_until(d).await,
        None => std::future::pending().await,
    }
}

/// Reset kapsamı.
#[derive(Debug, PartialEq)]
pub enum ResetTarget {
    /// Bulunduğun projede tek konunun progress'i.
    Topic(String),
    /// Bilinen tüm proje `.usta/`'ları + global brain — sıfır nokta.
    Factory,
    /// Global kullanıcı profili — gömülü jenerik şablona döner (yedekli).
    Profile,
}

/// Komut satırı komutu — argüman ayrıştırma tek yerde, saf ve test edilebilir.
#[derive(Debug, PartialEq)]
pub enum Command {
    /// `usta init` — iskelet kur, per-dosya durum yazdır.
    Init,
    /// `usta topics` — global katalogdan konu listesi.
    Topics,
    /// `usta reset <konu>` — progress sil (onaylı) + katalogdan düş.
    Reset(ResetTarget),
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
        Some("reset") => match rest.next().map(String::as_str) {
            Some("--factory") => Ok(Command::Reset(ResetTarget::Factory)),
            Some("--profile") | Some("--profil") => Ok(Command::Reset(ResetTarget::Profile)),
            Some(topic) => Ok(Command::Reset(ResetTarget::Topic(slugify_topic(topic)))),
            None => anyhow::bail!("usage: usta reset <topic>  |  --factory  |  --profile"),
        },
        Some(other) => anyhow::bail!(
            "unknown command: '{other}'. Commands: start [topic], init, topics, reset <topic>|--factory|--profile"
        ),
    }
}

/// Konuyu çöz: açık argüman > TTY promptu > sessiz "genel" default'u.
/// Stdin pipe'lanmışsa (TTY değilse) cevaplanamayacak bir prompt'a takılmadan
/// direkt "genel" döner. Kısa girdi yerel slug'lanır; cümle yazılırsa NE
/// öğrenmek istediğini modele çıkartıp en mantıklı slug'ı ona seçtiririz.
async fn resolve_topic(
    backend: &mut Backend,
    topic_arg: Option<String>,
    project_root: &Path,
    global: &Path,
) -> Result<(String, Option<String>)> {
    // Dönüş: (konu, intro) — intro = kullanıcının ham konu girişi; yeni konuda
    // tanışma turn'üne "ilk cevap" olarak taşınır (devam/pipe yollarında None).
    if let Some(raw) = topic_arg {
        let slug = slugify_topic(&raw);
        return Ok((slug, Some(raw)));
    }
    // Boş-stdin / pipe yolu DOKUNULMAZ: cevaplanamayacak prompt'a takılmadan "genel".
    if !std::io::stdin().is_terminal() {
        return Ok(("genel".to_string(), None));
    }
    // Bu projede devam edilebilir konuları göster — Enter = en sonuncusuna devam.
    let index_content =
        std::fs::read_to_string(global.join("learner/index.md")).unwrap_or_default();
    let local = index::local_topics(project_root, &index_content);
    if !local.is_empty() {
        println!("saved: {} — Enter = continue with {}", local.join(", "), local[0]);
    }
    let mut rl = DefaultEditor::new()?;
    // Yeni-konu onayı yalnız burada (plain yol) döngüye alınır: ret cevabı
    // "Konu nedir?" promptuna geri döner — TUI'deki reddet-tekrar-sor akışının
    // eşdeğeri. Resume/ilk-oturum yolları asla bu döngüde takılmaz.
    loop {
        let line = match rl.readline("What's the topic? (write it short or as a sentence): ") {
            Ok(l) => l,
            // Ctrl-D / Ctrl-C → engellemeden "genel"e düş.
            Err(_) => return Ok(("genel".to_string(), None)),
        };
        let raw = line.trim();
        // Konu girişi yorumu: devam mı, yeni konu mu? (spec K1). Plain yolda devam/yeni
        // ayrımı yalnız slug'a yansır — TUI'deki görsel notice farkı burada yok.
        match interpret_topic_input(raw, &local) {
            None => return Ok(("genel".to_string(), None)),
            Some(TopicChoice::Resume(t)) => return Ok((t, None)),
            Some(TopicChoice::New(raw)) => {
                // Kısa girdi (≤2 kelime) → yerel slug, boşuna LLM çağrısı yapma.
                let slug = if raw.split_whitespace().count() <= 2 {
                    slugify_topic(&raw)
                } else {
                    // Cümle → model ne istediğini çıkarıp slug seçsin (yerel konular K2 için).
                    derive_slug(backend, &raw, &local).await
                };
                if local.contains(&slug) {
                    // Model devam niyetini mevcut slug'a çözdü — onaysız devam.
                    return Ok((slug, None));
                }
                // İlk oturum (kayıtlı konu yok) → onay muafiyeti. Aksi halde sor.
                if local.is_empty()
                    || confirm(&format!("Open new topic '{slug}'? [y/N] "), &["e", "evet", "y", "yes"])?
                {
                    return Ok((slug, Some(raw)));
                }
                println!(
                    "cancelled — Enter = continue with {}, or type another topic",
                    local[0]
                );
                // Döngü başa döner: tekrar "Konu nedir?" sorulur.
            }
        }
    }
}

/// Companion (file-watch feedback) slash command. Slash lines never reach the LLM.
#[derive(Debug, PartialEq)]
pub(crate) enum WatchCmd { On, Off, Toggle }

pub(crate) fn parse_watch_command(line: &str) -> Option<WatchCmd> {
    match line.trim() {
        "/watch" => Some(WatchCmd::Toggle),
        "/watch on" => Some(WatchCmd::On),
        "/watch off" => Some(WatchCmd::Off),
        _ => None,
    }
}

pub(crate) fn apply_watch(cmd: WatchCmd, cur: bool) -> (bool, &'static str) {
    let next = match cmd {
        WatchCmd::On => true,
        WatchCmd::Off => false,
        WatchCmd::Toggle => !cur,
    };
    let msg = if next {
        "companion on — watching your files"
    } else {
        "companion paused — file feedback off"
    };
    (next, msg)
}

/// Cümleden konu slug'ı çıkaran system prompt — hem plain (`derive_slug`) hem
/// TUI konu girişi kullanır.
pub(crate) const SLUG_SYSTEM: &str = "Kullanıcının öğrenmek/yapmak istediğini TEK kısa dosya-adı slug'ına indir. \
    Kurallar: yalnız küçük harf, ascii (Türkçe karakter yok), kelimeler tire ile ayrılır, \
    EN FAZLA 3 kelime, dolgu kelimeleri (ben/bir/ile/yapmak/istiyorum) atılır. \
    SADECE slug'ı döndür — açıklama, tırnak, noktalama yok. \
    Örnek: 'ben rust ile bir todo yapmak istiyorum' -> rust-todo";

/// Slug sistem promptu — kayıtlı konular varsa devam-farkındalığı eklenir
/// (spec K2): model devam niyetini mevcut slug'a çevirir, akış Resume sayar.
pub(crate) fn slug_system(known: &[String]) -> String {
    if known.is_empty() {
        return SLUG_SYSTEM.to_string();
    }
    format!(
        "{SLUG_SYSTEM}\n\nMevcut konular: {list}. Kullanıcının yazdığı bu konulardan \
         birine DEVAM ETME isteğiyse (aynı işin sürdürülmesi, 'kaldığımız yer', önceki \
         çalışmaya atıf) SADECE o konunun slug'ını AYNEN döndür. Yeni bir konuysa yeni slug üret.",
        list = known.join(", ")
    )
}

/// Model slug cevabını nihai slug'a çevir — tireleri boşluğa çevirip `slugify_topic`
/// ile garantile; "genel"e düşerse ham girdiden yerel slug türet. Saf.
pub(crate) fn finalize_slug(raw: &str, model_reply: &str) -> String {
    let s = slugify_topic(&model_reply.trim().replace(['-', '_'], " "));
    if s == "genel" {
        slugify_topic(raw)
    } else {
        s
    }
}

/// Yeni konu onay metni (TUI tui_confirm için). Plain yol kendi `[e/H]`
/// rustyline formatını kullanır — sözcükler kasıtlı farklı, iki yüzey ayrı.
pub(crate) fn new_topic_confirm_msg(slug: &str) -> String {
    format!("yeni konu: {slug} — açayım mı? [e = evet / başka tuş = geri dön]")
}

/// Konu girişi yorumu: devam mı, yeni konu mu? (spec K1)
#[derive(Debug)]
pub(crate) enum TopicChoice {
    /// Mevcut proje-yerel konuya devam.
    Resume(String),
    /// Yeni konu akışı — ham girdi (slug'lama çağıranda).
    New(String),
}

/// Deterministik seçim kuralları — sıra spec §3/K1 tablosu. `None` = girdiyi
/// yut (boş + devam edilecek konu yok). LLM'siz; cümleler `New` döner, K2
/// (slug_system) orada devreye girer.
pub(crate) fn interpret_topic_input(raw: &str, local: &[String]) -> Option<TopicChoice> {
    let raw = raw.trim();
    // 1-2: boş Enter.
    if raw.is_empty() {
        return local.first().map(|t| TopicChoice::Resume(t.clone()));
    }
    // 3: rakam seçimi.
    if let Ok(n) = raw.parse::<usize>() {
        if n >= 1 && n <= local.len() {
            return Some(TopicChoice::Resume(local[n - 1].clone()));
        }
    }
    // 4: slug eşleşmesi.
    let slug = slugify_topic(raw);
    if let Some(t) = local.iter().find(|t| **t == slug) {
        return Some(TopicChoice::Resume(t.clone()));
    }
    // 5: kısa devam-kalıbı (deasciify sonrası substring).
    if !local.is_empty() && raw.split_whitespace().count() <= 4 {
        let d: String = raw.chars().map(deasciify).collect::<String>().to_lowercase();
        const RESUME_WORDS: &[&str] = &["devam", "kaldigimiz", "kaldigim", "continue", "resume"];
        if RESUME_WORDS.iter().any(|w| d.contains(w)) {
            return Some(TopicChoice::Resume(local[0].clone()));
        }
    }
    // 6: yeni konu.
    Some(TopicChoice::New(raw.to_string()))
}

/// Cümleden konu slug'ını modele çıkart (plain yol). Hata → yerel slug.
/// Çağrı sonrası CLI oturumu KOŞULSUZ sıfırlanır — slug mini-oturumu
/// öğrenme oturumuna resume edilip bağlamı kirletmesin (spec B1).
async fn derive_slug(backend: &mut Backend, raw: &str, known: &[String]) -> String {
    let history = [Message::user(raw)];
    let out = match ask_usta(backend, &slug_system(known), &history).await {
        Ok(reply) => finalize_slug(raw, &reply.text),
        Err(_) => slugify_topic(raw),
    };
    backend.reset_session();
    out
}

/// Türkçe harfi ascii'ye indir + küçük harfe çevir; diğerlerini küçült.
fn deasciify(c: char) -> char {
    match c {
        'ç' | 'Ç' => 'c',
        'ğ' | 'Ğ' => 'g',
        'ı' | 'İ' | 'I' => 'i',
        'ö' | 'Ö' => 'o',
        'ş' | 'Ş' => 's',
        'ü' | 'Ü' => 'u',
        other => other.to_ascii_lowercase(),
    }
}

/// Serbest metni konu slug'ına çevir — saf fonksiyon, test edilebilir.
/// Kural: Türkçe karakterleri sadeleştir, küçük harfe çevir, en fazla İLK 3
/// kelimeyi al, her kelimede yalnız ascii alfanümerik karakterleri tut,
/// kelimeleri tire ile birleştir. Sonuç boşsa `"genel"`.
/// "temel Linux güvenliği" → `temel-linux-guvenligi`.
pub fn slugify_topic(input: &str) -> String {
    // Deasciified (ç→c…) haliyle karşılaştırılan dolgu kelimeleri — slug'a
    // girmez ki "ben rust ile bir todo yapmak istiyorum" → "rust-todo".
    const STOPWORDS: &[&str] = &[
        "ben", "bir", "ile", "ve", "icin", "bu", "su", "yapmak", "yapmayi",
        "istiyorum", "ogrenmek", "ogreniyorum", "istiyor", "bana", "de", "da",
        "the", "a", "an", "to", "learn", "want", "make", "build",
    ];
    let words: Vec<String> = input
        .split_whitespace()
        .map(|w| {
            w.chars()
                .map(deasciify)
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
        })
        .filter(|w| !w.is_empty() && !STOPWORDS.contains(&w.as_str()))
        .take(3)
        .collect();
    if words.is_empty() {
        "genel".to_string()
    } else {
        words.join("-")
    }
}

/// `.usta/` iskeletini tembel kurar — `start`'ın kendi kendini bootstrap
/// etmesini sağlar, `usta init`'i opsiyonel yapar. (1) global brain kökünü
/// (`~/.config/usta`) tamamlar: kod-sahipli dosyalar gömülüyle senkronlanır,
/// kullanıcı-sahipliler korunur. (2) proje kökü yukarı doğru aranıp bulunamazsa `cwd`'de
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

/// `usta init` — global brain'i (`~/.config/usta`) varsayılanlarla doldurur
/// (kod-sahipliler senkronlanır, kullanıcı-sahiplilerin üstüne YAZMAZ) ve
/// CWD'de proje `.usta/` iskeletini kurar. Global brain "bir kere kurulur, tüm projelerde paylaşılır";
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

    println!("Ready. Start with 'usta start <topic>'.");
    Ok(())
}

/// `usta topics` — global katalogdaki kayıtları listele. LLM gerekmez.
fn run_topics() -> Result<()> {
    let global = config::global_root()?;
    let content =
        std::fs::read_to_string(global.join("learner/index.md")).unwrap_or_default();
    let list = index::entries(&content);
    if list.is_empty() {
        println!("No saved topics — start with 'usta start <topic>'.");
        return Ok(());
    }
    println!("Topic | Project | Last session");
    for e in list {
        println!("{} | {} | {}", e.topic, e.project.display(), e.date);
    }
    Ok(())
}

/// `usta reset <konu>` — bulunduğun projenin o konudaki progress'ini sil
/// (onaylı) ve global katalogdan düş. LLM gerekmez.
fn run_reset_topic(topic: &str) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let Some(root) = config::find_project_root(&cwd) else {
        anyhow::bail!("no .usta in this directory (or above) — no project found to reset");
    };
    let path = progress::progress_path(&root, topic);
    if !path.is_file() {
        println!("no record: {}", path.display());
        return Ok(());
    }
    if !confirm(&format!("{} will be deleted. Are you sure? [y/N] ", path.display()), &["e", "evet", "y", "yes"])? {
        println!("cancelled.");
        return Ok(());
    }
    std::fs::remove_file(&path)
        .with_context(|| format!("could not delete: {}", path.display()))?;
    println!("deleted: {}", path.display());

    // Katalogdan da düş — katalog yoksa/okunamıyorsa sessizce geç.
    let global = config::global_root()?;
    let index_path = global.join("learner/index.md");
    if let Ok(current) = std::fs::read_to_string(&index_path) {
        let updated = index::remove(&current, topic, &root);
        progress::write_atomic(&index_path, &updated)?;
    }
    Ok(())
}

/// `usta reset --factory` — katalogdaki tüm projelerin `.usta/`'sı + global
/// brain silinir. Sonraki `usta` çalıştırması her şeyi varsayılanlardan
/// baştan kurar (bootstrap) — Usta kullanıcıyı hiç tanımamış gibi başlar.
fn run_reset_factory() -> Result<()> {
    let global = config::global_root()?;
    let index_content =
        std::fs::read_to_string(global.join("learner/index.md")).unwrap_or_default();
    let mut targets: Vec<PathBuf> = index::entries(&index_content)
        .into_iter()
        .map(|e| e.project.join(".usta"))
        .filter(|p| p.is_dir())
        .collect();
    targets.sort();
    targets.dedup();

    println!("FACTORY RESET — will be deleted:");
    for t in &targets {
        println!("  {}", t.display());
    }
    println!("  {} (global brain)", global.display());
    println!("Note: old projects not in the catalog are NOT in this list.");
    println!("Check: find ~ -maxdepth 5 -name .usta -type d");

    if !confirm("Everything will be permanently deleted. Type 'yes' to confirm: ", &["evet", "yes"])? {
        println!("cancelled.");
        return Ok(());
    }
    for t in &targets {
        std::fs::remove_dir_all(t)
            .with_context(|| format!("could not delete: {}", t.display()))?;
        println!("deleted: {}", t.display());
    }
    if global.is_dir() {
        std::fs::remove_dir_all(&global)
            .with_context(|| format!("could not delete: {}", global.display()))?;
        println!("deleted: {}", global.display());
    }
    println!("Zero point. The next 'usta' run will set everything up from scratch.");
    Ok(())
}

/// Profil hâlâ gömülü jenerik şablon mu? (= Usta kullanıcıyı henüz tanımıyor.)
/// Trim'li karşılaştırma — satır sonu/boşluk farkı yanlış-negatif üretmesin.
pub(crate) fn profile_is_generic(disk: &str) -> bool {
    defaults::global_defaults()
        .into_iter()
        .find(|(rel, _, _)| *rel == "USER.md")
        .map(|(_, c, _)| c.trim() == disk.trim())
        .unwrap_or(false)
}

/// Profil sıfırlama çekirdeği — SAF (onay yok, global_root yok): mevcut
/// profili `.bak`'a al, gömülü jenerik şablonu yaz. Konu progress'lerine
/// DOKUNMAZ (spec Ç2).
fn reset_profile_files(global: &Path) -> Result<()> {
    let sablon = defaults::global_defaults()
        .into_iter()
        .find(|(rel, _, _)| *rel == "USER.md")
        .map(|(_, c, _)| c)
        .context("embedded profile template not found")?;
    let path = global.join("USER.md");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create directory: {}", parent.display()))?;
    }
    if path.exists() {
        std::fs::copy(&path, path.with_extension("md.bak"))
            .with_context(|| format!("could not back up: {}", path.display()))?;
    }
    std::fs::write(&path, sablon)
        .with_context(|| format!("could not write: {}", path.display()))?;
    Ok(())
}

/// `usta reset --profile` — onaylı; Usta kullanıcıyı "tanımadan" başlar.
/// Yıkıcı işlem: TTY yoksa (onay alınamayacak durumda) sessizce koşmak
/// yerine hatayla çıkar — `confirm()` boş stdin'de "hayır"a düşse de, bu
/// davranış pipe'ın içeriğine bağımlı kalmasın diye burada açıkça bekleniyor.
fn run_reset_profile() -> Result<()> {
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("no TTY — cannot get confirmation, profile not reset. Run in an interactive terminal.");
    }
    let global = config::global_root()?;
    let path = global.join("USER.md");
    if !confirm(
        &format!(
            "Profile will be reset — Usta will start not knowing you (backup: {}.bak). Continue? [y/N] ",
            path.display()
        ),
        &["e", "evet", "y", "yes"],
    )? {
        println!("cancelled — profile unchanged.");
        return Ok(());
    }
    reset_profile_files(&global)?;
    println!("profile reset: {} (old version in .bak)", path.display());
    Ok(())
}

/// Onay iste: stdin'den tek satır oku, kabul listesiyle (küçük harf)
/// karşılaştır. Stdin kapalı/boş = hayır — güvenli varsayılan.
fn confirm(prompt: &str, yes: &[&str]) -> Result<bool> {
    use std::io::Write;
    print!("{prompt}");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(yes.contains(&line.trim().to_lowercase().as_str()))
}

/// `usta init`'in per-dosya/dizin durum satırı.
fn print_scaffold_status(path: &Path, wrote: bool) {
    if wrote {
        println!("written: {}", path.display());
    } else {
        println!("already exists, skipped: {}", path.display());
    }
}

/// Eski profil konumundan (önceki `learner/` alt yolu) yeni köke (`USER.md`)
/// tek-seferlik geçiş. Eski dosya var + yeni yoksa taşır (`true`); aksi halde no-op
/// (`false`) — mevcut `USER.md` asla ezilmez, veri kaybı riski alınmaz.
fn migrate_profile_to_user_md(global: &Path) -> Result<bool> {
    let old = global.join("learner/profile.md");
    let new = global.join("USER.md");
    if old.exists() && !new.exists() {
        std::fs::rename(&old, &new)
            .with_context(|| format!("profile could not be moved: {} → {}", old.display(), new.display()))?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Global brain kökünü oluşturup varsayılan dosyaları yazar. Kod-sahipli
/// dosyalar (USTA.md, approaches/*) gömülü içerikle senkronlanır — eskiyse
/// üstüne yazılır; kullanıcı-sahipli dosyalar (learner/*, USER.md) yalnız
/// ilk-kez yazılır (`defaults::Ownership`). Her dosya için `(yol, yazıldı-mı)`
/// döner — `run_init` bunu yazdırır, `ensure_scaffold` sessizce yutar.
/// Yazım döngüsünden ÖNCE eski profil konumundan `USER.md`'ye geçiş
/// çalıştırılır (spec §5 sıra şartı) — böylece hem `ensure_scaffold` hem
/// `run_init` (ikisi de bu fonksiyonu çağırır) mevcut kullanıcı verisini
/// korur, `USER.md`'nin `Ownership::User` ilk-kez-yaz kuralı taşınan dosyanın
/// üstüne yazmaz.
fn write_global_defaults(global: &Path) -> Result<Vec<(PathBuf, bool)>> {
    std::fs::create_dir_all(global)
        .with_context(|| format!("could not create global root: {}", global.display()))?;
    migrate_profile_to_user_md(global)?;

    let mut results = Vec::new();
    for (rel, content, ownership) in defaults::global_defaults() {
        let path = global.join(rel);
        let write_needed = match ownership {
            defaults::Ownership::Code => config::needs_sync(&path, content),
            defaults::Ownership::User => config::should_write(&path),
        };
        let wrote = if write_needed {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("could not create directory: {}", parent.display()))?;
            }
            std::fs::write(&path, content)
                .with_context(|| format!("could not write: {}", path.display()))?;
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
            .with_context(|| format!("could not create directory: {}", dir.display()))?;
        results.push((dir.clone(), !dir_existed));

        // .gitkeep — boş dizin de commit edilebilsin.
        let gitkeep = dir.join(".gitkeep");
        if config::should_write(&gitkeep) {
            std::fs::write(&gitkeep, "")
                .with_context(|| format!("could not write: {}", gitkeep.display()))?;
        }
    }

    Ok(results)
}

/// Dosya değişikliği feedback sonucu — çağıran (plain/TUI) kendi basar.
/// `handle_file_change` hiçbir şey println! etmez (raw-mode'da stdout
/// bozulmasın); tam `Reply` taşınır ki `web` bayrağı ve context gauge
/// çağıran tarafta orijinal davranışla (print_reply) yeniden üretilebilsin.
pub(crate) enum FileFeedback {
    /// Skip — çıktı yok.
    Sessiz,
    /// Büyük dosya bildirimi — çağıran kendi yolunda gösterir
    /// (plain: `println!`, TUI: `page_notice`).
    Bildirim(String),
    /// Gerçek yanıt — bağlam token'ı + tam `Reply` (web bayrağı korunur).
    Yanit {
        tokens: Option<u64>,
        reply: backend::Reply,
    },
}

/// Kaydedilen dosyayı FileMemory'den geçir; ilk görüşte tam içerik, sonrasında
/// diff olarak sentetik user turn'e çevir → Socratic feedback. Cargo projesiyse
/// check sonucu "sadece Usta'nın gözü için" bloğuyla eklenir (tahmin protokolü).
/// Çıktı BASMAZ — hem plain hem TUI yolu dönen `FileFeedback`'i kendi sunum
/// diliyle basar (raw-mode'da stdout bozulmasın).
pub(crate) async fn handle_file_change(
    backend: &mut Backend,
    session: &mut Session,
    files: &mut feedback::FileMemory,
    project_root: &Path,
    path: &Path,
    recorder: &transcript::Recorder,
) -> Result<FileFeedback> {
    let contents = std::fs::read_to_string(path)?;
    let mut injected = match files.observe(path, contents) {
        feedback::ChangePayload::Skip => return Ok(FileFeedback::Sessiz),
        feedback::ChangePayload::TooLarge(len) => {
            return Ok(FileFeedback::Bildirim(format!(
                "(large file — not watched: {} — {len} bytes)",
                path.display()
            )));
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
    recorder.user(&injected);
    let reply = ask_usta(backend, &session.system, session.history()).await?;
    let tokens = reply.context_tokens;
    recorder.assistant(&reply.text);
    session.push_assistant(reply.text.clone());
    Ok(FileFeedback::Yanit { tokens, reply })
}

/// Usta yanıtını sunum katmanına devret.
fn print_reply(reply: &backend::Reply, window: u64) {
    ui::print_usta_reply(&reply.text, reply.web);
    ui::context_gauge(reply.context_tokens, window);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_target_maps_profile_to_global_other_three_to_project() {
        let project = Path::new("/proje");
        let global = Path::new("/glob");
        assert_eq!(
            flush_target("profile", project, global, "rust"),
            Some(PathBuf::from("/glob/USER.md"))
        );
        assert_eq!(
            flush_target("progress", project, global, "rust"),
            Some(PathBuf::from("/proje/.usta/learner/progress/rust.md"))
        );
        assert_eq!(
            flush_target("approach", project, global, "rust"),
            Some(PathBuf::from("/proje/.usta/approaches/rust.md"))
        );
        assert_eq!(
            flush_target("curriculum", project, global, "rust"),
            Some(PathBuf::from("/proje/.usta/learner/curriculum/rust.md"))
        );
    }

    #[test]
    fn flush_target_rejects_unknown_name() {
        assert_eq!(
            flush_target("bilinmeyen", Path::new("/proje"), Path::new("/glob"), "rust"),
            None
        );
    }

    #[test]
    fn slugify_lowercases_simple_word() {
        assert_eq!(slugify_topic("JavaScript"), "javascript");
    }

    #[test]
    fn slugify_hyphenates_short_phrase_and_deasciifies() {
        assert_eq!(slugify_topic("temel Linux güvenliği"), "temel-linux-guvenligi");
        assert_eq!(slugify_topic("todo app"), "todo-app");
    }

    #[test]
    fn slugify_drops_non_alnum_chars() {
        assert_eq!(slugify_topic("C++"), "c");
    }

    #[test]
    fn slugify_caps_at_three_content_words() {
        assert_eq!(slugify_topic("alfa beta gama delta"), "alfa-beta-gama");
    }

    #[test]
    fn slugify_strips_stopwords_from_sentence() {
        // "ben ... ile bir ... yapmak istiyorum" dolgu kelimeleri düşer.
        assert_eq!(
            slugify_topic("ben rust ile bir todo uygulaması yapmak istiyorum"),
            "rust-todo-uygulamasi"
        );
        assert_eq!(slugify_topic("Rust öğreniyorum"), "rust");
    }

    #[test]
    fn slugify_blank_input_falls_back_to_genel() {
        assert_eq!(slugify_topic("   "), "genel");
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

    #[test]
    fn parse_reset_topic_is_slugified() {
        let args = vec!["usta".into(), "reset".into(), "C++".into()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Reset(ResetTarget::Topic("c".to_string()))
        );
    }

    #[test]
    fn parse_reset_without_arg_errors() {
        assert!(parse_command(&["usta".into(), "reset".into()]).is_err());
    }

    #[test]
    fn parse_reset_factory_flag() {
        let args = vec!["usta".into(), "reset".into(), "--factory".into()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Reset(ResetTarget::Factory)
        );
    }

    #[test]
    fn parse_reset_profile_flag_both_spellings() {
        let args = |s: &str| vec!["usta".to_string(), "reset".to_string(), s.to_string()];
        assert_eq!(parse_command(&args("--profile")).unwrap(), Command::Reset(ResetTarget::Profile));
        assert_eq!(parse_command(&args("--profil")).unwrap(), Command::Reset(ResetTarget::Profile));
        // Regresyon: konu ve factory aynen.
        assert_eq!(parse_command(&args("--factory")).unwrap(), Command::Reset(ResetTarget::Factory));
        assert!(matches!(parse_command(&args("rust")).unwrap(), Command::Reset(ResetTarget::Topic(t)) if t == "rust"));
    }

    #[test]
    fn profile_is_generic_matches_embedded_template_only() {
        let sablon = defaults::global_defaults()
            .into_iter()
            .find(|(rel, _, _)| *rel == "USER.md")
            .map(|(_, c, _)| c)
            .unwrap();
        assert!(profile_is_generic(sablon));
        assert!(profile_is_generic(&format!("{sablon}\n"))); // satır sonu toleransı
        assert!(!profile_is_generic("# Öğrenci Profili — Anil\nkişisel"));
    }

    #[test]
    fn reset_profile_files_backs_up_and_writes_generic_template() {
        let base = std::env::temp_dir().join(format!("usta_reset_profile_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("USER.md"), "# Öğrenci Profili — Anil\nkişisel notlar").unwrap();

        reset_profile_files(&base).unwrap();

        let yeni = std::fs::read_to_string(base.join("USER.md")).unwrap();
        let sablon = defaults::global_defaults()
            .into_iter()
            .find(|(rel, _, _)| *rel == "USER.md")
            .map(|(_, c, _)| c)
            .unwrap();
        assert_eq!(yeni, sablon); // jenerik şablona eşit
        assert_eq!(
            std::fs::read_to_string(base.join("USER.md.bak")).unwrap(),
            "# Öğrenci Profili — Anil\nkişisel notlar"
        ); // eski içerik yedekte
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn reset_profile_files_works_without_existing_profile() {
        let base = std::env::temp_dir().join(format!("usta_reset_profile_yok_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        reset_profile_files(&base).unwrap(); // dosya yokken de: dizin kurulur, şablon yazılır, .bak yok
        assert!(base.join("USER.md").exists());
        assert!(!base.join("USER.md.bak").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn migrate_moves_old_profile_once() {
        let base = std::env::temp_dir().join(format!("usta_migrate_moves_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("learner")).unwrap();
        std::fs::write(base.join("learner/profile.md"), "KIŞISEL").unwrap();

        let moved = migrate_profile_to_user_md(&base).unwrap();
        assert!(moved);
        assert_eq!(std::fs::read_to_string(base.join("USER.md")).unwrap(), "KIŞISEL");
        assert!(!base.join("learner/profile.md").exists());

        // İkinci çağrı: eski yol artık yok → no-op.
        let moved_again = migrate_profile_to_user_md(&base).unwrap();
        assert!(!moved_again);
        assert_eq!(std::fs::read_to_string(base.join("USER.md")).unwrap(), "KIŞISEL");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn migrate_never_overwrites_existing_user_md() {
        let base = std::env::temp_dir().join(format!("usta_migrate_no_overwrite_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("learner")).unwrap();
        std::fs::write(base.join("learner/profile.md"), "ESKİ").unwrap();
        std::fs::write(base.join("USER.md"), "YENİ").unwrap();

        let moved = migrate_profile_to_user_md(&base).unwrap();
        assert!(!moved);
        assert_eq!(std::fs::read_to_string(base.join("USER.md")).unwrap(), "YENİ");
        // Veri kaybı riski alınmaz — eski dosya da yerinde bırakılır.
        assert_eq!(std::fs::read_to_string(base.join("learner/profile.md")).unwrap(), "ESKİ");

        let _ = std::fs::remove_dir_all(&base);
    }

    /// `write_project_scaffold` bir temp dizinde `.usta/` iskeletini kurar —
    /// `global_root()`'a hiç dokunmadan (gerçek `~/.config`'i etkilemez).
    #[test]
    fn write_global_defaults_syncs_code_owned_preserves_user_owned() {
        let base = std::env::temp_dir().join(format!(
            "usta_main_test_global_sync_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);

        // İlk yazım: her şey yazılır.
        let first = write_global_defaults(&base).unwrap();
        assert!(first.iter().all(|(_, wrote)| *wrote));

        // Kirlet: kod-sahipli USTA.md eskisin, kullanıcı-sahipli profile düzenlensin.
        std::fs::write(base.join("USTA.md"), "eski sürüm").unwrap();
        std::fs::write(base.join("USER.md"), "kullanıcı düzenlemesi").unwrap();

        write_global_defaults(&base).unwrap();

        // Kod-sahipli senkronlandı — gömülü güncel içerik geri geldi.
        // Not: USTA.md, Task 1'in brain-split'iyle davranışsız bir indekse
        // dönüştü ("Sert Kurallar" artık RULES.md'de) — assertion güncel
        // gömülü içeriğe göre düzeltildi.
        let usta = std::fs::read_to_string(base.join("USTA.md")).unwrap();
        assert!(usta.contains("Müdahale Haritası"));
        // Kullanıcı-sahipli korundu.
        assert_eq!(
            std::fs::read_to_string(base.join("USER.md")).unwrap(),
            "kullanıcı düzenlemesi"
        );

        // Değişiklik yokken hiçbir şey yeniden yazılmaz.
        let third = write_global_defaults(&base).unwrap();
        assert!(third.iter().all(|(_, wrote)| !*wrote));

        let _ = std::fs::remove_dir_all(&base);
    }

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

    #[test]
    fn finalize_slug_uses_model_reply_then_slugifies() {
        // Model tire'li slug döndürür → tireler korunur, slugify garantiler.
        assert_eq!(finalize_slug("ben golang öğrenmek istiyorum", "golang-web"), "golang-web");
        // Model gürültülü döndürürse yine slug'lanır.
        assert_eq!(finalize_slug("x", "Rust Todo"), "rust-todo");
    }

    #[test]
    fn finalize_slug_falls_back_to_raw_when_model_gives_genel() {
        // Model "genel" derse ham girdiden yerel slug türet.
        assert_eq!(finalize_slug("temel linux güvenliği", "genel"), "temel-linux-guvenligi");
    }

    #[test]
    fn slug_system_injects_known_topics() {
        let s = slug_system(&["linux-guvenlik".to_string(), "rust".to_string()]);
        assert!(s.contains("linux-guvenlik, rust"));
        assert!(s.contains("DEVAM"));
    }

    #[test]
    fn slug_system_without_topics_is_base_only() {
        let s = slug_system(&[]);
        assert!(s.contains("slug"));
        assert!(!s.contains("Mevcut konular"));
    }

    #[test]
    fn interpret_empty_resumes_latest_or_swallows() {
        let local = vec!["son-konu".to_string(), "eski".to_string()];
        assert!(matches!(interpret_topic_input("", &local), Some(TopicChoice::Resume(t)) if t == "son-konu"));
        assert!(interpret_topic_input("  ", &[]).is_none()); // konu yok → yut
    }

    #[test]
    fn interpret_digit_selects_from_list_out_of_range_is_new() {
        let local = vec!["a".to_string(), "b".to_string()];
        assert!(matches!(interpret_topic_input("2", &local), Some(TopicChoice::Resume(t)) if t == "b"));
        assert!(matches!(interpret_topic_input("5", &local), Some(TopicChoice::New(r)) if r == "5"));
    }

    #[test]
    fn interpret_existing_slug_match_resumes() {
        let local = vec!["linux-guvenlik".to_string()];
        // Slugify eşleşmesi: Türkçe yazım da yakalanır.
        assert!(matches!(
            interpret_topic_input("Linux Güvenlik", &local),
            Some(TopicChoice::Resume(t)) if t == "linux-guvenlik"
        ));
    }

    #[test]
    fn interpret_resume_phrases_short_input_only() {
        let local = vec!["son-konu".to_string()];
        for s in ["devam", "devam edelim", "kaldığımız yerden devam", "continue", "resume"] {
            assert!(matches!(interpret_topic_input(s, &local), Some(TopicChoice::Resume(t)) if t == "son-konu"), "{s}");
        }
        // >4 kelime → LLM'e/yeni akışa (K2 yakalar).
        assert!(matches!(
            interpret_topic_input("devam edelim ama bu sefer docker öğrenelim", &local),
            Some(TopicChoice::New(_))
        ));
        // Devam kalıbı ama hiç konu yok → yeni konu.
        assert!(matches!(interpret_topic_input("devam", &[]), Some(TopicChoice::New(_))));
    }

    #[test]
    fn interpret_other_input_is_new() {
        let local = vec!["son-konu".to_string()];
        assert!(matches!(interpret_topic_input("docker compose", &local), Some(TopicChoice::New(r)) if r == "docker compose"));
    }

    #[test]
    fn new_topic_confirm_msg_names_slug_and_keys() {
        let m = new_topic_confirm_msg("rust-cli");
        assert!(m.contains("rust-cli"));
        assert!(m.contains("[e"));
    }

    #[test]
    fn parse_watch_command_variants() {
        assert_eq!(parse_watch_command("/watch"), Some(WatchCmd::Toggle));
        assert_eq!(parse_watch_command("/watch on"), Some(WatchCmd::On));
        assert_eq!(parse_watch_command("/watch off"), Some(WatchCmd::Off));
        assert_eq!(parse_watch_command("  /watch off  "), Some(WatchCmd::Off));
        assert_eq!(parse_watch_command("hello"), None);
        assert_eq!(parse_watch_command("/quit"), None);
    }

    #[test]
    fn apply_watch_transitions() {
        assert_eq!(apply_watch(WatchCmd::Off, true).0, false);
        assert_eq!(apply_watch(WatchCmd::On, false).0, true);
        assert_eq!(apply_watch(WatchCmd::Toggle, true).0, false);
        assert_eq!(apply_watch(WatchCmd::Toggle, false).0, true);
        assert!(apply_watch(WatchCmd::On, false).1.contains("on"));
        assert!(apply_watch(WatchCmd::Off, true).1.contains("off"));
    }
}
