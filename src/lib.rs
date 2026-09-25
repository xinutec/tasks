//! tasks — the work Claude sessions and Pippijn hand between each other.
//!
//! **Why a service and not a file.** A task list kept in a file is
//! re-serialised into the conversation every turn and grows until it dominates
//! the transcript. The fix is a different shape: **inject an index, fetch the
//! content.** Every change here keeps one property:
//!
//! > **What reaches a prompt is one line per OPEN task, and nothing else.**
//!
//! [`digest`] enforces it and is the only module whose output a hook sees.
//! Bodies, history and who moved what are fetched by whoever opens a task.
//!
//! Done tasks are kept and `task_events` records every move; the property is
//! held by the query — nothing injected selects a done row — not by deletion.

pub mod access;
pub mod agent_name;
pub mod config;
pub mod db;
pub mod digest;
pub mod error;
pub mod hook;
pub mod nextcloud;
pub mod routes;
pub mod session;
pub mod sessions;
pub mod state;
pub mod tasks;
pub mod wire;
