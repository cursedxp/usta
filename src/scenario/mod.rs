//! Scenario mode: Usta brings the work. A scenario is a package (role,
//! situation, task, check) run in a pluggable environment; the engine knows
//! only the five-operation `Environment` contract. Spec:
//! docs/superpowers/specs/2026-11-04-scenario-mode-design.md

pub(crate) mod command;
pub(crate) mod container;
pub(crate) mod docker;
pub(crate) mod dockerfile;
pub(crate) mod engine;
pub(crate) mod env;
pub(crate) mod ledger;
pub(crate) mod library;
pub(crate) mod limited_build;
pub(crate) mod lint;
pub(crate) mod package;
pub(crate) mod pick;
pub(crate) mod route;
pub(crate) mod select;
pub(crate) mod slot;
pub(crate) mod steps;
pub(crate) mod store;
pub(crate) mod sweep;
pub(crate) mod terminal;
pub(crate) mod turns;

#[cfg(test)]
mod docker_tests;
#[cfg(test)]
mod flow_tests;
