//! What `task list` asks the service for.
//!
//! ⚠ **The default is the caller, not the fleet.** Every open task there is
//! would land in a conversation's context, almost none of it work that
//! conversation can act on — the cost `digest.rs` exists to refuse. So each
//! question is named:
//!
//! | | means | asks for |
//! | --- | --- | --- |
//! | (bare) | what could I pick up | my own, and the pile |
//! | `--mine` | what am I holding | my own, strictly |
//! | `--pile` | what is going spare | the unheld, strictly |
//! | `--all` | what is going on | every holder |
//! | `--handed-out` | what did I hand out | filed by me, held by anyone else |
//!
//! ⚠ **A missing view is not neutral — it gets answered anyway, by guesswork**:
//! a session hand-filtering `--all --json` for a view with no flag can invent
//! a field and report a wrong count. Every question worth asking gets a flag
//! and its tests.
//!
//! ⚠ **`--handed-out` is the only one not about the holder**, and a routing
//! session needs it most: the digest shows a session its own work and the pile,
//! never another conversation's, so work it handed out is by construction what
//! it never sees again.
//!
//! The bare form matches [`Filter::digest_for`](super::repo::Filter) rather than
//! `--mine`: the pile is the handover channel, and a session that cannot see it
//! cannot take work left for whichever conversation is around.
//!
//! In the library rather than the CLI because the parameters it emits are
//! defined by `ListQuery` in `routes::api`, and keeping the two beside each
//! other stops them drifting.

use anyhow::{Context, Result};

/// Whose list is being asked for, once a name has been resolved.
///
/// ⚠ **A person and a session are different columns**, which is why this is not
/// a string: naming a session in the person column silently matches nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder<'a> {
    Person(&'a str),
    Session(&'a str),
    Nobody,
}

/// The query parameters for `GET /api/tasks`.
///
/// **Without a session id there is no "own" to narrow to**, so the caller gets
/// everything — as `/api/digest` does for a person who names no session. The
/// token path refuses to run without an id, so this is not a live case.
pub fn list_query(
    all: bool,
    mine: bool,
    pile: bool,
    handed_out: bool,
    done: bool,
    session: Option<&str>,
    to: Option<Holder<'_>>,
) -> Result<Vec<(String, String)>> {
    let mut query: Vec<(String, String)> = Vec::new();
    if done {
        query.push(("done".into(), "true".into()));
    }
    if all {
        return Ok(query);
    }
    // Before the session clauses and without an id: the pile has no holder, so
    // an id alongside would intersect two disjoint sets.
    if pile {
        query.push(("unheld".into(), "true".into()));
        return Ok(query);
    }
    // Before `--mine` and needing the same id, because it asks about the same
    // session from the other side: what it wrote rather than what it holds.
    if handed_out {
        let session =
            session.context("--handed-out needs a session id (--session, or $TASKS_SESSION)")?;
        query.push(("handed_out".into(), session.into()));
        return Ok(query);
    }
    if mine {
        let session =
            session.context("--mine needs a session id (--session, or $TASKS_SESSION)")?;
        query.push(("session".into(), session.into()));
        return Ok(query);
    }
    // ⚠ **Strictly that holder, with no pile**: `--to` answers "what is X
    // carrying", and the pile is on no holder's plate.
    if let Some(holder) = to {
        match holder {
            Holder::Person(name) => query.push(("person".into(), name.into())),
            Holder::Session(id) => query.push(("session".into(), id.into())),
            // The pile IS a holder in the vocabulary.
            Holder::Nobody => query.push(("unheld".into(), "true".into())),
        }
        return Ok(query);
    }
    if let Some(session) = session {
        query.push(("session".into(), session.into()));
        query.push(("pile".into(), "true".into()));
    }
    Ok(query)
}
