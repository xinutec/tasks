//! Reading and writing tasks.
//!
//! Every write records a [`task_events`] row in the same transaction as the
//! change it describes, because the history is not a log beside the data — it
//! is the part of the old file scheme that git used to provide, and a history
//! that can be absent for a write nobody noticed is not one.

use anyhow::Context;
use chrono::{DateTime, NaiveDateTime, Utc};
use sqlx::{MySql, MySqlPool, QueryBuilder, Transaction};

use crate::error::AppError;
use crate::tasks::types::{
    Actor, Assignee, AssigneeKind, Event, MAX_SUBJECT, Status, Task, TaskDetail,
};

type Result<T> = std::result::Result<T, AppError>;

/// Which tasks a caller is asking for.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Repositories to include. Empty means every task, **including the ones
    /// belonging to no repo** — which is the only view they appear in besides a
    /// person's own list. A session asks by repo, so it is never handed the
    /// personal items that have no checkout.
    pub repos: Vec<String>,
    /// Include finished tasks. Off by default, and every injected path leaves
    /// it off: see the invariant in `lib.rs`.
    pub include_done: bool,
    /// Only tasks held by this session id.
    pub session: Option<String>,
    /// Only tasks held by this person (Nextcloud user id).
    pub person: Option<String>,
}

impl Filter {
    /// Just the open tasks of these repositories — what a digest asks for.
    pub fn open_in(repos: Vec<String>) -> Self {
        Self {
            repos,
            ..Default::default()
        }
    }
}

/// A task row joined to the name of whichever session holds it.
#[derive(sqlx::FromRow)]
struct Row {
    id: u64,
    repo: Option<String>,
    subject: String,
    status: Status,
    assignee_kind: AssigneeKind,
    assignee_person: Option<String>,
    assignee_session: Option<String>,
    /// Resolved through the join, so a list never needs a second query per row.
    session_name: Option<String>,
    /// Whether the body has anything in it. Computed in SQL rather than
    /// selected, so a list of forty tasks does not carry forty bodies across
    /// the wire to answer a boolean — the mistake this whole service exists to
    /// avoid, in miniature.
    detailed: i8,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    closed_at: Option<NaiveDateTime>,
}

/// The session zone is pinned to UTC in `db::connect`, so a DB-clock column
/// read back as naive really is UTC.
fn utc(at: NaiveDateTime) -> DateTime<Utc> {
    at.and_utc()
}

impl Row {
    fn into_task(self) -> Task {
        let assignee = match self.assignee_kind {
            AssigneeKind::Nobody => Assignee::nobody(),
            AssigneeKind::Person => Assignee {
                kind: AssigneeKind::Person,
                name: self.assignee_person.clone(),
                id: self.assignee_person,
            },
            AssigneeKind::Session => Assignee {
                kind: AssigneeKind::Session,
                id: self.assignee_session,
                name: self.session_name,
            },
        };
        Task {
            id: self.id,
            repo: self.repo,
            subject: self.subject,
            status: self.status,
            assignee,
            detailed: self.detailed != 0,
            created_at: utc(self.created_at),
            updated_at: utc(self.updated_at),
            closed_at: self.closed_at.map(utc),
        }
    }
}

/// The task projection, with a literal tail appended.
///
/// A macro rather than a `const` + `format!` because sqlx 0.9 accepts only
/// `&'static str` — a runtime-built query string has to be asserted safe by
/// hand, and the whole reason that bound exists is to make somebody stop and
/// look. `concat!` keeps every query in this file a literal the compiler
/// assembled, so there is nothing to audit.
macro_rules! select {
    ($tail:literal) => {
        concat!(
            "SELECT t.id, t.repo, t.subject, t.status, t.assignee_kind, ",
            "t.assignee_person, t.assignee_session, s.name AS session_name, ",
            "(LENGTH(TRIM(t.body)) > 0) AS detailed, ",
            "t.created_at, t.updated_at, t.closed_at ",
            "FROM tasks t LEFT JOIN sessions s ON s.id = t.assignee_session",
            $tail
        )
    };
}

/// Tasks matching a filter, oldest id first.
///
/// ⚠ **Ordered by id, which is creation order, and not by status.** A list that
/// re-sorts as work starts on an item moves the line somebody was reading; the
/// client groups when it wants to.
pub async fn list(pool: &MySqlPool, filter: &Filter) -> Result<Vec<Task>> {
    let mut query = QueryBuilder::<MySql>::new(select!(""));
    query.push(" WHERE 1 = 1");
    if !filter.include_done {
        query.push(" AND t.status <> 'done'");
    }
    if !filter.repos.is_empty() {
        query.push(" AND t.repo IN (");
        let mut list = query.separated(", ");
        for repo in &filter.repos {
            list.push_bind(repo);
        }
        query.push(")");
    }
    if let Some(session) = &filter.session {
        query.push(" AND t.assignee_kind = 'session' AND t.assignee_session = ");
        query.push_bind(session);
    }
    if let Some(person) = &filter.person {
        query.push(" AND t.assignee_kind = 'person' AND t.assignee_person = ");
        query.push_bind(person);
    }
    query.push(" ORDER BY t.id");
    let rows: Vec<Row> = query
        .build_query_as()
        .fetch_all(pool)
        .await
        .context("listing tasks")?;
    Ok(rows.into_iter().map(Row::into_task).collect())
}

/// One task, its prose and its history. `None` when there is no such task.
pub async fn get(pool: &MySqlPool, id: u64) -> Result<Option<TaskDetail>> {
    let row: Option<Row> = sqlx::query_as(select!(" WHERE t.id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("reading a task")?;
    let Some(row) = row else {
        return Ok(None);
    };
    let body: Option<(String,)> = sqlx::query_as("SELECT body FROM tasks WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("reading a task body")?;
    let body = body.map(|(b,)| b).unwrap_or_default();
    let events = events(pool, id).await?;
    Ok(Some(TaskDetail {
        task: row.into_task(),
        body_html: render_markdown(&body),
        body,
        events,
    }))
}

#[derive(sqlx::FromRow)]
struct EventRow {
    at: NaiveDateTime,
    actor_kind: String,
    actor_id: Option<String>,
    kind: String,
    detail: Option<String>,
}

/// What has happened to a task, oldest first.
pub async fn events(pool: &MySqlPool, id: u64) -> Result<Vec<Event>> {
    let rows: Vec<EventRow> = sqlx::query_as(
        "SELECT at, actor_kind, actor_id, kind, detail FROM task_events \
         WHERE task_id = ? ORDER BY at, id",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .context("reading task history")?;
    Ok(rows
        .into_iter()
        .map(|row| Event {
            at: utc(row.at),
            kind: row.kind,
            detail: row.detail,
            actor: row.actor_id.unwrap_or_else(|| row.actor_kind.clone()),
        })
        .collect())
}

/// A task being filed.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct NewTask {
    pub repo: Option<String>,
    pub subject: String,
    #[serde(default)]
    pub body: String,
    /// Who it is for. Absent leaves it in the pile.
    #[serde(default)]
    pub assignee: Option<Assignee>,
}

/// A change to an existing task. Every field is optional and absent means
/// *leave it alone* — a genuine partial update, so a client changing a status
/// need not restate a body it has not read.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct Change {
    pub subject: Option<String>,
    pub body: Option<String>,
    pub status: Option<Status>,
    pub assignee: Option<Assignee>,
}

fn check_subject(subject: &str) -> Result<String> {
    let subject = subject.trim();
    if subject.is_empty() {
        return Err(AppError::BadRequest("a task needs a subject".into()));
    }
    if subject.chars().count() > MAX_SUBJECT {
        return Err(AppError::BadRequest(format!(
            "a subject is one line and at most {MAX_SUBJECT} characters — this is {}. \
             The rest of it is a body.",
            subject.chars().count()
        )));
    }
    // A newline in a subject would split one task across two lines of an
    // injected digest, where every line is read as its own task.
    if subject.contains(['\n', '\r']) {
        return Err(AppError::BadRequest(
            "a subject is one line — put the detail in the body".into(),
        ));
    }
    Ok(subject.to_string())
}

/// Split an assignee into the three columns that store it.
fn assignee_columns(assignee: &Assignee) -> (AssigneeKind, Option<&str>, Option<&str>) {
    match assignee.kind {
        AssigneeKind::Nobody => (AssigneeKind::Nobody, None, None),
        AssigneeKind::Person => (AssigneeKind::Person, assignee.id.as_deref(), None),
        AssigneeKind::Session => (AssigneeKind::Session, None, assignee.id.as_deref()),
    }
}

fn check_assignee(assignee: &Assignee) -> Result<()> {
    match assignee.kind {
        AssigneeKind::Nobody => Ok(()),
        AssigneeKind::Person | AssigneeKind::Session => {
            if assignee.id.as_deref().unwrap_or("").trim().is_empty() {
                Err(AppError::BadRequest(format!(
                    "an assignee of kind {} needs an id",
                    assignee.kind
                )))
            } else {
                Ok(())
            }
        }
    }
}

async fn record(
    tx: &mut Transaction<'_, MySql>,
    task_id: u64,
    actor: &Actor,
    kind: &str,
    detail: Option<String>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO task_events (task_id, actor_kind, actor_id, kind, detail) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(task_id)
    .bind(actor.kind())
    .bind(actor.id())
    .bind(kind)
    .bind(detail)
    .execute(&mut **tx)
    .await
    .context("recording a task event")?;
    Ok(())
}

/// File a new task.
pub async fn create(pool: &MySqlPool, new: NewTask, actor: &Actor) -> Result<Task> {
    let subject = check_subject(&new.subject)?;
    let assignee = new.assignee.unwrap_or_else(Assignee::nobody);
    check_assignee(&assignee)?;
    let (kind, person, session) = assignee_columns(&assignee);

    let mut tx = pool.begin().await.context("opening a transaction")?;
    let done = sqlx::query(
        "INSERT INTO tasks (repo, subject, body, assignee_kind, assignee_person, assignee_session) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(new.repo.as_deref())
    .bind(&subject)
    .bind(&new.body)
    .bind(kind)
    .bind(person)
    .bind(session)
    .execute(&mut *tx)
    .await
    .context("filing a task")?;
    let id = done.last_insert_id();
    record(&mut tx, id, actor, "created", Some(subject.clone())).await?;
    if kind != AssigneeKind::Nobody {
        record(
            &mut tx,
            id,
            actor,
            "assigned",
            Some(format!("→ {}", assignee.label())),
        )
        .await?;
    }
    tx.commit().await.context("committing a new task")?;

    list_one(pool, id).await
}

/// Apply a partial change.
///
/// ⚠ **Reads the row first and compares.** A change that alters nothing writes
/// no event: a client that PUTs the whole object on every keystroke would
/// otherwise fill the history with `open → open`, and a history full of
/// non-events is one nobody reads.
pub async fn update(pool: &MySqlPool, id: u64, change: Change, actor: &Actor) -> Result<Task> {
    let before = list_one(pool, id).await?;

    let mut tx = pool.begin().await.context("opening a transaction")?;

    if let Some(subject) = &change.subject {
        let subject = check_subject(subject)?;
        if subject != before.subject {
            sqlx::query("UPDATE tasks SET subject = ? WHERE id = ?")
                .bind(&subject)
                .bind(id)
                .execute(&mut *tx)
                .await
                .context("changing a subject")?;
            record(&mut tx, id, actor, "edited", Some(subject)).await?;
        }
    }

    if let Some(body) = &change.body {
        sqlx::query("UPDATE tasks SET body = ? WHERE id = ?")
            .bind(body)
            .bind(id)
            .execute(&mut *tx)
            .await
            .context("changing a body")?;
        record(&mut tx, id, actor, "edited", Some("body".into())).await?;
    }

    if let Some(status) = change.status
        && status != before.status
    {
        // `closed_at` moves with the status in the same statement, so a row
        // cannot be done with no closing time or open with one.
        sqlx::query(
            "UPDATE tasks SET status = ?, closed_at = IF(? = 'done', NOW(), NULL) WHERE id = ?",
        )
        .bind(status)
        .bind(status)
        .bind(id)
        .execute(&mut *tx)
        .await
        .context("changing a status")?;
        record(
            &mut tx,
            id,
            actor,
            "status",
            Some(format!("{} → {}", before.status, status)),
        )
        .await?;
    }

    if let Some(assignee) = &change.assignee {
        check_assignee(assignee)?;
        let (kind, person, session) = assignee_columns(assignee);
        // Compared as (kind, id), which is what the three columns encode: for
        // `Nobody` both ids are absent, and for the other two exactly one is
        // set — so this one comparison covers every move, including a handover
        // from one session to another.
        let moved =
            (kind, assignee.id.as_deref()) != (before.assignee.kind, before.assignee.id.as_deref());
        if moved {
            sqlx::query(
                "UPDATE tasks SET assignee_kind = ?, assignee_person = ?, assignee_session = ? \
                 WHERE id = ?",
            )
            .bind(kind)
            .bind(person)
            .bind(session)
            .bind(id)
            .execute(&mut *tx)
            .await
            .context("moving a task")?;
            record(
                &mut tx,
                id,
                actor,
                "assigned",
                Some(format!(
                    "{} → {}",
                    before.assignee.label(),
                    assignee.label()
                )),
            )
            .await?;
        }
    }

    tx.commit().await.context("committing a task change")?;
    list_one(pool, id).await
}

/// One task without its prose or history — the read every write does first,
/// and the value every write returns.
async fn list_one(pool: &MySqlPool, id: u64) -> Result<Task> {
    let row: Option<Row> = sqlx::query_as(select!(" WHERE t.id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("reading a task")?;
    row.map(Row::into_task).ok_or(AppError::NotFound)
}

/// Render a task body's markdown.
///
/// Same options as the memory corpus viewer minus its wikilinks: task prose is
/// written by the same hands, so tables, strikethrough and task lists are all
/// in use, and raw HTML renders escaped rather than being dropped so the text
/// stays visible.
pub fn render_markdown(md: &str) -> String {
    let mut options = comrak::Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.render.escape = true;
    comrak::markdown_to_html(md, &options)
}
