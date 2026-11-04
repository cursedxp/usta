//! The environment contract. The scenario engine never knows which environment
//! it runs in; a new environment (browser, VM, account) is a new implementation
//! of these five operations and the engine does not change.

use anyhow::Result;

use crate::scenario::package::Package;

pub(crate) const CONTAINER: &str = "container";
pub(crate) const FOLDER: &str = "folder";
pub(crate) const CONVERSATION: &str = "conversation";

/// What `prepare` returns and every later operation receives. Opaque to the engine.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EnvHandle {
    /// Environment-specific reference (container name, folder path, ...).
    pub reference: String,
    /// Command line that opens a shell inside the task. The terminal launcher
    /// runs it; it is shown to the learner only if no window can be opened.
    pub access: String,
}

/// An error whose text is meant for the log, not the learner (tool output,
/// exit codes). The command layer logs it and shows a plain sentence instead.
#[derive(Debug)]
pub(crate) struct Internal(pub String);

impl std::fmt::Display for Internal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Internal {}

pub(crate) fn internal(msg: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(Internal(msg.into()))
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CheckOutcome {
    Pass,
    /// `output` is shown to the learner, so check scripts must only print lines safe to show.
    Fail {
        output: String,
    },
}

pub(crate) trait Environment {
    /// Build the starting state. On error nothing may be left behind.
    fn prepare(&mut self, pkg: &Package, run_id: &str) -> Result<EnvHandle>;
    /// Release resources, keep the work.
    fn pause(&mut self, h: &EnvHandle) -> Result<()>;
    /// Return to the paused state. `Some(warning)` (raw text, for the log): the
    /// work is back but its programs did not come up, often because of the
    /// learner's half-done fix, so they must still be let in.
    fn resume(&mut self, pkg: &Package, h: &EnvHandle) -> Result<Option<String>>;
    /// Decide whether the task is done. Must not change what the learner built.
    fn check(&mut self, pkg: &Package, h: &EnvHandle) -> Result<CheckOutcome>;
    /// Remove what is left after a finished task. Missing is success.
    fn clean(&mut self, h: &EnvHandle) -> Result<()>;
}
