//! What one conversation is working on right now.
//!
//! A session with fifty open tasks pays for all fifty on every turn, and on any
//! given afternoon it is working on two of them. [`digest`](crate::digest) is
//! already the module that refuses per-turn cost; this is the one that lets a
//! session say which cost is worth paying at all:
//!
//! ```text
//! task focus 849 850 --for 4h
//! ```
//!
//! For four hours its digest recites those two and **counts** the rest.
//!
//! ⚠ **This is the only thing in the service that hides an open task, so three
//! rules hold and a change that breaks one is a regression however convenient
//! it looks.**
//!
//! 1. **It expires, and there is no way to say "until I say otherwise".** The
//!    expiry is what makes hiding safe: a focus nobody remembers to clear stops
//!    applying at its hour. [`MAX`] is a day for the same reason — past that it
//!    is not a focus, it is a quiet reassignment, and `task move` is how work
//!    changes hands where everybody can see it.
//! 2. **What is hidden is counted, never silent.** The digest says how many of
//!    each kind it left out and how to end the focus. The pile cap already
//!    works this way, and for the same reason: the party paying for a trim is
//!    the one who has to be told it happened.
//! 3. **The urgent breaks through** — see [`breaks_through`]. Without it a
//!    four-hour focus buries a P0 that Pippijn files five minutes into it, and a
//!    P0 from Pippijn is the drop-everything signal.
//!
//! **It applies to the digest and to nothing else.** `task list` keeps showing
//! everything, unmarked and unchanged: the digest is the channel nobody asked
//! for and is re-serialised every turn, where a list somebody typed is one they
//! wanted — and a focused session running `task list` is asking what to pick up
//! next, which is the one question focus must not answer with silence.
//!
//! It is also why `list` costs no extra request. Marking the focused rows there
//! would mean asking this module on the most-used command in the tool, to tell
//! a session something it typed a moment ago; bare `task focus` answers it on
//! demand instead.

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
/// Below this the period lapses before the work starts, and a focus that has
/// already expired by the time the next prompt renders looks exactly like a
/// broken one.
pub const MIN: Duration = Duration::minutes(15);

/// The longest.
///
/// ⚠ **A day, and it is refused rather than clamped past that.** fleetwatch
/// clamps an over-long mute silently, which means the caller believes a number
/// that was never applied; here the bound is named in the refusal. A week-long
/// focus is not a statement about this afternoon — it is a claim that the other
/// forty-eight tasks are somebody else's, and the honest way to say that is to
/// move them.
pub const MAX: Duration = Duration::hours(24);

/// A session's focus period: what it is on, and when it lapses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Focus {
    pub until: DateTime<Utc>,
    /// The task ids, ascending. A set rather than a list: focus answers *which*
    /// tasks a prompt recites and never in what order — the digest's own id
    /// order is the only ordering there is.
    pub tasks: BTreeSet<u64>,
}

impl Focus {
    /// Whether this focus still applies, at a given moment.
    ///
    /// Taken as an argument rather than read from the clock so that a test can
    /// state the hour it means. Every caller in the service passes `Utc::now()`.
    pub fn holds_at(&self, now: DateTime<Utc>) -> bool {
        self.until > now
    }
}

/// Whether a task is shown whatever the focus is.
///
/// ⚠ **The effective rank, not the chosen one.** `escalated_to` is what the
/// list sorted by — a deadline inside the week raises a task to `P0` without
/// anything being written — so reading `priority` here would let a focus bury
/// exactly the task the escalation exists to raise. This is the same
/// `escalated_to ?? priority` every renderer draws.
///
/// Overdue is its own arm rather than a consequence: a task can be past its
/// date with no rank at all, and the one thing a deadline that has already
/// passed must not do is go quiet.
pub fn breaks_through(task: &Task) -> bool {
    task.overdue || task.escalated_to.or(task.priority) == Some(Priority::P0)
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
    if until <= Utc::now() {
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
/// ⚠ **Naming no task is refused.** An empty focus is not "focus on nothing", it
/// is a digest with every task counted and none recited — the one state from
/// which a session cannot find its way back out, because the way back is a task
/// id it can no longer see.
///
/// ⚠ **Ids nobody holds are accepted on purpose.** Focusing on a task in the
/// pile is how a session says it has picked something up before it has moved
/// it, and refusing that would make focus a thing you can only do to work
/// already assigned. What is refused is an id that names nothing at all, since
/// that is a typo and the alternative is a focus quietly one task narrower than
/// the caller asked for.
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
        // ⚠ **Two bounds, two different mistakes, and the advice for one is
        // nonsense for the other.** A single sentence about handovers told a
        // caller who had asked for five minutes that their focus was too long.
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
    // The whole set is replaced, which is what `--blocked-on` already means one
    // command over: a caller states what it is on, never what to add.
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
/// one — `--for 30` is half an hour and not thirty hours, and reading it the
/// other way would silently grant thirty times what was asked for. It is inside
/// [`MAX`] either way, which is what makes the wrong reading a quiet one rather
/// than a refusal.
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
