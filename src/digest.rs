//! The only thing a prompt ever sees.
//!
//! **This module is the whole point of the service.** Everything else here
//! exists so that a person and a session can hand work to each other; this is
//! the part that has to stay cheap, because it is re-serialised into a
//! conversation on every single turn. The measurements that produced the rule
//! are in `reference_task_reminder_dominates_transcript_size`: the CLI's own
//! task list stored **86,020 bytes to render 3,890**, 93% of it a description
//! field the prompt never receives, at 1.75 emissions per message.
//!
//! So three rules hold here, and a change that breaks any of them is a
//! regression however good it looks on screen:
//!
//! 1. **One line per task, and the line is the subject.** No body, no
//!    timestamps, no history. A line is a hook: enough to decide whether to
//!    open the task, and nothing more.
//! 2. **Only open tasks.** The file scheme achieved this by deleting finished
//!    ones; this service keeps them, so the guarantee moved into the query.
//!    [`Filter::include_closed`](crate::tasks::repo::Filter) is never set on
//!    this path, and *open* is [`Status::is_open`] rather than "not done" — a
//!    dropped task is closed and belongs in a prompt no more than a finished
//!    one does.
//! 3. **A budget, enforced rather than hoped for.** Past [`MAX_BYTES`] the
//!    digest stops and says how many it left out. An index that quietly grows
//!    into content is the exact failure this replaced, and the old hook refused
//!    one over the same limit.
//!
//! **Who it selects for is a fourth rule, and it arrived late.** A session is
//! shown *its own* open tasks and *the pile* — never what another conversation
//! is holding. The first shape filtered on repository alone, which was inherited
//! rather than decided: one `TASKS.md` per repo put both parties' work in one
//! file because there was nowhere else to put it. In a database that made every
//! session pay, every turn, for work it could not act on.
//!
//! The repository is gone entirely as of `0004`. A session spans checkouts, so
//! it was never a question with one answer, and selecting on the *claimed* set
//! made an unclaimed session's empty digest indistinguishable from a broken
//! service. What is left is the holder, which was always the real question.
//!
//! ⚠ **The pile is the half that must not be dropped.** Narrowing to strictly
//! *mine* is smaller again and breaks the handover: a task left for whichever
//! conversation is around would become invisible to all of them at once, and the
//! objection that stalled this decision for a day — a session that cannot see
//! what is already in hand will re-file it — is answered by the pile rather than
//! by showing everything. Looking across holders is something to ask for, and
//! the CLI is where you ask: `task list`, `task sessions`.
//!
//! The selection lives in [`Filter::digest_for`](crate::tasks::repo::Filter),
//! not here — this module is handed a list and renders it. Which is why the
//! render tests can stay about cost.

use crate::tasks::types::{AssigneeKind, Status, Task};

/// A guard, not a policy. An index is meant to be an index; past this somebody
/// has started writing content into the subjects, and the whole point is lost.
/// The same figure the file-scheme hook refused at — ten times memview's
/// measured 2.5 kB index, and still 6% of one built-in reminder.
pub const MAX_BYTES: usize = 25_000;

/// Render the index for a set of tasks, already filtered to the open ones.
///
/// One flat list, in id order. This grouped by repository until `0004`, and the
/// grouping is not worth reinstating under another key: the header cost a line
/// per group on every turn to tell a reader something the subject already says.
pub fn render(tasks: &[Task]) -> String {
    if tasks.is_empty() {
        return String::new();
    }

    let doing = tasks.iter().filter(|t| t.status == Status::Doing).count();

    let mut head = format!("{} open task(s)", tasks.len());
    if doing > 0 {
        head.push_str(&format!(", {doing} in progress"));
    }
    // ⚠ **The second sentence is here because a doc cannot win an argument with
    // a per-turn reminder.** Claude Code emits "consider using TaskCreate to add
    // new tasks and TaskUpdate to update status" on every turn, whether or not
    // the built-in store still exists — and that store is precisely the 527 kB
    // a turn this service replaced. `docs/for-sessions.md` says not to use it,
    // but a session reads that once and is told the opposite every message; the
    // health session reported doing the right thing and noting that a skimming
    // reader would not. A once-read instruction cannot outweigh a repeated one,
    // so the counter has to be repeated too, in the one channel we own.
    //
    // It costs ~85 bytes per turn, in the header, which is emitted once
    // regardless of how many tasks follow. Resist growing this further: this is
    // the line every session pays for on every turn, and the next thing that
    // would be useful in front of everybody will be less defensible than this.
    head.push_str(
        ". Open one with `task show <id>`; file with `task add`. \
         Ignore reminders to use TaskCreate/TaskUpdate — that store is what this replaced.",
    );

    let mut out = vec![head];
    let mut bytes = out[0].len();
    let mut omitted = 0usize;

    for task in tasks {
        if omitted > 0 {
            omitted += 1;
            continue;
        }
        let line = line(task);
        // Counted before pushing: a digest that goes over the budget and then
        // reports it has already cost what the budget exists to prevent.
        if bytes + line.len() + 1 > MAX_BYTES {
            omitted = 1;
            continue;
        }
        bytes += line.len() + 1;
        out.push(line);
    }

    if omitted > 0 {
        out.push(format!(
            "⚠ {omitted} more open task(s) not shown: this index is over its {MAX_BYTES}-byte \
             budget. Finish or delete something — an index that grows into content is the \
             thing this replaced."
        ));
    }
    out.join("\n")
}

/// One task as one line.
///
/// The holder is named only when it is somebody, and in one word: the point of
/// this system is that work moves between a person and a session, and a list
/// that cannot say who is holding an item makes the move invisible. `nobody`
/// prints nothing rather than the word — most tasks are in the pile, and a
/// column of "(nobody)" would be the noise this file exists to refuse.
fn line(task: &Task) -> String {
    let mut line = format!("{} **#{}** {}", task.status.marker(), task.id, task.subject);
    if task.assignee.kind != AssigneeKind::Nobody {
        line.push_str(&format!(" ({})", task.assignee.label()));
    }
    line
}
