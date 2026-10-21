//! What the model checks did, so their rate and latency are facts: each runs a
//! one-shot session and deletes its transcript, leaving nothing else behind.
//! Written on every path, failures included: a check that did not run is the
//! finding.

use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{MySqlConnection, MySqlPool};

use crate::error::AppError;
use crate::tasks::repo::parse_ids;
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
    /// An open task with exactly this subject: no model; recorded so its refusal licenses an override.
    Collision,
}

/// What came of it. ⚠ `Quiet` and `Timeout` look alike from outside: one read
/// the input and found nothing, the other never answered. Counted together they
/// report a well-behaved tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// It ran and said nothing.
    Quiet,
    /// It named a duplicate, or gave advice on a body.
    Spoke,
    /// No answer within the caller's patience.
    Timeout,
    /// Couldn't be asked, or the answer was unreadable.
    Error,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Filing => "filing",
            Kind::Density => "density",
            Kind::Collision => "collision",
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

/// One run as the CLI reports it; the service supplies session and clock.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub kind: Kind,
    /// The task whose body was read. Absent on a filing check.
    #[serde(default)]
    pub task_id: Option<u64>,
    pub input_chars: u32,
    /// What crossed the sampler, on a density read.
    #[serde(default)]
    pub accreted: Option<u32>,
    pub elapsed_ms: u32,
    pub outcome: Outcome,
    /// The refused subject, hashed ([`subject_key`]): set only by a refusal, and
    /// what licenses a later `--not-duplicate-of` for that subject.
    #[serde(default)]
    pub subject_key: Option<String>,
    /// The tasks a filing check named. Empty on every other run.
    #[serde(default)]
    pub matched: Vec<u64>,
    /// What a density read said, verbatim, sent with the run (not a second call
    /// that could fail). Absent on a quiet read, which [`record`] takes as clearing
    /// the finding.
    #[serde(default)]
    pub said: Option<String>,
}

/// The key a refusal is remembered by, ignoring case and surrounding space as
/// [`same_subject`] does, so a re-typed re-run is still licensed.
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
            ("kind", "`filing`, `density` or `collision`"),
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

pub async fn record(pool: &MySqlPool, session: &str, run: &Run) -> Result<()> {
    let mut tx = pool.begin().await.context("recording what a check did")?;
    let id = sqlx::query(
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
    .execute(&mut *tx)
    .await
    .context("recording what a check did")?
    .last_insert_id();
    for task in &run.matched {
        sqlx::query("INSERT IGNORE INTO check_match (run_id, task_id) VALUES (?, ?)")
            .bind(id)
            .bind(task)
            .execute(&mut *tx)
            .await
            .context("recording what a check named")?;
    }
    remember(&mut tx, run).await?;
    tx.commit().await.context("recording what a check did")?;
    Ok(())
}

/// Keep what a density read said on its task. ⚠ Only `spoke` and `quiet` touch
/// it: a timeout or error never judged the body, and clearing on one would let a
/// slow model retire an unaddressed finding.
async fn remember(conn: &mut MySqlConnection, run: &Run) -> Result<()> {
    let (Kind::Density, Some(id)) = (run.kind, run.task_id) else {
        return Ok(());
    };
    match run.outcome {
        // The latest read replaces the last; `check_run` keeps the log.
        Outcome::Spoke => {
            sqlx::query("UPDATE tasks SET sprawl_said = ?, sprawl_chars = ? WHERE id = ?")
                .bind(run.said.as_deref())
                .bind(run.input_chars)
                .bind(id)
                .execute(&mut *conn)
                .await
                .context("keeping what a density read said")?;
        }
        // `DENSE` clears it. It can't be summoned: the read fires only on fresh
        // growth.
        Outcome::Quiet => {
            sqlx::query("UPDATE tasks SET sprawl_said = NULL, sprawl_chars = NULL WHERE id = ?")
                .bind(id)
                .execute(&mut *conn)
                .await
                .context("clearing what a density read said")?;
        }
        Outcome::Timeout | Outcome::Error => {}
    }
    Ok(())
}

/// What to record for an answer. Timeouts are told from quiet checks by the
/// error chain, not its message. `spoke` is the caller's call: the same answer
/// is a refusal on one path and advice on the other.
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

/// A recorded run with the service's clock: a separate type, so nothing can send a time in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ran {
    pub ran_at: DateTime<Utc>,
    pub kind: Kind,
    pub task_id: Option<u64>,
    pub input_chars: u32,
    pub accreted: Option<u32>,
    pub elapsed_ms: u32,
    pub outcome: Outcome,
    /// What a filing check named; see [`Run::matched`].
    #[serde(default)]
    pub matched: Vec<u64>,
}

impl Kind {
    fn read(word: &str) -> Option<Kind> {
        match word {
            "filing" => Some(Kind::Filing),
            "collision" => Some(Kind::Collision),
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

/// Every run in the last `days`, newest first. An unknown word means a newer
/// version wrote it: an error, not a skipped row.
pub async fn recent(pool: &MySqlPool, days: u32) -> Result<Vec<Ran>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        ran_at: chrono::NaiveDateTime,
        kind: String,
        task_id: Option<u64>,
        input_chars: u32,
        accreted: Option<u32>,
        elapsed_ms: u32,
        outcome: String,
        matched: Option<String>,
    }

    let rows: Vec<Row> = sqlx::query_as(
        "SELECT r.ran_at, r.kind, r.task_id, r.input_chars, r.accreted, r.elapsed_ms, r.outcome, \
           (SELECT GROUP_CONCAT(m.task_id ORDER BY m.task_id) FROM check_match m \
            WHERE m.run_id = r.id) AS matched \
         FROM check_run r WHERE r.ran_at > NOW() - INTERVAL ? DAY ORDER BY r.ran_at DESC",
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
                matched: parse_ids(row.matched.as_deref()),
            })
        })
        .collect()
}

/// What one kind of check did over the period.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
    pub kind: Kind,
    pub runs: usize,
    /// Separate counts: the difference between them is the finding.
    pub quiet: usize,
    pub spoke: usize,
    pub timeout: usize,
    pub error: usize,
    /// Nearest rank over every run, timeouts included: they took the whole patience.
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

/// Fold runs into one line per model check, in the order the kinds are
/// declared. A collision asks no model, so it has no latency to report.
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

/// Long enough to read a refusal and re-run; too short to skip checks all afternoon.
const LICENCE: i64 = 1800;

/// What this session's latest refusal of `subject` named, if within
/// `LICENCE`: the tasks an override must name. Per subject, never a licence
/// for the next filing.
pub async fn licence(pool: &MySqlPool, session: &str, subject: &str) -> Result<Option<Vec<u64>>> {
    let key = subject_key(subject);
    let found: Option<(u64, Option<String>)> = sqlx::query_as(
        "SELECT r.id, (SELECT GROUP_CONCAT(m.task_id ORDER BY m.task_id) FROM check_match m \
                       WHERE m.run_id = r.id) \
         FROM check_run r \
         WHERE r.session = ? AND r.subject_key = ? AND r.outcome = 'spoke' \
           AND r.kind IN ('filing', 'collision') \
           AND r.ran_at > NOW() - INTERVAL ? SECOND \
         ORDER BY r.id DESC LIMIT 1",
    )
    .bind(session)
    .bind(&key)
    .bind(LICENCE)
    .fetch_optional(pool)
    .await
    .context("looking for a refusal that would licence this filing")?;
    Ok(found.map(|(_, named)| parse_ids(named.as_deref())))
}

/// `GET /api/checks/licence`: `None` when nothing refused this subject, so the check runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Licence {
    pub matched: Option<Vec<u64>>,
}

/// When an override leaves out a task the refusal named.
pub fn unnamed(missing: &[u64]) -> String {
    let ids: Vec<String> = missing.iter().map(|id| format!("#{id}")).collect();
    format!(
        "NOT FILED — the refusal of this subject named {}, and this override does not. \
         An override is a judgment about each task the refusal named: add {} to \
         --not-duplicate-of if each really is different work.",
        ids.join(" and "),
        missing
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(","),
    )
}

/// Per session: refusals, and filings past one. Overruling every refusal is not judging them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Overrules {
    pub session: String,
    pub refused: u64,
    pub overruled: u64,
}

pub async fn overrules(pool: &MySqlPool, days: u32) -> Result<Vec<Overrules>> {
    let rows: Vec<(String, Option<String>, i64, i64)> = sqlx::query_as(
        "SELECT x.session, s.name, CAST(SUM(x.refused) AS SIGNED), CAST(SUM(x.overruled) AS SIGNED) FROM ( \
           SELECT session, 1 AS refused, 0 AS overruled FROM check_run \
            WHERE outcome = 'spoke' AND kind IN ('filing', 'collision') \
              AND ran_at > NOW() - INTERVAL ? DAY \
           UNION ALL \
           SELECT actor_id, 0, 1 FROM task_events \
            WHERE kind = 'overrode' AND actor_kind = 'session' \
              AND at > NOW() - INTERVAL ? DAY \
         ) x LEFT JOIN sessions s ON s.id = x.session \
         GROUP BY x.session, s.name ORDER BY SUM(x.overruled) DESC, SUM(x.refused) DESC",
    )
    .bind(days)
    .bind(days)
    .fetch_all(pool)
    .await
    .context("counting refusals and overrides")?;
    Ok(rows
        .into_iter()
        .map(|(id, name, refused, overruled)| Overrules {
            session: name.filter(|n| !n.is_empty()).unwrap_or(id),
            refused: refused.max(0) as u64,
            overruled: overruled.max(0) as u64,
        })
        .collect())
}

/// An override with no refusal behind it. Says plainly what to do: the flag is documented.
pub fn unlicensed() -> String {
    "NOT FILED — nothing has refused this subject in this conversation, so there is \
     nothing to overrule. Re-run without --not-duplicate-of: the check runs, and if it \
     names something that is genuinely different work, re-run with \
     --not-duplicate-of <the ids it named>."
        .to_string()
}
