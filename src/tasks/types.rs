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
    pub assignee: Assignee,
    /// Whether there is prose behind it worth opening. A task written as a
    /// one-line reminder has none, and offering to open an empty sheet is worse
    /// than not offering.
    pub detailed: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<DateTime<Utc>>,
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
