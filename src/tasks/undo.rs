//! Whether `task undo` may restore without `--anyway`.
//!
//! ⚠ **It reverts the task's last edit, not yours.** One version is kept per
//! task, so with several conversations at work it may be somebody else's edit
//! that comes back undone. That case is refused, not warned about: it is rare,
//! so the flag still means something.

use crate::tasks::types::Revision;

/// Only someone else's edit needs `--anyway`; your own is yours to put back.
pub fn needs_saying(was: &Revision) -> bool {
    !was.mine
}

/// The refusal. It carries the date: a collision seconds old and an edit from
/// last week call for different decisions.
pub fn refusal(was: &Revision, id: u64) -> String {
    format!(
        "the last edit to #{id} was {}'s, not yours, made {}. Restoring would revert \
         THEIR edit — undo puts back the one version this task keeps, whoever displaced it, \
         and only one is kept. `task show {id} --previous` to read what would come back. \
         If you do mean to revert their edit, re-run with --anyway.",
        was.actor,
        was.at.format("%Y-%m-%d %H:%M UTC")
    )
}
