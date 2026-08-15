//! Whether putting a version back is safe to do without saying so.
//!
//! ⚠ **`task undo` reverts THE last edit, not YOUR last edit.** One version is
//! kept per task, not per actor, so with conversations working at once the
//! version waiting to come back can be one somebody else displaced seconds ago.
//! Restoring it then reverts *their* edit while reading, to whoever typed it, as
//! undoing their own mistake.
//!
//! That happened on 2026-08-15, to the session that had shipped the revision
//! store the day before:
//!
//! ```text
//! 13:54:34  tasks     edited  body 1087 → 366 chars   an over-deletion
//! 13:54:45  dev-lint  edited  body 366 → 752 chars    their append, 11s later
//! 13:55:03  tasks     edited  body 752 → 366 chars    `task undo`
//! ```
//!
//! The intent was to revert 13:54:34. What went was 13:54:45. It came back only
//! because a second undo still had it — one more write from anyone and the
//! store's single version would have been the wrong one.
//!
//! **Why this gates where editing does not.** [`crate::tasks::duplicates`]
//! argues that a gate on a frequent correct operation teaches everyone to pass
//! it, which is why overwriting another session's text is not gated: it is
//! ordinary, permitted, and done often. Reverting another session's edit is
//! none of those. The flag should stay rare enough to mean something.
//!
//! The warning this replaces was already printed, and printed accurately —
//! `replaced text last written … by dev-lint`. It was piped to `/dev/null`. A
//! second line saying the same thing louder would have gone the same way, which
//! is the whole case for refusing instead.

use crate::tasks::types::Revision;

/// Whether restoring this version needs saying out loud first.
///
/// Only the authorship question. A revision that is the caller's own is theirs
/// to put back, however old.
pub fn needs_saying(was: &Revision) -> bool {
    !was.mine
}

/// What to say when it does, naming whose edit is at stake and when.
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
