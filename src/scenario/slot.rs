//! The session's identity: pauses runs this process owns when the session ends.

use std::path::Path;

use crate::scenario::command::{execute, Ctx, Reply, ScenarioCmd};
use crate::scenario::container::ContainerEnv;
use crate::scenario::docker::RealDocker;
use crate::scenario::engine::{self, Tick};
use crate::scenario::store::{project_tag, RunState, Store};

/// Holds the session's identity; pauses runs this process owns when dropped
/// (normal close, `?` error, unwind). A hard kill is handled by the startup sweep.
pub(crate) struct Slot {
    ctx: Ctx,
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
        }
    }

    pub async fn handle(&self, cmd: ScenarioCmd) -> Reply {
        let ctx = self.ctx.clone();
        tokio::task::spawn_blocking(move || execute(&ctx, cmd))
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
