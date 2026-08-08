//! Naming a task: `79`, or what it was called before it lived here.
//!
//! ⚠ **Two spellings, because the id is not stable and the other name is.** A
//! built-in task number was unique only inside one session — four sessions each
//! had a `#79` — so when the 598 of them moved into one global id space, 46%
//! could not keep the number they had. `recall#79` is what old prose, old
//! memories and the sessions' own notes still contain, and `origin_session` +
//! `origin_number` keep it resolvable for ever.
//!
//! This lives in the library rather than in the CLI because the route table does
//! too: [`TaskRef::path`] is the URL for a reference, and having it beside the
//! router is what stops the two drifting.

use std::fmt;
use std::str::FromStr;

/// How a task was named by whoever is asking for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskRef {
    /// This service's id.
    Id(u64),
    /// `recall#79` — a session's name, and its number there.
    Origin(String, u64),
}

impl TaskRef {
    /// Where to GET it.
    ///
    /// ⚠ **`by/…` is two path segments, never an escaped `#`.** A `#` in a URL
    /// is the fragment delimiter: unescaped it truncates the request to
    /// `/api/tasks/recall`, and escaped it works only for callers that remember.
    pub fn path(&self) -> String {
        match self {
            TaskRef::Id(id) => format!("/api/tasks/{id}"),
            TaskRef::Origin(session, number) => format!("/api/tasks/by/{session}/{number}"),
        }
    }
}

impl fmt::Display for TaskRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskRef::Id(id) => write!(f, "#{id}"),
            TaskRef::Origin(session, number) => write!(f, "{session}#{number}"),
        }
    }
}

impl FromStr for TaskRef {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        // `#79` and `79` are the same thing: the digest prints `#79` on every
        // line of every prompt, so a session copying one out of its own context
        // must not be corrected for it.
        let (session, number) = match s.split_once('#') {
            Some((session, number)) => (session.trim(), number),
            None => ("", s),
        };
        let number: u64 = number
            .trim()
            .parse()
            .map_err(|_| format!("{s:?} is not a task: expected 79, or recall#79"))?;
        Ok(if session.is_empty() {
            TaskRef::Id(number)
        } else {
            TaskRef::Origin(session.to_string(), number)
        })
    }
}
