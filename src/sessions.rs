//! The conversations work can be handed to. The session id is the identity
//! and everything points at it; the name is a cached attribute the CLI reads
//! from the transcript ([`crate::agent_name`]) and sends on every request. A
//! session's first request creates its row: there is no enrolment step.

use anyhow::Context;
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Serialize;
use sqlx::MySqlPool;

use crate::error::AppError;
use crate::still_open;

type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, Clone, Serialize)]
pub struct Session {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    /// Open tasks held, counted in the same query.
    pub open: i64,
}

#[derive(sqlx::FromRow)]
struct Row {
    id: String,
    name: Option<String>,
    first_seen: NaiveDateTime,
    last_seen: NaiveDateTime,
    open: i64,
}

/// Record that a session exists, and its name.
///
/// An absent or empty name keeps the stored one (the prompt hook sends only
/// the id). ⚠ A non-UUID id equal to another conversation's name is refused, or
/// it would capture every `--to <name>` meant for that one.
pub async fn touch(pool: &MySqlPool, id: &str, name: Option<&str>) -> Result<()> {
    let name = name.map(str::trim).filter(|n| !n.is_empty());
    if !is_uuid(id) {
        let named: Option<(String,)> =
            sqlx::query_as("SELECT id FROM sessions WHERE name = ? AND id <> ? LIMIT 1")
                .bind(id)
                .bind(id)
                .fetch_optional(pool)
                .await
                .context("checking a session id against the names")?;
        if let Some((other,)) = named {
            return Err(AppError::BadRequest(format!(
                "`{id}` is the name of conversation {other}, not an id. A session id is \
                 the conversation's own, `$CLAUDE_CODE_SESSION_ID`: drop `--session` or \
                 `TASKS_SESSION`, or pass {other}. Nothing was recorded."
            )));
        }
    }
    sqlx::query(
        "INSERT INTO sessions (id, name) VALUES (?, ?) \
         ON DUPLICATE KEY UPDATE name = COALESCE(VALUES(name), name), last_seen = NOW()",
    )
    .bind(id)
    .bind(name)
    .execute(pool)
    .await
    .context("recording a session")?;
    Ok(())
}

/// One party's share of the work. `total` adds what they finished, so `0/56`
/// reads as a cleared plate, not an idle one. Dropped tasks count in neither.
#[derive(Debug, Clone, Serialize)]
pub struct Holder {
    /// `session`, `person` or `nobody`, as for an assignee.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// A session may have none; the client then shows the id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub open: i64,
    /// Open plus finished, counted by the same query as `open`.
    pub total: i64,
}

/// Count a group of tasks as `open` and `done`, each named positively: "all
/// but dropped" would miscount when a status is added. The CAST is because
/// `SUM` comes back DECIMAL, which sqlx won't decode into `i64`.
macro_rules! tally {
    ($extra:literal, $tail:literal) => {
        concat!(
            "SELECT ",
            $extra,
            "CAST(COALESCE(SUM(",
            still_open!("t.status"),
            "), 0) AS SIGNED) AS open, ",
            "CAST(COALESCE(SUM(t.status = 'done'), 0) AS SIGNED) AS done ",
            $tail
        )
    };
}

/// Who holds what: every session ever given a task, the person, and the pile,
/// most loaded first. Sessions never given one are left out (nearly all of
/// them); [`list`] has every session.
pub async fn holders(pool: &MySqlPool, person: &str) -> Result<Vec<Holder>> {
    // `COUNT(t.id)` over the left join: ever assigned anything.
    let sessions: Vec<(String, Option<String>, i64, i64, i64)> = sqlx::query_as(tally!(
        "s.id, s.name, CAST(COUNT(t.id) AS SIGNED) AS ever, ",
        "FROM sessions s LEFT JOIN tasks t ON t.assignee_session = s.id GROUP BY s.id, s.name"
    ))
    .fetch_all(pool)
    .await
    .context("counting what each session holds")?;

    let mut out: Vec<Holder> = sessions
        .into_iter()
        .filter(|(_, _, ever, _, _)| *ever > 0)
        .map(|(id, name, _, open, done)| Holder {
            kind: "session".into(),
            id: Some(id),
            name,
            open,
            total: open + done,
        })
        .collect();
    out.sort_by_key(|h| (-h.open, -h.total));

    let (open, done): (i64, i64) =
        sqlx::query_as(tally!("", "FROM tasks t WHERE t.assignee_kind = 'person'"))
            .fetch_one(pool)
            .await
            .context("counting what the person holds")?;
    out.push(Holder {
        kind: "person".into(),
        id: Some(person.into()),
        name: Some(person.into()),
        open,
        total: open + done,
    });

    let (open, done): (i64, i64) =
        sqlx::query_as(tally!("", "FROM tasks t WHERE t.assignee_kind = 'nobody'"))
            .fetch_one(pool)
            .await
            .context("counting the pile")?;
    out.push(Holder {
        kind: "nobody".into(),
        id: None,
        name: Some("nobody".into()),
        open,
        total: open + done,
    });

    Ok(out)
}

/// Every session known, most recently seen first.
pub async fn list(pool: &MySqlPool) -> Result<Vec<Session>> {
    let rows: Vec<Row> = sqlx::query_as(concat!(
        "SELECT s.id, s.name, s.first_seen, s.last_seen, COUNT(t.id) AS open ",
        "FROM sessions s ",
        "LEFT JOIN tasks t ON t.assignee_session = s.id AND ",
        still_open!("t.status"),
        " GROUP BY s.id, s.name, s.first_seen, s.last_seen ",
        "ORDER BY s.last_seen DESC",
    ))
    .fetch_all(pool)
    .await
    .context("listing sessions")?;
    Ok(rows
        .into_iter()
        .map(|row| Session {
            id: row.id,
            name: row.name,
            first_seen: row.first_seen.and_utc(),
            last_seen: row.last_seen.and_utc(),
            open: row.open,
        })
        .collect())
}

/// Whether `id` has a UUID's shape, the id Claude Code gives a conversation.
fn is_uuid(id: &str) -> bool {
    let groups: Vec<&str> = id.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, len)| g.len() == len && g.chars().all(|c| c.is_ascii_hexdigit()))
}
