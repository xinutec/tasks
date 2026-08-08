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
    origin_session: Option<String>,
    origin_number: Option<u32>,
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
            // Rendered here rather than stored joined, so the two columns stay
            // separately queryable and the wire carries one readable thing.
            origin: match (self.origin_session, self.origin_number) {
                (Some(session), Some(number)) => Some(format!("{session}#{number}")),
                // A session with no number, or the other way round, is a
                // half-written import; say what is known rather than nothing.
                (Some(session), None) => Some(session),
                (None, Some(number)) => Some(format!("#{number}")),
                (None, None) => None,
            },
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
            "t.created_at, t.updated_at, t.closed_at, ",
            "t.origin_session, t.origin_number ",
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
    // `select!` expands to `concat!`, so this argument IS a string literal by the
    // time rustc sees it; only the linter's reader sees a macro. The alternative
    // is writing the twelve-column projection out twice.
    // dev-lint: allow-sqlx — a `concat!`ed literal, not a runtime-built string.
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
    /// Absent when the actor is a person, or a session nobody has named.
    actor_name: Option<String>,
    kind: String,
    detail: Option<String>,
}

/// What has happened to a task, oldest first.
///
/// ⚠ **The actor is resolved at READ time and the detail was rendered at WRITE
/// time**, so a session that has since renamed itself reads as its current name
/// in the `actor` column and as its old one inside `nobody → memview`. That is
/// deliberate and it is not a contradiction: the actor answers *who did this*,
/// which is a live conversation you may want to hand the next thing to, and the
/// detail answers *what the line said then*, which must not be rewritten by a
/// later rename. Resolving the actor at write time instead would print a
/// 36-character id in every line for a session that had not yet named itself —
/// the shape this replaced, and unreadable on a phone.
pub async fn events(pool: &MySqlPool, id: u64) -> Result<Vec<Event>> {
    let rows: Vec<EventRow> = sqlx::query_as(
        "SELECT e.at, e.actor_kind, e.actor_id, s.name AS actor_name, e.kind, e.detail \
         FROM task_events e LEFT JOIN sessions s ON s.id = e.actor_id \
         WHERE e.task_id = ? ORDER BY e.at, e.id",
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
            // Name, then id, then the bare kind. Each fallback is a real state:
            // a session that never named itself, and — for a person — no
            // `sessions` row to join to at all.
            actor: row
                .actor_name
                .filter(|name| !name.is_empty())
                .or(row.actor_id)
                .unwrap_or(row.actor_kind),
        })
        .collect())
}

/// A task being filed.
///
/// ⚠ **The `skip_serializing_if` attributes change no behaviour** — this struct
/// is deserialised and never serialised. They state the shape the *client*
/// sends, which is the contract both sides are written against and what
/// dev-lint's wire-mirror check compares `frontend/src/app/models.ts` against.
/// Without them the check reads "always present" from a type nothing ever
/// serialises, and asks the client to send a key whose absence is the meaning.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct NewTask {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    pub subject: String,
    #[serde(default)]
    pub body: String,
    /// Who it is for. Absent leaves it in the pile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<Assignee>,
}

/// A change to an existing task. Every field is optional and absent means
/// *leave it alone* — a genuine partial update, so a client changing a status
/// need not restate a body it has not read.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct Change {
    // As in `NewTask`: absence is the meaning, and these attributes are how that
    // is stated to the mirror check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

/// What to call an assignee in the history, resolved against the session table.
///
/// ⚠ **Written history needs the same name on both sides of an arrow.** A
/// caller supplies an assignee as a kind and an id and nothing else, while the
/// task being changed was read back through the join and carries a name — so
/// rendering both with [`Assignee::label`] produced `nobody → sess-1` followed
/// by `memview → pippijn`, two names for one conversation in two consecutive
/// lines of the same task's history. The detail is rendered at write time on
/// purpose (the actor may be gone by the time anybody reads it), which means the
/// resolution has to happen at write time too.
async fn label_of(tx: &mut Transaction<'_, MySql>, assignee: &Assignee) -> Result<String> {
    let (AssigneeKind::Session, Some(id)) = (assignee.kind, assignee.id.as_deref()) else {
        return Ok(assignee.label());
    };
    let name: Option<(Option<String>,)> = sqlx::query_as("SELECT name FROM sessions WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .context("resolving a session name")?;
    // A session nobody has named yet reads as its id, which is truer than
    // silence and is what the list shows too.
    Ok(name
        .and_then(|(name,)| name)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| id.to_string()))
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
        let to = label_of(&mut tx, &assignee).await?;
        record(&mut tx, id, actor, "assigned", Some(format!("→ {to}"))).await?;
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
            let to = label_of(&mut tx, assignee).await?;
            record(
                &mut tx,
                id,
                actor,
                "assigned",
                Some(format!("{} → {to}", before.assignee.label())),
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
    // dev-lint: allow-sqlx — a `concat!`ed literal; see `get` above.
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
