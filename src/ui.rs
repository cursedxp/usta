//! Sunum katmanı: kim konuşuyor tek bakışta belli olsun. ● (turuncu) = Usta,
//! ■ = kullanıcı promptu, soluk `·` = sistem bildirimi. Usta yanıtları
//! termimad ile gerçek markdown olarak çizilir. `is_plain` kapısı: TTY yoksa
//! veya NO_COLOR set'liyse ANSI'siz düz çıktı — pipe/test bozulmaz.
//! Davranış burada yaşamaz — sadece görünüm.

use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use termimad::MadSkin;

pub const ORANGE: &str = "\x1b[38;5;208m";
pub const DIM: &str = "\x1b[2m";
pub const YELLOW: &str = "\x1b[33m";
pub const RESET: &str = "\x1b[0m";

/// TUI aktif mi? true iken notice/warn/Spinner ham ANSI basmaz — ratatui
/// inline viewport'u ile çakışmasınlar diye tamponlanır/no-op olur.
static TUI_ACTIVE: AtomicBool = AtomicBool::new(false);

/// TUI aktifken biriken bildirimler — run.rs bunları page_notice ile
/// scrollback'e boşaltır.
static TUI_NOTICES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// TUI döngüsüne girerken/çıkarken bayrağı ayarla (main.rs sorumlu).
pub fn set_tui_active(on: bool) {
    TUI_ACTIVE.store(on, Ordering::SeqCst);
}

fn tui_active() -> bool {
    TUI_ACTIVE.load(Ordering::SeqCst)
}

/// Biriken TUI bildirimlerini al ve tamponu boşalt.
pub fn drain_tui_notices() -> Vec<String> {
    match TUI_NOTICES.lock() {
        Ok(mut guard) => std::mem::take(&mut *guard),
        // Poison olsa bile temizlik yolunda panic yok — kilidi kurtar.
        Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
    }
}

fn push_tui_notice(msg: String) {
    match TUI_NOTICES.lock() {
        Ok(mut guard) => guard.push(msg),
        Err(poisoned) => poisoned.into_inner().push(msg),
    }
}

/// Düz mod: stdout TTY değil veya kullanıcı NO_COLOR istemiş.
pub fn is_plain() -> bool {
    !std::io::stdout().is_terminal() || std::env::var_os("NO_COLOR").is_some()
}

/// Usta yanıt bloğu: boş satır + turuncu ● + markdown render + boş satır.
pub fn print_usta_reply(reply: &str, web: bool) {
    if is_plain() {
        println!("Usta> {reply}");
        if web {
            println!("(🔎 web researched)");
        }
        return;
    }
    println!("\n{ORANGE}●{RESET}");
    let width = termimad::terminal_size().0.max(40) as usize;
    let skin = skin();
    let text = skin.text(reply, Some(width.saturating_sub(4)));
    // termimad başlık/bold ile başlayınca öne-sona boş satır ekliyor —
    // `●`'dan sonra gap olmasın diye baş/son boş satırları at (iç boşluklar
    // paragraf ayracı olarak kalır).
    let block = format!("{text}");
    let lines: Vec<&str> = block.lines().collect();
    let start = lines.iter().position(|l| !l.trim().is_empty()).unwrap_or(0);
    let end = lines
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map(|i| i + 1)
        .unwrap_or(0);
    for line in &lines[start..end] {
        println!("  {line}");
    }
    if web {
        println!("{DIM}  🔎 web researched{RESET}");
    }
    println!();
}

/// Soluk bilgi satırı (stdout) — ana akıştan görsel olarak ayrılır.
pub fn notice(msg: &str) {
    if tui_active() {
        // TUI canlı viewport'u basıyor — ham ANSI çakışmasın, biriktir.
        push_tui_notice(msg.to_string());
        return;
    }
    if is_plain() {
        println!("({msg})");
    } else {
        println!("{DIM}· {msg}{RESET}");
    }
}

/// Soluk uyarı satırı (stderr).
pub fn warn(msg: &str) {
    if tui_active() {
        push_tui_notice(format!("⚠ {msg}"));
        return;
    }
    if is_plain() {
        eprintln!("({msg})");
    } else {
        eprintln!("{DIM}! {msg}{RESET}");
    }
}

/// Oturum açılış satırı — konu + model + çıkış ipucu.
pub fn banner(topic: &str, model: &str) {
    if is_plain() {
        println!("Usta ready — topic: {topic} · model: {model}. (/quit to exit)");
        return;
    }
    println!("{ORANGE}● Usta{RESET} {DIM}— topic: {topic} · model: {model} · /quit to exit{RESET}");
}

/// Bağlam doluluk göstergesi — 8 hücreli bar, ≥%70 sarı uyarı.
/// Token bilgisi yoksa veya düz moddaysa hiç çizilmez (gürültü yok).
pub fn context_gauge(tokens: Option<u64>, window: u64) {
    let Some(t) = tokens else { return };
    if is_plain() {
        return;
    }
    let ratio = (t as f64 / window as f64).min(1.0);
    let filled = ((ratio * 8.0).round() as usize).min(8);
    let bar = format!("{}{}", "▓".repeat(filled), "░".repeat(8 - filled));
    let color = if ratio >= 0.7 { YELLOW } else { DIM };
    println!("{color}  {bar} context {}k/{}k{RESET}", t / 1000, window / 1000);
}

/// LLM beklerken tek satır animasyon. Düz modda hiç çizmez.
/// `stop` çağrılınca satır silinir — yanıt temiz zemine basılır.
pub struct Spinner {
    stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl Spinner {
    pub fn start(msg: &'static str) -> Spinner {
        // TUI aktifken de no-op: kendi spinner'ı var (Status::Thinking),
        // burada başlarsa arka plan print! task'ı viewport'u ezer.
        if is_plain() || tui_active() {
            return Spinner { stop_tx: None, handle: None };
        }
        let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            const FRAMES: [&str; 4] = ["⠋", "⠙", "⠸", "⠴"];
            let mut i = 0usize;
            loop {
                print!("\r{DIM}{} {msg}{RESET}", FRAMES[i % FRAMES.len()]);
                let _ = std::io::Write::flush(&mut std::io::stdout());
                i += 1;
                tokio::select! {
                    _ = &mut rx => break,
                    _ = tokio::time::sleep(std::time::Duration::from_millis(120)) => {}
                }
            }
            // Satırı sil — yanıt temiz zemine bassın.
            print!("\r\x1b[2K");
            let _ = std::io::Write::flush(&mut std::io::stdout());
        });
        Spinner { stop_tx: Some(tx), handle: Some(handle) }
    }

    pub async fn stop(mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(h) = self.handle.take() {
            let _ = h.await;
        }
    }
}

/// Usta yanıtını ANSI'li String'e render et — TUI yolu bunu ratatui Text'ine
/// çevirir (tui::convert). Satır başına 2 boşluk girinti print_usta_reply ile
/// aynı görsel dil.
pub fn render_markdown(md: &str, width: usize) -> String {
    let skin = skin();
    let text = skin.text(md, Some(width.saturating_sub(4)));
    format!("{text}")
        .lines()
        .map(|l| format!("  {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Usta yanıtlarının markdown teni: başlık+bold turuncu, inline code yeşil.
/// (termimad API'si sürüme göre değişebilir — hedef renkler Global
/// Constraints'te; eş değer çağrıyı kullan.)
fn skin() -> MadSkin {
    use termimad::crossterm::style::Color;
    let mut skin = MadSkin::default();
    skin.set_headers_fg(Color::AnsiValue(208));
    skin.bold.set_fg(Color::AnsiValue(208));
    skin.inline_code.set_fg(Color::AnsiValue(114));
    skin
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_markdown_indents_and_keeps_content() {
        let out = render_markdown("**kalın** metin", 60);
        assert!(out.contains("kalın"));
        assert!(out.lines().all(|l| l.is_empty() || l.starts_with("  ")));
    }
}
