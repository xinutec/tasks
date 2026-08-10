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
use crate::still_open;
use crate::tasks::types::{
    Actor, Assignee, AssigneeKind, Event, MAX_SUBJECT, Status, Task, TaskDetail,
};

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
            "SELECT t.id, t.subject, t.status, t.assignee_kind, ",
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

/// Tasks matching a filter, oldest id first.
///
/// ⚠ **Ordered by id, which is creation order, and not by status.** A list that
/// re-sorts as work starts on an item moves the line somebody was reading; the
/// client groups when it wants to.
pub async fn list(pool: &MySqlPool, filter: &Filter) -> Result<Vec<Task>> {
    let mut query = QueryBuilder::<MySql>::new(select!(""));
    query.push(" WHERE 1 = 1");
    if !filter.include_closed {
        query.push(concat!(" AND ", still_open!("t.status")));
    }
    if let Some(session) = &filter.session {
        query.push(" AND ((t.assignee_kind = 'session' AND t.assignee_session = ");
        query.push_bind(session);
        query.push(")");
        if filter.or_unheld {
            query.push(" OR t.assignee_kind = 'nobody'");
        }
        query.push(")");
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
        "INSERT INTO tasks (subject, body, assignee_kind, assignee_person, assignee_session) \
         VALUES (?, ?, ?, ?, ?)",
    )
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
