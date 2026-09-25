//! What one conversation is working on right now.
//!
//! A session pays for every open task it holds on every turn, while working on
//! one or two. This lets it say which are worth paying for: for the period
//! named, its digest recites the chosen tasks and **counts** the rest.
//!
//! ⚠ **This is the only thing in the service that hides an open task, so three
//! rules hold and a change that breaks one is a regression however convenient
//! it looks.**
//!
//! 1. **It expires, and there is no "until I say otherwise".** The expiry is
//!    what makes hiding safe: a focus nobody clears stops applying at its hour.
//!    See [`MAX`].
//! 2. **What is hidden is counted, never silent.** The digest says how many of
//!    each kind it left out and how to end the focus.
//! 3. **The urgent breaks through** — see [`breaks_through`]. Without it a focus
//!    buries a P0 filed minutes into it, and a P0 is the drop-everything signal.
//!
//! **It applies to the digest and to nothing else.** The digest is the channel
//! nobody asked for; a list somebody typed is one they wanted, and a focused
//! session running `task list` is asking what to pick up next.

use std::collections::BTreeSet;

use anyhow::Context;
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;

use crate::error::AppError;
use crate::tasks::types::{Priority, Task};

type Result<T> = std::result::Result<T, AppError>;

/// The shortest focus worth entering.
///
/// Below this the period lapses before the work starts, and an expired focus
/// looks exactly like a broken one.
pub const MIN: Duration = Duration::minutes(15);

/// The longest.
///
/// ⚠ **Refused past this, not clamped**: clamping leaves the caller believing a
/// number that was never applied. Longer than a day is not a focus but a quiet
/// reassignment, and `task move` is how work changes hands where everybody
/// can see it.
pub const MAX: Duration = Duration::hours(24);

/// A session's focus period: what it is on, and when it lapses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Focus {
    pub until: DateTime<Utc>,
    /// The task ids. A set: focus says *which* tasks are recited, never in what
    /// order.
    pub tasks: BTreeSet<u64>,
}

impl Focus {
    /// Whether this focus still applies, at a given moment.
    ///
    /// `now` is an argument so a test can state the hour it means.
    pub fn holds_at(&self, now: DateTime<Utc>) -> bool {
        Self::live_at(self.until, now)
    }

    /// The rule itself, before there is a [`Focus`] to ask.
    ///
    /// ⚠ **One spelling of the rule**: [`current`] decides it from a bare
    /// timestamp before reading the task ids, and an inline comparison there
    /// would be an untested second copy.
    pub fn live_at(until: DateTime<Utc>, now: DateTime<Utc>) -> bool {
        until > now
    }
}

/// Whether a task is shown whatever the focus is.
///
/// ⚠ **The effective rank, not the chosen one.** A near deadline raises a task
/// to `P0` without anything being written, so reading `priority` would let a
/// focus bury exactly the task the escalation exists to raise.
///
/// Overdue is its own arm: a task can be past its date with no rank at all,
/// and a passed deadline must not go quiet.
pub fn breaks_through(task: &Task) -> bool {
    task.overdue || task.urgency() == Some(Priority::P0)
}

/// Read a session's focus, if it has one that still applies.
///
/// **Expiry is answered here rather than by a sweep.** The row survives its
/// period so that `task focus` can say what the last one was; this returns
/// `None` for it, so no caller has to remember to compare the clock.
pub async fn current(pool: &MySqlPool, session: &str) -> Result<Option<Focus>> {
    let Some(until): Option<Option<NaiveDateTime>> =
        sqlx::query_scalar("SELECT focus_until FROM sessions WHERE id = ?")
            .bind(session)
            .fetch_optional(pool)
            .await
            .context("reading a focus")?
    else {
        return Ok(None);
    };
    let Some(until) = until else {
        return Ok(None);
    };
    let until = until.and_utc();
    if !Focus::live_at(until, Utc::now()) {
        return Ok(None);
    }
    let tasks: Vec<u64> =
        sqlx::query_scalar("SELECT task_id FROM session_focus WHERE session = ? ORDER BY task_id")
            .bind(session)
            .fetch_all(pool)
            .await
            .context("reading a focus")?;
    Ok(Some(Focus {
        until,
        tasks: tasks.into_iter().collect(),
    }))
}

/// Enter a focus period, replacing whatever the session was focused on.
///
/// ⚠ **Naming no task is refused.** An empty focus counts every task and
/// recites none, and the way back out is a task id the session can no longer
/// see.
///
/// ⚠ **Ids nobody holds are accepted**: focusing on a pile task is how a session
/// says it has picked it up before moving it. An id that names nothing is
/// refused — a typo would otherwise narrow the focus silently.
pub async fn enter(
    pool: &MySqlPool,
    session: &str,
    tasks: &BTreeSet<u64>,
    period: Duration,
) -> Result<Focus> {
    if tasks.is_empty() {
        return Err(AppError::BadRequest(
            "a focus has to name at least one task — an empty one would hide every task \
             you have and leave you nothing to focus on next."
                .into(),
        ));
    }
    if period < MIN || period > MAX {
        // Two bounds, two mistakes, and the advice for one is nonsense for the
        // other.
        let why = if period > MAX {
            "Longer than a day is not a focus but a handover — `task move <id> <who>` is \
             how work changes hands where everybody can see it."
        } else {
            "A shorter one lapses before the work starts, and an expired focus reads \
             exactly like a broken one."
        };
        return Err(AppError::BadRequest(format!(
            "a focus runs from {} to {}, and {} is outside that. {why}",
            spell(MIN),
            spell(MAX),
            spell(period),
        )));
    }

    let mut tx = pool.begin().await.context("starting a focus")?;
    for id in tasks {
        let exists: Option<u64> = sqlx::query_scalar("SELECT id FROM tasks WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .context("checking a task")?;
        if exists.is_none() {
            return Err(AppError::BadRequest(format!(
                "there is no task #{id}, so nothing was focused."
            )));
        }
    }

    let until = Utc::now() + period;
    sqlx::query("UPDATE sessions SET focus_until = ? WHERE id = ?")
        .bind(until.naive_utc())
        .bind(session)
        .execute(&mut *tx)
        .await
        .context("starting a focus")?;
    // The whole set is replaced, as `--blocked-on` is: a caller states what it
    // is on, never what to add.
    sqlx::query("DELETE FROM session_focus WHERE session = ?")
        .bind(session)
        .execute(&mut *tx)
        .await
        .context("starting a focus")?;
    for id in tasks {
        sqlx::query("INSERT INTO session_focus (session, task_id) VALUES (?, ?)")
            .bind(session)
            .bind(id)
            .execute(&mut *tx)
            .await
            .context("starting a focus")?;
    }
    tx.commit().await.context("starting a focus")?;

    Ok(Focus {
        until,
        tasks: tasks.clone(),
    })
}

/// End a focus period early. Silent about there not having been one — the
/// caller asked to be unfocused and is.
pub async fn leave(pool: &MySqlPool, session: &str) -> Result<()> {
    sqlx::query("UPDATE sessions SET focus_until = NULL WHERE id = ?")
        .bind(session)
        .execute(pool)
        .await
        .context("ending a focus")?;
    Ok(())
}

/// A period as somebody would say it: `4h`, `90m`, `2h30m`.
///
/// Used in the refusal above, so the bound a caller is told is spelled the same
/// way as the argument they typed.
pub fn spell(period: Duration) -> String {
    let (hours, minutes) = (period.num_hours(), period.num_minutes() % 60);
    match (hours, minutes) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h{m}m"),
    }
}

/// Read a period the way somebody types one: `4h`, `90m`, `2h30m`, `45`.
///
/// ⚠ **A bare number is minutes**, because the unit somebody omits is the small
/// one. Both readings of `--for 30` sit inside [`MAX`], so the wrong one would
/// pass silently.
pub fn parse(text: &str) -> anyhow::Result<Duration> {
    let text = text.trim().to_ascii_lowercase();
    anyhow::ensure!(!text.is_empty(), "a focus needs a period: --for 4h");

    let mut total = Duration::zero();
    let mut digits = String::new();
    let mut had_unit = false;
    for c in text.chars() {
        match c {
            '0'..='9' => digits.push(c),
            'h' | 'm' => {
                let n: i64 = digits
                    .parse()
                    .with_context(|| format!("{text:?} is not a period — try 4h, 90m, 2h30m"))?;
                digits.clear();
                had_unit = true;
                total += if c == 'h' {
                    Duration::hours(n)
                } else {
                    Duration::minutes(n)
                };
            }
            _ => anyhow::bail!("{text:?} is not a period — try 4h, 90m, 2h30m"),
        }
    }
    if !digits.is_empty() {
        let n: i64 = digits
            .parse()
            .with_context(|| format!("{text:?} is not a period — try 4h, 90m, 2h30m"))?;
        total += Duration::minutes(n);
    } else {
        anyhow::ensure!(had_unit, "{text:?} is not a period — try 4h, 90m, 2h30m");
    }
    Ok(total)
}
