//! What is standing in the tracker, as numbers — the WORK, where the command
//! and check timings describe the tool. It answers *is the backlog getting
//! better or worse*, and whether a fix works — does the sprawl count fall —
//! without anyone hand-filtering `task list --json`.
//!
//! ⚠ **One query, and only for the caller handed the reporting job.** This rides
//! `POST /api/commands`, which every command hits;
//! [`commands::due_to_report`](crate::tasks::commands::due_to_report) keeps the
//! aggregates off the hot path of `task list`.
//!
//! ⚠ **Counted with the SAME macros the lists sort by.** `still_open!` and
//! `due_soon!` are shared rather than re-spelled, so a graph and a digest cannot
//! disagree about what is open or about which day it is.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;

use crate::error::AppError;
use crate::{due_soon, still_open};

type Result<T> = std::result::Result<T, AppError>;

/// What is standing, over every open task there is.
///
/// ⚠ **Fleet-wide and not per holder**, deliberately: a series per session
/// churns as conversations come and go, and the report is long already.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
    /// Open and doing alike: `still_open!`, so this is the same population every
    /// list in the service counts.
    pub open: u64,
    /// Held by nobody — the handover channel. An empty pile and a growing one
    /// mean opposite things, and neither shows in `open`.
    pub unheld: u64,
    /// Past its deadline by the DATABASE's clock, which is the one clock the
    /// digest, the app and the CLI already share.
    pub overdue: u64,
    /// `P0` or `P1` by EFFECTIVE rank, the mirror of `repo::list`'s ORDER BY:
    /// what the list sorts by rather than what somebody typed.
    pub urgent: u64,
    /// Waiting on something still open. Not the same as HAVING a blocker: the
    /// link is kept after a blocker closes, and what ends is its effect.
    pub blocked: u64,
    /// Carrying a density finding nobody has addressed.
    pub sprawling: u64,
}

/// Count it, in one pass.
pub async fn standing(pool: &MySqlPool) -> Result<Tally> {
    // ⚠ **Every `SUM` is CAST to SIGNED.** MariaDB types `SUM()` as DECIMAL,
    // which sqlx refuses to decode into `i64` — at runtime, on a real row.
    // `COUNT(*)` is already BIGINT.
    let row: (i64, i64, i64, i64, i64, i64) = sqlx::query_as(concat!(
        "SELECT COUNT(*), ",
        "CAST(COALESCE(SUM(t.assignee_kind = 'nobody'), 0) AS SIGNED), ",
        "CAST(COALESCE(SUM(t.due IS NOT NULL AND t.due < CURDATE()), 0) AS SIGNED), ",
        // The effective rank, spelled exactly as the sort spells it.
        "CAST(COALESCE(SUM(IF(",
        due_soon!("t.due"),
        ", 'P0', COALESCE(t.priority, 'P2')) <= 'P1'), 0) AS SIGNED), ",
        // Correlated rather than joined, for the reason the projection in `repo`
        // gives: a join to `task_blocks` MULTIPLIES the row by its edges, and a
        // COUNT over that would report edges as tasks.
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
