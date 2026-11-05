//! One `Environment` over all environments, so the engine stays unaware of them.
//! `prepare` routes on the package, the others on `EnvHandle::kind`, because
//! `release_others` pauses runs of other kinds through this same object.

use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::scenario::container::ContainerEnv;
use crate::scenario::docker::Docker;
use crate::scenario::env::{CheckOutcome, EnvHandle, Environment, CONTAINER, CONVERSATION, FOLDER};
use crate::scenario::folder::FolderEnv;
use crate::scenario::model::Model;
use crate::scenario::package::Package;
use crate::scenario::talk::TalkEnv;

pub(crate) struct Router<'a> {
    pub container: ContainerEnv<'a>,
    pub folder: FolderEnv<'a>,
    pub talk: TalkEnv<'a>,
}

impl<'a> Router<'a> {
    pub fn new(
        docker: &'a dyn Docker,
        tag: &str,
        tasks: PathBuf,
        model: &'a dyn Model,
        said: &'a [String],
    ) -> Router<'a> {
        Router {
            container: ContainerEnv {
                docker,
                project_tag: tag.to_string(),
                tasks: tasks.clone(),
            },
            folder: FolderEnv {
                docker,
                project_tag: tag.to_string(),
                tasks,
                model,
            },
            talk: TalkEnv {
                project_tag: tag.to_string(),
                model,
                said,
            },
        }
    }

    fn pick(&mut self, kind: &str) -> Result<&mut dyn Environment> {
        match kind {
            CONTAINER => Ok(&mut self.container),
            FOLDER => Ok(&mut self.folder),
            CONVERSATION => Ok(&mut self.talk),
            other => bail!("environment kind '{other}' is not supported"),
        }
    }
}

impl Environment for Router<'_> {
    fn prepare(&mut self, pkg: &Package, run_id: &str) -> Result<EnvHandle> {
        self.pick(&pkg.environment.kind)?.prepare(pkg, run_id)
    }
    fn pause(&mut self, h: &EnvHandle) -> Result<()> {
        self.pick(&h.kind)?.pause(h)
    }
    fn resume(&mut self, pkg: &Package, h: &EnvHandle) -> Result<Option<String>> {
        self.pick(&h.kind)?.resume(pkg, h)
    }
    fn check(&mut self, pkg: &Package, h: &EnvHandle) -> Result<CheckOutcome> {
        self.pick(&h.kind)?.check(pkg, h)
    }
    fn clean(&mut self, h: &EnvHandle) -> Result<()> {
        self.pick(&h.kind)?.clean(h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::docker::fake::{ok, FakeDocker};
    use crate::scenario::env::{EnvHandle, Environment};
    use crate::scenario::model::NoModel;

    #[test]
    fn a_handle_of_an_unknown_kind_is_refused_and_touches_no_runtime() {
        let d = FakeDocker::new(|_| ok(""));
        let mut r = Router::new(&d, "aaaa1111", PathBuf::from("t"), &NoModel, &[]);
        let h = EnvHandle {
            reference: "usta-aaaa1111-x".into(),
            access: String::new(),
            kind: "vm".into(),
        };
        assert!(r.pause(&h).is_err());
        assert!(d.calls().is_empty());
    }

    #[test]
    fn a_container_handle_reaches_the_container_environment() {
        let d = FakeDocker::new(|_| ok(""));
        let mut r = Router::new(&d, "aaaa1111", PathBuf::from("t"), &NoModel, &[]);
        let h = EnvHandle {
            reference: "usta-aaaa1111-x".into(),
            access: String::new(),
            kind: "container".into(),
        };
        r.pause(&h).unwrap();
        assert_eq!(d.calls(), vec!["stop -t 2 usta-aaaa1111-x".to_string()]);
    }
}
