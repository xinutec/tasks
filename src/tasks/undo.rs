//! Whether putting a version back is safe to do without saying so.
//!
//! ⚠ **`task undo` reverts THE last edit, not YOUR last edit.** One version is
//! kept per task, not per actor, so with conversations working at once the
//! version waiting to come back can be one somebody else displaced seconds ago.
//! Restoring it reverts *their* edit while reading, to whoever typed it, as
//! undoing their own mistake — and one more write from anyone loses it.
//!
//! **Why this gates where editing does not.** A gate on a frequent correct
//! operation teaches everyone to pass it, which is why overwriting another
//! session's text is only warned about: it is ordinary and done often.
//! Reverting another session's edit is neither, so the flag stays rare enough to
//! mean something. The warning this replaces was accurate and got piped to
//! `/dev/null`; a louder one would have gone the same way.

use crate::tasks::types::Revision;

///
/// Only the authorship question. A revision that is the caller's own is theirs
/// to put back, however old.
pub fn needs_saying(was: &Revision) -> bool {
    !was.mine
}

///
/// The date is included because "by dev-lint" alone does not tell you whether
/// you are looking at a collision seconds old or an edit from last week, and the
/// two want different decisions.
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
