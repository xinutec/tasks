//! Two closed vocabularies, a status and a holder, and the records built from
//! them. Stored as `VARCHAR` and parsed on read, so an unknown value fails loudly.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// Store a fieldless enum in a `VARCHAR` column.
///
/// ⚠ Not `#[derive(sqlx::Type)]`: that declares `ENUM`, and against `VARCHAR` it
/// compiles, passes tests that don't read the column, and fails every real row
/// at runtime. These impls delegate to `str`, and `Decode` parses.
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

/// Where a task stands: two open states (`Doing` tells others not to start it)
/// and two ways out. `Dropped` is closed but never done, so obsolete work neither
/// stays open nor credits work nobody did. Why it went is prose, in the body.
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

    /// Whether this status is still work: a method, so a new state can't be missed
    /// at a call site. The SQL half is [`still_open!`](crate::still_open).
    pub fn is_open(self) -> bool {
        match self {
            Status::Open | Status::Doing => true,
            Status::Done | Status::Dropped => false,
        }
    }

    /// The Markdown task-list checkbox the digest renders.
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

/// SQL for a deadline close enough to raise the rank: under a week, a constant
/// on purpose. One spelling, because the sort, the projection and the P0 guard
/// must agree on the day. `<`, not `<=` (`tests/blocking.rs` pins the boundary).
#[macro_export]
macro_rules! due_soon {
    ($column:literal) => {
        concat!(
            $column,
            " IS NOT NULL AND ",
            $column,
            " < CURDATE() + INTERVAL 7 DAY"
        )
    };
}

/// SQL for the body as a reader sees it, trimmed at both ends.
///
/// ⚠ SQL `TRIM()` removes only spaces, so a body of newlines would count as
/// prose. `[[:space:]]`, not `\s`, which `NO_BACKSLASH_ESCAPES` would break
/// silently.
#[macro_export]
macro_rules! body_shown {
    ($column:literal) => {
        concat!(
            "REGEXP_REPLACE(",
            $column,
            ", '^[[:space:]]+|[[:space:]]+$', '')"
        )
    };
}

/// SQL for "this task is still work", in one place.
///
/// ⚠ The obvious `status <> 'done'` counts dropped tasks as open. A macro so it
/// expands inside `concat!` (sqlx wants `&'static str`); the column is an
/// argument for joins. `tests/tasks_db.rs::a_dropped_task_is_not_open_anywhere`
/// ties it to [`Status::is_open`].
#[macro_export]
macro_rules! still_open {
    ($column:literal) => {
        concat!($column, " IN ('open', 'doing')")
    };
}

varchar_enum!(Status);

/// How urgent a task is, when somebody said. Absence is not a level: most tasks
/// never get one, so the column is nullable rather than defaulting to `P2`.
/// Unranked still sorts as `P2` ([`Priority::rank`], `COALESCE(priority, 'P2')`),
/// so `P3`/`P4` sort below the unranked tasks. Within a rank, id order.
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

    /// What each level means. Each is a test that can fail, applied as a cascade,
    /// so two conversations reach the same rank; a scale of feeling drifts to all
    /// `P0`. A spread of ranks checks the ranker; it is never a quota. Printed by
    /// `task --help`.
    pub fn gloss(self) -> &'static str {
        match self {
            Priority::P0 => "damage is accruing — every hour it stays open costs more",
            Priority::P1 => "nothing is accruing, but other work is waiting on this",
            Priority::P2 => "ordinary work, nothing waiting on it — where UNRANKED sits",
            Priority::P3 => "a workaround exists and is in use; what it costs is friction",
            Priority::P4 => "nothing is being paid for it today; not recited in your prompt",
        }
    }

    /// Every level, most urgent first, for `--help` and the parser.
    pub fn all() -> [Priority; 5] {
        [
            Priority::P0,
            Priority::P1,
            Priority::P2,
            Priority::P3,
            Priority::P4,
        ]
    }

    /// Where an `Option<Priority>` sorts: unranked as `P2`, the twin of the SQL's
    /// `COALESCE` (`tests/priority.rs` compares them on a real database).
    pub fn rank(this: Option<Priority>) -> Priority {
        this.unwrap_or(Priority::P2)
    }
}

/// What a filer said about urgency; there is no "did not say". `Unassessed` is
/// an answer, kept so `P2` means somebody looked: a required field with an easy
/// safe answer gets filled reflexively. Both sort at `P2`.
///
/// On the wire: a level or `null`; an absent key is an error. (Dropping
/// `#[serde(default)]` does not make an `Option` field mandatory.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ranking {
    /// Judged, at this level.
    At(Priority),
    /// Explicitly not judged; sorts as `P2`.
    Unassessed,
}

impl Ranking {
    /// The column value; `NULL` for unassessed.
    pub fn stored(self) -> Option<Priority> {
        match self {
            Ranking::At(priority) => Some(priority),
            Ranking::Unassessed => None,
        }
    }
}

impl<'de> Deserialize<'de> for Ranking {
    /// `null` is unassessed, a string must be a level, and absent is an error.
    ///
    /// ⚠ `deserialize_any`, not `Option::<Priority>::deserialize`: serde fills a
    /// missing field via `missing_field`, which answers `deserialize_option` with
    /// `None`, so the obvious spelling accepts a filing with no key. The test pins it.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Stated;

        impl<'de> serde::de::Visitor<'de> for Stated {
            type Value = Ranking;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a priority (\"P0\" to \"P4\"), or null for unassessed")
            }

            /// [`Priority`]'s derive, not `FromStr`: the wire isn't case-folded.
            fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<Ranking, E> {
                Priority::deserialize(serde::de::value::StrDeserializer::<E>::new(s))
                    .map(Ranking::At)
            }

            fn visit_unit<E: serde::de::Error>(self) -> Result<Ranking, E> {
                Ok(Ranking::Unassessed)
            }

            /// `null` from a format that models it as an option.
            fn visit_none<E: serde::de::Error>(self) -> Result<Ranking, E> {
                Ok(Ranking::Unassessed)
            }
        }

        d.deserialize_any(Stated)
    }
}

impl FromStr for Priority {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Case-folded and digit-only allowed for typing (`p0`, `0`); stored as `P<n>`.
        match s.to_ascii_uppercase().as_str() {
            "P0" | "0" => Ok(Priority::P0),
            "P1" | "1" => Ok(Priority::P1),
            "P2" | "2" => Ok(Priority::P2),
            "P3" | "3" => Ok(Priority::P3),
            "P4" | "4" => Ok(Priority::P4),
            other => Err(format!("unknown priority {other:?} — P0 to P4")),
        }
    }
}

varchar_enum!(Priority);

/// Which kind of holder a task has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssigneeKind {
    /// In the pile: whichever conversation is around may take it.
    Nobody,
    /// A Nextcloud user.
    Person,
    /// A Claude Code conversation, by session id.
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

/// Who holds a task. `id` is stored and sent back; `name` is for reading and may
/// be absent. Never key on `name`: sessions rename themselves.
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

    /// One word for a digest line or chip: the name, else the id, else `nobody`; a
    /// blank would read as unassigned.
    pub fn label(&self) -> String {
        match (&self.name, &self.id) {
            (Some(name), _) if !name.is_empty() => name.clone(),
            (_, Some(id)) => id.clone(),
            _ => "nobody".to_string(),
        }
    }
}

/// A task in a list: everything but the prose, which [`TaskDetail`] carries one
/// task at a time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: u64,
    pub subject: String,
    pub status: Status,
    /// When somebody said; usually absent ([`Priority`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    /// The day it must be done by, when something outside decides: a date, not an
    /// instant, inventing no time or zone. It reorders nothing, except within the
    /// week ([`escalated_to`](Self::escalated_to)).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due: Option<NaiveDate>,
    /// What it sorts as when a near deadline raised it: always `P0` when present,
    /// while [`priority`](Self::priority) keeps what was chosen. Derived at read
    /// time from `due` and the clock, never stored, so it can't drift; renderers use
    /// [`Task::urgency`] and the rule lives in [`due_soon!`](crate::due_soon).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub escalated_to: Option<Priority>,
    /// Whether [`due`](Self::due) has passed, by the database's clock, so CLI and app agree.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub overdue: bool,
    /// The tasks this one waits for, oldest first. A list, so a second blocker
    /// doesn't go stale in the body. A task may not outrank its least urgent open
    /// blocker. Kept after a blocker closes; [`blocked`](Self::blocked) is the effect.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_on: Vec<u64>,
    /// Whether any blocker is still open, resolved in the projection.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub blocked: bool,
    pub assignee: Assignee,
    /// Whether there is prose worth opening.
    pub detailed: bool,
    /// Body lines, always sent, so a reader that truncates (`| head`) knows it did.
    pub body_lines: u32,
    /// What the filing session calls itself (`observe`, `dev-lint`); absent for the
    /// person or an unnamed session. A hint where the work lives, never a filter.
    /// Read from the `created` event, so nothing has to be kept true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filed_by: Option<String>,
    /// The body's length when a model last read it and spoke; absent when nothing is
    /// outstanding. The words are on [`TaskDetail::sprawl_said`]. Cleared only by an
    /// edit that shrinks the body (`repo::update`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprawl_chars: Option<u32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<DateTime<Utc>>,
}
impl Task {
    /// The rank it sorts and renders as. ⚠ `escalated_to` first: reversed, it still
    /// compiles and the list stops agreeing with itself.
    pub fn urgency(&self) -> Option<Priority> {
        self.escalated_to.or(self.priority)
    }
}

/// What a write moved: one vocabulary for `task_events.kind` and a write's
/// `changed` list, which would drift if spelled at each site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Moved {
    Created,
    Edited,
    Status,
    Ranked,
    Due,
    Blocked,
    Assigned,
    /// Filed past a duplicate refusal.
    Overrode,
}

impl Moved {
    pub fn as_str(self) -> &'static str {
        match self {
            Moved::Created => "created",
            Moved::Edited => "edited",
            Moved::Status => "status",
            Moved::Ranked => "ranked",
            Moved::Due => "due",
            Moved::Blocked => "blocked",
            Moved::Assigned => "assigned",
            Moved::Overrode => "overrode",
        }
    }
}

/// A task after a write. An empty `changed` says the call did nothing; that is
/// reported, not refused, since a no-op is often right.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Updated {
    #[serde(flatten)]
    pub task: Task,
    /// The event kinds written, in the order written.
    pub changed: Vec<Moved>,
    /// What this edit displaced, if it replaced any text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaced: Option<Replaced>,
    /// When the text was last written, if this change closed the task without a
    /// rewrite ([`lifecycle::unwritten`](crate::tasks::lifecycle::unwritten)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unwritten: Option<DateTime<Utc>>,
}

/// What filing answers: the task, and hints about what was meant instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Created {
    #[serde(flatten)]
    pub task: Task,
    /// Tasks this filing names as `#<id>` that its filer closed recently, likely a
    /// continuation ([`lifecycle`](crate::tasks::lifecycle)).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub closed: Vec<crate::tasks::lifecycle::Closed>,
}

/// What an edit overwrote, told to whoever made it. Refuses nothing (sessions
/// may rewrite each other's text), but a writer told it landed on someone's
/// recent text can stop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Replaced {
    /// When the text this edit replaced was last written.
    pub at: DateTime<Utc>,
    /// Who wrote it, resolved like a history line's actor.
    pub by: String,
    /// Lengths before and after, so losing most of a body shows.
    pub was: usize,
    pub now: usize,
    /// Characters grown since the last edit that shrank it, this one included: the
    /// text nobody has read whole. Neither size nor edit count can show that. Zero on
    /// the consolidating edit.
    pub accreted: usize,
}

/// A task's previous version, restored whole; `Deserialize` because the CLI
/// checks it before restoring.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct Revision {
    /// When and by whom it was displaced, read off the event.
    pub at: DateTime<Utc>,
    pub actor: String,
    /// Whether the edit this would revert is the asker's, decided by the server on
    /// stored identity, not the rendered [`actor`](Self::actor) label
    /// ([`crate::tasks::undo`]).
    pub mine: bool,
    pub subject: String,
    pub body: String,
}

/// One task with its prose and its history — what opening a task returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDetail {
    #[serde(flatten)]
    pub task: Task,
    /// The body as written, in markdown.
    pub body: String,
    /// Rendered HTML for the app; the markdown for the CLI to edit.
    pub body_html: String,
    pub events: Vec<Event>,
    /// Whether there is a previous version to restore.
    pub restorable: bool,
    /// What a model last said about this body, kept for whoever opens it next.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprawl_said: Option<String>,
}

/// Something that happened to a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub at: DateTime<Utc>,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Who did it, resolved at read time ([`repo::events`](crate::tasks::repo::events)).
    pub actor: String,
}

/// Who is making a change, carried into every write for `task_events`.
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

/// Matches the column. A cap, not style: subjects are the per-turn cost, and
/// what doesn't fit on a line is a body.
pub const MAX_SUBJECT: usize = 200;
