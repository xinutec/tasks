//! Ending a wait, and saying how it ended.
//!
//! A session that hits a problem somebody else has to fix files the task,
//! records the edge with `--blocked-on`, and then has nothing to do. The digest
//! already marks the waiting task and clears the mark when the blocker closes,
//! but a digest renders when the session takes a turn and a blocked session is
//! not taking turns.
//!
//! So the wake cannot be delivered; it has to be something the session already
//! holds. `task wait` is that — a background command whose returning is what
//! brings the session back.
//!
//! ⚠ **The sleeping is not the interesting part and is not here.** What is here
//! is the decision: whether the wait is over, and what the returning session is
//! told. Those are the two things a hand-rolled shell loop gets wrong.

use std::time::Duration;

use crate::tasks::types::Status;

/// Whether the wait is over, and how it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Still blocked, on these — in the order they were named.
    Waiting(Vec<u64>),
    /// Every blocker finished. This is the only ending that means *carry on*.
    Done,
    /// Everything is closed, but these were dropped rather than done.
    ///
    /// ⚠ **A separate answer.** `drop` is *overtaken, obsolete, or decided
    /// against* — the problem was not fixed. Folding it into
    /// [`Done`](Verdict::Done) would resume the session on a premise that has
    /// been explicitly refused, which is worse than never waking it.
    Dropped(Vec<u64>),
}

/// Read the blockers' statuses into an ending.
///
/// ⚠ **Through [`Status::is_open`], never `== Status::Open`.** A blocker
/// somebody has picked up is `doing` and is still a blocker; the obvious
/// comparison ends the wait the moment the fix STARTS.
pub fn verdict(blockers: &[(u64, Status)]) -> Verdict {
    let open: Vec<u64> = blockers
        .iter()
        .filter(|(_, status)| status.is_open())
        .map(|(id, _)| *id)
        .collect();
    if !open.is_empty() {
        return Verdict::Waiting(open);
    }
    let dropped: Vec<u64> = blockers
        .iter()
        .filter(|(_, status)| *status == Status::Dropped)
        .map(|(id, _)| *id)
        .collect();
    if dropped.is_empty() {
        Verdict::Done
    } else {
        Verdict::Dropped(dropped)
    }
}

/// How eagerly to ask, given how long this has been waiting already.
const EAGER: Duration = Duration::from_secs(5);
const LAZY: Duration = Duration::from_secs(30);
/// How long the eager phase lasts.
const KEENLY: Duration = Duration::from_secs(120);

/// How long to sleep before asking again.
///
/// ⚠ **Two rates, because the cases are minutes and days apart.** A blocker
/// closed while somebody is at the keyboard should wake the session promptly;
/// one closed tomorrow can afford a slower poll.
///
/// ⚠ **A ceiling, not a ramp.** A wait that has run for a week still answers
/// within `LAZY`: a backoff that keeps growing makes the longest waits — the
/// ones most likely to be forgotten — the slowest to come back.
pub fn interval(waited: Duration) -> Duration {
    if waited < KEENLY { EAGER } else { LAZY }
}

/// What the returning session reads.
///
/// ⚠ **The ids, not just the outcome.** The session may have been away for
/// days, and the task number is what reconnects the wake to the work it stopped
/// for. `asked` is what the wait was called with, so a partial ending can say
/// which of them.
pub fn said(verdict: &Verdict, asked: &[u64]) -> String {
    match verdict {
        Verdict::Done => format!("{} closed — carry on.", list(asked)),
        // ⚠ **Said once when there is one**: naming it twice reads as two tasks.
        Verdict::Dropped(ids) if ids == asked => format!(
            "{} was dropped rather than done — overtaken, obsolete, or decided \
             against. Whatever this was waiting for did NOT happen; read it \
             before continuing.",
            list(ids)
        ),
        Verdict::Dropped(ids) => format!(
            "{} closed, but {} was dropped rather than done — overtaken, obsolete, \
             or decided against. Whatever this was waiting for did NOT happen; \
             read it before continuing.",
            list(asked),
            list(ids)
        ),
        Verdict::Waiting(ids) => format!(
            "gave up waiting: {} is still open. Nothing about the work has \
             changed — run the wait again, or pick something else up.",
            list(ids)
        ),
    }
}

/// Task numbers as somebody would say them: one, or `a, b and c`.
fn list(ids: &[u64]) -> String {
    let said: Vec<String> = ids.iter().map(|id| format!("#{id}")).collect();
    match said.split_last() {
        None => "nothing".to_string(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}
