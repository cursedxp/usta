//! Terminal close / kill -> the normal save path (review P2-9). `listen()` runs
//! once a session exists; from then on `requested()` resolves as soon as
//! SIGTERM or SIGHUP (Unix) / a console close or shutdown event (Windows) has
//! arrived, immediately if it already did. SIGINT is untouched: in the TUI
//! Ctrl+C is a key.

use std::future::Future;
use std::sync::OnceLock;

use tokio::sync::watch;

static STOP: OnceLock<watch::Receiver<bool>> = OnceLock::new();

/// Register the handlers (once per process) and flip the flag when one fires.
/// If registration fails the flag never flips; the session runs as before.
pub(crate) fn listen() {
    STOP.get_or_init(|| {
        let (tx, rx) = watch::channel(false);
        if let Ok(wait) = signals() {
            tokio::spawn(async move {
                wait.await;
                let _ = tx.send(true);
            });
        }
        rx
    });
}

/// Resolves once a shutdown signal was seen; pends forever before `listen()`
/// or when registration failed. Re-created on every `select!` turn on purpose:
/// the flag is state, so a signal that arrived meanwhile is not lost.
pub(crate) async fn requested() {
    if let Some(rx) = STOP.get() {
        let mut rx = rx.clone();
        if rx.wait_for(|stop| *stop).await.is_ok() {
            return;
        }
    }
    std::future::pending::<()>().await
}

#[cfg(unix)]
fn signals() -> std::io::Result<impl Future<Output = ()>> {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = signal(SignalKind::terminate())?;
    let mut hup = signal(SignalKind::hangup())?;
    Ok(async move {
        tokio::select! {
            _ = term.recv() => {}
            _ = hup.recv() => {}
        }
    })
}

#[cfg(windows)]
fn signals() -> std::io::Result<impl Future<Output = ()>> {
    use tokio::signal::windows::{ctrl_close, ctrl_shutdown};
    let mut close = ctrl_close()?;
    let mut shutdown = ctrl_shutdown()?;
    Ok(async move {
        tokio::select! {
            _ = close.recv() => {}
            _ = shutdown.recv() => {}
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Handler registered first, then SIGHUP to ourselves: the future resolves.
    /// (Once tokio owns SIGHUP the default "terminate" action is gone for the
    /// whole test process, so the signal can't kill the test run.)
    #[cfg(unix)]
    #[tokio::test]
    async fn sighup_to_self_resolves_the_signal_future() {
        let wait = signals().expect("handlers register");
        let sent = std::process::Command::new("kill")
            .args(["-HUP", &std::process::id().to_string()])
            .status()
            .unwrap();
        assert!(sent.success());
        tokio::time::timeout(std::time::Duration::from_secs(5), wait)
            .await
            .expect("SIGHUP reaches the future");
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
