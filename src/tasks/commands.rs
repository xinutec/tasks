//! What the CLI did, recorded from real use rather than sampled by a probe.
//!
//! [`checks`](crate::tasks::checks) records the two model checks, which is the
//! expensive part of ONE command. This records every command: how long `list`,
//! `show`, `edit` and the rest actually took for the session that ran them.
//!
//! ⚠ **Real invocations only. There is no prober and there must not be one.**
//! The first version was a timer running `task list --all` on a fixed cadence.
//! Two things were wrong and the shape is tempting enough to keep written down:
//! it timed a command **no session runs**, from a process with no session id and
//! a cold cache, so the numbers described the probe; and a fixed cadence samples
//! the clock rather than the usage. What a session waits for is only visible
//! from what sessions do.
//!
//! ⚠ **Recording must never cost the command anything.** The write goes out
//! after the work is finished and its answer is already printed, and every
//! failure of it is silent: a session that cannot reach the service has a worse
//! problem than a missing row, and a tracker that got slower to *use* because it
//! was measuring how slow it was to use would be the funniest possible outcome.

use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;

use crate::error::AppError;
use crate::wire::RequiredKeys;

type Result<T> = std::result::Result<T, AppError>;

/// ⚠ **Counted apart, never folded together.** The error path is usually the
/// fast one — a refusal prints and returns without a round trip — so a median
/// over both reports the tool as quicker than any session experiences it.
///
/// ⚠ **Folding a refusal into `Error` makes the failure rate unreadable.** `add`
/// then looks like a broken command, when most of what it counts is either the
/// CLI declining a malformed invocation — returning before any round trip — or
/// the duplicate check refusing. Both are the tool working, and one number
/// carrying both findings shows neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ended {
    Ok,
    /// The tool DECLINED — a guard fired, or a check refused. Not a fault: this
    /// is the tool doing its job, and counting it as breakage hides both.
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

    /// ⚠ **An older client sends `error` for both**, and those rows stay
    /// `Error` rather than being re-attributed: nothing recorded which they
    /// were, and guessing would invent the split this exists to measure.
    fn read(word: &str) -> Option<Ended> {
        match word {
            "ok" => Some(Ended::Ok),
            "refused" => Some(Ended::Refused),
            "error" => Some(Ended::Error),
            _ => None,
        }
    }
}

/// ⚠ **A type, never a message match.** `checks::outcome` already makes this
/// argument for timeouts: it turns on the error's chain so that rewording a line
/// a caller prints cannot silently reclassify a month of runs. The same holds
/// here, and harder — these messages are long, they get edited, and the
/// formatter rewrites the text anyone would have matched on.
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
/// READ THE LAST THREE LINES.** `declined` puts the message in the context and
/// the `Refused` marker underneath, where only [`ended`] looks — but anyhow's
/// default rendering prints the whole chain, so a one-line refusal came out as
/// four lines whose last three were a blank, `Caused by:` and `the tool
/// declined`. Piped to `tail -3` that is the trailer with the cause cut off.
///
/// ⚠ **A truncated error does not read as truncated; it reads as the whole
/// answer**, and a session builds an explanation on it. One read `the tool
/// declined` three times through `tail -3`, concluded the permission layer had
/// tightened, said so, and stopped retrying — while the line above the cut named
/// the remedy outright.
///
/// ⚠ **The chain STAYS for anything that actually went wrong.** A transport
/// failure is diagnosed from its causes, so this is a branch and not a blanket
/// `{}`. Same argument as [`Refused`] being a type: the classifier's marker was
/// never a message.
pub fn said(why: &anyhow::Error) -> String {
    match was_refused(why) {
        true => format!("{why}"),
        false => format!("{why:?}"),
    }
}

///
/// The clock and the session are the service's and the caller's respectively,
/// for the same reason as [`checks::Run`](crate::tasks::checks::Run): a client
/// that timed itself is trusted for the duration, and not for when it happened.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    /// `list`, `show`, `add`, … from the CLI's own command enum.
    pub verb: String,
    pub elapsed_ms: u32,
    pub outcome: Ended,
    /// Whether this invocation waited for a model check.
    ///
    /// ⚠ **The variable that explains the whole `edit` distribution.** A checked
    /// edit waits on a model and an unchecked one does not, while the service's
    /// own share is the same either way. Without this the reported percentile is
    /// the MIX, which moves when the check rate moves AND when the model slows
    /// down, and cannot say which happened.
    ///
    /// Absent from an older client, and stored as NULL rather than `false`: not
    /// knowing is a third answer, and folding it into the fast population would
    /// invent the number this exists to measure.
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
                "`ok` or `error` — a command that failed still took time, and \
                 the two are counted apart",
            ),
        ]
    }
}

/// The longest a `verb` may be, matching the column.
///
/// ⚠ **Refused rather than truncated.** The column is `VARCHAR(32)` and `verb`
/// is the trend key: a longer value silently cut to fit would split one
/// command's history at the point somebody added a subcommand with a long name,
/// and the break would look like the command had never run before.
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
    /// Whether it waited for a model — `None` on rows written before `0015`.
    pub waited_for_a_model: Option<bool>,
}

///
/// ⚠ **An outcome this module cannot read is an error, not a skipped row** —
/// the same rule as `checks::recent`, and for the same reason: dropping it would
/// quietly shrink exactly the counts somebody is reading the table to get.
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
    /// Went wrong. ⚠ **No longer includes a refusal** — see [`Ended`]. Rows
    /// written before the split could not tell them apart and are all counted
    /// here, so this figure falls as they age out rather than because anything
    /// improved.
    pub failed: usize,
    /// The tool declined: a guard fired, or a check refused.
    pub refused: usize,
    /// Milliseconds, by nearest rank over the runs that SUCCEEDED.
    ///
    /// ⚠ **Successes only, and this is the one place the two are not summed.**
    /// A refusal returns without a round trip, so folding the error path in
    /// pulls every percentile down — the tool would look fastest on the day it
    /// started refusing everything. `failed` carries the other half beside it.
    pub median_ms: u32,
    pub p90_ms: u32,
    pub worst_ms: u32,
    /// The same percentiles over only the runs that did NOT wait for a model.
    ///
    /// ⚠ **This is the service's latency; the fields above are the mix.** A
    /// checked edit spends orders of magnitude more time in the model than the
    /// service spends on the whole request, so a percentile over both reports
    /// what fraction of edits crossed the sampler, expressed in milliseconds — a
    /// real service regression hides underneath a far larger term.
    ///
    /// `None` when no run in the window said either way. Absent rather than
    /// equal to the mix: a figure that silently falls back to the number it is
    /// meant to correct looks like the fix working.
    pub unchecked_p90_ms: Option<u32>,
    /// How many runs waited for a model, and how many said nothing.
    ///
    /// ⚠ **`unknown` is carried rather than folded into either side.** Older
    /// rows know nothing, and counting them as unchecked files slow edits into
    /// the fast population — inventing the number this exists to measure.
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

/// ⚠ **Ordered by how often a command is RUN, not by how slow it is.** The
/// question this answers is what sessions spend their time on, and a rarely-used
/// command with a bad worst case sorts above `list` on latency while costing
/// nobody anything.
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
    // Busiest first, then by name so two verbs run equally often do not swap
    // places between readings and make a diff of two days unreadable.
    out.sort_by(|a, b| b.runs.cmp(&a.runs).then_with(|| a.verb.cmp(&b.verb)));
    out
}

/// How long a caller waits before another is handed the reporting job.
///
/// ⚠ **This is the PUSH window, and it is not what fleetwatch grades.** The
/// staleness bands come from the `interval_s` the report declares — see
/// [`REPORTING_INTERVAL_S`] — and the two answer different questions: this is
/// how often a fresh point lands, that is how long silence is tolerated before
/// it is a fault. Declare one and run at the other and a dead collector goes
/// unreported for as long as the gap between them.
const REPORT_EVERY: chrono::TimeDelta = chrono::TimeDelta::hours(1);

/// The cadence the report declares to fleetwatch, in seconds.
///
/// ⚠ **Worked back from fleetwatch's own bands, not chosen.** It grades a report
/// `Fresh` within 1.5× this, `Overdue` to 3×, and `Silent` — a FAILURE — beyond.
/// The requirement was that several days of nothing is a problem and anything
/// short of that is not, so this is whatever makes 3× land there: a quiet night
/// and weekend stay `Fresh`, the gap after is a warning, and the failure is
/// where somebody actually wants it.
///
/// ⚠ **Silence here means NOBODY USED THE TRACKER, which is not the same as the
/// tracker being broken**, and with no prober the two cannot be told apart. That
/// is the accepted cost of measuring real use instead of polling: the number
/// above is what makes the conflation tolerable, by only firing when the silence
/// is long enough to be worth a look either way.
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
    // The first ever call: no row to update. `INSERT IGNORE` so two callers
    // racing to create it do not both win — the loser's insert is a no-op and
    // it correctly reports that it has nothing to do.
    let created =
        sqlx::query("INSERT IGNORE INTO reported (what, claimed_at) VALUES ('timings', NOW())")
            .execute(pool)
            .await
            .context("opening the reporting claim")?
            .rows_affected();
    Ok(created > 0)
}
