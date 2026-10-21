//! What one conversation is working on right now: for a period, its digest
//! recites the chosen tasks and only counts the rest.
//!
//! ⚠ The only thing that hides an open task, so three rules hold:
//!
//! 1. It always expires ([`MAX`]); there is no "until I say otherwise".
//! 2. What is hidden is counted, with how to end the focus.
//! 3. The urgent breaks through ([`breaks_through`]), or a P0 filed mid-focus
//!    is buried.
//!
//! It applies to the digest only; a typed `task list` shows everything.

use std::collections::BTreeSet;

use anyhow::Context;
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;

use crate::error::AppError;
use crate::tasks::types::{Priority, Task};

type Result<T> = std::result::Result<T, AppError>;

/// Shorter lapses before the work starts, and looks broken.
pub const MIN: Duration = Duration::minutes(15);

/// Refused past this, not clamped (a clamp leaves the caller believing a number
/// never applied). Longer is a quiet handover; `task move` is the visible one.
pub const MAX: Duration = Duration::hours(24);

/// A session's focus period: what it is on, and when it lapses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Focus {
    pub until: DateTime<Utc>,
    /// Which tasks are recited; order is not part of it.
    pub tasks: BTreeSet<u64>,
}

impl Focus {
    /// Whether it still applies at `now` (an argument, for tests).
    pub fn holds_at(&self, now: DateTime<Utc>) -> bool {
        Self::live_at(self.until, now)
    }

    /// The rule on a bare timestamp, so [`current`] needn't spell its own copy.
    pub fn live_at(until: DateTime<Utc>, now: DateTime<Utc>) -> bool {
        until > now
    }
}

/// Whether a task shows whatever the focus.
///
/// ⚠ By effective rank, not the stored one: a near deadline makes a task `P0`
/// without a write. Overdue is its own arm, since an overdue task may have no
/// rank at all.
pub fn breaks_through(task: &Task) -> bool {
    task.overdue || task.urgency() == Some(Priority::P0)
}

/// A session's focus, if it still applies. Expiry is decided here, not by a
/// sweep: the row outlives its period so `task focus` can show the last one.
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

/// Enter a focus, replacing any current one. Naming no task is refused (it
/// would hide everything). Pile tasks are allowed; an id that names nothing is
/// refused, so a typo can't silently narrow the focus.
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
    // Replaced whole, like `--blocked-on`.
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

/// End a focus early; quiet if there was none.
pub async fn leave(pool: &MySqlPool, session: &str) -> Result<()> {
    sqlx::query("UPDATE sessions SET focus_until = NULL WHERE id = ?")
        .bind(session)
        .execute(pool)
        .await
        .context("ending a focus")?;
    Ok(())
}

/// A period as typed: `4h`, `90m`, `2h30m`.
pub fn spell(period: Duration) -> String {
    let (hours, minutes) = (period.num_hours(), period.num_minutes() % 60);
    match (hours, minutes) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h{m}m"),
    }
}

/// Read `4h`, `90m`, `2h30m` or `45`. ⚠ A bare number is minutes: both
/// readings of `--for 30` fit under [`MAX`], so the wrong one would pass.
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
