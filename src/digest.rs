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
//!    [`Filter::include_done`](crate::tasks::repo::Filter) is never set on this
//!    path.
//! 3. **A budget, enforced rather than hoped for.** Past [`MAX_BYTES`] the
//!    digest stops and says how many it left out. An index that quietly grows
//!    into content is the exact failure this replaced, and the old hook refused
//!    one over the same limit.
//!
//! **What is deliberately NOT here yet: filtering by who holds a task.** A
//! session is shown every open task in the repos it claimed, including the ones
//! Pippijn is holding — which is what the file scheme did, since both lived in
//! one file. Showing only its own would be smaller still and is the obvious next
//! step; it is left until the shape has been used, because a session that cannot
//! see what the other party is holding will re-file work already in hand.

use crate::tasks::types::{AssigneeKind, Status, Task};

/// A guard, not a policy. An index is meant to be an index; past this somebody
/// has started writing content into the subjects, and the whole point is lost.
/// The same figure the file-scheme hook refused at — ten times memview's
/// measured 2.5 kB index, and still 6% of one built-in reminder.
pub const MAX_BYTES: usize = 25_000;

/// Render the index for a set of tasks, already filtered to the open ones.
///
/// `groups_named` asks for the repository to be named above each group, which
/// the old hook did only when a session had claimed more than one — a bare
/// `#4` means nothing when two repos both have one. That argument no longer
/// applies to the id, which is global now, but the grouping still tells a
/// reader which checkout the work is in.
pub fn render(tasks: &[Task]) -> String {
    if tasks.is_empty() {
        return String::new();
    }

    let doing = tasks.iter().filter(|t| t.status == Status::Doing).count();
    let mut repos: Vec<Option<&str>> = Vec::new();
    for task in tasks {
        let repo = task.repo.as_deref();
        if !repos.contains(&repo) {
            repos.push(repo);
        }
    }

    let mut head = format!("{} open task(s)", tasks.len());
    if repos.len() > 1 {
        head.push_str(&format!(" across {} repos", repos.len()));
    }
    if doing > 0 {
        head.push_str(&format!(", {doing} in progress"));
    }
    head.push_str(". Open one with `task show <id>` before acting on it.");

    let mut out = vec![head];
    let mut bytes = out[0].len();
    let mut omitted = 0usize;

    for repo in repos.iter() {
        let group: Vec<&Task> = tasks
            .iter()
            .filter(|t| t.repo.as_deref() == *repo)
            .collect();
        if repos.len() > 1 {
            // Named only when there is more than one, so the single-repo case —
            // which is most of them — reads exactly as it always has.
            let label = repo.unwrap_or("no repo");
            out.push(format!("{label} ({} open)", group.len()));
        }
        for task in group {
            if omitted > 0 {
                omitted += 1;
                continue;
            }
            let line = line(task);
            // Counted before pushing: a digest that goes over the budget and
            // then reports it has already cost what the budget exists to
            // prevent.
            if bytes + line.len() + 1 > MAX_BYTES {
                omitted = 1;
                continue;
            }
            bytes += line.len() + 1;
            out.push(line);
        }
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
