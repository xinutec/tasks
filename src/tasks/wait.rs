//! When `task wait` ends, and what it tells the session.
//!
//! A blocked session takes no turns, so the digest can't wake it; `task wait`
//! is a background command whose return does. This module decides whether the
//! wait is over and what to say, the two things a hand-rolled loop gets wrong.

use std::time::Duration;

use crate::tasks::types::Status;

/// Whether the wait is over, and how it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Still blocked, on these, in the order named.
    Waiting(Vec<u64>),
    /// Every blocker done: the only ending that means *carry on*.
    Done,
    /// All closed, but these were dropped, not done: the fix did not happen, so
    /// the session must not carry on as if it had.
    Dropped(Vec<u64>),
}

/// ⚠ Uses [`Status::is_open`], not `== Status::Open`: a blocker in `doing` is
/// still a blocker, and the obvious comparison ends the wait when the fix starts.
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

/// Poll fast for the first `KEENLY`, when a fix is likely being made right now,
/// then slow. A ceiling, not a growing backoff, so a week-old wait still
/// answers within `LAZY`.
const EAGER: Duration = Duration::from_secs(5);
const LAZY: Duration = Duration::from_secs(30);
const KEENLY: Duration = Duration::from_secs(120);

pub fn interval(waited: Duration) -> Duration {
    if waited < KEENLY { EAGER } else { LAZY }
}

/// What the returning session reads. It names the ids: after days away, the
/// number is what reconnects the wake to the work.
pub fn said(verdict: &Verdict, asked: &[u64]) -> String {
    match verdict {
        Verdict::Done => format!("{} closed — carry on.", list(asked)),
        // One id is named once: twice reads as two tasks.
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
