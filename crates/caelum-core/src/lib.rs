#![forbid(unsafe_code)]

pub mod antigravity;
pub mod application;
pub mod approvals;
mod blocking_actor;
pub mod checkpoints;
pub mod codex;
pub mod database;
pub mod desktop;
pub mod diagnostics;
mod executable;
pub mod files;
pub mod git;
pub mod github;
pub mod host_stats;
mod images;
mod json_lines;
pub mod opencode;
pub mod processes;
pub mod projects;
pub mod questions;
pub mod remote;
pub mod runner;
pub mod session_branches;
pub mod side_questions;
pub mod telemetry;
pub mod templates;
pub mod updates;
pub mod worktrees;

#[cfg(all(test, unix))]
mod test_support;
