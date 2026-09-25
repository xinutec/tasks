//! What the CLI did, recorded from real use rather than sampled by a probe.
//!
//! [`checks`](crate::tasks::checks) records the two model checks, which is the
//! expensive part of ONE command. This records every command: how long `list`,
//! `show`, `edit` and the rest actually took for the session that ran them.
//!
//! ⚠ **Real invocations only. There is no prober and there must not be one.**
//! A timer running a command on a cadence times what no session runs, from a
//! process with no session and a cold cache, and samples the clock rather than
//! the usage. What a session waits for is only visible from what sessions do.
//!
//! ⚠ **Recording must never cost the command anything.** The write goes out
//! after the answer is printed, and its failures are silent: a session that
//! cannot reach the service has a worse problem than a missing row.

use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;

use crate::error::AppError;
use crate::wire::RequiredKeys;

type Result<T> = std::result::Result<T, AppError>;

/// How a command ended.
///
/// ⚠ **Counted apart, never folded together.** A refusal usually returns
/// without a round trip, so a median over all outcomes reports the tool quicker
/// than any session experiences it; and a refusal counted as an `Error` makes
/// `add`, whose duplicate check refuses by design, look like a broken command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ended {
    Ok,
    /// The tool DECLINED — a guard fired, or a check refused. Not a fault.
    Refused,
    /// It could not. Something went wrong that nobody chose.
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

    /// ⚠ **An older client sends `error` for a refusal too**; those rows stay
    /// `Error`, since guessing would invent the split this exists to measure.
    fn read(word: &str) -> Option<Ended> {
        match word {
            "ok" => Some(Ended::Ok),
            "refused" => Some(Ended::Refused),
            "error" => Some(Ended::Error),
            _ => None,
        }
    }
}

/// The marker [`declined`] puts under a refusal's message.
///
/// ⚠ **A type, never a message match**, as in `checks::outcome`: rewording a
/// line a caller prints must not silently reclassify a month of runs.
#[derive(Debug)]
pub struct Refused;

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the tool declined")
    }
}

impl std::error::Error for Refused {}

/// An error that says the tool declined, carrying the same words as before.
///
/// The context IS the message a caller reads, so nothing about the output
/// changes; the marker rides underneath it where only the classifier looks.
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
/// ⚠⚠ **A REFUSAL PRINTS ITS SENTENCE AND NOTHING UNDER IT, BECAUSE SESSIONS
/// READ THE LAST THREE LINES.** anyhow's default rendering prints the whole
/// chain, so a one-line refusal would end in a blank, `Caused by:` and `the
/// tool declined` — and piped to `tail -3`, that reads as the whole answer with
/// the remedy cut off.
///
/// ⚠ **The chain STAYS for anything that actually went wrong**: a transport
/// failure is diagnosed from its causes.
pub fn said(why: &anyhow::Error) -> String {
    match was_refused(why) {
        true => format!("{why}"),
        false => format!("{why:?}"),
    }
}

/// One command, as the CLI reports it.
///
/// The clock is the service's and the session the credential's, as for
/// [`checks::Run`](crate::tasks::checks::Run): a client is trusted for how long
/// it took, not for when it happened.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    /// `list`, `show`, `add`, … from the CLI's own command enum.
    pub verb: String,
    pub elapsed_ms: u32,
    pub outcome: Ended,
    /// Whether this invocation waited for a model check.
    ///
    /// ⚠ **This explains the `edit` distribution.** A checked edit waits on a
    /// model, an unchecked one does not, and the percentile over both moves when
    /// the check rate moves AND when the model slows down.
    ///
    /// Absent from an older client, and stored as NULL rather than `false`: not
    /// knowing is a third answer.
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

/// The longest a `verb` may be, matching the column.
///
/// ⚠ **Refused rather than truncated.** `verb` is the trend key, and a value
/// cut to fit would split one command's history in two.
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
    /// Whether it waited for a model — `None` when the client did not say.
    pub waited_for_a_model: Option<bool>,
}

/// The commands run in the last `days`, newest first.
///
/// ⚠ **An outcome this module cannot read is an error, not a skipped row**, as
/// in `checks::recent`: dropping it would quietly shrink the counts.
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
    /// Went wrong — not a refusal, see [`Ended`]. An older client's refusals
    /// count here, since it could not tell them apart.
    pub failed: usize,
    /// The tool declined: a guard fired, or a check refused.
    pub refused: usize,
    /// Milliseconds, by nearest rank over the runs that SUCCEEDED.
    ///
    /// ⚠ **Successes only**, or the tool would look fastest on the day it started
    /// refusing everything.
    pub median_ms: u32,
    pub p90_ms: u32,
    pub worst_ms: u32,
    /// The same percentiles over only the runs that did NOT wait for a model.
    ///
    /// ⚠ **This is the service's latency; the fields above are the mix.** A
    /// model call dwarfs the whole request, so over both a percentile mostly
    /// reports how many edits were checked, and a service regression hides.
    ///
    /// `None` when no run in the window said either way — never the mix, which
    /// it exists to correct.
    pub unchecked_p90_ms: Option<u32>,
    /// How many runs waited for a model, and how many said nothing.
    ///
    /// ⚠ **`unknown` is carried, not folded into either side**: counting it as
    /// unchecked would file slow edits into the fast population.
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

/// Per verb, busiest first.
///
/// ⚠ **Ordered by how often a command is RUN, not how slow**: the question is
/// what sessions spend their time on.
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
            // The same population, minus the runs that waited for a model. A run
            // that said nothing is excluded from BOTH sides rather than assumed
            // fast — see `unknown`.
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
    // Then by name, so equally busy verbs do not swap places between readings.
    out.sort_by(|a, b| b.runs.cmp(&a.runs).then_with(|| a.verb.cmp(&b.verb)));
    out
}

/// How long a caller waits before another is handed the reporting job.
///
/// ⚠ **This is the PUSH window, not what fleetwatch grades.** This is how often
/// a fresh point lands; [`REPORTING_INTERVAL_S`] is how long silence is
/// tolerated before it is a fault.
const REPORT_EVERY: chrono::TimeDelta = chrono::TimeDelta::hours(1);

/// The cadence the report declares to fleetwatch, in seconds.
///
/// ⚠ **Worked back from fleetwatch's bands, not chosen.** It grades a report
/// `Fresh` within 1.5× this, `Overdue` to 3×, and `Silent` — a FAILURE —
/// beyond; this puts `Silent` at several days of nothing, so a quiet weekend is
/// not a fault.
///
/// ⚠ **Silence means NOBODY USED THE TRACKER**, which without a prober cannot be
/// told from it being broken. Firing only after days makes that tolerable.
pub const REPORTING_INTERVAL_S: u64 = 144_000;

/// Whether this caller is the one to carry the timings out, claiming the job if
/// so.
///
/// ⚠ **A conditional UPDATE, and the condition is the whole point.** Two
/// sessions asking in the same second both read the same old stamp; only one
/// can match it in the `WHERE`, so only one gets `true`. Doing this as a read
/// followed by a write would hand the job to both.
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
    // The first ever call: no row to update. `INSERT IGNORE`, so of two callers
    // racing to create it only one wins.
    let created =
        sqlx::query("INSERT IGNORE INTO reported (what, claimed_at) VALUES ('timings', NOW())")
            .execute(pool)
            .await
            .context("opening the reporting claim")?
            .rows_affected();
    Ok(created > 0)
}
