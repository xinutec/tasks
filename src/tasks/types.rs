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

/// SQL for *a deadline close enough to raise the rank*, in exactly one place.
///
/// **Less than one week.** A constant here and not a setting: a threshold
/// somebody can change from a UI is one nobody can reason about.
///
/// Spelled once because it appears in the sort key, the projection that reports
/// the raise, and the guard that stops a task already at `P0` claiming to have
/// been raised. Three copies of a date comparison are three chances to disagree
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
/// to open an empty sheet — the exact thing `detailed` exists to prevent. It
/// surfaced only when a second expression over the same column had to agree.
///
/// `[[:space:]]` rather than `\s` deliberately: a backslash in a SQL literal is
/// an escape that `NO_BACKSLASH_ESCAPES` turns off, and this would then trim
/// nothing while still looking right.
///
/// Spelled once because two projections depend on agreeing: whether there is a
/// body, and how many lines it is. Two definitions of *trimmed* are two answers
/// about one body.
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
/// ⚠ **The obvious spelling is the wrong one.** `status <> 'done'` means "open"
/// only while `done` is the only closed state, and stops being true the moment
/// [`Status::Dropped`] exists — after which a dropped task goes on being counted
/// as open everywhere, none of it failing loudly.
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
    /// ⚠ **Each one is a TEST that can fail, not a degree of feeling.** This is
    /// the difference between a scale that holds and one that inflates. A single
    /// axis — most important, quite important, less so — has no anchor, so every
    /// filer argues their own item is above the line, the line drifts up, and
    /// the end state is a spreadsheet where everything is `P0` and somebody
    /// invents `P-1`.
    ///
    /// So these are five distinct SITUATIONS, applied as a cascade — the first
    /// test that passes is the rank. *Is damage accruing?* *Is something else
    /// waiting?* *Is there a workaround in use?* *Is anything being paid
    /// for it today?* Each is answerable about a ticket rather than felt about it,
    /// which is what lets two conversations reach the same answer.
    ///
    /// ⚠ **A full range is a check on the RANKING, never a quota on the
    /// tickets.** If a pass comes back mostly `P0` the tests are being applied
    /// loosely; if it comes back all `P2` the ranker is not reading. Neither is
    /// fixed by moving tickets to fill a bucket — that is the curve-grading that
    /// makes the whole column a fiction.
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
    /// The Rust twin of the SQL's `COALESCE(priority, 'P2')`, and the two must
    /// agree — `tests/priority.rs` compares them against a real database rather
    /// than trusting that they were written on the same afternoon.
    pub fn rank(this: Option<Priority>) -> Priority {
        this.unwrap_or(Priority::P2)
    }
}

/// What a filer said about urgency. **There is no "did not say".**
///
/// ⚠ **This type exists so that omission is not a state.** `Option<Priority>` is
/// the right shape for a task that already exists — most are unranked and always
/// will be — but at the moment of FILING it lets a client skip the question
/// entirely, and then `None` means two different things: *nobody has judged
/// this* and *nobody was asked*. This says which.
///
/// [`Ranking::Unassessed`] is kept deliberately as the second answer rather than
/// removed. A required field whose safe answer is obvious gets filled in
/// reflexively — that is how everything ends up `P2` and the rank stops meaning
/// anything, the same failure as everything ending up `P0`. An honest *I am not
/// judging this* is worth more than a number nobody stood behind.
///
/// ⚠ **It changes no ordering.** Both answers still sort at `Priority::P2` via
/// [`Priority::rank`] and the SQL's `COALESCE(priority, 'P2')`. What it buys is
/// that `P2` now means **somebody looked and called it ordinary**.
///
/// The wire form is `Priority` or `null`, and the ABSENCE of the key is a
/// deserialisation error — which is the whole mechanism. A non-`Option` field
/// is required by serde's derive; an `Option` one is not, whatever attributes it
/// carries. Removing `#[serde(default)]` does NOT make an `Option` field
/// mandatory, which is the wrong first attempt this type invites.
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
    /// The two impls differ by one line and behave identically on every input
    /// except the one that matters, so the test is worth keeping.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Stated;

        impl<'de> serde::de::Visitor<'de> for Stated {
            type Value = Ranking;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a priority (\"P0\" to \"P4\"), or null for unassessed")
            }

            /// Deferred to [`Priority`]'s own derive rather than to `FromStr`:
            /// the CLI's parser case-folds so a hand can type `p0`, and the wire
            /// should not. One spelling on the wire, and no second list to drift.
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
        // Case-folded for the CLI's sake — `p0` is what a hand types — and the
        // stored spelling is the upper one, which is what the SQL sorts on.
        match s.to_ascii_uppercase().as_str() {
            // The bare digit is accepted because people type it and there is
            // no other meaning it could carry. The stored spelling is `P<n>`.
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
/// ⚠ **The body is deliberately not here.** This struct is what a list
/// serialises and a hook injects, and a list carrying bodies costs an order of
/// magnitude more than the lines it renders. [`TaskDetail`] carries prose, one
/// task at a time.
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
    /// ⚠ **A day, not an instant**, because that is what a deadline is — *before
    /// Sep 2026*, *by the 14th*. A time would invent precision nobody stated and
    /// make every reader choose a timezone to compare in.
    ///
    /// ⚠ **It does not reorder anything.** A deadline is evidence for a rank,
    /// not a competing answer to *what next*: how long the work takes is the
    /// term that would decide, and nothing records it. So a near date argues for
    /// a rank and a person makes it. See `repo::list`, still the only sort.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due: Option<NaiveDate>,
    /// What this sorts as instead, when a near deadline has raised it.
    ///
    /// ⚠ **Absent is the ordinary case, and present always means
    /// [`Priority::P0`].** Inside the deadline window a task sorts as `P0`
    /// whatever it was set to, and [`priority`](Self::priority) still holds what
    /// somebody actually chose.
    ///
    /// ⚠ **Derived at read time, never written.** A job that stamped `P0` into
    /// the row when the week arrived would edit history nobody asked for and
    /// need a scheduler to be correct. This is recomputable from `due` and the
    /// clock, so it cannot drift and cannot be wrong in the database.
    ///
    /// ⚠ **Carried as a value rather than a flag so no renderer has to know the
    /// rule.** The CLI, the app and the digest each draw
    /// `escalated_to.unwrap_or(priority)`; the week and the level it escalates
    /// to live in one place, in SQL.
    ///
    /// This is also the case where `P0`'s own test starts passing: with a fixed
    /// date and work remaining, every hour really does cost more, because the
    /// hours are the resource being spent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub escalated_to: Option<Priority>,
    /// Whether [`due`](Self::due) has passed, by the database's clock.
    ///
    /// Derived server-side for the same reason [`blocked`](Self::blocked) is:
    /// otherwise the CLI and the app each compare against their own idea of
    /// today, which is two copies of one rule and one timezone away from
    /// disagreeing. Overdue is a fact; *due soon* would need a threshold, so
    /// there is deliberately no such flag.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub overdue: bool,
    /// The tasks this one is waiting for, oldest id first. Usually empty.
    ///
    /// ⚠ **A LIST, and the first cut was a single id.** "No task names more
    /// than one blocker" is not evidence when there is nowhere to record even
    /// one — that measures the absence of the feature. With one slot the
    /// workaround for a second blocker is the body, which is the staleness this
    /// replaced.
    ///
    /// ⚠ **It carries a rule about [`priority`](Self::priority), not just a
    /// link.** A task may not be ranked more urgently than the thing blocking
    /// it — equal is allowed, higher is refused — because claiming *do this
    /// next* about something you cannot start is how a scale stops meaning
    /// anything. With several blockers the bound is the LEAST urgent open one:
    /// that is the one that decides when this can actually start.
    ///
    /// ⚠ **Kept when a blocker closes rather than cleared.** The dependency is a
    /// fact about how the work went; what stops is the *effect*. So a non-empty
    /// list is not the same as being blocked, and [`blocked`](Self::blocked) is
    /// the question a reader is actually asking.
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
    /// ⚠ **So a reader that truncates knows what it truncated.**
    /// [`detailed`](Self::detailed) answers *is there prose*; this answers *how
    /// much*, which is what anyone piping to `head` is asking. Bodies routinely
    /// run longer than a screen, and a truncated read that looks exactly like a
    /// complete one is the failure this exists to stop.
    ///
    /// ⚠ **A count, not a flag, and unconditional.** A threshold would carry the
    /// number only on bodies already known to be long — the one case a reader
    /// can see coming. The reader who needs it is the one who cannot.
    pub body_lines: u32,
    /// What the session that filed it calls itself — `observe`, `health`,
    /// `dev-lint`. Absent when Pippijn filed it, or when the filing session had
    /// not named itself.
    ///
    /// ⚠ **A hint about where the work lives, and deliberately not a filter.**
    /// The repo column was retired because a session spans checkouts and *which
    /// repo is this in* had no single answer. That removed two things at once
    /// and only one was wrong: *which sessions should be shown this* was a
    /// filter and it hid work; *where does this work live* is a hint, and
    /// without it a session scanning the pile must open a task to learn the
    /// answer is no.
    ///
    /// **A fact rather than a field**: `task_events` already records who filed
    /// every task, so there is nothing to set and nothing to keep true.
    /// Resolved through the join like a holder's, so a session that renames
    /// itself is called the same thing everywhere at once.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filed_by: Option<String>,
    /// How long this body was when a model last read it and had something to
    /// say — absent when nothing is outstanding.
    ///
    /// ⚠ **The number, not the words.** The critique itself is on
    /// [`TaskDetail::sprawl_said`], because a list must not carry prose: this is
    /// the same trade [`detailed`](Self::detailed) makes, and for the same
    /// reason: otherwise every row crosses the wire carrying a paragraph nobody
    /// asked to read.
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
    /// ⚠⚠ **`escalated_to` first, and never the other way round.** A deadline
    /// inside the week raises a task to `P0` without anybody writing one, so
    /// reading `priority` alone shows the chosen rank and hides the effective
    /// one — a task the escalation exists to raise reads as ordinary. Reversed
    /// to `priority.or(escalated_to)` nothing fails to compile and nothing fails
    /// a test; the list simply stops agreeing with itself.
    ///
    /// ⚠ **Spelled once because it was spelled four times** — `digest::parked`,
    /// `focus::breaks_through`, `digest::line` and the CLI's own renderer, with
    /// `focus.rs`'s comment already noting it is "the same `escalated_to ??
    /// priority` every renderer draws" and nothing holding them together. Same
    /// reason `still_open!` exists.
    pub fn urgency(&self) -> Option<Priority> {
        self.escalated_to.or(self.priority)
    }
}

/// What a write moved, in the words the history uses.
///
/// ⚠⚠ **One vocabulary, because there were two and they had already drifted.**
/// `task_events.kind` and the `changed` list on a write's response name the same
/// facts, and spelled by hand at every site they diverged within one function —
/// `ranked` into the history against `priority` into the response. A client
/// keying off one and a reader of the other then disagree about what happened.
///
/// The [`Status`] lesson at one remove: a vocabulary spelled by hand at N sites
/// drifts at the first addition.
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
/// ⚠ **An empty `changed` means the call did nothing, and saying so is the whole
/// point.** A write that changes nothing otherwise answers exactly like one that
/// worked — `start` on a task already `doing`, a rename to a blank name, closing
/// into the pile — and each such defect is only findable by reproducing it.
///
/// **Reported rather than refused.** A no-op is often correct — `start` on a
/// task already yours is meant to be quiet, and refusing it would trade a silent
/// success for a spurious failure. What was missing was never the refusal; it
/// was the sentence.
///
/// The vocabulary is `task_events`' own — `status`, `assigned`, `edited` — so
/// what a write reports and what the history records cannot drift into two
/// spellings of the same event.
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
}

/// What an edit overwrote, told to whoever made it.
///
/// ⚠ **This is the whole of the prevention, and it refuses nothing.** The loss
/// it addresses is a session writing a body from a stale snapshot it never
/// re-read, and gating that would mean refusing an ordinary permitted operation:
/// sessions rewrite each other's task words by standing permission. So the write
/// goes through and says what it landed on — a writer who believes a body is
/// days old, told it was rewritten yesterday by somebody else, can stop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Replaced {
    /// When the text this edit replaced was last written.
    pub at: DateTime<Utc>,
    /// Who wrote it, resolved the same way a history line's actor is.
    pub by: String,
    /// Body length before and after, in characters. A rewrite that loses two
    /// thirds of a body says so here even when nobody reads the dates.
    pub was: usize,
    pub now: usize,
    /// How much this body has grown, in characters, since the last edit that
    /// made it smaller — this one included.
    ///
    /// ⚠ **Characters since the last consolidation, which is neither a size nor
    /// a count of edits.** An absolute size cannot tell a long body somebody has
    /// just rewritten from a short one that has doubled since anyone read it,
    /// and a count of edits cannot tell typo fixes from wholesale dumps. What is
    /// measured is the text nobody has read as a whole, which is exactly the
    /// text that goes stale in place.
    ///
    /// Zero on the edit that consolidates, because that edit is the answer.
    pub accreted: usize,
}

/// A task as it stood before an edit — one complete previous version.
///
/// Both columns, always: a revision is restored as a unit, so there is no state
/// in which a subject comes from one moment and a body from another.
/// `Deserialize` as well as `Serialize`, unlike its neighbours: the CLI reads
/// this one back to decide whether restoring is safe, and a hand-rolled read of
/// `mine` out of a `Value` would be a second copy of the shape to keep level.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct Revision {
    /// When the edit that displaced this text was made, and by whom. Read off
    /// the event this revision hangs from rather than stored a second time.
    pub at: DateTime<Utc>,
    pub actor: String,
    /// Whether the edit this would revert was made by whoever is asking.
    ///
    /// ⚠ **Answered here rather than by comparing [`actor`](Self::actor)**, which
    /// is a rendered label — a session's display name, or a person's id. Two
    /// conversations can be renamed to the same words, and a rename would make a
    /// caller's own edit stop looking like theirs. The comparison is on the
    /// stored identity, which is why it is the server's answer and not the
    /// client's.
    ///
    /// Restoring is not undoing *your* last edit — it is undoing *the* last
    /// edit, whoever made it, because one version is kept per task and not per
    /// actor. This is what lets a caller tell those apart before it acts.
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
    /// back. Answered in SQL rather than by handing the client a revision it
    /// mostly will not want — the same reasoning as [`Task::detailed`].
    pub restorable: bool,
    /// What a model last said about this body, verbatim, when it had something
    /// to say.
    ///
    /// ⚠ **Stored because it used to evaporate.** Printed once as the tail of a
    /// successful edit, to a session recording a finding rather than judging a
    /// document, and then gone. Kept here it reaches whoever opens the task
    /// next, who it was always about.
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
