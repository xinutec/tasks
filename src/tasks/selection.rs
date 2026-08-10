//! What `task list` asks the service for.
//!
//! ⚠ **The default is the caller, not the fleet.** A bare `task list` used to
//! mean *every open task there is*: 135 lines and 12,804 bytes when that was
//! measured, against one line for this session's own plate. Every one of those
//! lines lands in a conversation's context, and almost none of it is work that
//! conversation can act on — the same cost `digest.rs` exists to refuse, reached
//! through the one command a session runs when it wants to know what to do next.
//!
//! So the three questions are named:
//!
//! | | means | asks for |
//! | --- | --- | --- |
//! | (bare) | what could I pick up | my own, and the pile |
//! | `--mine` | what am I holding | my own, strictly |
//! | `--pile` | what is going spare | the unheld, strictly |
//! | `--all` | what is going on | every holder |
//!
//! ⚠ **`--pile` is here because its absence was answered by guesswork.** It was
//! the one question with no name, so a session hand-filtered `--all --json`,
//! invented a `session` field that does not exist, and reported **137** tasks in
//! the pile when there were **5** — a number that reached Pippijn before anybody
//! caught it. A missing view is not neutral: it is answered anyway, by whoever
//! needs it, without the tests this file has.
//!
//! The bare form deliberately matches [`Filter::digest_for`](super::repo::Filter)
//! rather than `--mine`: the pile is the handover channel, and a session that
//! cannot see it cannot take work left for whichever conversation is around.
//! `--mine` is the narrower question and stays available for asking it.
//!
//! This lives in the library rather than in the CLI for the reason
//! [`reference`](super::reference) does: the parameters it emits are defined by
//! `ListQuery` in `routes::api`, and having the two beside each other is what
//! stops them drifting. It is also what lets `tests/selection.rs` exercise it as
//! public API rather than through an inline test module.

use anyhow::{Context, Result};

/// The query parameters for `GET /api/tasks`.
///
/// **Without a session id there is no "own" to narrow to**, so the caller gets
/// everything — the same answer `/api/digest` gives a person who names no
/// session. In practice this is not reachable through the token path, which
/// refuses to run at all without an id; it is the honest answer rather than a
/// live case.
pub fn list_query(
    all: bool,
    mine: bool,
    pile: bool,
    done: bool,
    session: Option<&str>,
) -> Result<Vec<(String, String)>> {
    let mut query: Vec<(String, String)> = Vec::new();
    if done {
        query.push(("done".into(), "true".into()));
    }
    if all {
        return Ok(query);
    }
    // Before the session clauses and without one: the pile has no holder, so
    // sending an id alongside would ask for the intersection of two disjoint
    // sets. Needs no session id at all, which is also what makes it the one
    // list a person can ask for the same way a session does.
    if pile {
        query.push(("unheld".into(), "true".into()));
        return Ok(query);
    }
    if mine {
        let session =
            session.context("--mine needs a session id (--session, or $TASKS_SESSION)")?;
        query.push(("session".into(), session.into()));
        return Ok(query);
    }
    if let Some(session) = session {
        query.push(("session".into(), session.into()));
        query.push(("pile".into(), "true".into()));
    }
    Ok(query)
}
