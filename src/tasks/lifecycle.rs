//! Checks on what happens to a task, answered by keyed reads, cheap enough for
//! any command (the model checks cost seconds). They warn and never refuse: the
//! caller may have reason to act against them, just not without seeing them.

use std::collections::BTreeSet;

use anyhow::Context;
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;

use crate::error::AppError;
use crate::tasks::types::{Actor, Moved, Status};

type Result<T> = std::result::Result<T, AppError>;

/// How long after a close a filing naming the task likely continues it, in seconds.
pub const RECENT: i64 = 24 * 60 * 60;

/// A task the filer closed recently, named by what they are filing now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Closed {
    pub id: u64,
    pub status: Status,
    /// When the filer closed it.
    pub at: DateTime<Utc>,
}

/// The ids a text names as `#<digits>`, in order, each once. A bare number is
/// as often a count or a port.
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

/// The tasks among `ids` that `actor` closed within [`RECENT`], by the last
/// status change: if somebody reopened and reclosed it, it is theirs now. Read
/// from columns, never `task_events.detail`, which is prose.
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

#[derive(sqlx::FromRow)]
struct Written {
    id: u64,
    kind: String,
    at: NaiveDateTime,
}

/// When a just-closed task's text was last written, if nothing was written
/// since its previous status change (or its filing). A body still describing
/// the problem under a done status misleads the next reader and the duplicate
/// check. Call only right after a close.
pub async fn unwritten(pool: &MySqlPool, id: u64) -> Result<Option<DateTime<Utc>>> {
    let last: Option<Written> = sqlx::query_as(
        "SELECT id, kind, at FROM task_events \
         WHERE task_id = ? AND kind IN ('created', 'edited') ORDER BY id DESC LIMIT 1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("reading when a task's text was last written")?;
    let Some(last) = last else {
        return Ok(None);
    };
    // The status event before the close.
    let moved: Option<(u64,)> = sqlx::query_as(
        "SELECT id FROM task_events WHERE task_id = ? AND kind = 'status' \
         ORDER BY id DESC LIMIT 1 OFFSET 1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("reading when a task last changed status")?;
    let stale =
        last.kind == Moved::Created.as_str() || moved.is_some_and(|(moved,)| last.id < moved);
    // The session zone is pinned to UTC in `db::connect`.
    Ok(stale.then(|| last.at.and_utc()))
}

/// How long before `now` something happened, as a person says it.
fn ago(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let minutes = (now - at).num_minutes().max(0);
    match minutes {
        0 => "just now".to_string(),
        1..=59 => format!("{minutes} min ago"),
        60..=2879 => format!("{} h ago", minutes / 60),
        _ => format!("{} days ago", minutes / 1440),
    }
}

/// What a close is told when it rewrote nothing.
pub fn rewrite_hint(id: u64, written: DateTime<Utc>, now: DateTime<Utc>) -> String {
    format!(
        "#{id}'s text was last written {}, before this work. Closing is a rewrite: \
         `task edit {id} --body -` to say what was found.",
        ago(written, now)
    )
}

/// What a filing is told about a task its filer closed. It gives both halves of
/// the remedy, since the new task has already landed.
pub fn reopen_hint(closed: &Closed, filed: u64, now: DateTime<Utc>) -> String {
    let ago = ago(closed.at, now);
    let id = closed.id;
    format!(
        "#{id} was closed ({}) by you {ago}. If this continues it: \
         `task reopen {id}` and `task drop {filed} --reason \"continues #{id}\"`.",
        closed.status.as_str()
    )
}

/// A command's line with its hints appended: sessions keep `| head -1` of a
/// write, so a hint on the next line is lost.
pub fn on_the_line(row: String, hints: impl IntoIterator<Item = String>) -> String {
    hints
        .into_iter()
        .fold(row, |line, hint| format!("{line}  {hint}"))
}
