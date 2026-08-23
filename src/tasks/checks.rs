//! What the two model checks did, recorded so that their rate and their latency
//! are facts.
//!
//! [`duplicates`](crate::tasks::duplicates) runs before a filing and can refuse
//! it; [`density`](crate::tasks::density) runs after an edit and can only
//! advise. Both spawn a one-shot session, read its answer, and delete its
//! transcript — so until this table, a check that ran left nothing behind
//! except, on the filing side, a line on the caller's stderr.
//!
//! ⚠ **The questions this exists to answer are already open.** Whether the
//! density read fires at the rate it was calibrated for, and whether
//! `PATIENCE` abandons calls that would have answered, are both questions about
//! a distribution — and a distribution cannot be read out of the transcripts
//! that happen to survive. The row is written on every path, including the ones
//! that failed, because a check that did not run is the finding.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;

use crate::error::AppError;
use crate::wire::RequiredKeys;

type Result<T> = std::result::Result<T, AppError>;

/// Which check ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Before a filing, against every open title.
    Filing,
    /// After an edit, against the one body that has grown.
    Density,
}

/// What came of it.
///
/// ⚠ **`Quiet` and `Timeout` are the two that look alike from outside and mean
/// opposite things.** A quiet check read the input and had nothing to say; a
/// timed-out one never answered at all, and the task was filed or the edit kept
/// regardless. Counting them together would report a well-behaved tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// It ran and said nothing.
    Quiet,
    /// It named a duplicate, or gave advice on a body.
    Spoke,
    /// No answer inside the patience the caller allowed.
    Timeout,
    /// It could not be asked, or what came back could not be read.
    Error,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Filing => "filing",
            Kind::Density => "density",
        }
    }
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Outcome::Quiet => "quiet",
            Outcome::Spoke => "spoke",
            Outcome::Timeout => "timeout",
            Outcome::Error => "error",
        }
    }
}

/// One run, as the CLI reports it.
///
/// The session and the clock are the service's: a client that timed its own
/// call is already trusted for the duration, but who it was and when it
/// happened are the two fields a caller must not be able to get wrong.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub kind: Kind,
    /// The task whose body was read. Absent on a filing check.
    #[serde(default)]
    pub task_id: Option<u64>,
    /// Characters put to the model.
    pub input_chars: u32,
    /// What crossed the sampler, on a density read.
    #[serde(default)]
    pub accreted: Option<u32>,
    pub elapsed_ms: u32,
    pub outcome: Outcome,
}

impl RequiredKeys for Run {
    fn required() -> &'static [(&'static str, &'static str)] {
        &[
            ("kind", "`filing` or `density`"),
            ("input_chars", "how many characters were put to the model"),
            ("elapsed_ms", "how long the call took, in milliseconds"),
            (
                "outcome",
                "`quiet`, `spoke`, `timeout` or `error` — a check that did not \
                 run is the finding, so there is no arm for leaving it out",
            ),
        ]
    }
}

/// Write one down.
pub async fn record(pool: &MySqlPool, session: &str, run: &Run) -> Result<()> {
    sqlx::query(
        "INSERT INTO check_run \
         (ran_at, kind, session, task_id, input_chars, accreted, elapsed_ms, outcome) \
         VALUES (NOW(), ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(run.kind.as_str())
    .bind(session)
    .bind(run.task_id)
    .bind(run.input_chars)
    .bind(run.accreted)
    .bind(run.elapsed_ms)
    .bind(run.outcome.as_str())
    .execute(pool)
    .await
    .context("recording what a check did")?;
    Ok(())
}

/// What to record for an answer that came back, or did not.
///
/// ⚠ **A timed-out check and a quiet one are opposite findings that look the
/// same from outside**: both leave the caller with no advice and the write
/// already done. This is where they are told apart, and it turns on the error's
/// chain rather than on its message, so rewording the line a caller prints
/// cannot silently reclassify a month of runs.
///
/// `spoke` is the caller's, because only it knows whether the words amounted to
/// anything: the same answer is a refusal on one path and advice on the other.
pub fn outcome(said: &anyhow::Result<String>, spoke: bool) -> Outcome {
    match said {
        Ok(_) if spoke => Outcome::Spoke,
        Ok(_) => Outcome::Quiet,
        Err(why)
            if why
                .chain()
                .any(|link| link.is::<tokio::time::error::Elapsed>()) =>
        {
            Outcome::Timeout
        }
        Err(_) => Outcome::Error,
    }
}
