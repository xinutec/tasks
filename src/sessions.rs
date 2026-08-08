//! The conversations work can be handed to.
//!
//! ⚠ **The CLI's session id is the identity; the name is an attribute.** A
//! session renames itself as its job changes — Pippijn's own note: it happens,
//! rarely, and the rename arrives here as an update. Everything that points at a
//! session points at the id, so a rename touches one column and no task moves.
//!
//! A row is created by the first thing a session does, not by a registration
//! step: a session that has to be enrolled before it can be given work is a
//! session that will be given work before it is enrolled.

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
    /// How much is on this session's plate right now. The front page draws it
    /// per row, so it is swept for every session in one query rather than asked
    /// once per row.
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
/// ⚠ **An absent name does not erase the stored one.** A caller that knows its
/// id but not its name (the prompt hook, which is given only the id) would
/// otherwise blank the name on every prompt, and the list would be a column of
/// uuids by lunchtime. Passing `Some("")` is treated the same way: the CLI
/// reports an empty name before a session has titled itself.
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
/// ⚠ **`total` counts finished work, which is the only reason this type exists
/// apart from [`Session`].** `open` alone says who is busy; it says nothing
/// about who has done anything, because a task leaves `open` the moment it is
/// finished. A session with `0/56` has cleared its plate, and a bare `0` reads
/// as an idle one.
///
/// ⚠ **A dropped task is in neither number.** It is not work in hand and it is
/// not work done, and counting it would make `total` mean "tasks that reached an
/// end", which is a figure nobody wants — dropping ten stale items would read as
/// having finished ten. So a task that is dropped simply leaves the tally, which
/// is the honest thing for a number whose whole job is to say what somebody has
/// got through.
#[derive(Debug, Clone, Serialize)]
pub struct Holder {
    /// `session`, `person` or `nobody` — the same vocabulary as an assignee.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What to call them. A session that has not named itself has none, and the
    /// client shows the id; the pile and the person always have one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub open: i64,
    /// Open plus finished. Never derived on a client: two numbers that must
    /// agree should be counted by one query.
    pub total: i64,
}

/// Count a group of tasks as `open` and `done`.
///
/// ⚠ **Both halves are counted, and `total` is added up in Rust.** Not for want
/// of a third `SUM`: the two figures must agree, and expressing the second as
/// "everything except dropped" would have put the closed vocabulary into SQL
/// three more times, in the exact shape (`<> 'something'`) that made the fourth
/// status a silent miscount in the first place. `still_open!` and `= 'done'`
/// are the only two things these queries know how to say. `SUM` of a boolean
/// comes back DECIMAL, hence the cast — sqlx will not decode it into `i64`, and
/// nothing but a real query finds that out.
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

/// Who holds what: every session, Pippijn, and the pile.
///
/// Three queries rather than one union, because the three groups are counted
/// from different columns — `assignee_session`, `assignee_person`, and the
/// absence of both. Ordered by what is open, most first, since the question
/// this answers is "who is loaded"; ties go to the larger history.
pub async fn holders(pool: &MySqlPool) -> Result<Vec<Holder>> {
    // dev-lint: allow-sqlx — a `concat!`ed literal assembled by `tally!`, not a
    // string built at runtime; the only interpolation is another macro.
    let sessions: Vec<(String, Option<String>, i64, i64)> = sqlx::query_as(tally!(
        "s.id, s.name, ",
        "FROM sessions s LEFT JOIN tasks t ON t.assignee_session = s.id GROUP BY s.id, s.name"
    ))
    .fetch_all(pool)
    .await
    .context("counting what each session holds")?;

    let mut out: Vec<Holder> = sessions
        .into_iter()
        .map(|(id, name, open, done)| Holder {
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
