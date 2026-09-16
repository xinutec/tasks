//! Naming a task: a bare number, or `#`-prefixed as the digest prints it.
//!
//! ⚠ **Both spellings are the same thing, and that is the whole job.** The
//! digest puts the hash on every line of every prompt, so a session copying one
//! out of its own context must not be corrected for what it was shown. Parsing
//! in one place stops each subcommand growing its own opinion.
//!
//! In the library rather than the CLI because [`TaskRef::path`] is the URL for a
//! reference, and keeping it beside the router stops the two drifting.

use std::fmt;
use std::str::FromStr;

/// A task, by this service's id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskRef(pub u64);

impl TaskRef {
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
