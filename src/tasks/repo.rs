//! Reading and writing tasks.
//!
//! Every write records a [`task_events`] row in the same transaction as the
//! change it describes, because the history is not a log beside the data — it
//! is the part of the old file scheme that git used to provide, and a history
//! that can be absent for a write nobody noticed is not one.

use anyhow::Context;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use sqlx::{MySql, MySqlPool, QueryBuilder, Transaction};

use crate::error::AppError;
use crate::tasks::types::{
    Actor, Assignee, AssigneeKind, Event, MAX_SUBJECT, Priority, Ranking, Status, Task, TaskDetail,
    Updated,
};
use crate::{due_soon, still_open};

type Result<T> = std::result::Result<T, AppError>;

/// Which tasks a caller is asking for.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Include closed tasks — both the done and the dropped. Off by default,
    /// and every injected path leaves it off: see the invariant in `lib.rs`.
    ///
    /// One flag rather than two, because the caller asking this question is
    /// asking to see history at all; which *kind* of closed a task is is a
    /// property of the row it then reads.
    pub include_closed: bool,
    /// Only tasks held by this session id.
    pub session: Option<String>,
    /// Only tasks held by this person (Nextcloud user id).
    pub person: Option<String>,
    /// Widen [`session`](Self::session) to *and the ones nobody holds*.
    ///
    /// ⚠ **The pile is not a courtesy here, it is the handover channel.** A
    /// digest narrowed to strictly its own tasks would be smaller still and
    /// would also make the pile invisible — and a pile nobody can see is one
    /// nobody takes from, which is how Pippijn hands work to whichever
    /// conversation is around. Ignored unless `session` is set: on its own it
    /// would mean "held tasks, plus the pile", which is every task there is.
    pub or_unheld: bool,
    /// Strictly the tasks nobody holds — the pile, and nothing else.
    ///
    /// ⚠ **The narrow twin of [`or_unheld`](Self::or_unheld), and not the same
    /// question.** That one widens a session's own plate to include the pile;
    /// this one asks what is going spare. Both existed as ideas from the start
    /// and only the widening one was built, so "what is in the pile" had to be
    /// answered by filtering `--all` by hand — which on 2026-08-10 reported 137
    /// unheld tasks when there were 5, because the guesser invented a `session`
    /// field that does not exist and matched every row without one.
    ///
    /// Wins over the other two when set: a caller asking for the pile is not
    /// asking about a holder, so a session id alongside it is ignored rather
    /// than intersected — an intersection would always be empty.
    pub unheld: bool,
}

impl Filter {
    /// What a session's digest asks for: its own open tasks and the pile.
    ///
    /// There is nothing else to narrow by. The repository used to be the second
    /// half of this and was dropped in `0004`: a session spans checkouts, and
    /// selecting on a *claimed* set meant a session that had claimed nothing saw
    /// an empty digest that looked exactly like a broken service.
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
    /// How many of them are still open. Counted in SQL for the same reason
    /// `detailed` is: a client must not have to fetch rows to answer a boolean.
    open_blockers: i64,
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
            filed_by: self.filed_by,
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
            "SELECT t.id, t.subject, t.status, t.priority, t.due, ",
            // Reported as the VALUE it sorts as, not as a flag, so that no
            // renderer has to know the rule — the week and the level both live
            // here. NULL when the stored rank is already `P0`, because nothing
            // was raised and saying otherwise would invite a client to draw a
            // difference that does not exist.
            "IF(",
            due_soon!("t.due"),
            " AND COALESCE(t.priority, 'P2') > 'P0', ",
            "'P0', NULL) AS escalated_to, ",
            // One clock — the database's — so the CLI, the app and the digest
            // cannot disagree about which day it is. `CURDATE()` is the session
            // zone, pinned to UTC in `db::connect`.
            "(t.due IS NOT NULL AND t.due < CURDATE()) AS past_due, ",
            "t.assignee_kind, ",
            // Two correlated subqueries rather than a join, for the reason
            // `filed_by` below is one: a join to `task_blocks` MULTIPLIES the
            // task row by its edges, and a list is the one thing here that must
            // not gain rows. Both are covered by the table's primary key.
            //
            // ⚠ GROUP_CONCAT is capped by `group_concat_max_len` (1024 bytes by
            // default, ~140 ids). A task with more blockers than that has a
            // problem this truncation is not the biggest part of, and the count
            // beside it is exact regardless — so `blocked` stays right even if
            // the list were ever clipped.
            "(SELECT GROUP_CONCAT(b.blocked_on ORDER BY b.blocked_on) FROM task_blocks b ",
            "WHERE b.task_id = t.id) AS blocked_on, ",
            "(SELECT COUNT(*) FROM task_blocks b JOIN tasks bt ON bt.id = b.blocked_on ",
            "WHERE b.task_id = t.id AND ",
            still_open!("bt.status"),
            ") AS open_blockers, ",
            "t.assignee_person, t.assignee_session, s.name AS session_name, ",
            "(LENGTH(TRIM(t.body)) > 0) AS detailed, ",
            // Correlated rather than joined: a task has one `created` event, but
            // a join that ever saw two would silently DUPLICATE the task in
            // every list — a list is the one thing here that must not gain rows.
            // `LIMIT 1` makes that impossible to reach rather than unlikely.
            // Covered by `idx_task_events_task (task_id, at)`, so no migration.
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
/// ⚠ **Ordered by id and NOT by status.** A list that re-sorts as work starts on
/// an item moves the line somebody was reading; the client groups when it wants
/// to. Id order is oldest first, and that is the point rather than an accident —
/// old tickets belong at the top so they get fixed rather than buried under
/// whatever was filed this morning. Pippijn refused two proposals to reorder by
/// anything else on 2026-08-10 (`#704`, `#723`).
///
/// ⚠ **Priority is the one thing that reorders, because it is the one order
/// somebody stated.** `COALESCE(t.priority, 'P2')` is the whole of it: an
/// unranked task sorts exactly where an ordinary one does, so `P0` and `P1` rise
/// above the untriaged and `P3`/`P4` sink below, and everything nobody has
/// touched keeps its id order untouched. The mirror of
/// [`Priority::rank`](crate::tasks::types::Priority::rank), which
/// `tests/priority.rs` checks against a real database rather than by inspection.
///
/// ⚠ **This is the only sort in the service**, deliberately: `digest::render`
/// preserves the order it is handed, so a second ordering for the prompt would
/// be a second thing to keep true.
pub async fn list(pool: &MySqlPool, filter: &Filter) -> Result<Vec<Task>> {
    let mut query = QueryBuilder::<MySql>::new(select!(""));
    query.push(" WHERE 1 = 1");
    if !filter.include_closed {
        query.push(concat!(" AND ", still_open!("t.status")));
    }
    // Before the holder clauses, and exclusive of them: "what is going spare"
    // has no holder to narrow by, and intersecting the two would always answer
    // nothing at all.
    if filter.unheld {
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
        && let Some(person) = &filter.person
    {
        query.push(" AND t.assignee_kind = 'person' AND t.assignee_person = ");
        query.push_bind(person);
    }
    // The sort key is the EFFECTIVE rank: a deadline inside the week raises a
    // task to `P0` wherever it was set. This is the one thing a deadline is
    // allowed to reorder, and only because Pippijn stated the rule — the earlier
    // refusal to let dates reorder was about arithmetic overriding a human
    // decision, and a rule he sets IS the decision.
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
    pub subject: String,
    #[serde(default)]
    pub body: String,
    /// How urgent — and **the one field a filer may not leave out**.
    ///
    /// ⚠ **No `serde(default)`, and no `skip_serializing_if`, deliberately.**
    /// Every other field here means *leave it alone* when absent; this one has
    /// no such reading. A missing key is refused, so the two states are both
    /// things somebody SAID rather than one thing said and one thing skipped:
    ///
    /// * `"priority": "P2"` — a level, judged.
    /// * `"priority": null` — **unassessed**: nobody has judged this yet.
    ///
    /// `null` still sorts exactly where `P2` does (`COALESCE(priority, 'P2')`),
    /// so this costs no ordering and buys one thing: `P2` now means *somebody
    /// looked and called it ordinary*, where before it was indistinguishable
    /// from *nobody looked*. Asked for by Pippijn 2026-08-11 — "I want
    /// everything to have a priority" — with the explicit escape kept, because
    /// a filer working outside their own domain genuinely cannot judge, and
    /// forcing a number out of them would buy false precision rather than
    /// triage.
    ///
    /// Carrying no attributes at all is what states this to the mirror check:
    /// `models.ts` must declare the key as always present and nullable, not
    /// optional.
    pub priority: Ranking,
    /// The day it has to be done by, if something outside already decides it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<NaiveDate>,
    /// The tasks this one waits for, if they are known at filing time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_on: Vec<u64>,
    /// Who it is for. Absent leaves it in the pile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<Assignee>,
}

/// ⚠ **What the refusal SAYS, and nothing about whether there is one.** The
/// type above is what enforces this — `Ranking` has no reading for an absent
/// key — and if the two ever disagree the deserialiser still refuses, with
/// serde's wording instead of these words. So the cost of forgetting to keep
/// this level is a worse message, never an accepted filing.
///
/// It says both answers because saying only the first is what the whole rule
/// was going to be misread as. See `crate::wire`.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    /// ⚠ **Absent means leave it alone, so this cannot CLEAR a priority.** That
    /// is the same rule every other field here follows, and the cost of an
    /// exception — `Option<Option<Priority>>`, which serde renders as a field
    /// whose null is meaningful — is not worth a gesture nobody has asked for.
    /// Ranking a task wrongly is corrected by ranking it again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    /// Set the day it has to be done by.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<NaiveDate>,
    /// Take the deadline off.
    ///
    /// ⚠ **An explicit flag, where `blocked_on` needs none.** The reasoning is
    /// the same and the types give different answers: an empty LIST is a value
    /// that says *nothing blocks this*, so absence can keep meaning leave-alone.
    /// A date has no such value — every date is a real deadline — so removing
    /// one has to be said some other way. `Option<Option<NaiveDate>>` would be
    /// the alternative, and a null with meaning is the shape that makes every
    /// client guess.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub clear_due: bool,
    /// The blockers as they should now be — the whole set, not an addition.
    ///
    /// ⚠ **An empty list is how a task stops being blocked**, and that is why
    /// there is no `unblock` flag beside this. Absence still means *leave it
    /// alone*, as everywhere else here; `[]` is a value rather than an absence,
    /// so unblocking needs no second way to say it. Unlike `priority`, which
    /// genuinely cannot be cleared, this one can, because a task really does
    /// stop waiting and that is an event rather than a correction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_on: Option<Vec<u64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<Assignee>,
}

/// Nothing is required of a change, and taking the default is how that is said.
///
/// ⚠ **Not an oversight to be tidied up later by copying `NewTask`'s list.** A
/// change means *leave alone what I did not mention*, so a client moving a
/// status must not have to restate a priority it has never read — and requiring
/// one here would make every edit through this route restate the rank, which is
/// how a rank stops being something somebody said.
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

/// Split an assignee into the three columns that store it.
fn assignee_columns(assignee: &Assignee) -> (AssigneeKind, Option<&str>, Option<&str>) {
    match assignee.kind {
        AssigneeKind::Nobody => (AssigneeKind::Nobody, None, None),
        AssigneeKind::Person => (AssigneeKind::Person, assignee.id.as_deref(), None),
        AssigneeKind::Session => (AssigneeKind::Session, None, assignee.id.as_deref()),
    }
}

/// Whoever is asking, as a holder.
///
/// The three places a holder is inferred rather than stated — filing, starting,
/// closing — all mean the same thing by it, so they say it once. The name is
/// left empty: it is resolved through the session join on the way back out, and
/// writing one here would be a second copy to keep level with a rename.
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

/// Pippijn's rule on a blocked task, checked at both ends.
///
/// *"It can be the same, but not higher priority than the thing it's blocked
/// on."* (2026-08-11.) A task you cannot start must not claim to be the next
/// thing anybody does — that is the single move that inflates a scale, and it is
/// the one shape a machine can catch.
///
/// ⚠ **The bound is the LEAST urgent open blocker**, which is the one that
/// decides when this can actually start. Blocked on a `P1` and a `P3`, a task
/// waits for the `P3`.
///
/// ⚠ **Both ends, because either edit can break it.** Ranking the BLOCKED task
/// up is the obvious one. Ranking a BLOCKER *down* is the one that would
/// otherwise slip through: demote a `P1` to `P3` and everything waiting on it is
/// silently left more urgent than what it waits for.
///
/// ⚠ **Refused, not cascaded.** Quietly demoting whatever waits on a demoted
/// blocker would edit rows nobody asked about, and the caller would learn what
/// happened by going and looking. The refusal names both tasks, so the person
/// deciding sees the pair.
///
/// ⚠ **Only while the blocker is OPEN.** A closed blocker constrains nothing —
/// the work is free to start — and applying the rule to it would leave a
/// finished dependency holding a rank down for ever.
///
/// ⚠ **The rule binds a CLAIM, so an unranked task is never in violation.** This
/// is the one asymmetry and it is deliberate. Unranked sorts as [`Priority::P2`]
/// — that is the ordering — but it asserts nothing, and the rule is about
/// asserting *do this next* for work you cannot start. Applying it to untriaged
/// tasks would mean recording *"#726 waits for #697"* is refused until #726 is
/// ranked, which turns writing down a fact into making a decision. That is the
/// pressure that ends with everything ranked to satisfy a field, and a scale
/// where every value has been satisfied rather than chosen says nothing.
///
/// So: the blocked task is checked only once somebody has ranked it. A
/// BLOCKER's absent rank still counts as `P2`, because that is genuinely where
/// it sits in the list and the claim above it has to clear something.
async fn blocking_is_consistent(
    tx: &mut Transaction<'_, MySql>,
    id: u64,
    priority: Option<Priority>,
    due: Option<NaiveDate>,
) -> Result<()> {
    // ⚠ **The deadline twin of the rank rule, and it needs no threshold.** A
    // task cannot be finished before the thing it is waiting for, so a due date
    // earlier than an open blocker's is not a priority call — it is arithmetic,
    // and it is wrong however anybody feels about it. Equal is allowed: both
    // landing on the same day is tight, not impossible.
    //
    // Checked before the rank, because it is the harder fact.
    if let Some(mine) = due {
        // dev-lint: allow-sqlx — a `concat!`ed literal; see `get` below.
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
    // dev-lint: allow-sqlx — a `concat!`ed literal; see `get` below.
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
    // dev-lint: allow-sqlx — a `concat!`ed literal; see `get` below.
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
/// Split out only so the early return above can reach it — a task with no rank
/// of its own can still be the BLOCKER whose demotion would strand something
/// that does have one.
async fn unblocked_end(
    tx: &mut Transaction<'_, MySql>,
    id: u64,
    priority: Option<Priority>,
) -> Result<()> {
    let mine = Priority::rank(priority);
    // dev-lint: allow-sqlx — a `concat!`ed literal; see `get` below.
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
/// ⚠ **A cycle makes the rank rule unsatisfiable rather than merely odd**: every
/// task around the loop would have to be no more urgent than the next, all the
/// way back to itself, and none of them could ever start. Walked breadth-first
/// over the whole edge set rather than checked one step, because `A → B → C → A`
/// is the same mistake spread over three separate edits. Bounded by the graph:
/// every task is enqueued at most once.
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
/// The whole set is written rather than added to: `Change::blocked_on` is the
/// list as it should now be, so an empty one is how a task stops being blocked.
/// That is why there is no `--unblock` flag anywhere — an empty list is a value,
/// not an absence, and needs no second way to say it.
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
/// The same defect and the same fix as `unknown_holder` above — both foreign
/// keys arrive as `ErrorKind::ForeignKeyViolation`, and on this statement there
/// is one row that can be missing.
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
/// ⚠ **`fk_tasks_session` was always doing the refusing.** What was wrong was
/// the answer: the violation arrived as `AppError::Other`, which is a 500 logged
/// as an internal error and reaching the caller as the anyhow context — the
/// words `moving a task`, which name the operation they already know they asked
/// for. The service knows exactly which id has no row and can say so.
///
/// ⚠ **Discriminated by KIND rather than by the constraint's name**, which is
/// not the obvious way round and is forced: sqlx 0.9's MySQL driver answers
/// `DatabaseError::constraint()` with `None`, so `fk_tasks_session` reaches Rust
/// only inside the message text, and matching on that is a parse of an English
/// sentence MariaDB is free to reword. `ErrorKind::ForeignKeyViolation` is
/// typed — and on a statement writing `assignee_session` there is exactly one
/// key it can be, this schema's other one belonging to `task_events`.
///
/// ⚠ **Reading the constraint's answer rather than checking the row first.**
/// A `SELECT` before the write spends a query on every move to learn what the
/// constraint is about to enforce anyway, and still races a session deleted
/// between the two. This cannot: the write already happened.
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
    // Filing a task takes it on, unless the caller says where it goes.
    //
    // ⚠ **The default used to be the pile, and that was the wrong way round.**
    // Nothing was ever implicitly the filer's, so a session filed, worked and
    // closed without the holder column ever naming it while the work was in
    // flight — `task sessions` read `0/3 open` for a conversation that had spent
    // hours on three tasks, because a holder was only ever recorded on the way
    // out. Pippijn's rule: a task a Claude session deals with is that session's
    // by default, the way the built-in task tool behaves. The pile is still one
    // word away (`--to nobody`, or "nobody" in the form) and is now something
    // said rather than something fallen into.
    //
    // ⚠ **A session filing a task now needs a row in `sessions`**, where before
    // it did not: the default holder is a foreign key. Both write routes call
    // `sessions::touch` before reaching here, which is what makes that hold —
    // and a caller coming in below the routes (a test, an import) has to do the
    // same. Falling back to the pile for an unknown session would hide a
    // conversation's work rather than fail, which is the wrong way round.
    let assignee = new.assignee.unwrap_or_else(|| actor_holder(actor));
    check_assignee(&assignee)?;
    let (kind, person, session) = assignee_columns(&assignee);

    let mut tx = pool.begin().await.context("opening a transaction")?;
    let done = sqlx::query(
        "INSERT INTO tasks (subject, body, priority, due, assignee_kind, assignee_person, \
         assignee_session) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&subject)
    .bind(&new.body)
    .bind(new.priority.stored())
    .bind(new.due)
    .bind(kind)
    .bind(person)
    .bind(session)
    .execute(&mut *tx)
    .await
    .map_err(|e| unknown_holder(e, session, "filing a task"))?;
    let id = done.last_insert_id();
    record(&mut tx, id, actor, "created", Some(subject.clone())).await?;
    if let Some(moved) = set_blockers(&mut tx, id, &new.blocked_on).await? {
        record(&mut tx, id, actor, "blocked", Some(moved)).await?;
        blocking_is_consistent(&mut tx, id, new.priority.stored(), new.due).await?;
    }
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
pub async fn update(pool: &MySqlPool, id: u64, change: Change, actor: &Actor) -> Result<Updated> {
    let before = list_one(pool, id).await?;

    // ⚠ **A closed task may not be left in the pile.** The pile means *for
    // whoever picks it up*, and nobody picks up a finished task; what it
    // actually produces is a list saying a thing was done by nobody, which is
    // the one question `assignee` exists to answer. Three tasks reached that
    // state before the rule existed — #106 closed in the 1h37m between this
    // service going live and `eaf64c4` adding the finisher, and #629/#630
    // imported already `done` from a file scheme that recorded no owner. All
    // three were attributed by hand on 2026-08-09.
    //
    // **Only an EXPLICIT `nobody` needs refusing**, which is narrower than it
    // looks. Closing without naming anybody is already covered: the finisher
    // below claims it for whoever asked. So there is no need to reason about
    // the holder a task would end up with after inference — and reasoning about
    // it here would be wrong, since `change.assignee` is `None` on an ordinary
    // `task done` and the effective holder still reads as `nobody` at this
    // point. That version rejects every close there is.
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

    // What actually moved. Pushed beside each `record`, because the event rows
    // ARE the answer — a write that writes no history changed nothing — and two
    // lists that have to be kept level by hand would drift the first time an
    // axis was added.
    let mut changed: Vec<&'static str> = Vec::new();

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
            changed.push("edited");
        }
    }

    // ⚠ **Compared first, like the subject beside it.** This branch used to write
    // and record unconditionally, which put an `edited` in the history for
    // saving a body somebody had not touched — and, once a write began
    // REPORTING what it moved, would have made it claim an edit that never
    // happened. The extra read is on the write path only; it never touches the
    // digest, which is the one thing charged per turn.
    if let Some(body) = &change.body {
        let current: Option<(String,)> = sqlx::query_as("SELECT body FROM tasks WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .context("reading a body before changing it")?;
        if current.map(|(b,)| b).as_deref() != Some(body.as_str()) {
            sqlx::query("UPDATE tasks SET body = ? WHERE id = ?")
                .bind(body)
                .bind(id)
                .execute(&mut *tx)
                .await
                .context("changing a body")?;
            record(&mut tx, id, actor, "edited", Some("body".into())).await?;
            changed.push("edited");
        }
    }

    if let Some(status) = change.status
        && status != before.status
    {
        // `closed_at` moves with the status in the same statement, so a row
        // cannot be closed with no closing time or open with one. The condition
        // is computed here rather than written as SQL against the word: the
        // vocabulary lives in `Status::is_open`, whose `match` is exhaustive,
        // and `IF(? = 'done', …)` was how a dropped task would have ended up
        // closed with no closing time.
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
            "status",
            Some(format!("{} → {}", before.status, status)),
        )
        .await?;
        changed.push("status");
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
            "ranked",
            Some(format!("{was} → {priority}")),
        )
        .await?;
        changed.push("priority");
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
            "due",
            Some(format!("{} → {}", show(before.due), show(due))),
        )
        .await?;
        changed.push("due");
    }

    if let Some(want) = &change.blocked_on
        && let Some(moved) = set_blockers(&mut tx, id, want).await?
    {
        record(&mut tx, id, actor, "blocked", Some(moved)).await?;
        changed.push("blocked_on");
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

    // Closing a task makes the closer its holder.
    //
    // ⚠ **Not decoration: `assignee` is the only place a LIST can say who did
    // something.** The history knows — every change records its actor — but no
    // list renders a history, so a task closed while held by `nobody` reads as
    // "done by nobody" everywhere it is ever seen again. That was true of #461,
    // finished by the session that wrote this.
    //
    // **Dropping counts as closing here**, on the same argument read the other
    // way: *who decided this was not worth doing* is exactly as much a thing a
    // list should be able to say as who did it, and the status beside the name
    // is what distinguishes the two.
    //
    // An explicit assignee in the same change wins: a caller saying where a task
    // should go is more specific than this rule inferring it from who is asking.
    // Only on the way OUT of open — reopening leaves the holder alone, because
    // the last person to touch it is a better guess than nobody, and done →
    // dropped is a correction to a closed task rather than a new closing.
    let finisher = (change.status.is_some_and(|status| !status.is_open())
        && before.status.is_open()
        && change.assignee.is_none())
    .then(|| actor_holder(actor));

    // Starting a task claims it too — the same rule read at the other end.
    //
    // ⚠ **Without this the holder column could only ever describe the past.** It
    // was set when a task was closed and at no other time, so a list could say
    // who had finished something and never who was carrying it; every session
    // showed its in-flight work as belonging to nobody. `task start` was already
    // documented as the way a session takes a task on, and it did not do it.
    //
    // ⚠ **Out of the PILE only, which is narrower than it first shipped.** The
    // guard was on the status — claim unless the task was already `doing` — and
    // that read as safe while being wrong: a task Pippijn had handed to one
    // conversation, which had not got to it yet, was taken off it by any other
    // session running `start`, silently. `starting_a_task_assigned_to_another_
    // session_takes_nothing` is that case, and it failed against the rule the
    // comment here originally claimed to implement.
    //
    // So: a holder is inferred only where there is none. If the task is already
    // yours there is nothing to move; if it is somebody else's, taking it is a
    // handover and `move` is the word for that.
    //
    // ⚠ **And it does not read the status, which was the last thing keeping this
    // from firing where it was most needed.** A `&& before.status != Doing`
    // clause survived that narrowing, on the argument that starting an
    // already-started task should write no history — true of every task that is
    // `doing` *because somebody is doing it*, and false of the one state where
    // the status says nothing about the holder. A session that stops work
    // deliberately hands the task back without closing it, leaving it `doing`
    // and in the pile (#19), and `start` — the documented way to pick something
    // up — then reported success and moved nobody. The no-history property was
    // never this clause's to keep: `moved` below compares the holders and
    // suppresses a write when they already match.
    let starter = (change.status == Some(Status::Doing)
        && before.assignee.kind == AssigneeKind::Nobody
        && change.assignee.is_none())
    .then(|| actor_holder(actor));

    if let Some(assignee) = change
        .assignee
        .as_ref()
        .or(finisher.as_ref())
        .or(starter.as_ref())
    {
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
            .map_err(|e| unknown_holder(e, session, "moving a task"))?;
            let to = label_of(&mut tx, assignee).await?;
            record(
                &mut tx,
                id,
                actor,
                "assigned",
                Some(format!("{} → {to}", before.assignee.label())),
            )
            .await?;
            changed.push("assigned");
        }
    }

    tx.commit().await.context("committing a task change")?;
    Ok(Updated {
        task: list_one(pool, id).await?,
        changed,
    })
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
