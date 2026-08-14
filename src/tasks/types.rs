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
/// SQL for *a deadline close enough to raise the rank*, in exactly one place.
///
/// Pippijn's rule, 2026-08-11: **less than one week**. Seven days is his number
/// rather than a guess, which is why it is a constant here and not a setting —
/// a threshold somebody can change from a UI is one nobody can reason about.
///
/// Spelled once because it appears three times: the sort key, the projection
/// that reports the raise, and the guard that stops a task already at `P0`
/// claiming to have been raised. Three copies of a date comparison are three
/// chances to disagree about what day it is.
///
/// `<` rather than `<=`: *less than* a week. A task due exactly seven days out
/// is not yet inside it, and `tests/blocking.rs` pins that boundary.
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
    /// ⚠ **Each one is a TEST that can fail, not a degree of feeling.** This is
    /// the difference between a scale that holds and one that inflates. A single
    /// axis — most important, quite important, less so — has no anchor, so every
    /// filer argues their own item is above the line, the line drifts up, and
    /// the end state is a spreadsheet where everything is `P0` and somebody
    /// invents `P-1`. Pippijn has watched that happen and asked for it not to
    /// happen here (2026-08-11).
    ///
    /// So these are five distinct SITUATIONS, applied as a cascade — the first
    /// test that passes is the rank. *Is damage accruing?* *Is something else
    /// waiting?* *Is there a workaround in use?* *Would this be kept only as a
    /// record?* Each is answerable about a ticket rather than felt about it,
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
            Priority::P4 => "kept as a record rather than a plan; it may never happen",
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
/// Asked for by Pippijn 2026-08-11: "I want everything to have a priority",
/// with [`Ranking::Unassessed`] kept deliberately as the second answer rather
/// than removed. A required field whose safe answer is obvious gets filled in
/// reflexively — that is how everything ends up `P2` and the rank stops meaning
/// anything, which is the same failure as everything ending up `P0`. An honest
/// *I am not judging this* is worth more than a number nobody stood behind.
///
/// ⚠ **It changes no ordering.** Both answers still sort at `Priority::P2` via
/// [`Priority::rank`] and the SQL's `COALESCE(priority, 'P2')`. What it buys is
/// that `P2` now means **somebody looked and called it ordinary**.
///
/// The wire form is `Priority` or `null`, and the ABSENCE of the key is a
/// deserialisation error — which is the whole mechanism. A non-`Option` field
/// is required by serde's derive; an `Option` one is not, whatever attributes it
/// carries. That surprise cost a wrong first attempt here: `#[serde(default)]`
/// was removed from `NewTask::priority` on the belief it made the field
/// mandatory, and `tests/priority.rs::filing` caught that it did not.
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
    /// ⚠ **`deserialize_any`, and NOT `Option::<Priority>::deserialize`.** That
    /// obvious spelling is what this was written as first, and it silently
    /// accepted a filing with no `priority` key at all. Serde fills a missing
    /// field through `missing_field`, whose deserialiser rejects everything
    /// EXCEPT `deserialize_option` — which it answers with `visit_none`. So
    /// delegating to `Option` opts straight into the fallback this type exists
    /// to refuse, and the field reads as `Unassessed` when nobody was asked.
    /// Going through `deserialize_any` takes the path `missing_field` errors on.
    ///
    /// `tests/priority.rs::filing::omitting_priority_is_refused` is the guard,
    /// and it caught this. It is worth keeping precisely because the two impls
    /// differ by one line and behave identically on every input except the one
    /// that matters.
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
    /// [`Priority::P0`].** Pippijn, 2026-08-11: *"can we make it P0 only when
    /// there's less than 1 week until deadline?"* — so inside that week a task
    /// sorts as `P0` whatever it was set to, and [`priority`](Self::priority)
    /// still holds what somebody actually chose.
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
    /// ⚠ **A LIST, and the first cut of this was a single id.** The measurement
    /// said no open task named more than one blocker, which is not evidence:
    /// there was nowhere to record even one, so it counted the absence of the
    /// feature. Pippijn caught it (2026-08-11). With one slot the workaround for
    /// a second blocker is the body, which is the staleness this replaced.
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
    /// What this edit displaced, when it displaced any text. Absent for a
    /// change that moved only a status, a rank or a holder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaced: Option<Replaced>,
}

/// What an edit overwrote, told to whoever made it.
///
/// ⚠ **This is the whole of the prevention, and it refuses nothing.** The loss
/// on 2026-08-14 was a session writing a body from a three-day-old snapshot it
/// had never re-read; a gate on that would have to refuse an ordinary,
/// permitted operation — sessions rewrite each other's task words by standing
/// permission — and the duplicate check in `duplicates.rs` records what
/// refusing a frequent correct operation costs. So the write goes through and
/// says what it landed on. A writer who believes a body is three days old, told
/// it was rewritten yesterday by somebody else, has everything needed to stop.
#[derive(Debug, Clone, Serialize)]
pub struct Replaced {
    /// When the text this edit replaced was last written.
    pub at: DateTime<Utc>,
    /// Who wrote it, resolved the same way a history line's actor is.
    pub by: String,
    /// Body length before and after, in characters. A rewrite that loses two
    /// thirds of a body says so here even when nobody reads the dates.
    pub was: usize,
    pub now: usize,
}

/// A task as it stood before an edit — one complete previous version.
///
/// Both columns, always: a revision is restored as a unit, so there is no state
/// in which a subject comes from one moment and a body from another.
#[derive(Debug, Clone, Serialize)]
pub struct Revision {
    /// When the edit that displaced this text was made, and by whom. Read off
    /// the event this revision hangs from rather than stored a second time.
    pub at: DateTime<Utc>,
    pub actor: String,
    pub subject: String,
    pub body: String,
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
