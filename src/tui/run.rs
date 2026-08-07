//! TUI oturum döngüsü: tuş + watcher + LLM tek select!'te. Kalıcı içerik
//! insert_before ile scrollback'e akar; alt bölge canlı çizilir. Spec §3.
//!
//! Plain modda (ui::is_plain) bu modül hiç kullanılmaz — main dallanması
//! plain yolu run_plain_loop'a yönlendirir. Burada alt-ekran YOK: yalnız
//! inline viewport + insert_before, scrollback korunur.

use std::path::{Path, PathBuf};

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent};
use futures_util::StreamExt;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Paragraph, Widget};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::anthropic::Message;
use crate::backend::Backend;
use crate::session::Session;
use crate::transcript::Recorder;
use crate::tui::convert::ansi_to_text;
use crate::tui::editor::{Action, InputBox};
use crate::tui::status::{render_status, Status};
use crate::tui::term::{Tui, VIEWPORT_H};
use crate::tui::welcome;
use crate::{feedback, progress, ui, watcher};

/// Kalıcı içeriği viewport üstüne (scrollback'e) bas.
fn page(tui: &mut Tui, text: Text<'static>) -> Result<()> {
    let h = text.height() as u16;
    tui.terminal.insert_before(h, |buf| {
        Paragraph::new(text).render(buf.area, buf);
    })?;
    Ok(())
}

/// Usta yanıtını görsel dille bas: turuncu ● satırı + markdown + boş satır.
fn page_reply(tui: &mut Tui, reply: &str, width: u16) -> Result<()> {
    let ansi = ui::render_markdown(reply, width as usize);
    let mut t = ansi_to_text(&format!("\x1b[38;5;208m●\x1b[0m\n{ansi}\n"));
    t.lines.push(Line::raw(""));
    page(tui, t)
}

/// Soluk sistem bildirimi (ui::notice'un TUI karşılığı).
fn page_notice(tui: &mut Tui, msg: &str) -> Result<()> {
    page(tui, ansi_to_text(&format!("\x1b[2m· {msg}\x1b[0m")))
}

/// Alt bölgeyi çiz: girdi kutusu (üstte) + durum satırı (altta).
fn draw(
    tui: &mut Tui,
    editor: &InputBox,
    status: &Status,
    tokens: Option<u64>,
    window: u64,
) -> Result<()> {
    tui.terminal.draw(|f| {
        let [box_area, status_area] =
            Layout::vertical([Constraint::Length(VIEWPORT_H - 1), Constraint::Length(1)])
                .areas(f.area());
        editor.render(f, box_area);
        f.render_widget(render_status(status, tokens, window), status_area);
    })?;
    Ok(())
}

fn draw_locked(
    tui: &mut Tui,
    editor: &InputBox,
    frame: usize,
    tokens: Option<u64>,
    window: u64,
) -> Result<()> {
    draw(tui, editor, &Status::Thinking { frame }, tokens, window)
}

/// Kilitli modda tuş: Enter ve Ctrl-C/D yutulur, gerisi editöre gider —
/// tek-turn ilkesi (yanıt beklenirken yeni turn başlatılamaz).
fn editor_key_locked(editor: &mut InputBox, k: KeyEvent) -> Action {
    if matches!(k.code, KeyCode::Enter) {
        return Action::None;
    }
    match editor.handle_key(k) {
        Action::Exit => Action::None, // kapanış sadece idle'da
        other => other,
    }
}

/// LLM çağrısını canlı arayüzle bekle: spinner döner, tuşlar editöre işler
/// ama Submit/Exit KİLİTLİ (tek turn ilkesi) — Enter yutulur.
async fn ask_live(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
    backend: &mut Backend,
    system: &str,
    history: &[Message],
    tokens: Option<u64>,
) -> Result<crate::backend::Reply> {
    let window = backend.context_window();
    let fut = backend.complete(system, history);
    tokio::pin!(fut);
    let mut frame = 0usize;
    loop {
        draw_locked(tui, editor, frame, tokens, window)?;
        tokio::select! {
            r = &mut fut => return r,
            Some(Ok(ev)) = events.next() => {
                if let Event::Key(k) = ev {
                    // Enter/Ctrl-C burada işlem başlatamaz — sadece edit tuşları.
                    let _ = editor_key_locked(editor, k);
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(120)) => { frame += 1; }
        }
    }
}

/// TUI oturumu: açılış kutusu + drill/tanışma + ana döngü. Dönüşte Tui drop
/// olur → terminal restore; kapanış flush'ı main'de plain yolla koşar.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    backend: &mut Backend,
    session: &mut Session,
    recorder: &Recorder,
    project_root: &Path,
    global: &Path,
    topic: &str,
    has_progress: bool,
    max_feedback_batch: usize,
    watch_rx: &mut UnboundedReceiver<PathBuf>,
) -> Result<()> {
    // setup() panic hook'u kurar → SÜREÇ BAŞINA TAM BİR KEZ çağrılır (döngüde değil).
    let mut tui = crate::tui::term::setup()?;
    let mut editor = InputBox::new();
    let mut events = EventStream::new();
    let mut debouncer = watcher::Debouncer::new(std::time::Duration::from_millis(1000));
    let mut files = feedback::FileMemory::new();
    let mut last_tokens: Option<u64> = None;
    let window = backend.context_window();

    // Açılış kutusu — bir kere, scrollback'e.
    let width = tui.terminal.size()?.width;
    let read = |p: PathBuf| std::fs::read_to_string(p).ok();
    let data = welcome::gather(
        read(global.join("learner/profile.md")).as_deref(),
        read(progress::progress_path(project_root, topic)).as_deref(),
        read(progress::curriculum_path(project_root, topic)).as_deref(),
        topic,
        &backend.label(),
        &short_dir(project_root),
    );
    page(&mut tui, welcome::render_welcome(&data, width))?;

    // Açılış drilli / tanışma (main.rs plain yolunun TUI karşılığı).
    let opening = if has_progress {
        progress::opening_prompt(topic)
    } else {
        progress::onboarding_prompt(topic)
    };
    session.push_user(&opening);
    recorder.user(&opening);
    match ask_live(
        &mut tui,
        &mut editor,
        &mut events,
        backend,
        &session.system,
        session.history(),
        last_tokens,
    )
    .await
    {
        Ok(reply) => {
            last_tokens = reply.context_tokens;
            page_reply(&mut tui, &reply.text, width)?;
            recorder.assistant(&reply.text);
            session.push_assistant(reply.text);
        }
        Err(e) => page_notice(&mut tui, &format!("açılış turu atlandı: {e}"))?,
    }

    loop {
        // Tamponu her iterasyon başında boşalt — transcript yazım hatası gibi
        // maybe_compact dışında biriken bildirimler de asla kaybolmasın.
        for m in ui::drain_tui_notices() {
            page_notice(&mut tui, &m)?;
        }
        draw(&mut tui, &editor, &Status::Idle, last_tokens, window)?;
        tokio::select! {
            Some(Ok(ev)) = events.next() => {
                let Event::Key(k) = ev else { continue };
                match editor.handle_key(k) {
                    Action::None => {}
                    Action::Exit => break,
                    Action::Submit(line) => {
                        if line == "/quit" { break; }
                        // Gönderilen satırı soluk iz olarak scrollback'e bas.
                        page(&mut tui, ansi_to_text(&format!("\x1b[2m│ > {line}\x1b[0m")))?;
                        session.push_user(&line);
                        recorder.user(&line);
                        match ask_live(
                            &mut tui, &mut editor, &mut events, backend,
                            &session.system, session.history(), last_tokens,
                        ).await {
                            Ok(reply) => {
                                last_tokens = reply.context_tokens;
                                page_reply(&mut tui, &reply.text, width)?;
                                recorder.assistant(&reply.text);
                                session.push_assistant(reply.text);
                                crate::maybe_compact(backend, session, project_root, last_tokens).await;
                            }
                            Err(e) => page_notice(&mut tui, &format!("hata: {e}"))?,
                        }
                    }
                }
            }
            Some(path) = watch_rx.recv() => {
                debouncer.push(path, tokio::time::Instant::now());
            }
            _ = crate::sleep_until_deadline(debouncer.deadline()), if debouncer.deadline().is_some() => {
                let batch = debouncer.flush();
                if batch.len() > max_feedback_batch {
                    page_notice(&mut tui, &format!(
                        "toplu değişiklik ({} dosya) — feedback atlandı, izleme sürüyor",
                        batch.len()
                    ))?;
                    // FileMemory'yi sessizce senkronla: sonraki tekil kayıt dev diff üretmesin.
                    for path in batch {
                        if let Ok(c) = std::fs::read_to_string(&path) {
                            let _ = files.observe(&path, c);
                        }
                    }
                } else {
                    for path in batch {
                        match crate::handle_file_change(backend, session, &mut files, project_root, &path, recorder).await {
                            Ok(crate::FileFeedback::Sessiz) => {}
                            Ok(crate::FileFeedback::Bildirim(m)) => page_notice(&mut tui, &m)?,
                            Ok(crate::FileFeedback::Yanit { tokens, reply }) => {
                                if let Some(t) = tokens { last_tokens = Some(t); }
                                page_reply(&mut tui, &reply.text, width)?;
                                crate::maybe_compact(backend, session, project_root, tokens).await;
                            }
                            Err(e) => page_notice(&mut tui, &format!("dosya feedback atlandı: {}: {e}", path.display()))?,
                        }
                    }
                }
            }
        }
    }
    // Çıkıştan hemen önce son iterasyonun bildirimlerini boşalt — /quit veya
    // Exit yolunda buffer'a düşen bir transcript uyarısı TUI hâlâ ayaktayken görünsün.
    for m in ui::drain_tui_notices() {
        page_notice(&mut tui, &m)?;
    }
    Ok(()) // Tui drop → restore
}

/// `$HOME` → `~` kısaltmalı proje dizini.
fn short_dir(p: &Path) -> String {
    let s = p.display().to_string();
    match dirs::home_dir() {
        Some(h) => s.replace(&h.display().to_string(), "~"),
        None => s,
    }
}
