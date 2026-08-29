//! What is standing in the tracker, as numbers — the half fleetwatch never saw.
//!
//! ⚠ **Every line the `task-timings` collector sent was about the TOOL.**
//! Command latency, check latency, how often a check spoke or never answered —
//! all of it describes the tracker's machinery and none of it describes the work
//! the tracker exists to hold. So the one question a graph could not answer was
//! *is the backlog getting better or worse*, which is the question somebody
//! looking at a task tracker's dashboard is actually asking.
//!
//! ⚠ **The sprawl count is why this was built when it was.** `0014` put a
//! density read's critique on the task and a `[sprawl N]` mark in every holder's
//! digest, on the argument that a per-turn reminder is the one channel that
//! cannot be scrolled past. Whether that WORKS is exactly one number — does the
//! flagged count fall — and the only way to ask it was a hand-filtered
//! `task list --all --json`, which is the shape that once reported **137** tasks
//! in the pile against a real **5**. A fix nobody can chart is a fix nobody can
//! defend keeping.
//!
//! ⚠ **One query, and only for the caller that was handed the reporting job.**
//! This rides `POST /api/commands`, which every single command hits; a tally
//! computed on every one of them would put six aggregates on the hot path of
//! `task list`. `commands::due_to_report` gates it to one caller an hour — see
//! `routes::api`, where the same gate already guards the other two tallies.
//!
//! ⚠ **Counted with the SAME macros the lists sort by.** `still_open!` and
//! `due_soon!` are shared rather than re-spelled, so a graph and a digest cannot
//! disagree about what is open or about which day it is. Three copies of a date
//! comparison are three chances to be wrong, which is what those macros exist to
//! say.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;

use crate::error::AppError;
use crate::{due_soon, still_open};

type Result<T> = std::result::Result<T, AppError>;

/// What is standing, over every open task there is.
///
/// ⚠ **Fleet-wide and not per holder**, deliberately. A series per session is a
/// series that churns as conversations come and go, and the report already runs
/// to fifteen lines — see `#1253`, which is where that decision belongs rather
/// than here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
    /// Open and doing alike: `still_open!`, so this is the same population every
    /// list in the service counts.
    pub open: u64,
    /// Held by nobody — the handover channel. Its own line because an empty pile
    /// and a growing one mean opposite things about whether work is reaching
    /// somebody, and neither is visible in `open`.
    pub unheld: u64,
    /// Past its deadline by the DATABASE's clock, which is the one clock the
    /// digest, the app and the CLI already share.
    pub overdue: u64,
    /// `P0` or `P1` by EFFECTIVE rank — a deadline inside the week raises a task,
    /// and this counts what the list actually sorts by rather than what somebody
    /// typed. The mirror of `repo::list`'s ORDER BY.
    pub urgent: u64,
    /// Waiting on something still open. Not the same as HAVING a blocker: the
    /// link is kept after a blocker closes, and what ends is its effect.
    pub blocked: u64,
    /// Carrying a density finding nobody has addressed — see `0014`. The number
    /// this module was built to make chartable.
    pub sprawling: u64,
}

/// Count it, in one pass.
pub async fn standing(pool: &MySqlPool) -> Result<Tally> {
    // ⚠ **Every `SUM` is CAST to SIGNED, and this is not decoration.** MariaDB
    // types `SUM()` as DECIMAL, and sqlx refuses to decode a DECIMAL into `i64`
    // — at RUNTIME, on a real row, so a type that compiles cleanly fails the
    // first time it meets a database. `COUNT(*)` is already BIGINT and needs no
    // cast; the five conditional aggregates all do.
    //
    // dev-lint: allow-sqlx — a `concat!`ed literal; the macros expand at compile
    // time and nothing here is built from a runtime string.
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
