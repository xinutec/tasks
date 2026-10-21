//! What `task list` asks the service for. The default is the caller, not the
//! fleet: everything would fill a conversation's context with work it cannot
//! act on. Each question has its flag:
//!
//! | | means | asks for |
//! | --- | --- | --- |
//! | (bare) | what could I pick up | my own, and the pile |
//! | `--mine` | what am I holding | my own, strictly |
//! | `--pile` | what is going spare | the unheld, strictly |
//! | `--all` | what is going on | every holder |
//! | `--handed-out` | what did I hand out | filed by me, held by anyone else |
//!
//! A view with no flag still gets asked for, by a session hand-filtering
//! `--all --json`, which invents fields and miscounts; so every question worth
//! asking gets a flag. `--handed-out` exists because the digest never shows a
//! session the work it handed to others. The bare form includes the pile, as
//! [`Filter::digest_for`](super::repo::Filter) does, so left-over work is seen.
//!
//! Lives in the library, beside `ListQuery` in `routes::api`, so the two
//! cannot drift.

use anyhow::{Context, Result};

/// Whose list, once a name is resolved. Not a string: a person and a session
/// are different columns, and the wrong one silently matches nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder<'a> {
    Person(&'a str),
    Session(&'a str),
    Nobody,
}

/// The query parameters for `GET /api/tasks`. Without a session id there is no
/// "own" to narrow to, so the caller gets everything, as `/api/digest` does.
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
    // Without an id: the pile has no holder, so adding one would match nothing.
    if pile {
        query.push(("unheld".into(), "true".into()));
        return Ok(query);
    }
    // Needs the id, like `--mine`: what this session filed, not what it holds.
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
    // Strictly that holder: the pile is on nobody's plate.
    if let Some(holder) = to {
        match holder {
            Holder::Person(name) => query.push(("person".into(), name.into())),
            Holder::Session(id) => query.push(("session".into(), id.into())),
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
