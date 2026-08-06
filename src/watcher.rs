//! Arka plan dosya izleyici (`notify` v6). Kaydedilen dosyaları proaktif
//! Socratic feedback için REPL'e iletir.
//!
//! `notify` senkron çalışır — async'e zorlamayız. Arka plan thread'i + std mpsc
//! doğru desen: watcher thread'de canlı tutulur, değişen yollar kanaldan akar.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use notify::{EventKind, RecursiveMode, Watcher};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
use tokio::time::Instant;

/// `root` altını özyinelemeli izle; modify olaylarındaki dosya yollarını gönder.
/// Dönen alıcı select-loop'ta `recv().await` (veya `try_recv`) ile tüketilir.
pub fn spawn(root: &Path) -> Result<UnboundedReceiver<PathBuf>> {
    let (out_tx, out_rx) = unbounded_channel::<PathBuf>();
    let (ev_tx, ev_rx) = mpsc::channel::<notify::Result<notify::Event>>();

    let mut watcher = notify::recommended_watcher(move |res| {
        // Alıcı düşmüşse sessizce yut — thread yakında kapanır.
        let _ = ev_tx.send(res);
    })
    .context("dosya izleyici oluşturulamadı")?;

    watcher
        .watch(root, RecursiveMode::Recursive)
        .with_context(|| format!("izlenemedi: {}", root.display()))?;

    thread::spawn(move || {
        // Watcher'ı thread içinde canlı tut — düşerse izleme durur.
        let _watcher = watcher;
        for res in ev_rx {
            let Ok(event) = res else { continue };
            if matches!(event.kind, EventKind::Modify(_)) {
                for path in event.paths {
                    if is_ignored(&path) {
                        continue;
                    }
                    // REPL alıcısı kapandıysa thread'i bitir.
                    if out_tx.send(path).is_err() {
                        return;
                    }
                }
            }
        }
    });

    Ok(out_rx)
}

/// Build/VCS/gizli dizin gürültüsünü ele — yol bileşenlerinden biri `target`,
/// `node_modules` ise veya `.` ile başlıyorsa (örn. `.git`, `.venv`) yok say.
/// Dil-agnostik: uzantıya göre filtrelemiyoruz, Usta çok alanlı çalışır.
pub fn is_ignored(path: &Path) -> bool {
    path.components().any(|c| match c {
        std::path::Component::Normal(s) => {
            let s = s.to_string_lossy();
            s == "target" || s == "node_modules" || s.starts_with('.')
        }
        _ => false,
    })
}

/// Kayıt fırtınasını yatıştıran saf debounce durumu. Editörler tek kayıtta
/// birden çok modify olayı üretir; her olay `push`lanır, son olaydan `window`
/// sonra `deadline` dolar ve select-loop `flush` ile hepsini tek seferde işler.
/// Henüz `main.rs`'e bağlanmadı (Task 3) — şimdilik sadece testlerden kullanılıyor.
#[allow(dead_code)]
pub struct Debouncer {
    pending: Vec<PathBuf>,
    deadline: Option<Instant>,
    window: Duration,
}

#[allow(dead_code)]
impl Debouncer {
    pub fn new(window: Duration) -> Self {
        Debouncer { pending: Vec::new(), deadline: None, window }
    }

    /// Yolu biriktir (tekrarları ilk-görülme sırasını koruyarak ele) ve
    /// deadline'ı ileri at.
    pub fn push(&mut self, path: PathBuf, now: Instant) {
        if !self.pending.contains(&path) {
            self.pending.push(path);
        }
        self.deadline = Some(now + self.window);
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Birikeni boşalt, deadline'ı sıfırla.
    pub fn flush(&mut self) -> Vec<PathBuf> {
        self.deadline = None;
        std::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::Instant;

    #[test]
    fn debouncer_push_dedups_and_preserves_order() {
        let mut d = Debouncer::new(Duration::from_millis(1000));
        let now = Instant::now();
        d.push(PathBuf::from("a.rs"), now);
        d.push(PathBuf::from("b.rs"), now);
        d.push(PathBuf::from("a.rs"), now);
        assert_eq!(d.flush(), vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")]);
    }

    #[test]
    fn debouncer_push_extends_deadline() {
        let mut d = Debouncer::new(Duration::from_millis(1000));
        let t0 = Instant::now();
        d.push(PathBuf::from("a.rs"), t0);
        let t1 = t0 + Duration::from_millis(500);
        d.push(PathBuf::from("b.rs"), t1);
        assert_eq!(d.deadline(), Some(t1 + Duration::from_millis(1000)));
    }

    #[test]
    fn debouncer_flush_clears_pending_and_deadline() {
        let mut d = Debouncer::new(Duration::from_millis(1000));
        d.push(PathBuf::from("a.rs"), Instant::now());
        let _ = d.flush();
        assert!(d.deadline().is_none());
        assert!(d.flush().is_empty());
    }

    #[test]
    fn debouncer_empty_has_no_deadline() {
        let d = Debouncer::new(Duration::from_millis(1000));
        assert!(d.deadline().is_none());
    }

    #[test]
    fn is_ignored_flags_target_dir() {
        assert!(is_ignored(Path::new("target/debug/x.rs")));
    }

    #[test]
    fn is_ignored_flags_hidden_dir() {
        assert!(is_ignored(Path::new(".git/HEAD")));
    }

    #[test]
    fn is_ignored_flags_node_modules() {
        assert!(is_ignored(Path::new("node_modules/foo/index.js")));
    }

    #[test]
    fn is_ignored_allows_src_file() {
        assert!(!is_ignored(Path::new("src/main.rs")));
    }

    #[test]
    fn is_ignored_allows_arbitrary_extension() {
        assert!(!is_ignored(Path::new("foo.py")));
    }
}
