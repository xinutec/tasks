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
//! the CLI is where you ask: `task list --all`, `task sessions`.
//!
//! Bare `task list` answers this same question — own plus the pile — as of
//! 2026-08-09. It used to answer with every open task there is, which put the
//! cost this module refuses behind the one command a session runs to decide
//! what to do next: 12,804 bytes, against one line for the session that ran it.
//!
//! **A session can narrow this further, for a few hours, and only itself.**
//! `task focus 849 850 --for 4h` — see [`crate::tasks::focus`], which is the
//! only thing in the service that hides an *open* task and carries the three
//! rules that make that safe. The two trims here compose in one direction:
//! focus runs first and the pile cap runs on what survives, or the five pile
//! lines would be spent on tasks the focus then hid.
//!
//! The selection lives in [`Filter::digest_for`](crate::tasks::repo::Filter),
//! not here — this module is handed a list and renders it. Which is why the
//! render tests can stay about cost.

use crate::tasks::focus::{self, Focus};
use crate::tasks::types::{AssigneeKind, Status, Task};

/// A guard, not a policy. An index is meant to be an index; past this somebody
/// has started writing content into the subjects, and the whole point is lost.
/// The same figure the file-scheme hook refused at — ten times memview's
/// measured 2.5 kB index, and still 6% of one built-in reminder.
pub const MAX_BYTES: usize = 25_000;

/// How many unheld tasks a digest recites before it starts counting them.
///
/// ⚠ **The pile has a different denominator from everything else here, and
/// that is the whole reason for this constant.** A task a session holds is in
/// one conversation's prompt; an unheld one is in *every* conversation's, on
/// every turn, because the pile is global and is shown to all of them. So one
/// line left for whoever picks it up costs as many prompts as there are live
/// sessions — fourteen, when this was written — and nothing in the system was
/// pushing back on that. [`MAX_BYTES`] is not the guard: it is a runaway stop
/// at some two hundred lines, per session, which the pile would reach only
/// after having been ruinous for weeks.
///
/// `README.md` argues the pile is affordable, and the measurement it argues
/// from is *3 unheld of 134 open*. That is a **condition**, not a property —
/// two days after the cutover the recall session's digest carried 5 pile lines
/// against its own 3, with nothing anywhere keeping the number down. This is
/// what keeps it true.
///
/// Five, because the pile is a handover channel rather than a backlog: enough
/// to notice that something is waiting and take it, and past that it is a list
/// to browse when you have asked for it. Growing this is spending every
/// conversation's context to save one `task list`.
pub const PILE_LINES: usize = 5;

/// Render the index for a set of tasks, already filtered to the open ones.
///
/// One flat list, in id order. This grouped by repository until `0004`, and the
/// grouping is not worth reinstating under another key: the header cost a line
/// per group on every turn to tell a reader something the subject already says.
/// Neither trim reshuffles it either — the tasks that survive the pile cap and
/// the focus stay where their ids put them, interleaved with the session's own.
///
/// ⚠ **A focus is applied as given and its expiry is not checked here.** This
/// renders; [`focus::current`] decides whether a period still holds, and it is
/// the only place that reads the clock. The digest is cached for sixty seconds
/// and read minutes later, so a renderer comparing against its own `now` would
/// answer a question about a moment that had passed — the same reason the due
/// date is printed as a date and never as a countdown.
pub fn render(tasks: &[Task], focus: Option<&Focus>) -> String {
    if tasks.is_empty() {
        return String::new();
    }

    let doing = tasks.iter().filter(|t| t.status == Status::Doing).count();

    // Focus first, then the pile cap on what survives it: capping first would
    // spend the five pile lines on tasks the focus then hides, and report a
    // pile shorter than it is.
    let mut focus_hidden_own = 0usize;
    let mut focus_hidden_pile = 0usize;
    // The pile, trimmed. Counted in id order, so what survives is the oldest
    // of it: a task nobody has taken in a fortnight is the one at risk of being
    // forgotten, and the newest is the one whoever filed it still remembers.
    let mut piled = 0usize;
    let mut pile_hidden = 0usize;
    let mut selected: Vec<&Task> = Vec::with_capacity(tasks.len());
    for task in tasks {
        let unheld = task.assignee.kind == AssigneeKind::Nobody;
        if let Some(focus) = focus
            && !focus.tasks.contains(&task.id)
            && !focus::breaks_through(task)
        {
            if unheld {
                focus_hidden_pile += 1;
            } else {
                focus_hidden_own += 1;
            }
            continue;
        }
        if unheld {
            piled += 1;
            if piled > PILE_LINES {
                pile_hidden += 1;
                continue;
            }
        }
        selected.push(task);
    }

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

    for task in selected {
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

    // ⚠ **A focus that hid something must say so, and say how to stop.** The
    // whole risk of this feature is a session reading a short list as an empty
    // plate; the count is what makes the short list legible, and naming the
    // breakthrough rule is what makes it trustworthy — a session that does not
    // know P0 still arrives will run `task list` every turn to check, which
    // costs far more than the sentence does. Emitted whenever a focus holds,
    // including when it happens to be hiding nothing: it is the explanation for
    // the shape of everything above it.
    if let Some(focus) = focus {
        let mut says = format!("⚠ focused until {} UTC", focus.until.format("%H:%M"));
        match (focus_hidden_own, focus_hidden_pile) {
            (0, 0) => says.push_str(": nothing else of yours is open."),
            (own, 0) => says.push_str(&format!(": {own} more of yours not shown.")),
            (0, pile) => says.push_str(&format!(": {pile} in the pile not shown.")),
            (own, pile) => says.push_str(&format!(
                ": {own} more of yours and {pile} in the pile not shown."
            )),
        }
        if focus_hidden_own + focus_hidden_pile > 0 {
            says.push_str(" P0 and overdue still break through.");
        }
        says.push_str(" `task focus --clear` ends it, `task list` shows everything.");
        out.push(says);
    }

    // Two notices, never merged: they are different failures with different
    // remedies. The budget means a session's own plate has grown into content;
    // the pile means work is piling up that nobody has taken.
    if pile_hidden > 0 {
        out.push(format!("⚠ {pile_hidden} more in the pile — `task list`."));
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
    // ⚠ **Costs nothing until somebody ranks something**, which is why it is
    // allowed in the one place every session pays for on every turn: an
    // unranked task prints exactly what it printed before, and almost every
    // task is unranked. A ranked one spends five bytes to say the thing the
    // reader opened this list to find out.
    //
    // The ORDER BY has already put these at the top — `repo::list` sorts,
    // `render` never does — so this marks lines that are already where the
    // reader is looking rather than asking anybody to scan for them.
    // `escalated_to` first: a near deadline raises the rank, and the line has to
    // show what the list actually sorted by or the order reads as random. The
    // `!` says the level was not the one anybody chose — `task show` gives both.
    if let Some(raised) = task.escalated_to {
        line.push_str(&format!(" [{raised}!]"));
    } else if let Some(priority) = task.priority {
        line.push_str(&format!(" [{priority}]"));
    }
    // ⚠ **Only while a blocker is still open**, which is what `blocked` means —
    // a finished dependency is a fact about the past and belongs in the task,
    // not in every prompt. Costs nothing on the tasks that are not waiting,
    // which is nearly all of them, and on the ones that are it answers the
    // question a reader would otherwise open the task to ask.
    // ⚠ **The date itself, not a countdown.** A countdown would be recomputed
    // against whatever "now" the renderer had, and this line is cached for
    // sixty seconds and read minutes later. A date is the same fact whenever it
    // is read. OVERDUE is shouted because it is a fact that has changed state
    // and nothing else on the line would say so.
    if let Some(due) = task.due {
        if task.overdue {
            line.push_str(&format!(" OVERDUE {due}"));
        } else {
            line.push_str(&format!(" due {due}"));
        }
    }
    if task.blocked {
        let on: Vec<String> = task.blocked_on.iter().map(|id| format!("#{id}")).collect();
        line.push_str(&format!(" ⛔{}", on.join(",")));
    }
    if task.assignee.kind != AssigneeKind::Nobody {
        line.push_str(&format!(" ({})", task.assignee.label()));
    }
    line
}
