//! What a task is.
//!
//! Two small closed vocabularies — a status and who is holding it — and the
//! records built from them. Both vocabularies are stored as `VARCHAR` and
//! parsed on the way out, so a value outside the set fails the query loudly
//! instead of arriving as a default.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Store a fieldless enum in a `VARCHAR` column.
///
/// ⚠ **Not `#[derive(sqlx::Type)]`.** That derive declares the SQL type as
/// `ENUM`, and against a `VARCHAR` column it compiles, passes every test that
/// does not touch the column, and fails *every read of a real row* at runtime.
/// The three impls delegate to `str` instead. `Decode` parses, so an
/// out-of-vocabulary value stored by anything else is an error rather than a
/// silent default.
macro_rules! varchar_enum {
    ($name:ident) => {
        impl sqlx::Type<sqlx::MySql> for $name {
            fn type_info() -> <sqlx::MySql as sqlx::Database>::TypeInfo {
                <str as sqlx::Type<sqlx::MySql>>::type_info()
            }
            fn compatible(ty: &<sqlx::MySql as sqlx::Database>::TypeInfo) -> bool {
                <str as sqlx::Type<sqlx::MySql>>::compatible(ty)
            }
        }
        impl<'q> sqlx::Encode<'q, sqlx::MySql> for $name {
            fn encode_by_ref(
                &self,
                buf: &mut <sqlx::MySql as sqlx::Database>::ArgumentBuffer,
            ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
                <&str as sqlx::Encode<'q, sqlx::MySql>>::encode_by_ref(&self.as_str(), buf)
            }
        }
        impl<'r> sqlx::Decode<'r, sqlx::MySql> for $name {
            fn decode(
                value: <sqlx::MySql as sqlx::Database>::ValueRef<'r>,
            ) -> Result<Self, sqlx::error::BoxDynError> {
                <&str as sqlx::Decode<'r, sqlx::MySql>>::decode(value)?
                    .parse()
                    .map_err(Into::into)
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

/// Where a task stands.
///
/// Two open states and two ways out. The first three match the two the file
/// scheme wrote (`- [ ]` and `- [>]`) plus the one it expressed by deleting the
/// line. `Doing` is not decoration: a session that has picked a task up says so,
/// and that is how the other reader knows not to start it.
///
/// ⚠ **`Dropped` is a closed task that was never done**, and it exists because
/// the alternative was worse in both directions: leaving a task that has gone
/// out of date open for ever, or closing it as `Done` and having every later
/// list credit somebody with work nobody did. The distinction is only ever read
/// *after* the fact — nothing injected selects a closed row either way — so it
/// buys nothing at all except an honest record, which is the whole of the case
/// for it. There is deliberately no *reason* field beside it: if why it went
/// matters, that is prose, and the body is where prose lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Open,
    Doing,
    Done,
    /// Closed without being done: overtaken, obsolete, or decided against.
    Dropped,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Open => "open",
            Status::Doing => "doing",
            Status::Done => "done",
            Status::Dropped => "dropped",
        }
    }

    /// Whether this status is still work. The digest selects on exactly this,
    /// and it is a method rather than a comparison at each call site so that
    /// adding a fourth state cannot quietly leave one of them behind.
    ///
    /// It very nearly did. Every SQL query that meant *open* spelled it
    /// `status <> 'done'`, which was the same thing right up until it wasn't —
    /// see [`still_open!`](crate::still_open), which is this predicate's other
    /// half and the only place the vocabulary appears in SQL.
    pub fn is_open(self) -> bool {
        match self {
            Status::Open | Status::Doing => true,
            Status::Done | Status::Dropped => false,
        }
    }

    /// The checkbox the file scheme used, kept because the digest still renders
    /// it and a session has read thousands of these lines. `- [-]` is the one
    /// spelling that was never in those files, because the scheme had no way to
    /// say it.
    pub fn marker(self) -> &'static str {
        match self {
            Status::Open => "- [ ]",
            Status::Doing => "- [>]",
            Status::Done => "- [x]",
            Status::Dropped => "- [-]",
        }
    }
}

impl FromStr for Status {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "open" => Ok(Status::Open),
            "doing" => Ok(Status::Doing),
            "done" => Ok(Status::Done),
            "dropped" => Ok(Status::Dropped),
            other => Err(format!("unknown task status {other:?}")),
        }
    }
}

/// SQL for *this task is still work*, spelled in exactly one place.
///
/// ⚠ **The obvious spelling is the wrong one.** Six queries said `status <>
/// 'done'` and meant "open", which was true while there were three states and
/// false the moment [`Status::Dropped`] existed — a dropped task would have gone
/// on being counted as open in the list, the digest's counts and all three
/// of the `/who` tallies, none of which would have failed loudly.
///
/// A macro rather than a `const` because sqlx 0.9 takes only `&'static str`:
/// this expands inside `concat!` and the compiler assembles the literal, so
/// there is still nothing built at runtime for anybody to audit. The column is
/// an argument because half these queries join and have to qualify it.
///
/// `tests/tasks_db.rs::a_dropped_task_is_not_open_anywhere` is what ties this to
/// [`Status::is_open`]: nothing else can compare a match arm against a string
/// living in a database.
#[macro_export]
macro_rules! still_open {
    ($column:literal) => {
        concat!($column, " IN ('open', 'doing')")
    };
}

varchar_enum!(Status);

/// How urgent a task is, when somebody has said.
///
/// ⚠ **Absence is not a level, and every list depends on that.** Most tasks have
/// no priority and always will: there were 700-odd rows the day this was added
/// and none of them were going to be triaged. A default of `P2` would have all
/// of them assert something nobody said, so the column is nullable and this type
/// only ever describes a task somebody ranked. `Option<Priority>` throughout.
///
/// ⚠ **Untriaged sorts as [`Priority::P2`] all the same** — [`Priority::rank`],
/// and `COALESCE(priority, 'P2')` in the SQL. That is the one decision the
/// whole feature turns on: it puts `P0`/`P1` above the untriaged pile and `P3`/
/// `P4` *below* it, where "sort the ranked first, the rest after" would have
/// lifted a task marked *when there is room* above four hundred nobody had read.
/// Within a rank it stays id order, oldest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Priority {
    P0,
    P1,
    P2,
    P3,
    P4,
}

impl Priority {
    pub fn as_str(self) -> &'static str {
        match self {
            Priority::P0 => "P0",
            Priority::P1 => "P1",
            Priority::P2 => "P2",
            Priority::P3 => "P3",
            Priority::P4 => "P4",
        }
    }

    /// What each level means, in one line.
    ///
    /// ⚠ **Without this the levels are worth nothing.** Five names two readers
    /// interpret differently do not compare across holders, and this system's
    /// whole point is that work moves between a person and several
    /// conversations. Printed by `task --help`, which is where it will actually
    /// be read.
    pub fn gloss(self) -> &'static str {
        match self {
            Priority::P0 => "drop what you are doing; nothing else moves until this does",
            Priority::P1 => "next, ahead of anything unranked",
            Priority::P2 => "ordinary work — and where an UNRANKED task already sits",
            Priority::P3 => "when there is room; it will not be missed this week",
            Priority::P4 => "kept on purpose but not scheduled — the alternative to dropping it",
        }
    }

    /// Every level, most urgent first. One source for `--help` and the parser.
    pub fn all() -> [Priority; 5] {
        [
            Priority::P0,
            Priority::P1,
            Priority::P2,
            Priority::P3,
            Priority::P4,
        ]
    }

    /// Where an `Option<Priority>` sorts. Unranked ranks as `P2`.
    ///
    /// The Rust twin of the SQL's `COALESCE(priority, 'P2')`, and the two must
    /// agree — `tests/priority.rs` compares them against a real database rather
    /// than trusting that they were written on the same afternoon.
    pub fn rank(this: Option<Priority>) -> Priority {
        this.unwrap_or(Priority::P2)
    }
}

impl FromStr for Priority {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Case-folded for the CLI's sake — `p0` is what a hand types — and the
        // stored spelling is the upper one, which is what the SQL sorts on.
        match s.to_ascii_uppercase().as_str() {
            "P0" => Ok(Priority::P0),
            "P1" => Ok(Priority::P1),
            "P2" => Ok(Priority::P2),
            "P3" => Ok(Priority::P3),
            "P4" => Ok(Priority::P4),
            other => Err(format!("unknown priority {other:?} — P0 to P4")),
        }
    }
}

varchar_enum!(Priority);

/// Which kind of holder a task has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssigneeKind {
    /// In the pile: nobody has taken it, and whichever conversation is around may.
    Nobody,
    /// A Nextcloud user — Pippijn.
    Person,
    /// A Claude Code conversation, by the CLI's session id.
    Session,
}

impl AssigneeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AssigneeKind::Nobody => "nobody",
            AssigneeKind::Person => "person",
            AssigneeKind::Session => "session",
        }
    }
}

impl FromStr for AssigneeKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "nobody" => Ok(AssigneeKind::Nobody),
            "person" => Ok(AssigneeKind::Person),
            "session" => Ok(AssigneeKind::Session),
            other => Err(format!("unknown assignee kind {other:?}")),
        }
    }
}

varchar_enum!(AssigneeKind);

/// Who is holding a task, resolved for display.
///
/// The `id` is what the database stores and what an API caller sends back;
/// `name` is what a person reads and may be absent — a session that has not
/// named itself yet, or one whose row has been forgotten. **Never key anything
/// on `name`**: a session renames itself as its job changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignee {
    pub kind: AssigneeKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Assignee {
    pub fn nobody() -> Self {
        Self {
            kind: AssigneeKind::Nobody,
            id: None,
            name: None,
        }
    }

    /// What to call this holder in one word — for a digest line or a chip.
    /// Falls back to the id, and then to `nobody`, because a blank there reads
    /// as "unassigned" and would be a lie about a task somebody is holding.
    pub fn label(&self) -> String {
        match (&self.name, &self.id) {
            (Some(name), _) if !name.is_empty() => name.clone(),
            (_, Some(id)) => id.clone(),
            _ => "nobody".to_string(),
        }
    }
}

/// A task as it appears in any list: everything except the prose.
///
/// ⚠ **The body is deliberately not here.** This struct is what a list
/// serialises, and one list is what a hook injects; the whole reason this
/// service exists is that a list carrying bodies cost 86 kB to render 3.9 kB.
/// [`TaskDetail`] is the one that carries prose, and it is fetched for one task
/// at a time.
#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: u64,
    pub subject: String,
    pub status: Status,
    /// How urgent, when somebody has said. Absent is the ordinary case and is
    /// not a level — see [`Priority`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    pub assignee: Assignee,
    /// Whether there is prose behind it worth opening. A task written as a
    /// one-line reminder has none, and offering to open an empty sheet is worse
    /// than not offering.
    pub detailed: bool,
    /// What the session that filed it calls itself — `observe`, `health`,
    /// `dev-lint`. Absent when Pippijn filed it, or when the filing session had
    /// not named itself.
    ///
    /// ⚠ **A hint about where the work lives, and deliberately not a filter.**
    /// The repo column was retired in `0004` because a session spans checkouts
    /// and *which repo is this in* had no single answer. That removed two
    /// different things at once, and only one of them was wrong: *which sessions
    /// should be shown this* was a filter and it hid work, while *where does
    /// this work live* is a hint, and without it a session scanning the pile
    /// pays to open a task before it can learn the answer is no — 548 bytes to
    /// see the whole pile against 2,732 to read one line of it (#19, measured
    /// 2026-08-09).
    ///
    /// **A fact rather than a field**: `task_events` already records who filed
    /// every task, so there is nothing to set and nothing to keep true. It was
    /// known for 112 of the 139 open tasks the day this was added, and the names
    /// really are the project words. Resolved through the join like a holder's,
    /// so a session that renames itself is called the same thing everywhere at
    /// once.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filed_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<DateTime<Utc>>,
}

/// A task after a write, and what the write actually moved.
///
/// ⚠ **An empty `changed` means the call did nothing, and saying so is the whole
/// point.** Three defects in one day were writes that answered exactly like a
/// write that had worked: `start` on a task already `doing` in the pile, a
/// rename to a blank name, and closing into the pile. Each was found by
/// reproducing it against a scratch task, because success and no-op were
/// indistinguishable to the caller.
///
/// **Reported rather than refused.** A no-op is often correct — `start` on a
/// task already yours is meant to be quiet, and refusing it would trade a silent
/// success for a spurious failure. What was missing was never the refusal; it
/// was the sentence.
///
/// The vocabulary is `task_events`' own — `status`, `assigned`, `edited` — so
/// what a write reports and what the history records cannot drift into two
/// spellings of the same event.
#[derive(Debug, Clone, Serialize)]
pub struct Updated {
    #[serde(flatten)]
    pub task: Task,
    /// The event kinds written, in the order written.
    pub changed: Vec<&'static str>,
}

/// One task with its prose and its history — what opening a task returns.
#[derive(Debug, Clone, Serialize)]
pub struct TaskDetail {
    #[serde(flatten)]
    pub task: Task,
    /// The body as written, in markdown.
    pub body: String,
    /// The body rendered. Both are sent: the app shows the HTML, and a session
    /// reading through the CLI wants the markdown it will edit.
    pub body_html: String,
    pub events: Vec<Event>,
}

/// Something that happened to a task.
#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub at: DateTime<Utc>,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Who did it, already resolved to a readable label for the same reason
    /// `detail` is rendered at write time: the actor may be gone.
    pub actor: String,
}

/// Who is making a change — carried into every write so `task_events` can say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Actor {
    Person(String),
    Session(String),
}

impl Actor {
    pub fn kind(&self) -> &'static str {
        match self {
            Actor::Person(_) => "person",
            Actor::Session(_) => "session",
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Actor::Person(id) | Actor::Session(id) => id,
        }
    }
}

/// The longest subject that may be stored, matching the column.
///
/// A cap, not a style rule: this column is the per-turn cost of the whole
/// system, and something that does not fit on a line is a body.
pub const MAX_SUBJECT: usize = 200;
