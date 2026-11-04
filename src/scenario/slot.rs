//! The session's identity: pauses runs this process owns when the session ends.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::scenario::command::{execute, Ctx, Reply, ScenarioCmd};
use crate::scenario::container::ContainerEnv;
use crate::scenario::docker::RealDocker;
use crate::scenario::engine::{self, Tick};
use crate::scenario::store::{project_tag, RunState, Store};

/// Holds the session's identity; pauses runs this process owns when dropped
/// (normal close, `?` error, unwind). A hard kill is handled by the startup sweep.
pub(crate) struct Slot {
    ctx: Ctx,
    /// True while a command's blocking work runs. A learner who stopped waiting
    /// leaves that work finishing on its own; a second command must not race it.
    busy: Arc<AtomicBool>,
}

/// Clears the busy flag when the blocking work ends, panic included.
struct BusyGuard(Arc<AtomicBool>);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl Slot {
    pub fn new(global: &Path, project_root: &Path, topic: &str) -> Slot {
        Slot {
            ctx: Ctx {
                global: global.to_path_buf(),
                project_root: project_root.to_path_buf(),
                topic: topic.to_string(),
                pid: std::process::id(),
            },
            busy: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Plain loop: wait for the work, but give up on a shutdown signal.
    pub async fn handle_until_shutdown(&self, cmd: ScenarioCmd) -> Option<Reply> {
        tokio::select! {
            _ = crate::shutdown::requested() => None,
            r = self.handle(cmd) => Some(r),
        }
    }

    pub async fn handle(&self, cmd: ScenarioCmd) -> Reply {
        if self.busy.swap(true, Ordering::SeqCst) {
            return Reply::note("the previous step is still finishing; try again in a moment");
        }
        let ctx = self.ctx.clone();
        let guard = BusyGuard(self.busy.clone());
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            execute(&ctx, cmd)
        })
        .await
        .unwrap_or_else(|e| Reply::note(format!("scenario: internal error: {e}")))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let store = Store::new(&self.ctx.project_root);
        if !store.exists() {
            return;
        }
        let docker = RealDocker;
        let mut env = ContainerEnv {
            docker: &docker,
            project_tag: project_tag(&self.ctx.project_root),
        };
        let today = crate::lifecycle::today();
        let alive: &dyn Fn(u32) -> bool = &crate::lock::pid_alive;
        let tick = Tick {
            pid: self.ctx.pid,
            alive,
            stamp: "",
            today: &today,
        };
        for mut rec in store.all() {
            if rec.state == RunState::Active && rec.owner_pid == self.ctx.pid {
                let _ = engine::pause(&mut env, &store, &mut rec, &tick);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot() -> Slot {
        Slot::new(
            Path::new("/nonexistent-g"),
            Path::new("/nonexistent-p"),
            "linux",
        )
    }

    #[tokio::test]
    async fn a_second_command_while_one_is_finishing_is_turned_away() {
        let s = slot();
        s.busy.store(true, Ordering::SeqCst);
        let r = s.handle(ScenarioCmd::Usage).await;
        assert!(
            r.notices.join(" ").contains("still finishing"),
            "{:?}",
            r.notices
        );
        assert!(
            s.busy.load(Ordering::SeqCst),
            "the running step's flag stays set"
        );
    }

    #[tokio::test]
    async fn the_flag_clears_after_a_command_finishes() {
        let s = slot();
        let r = s.handle(ScenarioCmd::Usage).await;
        assert!(!r.notices.join(" ").contains("still finishing"));
        assert!(!s.busy.load(Ordering::SeqCst));
    }
}
