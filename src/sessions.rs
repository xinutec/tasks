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

/// Every session known, most recently seen first.
pub async fn list(pool: &MySqlPool) -> Result<Vec<Session>> {
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT s.id, s.name, s.first_seen, s.last_seen, \
                COUNT(t.id) AS open \
         FROM sessions s \
         LEFT JOIN tasks t \
                ON t.assignee_session = s.id AND t.status <> 'done' \
         GROUP BY s.id, s.name, s.first_seen, s.last_seen \
         ORDER BY s.last_seen DESC",
    )
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
