//! The conversation environment: no setup, no resources, Usta judges what was said.

use anyhow::Result;

use crate::scenario::env::{internal, CheckOutcome, EnvHandle, Environment, CONVERSATION};
use crate::scenario::model::Model;
use crate::scenario::package::Package;
use crate::scenario::rubric::panel;

pub(crate) struct TalkEnv<'a> {
    pub project_tag: String,
    pub model: &'a dyn Model,
    pub said: &'a [String],
}

impl TalkEnv<'_> {
    fn owned(&self, h: &EnvHandle) -> Result<()> {
        if h.reference
            .starts_with(&format!("usta-{}-", self.project_tag))
        {
            Ok(())
        } else {
            Err(internal(format!(
                "refusing a task that is not this project's: {}",
                h.reference
            )))
        }
    }
}

impl Environment for TalkEnv<'_> {
    fn prepare(&mut self, _: &Package, run_id: &str) -> Result<EnvHandle> {
        Ok(EnvHandle {
            reference: format!("usta-{}-{run_id}", self.project_tag),
            access: String::new(),
            kind: CONVERSATION.into(),
        })
    }
    fn pause(&mut self, h: &EnvHandle) -> Result<()> {
        self.owned(h)
    }
    fn resume(&mut self, _: &Package, h: &EnvHandle) -> Result<Option<String>> {
        self.owned(h).map(|_| None)
    }
    fn check(&mut self, pkg: &Package, h: &EnvHandle) -> Result<CheckOutcome> {
        self.owned(h)?;
        if !self.said.iter().any(|l| l.starts_with("learner:")) {
            return Ok(CheckOutcome::Fail {
                output: "Usta has nothing from you on this task yet. Answer here first, then /scenario check.".into(),
            });
        }
        panel(self.model, pkg, &self.said.join("\n"))
    }
    fn clean(&mut self, h: &EnvHandle) -> Result<()> {
        self.owned(h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::model::fake::FakeModel;
    use crate::scenario::package::tests::tmp;

    #[test]
    fn the_five_operations_need_no_runtime_and_silence_is_a_gentle_fail() {
        let root = tmp("talk_ops");
        let p = Package::load(&crate::scenario::lint::tests::folder_pkg(&root)).unwrap();
        let model = FakeModel::default();
        let mut e = TalkEnv {
            project_tag: "aaaa1111".into(),
            model: &model,
            said: &[],
        };
        let h = e.prepare(&p, "r1").unwrap();
        assert_eq!(
            (h.reference.as_str(), h.kind.as_str()),
            ("usta-aaaa1111-r1", "conversation")
        );
        e.pause(&h).unwrap();
        assert_eq!(e.resume(&p, &h).unwrap(), None);
        let CheckOutcome::Fail { output } = e.check(&p, &h).unwrap() else {
            panic!()
        };
        assert!(output.contains("Answer here first"), "{output}");
        assert!(model.prompts.lock().unwrap().is_empty());
        e.clean(&h).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }
}
