//! What a task is.
//!
//! Two small closed vocabularies — a status and who is holding it — and the
//! records built from them. Both vocabularies are stored as `VARCHAR` and
//! parsed on the way out, so a value outside the set fails the query loudly
//! instead of arriving as a default.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, NaiveDate, Utc};
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
/// Two open states and two ways out. `Doing` is how a session that has picked
/// a task up tells the others not to start it.
///
/// ⚠ **`Dropped` is a closed task that was never done.** Without it an obsolete
/// task stays open for ever, or is closed as `Done` and credits somebody with
/// work nobody did. It buys an honest record and nothing else. There is no
/// *reason* field beside it: why it went is prose, and prose lives in the body.
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

    /// Whether this status is still work. A method rather than a comparison at
    /// each call site, so adding a state cannot quietly leave one behind; its
    /// SQL half is [`still_open!`](crate::still_open).
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

/// SQL for *a deadline close enough to raise the rank*, in exactly one place.
///
/// **Less than one week.** A constant here and not a setting: a threshold
/// somebody can change from a UI is one nobody can reason about.
///
/// Spelled once because the sort key, the projection that reports the raise,
/// and the guard that stops a `P0` claiming to have been raised must agree
/// about what day it is.
///
/// `<` rather than `<=`: *less than*. A task due exactly on the boundary is not
/// yet inside it, and `tests/blocking.rs` pins that.
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

/// SQL for *the body as a reader actually sees it*: trimmed at both ends, the
/// way `task show` trims it before printing.
///
/// ⚠⚠ **`TRIM()` IS NOT `trim()` — it removes SPACES AND NOTHING ELSE.** A body
/// of nothing but newlines therefore has a non-zero trimmed length, so the
/// service reports prose behind a task that prints as nothing and the app offers
/// to open an empty sheet — the exact thing `detailed` exists to prevent.
///
/// `[[:space:]]` rather than `\s` deliberately: a backslash in a SQL literal is
/// an escape that `NO_BACKSLASH_ESCAPES` turns off, and this would then trim
/// nothing while still looking right.
///
/// Spelled once because two projections must agree: whether there is a body,
/// and how many lines it is.
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

/// SQL for *this task is still work*, spelled in exactly one place.
///
/// ⚠ **The obvious spelling is the wrong one.** `status <> 'done'` counts a
/// [`Status::Dropped`] task as open, everywhere and silently.
///
/// A macro rather than a `const` because sqlx takes only `&'static str`: this
/// expands inside `concat!` and the compiler assembles the literal, so nothing
/// is built at runtime. The column is an argument because half these queries
/// join and have to qualify it.
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
/// no priority and always will — nobody is going back to triage the backlog. A
/// default of `P2` would have all of them assert something nobody said, so the
/// column is nullable and this type only describes a task somebody ranked.
///
/// ⚠ **Untriaged sorts as [`Priority::P2`] all the same** — [`Priority::rank`]
/// and `COALESCE(priority, 'P2')`. That is the decision the whole feature turns
/// on: `P0`/`P1` go above the untriaged pile and `P3`/`P4` *below* it, where
/// "ranked first, the rest after" would lift a task marked *when there is room*
/// above everything nobody has read. Within a rank, id order.
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
    /// ⚠ **Each one is a TEST that can fail, not a degree of feeling.** A scale
    /// of importance has no anchor: every filer argues their item above the
    /// line, and it drifts until everything is `P0`. These are distinct
    /// SITUATIONS applied as a cascade — the first test that passes is the rank —
    /// answerable about a ticket, so two conversations reach the same answer.
    ///
    /// ⚠ **A full range is a check on the RANKING, never a quota on the
    /// tickets.** Mostly `P0` means the tests are applied loosely, all `P2` that
    /// the ranker is not reading; moving tickets to fill a bucket fixes neither.
    ///
    /// Printed by `task --help`, which is where it will actually be read.
    pub fn gloss(self) -> &'static str {
        match self {
            Priority::P0 => "damage is accruing — every hour it stays open costs more",
            Priority::P1 => "nothing is accruing, but other work is waiting on this",
            Priority::P2 => "ordinary work, nothing waiting on it — where UNRANKED sits",
            Priority::P3 => "a workaround exists and is in use; what it costs is friction",
            Priority::P4 => "nothing is being paid for it today; not recited in your prompt",
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
    /// The Rust twin of the SQL's `COALESCE(priority, 'P2')`; `tests/priority.rs`
    /// compares the two against a real database.
    pub fn rank(this: Option<Priority>) -> Priority {
        this.unwrap_or(Priority::P2)
    }
}

/// What a filer said about urgency. **There is no "did not say".**
///
/// ⚠ **This type exists so that omission is not a state.** `Option<Priority>`
/// fits a task that exists, but at FILING it lets a client skip the question,
/// and `None` would mean both *nobody has judged this* and *nobody was asked*.
///
/// [`Ranking::Unassessed`] is kept as an answer on purpose: a required field
/// with an obvious safe answer gets filled reflexively, and everything ends up
/// `P2`. An honest *I am not judging this* is worth more.
///
/// ⚠ **It changes no ordering** — both sort at `P2` via [`Priority::rank`]. What
/// it buys is that `P2` means **somebody looked and called it ordinary**.
///
/// The wire form is `Priority` or `null`, and an ABSENT key is a
/// deserialisation error. Removing `#[serde(default)]` does NOT make an
/// `Option` field mandatory, which is the wrong first attempt this invites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ranking {
    /// Judged, at this level.
    At(Priority),
    /// Explicitly not judged. Sorts as `P2`; means *nobody has assessed this*.
    Unassessed,
}

impl Ranking {
    /// What goes in the column: `NULL` for unassessed.
    pub fn stored(self) -> Option<Priority> {
        match self {
            Ranking::At(priority) => Some(priority),
            Ranking::Unassessed => None,
        }
    }
}

impl<'de> Deserialize<'de> for Ranking {
    /// `null` is [`Ranking::Unassessed`]; a string must be a level; **absent is
    /// an error**, which is the entire point of this impl.
    ///
    /// ⚠ **`deserialize_any`, and NOT `Option::<Priority>::deserialize`.** The
    /// obvious spelling silently accepts a filing with no `priority` key: serde
    /// fills a missing field through `missing_field`, whose deserialiser rejects
    /// everything EXCEPT `deserialize_option`, which it answers with
    /// `visit_none`. Delegating to `Option` opts straight into the fallback this
    /// type exists to refuse. `deserialize_any` takes the path it errors on.
    ///
    /// The two spellings differ by one line and on one input, so keep the test.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Stated;

        impl<'de> serde::de::Visitor<'de> for Stated {
            type Value = Ranking;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a priority (\"P0\" to \"P4\"), or null for unassessed")
            }

            /// [`Priority`]'s own derive, not `FromStr`: the CLI's parser
            /// case-folds so a hand can type `p0`, and the wire should not.
            fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<Ranking, E> {
                Priority::deserialize(serde::de::value::StrDeserializer::<E>::new(s))
                    .map(Ranking::At)
            }

            /// JSON `null` through `deserialize_any`.
            fn visit_unit<E: serde::de::Error>(self) -> Result<Ranking, E> {
                Ok(Ranking::Unassessed)
            }

            /// `null` reached through a self-describing format that models it as
            /// an option instead. Both spellings mean the same answer.
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
        // Case-folded for the CLI: `p0` is what a hand types. The bare digit is
        // accepted for the same reason. The stored spelling is `P<n>`.
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
/// ⚠ **The body is deliberately not here**: a list carrying bodies costs far
/// more than the lines it renders. [`TaskDetail`] carries prose, one task at a
/// time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: u64,
    pub subject: String,
    pub status: Status,
    /// How urgent, when somebody has said. Absent is the ordinary case and is
    /// not a level — see [`Priority`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    /// The day this has to be done by, when something outside decides.
    ///
    /// ⚠ **A day, not an instant** — *by the 14th*. A time would invent
    /// precision nobody stated and make every reader choose a timezone.
    ///
    /// ⚠ **It does not reorder anything by itself.** How long the work takes
    /// would decide *what next*, and nothing records it, so a date is evidence
    /// for a rank that a person makes — except inside the week, see
    /// [`escalated_to`](Self::escalated_to).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due: Option<NaiveDate>,
    /// What this sorts as instead, when a near deadline has raised it.
    ///
    /// ⚠ **Absent is the ordinary case, and present always means
    /// [`Priority::P0`].** Inside the deadline window a task sorts as `P0`
    /// whatever it was set to, and [`priority`](Self::priority) still holds what
    /// somebody actually chose.
    ///
    /// ⚠ **Derived at read time, never written**: stamping `P0` into the row
    /// would edit history and need a scheduler. Recomputed from `due` and the
    /// clock, it cannot drift.
    ///
    /// ⚠ **A value rather than a flag, so no renderer knows the rule**: each
    /// draws [`Task::urgency`], and the window and level live in
    /// [`due_soon!`](crate::due_soon). With a fixed date and work remaining,
    /// `P0`'s own test does pass: every hour costs more.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub escalated_to: Option<Priority>,
    /// Whether [`due`](Self::due) has passed, by the database's clock.
    ///
    /// Derived server-side, or the CLI and the app would each compare against
    /// their own idea of today.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub overdue: bool,
    /// The tasks this one is waiting for, oldest id first. Usually empty.
    ///
    /// ⚠ **A LIST**: with one slot, a second blocker would go in the body and
    /// go stale there.
    ///
    /// ⚠ **It carries a rule about [`priority`](Self::priority).** A task may not
    /// be ranked more urgently than what blocks it — equal is allowed — because
    /// *do this next* about something you cannot start empties the scale. With
    /// several blockers the bound is the LEAST urgent open one, which decides
    /// when this can start.
    ///
    /// ⚠ **Kept when a blocker closes.** The dependency is a fact about how the
    /// work went; what stops is the effect, which is [`blocked`](Self::blocked).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_on: Vec<u64>,
    /// Whether any of [`blocked_on`](Self::blocked_on) is still open — resolved
    /// through the projection, so no reader needs a second query and no client
    /// has to know that a closed blocker does not count.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub blocked: bool,
    pub assignee: Assignee,
    /// Whether there is prose behind it worth opening. A task written as a
    /// one-line reminder has none, and offering to open an empty sheet is worse
    /// than not offering.
    pub detailed: bool,
    /// How many lines the body prints as, `0` when there is none.
    ///
    /// ⚠ **So a reader that truncates knows what it truncated** — a read piped
    /// to `head` otherwise looks complete. Unconditional, not a flag over a
    /// threshold: the reader who needs it is the one who cannot see it coming.
    pub body_lines: u32,
    /// What the session that filed it calls itself — `observe`, `dev-lint`.
    /// Absent when Pippijn filed it, or the filing session has no name.
    ///
    /// ⚠ **A hint about where the work lives, deliberately not a filter**: a
    /// session scanning the pile learns from it without opening the task, and
    /// a filter by where work lives hides work.
    ///
    /// **A fact rather than a field**: read from the `created` event and
    /// resolved through the join like a holder's, so nothing has to be kept true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filed_by: Option<String>,
    /// How long this body was when a model last read it and had something to
    /// say — absent when nothing is outstanding.
    ///
    /// ⚠ **The number, not the words**: a list must not carry prose, so the
    /// critique is on [`TaskDetail::sprawl_said`].
    ///
    /// ⚠ **Present means the LAST read spoke, not that it ever did.** Cleared
    /// only by an edit that makes the body smaller — see `repo::update`, which
    /// keeps this and the sampler agreeing on what counts as consolidating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprawl_chars: Option<u32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<DateTime<Utc>>,
}
impl Task {
    /// The rank this actually sorts and renders as.
    ///
    /// ⚠⚠ **`escalated_to` first, and never the other way round.** Reading
    /// `priority` alone hides the rank a near deadline raised; reversed, nothing
    /// fails to compile and the list stops agreeing with itself. Every renderer
    /// calls this rather than spelling the rule.
    pub fn urgency(&self) -> Option<Priority> {
        self.escalated_to.or(self.priority)
    }
}

/// What a write moved, in the words the history uses.
///
/// ⚠⚠ **One vocabulary** for `task_events.kind` and a write's `changed` list,
/// which name the same facts: spelled by hand at each site, they drift, and a
/// client keying off one disagrees with a reader of the other.
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
        }
    }
}

/// A task after a write, and what the write actually moved.
///
/// ⚠ **An empty `changed` means the call did nothing, and says so.** Otherwise a
/// no-op answers exactly like a write that worked.
///
/// **Reported rather than refused**: a no-op is often correct — `start` on a
/// task already yours is meant to be quiet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Updated {
    #[serde(flatten)]
    pub task: Task,
    /// The event kinds written, in the order written.
    pub changed: Vec<Moved>,
    /// What this edit displaced, when it displaced any text. Absent for a
    /// change that moved only a status, a rank or a holder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaced: Option<Replaced>,
    /// When this change closed the task without anything having been written
    /// to it since it last moved: when its text was last written. See
    /// [`lifecycle::unwritten`](crate::tasks::lifecycle::unwritten).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unwritten: Option<DateTime<Utc>>,
}

/// What filing a task answers: the task, and what the filing may have meant
/// instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Created {
    #[serde(flatten)]
    pub task: Task,
    /// Tasks this filing names as `#<id>` that its filer closed recently —
    /// usually a sign it continues one and `task reopen` was meant. See
    /// [`lifecycle`](crate::tasks::lifecycle).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub closed: Vec<crate::tasks::lifecycle::Closed>,
}

/// What an edit overwrote, told to whoever made it.
///
/// ⚠ **This is the whole of the prevention, and it refuses nothing.** The loss
/// is a session writing a body from a stale snapshot, but sessions rewrite each
/// other's words by standing permission, so the write goes through and says
/// what it landed on — a writer told the text was rewritten recently by
/// somebody else can stop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Replaced {
    /// When the text this edit replaced was last written.
    pub at: DateTime<Utc>,
    /// Who wrote it, resolved the same way a history line's actor is.
    pub by: String,
    /// Body length before and after, in characters: a rewrite that loses most
    /// of a body says so even when nobody reads the dates.
    pub was: usize,
    pub now: usize,
    /// How much this body has grown, in characters, since the last edit that
    /// made it smaller — this one included.
    ///
    /// ⚠ **Neither a size nor a count of edits.** A size cannot tell a body just
    /// rewritten from one that has doubled unread, and an edit count cannot tell
    /// typo fixes from dumps. This is the text nobody has read as a whole —
    /// exactly what goes stale in place.
    ///
    /// Zero on the edit that consolidates, because that edit is the answer.
    pub accreted: usize,
}

/// A task as it stood before an edit — one complete previous version.
///
/// Both columns, always: a revision is restored as a unit. `Deserialize` too,
/// because the CLI reads it back to decide whether restoring is safe.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct Revision {
    /// When the edit that displaced this text was made, and by whom. Read off
    /// the event this revision hangs from rather than stored a second time.
    pub at: DateTime<Utc>,
    pub actor: String,
    /// Whether the edit this would revert was made by whoever is asking.
    ///
    /// ⚠ **Answered by the server on the stored identity, not by comparing
    /// [`actor`](Self::actor)**, a rendered label two conversations can share
    /// and a rename changes. See [`crate::tasks::undo`] for why it matters.
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
    /// The body rendered. Both are sent: the app shows the HTML, and a session
    /// reading through the CLI wants the markdown it will edit.
    pub body_html: String,
    pub events: Vec<Event>,
    /// Whether an edit has replaced text here, so there is something to put
    /// back — answered without sending a revision the client mostly will not
    /// want.
    pub restorable: bool,
    /// What a model last said about this body, verbatim, when it had something
    /// to say.
    ///
    /// ⚠ **Stored so it reaches whoever opens the task next**, rather than only
    /// the session whose edit prompted it.
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
    /// Who did it, resolved to a readable label at READ time — see
    /// [`repo::events`](crate::tasks::repo::events).
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
