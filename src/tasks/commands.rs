//! How long each CLI command took, recorded from real use. No prober: a timer
//! times what no session runs, from a cold process. Recording is sent after the
//! answer prints and fails silently.

use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;

use crate::error::AppError;
use crate::wire::RequiredKeys;

type Result<T> = std::result::Result<T, AppError>;

/// How a command ended. Refusals are counted apart: they usually skip the round
/// trip, and counted as errors they make `add`, which refuses by design, look
/// broken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ended {
    Ok,
    /// The tool declined: a guard fired, or a check refused.
    Refused,
    /// Something went wrong.
    Error,
}

impl Ended {
    fn as_str(self) -> &'static str {
        match self {
            Ended::Ok => "ok",
            Ended::Refused => "refused",
            Ended::Error => "error",
        }
    }

    /// Older clients send `error` for refusals too; those stay `Error`.
    fn read(word: &str) -> Option<Ended> {
        match word {
            "ok" => Some(Ended::Ok),
            "refused" => Some(Ended::Refused),
            "error" => Some(Ended::Error),
            _ => None,
        }
    }
}

/// The marker [`declined`] puts under a refusal: a type, never a message match,
/// so rewording can't reclassify past runs.
#[derive(Debug)]
pub struct Refused;

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the tool declined")
    }
}

impl std::error::Error for Refused {}

/// A refusal: the caller reads the same words; only the classifier sees the marker.
pub fn declined(said: impl std::fmt::Display) -> anyhow::Error {
    anyhow::Error::new(Refused).context(said.to_string())
}

pub fn ended(done: &anyhow::Result<()>) -> Ended {
    match done {
        Ok(()) => Ended::Ok,
        Err(why) if was_refused(why) => Ended::Refused,
        Err(_) => Ended::Error,
    }
}

fn was_refused(why: &anyhow::Error) -> bool {
    why.chain().any(|link| link.is::<Refused>())
}

/// What a caller reads when a command ends badly.
///
/// ⚠ A refusal prints its sentence and nothing below it: sessions read the last
/// lines, and anyhow's chain would end with `Caused by: the tool declined`.
/// Real failures keep the chain, which is how they are diagnosed.
pub fn said(why: &anyhow::Error) -> String {
    match was_refused(why) {
        true => format!("{why}"),
        false => format!("{why:?}"),
    }
}

/// One command as the CLI reports it. The service supplies the clock and the
/// session; the client is trusted only for how long it took.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    /// From the CLI's own command enum.
    pub verb: String,
    pub elapsed_ms: u32,
    pub outcome: Ended,
    /// Whether it waited for a model check, which is what separates the two `edit`
    /// populations. NULL from older clients: unknown, not `false`.
    #[serde(default)]
    pub waited_for_a_model: Option<bool>,
}

impl RequiredKeys for Run {
    fn required() -> &'static [(&'static str, &'static str)] {
        &[
            ("verb", "which subcommand ran"),
            ("elapsed_ms", "how long it took, in milliseconds"),
            (
                "outcome",
                "`ok`, `refused` or `error` — a command that did not succeed \
                 still took time, and each is counted apart",
            ),
        ]
    }
}

/// Matches the column. Refused, not truncated: a cut verb splits its history.
const VERB_MAX: usize = 32;

pub async fn record(pool: &MySqlPool, session: &str, run: &Run) -> Result<()> {
    if run.verb.is_empty() || run.verb.len() > VERB_MAX {
        return Err(AppError::from(anyhow::anyhow!(
            "`{}` is not a verb this table can hold: 1 to {VERB_MAX} bytes",
            run.verb
        )));
    }
    sqlx::query(
        "INSERT INTO command_run (ran_at, verb, session, elapsed_ms, outcome, \
         waited_for_a_model) VALUES (NOW(), ?, ?, ?, ?, ?)",
    )
    .bind(&run.verb)
    .bind(session)
    .bind(run.elapsed_ms)
    .bind(run.outcome.as_str())
    .bind(run.waited_for_a_model)
    .execute(pool)
    .await
    .context("recording what a command did")?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ran {
    pub ran_at: DateTime<Utc>,
    pub verb: String,
    pub elapsed_ms: u32,
    pub outcome: Ended,
    /// `None` when the client didn't say.
    pub waited_for_a_model: Option<bool>,
}

/// The commands run in the last `days`, newest first. An unreadable outcome is
/// an error, not a skipped row, or the counts shrink quietly.
pub async fn recent(pool: &MySqlPool, days: u32) -> Result<Vec<Ran>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        ran_at: chrono::NaiveDateTime,
        verb: String,
        elapsed_ms: u32,
        outcome: String,
        waited_for_a_model: Option<bool>,
    }

    let rows: Vec<Row> = sqlx::query_as(
        "SELECT ran_at, verb, elapsed_ms, outcome, waited_for_a_model FROM command_run \
         WHERE ran_at > NOW() - INTERVAL ? DAY ORDER BY ran_at DESC",
    )
    .bind(days)
    .fetch_all(pool)
    .await
    .context("reading what the commands did")?;
    rows.into_iter()
        .map(|row| {
            Ok(Ran {
                ran_at: row.ran_at.and_utc(),
                verb: row.verb,
                elapsed_ms: row.elapsed_ms,
                outcome: Ended::read(&row.outcome).with_context(|| {
                    format!("`{}` is not an outcome this version knows", row.outcome)
                })?,
                waited_for_a_model: row.waited_for_a_model,
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
    pub verb: String,
    pub runs: usize,
    /// Went wrong (older clients' refusals land here too).
    pub failed: usize,
    /// The tool declined: a guard fired, or a check refused.
    pub refused: usize,
    /// Nearest-rank milliseconds over successes only, or the tool looks fastest on
    /// the day it refuses everything.
    pub median_ms: u32,
    pub p90_ms: u32,
    pub worst_ms: u32,
    /// The same over runs that didn't wait for a model: the service's own latency.
    /// Over both, a percentile mostly counts checked edits. `None` when no run said.
    pub unchecked_p90_ms: Option<u32>,
    /// Runs that waited for a model, and that didn't say; unknown is never folded into either.
    pub waited: usize,
    pub unknown: usize,
}

fn rank(sorted: &[u32], part: f64) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let at = ((sorted.len() as f64) * part).ceil() as usize;
    sorted[at.clamp(1, sorted.len()) - 1]
}

/// Per verb, most run first: the question is where sessions spend time.
pub fn tally(runs: &[Ran]) -> Vec<Tally> {
    let mut verbs: Vec<&str> = runs.iter().map(|r| r.verb.as_str()).collect();
    verbs.sort_unstable();
    verbs.dedup();
    let mut out: Vec<Tally> = verbs
        .into_iter()
        .map(|verb| {
            let mine: Vec<&Ran> = runs.iter().filter(|r| r.verb == verb).collect();
            let mut spent: Vec<u32> = mine
                .iter()
                .filter(|r| r.outcome == Ended::Ok)
                .map(|r| r.elapsed_ms)
                .collect();
            spent.sort_unstable();
            // Minus the runs that waited; runs that didn't say are excluded from both.
            let mut alone: Vec<u32> = mine
                .iter()
                .filter(|r| r.outcome == Ended::Ok && r.waited_for_a_model == Some(false))
                .map(|r| r.elapsed_ms)
                .collect();
            alone.sort_unstable();
            Tally {
                verb: verb.to_string(),
                runs: mine.len(),
                failed: mine.iter().filter(|r| r.outcome == Ended::Error).count(),
                refused: mine.iter().filter(|r| r.outcome == Ended::Refused).count(),
                median_ms: rank(&spent, 0.5),
                p90_ms: rank(&spent, 0.9),
                worst_ms: *spent.last().unwrap_or(&0),
                unchecked_p90_ms: (!alone.is_empty()).then(|| rank(&alone, 0.9)),
                waited: mine
                    .iter()
                    .filter(|r| r.waited_for_a_model == Some(true))
                    .count(),
                unknown: mine
                    .iter()
                    .filter(|r| r.waited_for_a_model.is_none())
                    .count(),
            }
        })
        .collect();
    // Then by name, so ties don't swap between readings.
    out.sort_by(|a, b| b.runs.cmp(&a.runs).then_with(|| a.verb.cmp(&b.verb)));
    out
}

/// How often a fresh point is pushed. Not what fleetwatch grades; that is
/// [`REPORTING_INTERVAL_S`].
const REPORT_EVERY: chrono::TimeDelta = chrono::TimeDelta::hours(1);

/// The cadence declared to fleetwatch, worked back from its bands (`Silent`, a
/// failure, past 3×): several days of nothing, so a quiet weekend isn't a fault.
/// Without a prober, silence can't be told from breakage.
pub const REPORTING_INTERVAL_S: u64 = 144_000;

/// Whether this caller carries the timings out, claiming the job if so. A
/// conditional UPDATE: two callers reading the same old stamp can't both match
/// it. A read then a write would hand the job to both.
pub async fn due_to_report(pool: &MySqlPool) -> Result<bool> {
    let claimed = sqlx::query(
        "UPDATE reported SET claimed_at = NOW() \
         WHERE what = 'timings' AND claimed_at < NOW() - INTERVAL ? SECOND",
    )
    .bind(REPORT_EVERY.num_seconds())
    .execute(pool)
    .await
    .context("claiming the reporting job")?
    .rows_affected();
    if claimed > 0 {
        return Ok(true);
    }
    // First ever call: `INSERT IGNORE`, so one racer wins.
    let created =
        sqlx::query("INSERT IGNORE INTO reported (what, claimed_at) VALUES ('timings', NOW())")
            .execute(pool)
            .await
            .context("opening the reporting claim")?
            .rows_affected();
    Ok(created > 0)
}
