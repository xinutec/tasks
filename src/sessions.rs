//! The conversations work can be handed to.
//!
//! ⚠ **The CLI's session id is the identity; the name is an attribute.**
//! Everything points at the id, so a rename touches one column and no task
//! moves. The CLI reads the name from the conversation's own transcript
//! ([`crate::agent_name`]) and sends it on every request, so the column is a
//! cache of what Claude Code calls the conversation; [`touch`] stores it.
//!
//! A row is created by the first thing a session does, not by a registration
//! step: a session that must be enrolled before it can be given work will be
//! given work before it is enrolled.

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
    /// How much is on this session's plate, counted for every session in one
    /// query because the front page draws it per row.
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

/// Record that a session exists, and what it calls itself.
///
/// ⚠ **An absent name does not erase the stored one**, or the prompt hook,
/// which knows only the id, would blank it on every prompt. `Some("")` counts
/// as absent: the CLI reports an empty name before a session has one.
pub async fn touch(pool: &MySqlPool, id: &str, name: Option<&str>) -> Result<()> {
    let name = name.map(str::trim).filter(|n| !n.is_empty());
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

/// One party's share of the work: what they are holding, and what they have held.
///
/// ⚠ **`total` counts finished work — the reason this type exists apart from
/// [`Session`].** `open` says who is busy, not who has done anything: `0/56`
/// is a cleared plate, where a bare `0` reads as an idle one.
///
/// ⚠ **A dropped task is in neither number.** It is neither in hand nor done;
/// counting it would make dropping ten stale items read as finishing ten.
#[derive(Debug, Clone, Serialize)]
pub struct Holder {
    /// `session`, `person` or `nobody` — the same vocabulary as an assignee.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What to call them. A session may have none, and the client shows the id;
    /// the pile and the person always have one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub open: i64,
    /// Open plus finished. Never derived on a client: two numbers that must
    /// agree should be counted by one query.
    pub total: i64,
}

/// Count a group of tasks as `open` and `done`.
///
/// ⚠ **Both halves are counted, and `total` is added in Rust.** Spelling it as
/// "everything except dropped" in SQL would be a `<> 'something'`, which
/// silently miscounts when a status is added; `still_open!` and `= 'done'` are
/// the only two things these queries say. `SUM` of a boolean comes back
/// DECIMAL, which sqlx will not decode into `i64` — hence the cast.
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

/// Who holds what: every session that has held something, Pippijn, and the pile.
///
/// Three queries rather than one union: the groups are counted from different
/// columns — `assignee_session`, `assignee_person`, and the absence of both.
/// Most open first, since the question is "who is loaded"; ties go to the
/// larger history.
///
/// ⚠ **A session that has never been given a task is left out.** Every
/// conversation that ever ran has a row, and nearly all of them never held
/// anything; listing them would bury the holders under `0/0` lines. The
/// predicate is *ever assigned*, not *anything open*, so a cleared plate still
/// says who cleared it. [`list`] is every session known (`task sessions --all`).
pub async fn holders(pool: &MySqlPool) -> Result<Vec<Holder>> {
    // `COUNT(t.id)` over the left join is *ever assigned anything* —
    // deliberately not spelled with the status vocabulary.
    //
    // dev-lint: allow-sqlx — a `concat!`ed literal assembled by `tally!`, not a
    // string built at runtime; the only interpolation is another macro.
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

    // dev-lint: allow-sqlx — as above.
    let (open, done): (i64, i64) =
        sqlx::query_as(tally!("", "FROM tasks t WHERE t.assignee_kind = 'person'"))
            .fetch_one(pool)
            .await
            .context("counting what the person holds")?;
    out.push(Holder {
        kind: "person".into(),
        id: Some("pippijn".into()),
        name: Some("Pippijn".into()),
        open,
        total: open + done,
    });

    // dev-lint: allow-sqlx — as above.
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
    // dev-lint: allow-sqlx — a `concat!`ed literal; the only interpolation is
    // `still_open!`, which is where the open vocabulary lives.
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
