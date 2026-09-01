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
use chrono::{DateTime, Utc};
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
    pub fn as_str(self) -> &'static str {
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
    /// What was refused, hashed — see [`subject_key`].
    ///
    /// ⚠ **Only a filing check that REFUSED sets this**, and it is what licenses
    /// a later `--no-duplicate-check` for the same subject. A density read has
    /// no subject and a check that passed has nothing to license.
    #[serde(default)]
    pub subject_key: Option<String>,
    /// What a density read said, verbatim, so it outlives the tool result.
    ///
    /// ⚠ **Sent on the run that is already being recorded, not by a second
    /// call.** The alternative was a `PATCH /api/tasks/{id}`, which would put a
    /// write to a task on the failure path of a check that is explicitly allowed
    /// to fail silently — `accreting()` swallows everything, because the edit
    /// has already landed and a session must not pay attention to the checker.
    ///
    /// Absent on a filing check, which has no body to be about, and absent on a
    /// density read that was quiet — which is not the same as absent, and is
    /// handled by [`record`] as the clear it is.
    #[serde(default)]
    pub said: Option<String>,
}

/// The key a refusal is remembered by.
///
/// ⚠ **Case and surrounding space are ignored, exactly as [`same_subject`]
/// ignores them**, or the override would refuse to honour a re-run that differs
/// from the refused filing only in whitespace — which is the same command typed
/// again, and the whole point.
///
/// [`same_subject`]: crate::tasks::duplicates::same_subject
pub fn subject_key(subject: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(subject.trim().to_lowercase().as_bytes());
    hex::encode(hash.finalize())
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
         (ran_at, kind, session, task_id, input_chars, accreted, elapsed_ms, outcome, subject_key) \
         VALUES (NOW(), ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(run.kind.as_str())
    .bind(session)
    .bind(run.task_id)
    .bind(run.input_chars)
    .bind(run.accreted)
    .bind(run.elapsed_ms)
    .bind(run.outcome.as_str())
    .bind(run.subject_key.as_deref())
    .execute(pool)
    .await
    .context("recording what a check did")?;
    remember(pool, run).await
}

/// Keep what a density read said, on the task it was about.
///
/// ⚠ **Only `spoke` and `quiet` touch the flag; a timeout or an error must
/// not.** Those two mean the body was never judged — 37 of 268 reads timed out
/// over the 5.6 days to 2026-08-29 — and clearing on them would let a slow model
/// retire a finding nobody has addressed. Silence from a checker that never ran
/// is not a verdict, which is the distinction [`Outcome`] exists to preserve.
async fn remember(pool: &MySqlPool, run: &Run) -> Result<()> {
    let (Kind::Density, Some(id)) = (run.kind, run.task_id) else {
        return Ok(());
    };
    match run.outcome {
        // A later read replaces an earlier one: this is the last thing said
        // about the body, not a log of opinions. `check_run` keeps the log.
        Outcome::Spoke => {
            sqlx::query("UPDATE tasks SET sprawl_said = ?, sprawl_chars = ? WHERE id = ?")
                .bind(run.said.as_deref())
                .bind(run.input_chars)
                .bind(id)
                .execute(pool)
                .await
                .context("keeping what a density read said")?;
        }
        // `DENSE` is a verdict about the body as it stands now, and a later
        // verdict outranks an earlier one. It cannot be summoned to clear a
        // flag: the read only fires on 3,000 characters of fresh accretion, so
        // the cheapest way to reach it is to make the body worse.
        Outcome::Quiet => {
            sqlx::query("UPDATE tasks SET sprawl_said = NULL, sprawl_chars = NULL WHERE id = ?")
                .bind(id)
                .execute(pool)
                .await
                .context("clearing what a density read said")?;
        }
        Outcome::Timeout | Outcome::Error => {}
    }
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

/// One recorded run, as it comes back out.
///
/// The same fields as [`Run`] plus the service's clock. Two shapes rather than
/// one optional field: a client reports what it did and never when, so a struct
/// that could carry a time on the way in is one somebody will eventually fill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ran {
    pub ran_at: DateTime<Utc>,
    pub kind: Kind,
    pub task_id: Option<u64>,
    pub input_chars: u32,
    pub accreted: Option<u32>,
    pub elapsed_ms: u32,
    pub outcome: Outcome,
}

impl Kind {
    fn read(word: &str) -> Option<Kind> {
        match word {
            "filing" => Some(Kind::Filing),
            "density" => Some(Kind::Density),
            _ => None,
        }
    }
}

impl Outcome {
    fn read(word: &str) -> Option<Outcome> {
        match word {
            "quiet" => Some(Outcome::Quiet),
            "spoke" => Some(Outcome::Spoke),
            "timeout" => Some(Outcome::Timeout),
            "error" => Some(Outcome::Error),
            _ => None,
        }
    }
}

/// Every run in the last `days`, newest first.
///
/// ⚠ **A word this module cannot read is an error, not a skipped row.** The
/// only writer is this module, so an unknown `kind` means a newer version wrote
/// the table — and dropping the row would quietly shrink exactly the counts
/// somebody is reading the table to get.
pub async fn recent(pool: &MySqlPool, days: u32) -> Result<Vec<Ran>> {
    /// A row as the table holds it: the two words are strings there, and
    /// reading them back into the enums is what this function is for.
    #[derive(sqlx::FromRow)]
    struct Row {
        ran_at: chrono::NaiveDateTime,
        kind: String,
        task_id: Option<u64>,
        input_chars: u32,
        accreted: Option<u32>,
        elapsed_ms: u32,
        outcome: String,
    }

    let rows: Vec<Row> = sqlx::query_as(
        "SELECT ran_at, kind, task_id, input_chars, accreted, elapsed_ms, outcome \
         FROM check_run WHERE ran_at > NOW() - INTERVAL ? DAY ORDER BY ran_at DESC",
    )
    .bind(days)
    .fetch_all(pool)
    .await
    .context("reading what the checks did")?;
    rows.into_iter()
        .map(|row| {
            Ok(Ran {
                ran_at: row.ran_at.and_utc(),
                kind: Kind::read(&row.kind)
                    .with_context(|| format!("`{}` is not a check this version knows", row.kind))?,
                task_id: row.task_id,
                input_chars: row.input_chars,
                accreted: row.accreted,
                elapsed_ms: row.elapsed_ms,
                outcome: Outcome::read(&row.outcome).with_context(|| {
                    format!("`{}` is not an outcome this version knows", row.outcome)
                })?,
            })
        })
        .collect()
}

/// What one kind of check did over the period.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
    pub kind: Kind,
    pub runs: usize,
    /// Counted separately rather than summed into "ran" and "did not": the
    /// difference between them is the finding.
    pub quiet: usize,
    pub spoke: usize,
    pub timeout: usize,
    pub error: usize,
    /// Milliseconds, by nearest rank over every run including the abandoned
    /// ones — a timeout took the whole patience and pretending otherwise would
    /// make the bound look comfortable.
    pub median_ms: u32,
    pub p90_ms: u32,
    pub worst_ms: u32,
}

/// Nearest rank, on a slice that is already sorted.
fn rank(sorted: &[u32], part: f64) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let at = ((sorted.len() as f64) * part).ceil() as usize;
    sorted[at.clamp(1, sorted.len()) - 1]
}

/// Fold runs into one line per kind, in the order the kinds are declared.
pub fn tally(runs: &[Ran]) -> Vec<Tally> {
    [Kind::Filing, Kind::Density]
        .into_iter()
        .filter_map(|kind| {
            let mine: Vec<&Ran> = runs.iter().filter(|r| r.kind == kind).collect();
            if mine.is_empty() {
                return None;
            }
            let mut spent: Vec<u32> = mine.iter().map(|r| r.elapsed_ms).collect();
            spent.sort_unstable();
            let count = |what: Outcome| mine.iter().filter(|r| r.outcome == what).count();
            Some(Tally {
                kind,
                runs: mine.len(),
                quiet: count(Outcome::Quiet),
                spoke: count(Outcome::Spoke),
                timeout: count(Outcome::Timeout),
                error: count(Outcome::Error),
                median_ms: rank(&spent, 0.5),
                p90_ms: rank(&spent, 0.9),
                worst_ms: *spent.last().unwrap_or(&0),
            })
        })
        .collect()
}

/// How long a refusal licenses an override for.
///
/// ⚠ **Long enough to re-run, short enough not to become the habit.** The
/// intended sequence is: the filing is refused, the caller reads it, and re-runs
/// the same command with `--no-duplicate-check` — seconds, sometimes a minute if
/// they open the task it named first. Half an hour is generous for that and far
/// too short to let a session collect a licence in the morning and skip checks
/// all afternoon.
const LICENCE: i64 = 1800;

/// Whether this session has been refused this exact subject, recently.
///
/// ⚠ **This is what makes `--no-duplicate-check` cost a re-run.** Measured over
/// every transcript, 63 of 644 filings passed that flag on the way in and only
/// 16 followed a refusal — so for 47 sessions the check never ran at all, and
/// the trade the whole module rests on never happened. The flag stays, because
/// roughly one refusal in six is wrong and removing the escape would turn every
/// false positive into a lost body; it just cannot be used FIRST any more.
///
/// Keyed on the subject, not merely the session: one refusal licenses re-filing
/// the thing that was refused, and nothing else.
pub async fn refused_recently(pool: &MySqlPool, session: &str, subject: &str) -> Result<bool> {
    let key = subject_key(subject);
    let found: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM check_run \
         WHERE session = ? AND subject_key = ? AND outcome = 'spoke' AND kind = 'filing' \
           AND ran_at > NOW() - INTERVAL ? SECOND LIMIT 1",
    )
    .bind(session)
    .bind(&key)
    .bind(LICENCE)
    .fetch_optional(pool)
    .await
    .context("looking for a refusal that would licence this filing")?;
    Ok(found.is_some())
}

/// What a filing is told when it skipped the check without having been refused.
///
/// ⚠ **It must not read as a bug.** The caller passed a documented flag and got
/// a refusal from the service, which is confusing unless the message says
/// plainly what to do — and what to do is the easy thing: drop the flag.
pub fn unlicensed() -> String {
    "NOT FILED — --no-duplicate-check is for overruling a refusal you have already seen, \
     and nothing has refused this one. Re-run without it. If the check then names something \
     that is genuinely different work, re-run the same command WITH the flag and it will file."
        .to_string()
}
