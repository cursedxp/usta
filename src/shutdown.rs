//! Terminal close / kill -> the normal save path (review P2-9). `listen()` runs
//! once a session exists; from then on `requested()` resolves as soon as
//! SIGTERM or SIGHUP (Unix) / a console close or shutdown event (Windows) has
//! arrived, immediately if it already did. SIGINT is untouched: in the TUI
//! Ctrl+C is a key.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use tokio::sync::watch;

static STOP: OnceLock<watch::Receiver<bool>> = OnceLock::new();
static SEEN: AtomicBool = AtomicBool::new(false);
static TERM_SEEN: AtomicBool = AtomicBool::new(false);

/// Which signal fired. Only a repeated SIGTERM escalates: closing a terminal
/// window delivers SIGHUP twice a few ms apart (shell resend + kernel), and
/// that ordinary close must still reach the save.
#[derive(Clone, Copy, PartialEq, Debug)]
#[cfg_attr(windows, allow(dead_code))]
enum Kind {
    Term,
    Other,
}

fn escalates(kind: Kind, seen: &AtomicBool) -> bool {
    kind == Kind::Term && seen.swap(true, Ordering::SeqCst)
}

/// A loop that failed after a shutdown signal (dead terminal, killed CLI child)
/// drops its error so the closing save still runs; otherwise it propagates.
pub(crate) fn keep_going_to_save(res: anyhow::Result<()>, shutdown: bool) -> anyhow::Result<()> {
    match res {
        Err(e) if shutdown => {
            crate::ui::warn(&format!("session ended by signal: {e}"));
            Ok(())
        }
        other => other,
    }
}

/// Non-blocking read of the flag: true once a shutdown signal was seen, false
/// before `listen()`. Loops check it before any terminal write — after a
/// SIGHUP the terminal is gone and every write fails.
pub(crate) fn is_requested() -> bool {
    SEEN.load(Ordering::SeqCst)
}

/// Register the handlers (once per process) and flip the flag when one fires.
/// If registration fails the flag never flips; the session runs as before.
pub(crate) fn listen() {
    STOP.get_or_init(|| {
        let (tx, rx) = watch::channel(false);
        let on_signal = move |kind: Kind| {
            SEEN.store(true, Ordering::SeqCst);
            if escalates(kind, &TERM_SEEN) {
                // Second SIGTERM while the first is still being handled (e.g.
                // the long closing save): stop now. exit runs no destructors
                // (raw mode stays on); the lock may be left behind, a dead pid
                // no longer blocks recovery (P2-8). Repeated SIGHUP / Windows
                // close events never escalate.
                std::process::exit(130);
            }
            let _ = tx.send(true);
        };
        if let Ok(wait) = signals(on_signal) {
            tokio::spawn(wait);
        }
        rx
    });
}

/// Resolves once a shutdown signal was seen; pends forever before `listen()`
/// or when registration failed. Re-created on every `select!` turn on purpose:
/// the flag is state, so a signal that arrived meanwhile is not lost.
pub(crate) async fn requested() {
    match STOP.get() {
        Some(rx) => wait_flag(rx.clone()).await,
        None => std::future::pending::<()>().await,
    }
}

async fn wait_flag(mut rx: watch::Receiver<bool>) {
    if rx.wait_for(|stop| *stop).await.is_err() {
        std::future::pending::<()>().await
    }
}

#[cfg(unix)]
fn signals(
    mut on_signal: impl FnMut(Kind) + Send + 'static,
) -> std::io::Result<impl Future<Output = ()>> {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = signal(SignalKind::terminate())?;
    let mut hup = signal(SignalKind::hangup())?;
    Ok(async move {
        loop {
            tokio::select! {
                _ = term.recv() => on_signal(Kind::Term),
                _ = hup.recv() => on_signal(Kind::Other),
            }
        }
    })
}

#[cfg(windows)]
fn signals(
    mut on_signal: impl FnMut(Kind) + Send + 'static,
) -> std::io::Result<impl Future<Output = ()>> {
    use tokio::signal::windows::{ctrl_close, ctrl_shutdown};
    let mut close = ctrl_close()?;
    let mut shutdown = ctrl_shutdown()?;
    Ok(async move {
        loop {
            tokio::select! {
                _ = close.recv() => on_signal(Kind::Other),
                _ = shutdown.recv() => on_signal(Kind::Other),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Handler registered first, then SIGHUP to ourselves: the callback runs.
    /// (Once tokio owns SIGHUP the default "terminate" action is gone for the
    /// whole test process, so the signal can't kill the test run.)
    /// Note: this leaves SIGTERM/SIGHUP handlers installed for the test process.
    #[cfg(unix)]
    #[tokio::test]
    async fn sighup_to_self_runs_the_callback() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let wait = signals(move |_| {
            let _ = tx.send(());
        })
        .expect("handlers register");
        tokio::spawn(wait);
        let sent = std::process::Command::new("kill")
            .args(["-HUP", &std::process::id().to_string()])
            .status()
            .unwrap();
        assert!(sent.success());
        tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("SIGHUP reaches the callback");
    }

    #[test]
    fn is_requested_is_false_before_listen() {
        assert!(!is_requested());
    }

    /// The flag is state: a signal that arrived before the wait began still
    /// resolves it at once.
    #[tokio::test]
    async fn wait_flag_resolves_immediately_once_set() {
        let (tx, rx) = watch::channel(false);
        tx.send(true).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), wait_flag(rx))
            .await
            .expect("resolves");
    }

    #[test]
    fn only_a_repeated_sigterm_escalates() {
        let t = AtomicBool::new(false);
        assert!(!escalates(Kind::Other, &t), "hangup first");
        assert!(!escalates(Kind::Term, &t), "first term after hangup");
        assert!(escalates(Kind::Term, &t), "second term");
        let t = AtomicBool::new(false);
        assert!(!escalates(Kind::Term, &t));
        assert!(!escalates(Kind::Other, &t), "hangup never escalates");
        assert!(escalates(Kind::Term, &t));
    }

    #[test]
    fn errors_are_dropped_only_when_shutdown_was_requested() {
        assert!(keep_going_to_save(Err(anyhow::anyhow!("x")), true).is_ok());
        assert!(keep_going_to_save(Err(anyhow::anyhow!("x")), false).is_err());
        assert!(keep_going_to_save(Ok(()), false).is_ok());
    }

    /// Source pin: a repeated SIGTERM stops the process.
    #[test]
    fn second_signal_exits() {
        let prod = include_str!("shutdown.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(prod.contains("std::process::exit(130)"));
    }

    /// Source pin: every session select! loop watches the flag, and listening
    /// starts once the session exists.
    #[test]
    fn session_loops_watch_for_shutdown() {
        for (name, src) in [
            ("tui/run.rs", include_str!("tui/run.rs")),
            ("tui/ask.rs", include_str!("tui/ask.rs")),
            ("plain.rs", include_str!("plain.rs")),
        ] {
            let prod = src.split("#[cfg(test)]").next().unwrap();
            assert_eq!(
                prod.matches("crate::shutdown::requested()").count(),
                1,
                "{name}"
            );
        }
        assert!(include_str!("tui/run.rs").contains("crate::shutdown::listen();"));
        assert!(include_str!("plain.rs").contains("crate::shutdown::listen();"));
    }
}
