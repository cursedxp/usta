//! Scenario mode: Usta brings the work. A scenario is a package (role,
//! situation, task, check) run in a pluggable environment; the engine knows
//! only the five-operation `Environment` contract. Spec:
//! docs/superpowers/specs/2026-11-04-scenario-mode-design.md

pub(crate) mod command;
pub(crate) mod container;
pub(crate) mod docker;
pub(crate) mod dockerfile;
pub(crate) mod draft;
pub(crate) mod engine;
pub(crate) mod env;
pub(crate) mod folder;
pub(crate) mod generate;
pub(crate) mod ledger;
pub(crate) mod leftovers;
pub(crate) mod library;
pub(crate) mod limited_build;
pub(crate) mod lint;
pub(crate) mod model;
pub(crate) mod package;
pub(crate) mod pick;
pub(crate) mod prompt;
pub(crate) mod route;
pub(crate) mod rubric;
pub(crate) mod select;
pub(crate) mod slot;
pub(crate) mod start;
pub(crate) mod steps;
pub(crate) mod store;
pub(crate) mod sweep;
pub(crate) mod talk;
pub(crate) mod terminal;
pub(crate) mod turns;
pub(crate) mod validate;

#[cfg(test)]
mod docker_tests;
#[cfg(test)]
mod flow_tests;

#[cfg(test)]
mod pins {
    /// The hard rule, in code: a generated script never runs on the host, so no
    /// scenario module spawns anything except the docker seam and the window launcher.
    #[test]
    fn only_the_docker_seam_and_the_launcher_spawn_processes() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/scenario");
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if matches!(
                name.as_str(),
                "docker.rs" | "terminal.rs" | "docker_tests.rs" | "flow_tests.rs"
            ) {
                continue;
            }
            let text = std::fs::read_to_string(e.path()).unwrap();
            let prod = text.split("#[cfg(test)]").next().unwrap();
            for word in ["Command::new", "std::process::Command", "process::Command"] {
                assert!(!prod.contains(word), "{name} spawns a process");
            }
        }
    }

    /// `access` is written into the launcher file, so only a container run may have one.
    #[test]
    fn only_the_container_environment_builds_an_access_line() {
        for src in [include_str!("folder.rs"), include_str!("talk.rs")] {
            let prod = src.split("#[cfg(test)]").next().unwrap();
            assert!(prod.contains("access: String::new()"));
        }
    }
}
