//! The backlog as numbers: is it getting better or worse, and does a fix move
//! its count. (The command and check timings measure the tool instead.)
//!
//! Runs only for the caller [`commands::due_to_report`](crate::tasks::commands::due_to_report)
//! picks, keeping it off the hot path of every command. Counted with the same
//! `still_open!` and `due_soon!` the lists use, so a graph and a digest agree.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;

use crate::error::AppError;
use crate::{due_soon, still_open};

type Result<T> = std::result::Result<T, AppError>;

/// Over every open task, not per holder: per-session series churn as
/// conversations come and go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
    /// Open and doing alike (`still_open!`).
    pub open: u64,
    /// Held by nobody: the pile.
    pub unheld: u64,
    /// Past its deadline by the database's clock, the one the digest, app and
    /// CLI share.
    pub overdue: u64,
    /// `P0` or `P1` by effective rank, as `repo::list` sorts.
    pub urgent: u64,
    /// Waiting on something still open. A closed blocker's link is kept, so
    /// having one is not the same.
    pub blocked: u64,
    /// Carrying a density finding nobody has addressed.
    pub sprawling: u64,
}

/// Count it, in one pass.
pub async fn standing(pool: &MySqlPool) -> Result<Tally> {
    // ⚠ Every `SUM` is CAST to SIGNED: MariaDB types it DECIMAL, which sqlx
    // refuses to decode into `i64`, at runtime, on real rows only.
    let row: (i64, i64, i64, i64, i64, i64) = sqlx::query_as(concat!(
        "SELECT COUNT(*), ",
        "CAST(COALESCE(SUM(t.assignee_kind = 'nobody'), 0) AS SIGNED), ",
        "CAST(COALESCE(SUM(t.due IS NOT NULL AND t.due < CURDATE()), 0) AS SIGNED), ",
        // The effective rank, as the sort spells it.
        "CAST(COALESCE(SUM(IF(",
        due_soon!("t.due"),
        ", 'P0', COALESCE(t.priority, 'P2')) <= 'P1'), 0) AS SIGNED), ",
        // Correlated, not joined: a join to `task_blocks` repeats the row per
        // edge, and COUNT would count edges.
        "CAST(COALESCE(SUM(EXISTS(SELECT 1 FROM task_blocks b JOIN tasks bt ON bt.id = b.blocked_on ",
        "WHERE b.task_id = t.id AND ",
        still_open!("bt.status"),
        ")), 0) AS SIGNED), ",
        "CAST(COALESCE(SUM(t.sprawl_chars IS NOT NULL), 0) AS SIGNED) ",
        "FROM tasks t WHERE ",
        still_open!("t.status")
    ))
    .fetch_one(pool)
    .await
    .context("counting what is standing")?;

    Ok(Tally {
        open: row.0.max(0) as u64,
        unheld: row.1.max(0) as u64,
        overdue: row.2.max(0) as u64,
        urgent: row.3.max(0) as u64,
        blocked: row.4.max(0) as u64,
        sprawling: row.5.max(0) as u64,
    })
}
