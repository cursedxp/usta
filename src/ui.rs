//! Sunum katmanı: kim konuşuyor tek bakışta belli olsun. ● (turuncu) = Usta,
//! ■ = kullanıcı promptu, soluk `·` = sistem bildirimi. Usta yanıtları
//! termimad ile gerçek markdown olarak çizilir. `is_plain` kapısı: TTY yoksa
//! veya NO_COLOR set'liyse ANSI'siz düz çıktı — pipe/test bozulmaz.
//! Davranış burada yaşamaz — sadece görünüm.

use std::io::IsTerminal;

use termimad::MadSkin;

pub const ORANGE: &str = "\x1b[38;5;208m";
pub const DIM: &str = "\x1b[2m";
pub const RESET: &str = "\x1b[0m";

/// Düz mod: stdout TTY değil veya kullanıcı NO_COLOR istemiş.
pub fn is_plain() -> bool {
    !std::io::stdout().is_terminal() || std::env::var_os("NO_COLOR").is_some()
}

/// Usta yanıt bloğu: boş satır + turuncu ● + markdown render + boş satır.
pub fn print_usta_reply(reply: &str, web: bool) {
    if is_plain() {
        println!("Usta> {reply}");
        if web {
            println!("(🔎 web araştırıldı)");
        }
        return;
    }
    println!("\n{ORANGE}●{RESET}");
    skin().print_text(reply);
    if web {
        println!("{DIM}  🔎 web araştırıldı{RESET}");
    }
    println!();
}

/// Soluk bilgi satırı (stdout) — ana akıştan görsel olarak ayrılır.
pub fn notice(msg: &str) {
    if is_plain() {
        println!("({msg})");
    } else {
        println!("{DIM}· {msg}{RESET}");
    }
}

/// Soluk uyarı satırı (stderr).
pub fn warn(msg: &str) {
    if is_plain() {
        eprintln!("({msg})");
    } else {
        eprintln!("{DIM}! {msg}{RESET}");
    }
}

/// Oturum açılış satırı.
pub fn banner(topic: &str) {
    if is_plain() {
        println!("Usta hazır — konu: {topic}. Kod yaz, kaydet; ben izlerim. (/quit ile çık)");
        return;
    }
    println!("{ORANGE}● Usta{RESET} {DIM}— konu: {topic} · kod yaz, kaydet; izliyorum · /quit ile çık{RESET}");
}

/// LLM beklerken tek satır animasyon. Düz modda hiç çizmez.
/// `stop` çağrılınca satır silinir — yanıt temiz zemine basılır.
pub struct Spinner {
    stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl Spinner {
    pub fn start(msg: &'static str) -> Spinner {
        if is_plain() {
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
