//! Naming a task: a bare number, or `#`-prefixed as the digest prints it, so a
//! session copying from its own prompt is never corrected. Beside the router,
//! since [`TaskRef::path`] is its URL.

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
