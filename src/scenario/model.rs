//! How scenario code reaches the model: blocking, one self-contained prompt per
//! call, no session. Feedback rounds put everything into the next prompt, so the
//! CLI and API backends behave the same.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};

use crate::anthropic::{Message, CLOSING_MAX_TOKENS};
use crate::backend::Backend;

const CALL_LIMIT: Duration = Duration::from_secs(600);

#[derive(Debug, Clone)]
pub(crate) struct ModelText {
    pub text: String,
    /// The API reported the reply hit the output budget, so it is cut off. The CLI backend never sets it.
    pub truncated: bool,
}

pub(crate) trait Model: Send + Sync {
    fn ask(&self, system: &str, prompt: &str) -> Result<ModelText>;
}

pub(crate) struct BackendModel {
    template: Backend,
}

impl BackendModel {
    pub fn new(session: &Backend) -> BackendModel {
        BackendModel {
            template: session.detached(),
        }
    }
}

impl Model for BackendModel {
    fn ask(&self, system: &str, prompt: &str) -> Result<ModelText> {
        let mut backend = self.template.detached();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("cannot start the model call")?;
        let history = [Message::user(prompt)];
        let call = backend.complete_with_limit(system, &history, CLOSING_MAX_TOKENS);
        // A quit drops the call, which kills the `claude -p` child (kill_on_drop).
        let reply = rt.block_on(async {
            tokio::select! {
                biased;
                _ = crate::shutdown::requested() => Err(anyhow!("stopped: Usta is closing")),
                r = tokio::time::timeout(CALL_LIMIT, call) => r
                    .map_err(|_| anyhow!(
                        "the model did not answer within {} minutes",
                        CALL_LIMIT.as_secs() / 60
                    ))
                    .and_then(|r| r),
            }
        })?;
        Ok(ModelText {
            text: reply.text,
            truncated: reply.truncated,
        })
    }
}

/// The model could not be reached or did not answer. Its own type, not `Internal`:
/// an outage is not a bad reply, so no retry or feedback round may treat it as one.
/// The command layer logs the text and shows the learner one plain sentence.
#[derive(Debug)]
pub(crate) struct ModelFailed(pub String);

impl std::fmt::Display for ModelFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ModelFailed {}

pub(crate) fn model_failed(e: anyhow::Error) -> anyhow::Error {
    anyhow::Error::new(ModelFailed(format!("{e:#}")))
}

/// For paths that never reach a model (session end, the sweep).
pub(crate) struct NoModel;

impl Model for NoModel {
    fn ask(&self, _: &str, _: &str) -> Result<ModelText> {
        bail!("no model is available here")
    }
}

pub(crate) type Progress = Arc<dyn Fn(&str) + Send + Sync>;

const IDLE: u8 = 0;
const OPEN: u8 = 1;
const CANCELLED: u8 = 2;

/// Esc during generation. Only generation opens this: nothing the learner can see
/// changes until a package passes, so stopping there is safe. `seal` closes it
/// atomically before the install, so an Esc is honoured in full or not at all.
#[derive(Default)]
pub(crate) struct Cancel(AtomicU8);

/// Closes the window on every way out of generation.
pub(crate) struct CancelGuard<'a>(&'a Cancel);

impl Drop for CancelGuard<'_> {
    fn drop(&mut self) {
        self.0 .0.store(IDLE, Ordering::SeqCst);
    }
}

impl Cancel {
    pub fn open(&self) -> CancelGuard<'_> {
        self.0.store(OPEN, Ordering::SeqCst);
        CancelGuard(self)
    }

    /// True when generation is running, so this Esc stops it.
    pub fn request(&self) -> bool {
        self.0
            .compare_exchange(OPEN, CANCELLED, Ordering::SeqCst, Ordering::SeqCst)
            .map_or_else(|now| now == CANCELLED, |_| true)
    }

    pub fn requested(&self) -> bool {
        self.0.load(Ordering::SeqCst) == CANCELLED
    }

    /// Close the window before installing; true when a cancel got in first.
    pub fn seal(&self) -> bool {
        self.0.swap(IDLE, Ordering::SeqCst) == CANCELLED
    }
}

/// What a command gets from the live session.
#[derive(Clone)]
#[allow(dead_code)] // reached from the command layer in Task 12
pub(crate) struct Live {
    pub model: Arc<dyn Model>,
    /// The conversation since the task opened; evidence for a conversation task.
    pub said: Vec<String>,
    pub progress: Progress,
    pub cancel: Arc<Cancel>,
}

impl Live {
    #[allow(dead_code)] // reached from the command layer in Task 12
    pub fn idle() -> Live {
        Live {
            model: Arc::new(NoModel),
            said: Vec::new(),
            progress: Arc::new(|_| {}),
            cancel: Arc::default(),
        }
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use std::sync::Mutex;

    use super::*;

    /// Replies in order, and remembers every prompt.
    #[derive(Default)]
    pub(crate) struct FakeModel {
        pub replies: Mutex<std::collections::VecDeque<Result<String, String>>>,
        pub prompts: Mutex<Vec<(String, String)>>,
    }

    impl FakeModel {
        pub fn with(replies: &[&str]) -> FakeModel {
            let m = FakeModel::default();
            m.replies
                .lock()
                .unwrap()
                .extend(replies.iter().map(|r| Ok(r.to_string())));
            m
        }
    }

    impl Model for FakeModel {
        fn ask(&self, system: &str, prompt: &str) -> Result<ModelText> {
            self.prompts
                .lock()
                .unwrap()
                .push((system.into(), prompt.into()));
            match self.replies.lock().unwrap().pop_front() {
                Some(Ok(text)) => Ok(ModelText {
                    text,
                    truncated: false,
                }),
                Some(Err(e)) => Err(anyhow!(e)),
                None => bail!("the fake model has no reply left"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn a_call_is_session_free_and_runs_off_the_ui_task() {
        use std::os::unix::fs::PermissionsExt;
        let d = crate::scenario::package::tests::tmp("model_cli");
        let bin = d.join("claude");
        let args = d.join("args");
        std::fs::write(
            &bin,
            format!("#!/bin/sh\necho \"$*\" > {}\ncat > /dev/null\necho '{{\"result\":\"hello\",\"session_id\":\"s2\"}}'\n", args.display()),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let session = Backend::Cli {
            model: "opus".into(),
            session_id: Some("mentor-session".into()),
            bin,
        };
        let m = BackendModel::new(&session);
        let r = tokio::task::spawn_blocking(move || m.ask("sys", "make a task"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(r.text, "hello");
        assert!(!r.truncated);
        assert!(!std::fs::read_to_string(&args).unwrap().contains("--resume"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn no_model_is_a_plain_error() {
        assert!(NoModel.ask("s", "p").is_err());
    }

    #[test]
    fn esc_counts_only_while_generation_is_open_and_never_after_the_seal() {
        let c = Cancel::default();
        assert!(!c.request(), "nothing to cancel");
        let guard = c.open();
        assert!(!c.requested());
        assert!(c.request() && c.request() && c.requested());
        assert!(c.seal(), "the seal reports the cancel");
        assert!(!c.request(), "after the seal the wait is step 1's");
        drop(guard);
        let _guard = c.open();
        assert!(!c.seal());
        assert!(!c.request());
    }

    #[test]
    fn a_closed_generation_leaves_no_cancel_behind() {
        let c = Cancel::default();
        {
            let _guard = c.open();
            c.request();
        }
        assert!(!c.requested() && !c.request());
    }
}
