//! Arka plan dosya izleyici (`notify` v6). Kaydedilen dosyaları proaktif
//! Socratic feedback için REPL'e iletir.
//!
//! `notify` senkron çalışır — async'e zorlamayız. Arka plan thread'i + std mpsc
//! doğru desen: watcher thread'de canlı tutulur, değişen yollar kanaldan akar.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use anyhow::{Context, Result};
use notify::{EventKind, RecursiveMode, Watcher};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

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

/// Yol listesindeki tekrarları temizle — ilk görülme sırasını koru.
/// Saf fonksiyon: tek drain'de aynı dosyayı iki kez işlemeyi önler.
pub fn dedup_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut out: Vec<PathBuf> = Vec::new();
    for p in paths {
        if seen.insert(p.clone()) {
            out.push(p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_removes_consecutive_and_repeated() {
        let input = vec![
            PathBuf::from("a.rs"),
            PathBuf::from("a.rs"), // ardışık tekrar
            PathBuf::from("b.rs"),
            PathBuf::from("a.rs"), // sonradan tekrar
            PathBuf::from("c.rs"),
        ];
        let out = dedup_paths(input);
        assert_eq!(
            out,
            vec![
                PathBuf::from("a.rs"),
                PathBuf::from("b.rs"),
                PathBuf::from("c.rs"),
            ]
        );
    }

    #[test]
    fn dedup_empty_stays_empty() {
        assert!(dedup_paths(vec![]).is_empty());
    }

    #[test]
    fn dedup_preserves_order_of_first_seen() {
        let input = vec![
            PathBuf::from("z"),
            PathBuf::from("y"),
            PathBuf::from("z"),
        ];
        assert_eq!(dedup_paths(input), vec![PathBuf::from("z"), PathBuf::from("y")]);
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
