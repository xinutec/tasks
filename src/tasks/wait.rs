//! Ending a wait, and saying how it ended.
//!
//! A session that hits a problem somebody else has to fix files the task,
//! records the edge with `--blocked-on`, and then has nothing to do. The edge is
//! already visible — the digest marks the waiting task `⛔#1350` and stops the
//! moment the blocker closes — but a digest is rendered when the session takes a
//! turn, and a blocked session is not taking turns. Nothing reaches a
//! conversation that is not speaking.
//!
//! So the wake cannot be delivered; it has to be something the session is
//! already holding. `task wait` is that: a command run in the background, which
//! returns when the blockers close, and whose returning is what brings the
//! session back. Pippijn, 2026-09-05: *"It's not about direct communication. The
//! session should just wait, and I'll make sure the task is done when I think
//! it's time."*
//!
//! ⚠ **The sleeping is not the interesting part and is not here.** What is here
//! is the decision — whether the wait is over, and what the returning session is
//! told — because those are the two things a hand-rolled shell loop gets wrong.

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
    /// ⚠ **A separate ending because it is a separate answer.** `drop` is
    /// *overtaken, obsolete, or decided against* — the problem the session
    /// stopped for was not fixed. Folding it into [`Done`](Verdict::Done) would
    /// wake the session and tell it the opposite of what happened, which is
    /// worse than never waking it: it resumes on a premise that has been
    /// explicitly refused.
    Dropped(Vec<u64>),
}

/// Read the blockers' statuses into an ending.
///
/// ⚠ **Through [`Status::is_open`], never `== Status::Open`.** A blocker
/// somebody has picked up is `doing`, and is still a blocker; the obvious
/// comparison ends the wait the moment the fix STARTS. That predicate exists
/// because the same mistake was already made once in SQL, and this is the
/// second place that would have made it.
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
/// ⚠ **Two rates rather than one, because the two cases are minutes and days
/// apart.** A blocker closed while somebody is still at the keyboard should wake
/// the session in seconds; one closed tomorrow morning can afford half a minute,
/// and paying five-second polls for a day to shave that would be twelve thousand
/// requests for nothing. It is a ceiling and not a ramp: a wait that has run for
/// a week still answers within [`LAZY`], because a backoff that keeps growing
/// makes the longest waits — the ones most likely to be forgotten — the slowest
/// to come back.
pub fn interval(waited: Duration) -> Duration {
    if waited < KEENLY { EAGER } else { LAZY }
}

/// What the returning session reads.
///
/// ⚠ **The ids, not just the outcome.** The session has been away — possibly for
/// days, across a context it no longer holds — and the task number is the only
/// thing that reconnects the wake to the work it stopped for. `asked` is what
/// the wait was called with, so a partial ending can say which of them.
pub fn said(verdict: &Verdict, asked: &[u64]) -> String {
    match verdict {
        Verdict::Done => format!("{} closed — carry on.", list(asked)),
        // ⚠ **Said once when there is one.** `#1432 closed, but #1432 was
        // dropped` is what naming both ends of the same fact reads like, and the
        // sentence a woken session reads first is not the place to make it work
        // out that those are the same task.
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

/// Task numbers as somebody would say them: `#884`, `#884 and #1350`.
fn list(ids: &[u64]) -> String {
    let said: Vec<String> = ids.iter().map(|id| format!("#{id}")).collect();
    match said.split_last() {
        None => "nothing".to_string(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}
