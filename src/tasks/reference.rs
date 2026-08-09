//! Naming a task: `79`, or `#79` as the digest prints it.
//!
//! ⚠ **`#79` and `79` are the same thing, and that is the whole job.** The
//! digest puts `#79` on every line of every prompt, so a session copying one out
//! of its own context must not be corrected for the hash it was shown. Parsing
//! it in one place is what stops each subcommand growing its own opinion.
//!
//! **There used to be a second spelling.** `recall#79` named a task by what it
//! was called in the CLI's per-session store, resolved through
//! `origin_session` / `origin_number`, because 178 of the 620 imported tasks
//! could not keep their number. Those columns were retired in
//! `migrations/0003_drop_origin.sql` once the mapping had been spent — the
//! references that depended on it were rewritten to live ids first — so there is
//! one id space again and nothing to disambiguate.
//!
//! This lives in the library rather than in the CLI because [`TaskRef::path`] is
//! the URL for a reference, and having it beside the router is what stops the
//! two drifting.

use std::fmt;
use std::str::FromStr;

/// A task, by this service's id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskRef(pub u64);

impl TaskRef {
    /// Where to GET it.
    pub fn path(&self) -> String {
        format!("/api/tasks/{}", self.0)
    }

    /// The id itself, for the paths that build their own URL.
    pub fn id(&self) -> u64 {
        self.0
    }
}

impl fmt::Display for TaskRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

impl FromStr for TaskRef {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let number = s.strip_prefix('#').unwrap_or(s).trim();
        number
            .parse()
            .map(TaskRef)
            .map_err(|_| format!("{s:?} is not a task: expected 79, or #79"))
    }
}
