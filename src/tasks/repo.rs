//! Reading and writing tasks.
//!
//! Every write records a `task_events` row in the same transaction as the
//! change it describes: the history is not a log beside the data, and one that
//! can be absent for a write nobody noticed is not a history.

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
    /// Include closed tasks — done and dropped alike. Every injected path leaves
    /// it off: see the invariant in `lib.rs`.
    pub include_closed: bool,
    /// Only tasks held by this session id.
    pub session: Option<String>,
    /// Only tasks held by this person (Nextcloud user id).
    pub person: Option<String>,
    /// Widen [`session`](Self::session) to *and the ones nobody holds*.
    ///
    /// Ignored unless `session` is set. Why the digest needs it:
    /// [`digest_for`](Self::digest_for).
    pub or_unheld: bool,
    /// Strictly the tasks nobody holds — the pile, and nothing else.
    ///
    /// The narrow twin of [`or_unheld`](Self::or_unheld), which widens. Wins
    /// over `session` and `person`: an intersection with a holder would always
    /// be empty.
    pub unheld: bool,
    /// Tasks this session FILED and does not hold — what it handed out.
    ///
    /// ⚠ **The one question here not about the holder**: it narrows by who
    /// *wrote* a task and subtracts the writer's own plate. Held by anybody
    /// else, the pile included. The digest must not use this — it would show
    /// what another conversation holds.
    ///
    /// Wins over the three above: it has answered the holder half itself.
    pub handed_out_by: Option<String>,
}

impl Filter {
    /// What a session's digest asks for: its own open tasks and the pile.
    ///
    /// ⚠ **`or_unheld` is the half that must not be dropped.** The pile is the
    /// handover channel: strictly *mine* makes a task left for whichever
    /// conversation is around invisible to all of them at once. Looking across
    /// holders is something to ask for: `task list --all`.
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
    /// `P0` when a near deadline raises this task, else NULL — see `due_soon!`.
    escalated_to: Option<Priority>,
    /// Whether `due` has passed, by the DATABASE's clock — see the projection.
    past_due: i8,
    /// The blockers, comma-joined by SQL — see the projection for why.
    blocked_on: Option<String>,
    /// How many of them are still open, counted in SQL like `detailed`.
    open_blockers: i64,
    assignee_kind: AssigneeKind,
    assignee_person: Option<String>,
    assignee_session: Option<String>,
    /// Resolved through the join, so a list never needs a second query per row.
    session_name: Option<String>,
    /// Whether the body has anything in it. Computed in SQL, so a list does not
    /// carry every body across the wire to answer a boolean.
    detailed: i8,
    /// How many lines the body prints as. Counted in SQL for the reason
    /// `detailed` is, and over the TRIMMED body — see the projection.
    body_lines: i64,
    /// How long the body was when a model last read it and spoke. NULL when
    /// nothing is outstanding — see [`Task::sprawl_chars`]. The words it said
    /// are deliberately NOT here: a list must not carry prose.
    sprawl_chars: Option<u32>,
    /// The filing session's current name, or `NULL` where there is nothing to
    /// say — see [`Task::filed_by`].
    filed_by: Option<String>,
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

/// The task projection, with a literal tail appended.
///
/// A macro rather than a `const` + `format!` because sqlx accepts only
/// `&'static str`: `concat!` keeps every query a literal the compiler
/// assembled, so there is no runtime-built string to audit.
macro_rules! select {
    ($tail:literal) => {
        concat!(
            "SELECT t.id, t.subject, t.status, t.priority, t.due, ",
            // The VALUE it sorts as — see `Task::escalated_to`. NULL when the
            // stored rank is already `P0`: nothing was raised.
            "IF(",
            due_soon!("t.due"),
            " AND COALESCE(t.priority, 'P2') > 'P0', ",
            "'P0', NULL) AS escalated_to, ",
            // One clock — the database's — so the CLI, the app and the digest
            // cannot disagree about which day it is. `CURDATE()` is the session
            // zone, pinned to UTC in `db::connect`.
            "(t.due IS NOT NULL AND t.due < CURDATE()) AS past_due, ",
            "t.assignee_kind, ",
            // Correlated subqueries rather than a join: a join to `task_blocks`
            // MULTIPLIES the task row by its edges, and a list must not gain
            // rows. Covered by the table's primary key.
            //
            // ⚠ GROUP_CONCAT is capped by `group_concat_max_len` (1024 bytes by
            // default, ~140 ids); the count beside it is exact regardless, so
            // `blocked` stays right even if the list were clipped.
            "(SELECT GROUP_CONCAT(b.blocked_on ORDER BY b.blocked_on) FROM task_blocks b ",
            "WHERE b.task_id = t.id) AS blocked_on, ",
            "(SELECT COUNT(*) FROM task_blocks b JOIN tasks bt ON bt.id = b.blocked_on ",
            "WHERE b.task_id = t.id AND ",
            still_open!("bt.status"),
            ") AS open_blockers, ",
            "t.assignee_person, t.assignee_session, s.name AS session_name, ",
            // Both are about the body a reader SEES, so both go through
            // `body_shown!`.
            "(LENGTH(",
            body_shown!("t.body"),
            ") > 0) AS detailed, t.sprawl_chars, ",
            // ⚠ **Counted the way `show` PRINTS it.** Newlines by subtraction:
            // `LENGTH` is bytes, but a difference over a one-byte needle is an
            // exact newline count whatever multi-byte text is around it.
            // `CHAR(10)` rather than `'\n'` for the escaping reason `body_shown!`
            // gives; `+ 1` because a trimmed body's last line has no newline.
            // `CAST(... AS SIGNED)`: unsigned arithmetic comes back as `BIGINT
            // UNSIGNED`, which sqlx refuses as `i64` — at runtime, on real rows.
            "CAST(IF(LENGTH(",
            body_shown!("t.body"),
            ") = 0, 0, LENGTH(",
            body_shown!("t.body"),
            ") - LENGTH(REPLACE(",
            body_shown!("t.body"),
            ", CHAR(10), '')) + 1) AS SIGNED) AS body_lines, ",
            // Correlated, as above; `LIMIT 1` makes gaining rows impossible.
            // Covered by `idx_task_events_task (task_id, at)`.
            "(SELECT f.name FROM task_events c JOIN sessions f ON f.id = c.actor_id ",
            "WHERE c.task_id = t.id AND c.kind = 'created' AND c.actor_kind = 'session' ",
            "ORDER BY c.id LIMIT 1) AS filed_by, ",
            "t.created_at, t.updated_at, t.closed_at ",
            "FROM tasks t LEFT JOIN sessions s ON s.id = t.assignee_session",
            $tail
        )
    };
}

/// Tasks matching a filter: by priority, then oldest id first.
///
/// ⚠ **Ordered by id and NOT by status**: a list that re-sorts as work starts
/// moves the line somebody was reading. Oldest first, so old tickets get fixed
/// rather than buried under this morning's.
///
/// ⚠ **Priority is the one thing that reorders, because somebody stated it.**
/// `COALESCE(t.priority, 'P2')`, the mirror of
/// [`Priority::rank`](crate::tasks::types::Priority::rank): `P0`/`P1` rise
/// above the unranked, `P3`/`P4` sink below.
///
/// ⚠ **The only sort in the service**: `digest::render` preserves the order it
/// is handed.
pub async fn list(pool: &MySqlPool, filter: &Filter) -> Result<Vec<Task>> {
    let mut query = QueryBuilder::<MySql>::new(select!(""));
    query.push(" WHERE 1 = 1");
    if !filter.include_closed {
        query.push(concat!(" AND ", still_open!("t.status")));
    }
    // Exclusive, in the precedence the `Filter` fields document.
    if let Some(filer) = &filter.handed_out_by {
        // The `filed_by` subquery, compared on the actor id rather than the
        // name, which a rename changes.
        query.push(concat!(
            " AND (SELECT c.actor_id FROM task_events c ",
            "WHERE c.task_id = t.id AND c.kind = 'created' AND c.actor_kind = 'session' ",
            "ORDER BY c.id LIMIT 1) = "
        ));
        query.push_bind(filer);
        // Handed OUT: still on the filer's own plate is not handed anywhere.
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
    // The sort key is the EFFECTIVE rank: inside the week a deadline raises a
    // task to `P0`. Pippijn set that rule; otherwise a date reorders nothing.
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

/// One task, its prose and its history. `None` when there is no such task.
pub async fn get(pool: &MySqlPool, id: u64) -> Result<Option<TaskDetail>> {
    // `select!` expands to `concat!`, so rustc sees a literal; only the linter
    // sees a macro.
    // dev-lint: allow-sqlx — a `concat!`ed literal, not a runtime-built string.
    let row: Option<Row> = sqlx::query_as(select!(" WHERE t.id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("reading a task")?;
    let Some(row) = row else {
        return Ok(None);
    };
    // The critique rides along with the body it is about: one read, and the one
    // place prose is allowed to cross the wire.
    let body: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT body, sprawl_said FROM tasks WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
            .context("reading a task body")?;
    let (body, sprawl_said) = body.unwrap_or_default();
    let events = events(pool, id).await?;
    // A boolean answered in SQL, like `detailed`: fetching the revision would
    // carry a second body to every reader.
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
            actor: actor_label(row.actor_name, row.actor_id, row.actor_kind),
        })
        .collect())
}

/// Name, then id, then the bare kind.
///
/// Each fallback is a real state: a session that never named itself, and — for
/// a person — no `sessions` row to join to at all.
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

/// The task as it stood before its most recent edit.
///
/// `None` for a task nothing has overwritten, and for one predating revisions —
/// not worth distinguishing, since in both there is nothing to put back.
///
/// ⚠ **Newest by `event_id`, not by `at`.** `task_events.at` is `DATETIME` at
/// one-second resolution and a subject sweep writes several edits inside one
/// second, so ordering by time picks an arbitrary one as "the last". The id is
/// the only total order there is.
///
/// ⚠ **Takes the asker**, because what a caller needs before restoring is
/// whether the edit it would revert is its own — see [`Revision::mine`]. The
/// only identity a client has is the rendered label.
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
        // Both halves, or a person and a session sharing an id would read as
        // each other. `actor_id` is NULL only for rows written before the
        // column existed, and an absent id matches nobody.
        mine: row.actor_kind == who.kind() && row.actor_id.as_deref() == Some(who.id()),
        actor: actor_label(row.actor_name, row.actor_id, row.actor_kind),
        subject: row.subject,
        body: row.body,
    }))
}

/// When the text an edit is about to replace was last written, and by whom.
///
/// `created` counts, so a body nobody has edited still has provenance — filing
/// a task is how its first body got there.
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
    // ⚠ **Bound from [`Moved`], not spelled**: a spelled copy fails no test
    // when a variant is renamed, and silently stops matching.
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

/// A task being filed.
///
/// ⚠ **The `skip_serializing_if` attributes change no behaviour** — this struct
/// is never serialised. They state the shape the *client* sends, which
/// dev-lint's wire-mirror check compares `frontend/src/app/models.ts` against;
/// without them it would read "always present" and ask the client to send a
/// key whose absence is the meaning.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct NewTask {
    pub subject: String,
    #[serde(default)]
    pub body: String,
    /// Whether the filer let the duplicate check run.
    ///
    /// ⚠ **Absent means "it ran", so an old client is not silently exempted**
    /// from the refusal of a pre-emptive `--no-duplicate-check`. `false` has to
    /// be SAID.
    #[serde(default = "yes")]
    pub checked: bool,
    /// How urgent — and **the one field a filer may not leave out**.
    ///
    /// ⚠ **No `serde(default)`, and no `skip_serializing_if`, deliberately**: a
    /// missing key is refused, and `"P2"` and `null` (*unassessed*) are both
    /// things somebody SAID — see [`Ranking`]. Carrying no attributes tells the
    /// mirror check the key is present and nullable, not optional.
    pub priority: Ranking,
    /// The day it has to be done by, if something outside already decides it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<NaiveDate>,
    /// The tasks this one waits for, if they are known at filing time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_on: Vec<u64>,
    /// Who it is for. ⚠ **Absent means the filer**, not the pile — see [`create`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<Assignee>,
    /// Why this belongs to nobody — **required when, and only when, it does**.
    ///
    /// ⚠ **Filings to the pile were routinely corrected later**, so the pile is
    /// argued for rather than typed. With a holder this is refused: a reason
    /// for the pile says nothing about a task on somebody's plate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spare: Option<String>,
}

/// Both answers, because naming only the first reads as *you must pick a
/// level*. See [`crate::wire`].
impl crate::wire::RequiredKeys for NewTask {
    fn required() -> &'static [(&'static str, &'static str)] {
        &[(
            "priority",
            r#""P0" to "P4" if you have judged it, or null for unassessed if nobody has"#,
        )]
    }
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
    /// Text to put ABOVE the body there already is, keeping all of it.
    ///
    /// ⚠ **This is the field that stops the mistake `body` invites.** Without it,
    /// adding a paragraph means reading the body out, concatenating by hand and
    /// sending it all back; skip the read and the paragraph becomes the whole
    /// body.
    ///
    /// ⚠ **Above rather than below is the ordinary case** — a body grows in the
    /// order things happened, so what is still true sinks out of sight.
    ///
    /// **Resolved inside the transaction that reads the body**, so two
    /// conversations adding to one task seconds apart cannot lose each other's
    /// text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepend: Option<String>,
    /// Text to put BELOW the body there already is, keeping all of it.
    ///
    /// The twin of [`prepend`](Self::prepend). Both may be sent at once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub append: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    /// ⚠ **This cannot CLEAR a priority**: absent is leave-alone, and a
    /// meaningful null (`Option<Option<_>>`) makes every client guess. A wrong
    /// rank is corrected by ranking again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<NaiveDate>,
    /// Take the deadline off.
    ///
    /// ⚠ **An explicit flag, where `blocked_on` needs none**: an empty list is a
    /// value meaning *nothing blocks this*, while every date is a real deadline.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub clear_due: bool,
    /// The blockers as they should now be — the whole set, not an addition.
    ///
    /// ⚠ **An empty list is how a task stops being blocked**; absence still
    /// means leave it alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_on: Option<Vec<u64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<Assignee>,
    /// Say that a body which keeps almost nothing of the one it replaces is
    /// meant. See `collapses` for what is refused without it, and why.
    ///
    /// ⚠ **Undo sets this, and has to**: undoing an edit that *grew* a body
    /// restores a shorter one, the shape the guard refuses — and an undo has
    /// just read the text it restores.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replace_body: bool,
}

/// Nothing is required of a change, and taking the default is how that is said.
///
/// ⚠ **Not an oversight — do not copy `NewTask`'s list.** A change means *leave
/// alone what I did not mention*; requiring a rank here would make every edit
/// restate it, and a rank would stop being something somebody said.
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
    // A newline in a subject would split one task across two lines of an
    // injected digest, where every line is read as its own task.
    if subject.contains(['\n', '\r']) {
        return Err(AppError::BadRequest(
            "a subject is one line — put the detail in the body".into(),
        ));
    }
    Ok(subject.to_string())
}

/// The size below which a body is not worth guarding at all.
///
/// ⚠ **Deliberately far from anything a real rewrite does**, as is
/// [`KEEPS_AT_LEAST`]. The point is not to catch every loss — it is to catch the
/// writes that were never edits at all, without asking anybody about the
/// ordinary ones.
const WORTH_GUARDING: usize = 500;

/// The share of a guarded body an edit has to leave standing — a quarter.
///
/// ⚠ **A rewrite that keeps half is NOT caught.** This is a backstop against
/// truncation, not against a confident overwrite; `task undo` covers the rest.
const KEEPS_AT_LEAST: usize = 4;

/// Whether a new body keeps so little of the old one that it is more likely a
/// mistake than a rewrite.
///
/// ⚠ **This gates edits, which [`crate::tasks::undo`] argues against for
/// ordinary ones** — gating a frequent correct operation teaches everyone to
/// pass gates. A long body becoming a single word is neither frequent nor
/// correct: it is a session that read `--json`, took `detailed` for the prose,
/// and wrote the boolean over the lot.
///
/// ⚠ **Must NOT be tightened.** Genuine conclusion-on-top rewrites — what
/// `task edit --help` asks for — can keep as little as a third, so a higher cut
/// fires on more correct edits than mistakes. The middle band is answered by
/// [`Change::prepend`], which removes the need to rewrite.
fn collapses(was: &str, now: &str) -> bool {
    let (was, now) = (was.chars().count(), now.chars().count());
    was > WORTH_GUARDING && now * KEEPS_AT_LEAST < was
}

/// What a body becomes once something is added above or below it.
///
/// ⚠ **Exactly one blank line at each seam, however many newlines were there.** Not
/// cosmetic: markdown joins adjacent lines into one paragraph, so a one-line note
/// butted onto a body that opens with prose becomes the first sentence of it, and the
/// two read as one claim.
///
/// ⚠ **Newlines only, never spaces or tabs.** Whitespace inside a line is markdown
/// content — two trailing spaces are a hard line break, leading spaces set indentation
/// and continuation — so a plain `trim` would change what a body SAYS through a command
/// that promised to keep it.
///
/// An empty body takes the addition alone, with no seam to normalise: prepending to a
/// task filed with no prose must not leave it starting with a blank line.
///
/// ⚠ **The existing body is trimmed only on a side that gained a neighbour.** Trimming
/// both ends would let an `append` quietly restyle the TOP of a body it never touched.
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
        // A part that is only whitespace contributes nothing but would still
        // open a seam, leaving the blank line it was supposed to prevent.
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

/// Whoever is asking, as a holder.
///
/// Filing, starting and closing all infer a holder this way. The name is left
/// empty: it is resolved through the session join on the way back out.
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

/// The comma-joined ids `GROUP_CONCAT` returns, as numbers.
///
/// Anything unparseable is dropped rather than defaulted: these are foreign keys
/// the database itself produced, so a non-number here means the query changed,
/// and inventing a `0` would point at a task that cannot exist.
fn parse_ids(joined: Option<&str>) -> Vec<u64> {
    joined
        .unwrap_or("")
        .split(',')
        .filter_map(|id| id.trim().parse().ok())
        .collect()
}

/// A blocked task may be ranked the same as what blocks it, never higher.
/// Checked at both ends.
///
/// A task you cannot start must not claim to be the next thing anybody does —
/// the move that inflates a scale, and one a machine can catch.
///
/// ⚠ **The bound is the LEAST urgent open blocker**, since that is what decides when
/// this can actually start: blocked on a `P1` and a `P3`, a task waits for the `P3`.
///
/// ⚠ **Both ends, because either edit can break it**: ranking the blocked task
/// up, or ranking a BLOCKER *down* below what waits on it.
///
/// ⚠ **Refused, not cascaded.** Quietly demoting whatever waits on a demoted blocker
/// would edit rows nobody asked about. The refusal names both tasks, so whoever is
/// deciding sees the pair.
///
/// ⚠ **Only while the blocker is OPEN**, or a finished dependency would hold a
/// rank down for ever.
///
/// ⚠ **The rule binds a CLAIM, so an unranked task is never in violation.**
/// Otherwise *"this waits for that"* could not be recorded until the waiting
/// task was ranked. A BLOCKER's absent rank still counts as `P2`: the claim
/// above it has to clear something.
async fn blocking_is_consistent(
    tx: &mut Transaction<'_, MySql>,
    id: u64,
    priority: Option<Priority>,
    due: Option<NaiveDate>,
) -> Result<()> {
    // ⚠ **The deadline twin of the rank rule**: a task cannot be finished
    // before what it waits for, so a due date earlier than an open blocker's is
    // arithmetic, not a judgement. Equal is allowed. Checked before the rank,
    // because it is the harder fact.
    if let Some(mine) = due {
        // dev-lint: allow-sqlx — a `concat!`ed literal; see `get`.
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
    // And the other end: a blocker pushed out past something waiting on it.
    // dev-lint: allow-sqlx — a `concat!`ed literal; see `get`.
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
        // Nothing claimed at this end. Whatever is blocked ON this task is still
        // checked below only where IT has stated something.
        return unblocked_end(tx, id, None).await;
    };
    let mine = Priority::rank(Some(stated));

    // This end: what this task waits for, of those still open.
    // dev-lint: allow-sqlx — a `concat!`ed literal; see `get`.
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

/// The other end: what waits for this task, and whether demoting it breaks them.
///
/// Split out so the early return above reaches it: an unranked task can still
/// be the blocker of one that is ranked.
async fn unblocked_end(
    tx: &mut Transaction<'_, MySql>,
    id: u64,
    priority: Option<Priority>,
) -> Result<()> {
    let mine = Priority::rank(priority);
    // dev-lint: allow-sqlx — a `concat!`ed literal; see `get`.
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
        // Same asymmetry: an untriaged dependent has claimed nothing, so there
        // is nothing for this demotion to contradict.
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

/// Refuse a set of blockers that cannot all be satisfied.
///
/// ⚠ **In a cycle nothing could ever start.** The whole graph is walked, not one
/// step, because `A → B → C → A` arrives as three separate edits. Each task is
/// visited at most once.
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

/// Replace a task's blockers, and say what moved.
///
/// The whole set is written, not added to — see [`Change::blocked_on`].
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

/// A blocker id with no task behind it, said plainly instead of as a 500.
///
/// As [`unknown_holder`]: on this statement one row can be missing.
fn unknown_blocker(e: sqlx::Error, blocker: u64) -> AppError {
    if e.as_database_error()
        .is_some_and(|db| db.kind() == sqlx::error::ErrorKind::ForeignKeyViolation)
    {
        return AppError::BadRequest(format!("no task #{blocker} to be blocked on"));
    }
    AppError::Other(anyhow::Error::new(e).context("recording what blocks a task"))
}

/// The assignee foreign key's refusal, turned into an answer the caller can act
/// on. Any other failure keeps `doing` as its context and stays a 500.
///
/// ⚠ **Discriminated by KIND, not the constraint's NAME**: sqlx's MySQL driver
/// answers `DatabaseError::constraint()` with `None`, so the name is only in
/// message text MariaDB may reword. On a statement writing `assignee_session`
/// there is exactly one foreign key it can be.
///
/// ⚠ **The constraint's answer, not a `SELECT` first**, which would cost a
/// query on every move and still race.
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

/// What to call an assignee in the history, resolved against the session table.
///
/// ⚠ **Both sides of an arrow need the same kind of name.** A caller sends a
/// kind and an id, while the task read back through the join carries a name;
/// [`Assignee::label`] on both would write one conversation as its id on one
/// line and its name on the next. The detail is rendered at write time, so the
/// name is resolved at write time too.
async fn label_of(tx: &mut Transaction<'_, MySql>, assignee: &Assignee) -> Result<String> {
    let (AssigneeKind::Session, Some(id)) = (assignee.kind, assignee.id.as_deref()) else {
        return Ok(assignee.label());
    };
    let name: Option<(Option<String>,)> = sqlx::query_as("SELECT name FROM sessions WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .context("resolving a session name")?;
    // A session nobody has named reads as its id, as in the list.
    Ok(name
        .and_then(|(name,)| name)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| id.to_string()))
}

/// Write one history row, and say which one it was.
///
/// The id is returned because `task_revision` hangs off it: what an edit
/// replaced is stored against the event that recorded the edit.
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

/// The pile's stated reason, and the refusal when the two do not match.
///
/// Whitespace is not a reason: `--spare " "` would pass a presence check while
/// arguing nothing.
fn spare_note(kind: AssigneeKind, spare: Option<&str>) -> Result<Option<String>> {
    let said = spare.map(str::trim).filter(|why| !why.is_empty());
    // The verdict is shared with the CLI; only the words are this side's.
    let nobody = matches!(kind, AssigneeKind::Nobody);
    match crate::tasks::holder::pile_verdict(nobody, said.is_some()) {
        crate::tasks::holder::PileVerdict::Fits => Ok(said.map(str::to_string)),
        refused => Err(AppError::BadRequest(refused.said_to_api().into())),
    }
}

pub async fn create(pool: &MySqlPool, new: NewTask, actor: &Actor) -> Result<Task> {
    let subject = check_subject(&new.subject)?;
    // Filing a task takes it on, unless the caller says where it goes —
    // Pippijn's rule: a task a session deals with is that session's. The pile is
    // something said (`--to nobody`), not fallen into.
    //
    // ⚠ **A filing session needs a row in `sessions`**: the default holder is a
    // foreign key. The write routes `touch` before reaching here; a caller below
    // them (a test, an import) must too. Falling back to the pile would hide a
    // conversation's work rather than fail.
    let assignee = new.assignee.unwrap_or_else(|| actor_holder(actor));
    check_assignee(&assignee)?;
    let (kind, person, session) = assignee_columns(&assignee);
    // Before the transaction opens, so a refused filing costs no database work.
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
    if let Some(moved) = set_blockers(&mut tx, id, &new.blocked_on).await? {
        record(&mut tx, id, actor, Moved::Blocked, Some(moved)).await?;
        blocking_is_consistent(&mut tx, id, new.priority.stored(), new.due).await?;
    }
    // ⚠ **Recorded for the pile too**, with its reason, so how often the pile
    // is chosen — and why — can be counted rather than inferred.
    let note = match spare {
        Some(why) => format!("→ nobody: {why}"),
        None => format!("→ {}", label_of(&mut tx, &assignee).await?),
    };
    record(&mut tx, id, actor, Moved::Assigned, Some(note)).await?;
    tx.commit().await.context("committing a new task")?;

    list_one(pool, id).await
}

/// Who a write hands the task to, when the caller did not say.
///
/// ⚠ **Pure, and out here on purpose**: `update` needs a real database, and
/// these rules have regressed before. `tests/holder.rs` drives the whole path;
/// this lets the decision be asserted without one.
///
/// An explicit `change.assignee` always wins and is not considered here: a
/// caller naming where a task should go is more specific than any inference.
pub fn inferred_holder(before: &Task, change: &Change, actor: &Actor) -> Option<Assignee> {
    // FINISHING a task claims it.
    // Only on the way OUT of open — reopening leaves the holder alone, because
    // the last person to touch it is a better guess than nobody, and done →
    // dropped is a correction to a closed task rather than a new closing.
    let finisher = (change.status.is_some_and(|status| !status.is_open())
        && before.status.is_open()
        && change.assignee.is_none())
    .then(|| actor_holder(actor));

    // Starting a task claims it too — the same rule read at the other end.
    //
    // Without it the holder column would only say who finished a task, never
    // who is carrying it.
    //
    // ⚠ **Out of the PILE only.** Guarding on the status instead — claim unless
    // already `doing` — would let any session running `start` silently take a
    // task handed to another. Taking somebody else's is a handover: `move`.
    //
    // ⚠ **And it does not read the status at all.** A task handed back to the
    // pile stays `doing`, so `&& status != Doing` would make `start` report
    // success and move nobody. `update` already writes nothing when the holder
    // does not change.
    let starter = (change.status == Some(Status::Doing)
        && before.assignee.kind == AssigneeKind::Nobody
        && change.assignee.is_none())
    .then(|| actor_holder(actor));

    finisher.or(starter)
}

/// Apply a partial change.
///
/// ⚠ **Reads the row first and compares.** A change that alters nothing writes
/// no event: a client that PUTs the whole object on every keystroke would
/// otherwise fill the history with `open → open`, and a history full of
/// non-events is one nobody reads.
pub async fn update(pool: &MySqlPool, id: u64, change: Change, actor: &Actor) -> Result<Updated> {
    let before = list_one(pool, id).await?;

    // ⚠ **A closed task may not be left in the pile**: nobody picks up a
    // finished task, and it would read as done by nobody.
    //
    // **Only an EXPLICIT `nobody` is refused.** Closing without naming anybody
    // is claimed by the finisher in `inferred_holder`; testing the effective
    // holder here, before inference, would refuse every close from the pile.
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

    // What actually moved, pushed beside each `record`: the event rows ARE the
    // answer.
    let mut changed: Vec<Moved> = Vec::new();

    // ⚠ **Read ONCE, inside the transaction, whether or not it will change**:
    // the comparisons need it, and so does the revision snapshot, which holds
    // the *whole* previous task even when one column moved.
    //
    // ⚠ **`FOR UPDATE`; a transaction alone is NOT enough.** `prepend` and
    // `append` build the new body from this read, and under REPEATABLE READ a
    // plain `SELECT` does not lock: two conversations adding to one task would
    // both read the old body, and the second commit would drop the first's
    // text. `tests/body_add.rs` drives that interleaving by hand.
    let (was_subject, was_body): (String, String) =
        sqlx::query_as("SELECT subject, body FROM tasks WHERE id = ? FOR UPDATE")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .context("reading a task before changing it")?;

    // Asked before this update writes its own events, or it would answer with
    // the edit currently being made.
    let wrote_it = last_written(&mut tx, id).await?;

    // The event a revision hangs off: the first `edited` this update writes.
    // `None` until something textual actually moves, which is also the test for
    // whether a snapshot is owed at all.
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

    // Adding to a body is resolved into `body` here, against the text just
    // read, so everything downstream — the collapse guard, the `N → M chars`
    // line, the revision snapshot — treats it like any other edit.
    //
    // ⚠ **Adding nothing is refused, not quietly ignored.** An empty addition
    // means the heredoc, variable or command meant to produce the text
    // produced none, and reporting success would hide that.
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
        // Both spellings at once contradict each other — one replaces the body
        // and the other keeps it — so this is refused rather than ordered.
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
        // ⚠ **An edit that makes the body SMALLER clears the sprawl flag**, the
        // same step that resets `accreted`, so the flag and the sampler agree on
        // what consolidating is. A quiet density read also clears it, but that
        // cannot be summoned — see `checks::remember`. In the same transaction,
        // so no critique outlives the body it was about.
        if body.chars().count() < was_body.chars().count() {
            sqlx::query("UPDATE tasks SET sprawl_said = NULL, sprawl_chars = NULL WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await
                .context("clearing a body's sprawl flag")?;
        }
        // ⚠ **The size of the change belongs in the row, not only in the reply.**
        // The reply goes with the scrollback of whoever made the edit; stored,
        // it is what tells the next reader that a body was once emptied.
        let detail = format!(
            "body {} → {} chars",
            was_body.chars().count(),
            body.chars().count()
        );
        let event = record(&mut tx, id, actor, Moved::Edited, Some(detail)).await?;
        edit_event.get_or_insert(event);
        changed.push(Moved::Edited);
    }

    // ⚠ **One snapshot per update, not per event.** A `task edit --subject
    // --body` writes two history rows and must leave ONE previous version
    // behind: `task undo` restores a task as it stood, and two half-revisions
    // would make it restore a subject from one moment and a body from another.
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
        // `closed_at` moves with the status in the same statement. The
        // condition comes from `Status::is_open`, whose `match` is exhaustive,
        // not SQL against the word: `IF(? = 'done', …)` would leave a dropped
        // task with no closing time.
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

    // ⚠ **Compared before writing, like every other field here.** Re-ranking a
    // task to what it already is must write no event: the history is read, and
    // one full of `P2 → P2` is one nobody reads. `unranked` is spelled out
    // rather than left blank so the line says which direction the change went.
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

    // `clear_due` wins over `due`: a caller sending both has contradicted
    // itself, and taking the deadline off is the safer reading — it removes a
    // claim rather than asserting one.
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

    // ⚠ **After BOTH writes, and once — not inside either.** A change may move
    // the rank and the blockers together, and each is legal only against the
    // NEW value of the other: ranking to `P1` while also pointing at a `P1`
    // blocker is the case. Checking as we go would refuse a change that is
    // consistent the moment it lands. Inside the transaction, so a refusal rolls
    // the whole thing back rather than leaving half of it written.
    if change.priority.is_some() || change.blocked_on.is_some() || due_change.is_some() {
        blocking_is_consistent(
            &mut tx,
            id,
            change.priority.or(before.priority),
            due_change.unwrap_or(before.due),
        )
        .await?;
    }

    // Who this lands on when the caller did not say — see `inferred_holder`.
    let inferred = inferred_holder(&before, &change, actor);

    if let Some(assignee) = change.assignee.as_ref().or(inferred.as_ref()) {
        check_assignee(assignee)?;
        let (kind, person, session) = assignee_columns(assignee);
        // Compared as (kind, id), which is what the three columns encode, so
        // this covers every move, including session to session.
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
    // Only where text moved, and only where the task had a history to name.
    //
    // ⚠ **`now` is the RESOLVED `body`, not `change.body`**, which is None on
    // every `--prepend`/`--append` and would report a large addition as no
    // change — wrong in the reassuring direction.
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

    Ok(Updated {
        task: list_one(pool, id).await?,
        changed,
        replaced,
    })
}

/// How much this body has grown since anything last made it smaller.
///
/// ⚠ **Read out of `task_revision`, not parsed out of the history.**
/// `task_events.detail` is a line rendered for a person, and parsing it would
/// make that sentence a wire format. Sizes are `CHAR_LENGTH` in SQL, so no body
/// is loaded to be measured.
///
/// The count starts at the oldest revision, the task as FILED: a body written
/// in one go is not accretion, however long.
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
    // Backwards from now: every step that added text counts, and the first that
    // removed any ends the run — a rewrite has read what it swept up. A step of
    // ZERO does not end it: a subject-only edit stores a revision too, and must
    // not clear the count.
    let mut grown = 0;
    for step in sizes.windows(2).rev() {
        match step[1].checked_sub(step[0]) {
            Some(added) => grown += added,
            None => break,
        }
    }
    Ok(grown)
}

/// One task without its prose or history — the read every write does first,
/// and the value every write returns.
async fn list_one(pool: &MySqlPool, id: u64) -> Result<Task> {
    // dev-lint: allow-sqlx — a `concat!`ed literal; see `get`.
    let row: Option<Row> = sqlx::query_as(select!(" WHERE t.id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("reading a task")?;
    row.map(Row::into_task).ok_or(AppError::NotFound)
}

/// Render a task body's markdown.
///
/// Tables, strikethrough and task lists are all in use in task prose. Raw HTML
/// renders escaped rather than dropped, so the text stays visible.
pub fn render_markdown(md: &str) -> String {
    let mut options = comrak::Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.render.escape = true;
    comrak::markdown_to_html(md, &options)
}
