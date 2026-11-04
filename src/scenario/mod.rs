//! Scenario mode: Usta brings the work. A scenario is a package (role,
//! situation, task, check) run in a pluggable environment; the engine knows
//! only the five-operation `Environment` contract. Spec:
//! docs/superpowers/specs/2026-11-04-scenario-mode-design.md
#![allow(dead_code)] // removed in Task 13, once the commands are wired

pub(crate) mod package;
