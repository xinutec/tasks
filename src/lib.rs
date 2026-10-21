//! tasks — the work Claude sessions and the user hand between each other.
//!
//! A task list in a file is re-sent into the conversation every turn and grows
//! until it dominates the transcript, so this injects an index and fetches the
//! content. The one property every change keeps:
//!
//! > **What reaches a prompt is one line per OPEN task, and nothing else.**
//!
//! [`digest`] enforces it and is the only module a hook reads. Closed tasks and
//! their history are kept; the query, not deletion, keeps them out.

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
