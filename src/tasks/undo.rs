//! Whether putting a version back is safe to do without saying so.
//!
//! ⚠ **`task undo` reverts THE last edit, not YOUR last edit.** One version is
//! kept per task, not per actor, so with conversations working at once the
//! version waiting to come back can be one somebody else displaced seconds ago.
//! Restoring it reverts *their* edit while reading, to whoever typed it, as
//! undoing their own mistake — and one more write from anyone loses it.
//!
//! **Why this gates where editing does not.** [`crate::tasks::duplicates`]
//! argues that gating a frequent correct operation only teaches everyone to pass
//! the gate — which is why overwriting another session's text is merely warned
//! about. Reverting another session's edit is neither frequent nor ordinary, so
//! the flag stays rare enough to mean something; a warning here gets piped to
//! `/dev/null`.

use crate::tasks::types::Revision;

/// Whether restoring `was` needs `--anyway`: only when someone else made it. A
/// revision that is the caller's own is theirs to put back, however old.
pub fn needs_saying(was: &Revision) -> bool {
    !was.mine
}

/// What an undo of someone else's edit is told. The date is included because
/// a collision seconds old and an edit from last week want different decisions.
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
