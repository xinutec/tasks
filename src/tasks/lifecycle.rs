//! Checks on what happens to a task, answered from the store.
//!
//! The model checks ([`duplicates`](super::duplicates), [`density`](super::density))
//! read prose and cost tens of seconds, so they run only where prose is
//! written. A lifecycle question — is this id closed, by whom, when — is a keyed
//! read, cheap enough to ask on any command.
//!
//! ⚠ **These warn and never refuse.** Each looks at facts the caller may have
//! good reason to act against; what is wrong is acting without having seen
//! them.

use std::collections::BTreeSet;

use anyhow::Context;
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;

use crate::error::AppError;
use crate::tasks::types::{Actor, Status};

type Result<T> = std::result::Result<T, AppError>;

/// How long after closing a task a filing that names it is likely to continue
/// it, in seconds. A day: a session's working stretch, not its whole life.
pub const RECENT: i64 = 24 * 60 * 60;

/// A task the filer closed recently, named by what they are filing now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Closed {
    pub id: u64,
    pub status: Status,
    /// When the filer closed it.
    pub at: DateTime<Utc>,
}

/// The ids a text names as `#<digits>`, in order, each once.
///
/// Only the hashed spelling: a bare number in prose is as often a count or a
/// port as a task.
pub fn mentioned(text: &str) -> Vec<u64> {
    let mut seen = BTreeSet::new();
    let mut ids = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find('#') {
        rest = &rest[at + 1..];
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if let Ok(id) = rest[..digits].parse::<u64>()
            && seen.insert(id)
        {
            ids.push(id);
        }
    }
    ids
}

#[derive(sqlx::FromRow)]
struct Row {
    status: Status,
    at: NaiveDateTime,
}

/// The tasks among `ids` that `actor` closed within [`RECENT`].
///
/// ⚠ **Decided on the task's LAST status change.** A task the filer closed and
/// somebody else reopened and closed again is that other closer's call now.
///
/// Read from columns, never from `task_events.detail`: that is one line
/// rendered for a person, and parsing it would make it a wire format.
pub async fn closed_by(pool: &MySqlPool, actor: &Actor, ids: &[u64]) -> Result<Vec<Closed>> {
    let mut closed = Vec::new();
    for &id in ids {
        let row: Option<Row> = sqlx::query_as(
            "SELECT t.status, e.at FROM tasks t \
             JOIN task_events e ON e.task_id = t.id \
             WHERE t.id = ? AND t.status IN ('done', 'dropped') AND e.kind = 'status' \
               AND e.actor_kind = ? AND e.actor_id = ? \
               AND e.at > NOW() - INTERVAL ? SECOND \
               AND e.id = (SELECT MAX(l.id) FROM task_events l \
                           WHERE l.task_id = t.id AND l.kind = 'status')",
        )
        .bind(id)
        .bind(actor.kind())
        .bind(actor.id())
        .bind(RECENT)
        .fetch_optional(pool)
        .await
        .context("looking for a task this filer closed")?;
        if let Some(row) = row {
            closed.push(Closed {
                id,
                status: row.status,
                // The session zone is pinned to UTC in `db::connect`.
                at: row.at.and_utc(),
            });
        }
    }
    Ok(closed)
}

/// What a filing is told about a task its filer closed, `now` being when it
/// was filed.
///
/// Both halves of the remedy: the filing has landed, so continuing the old task
/// also means dropping the new one.
pub fn reopen_hint(closed: &Closed, filed: u64, now: DateTime<Utc>) -> String {
    let minutes = (now - closed.at).num_minutes().max(0);
    let ago = match minutes {
        0 => "just now".to_string(),
        1..=59 => format!("{minutes} min ago"),
        _ => format!("{} h ago", minutes / 60),
    };
    let id = closed.id;
    format!(
        "#{id} was closed ({}) by you {ago}. If this continues it: \
         `task reopen {id}` and `task drop {filed} --reason \"continues #{id}\"`.",
        closed.status.as_str()
    )
}
