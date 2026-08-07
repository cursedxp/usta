//! Terminal yaşam döngüsü: inline viewport kur, NE OLURSA OLSUN restore et.
//! Bozuk raw-mode'da bırakılan shell = en kötü kullanıcı deneyimi; Drop +
//! panic hook çifte emniyet.

use std::io::Stdout;

use anyhow::Result;
use ratatui::backend::CrosstermBackend;
use ratatui::{Terminal, TerminalOptions, Viewport};

/// Alt bölge: girdi kutusu (3-5 satır) + durum satırı (1).
pub const VIEWPORT_H: u16 = 6;

pub struct Tui {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
}

/// Raw mode + inline viewport. Panic hook'a restore zincirlenir — önceki
/// hook korunur (test harness'inin hook'u ezilmez).
pub fn setup() -> Result<Tui> {
    crossterm::terminal::enable_raw_mode()?;
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        prev(info);
    }));
    let terminal = Terminal::with_options(
        CrosstermBackend::new(std::io::stdout()),
        TerminalOptions { viewport: Viewport::Inline(VIEWPORT_H) },
    )?;
    Ok(Tui { terminal })
}

/// Raw mode'u kapat — idempotent, hata yutar (kapanış yolunda panik yok).
pub fn restore() {
    let _ = crossterm::terminal::disable_raw_mode();
}

impl Drop for Tui {
    fn drop(&mut self) {
        // Viewport bölgesini temizle ki kapanış mesajları temiz zemine bassın.
        let _ = self.terminal.clear();
        restore();
        println!();
    }
}
