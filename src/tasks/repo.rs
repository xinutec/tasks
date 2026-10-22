//! Every write records its `task_events` row in the same transaction: a
//! history that can miss a write is not one.

use anyhow::Context;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use sqlx::{MySql, MySqlPool, QueryBuilder, Transaction};

use crate::error::AppError;
use crate::tasks::types::{
    Actor, Assignee, AssigneeKind, Event, MAX_SUBJECT, Moved, Priority, Ranking, Replaced,
    Revision, Status, Task, TaskDetail, Updated,
};
use crate::{body_shown, due_soon, still_open};

type Result<T> = std::result::Result<T, AppError>;

/// Which tasks a caller is asking for.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Include closed tasks, done and dropped. No injected path sets it.
    pub include_closed: bool,
    /// Only tasks held by this session id.
    pub session: Option<String>,
    /// Only tasks held by this person (Nextcloud user id).
    pub person: Option<String>,
    /// Widen [`session`](Self::session) to include the pile ([`digest_for`](Self::digest_for)).
    pub or_unheld: bool,
    /// Strictly the pile; wins over `session` and `person`.
    pub unheld: bool,
    /// Tasks this session filed and doesn't hold: what it handed out, to anyone,
    /// the pile included. Wins over the fields above. Never for the digest, which
    /// must not show another conversation's tasks.
    pub handed_out_by: Option<String>,
}

impl Filter {
    /// A session's digest: its own open tasks and the pile. ⚠ Keep `or_unheld`:
    /// without it a task left for whoever is around is invisible to everyone.
    pub fn digest_for(session: &str) -> Self {
        Self {
            session: Some(session.to_string()),
            or_unheld: true,
            ..Default::default()
        }
    }
}

/// A task row joined to the name of whichever session holds it.
#[derive(sqlx::FromRow)]
struct Row {
    id: u64,
    subject: String,
    status: Status,
    priority: Option<Priority>,
    due: Option<NaiveDate>,
    /// `P0` when a near deadline raises it, else NULL (`due_soon!`).
    escalated_to: Option<Priority>,
    /// Whether `due` has passed, by the database's clock.
    past_due: i8,
    /// The blockers, comma-joined (see the projection).
    blocked_on: Option<String>,
    /// How many are still open.
    open_blockers: i64,
    assignee_kind: AssigneeKind,
    assignee_person: Option<String>,
    assignee_session: Option<String>,
    /// Through the join: no query per row.
    session_name: Option<String>,
    /// In SQL, so a list doesn't ship every body to answer a boolean.
    detailed: i8,
    /// Lines of the trimmed body, in SQL.
    body_lines: i64,
    /// See [`Task::sprawl_chars`]; the words stay off lists.
    sprawl_chars: Option<u32>,
    /// See [`Task::filed_by`].
    filed_by: Option<String>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    closed_at: Option<NaiveDateTime>,
}

/// The session zone is UTC (`db::connect`), so a naive DB-clock value is UTC.
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
            subject: self.subject,
            status: self.status,
            priority: self.priority,
            due: self.due,
            escalated_to: self.escalated_to,
            overdue: self.past_due != 0,
            blocked_on: parse_ids(self.blocked_on.as_deref()),
            blocked: self.open_blockers > 0,
            assignee,
            detailed: self.detailed != 0,
            body_lines: self.body_lines.max(0) as u32,
            sprawl_chars: self.sprawl_chars,
            filed_by: self.filed_by,
            created_at: utc(self.created_at),
            updated_at: utc(self.updated_at),
            closed_at: self.closed_at.map(utc),
        }
    }
}

/// The task projection plus a literal tail: a macro, so every query stays a compile-time
/// literal for sqlx.
macro_rules! select {
    ($tail:literal) => {
        concat!(
            "SELECT t.id, t.subject, t.status, t.priority, t.due, ",
            // What it sorts as (`Task::escalated_to`); NULL when already `P0`.
            "IF(",
            due_soon!("t.due"),
            " AND COALESCE(t.priority, 'P2') > 'P0', ",
            "'P0', NULL) AS escalated_to, ",
            // The database's clock (UTC, `db::connect`), so CLI, app and digest agree on the day.
            "(t.due IS NOT NULL AND t.due < CURDATE()) AS past_due, ",
            "t.assignee_kind, ",
            // Correlated, not joined: a join to `task_blocks` repeats the row per edge.
            //
            // ⚠ GROUP_CONCAT stops at `group_concat_max_len` (~140 ids); the count beside
            // it is exact, so `blocked` stays right.
            "(SELECT GROUP_CONCAT(b.blocked_on ORDER BY b.blocked_on) FROM task_blocks b ",
            "WHERE b.task_id = t.id) AS blocked_on, ",
            "(SELECT COUNT(*) FROM task_blocks b JOIN tasks bt ON bt.id = b.blocked_on ",
            "WHERE b.task_id = t.id AND ",
            still_open!("bt.status"),
            ") AS open_blockers, ",
            "t.assignee_person, t.assignee_session, s.name AS session_name, ",
            // Both measure the body as shown (`body_shown!`).
            "(LENGTH(",
            body_shown!("t.body"),
            ") > 0) AS detailed, t.sprawl_chars, ",
            // Counted as `show` prints it: newlines by length difference (exact over a
            // one-byte needle), `CHAR(10)` for the escaping reason in `body_shown!`, `+ 1`
            // for the last line. CAST: unsigned arithmetic won't decode as `i64`.
            "CAST(IF(LENGTH(",
            body_shown!("t.body"),
            ") = 0, 0, LENGTH(",
            body_shown!("t.body"),
            ") - LENGTH(REPLACE(",
            body_shown!("t.body"),
            ", CHAR(10), '')) + 1) AS SIGNED) AS body_lines, ",
            // Correlated; `LIMIT 1` can't add rows (`idx_task_events_task`).
            "(SELECT f.name FROM task_events c JOIN sessions f ON f.id = c.actor_id ",
            "WHERE c.task_id = t.id AND c.kind = 'created' AND c.actor_kind = 'session' ",
            "ORDER BY c.id LIMIT 1) AS filed_by, ",
            "t.created_at, t.updated_at, t.closed_at ",
            "FROM tasks t LEFT JOIN sessions s ON s.id = t.assignee_session",
            $tail
        )
    };
}

/// Tasks matching a filter, by effective priority, then oldest id first.
///
/// Never by status, so a line doesn't move while being read; oldest first, so old
/// tickets aren't buried. `COALESCE(t.priority, 'P2')` mirrors
/// [`Priority::rank`](crate::tasks::types::Priority::rank). The only sort in the
/// service: `digest::render` keeps this order.
pub async fn list(pool: &MySqlPool, filter: &Filter) -> Result<Vec<Task>> {
    let mut query = QueryBuilder::<MySql>::new(select!(""));
    query.push(" WHERE 1 = 1");
    if !filter.include_closed {
        query.push(concat!(" AND ", still_open!("t.status")));
    }
    // Exclusive, in the precedence `Filter` documents.
    if let Some(filer) = &filter.handed_out_by {
        // By actor id, not the name a rename changes.
        query.push(concat!(
            " AND (SELECT c.actor_id FROM task_events c ",
            "WHERE c.task_id = t.id AND c.kind = 'created' AND c.actor_kind = 'session' ",
            "ORDER BY c.id LIMIT 1) = "
        ));
        query.push_bind(filer);
        // Not still on the filer's own plate.
        query.push(" AND NOT (t.assignee_kind = 'session' AND t.assignee_session = ");
        query.push_bind(filer);
        query.push(")");
    } else if filter.unheld {
        query.push(" AND t.assignee_kind = 'nobody'");
    } else if let Some(session) = &filter.session {
        query.push(" AND ((t.assignee_kind = 'session' AND t.assignee_session = ");
        query.push_bind(session);
        query.push(")");
        if filter.or_unheld {
            query.push(" OR t.assignee_kind = 'nobody'");
        }
        query.push(")");
    }
    if !filter.unheld
        && filter.handed_out_by.is_none()
        && let Some(person) = &filter.person
    {
        query.push(" AND t.assignee_kind = 'person' AND t.assignee_person = ");
        query.push_bind(person);
    }
    // The effective rank: within the week a deadline raises a task to `P0`.
    query.push(concat!(
        " ORDER BY IF(",
        due_soon!("t.due"),
        ", 'P0', COALESCE(t.priority, 'P2')), t.id"
    ));
    let rows: Vec<Row> = query
        .build_query_as()
        .fetch_all(pool)
        .await
        .context("listing tasks")?;
    Ok(rows.into_iter().map(Row::into_task).collect())
}

/// One task with prose and history, or `None`.
pub async fn get(pool: &MySqlPool, id: u64) -> Result<Option<TaskDetail>> {
    let row: Option<Row> = sqlx::query_as(select!(" WHERE t.id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("reading a task")?;
    let Some(row) = row else {
        return Ok(None);
    };
    // The critique travels with its body, the one read that carries prose.
    let body: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT body, sprawl_said FROM tasks WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
            .context("reading a task body")?;
    let (body, sprawl_said) = body.unwrap_or_default();
    let events = events(pool, id).await?;
    // A boolean in SQL: fetching the revision would ship a second body.
    let restorable: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM task_revision r JOIN task_events e ON e.id = r.event_id \
         WHERE e.task_id = ?)",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .context("looking for a previous version")?;
    Ok(Some(TaskDetail {
        task: row.into_task(),
        body_html: render_markdown(&body),
        body,
        events,
        restorable,
        sprawl_said,
    }))
}

#[derive(sqlx::FromRow)]
struct EventRow {
    at: NaiveDateTime,
    actor_kind: String,
    actor_id: Option<String>,
    /// Absent for a person or an unnamed session.
    actor_name: Option<String>,
    kind: String,
    detail: Option<String>,
}

/// What happened to a task, oldest first. The actor is resolved at read time
/// (who to hand the next thing to); the detail was rendered at write time and
/// keeps the names of then. Resolving at write time would print raw ids for
/// sessions not yet named.
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
            actor: actor_label(row.actor_name, row.actor_id, row.actor_kind),
        })
        .collect())
}

/// Name, then id, then the bare kind: each fallback is a real state.
fn actor_label(name: Option<String>, id: Option<String>, kind: String) -> String {
    name.filter(|name| !name.is_empty()).or(id).unwrap_or(kind)
}

#[derive(sqlx::FromRow)]
struct RevisionRow {
    at: NaiveDateTime,
    actor_kind: String,
    actor_id: Option<String>,
    actor_name: Option<String>,
    subject: String,
    body: String,
}

/// The task before its latest edit, or `None` if nothing was overwritten.
///
/// ⚠ Newest by `event_id`: `at` has one-second resolution, and a sweep writes
/// several edits a second. Takes the asker, to answer [`Revision::mine`].
pub async fn previous(pool: &MySqlPool, id: u64, who: &Actor) -> Result<Option<Revision>> {
    let row: Option<RevisionRow> = sqlx::query_as(
        "SELECT e.at, e.actor_kind, e.actor_id, s.name AS actor_name, r.subject, r.body \
         FROM task_revision r JOIN task_events e ON e.id = r.event_id \
         LEFT JOIN sessions s ON s.id = e.actor_id \
         WHERE e.task_id = ? ORDER BY r.event_id DESC LIMIT 1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("reading a previous version")?;
    Ok(row.map(|row| Revision {
        at: utc(row.at),
        // Kind and id both, or a person and a session sharing an id would match.
        // Old rows have a NULL id, which matches nobody.
        mine: row.actor_kind == who.kind() && row.actor_id.as_deref() == Some(who.id()),
        actor: actor_label(row.actor_name, row.actor_id, row.actor_kind),
        subject: row.subject,
        body: row.body,
    }))
}

/// When and by whom the text about to be replaced was written; filing counts.
async fn last_written(
    tx: &mut Transaction<'_, MySql>,
    id: u64,
) -> Result<Option<(DateTime<Utc>, String)>> {
    let row: Option<EventRow> = sqlx::query_as(
        "SELECT e.at, e.actor_kind, e.actor_id, s.name AS actor_name, e.kind, e.detail \
         FROM task_events e LEFT JOIN sessions s ON s.id = e.actor_id \
         WHERE e.task_id = ? AND e.kind IN (?, ?) \
         ORDER BY e.id DESC LIMIT 1",
    )
    .bind(id)
    // From [`Moved`]: a spelled copy silently stops matching after a rename.
    .bind(Moved::Created.as_str())
    .bind(Moved::Edited.as_str())
    .fetch_optional(&mut **tx)
    .await
    .context("reading who last wrote a task")?;
    Ok(row.map(|row| {
        (
            utc(row.at),
            actor_label(row.actor_name, row.actor_id, row.actor_kind),
        )
    }))
}

/// The default for [`NewTask::checked`].
fn yes() -> bool {
    true
}

/// A task being filed. Never serialised; the `skip_serializing_if` attributes
/// tell dev-lint's wire-mirror check which keys the client may omit.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct NewTask {
    pub subject: String,
    #[serde(default)]
    pub body: String,
    /// Whether the duplicate check ran. Absent means it did, so an old client gets
    /// no silent exemption.
    #[serde(default = "yes")]
    pub checked: bool,
    /// The tasks a refusal named that the filer judged different work, when
    /// `checked` is false. The service requires every one the refusal named,
    /// and the filing's history records them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_duplicate_of: Vec<u64>,
    /// How urgent: the one field a filer may not leave out. No serde attributes on
    /// purpose, so a missing key is refused and the mirror reads it as required and
    /// nullable ([`Ranking`]).
    pub priority: Ranking,
    /// The deadline, if something outside decides it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<NaiveDate>,
    /// What it waits for, if known when filed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_on: Vec<u64>,
    /// Who it is for. Absent means the filer, not the pile ([`create`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<Assignee>,
    /// Why it belongs to nobody: required exactly when it does. Pile filings used
    /// to be corrected later, so the pile is argued for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spare: Option<String>,
}

/// Names both answers, or the message reads as *you must pick a level* ([`crate::wire`]).
impl crate::wire::RequiredKeys for NewTask {
    fn required() -> &'static [(&'static str, &'static str)] {
        &[(
            "priority",
            r#""P0" to "P4" if you have judged it, or null for unassessed if nobody has"#,
        )]
    }
}

/// A change to a task: absent means leave it alone, so a status change needn't restate the body.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct Change {
    // As in `NewTask`, stating to the mirror check that keys may be omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Text added above the body, keeping all of it, so adding a paragraph needn't
    /// read and resend the body. Above, because what is current otherwise sinks.
    /// Resolved under the row lock, so concurrent additions can't lose each other.
    /// Removing it was considered and refuted: sessions typed the same prepends by
    /// hand before it existed, so it saves no reading, only a `--body` that replaces
    /// everything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepend: Option<String>,
    /// Text added below the body. May be sent with `prepend`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub append: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    /// Can't clear a priority (a meaningful null makes clients guess); rank again instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<NaiveDate>,
    /// Take the deadline off. A flag, because every date is a real deadline.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub clear_due: bool,
    /// The whole blocker set, not an addition; empty unblocks, absent leaves alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_on: Option<Vec<u64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<Assignee>,
    /// Says that a body keeping almost nothing of the old one is meant
    /// (`collapses`). Undo sets it: restoring before an edit that grew a body is
    /// the shape the guard refuses.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replace_body: bool,
}

/// Nothing is required of a change: do not copy `NewTask`'s list, or every
/// edit would have to restate the rank.
impl crate::wire::RequiredKeys for Change {}

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
    // A newline in a subject would split one task over two digest lines.
    if subject.contains(['\n', '\r']) {
        return Err(AppError::BadRequest(
            "a subject is one line — put the detail in the body".into(),
        ));
    }
    Ok(subject.to_string())
}

/// Below this a body isn't guarded. Like [`KEEPS_AT_LEAST`], far from any real
/// rewrite: it catches writes that were never edits.
const WORTH_GUARDING: usize = 500;

/// A quarter. A rewrite keeping half passes: a backstop against truncation, not
/// against overwriting (`task undo` covers that).
const KEEPS_AT_LEAST: usize = 4;

/// Whether a new body keeps so little of the old that it is likely a mistake,
/// like a session writing `--json`'s `detailed` boolean over the prose.
///
/// ⚠ Don't tighten it: good conclusion-first rewrites keep as little as a third.
/// [`Change::prepend`] answers the middle ground.
fn collapses(was: &str, now: &str) -> bool {
    let (was, now) = (was.chars().count(), now.chars().count());
    was > WORTH_GUARDING && now * KEEPS_AT_LEAST < was
}

/// What a body becomes with an addition above or below.
///
/// Exactly one blank line at each seam, or markdown merges the note into the
/// next paragraph. Newlines only, never spaces or tabs, which carry markdown
/// meaning. The body is trimmed only on the side that gained a neighbour, and an
/// empty body takes the addition alone.
fn joined(was: &str, prepend: Option<&str>, append: Option<&str>) -> String {
    const SEAM: [char; 2] = ['\n', '\r'];

    let mut middle = was;
    if prepend.is_some() {
        middle = middle.trim_start_matches(SEAM);
    }
    if append.is_some() {
        middle = middle.trim_end_matches(SEAM);
    }

    let mut parts: Vec<&str> = Vec::with_capacity(3);
    parts.extend(prepend.map(|text| text.trim_matches(SEAM)));
    parts.push(middle);
    parts.extend(append.map(|text| text.trim_matches(SEAM)));
    parts
        .into_iter()
        // A whitespace-only part would still open a seam.
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn assignee_columns(assignee: &Assignee) -> (AssigneeKind, Option<&str>, Option<&str>) {
    match assignee.kind {
        AssigneeKind::Nobody => (AssigneeKind::Nobody, None, None),
        AssigneeKind::Person => (AssigneeKind::Person, assignee.id.as_deref(), None),
        AssigneeKind::Session => (AssigneeKind::Session, None, assignee.id.as_deref()),
    }
}

/// The asker as a holder, name left for the join to resolve.
fn actor_holder(actor: &Actor) -> Assignee {
    let (kind, id) = match actor {
        Actor::Person(id) => (AssigneeKind::Person, id),
        Actor::Session(id) => (AssigneeKind::Session, id),
    };
    Assignee {
        kind,
        id: Some(id.clone()),
        name: None,
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

/// The ids `GROUP_CONCAT` returns. Unparseable ones are dropped, never made
/// `0`: the database produced them, so one means the query changed.
pub(crate) fn parse_ids(joined: Option<&str>) -> Vec<u64> {
    joined
        .unwrap_or("")
        .split(',')
        .filter_map(|id| id.trim().parse().ok())
        .collect()
}

/// A blocked task may rank equal to its blocker, never higher, checked at both
/// ends (raising the task, or lowering a blocker). The bound is the least urgent
/// open blocker. Refused, not cascaded, naming both tasks. Closed blockers don't
/// count. An unranked task claims nothing, so is never in violation; an unranked
/// blocker counts as `P2`.
async fn blocking_is_consistent(
    tx: &mut Transaction<'_, MySql>,
    id: u64,
    priority: Option<Priority>,
    due: Option<NaiveDate>,
) -> Result<()> {
    // The deadline twin: a due date before an open blocker's is impossible, so it
    // is refused (equal is fine). Checked first, as the harder fact.
    if let Some(mine) = due {
        let ahead: Vec<(u64, NaiveDate)> = sqlx::query_as(concat!(
            "SELECT bt.id, bt.due FROM task_blocks b JOIN tasks bt ON bt.id = b.blocked_on ",
            "WHERE b.task_id = ? AND bt.due IS NOT NULL AND bt.due > ? AND ",
            still_open!("bt.status")
        ))
        .bind(id)
        .bind(mine)
        .fetch_all(&mut **tx)
        .await
        .context("reading the deadlines of what blocks this task")?;
        if let Some((blocker, theirs)) = ahead.first() {
            return Err(AppError::BadRequest(format!(
                "#{id} would be due {mine} while #{blocker}, which blocks it, is not due \
                 until {theirs} — it cannot be finished before the thing it waits for."
            )));
        }
    }
    // The other end: a blocker pushed past what waits on it.
    let stranded: Vec<(u64, NaiveDate)> = sqlx::query_as(concat!(
        "SELECT t.id, t.due FROM task_blocks b JOIN tasks t ON t.id = b.task_id ",
        "WHERE b.blocked_on = ? AND t.due IS NOT NULL AND ",
        still_open!("t.status"),
        " AND ? IS NOT NULL AND t.due < ?"
    ))
    .bind(id)
    .bind(due)
    .bind(due)
    .fetch_all(&mut **tx)
    .await
    .context("reading the deadlines of what this task blocks")?;
    if let Some((other, theirs)) = stranded.first() {
        let mine = due.expect("the query cannot match when this is null");
        return Err(AppError::BadRequest(format!(
            "#{id} would not be due until {mine} while #{other}, which is blocked on it, \
             is due {theirs} — that leaves #{other} due before the thing it waits for."
        )));
    }

    let Some(stated) = priority else {
        // This task claims nothing; its dependents are checked below only where they claim
        // something.
        return unblocked_end(tx, id, None).await;
    };
    let mine = Priority::rank(Some(stated));

    // What this task waits for, still open.
    let blockers: Vec<(u64, Option<Priority>)> = sqlx::query_as(concat!(
        "SELECT bt.id, bt.priority FROM task_blocks b JOIN tasks bt ON bt.id = b.blocked_on ",
        "WHERE b.task_id = ? AND ",
        still_open!("bt.status")
    ))
    .bind(id)
    .fetch_all(&mut **tx)
    .await
    .context("reading what blocks this task")?;
    for (blocker, blocker_priority) in blockers {
        let theirs = Priority::rank(blocker_priority);
        if mine < theirs {
            return Err(AppError::BadRequest(format!(
                "#{id} would be {mine} while #{blocker}, which blocks it, is {theirs} — \
                 a task cannot be more urgent than what it is waiting for. Rank \
                 #{blocker} up first, or rank this one no higher than {theirs}."
            )));
        }
    }

    unblocked_end(tx, id, Some(stated)).await
}

/// What waits for this task, and whether demoting it breaks them. Apart, so an unranked
/// task still checks its dependents.
async fn unblocked_end(
    tx: &mut Transaction<'_, MySql>,
    id: u64,
    priority: Option<Priority>,
) -> Result<()> {
    let mine = Priority::rank(priority);
    let waiting: Vec<(u64, Option<Priority>)> = sqlx::query_as(concat!(
        "SELECT t.id, t.priority FROM task_blocks b JOIN tasks t ON t.id = b.task_id ",
        "WHERE b.blocked_on = ? AND ",
        still_open!("t.status")
    ))
    .bind(id)
    .fetch_all(&mut **tx)
    .await
    .context("reading what this task blocks")?;
    for (other, other_priority) in waiting {
        // An unranked dependent claims nothing.
        let Some(theirs) = other_priority else {
            continue;
        };
        if theirs < mine {
            return Err(AppError::BadRequest(format!(
                "#{id} would be {mine} while #{other}, which is blocked on it, is \
                 {theirs} — that leaves #{other} more urgent than what it waits for. \
                 Rank #{other} down first."
            )));
        }
    }
    Ok(())
}

/// Refuse a cycle, in which nothing could start. The whole graph is walked:
/// `A → B → C → A` arrives as three edits.
async fn no_cycle(tx: &mut Transaction<'_, MySql>, id: u64, proposed: &[u64]) -> Result<()> {
    let mut seen: Vec<u64> = vec![id];
    let mut queue: Vec<u64> = proposed.to_vec();
    while let Some(next) = queue.pop() {
        if next == id {
            return Err(AppError::BadRequest(format!(
                "#{id} cannot be blocked on itself, or on anything waiting for it — \
                 nothing in such a loop could ever start"
            )));
        }
        if seen.contains(&next) {
            continue;
        }
        seen.push(next);
        let onward: Vec<u64> =
            sqlx::query_scalar("SELECT blocked_on FROM task_blocks WHERE task_id = ?")
                .bind(next)
                .fetch_all(&mut **tx)
                .await
                .context("walking the blocking graph")?;
        queue.extend(onward);
    }
    Ok(())
}

/// Replace a task's blockers (the whole set) and say what moved.
async fn set_blockers(
    tx: &mut Transaction<'_, MySql>,
    id: u64,
    want: &[u64],
) -> Result<Option<String>> {
    let mut want: Vec<u64> = want.to_vec();
    want.sort_unstable();
    want.dedup();
    let mut have: Vec<u64> =
        sqlx::query_scalar("SELECT blocked_on FROM task_blocks WHERE task_id = ?")
            .bind(id)
            .fetch_all(&mut **tx)
            .await
            .context("reading the current blockers")?;
    have.sort_unstable();
    if have == want {
        return Ok(None);
    }
    no_cycle(tx, id, &want).await?;

    sqlx::query("DELETE FROM task_blocks WHERE task_id = ?")
        .bind(id)
        .execute(&mut **tx)
        .await
        .context("clearing the old blockers")?;
    for blocker in &want {
        sqlx::query("INSERT INTO task_blocks (task_id, blocked_on) VALUES (?, ?)")
            .bind(id)
            .bind(blocker)
            .execute(&mut **tx)
            .await
            .map_err(|e| unknown_blocker(e, *blocker))?;
    }
    let show = |ids: &[u64]| {
        if ids.is_empty() {
            "nothing".to_string()
        } else {
            ids.iter()
                .map(|x| format!("#{x}"))
                .collect::<Vec<_>>()
                .join(", ")
        }
    };
    Ok(Some(format!("{} → {}", show(&have), show(&want))))
}

/// A blocker id with no task, said plainly instead of a 500.
fn unknown_blocker(e: sqlx::Error, blocker: u64) -> AppError {
    if e.as_database_error()
        .is_some_and(|db| db.kind() == sqlx::error::ErrorKind::ForeignKeyViolation)
    {
        return AppError::BadRequest(format!("no task #{blocker} to be blocked on"));
    }
    AppError::Other(anyhow::Error::new(e).context("recording what blocks a task"))
}

/// The assignee foreign key's refusal as an answer the caller can act on.
///
/// Matched by kind, not constraint name: sqlx's MySQL driver doesn't expose the
/// name, and this statement has only one foreign key. Using the constraint, not
/// a `SELECT` first, saves a query and can't race.
fn unknown_holder(e: sqlx::Error, session: Option<&str>, doing: &'static str) -> AppError {
    let violated = e
        .as_database_error()
        .is_some_and(|db| db.kind() == sqlx::error::ErrorKind::ForeignKeyViolation);
    match session.filter(|_| violated) {
        Some(id) => AppError::BadRequest(format!(
            "no session `{id}` — a task can only be held by a conversation this \
             service has seen, and `task sessions --all` lists them"
        )),
        None => AppError::Other(anyhow::Error::new(e).context(doing)),
    }
}

/// An assignee's name for history, resolved at write time like the rest of the
/// line, so one conversation isn't its id on one line and its name the next.
async fn label_of(tx: &mut Transaction<'_, MySql>, assignee: &Assignee) -> Result<String> {
    let (AssigneeKind::Session, Some(id)) = (assignee.kind, assignee.id.as_deref()) else {
        return Ok(assignee.label());
    };
    let name: Option<(Option<String>,)> = sqlx::query_as("SELECT name FROM sessions WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .context("resolving a session name")?;
    // An unnamed session reads as its id.
    Ok(name
        .and_then(|(name,)| name)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| id.to_string()))
}

/// Write one history row and return its id, which `task_revision` hangs off.
async fn record(
    tx: &mut Transaction<'_, MySql>,
    task_id: u64,
    actor: &Actor,
    kind: Moved,
    detail: Option<String>,
) -> Result<u64> {
    let done = sqlx::query(
        "INSERT INTO task_events (task_id, actor_kind, actor_id, kind, detail) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(task_id)
    .bind(actor.kind())
    .bind(actor.id())
    .bind(kind.as_str())
    .bind(detail)
    .execute(&mut **tx)
    .await
    .context("recording a task event")?;
    Ok(done.last_insert_id())
}

/// The pile's stated reason, or the refusal; whitespace is no reason.
fn spare_note(kind: AssigneeKind, spare: Option<&str>) -> Result<Option<String>> {
    let said = spare.map(str::trim).filter(|why| !why.is_empty());
    // The verdict is shared with the CLI.
    let nobody = matches!(kind, AssigneeKind::Nobody);
    match crate::tasks::holder::pile_verdict(nobody, said.is_some()) {
        crate::tasks::holder::PileVerdict::Fits => Ok(said.map(str::to_string)),
        refused => Err(AppError::BadRequest(refused.said_to_api().into())),
    }
}

pub async fn create(pool: &MySqlPool, new: NewTask, actor: &Actor) -> Result<Task> {
    let subject = check_subject(&new.subject)?;
    // Filing a task takes it on unless told otherwise; the pile is chosen
    // (`--to nobody`), never fallen into. The holder is a foreign key, so callers
    // must `touch` the session first.
    let assignee = new.assignee.unwrap_or_else(|| actor_holder(actor));
    check_assignee(&assignee)?;
    let (kind, person, session) = assignee_columns(&assignee);
    // Before the transaction, so a refusal costs nothing.
    let spare = spare_note(kind, new.spare.as_deref())?;

    let mut tx = pool.begin().await.context("opening a transaction")?;
    let done = sqlx::query(
        "INSERT INTO tasks (subject, body, priority, due, assignee_kind, assignee_person, \
         assignee_session) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&subject)
    .bind(&new.body)
    .bind(new.priority.stored())
    .bind(new.due)
    .bind(kind.as_str())
    .bind(person)
    .bind(session)
    .execute(&mut *tx)
    .await
    .map_err(|e| unknown_holder(e, session, "filing a task"))?;
    let id = done.last_insert_id();
    record(&mut tx, id, actor, Moved::Created, Some(subject.clone())).await?;
    if !new.checked {
        // In the filing's transaction, so no override goes unrecorded.
        let named: Vec<String> = new
            .not_duplicate_of
            .iter()
            .map(|id| format!("#{id}"))
            .collect();
        let said = match named.as_slice() {
            [] => "filed past a duplicate refusal".to_string(),
            _ => format!("filed past a duplicate refusal naming {}", named.join(", ")),
        };
        record(&mut tx, id, actor, Moved::Overrode, Some(said)).await?;
    }
    if let Some(moved) = set_blockers(&mut tx, id, &new.blocked_on).await? {
        record(&mut tx, id, actor, Moved::Blocked, Some(moved)).await?;
        blocking_is_consistent(&mut tx, id, new.priority.stored(), new.due).await?;
    }
    // Recorded for the pile too, with its reason, so it can be counted.
    let note = match spare {
        Some(why) => format!("→ nobody: {why}"),
        None => format!("→ {}", label_of(&mut tx, &assignee).await?),
    };
    record(&mut tx, id, actor, Moved::Assigned, Some(note)).await?;
    tx.commit().await.context("committing a new task")?;

    list_one(pool, id).await
}

/// Who a write hands the task to when the caller didn't say. Pure, so the rules
/// can be asserted without a database (`tests/holder.rs` drives the whole path).
pub fn inferred_holder(before: &Task, change: &Change, actor: &Actor) -> Option<Assignee> {
    // Finishing claims it, on the way out of open only: reopening keeps the holder,
    // and done → dropped corrects a close.
    let finisher = (change.status.is_some_and(|status| !status.is_open())
        && before.status.is_open()
        && change.assignee.is_none())
    .then(|| actor_holder(actor));

    // Starting claims it, from the pile only (taking someone else's is `move`),
    // and regardless of status: a task handed back to the pile stays `doing`.
    let starter = (change.status == Some(Status::Doing)
        && before.assignee.kind == AssigneeKind::Nobody
        && change.assignee.is_none())
    .then(|| actor_holder(actor));

    finisher.or(starter)
}

/// Apply a partial change. Compares first: a change that alters nothing writes
/// no event, so the history holds only real moves.
pub async fn update(pool: &MySqlPool, id: u64, change: Change, actor: &Actor) -> Result<Updated> {
    let before = list_one(pool, id).await?;

    // A closed task may not go to the pile. Only an explicit `nobody` is refused:
    // a close from the pile is claimed by the finisher in `inferred_holder`.
    let would_be_closed = change
        .status
        .map_or(!before.status.is_open(), |status| !status.is_open());
    if would_be_closed
        && change
            .assignee
            .as_ref()
            .is_some_and(|a| a.kind == AssigneeKind::Nobody)
    {
        return Err(AppError::BadRequest(
            "a closed task cannot be handed to nobody: the pile is for work to pick up, and a \
             finished task with no holder reads as done by nobody in every list it appears in. \
             Close it and let it be yours, or name somebody"
                .into(),
        ));
    }

    let mut tx = pool.begin().await.context("opening a transaction")?;

    // What moved, beside each `record`.
    let mut changed: Vec<Moved> = Vec::new();

    // Read once, locked: the comparisons and the revision snapshot need it.
    //
    // ⚠ `FOR UPDATE`; a transaction alone isn't enough. Under REPEATABLE READ a
    // plain `SELECT` doesn't lock, and two concurrent additions would both build on
    // the old body. `tests/body_add.rs` drives that interleaving.
    let (was_subject, was_body): (String, String) =
        sqlx::query_as("SELECT subject, body FROM tasks WHERE id = ? FOR UPDATE")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .context("reading a task before changing it")?;

    // Before this update's own events.
    let wrote_it = last_written(&mut tx, id).await?;

    // The first `edited` event, which the revision hangs off; `None` until text moves.
    let mut edit_event: Option<u64> = None;

    if let Some(subject) = &change.subject {
        let subject = check_subject(subject)?;
        if subject != was_subject {
            sqlx::query("UPDATE tasks SET subject = ? WHERE id = ?")
                .bind(&subject)
                .bind(id)
                .execute(&mut *tx)
                .await
                .context("changing a subject")?;
            let event = record(&mut tx, id, actor, Moved::Edited, Some(subject)).await?;
            edit_event.get_or_insert(event);
            changed.push(Moved::Edited);
        }
    }

    // Additions resolve into `body` here, so the guard, the size line and the
    // snapshot treat them like any edit. An empty addition is refused: the command
    // meant to produce it produced nothing.
    for (flag, text) in [("--prepend", &change.prepend), ("--append", &change.append)] {
        if text.as_ref().is_some_and(|t| t.trim().is_empty()) {
            return Err(AppError::BadRequest(format!(
                "{flag} was given nothing to add, so the task is untouched. If that text \
                 came from a command or a variable, it is empty."
            )));
        }
    }

    let added = joined(
        &was_body,
        change.prepend.as_deref(),
        change.append.as_deref(),
    );
    let body = match (
        &change.body,
        change.prepend.is_some() || change.append.is_some(),
    ) {
        // Replacing and adding at once contradict each other.
        (Some(_), true) => {
            return Err(AppError::BadRequest(
                "--body replaces the whole body and --prepend/--append keep it, so sending \
                 both says two different things. Pick one."
                    .into(),
            ));
        }
        (Some(body), false) => Some(body.clone()),
        (None, true) => Some(added),
        (None, false) => None,
    };

    if let Some(body) = &body
        && body != &was_body
    {
        if !change.replace_body && collapses(&was_body, body) {
            let now = body.chars().count();
            return Err(AppError::BadRequest(format!(
                "that would leave {now} character{} of a {}-character body, so nothing was \
                 written. Read what is there with `task show {id} --body`. If the body really \
                 should go, say so with --replace-body.",
                if now == 1 { "" } else { "s" },
                was_body.chars().count()
            )));
        }
        sqlx::query("UPDATE tasks SET body = ? WHERE id = ?")
            .bind(body)
            .bind(id)
            .execute(&mut *tx)
            .await
            .context("changing a body")?;
        // A shrinking edit clears the sprawl flag, the step that resets `accreted`,
        // so flag and sampler agree. Same transaction, so no critique outlives its body.
        if body.chars().count() < was_body.chars().count() {
            sqlx::query("UPDATE tasks SET sprawl_said = NULL, sprawl_chars = NULL WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await
                .context("clearing a body's sprawl flag")?;
        }
        // The size goes in the row too, so the next reader sees a body was once emptied.
        let detail = format!(
            "body {} → {} chars",
            was_body.chars().count(),
            body.chars().count()
        );
        let event = record(&mut tx, id, actor, Moved::Edited, Some(detail)).await?;
        edit_event.get_or_insert(event);
        changed.push(Moved::Edited);
    }

    // One snapshot per update, not per event: `undo` restores a task as it stood,
    // not a subject from one moment and a body from another.
    if let Some(event) = edit_event {
        sqlx::query("INSERT INTO task_revision (event_id, subject, body) VALUES (?, ?, ?)")
            .bind(event)
            .bind(&was_subject)
            .bind(&was_body)
            .execute(&mut *tx)
            .await
            .context("recording what an edit replaced")?;
    }

    if let Some(status) = change.status
        && status != before.status
    {
        // `closed_at` moves with the status, decided by `Status::is_open` (exhaustive)
        // rather than SQL on the word, which would miss `dropped`.
        sqlx::query("UPDATE tasks SET status = ?, closed_at = IF(?, NOW(), NULL) WHERE id = ?")
            .bind(status)
            .bind(!status.is_open())
            .bind(id)
            .execute(&mut *tx)
            .await
            .context("changing a status")?;
        record(
            &mut tx,
            id,
            actor,
            Moved::Status,
            Some(format!("{} → {}", before.status, status)),
        )
        .await?;
        changed.push(Moved::Status);
    }

    // Compared before writing: re-ranking to the same rank writes nothing.
    // `unranked` is spelled out so the line shows the direction.
    if let Some(priority) = change.priority.filter(|p| Some(*p) != before.priority) {
        sqlx::query("UPDATE tasks SET priority = ? WHERE id = ?")
            .bind(priority)
            .bind(id)
            .execute(&mut *tx)
            .await
            .context("ranking a task")?;
        let was = before.priority.map_or("unranked", Priority::as_str);
        record(
            &mut tx,
            id,
            actor,
            Moved::Ranked,
            Some(format!("{was} → {priority}")),
        )
        .await?;
        changed.push(Moved::Ranked);
    }

    // `clear_due` wins over `due`: removing a claim is the safer reading.
    let due_change = if change.clear_due {
        Some(None)
    } else {
        change.due.map(Some)
    };
    if let Some(due) = due_change
        && due != before.due
    {
        sqlx::query("UPDATE tasks SET due = ? WHERE id = ?")
            .bind(due)
            .bind(id)
            .execute(&mut *tx)
            .await
            .context("setting a deadline")?;
        let show = |d: Option<NaiveDate>| d.map_or("none".to_string(), |d| d.to_string());
        record(
            &mut tx,
            id,
            actor,
            Moved::Due,
            Some(format!("{} → {}", show(before.due), show(due))),
        )
        .await?;
        changed.push(Moved::Due);
    }

    if let Some(want) = &change.blocked_on
        && let Some(moved) = set_blockers(&mut tx, id, want).await?
    {
        record(&mut tx, id, actor, Moved::Blocked, Some(moved)).await?;
        changed.push(Moved::Blocked);
    }

    // After both writes, once: rank and blockers are each legal only against the
    // other's new value. In the transaction, so a refusal rolls back both.
    if change.priority.is_some() || change.blocked_on.is_some() || due_change.is_some() {
        blocking_is_consistent(
            &mut tx,
            id,
            change.priority.or(before.priority),
            due_change.unwrap_or(before.due),
        )
        .await?;
    }

    // Who this lands on when unstated (`inferred_holder`).
    let inferred = inferred_holder(&before, &change, actor);

    if let Some(assignee) = change.assignee.as_ref().or(inferred.as_ref()) {
        check_assignee(assignee)?;
        let (kind, person, session) = assignee_columns(assignee);
        // As (kind, id), covering every move.
        let moved =
            (kind, assignee.id.as_deref()) != (before.assignee.kind, before.assignee.id.as_deref());
        if moved {
            sqlx::query(
                "UPDATE tasks SET assignee_kind = ?, assignee_person = ?, assignee_session = ? \
                 WHERE id = ?",
            )
            .bind(kind.as_str())
            .bind(person)
            .bind(session)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| unknown_holder(e, session, "moving a task"))?;
            let to = label_of(&mut tx, assignee).await?;
            record(
                &mut tx,
                id,
                actor,
                Moved::Assigned,
                Some(format!("{} → {to}", before.assignee.label())),
            )
            .await?;
            changed.push(Moved::Assigned);
        }
    }

    tx.commit().await.context("committing a task change")?;
    // Only where text moved and there is history. Uses the resolved `body`:
    // `change.body` is None for additions and would report no change.
    let now = body
        .as_deref()
        .map_or(was_body.chars().count(), |body| body.chars().count());
    let replaced = match edit_event.and(wrote_it) {
        Some((at, by)) => Some(Replaced {
            at,
            by,
            was: was_body.chars().count(),
            now,
            accreted: accreted(pool, id, now).await?,
        }),
        None => None,
    };

    let task = list_one(pool, id).await?;
    // The change has landed; a failed hint is not a failed close.
    let unwritten = if changed.contains(&Moved::Status) && !task.status.is_open() {
        crate::tasks::lifecycle::unwritten(pool, id)
            .await
            .unwrap_or_else(|why| {
                tracing::warn!("reading whether #{id} was closed unwritten: {why}");
                None
            })
    } else {
        None
    };
    Ok(Updated {
        task,
        changed,
        replaced,
        unwritten,
    })
}

/// Growth since the body last shrank, from `task_revision` (never the prose
/// history), measured with `CHAR_LENGTH` in SQL. It starts at the filed body: a
/// body written in one go is not accretion.
async fn accreted(pool: &MySqlPool, id: u64, now: usize) -> Result<usize> {
    let stored: Vec<i64> = sqlx::query_scalar(
        "SELECT CHAR_LENGTH(r.body) FROM task_revision r \
         JOIN task_events e ON e.id = r.event_id \
         WHERE e.task_id = ? ORDER BY e.id",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .context("reading what a body has grown from")?;

    let mut sizes: Vec<usize> = stored.into_iter().map(|n| n.max(0) as usize).collect();
    sizes.push(now);
    // Backwards: growth counts, the first shrink ends the run, and a zero step
    // (a subject-only edit) doesn't end it.
    let mut grown = 0;
    for step in sizes.windows(2).rev() {
        match step[1].checked_sub(step[0]) {
            Some(added) => grown += added,
            None => break,
        }
    }
    Ok(grown)
}

/// One task without prose or history: what every write reads first and returns.
async fn list_one(pool: &MySqlPool, id: u64) -> Result<Task> {
    let row: Option<Row> = sqlx::query_as(select!(" WHERE t.id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("reading a task")?;
    row.map(Row::into_task).ok_or(AppError::NotFound)
}

/// Render a body's markdown (tables, strikethrough and task lists are in use).
/// Raw HTML is escaped, not dropped, so it stays visible.
pub fn render_markdown(md: &str) -> String {
    let mut options = comrak::Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.render.escape = true;
    comrak::markdown_to_html(md, &options)
}
