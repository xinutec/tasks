//! tasks — the work Claude sessions and Pippijn hand between each other.
//!
//! **Why this is a service and not a file.** Every task list this project has
//! had was measured and thrown away for the same reason: it was re-serialised
//! into the conversation on every turn. The CLI's built-in list reached 527 kB
//! a turn on one session — 93% of it a `description` field the prompt never
//! renders — and became 73% of a 3.7 GB transcript. The fix was never a shorter
//! list; it was a different shape: **inject an index, fetch the content.**
//!
//! That constraint survives the move off files and into this service, and it is
//! the one property every change here has to keep:
//!
//! > **What reaches a prompt is one line per OPEN task, and nothing else.**
//!
//! [`digest`] is where that is enforced, and it is deliberately the only module
//! whose output a hook ever sees. Everything else — bodies, history, who moved
//! what — is fetched by a person or a session that has decided to open one
//! particular task.
//!
//! **What is different now that there are no files.** The file scheme deleted a
//! finished task, because keeping it is what turned 48 live items into 366 and
//! git recorded the completion better than a flag did. There is no git here, so
//! this service keeps done tasks and `task_events` records the moves — see
//! `migrations/0001_init.sql`. The original property is preserved by the
//! *query*, not by deletion: nothing injected ever selects a done row.

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
