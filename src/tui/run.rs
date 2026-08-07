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

/// Anlık terminal genişliği — resize sonrası sarma doğru kalsın (spec B3).
/// Ölçüm başarısızsa 80'e düş (sarma bozulmaz, sadece dar olur).
fn current_width(tui: &Tui) -> u16 {
    tui.terminal.size().map(|s| s.width).unwrap_or(80)
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

/// ask_live sonucu: yanıt geldi ya da kullanıcı çift Ctrl-C ile iptal etti.
pub enum AskOutcome {
    Reply(crate::backend::Reply),
    Cancelled,
}

/// Kilitli moddaki tuşun anlamı — saf, testlenebilir (spec B2).
enum LockedKey {
    /// Editöre işlenecek tuş (Enter dahil — Enter yutulur ama edit sayılır).
    Edit,
    /// Ctrl-C / Ctrl-D — iptal isteği basamağı.
    CancelRequest,
}

fn classify_locked_key(k: KeyEvent) -> LockedKey {
    use crossterm::event::KeyModifiers;
    if k.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(k.code, KeyCode::Char('c') | KeyCode::Char('d'))
    {
        LockedKey::CancelRequest
    } else {
        LockedKey::Edit
    }
}

/// LLM çağrısını canlı arayüzle bekle: spinner döner, tuşlar editöre işler
/// ama Submit/Exit KİLİTLİ (tek turn ilkesi) — Enter yutulur. Çift Ctrl-C
/// (veya Ctrl-D) ile iptal edilebilir: ilk basış durum satırında ipucu
/// yakar, ikincisi future'ı düşürür (kill_on_drop çocuğu öldürür).
async fn ask_live(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
    backend: &mut Backend,
    system: &str,
    history: &[Message],
    tokens: Option<u64>,
) -> Result<AskOutcome> {
    let window = backend.context_window();
    let fut = backend.complete(system, history);
    tokio::pin!(fut);
    let mut frame = 0usize;
    let mut cancel_armed = false; // ilk Ctrl-C sonrası true — sayaç sıfırlanmaz (spec B2)
    loop {
        draw(tui, editor, &Status::Thinking { frame, cancel_hint: cancel_armed }, tokens, window)?;
        tokio::select! {
            r = &mut fut => return Ok(AskOutcome::Reply(r?)),
            Some(Ok(ev)) = events.next() => {
                if let Event::Key(k) = ev {
                    match classify_locked_key(k) {
                        LockedKey::CancelRequest if cancel_armed => {
                            // fut düşer → kill_on_drop çocuğu öldürür (backend.rs).
                            return Ok(AskOutcome::Cancelled);
                        }
                        LockedKey::CancelRequest => { cancel_armed = true; }
                        LockedKey::Edit => {
                            if !matches!(k.code, KeyCode::Enter) {
                                let _ = match editor.handle_key(k) {
                                    Action::Exit => Action::None, // buraya düşmez (CancelRequest yakalar) — emniyet
                                    other => other,
                                };
                            }
                        }
                    }
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(120)) => { frame += 1; }
        }
    }
}

/// Kimlik welcome'ı basıp konuyu girdi kutusundan okur. `None` = kullanıcı
/// konu vermeden çıktı (Ctrl-C/D). Slug çözümü çağırana bırakılır. Konu
/// girişinde watcher olayları burada TÜKETİLMEZ — sadece tuş dinlenir; kanalda
/// biriken olaylar oturum kurulduktan sonra sessizce sindirilir (bkz. `run`).
#[allow(clippy::too_many_arguments)]
async fn ask_topic(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
    global: &Path,
    profile: Option<&str>,
    model: &str,
    dir: &str,
) -> Result<Option<String>> {
    // Kayıtlı konular (global katalog) — boşsa ilk-oturum mesajı.
    let idx = std::fs::read_to_string(global.join("learner/index.md")).unwrap_or_default();
    let topics: Vec<String> = {
        let mut t: Vec<String> = crate::index::entries(&idx).into_iter().map(|e| e.topic).collect();
        t.dedup();
        t.truncate(6);
        t
    };
    let name = profile.and_then(welcome::extract_name);
    let width = current_width(tui);
    page(tui, welcome::render_welcome_identity(name.as_deref(), model, dir, &topics, width))?;
    page_notice(tui, "Ne öğrenmek istiyorsun? (kısa yaz ya da cümleyle anlat)")?;

    loop {
        draw(tui, editor, &Status::Idle, None, 0)?;
        if let Some(Ok(Event::Key(k))) = events.next().await {
            match editor.handle_key(k) {
                Action::Submit(line) => return Ok(Some(line)),
                Action::Exit => return Ok(None),
                Action::None => {}
            }
        }
    }
}

/// TUI'de tek-tuş onay: mesajı bas, bir tuş bekle. `e`/`E` → true, diğer → false.
async fn tui_confirm(
    tui: &mut Tui,
    editor: &InputBox,
    events: &mut EventStream,
    msg: &str,
) -> Result<bool> {
    page_notice(tui, msg)?;
    loop {
        draw(tui, editor, &Status::Idle, None, 0)?;
        if let Some(Ok(Event::Key(k))) = events.next().await {
            match k.code {
                KeyCode::Char('e') | KeyCode::Char('E') => return Ok(true),
                _ => return Ok(false),
            }
        }
    }
}

/// TUI oturumu: konu girişi (arg yoksa) + açılış kutusu + drill/tanışma + ana
/// döngü. Session/recorder içeride `build_session` ile kurulur. Dönüş:
/// `Some((session, recorder, lock))` — kapanış main'de plain yolla paylaşımlı;
/// `None` — kullanıcı konu vermeden çıktı (oturum yok, kilit yok). Dönüşte Tui
/// drop olur → terminal restore.
pub async fn run(
    backend: &mut Backend,
    global: &Path,
    project_root: &Path,
    today: &str,
    topic_arg: Option<String>,
    max_feedback_batch: usize,
    watch_rx: &mut UnboundedReceiver<PathBuf>,
) -> Result<Option<(Session, Recorder, PathBuf)>> {
    // setup() panic hook'u kurar → SÜREÇ BAŞINA TAM BİR KEZ çağrılır (döngüde değil).
    let mut tui = crate::tui::term::setup()?;
    let mut editor = InputBox::new();
    let mut events = EventStream::new();
    let read = |p: PathBuf| std::fs::read_to_string(p).ok();

    // Konu belirle: argüman verildiyse yerel slug'la (`usta start "JavaScript
    // Basics"` çalışsın). Argüman yoksa kimlik welcome + girdi kutusundan sor.
    let had_topic_arg = topic_arg.is_some();
    let topic = match topic_arg {
        Some(t) => crate::slugify_topic(&t),
        None => {
            let raw = match ask_topic(
                &mut tui,
                &mut editor,
                &mut events,
                global,
                read(global.join("learner/profile.md")).as_deref(),
                &backend.label(),
                &short_dir(project_root),
            )
            .await?
            {
                Some(line) => line,
                None => return Ok(None), // konu vermeden çıktı
            };
            // Kısa girdi → yerel slug; cümle → LLM slug (spinner), hata → yerel.
            if raw.split_whitespace().count() <= 2 {
                crate::slugify_topic(&raw)
            } else {
                let slug = match ask_live(
                    &mut tui,
                    &mut editor,
                    &mut events,
                    backend,
                    crate::SLUG_SYSTEM,
                    &[Message::user(raw.as_str())],
                    None,
                )
                .await
                {
                    Ok(AskOutcome::Reply(reply)) => crate::finalize_slug(&raw, &reply.text),
                    Ok(AskOutcome::Cancelled) | Err(_) => crate::slugify_topic(&raw),
                };
                // Slug mini-oturumu öğrenme oturumuna taşınmasın (spec B1).
                backend.reset_session();
                slug
            }
        }
    };
    if !had_topic_arg {
        page_notice(&mut tui, &format!("konu: {topic} — detayı sohbette anlatırsın"))?;
    }

    // Lock-çakışma onayı (TUI tek-tuş) — build_session'dan ÖNCE, kendi kilidini
    // yazmadan. Reddedilirse session/kilit yok → Tui drop restore.
    let lock = crate::lock_path(project_root, &topic);
    if lock.exists()
        && !tui_confirm(
            &mut tui,
            &editor,
            &mut events,
            "Bu konuda başka oturum açık olabilir — progress çakışabilir. Devam? [e/H]",
        )
        .await?
    {
        page_notice(&mut tui, "vazgeçildi")?;
        return Ok(None);
    }

    // build_session kendi kilidini yazar; dönen lock = aynı yol.
    let (mut session, recorder, lock, has_progress) =
        crate::build_session(global, project_root, &topic, today)?;

    let mut debouncer = watcher::Debouncer::new(std::time::Duration::from_millis(1000));
    let mut files = feedback::FileMemory::new();
    let mut last_tokens: Option<u64> = None;
    let window = backend.context_window();

    // Konu girişi sırasında biriken watcher olaylarını sessizce sindir — kullanıcı
    // konuyu yazarken kaydedilen dosyalar oturum başlar başlamaz sürpriz feedback
    // üretmesin (FileMemory senkronlanır, sonraki gerçek değişiklik ona göre diff'lenir).
    while let Ok(path) = watch_rx.try_recv() {
        if let Ok(c) = std::fs::read_to_string(&path) {
            let _ = files.observe(&path, c);
        }
    }

    // Welcome: konu baştan belliyse (arg verilmişti) tam-mod öğrenme durumu
    // basılır. Aksi halde kimlik welcome ask_topic içinde zaten basıldı — tek welcome.
    if had_topic_arg {
        let data = welcome::gather(
            read(global.join("learner/profile.md")).as_deref(),
            read(progress::progress_path(project_root, &topic)).as_deref(),
            read(progress::curriculum_path(project_root, &topic)).as_deref(),
            &topic,
            &backend.label(),
            &short_dir(project_root),
        );
        let w = current_width(&tui);
        page(&mut tui, welcome::render_welcome(&data, w))?;
    }

    // Açılış drilli / tanışma (main.rs plain yolunun TUI karşılığı).
    let opening = if has_progress {
        progress::opening_prompt(&topic)
    } else {
        progress::onboarding_prompt(&topic)
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
        Ok(AskOutcome::Reply(reply)) => {
            last_tokens = reply.context_tokens;
            let w = current_width(&tui);
            page_reply(&mut tui, &reply.text, w)?;
            recorder.assistant(&reply.text);
            session.push_assistant(reply.text);
        }
        Ok(AskOutcome::Cancelled) => {
            backend.reset_session();
            page_notice(&mut tui, "açılış turu iptal edildi")?;
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
                            Ok(AskOutcome::Reply(reply)) => {
                                last_tokens = reply.context_tokens;
                                let w = current_width(&tui);
                                page_reply(&mut tui, &reply.text, w)?;
                                recorder.assistant(&reply.text);
                                session.push_assistant(reply.text);
                                crate::maybe_compact(backend, &mut session, project_root, last_tokens).await;
                            }
                            Ok(AskOutcome::Cancelled) => {
                                // User turn history'de kalır (bilinçli — spec B2); CLI oturumu
                                // yarım — resume edilmesin, sonraki çağrı tam transcript'le gitsin.
                                backend.reset_session();
                                page_notice(&mut tui, "yanıt iptal edildi — mesajın kaldı, istersen devam et")?;
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
                        match crate::handle_file_change(backend, &mut session, &mut files, project_root, &path, &recorder).await {
                            Ok(crate::FileFeedback::Sessiz) => {}
                            Ok(crate::FileFeedback::Bildirim(m)) => page_notice(&mut tui, &m)?,
                            Ok(crate::FileFeedback::Yanit { tokens, reply }) => {
                                if let Some(t) = tokens { last_tokens = Some(t); }
                                let w = current_width(&tui);
                                page_reply(&mut tui, &reply.text, w)?;
                                crate::maybe_compact(backend, &mut session, project_root, tokens).await;
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
    Ok(Some((session, recorder, lock))) // Tui drop → restore
}

/// `$HOME` → `~` kısaltmalı proje dizini.
fn short_dir(p: &Path) -> String {
    let s = p.display().to_string();
    match dirs::home_dir() {
        Some(h) => s.replace(&h.display().to_string(), "~"),
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn classify_locked_key_ctrl_c_and_d_are_cancel_requests() {
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert!(matches!(classify_locked_key(ctrl_c), LockedKey::CancelRequest));
        assert!(matches!(classify_locked_key(ctrl_d), LockedKey::CancelRequest));
    }

    #[test]
    fn classify_locked_key_enter_and_chars_are_edits() {
        assert!(matches!(
            classify_locked_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            LockedKey::Edit
        ));
        assert!(matches!(
            classify_locked_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
            LockedKey::Edit
        ));
    }
}
