//! The only thing a prompt ever sees, re-sent every turn, so it stays cheap:
//!
//! 1. One line per task, the subject: no body, timestamps or history.
//! 2. Open tasks only ([`Status::is_open`]; a dropped task is closed too).
//! 3. A budget: past [`MAX_BYTES`] it stops and says how many it left out.
//! 4. A session sees its own tasks and the pile, never another conversation's
//!    ([`Filter::digest_for`](crate::tasks::repo::Filter::digest_for)).
//! 5. `P4` is counted, never recited.
//!
//! `task focus` narrows it for a while ([`crate::tasks::focus`]). Every trim is
//! counted, never silent.

use crate::tasks::focus::{self, Focus};
use crate::tasks::types::{AssigneeKind, Priority, Status, Task};

/// A runaway stop: an index this large has content in its subjects.
pub const MAX_BYTES: usize = 25_000;

/// Unheld tasks recited before the rest are counted. Small, because a pile
/// line is in every conversation's prompt every turn, not one's; the pile is a
/// handover channel, not a backlog.
pub const PILE_LINES: usize = 5;

/// Recited lines a session holds before the digest mentions `focus`, set at
/// the break in the distribution across sessions. Held and recited only: trimmed
/// tasks cost nothing, and the pile has [`PILE_LINES`]. The hint states that
/// focus exists; the digest can't know what to hide, so it never advises.
pub const FOCUS_HINT_LINES: usize = 12;

/// Whether a task is counted rather than recited: `P4`, where nothing is being
/// paid today. Counted is not closed, and the notice must not imply it (wording
/// that did led a session to leave decided questions open). By effective rank,
/// overdue apart, as in [`focus::breaks_through`]. Not `P3`: a workaround in use
/// is still a plan, and hiding it would push everything to P2.
fn parked(task: &Task) -> bool {
    !task.overdue && task.urgency() == Some(Priority::P4)
}

/// Render the index for tasks already filtered to the open ones: one flat list
/// in id order, no group headers.
///
/// ⚠ A focus is applied as given; [`focus::current`] owns the clock. The digest
/// is cached and read later, so it never compares against its own `now`.
pub fn render(tasks: &[Task], focus: Option<&Focus>) -> String {
    if tasks.is_empty() {
        return String::new();
    }

    let doing = tasks.iter().filter(|t| t.status == Status::Doing).count();

    // Focus first, then the pile cap, or the cap spends lines on hidden tasks.
    let mut focus_hidden_own = 0usize;
    let mut focus_hidden_pile = 0usize;
    // Oldest pile tasks survive: the newest is still remembered by its filer.
    let mut piled = 0usize;
    let mut pile_hidden = 0usize;
    let mut parked_hidden = 0usize;
    // Counted from what is recited: a trimmed task costs nothing.
    let mut own = 0usize;
    let mut selected: Vec<&Task> = Vec::with_capacity(tasks.len());
    for task in tasks {
        let unheld = task.assignee.kind == AssigneeKind::Nobody;
        // A task the focus names survives both trims.
        let named = focus.is_some_and(|focus| focus.tasks.contains(&task.id));
        if focus.is_some() && !named && !focus::breaks_through(task) {
            if unheld {
                focus_hidden_pile += 1;
            } else {
                focus_hidden_own += 1;
            }
            continue;
        }
        if !named && parked(task) {
            parked_hidden += 1;
            continue;
        }
        if unheld {
            piled += 1;
            if piled > PILE_LINES {
                pile_hidden += 1;
                continue;
            }
        } else {
            own += 1;
        }
        selected.push(task);
    }

    let mut head = format!("{} open task(s)", tasks.len());
    if doing > 0 {
        head.push_str(&format!(", {doing} in progress"));
    }
    // The second sentence answers Claude Code's per-turn reminder to use
    // TaskCreate, the store this replaced; only a line repeated every turn
    // outweighs it. Every session pays for this line every turn: keep it short.
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
        // Checked before pushing, so going over never happens.
        if bytes + line.len() + 1 > MAX_BYTES {
            omitted = 1;
            continue;
        }
        bytes += line.len() + 1;
        out.push(line);
    }

    // A focus says what it hid and how to end it, even if nothing: otherwise a
    // short list reads as an empty plate, and a session unsure P0 still shows runs
    // `task list` every turn.
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

    // Separate notices, each with its own remedy; empty when there is nothing.
    if parked_hidden > 0 {
        out.push(format!(
            "⚠ {parked_hidden} at P4 not shown — still open, just not recited; `task list`."
        ));
    }
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
    // Last, and never with a focus (whose notice already says how to end it).
    // Only `--help` mentioned `focus`, and it went unused.
    if focus.is_none() && own > FOCUS_HINT_LINES {
        out.push(format!(
            "⚠ {own} of these are yours — `task focus <id>… --for 4h` recites only what you name."
        ));
    }
    out.join("\n")
}

/// One task as one line. The holder is named in one word; the pile prints
/// nothing rather than a column of "(nobody)".
fn line(task: &Task) -> String {
    let mut line = format!("{} **#{}** {}", task.status.marker(), task.id, task.subject);
    // Shows why the line sits where it does: the rank it sorted by, with `!` when
    // a deadline raised it.
    if let Some(raised) = task.escalated_to {
        line.push_str(&format!(" [{raised}!]"));
    } else if let Some(priority) = task.priority {
        line.push_str(&format!(" [{priority}]"));
    }
    // The date, not a countdown, because the line is cached. OVERDUE is loud:
    // nothing else on the line shows the change.
    if let Some(due) = task.due {
        if task.overdue {
            line.push_str(&format!(" OVERDUE {due}"));
        } else {
            line.push_str(&format!(" due {due}"));
        }
    }
    // Only while a blocker is still open.
    if task.blocked {
        let on: Vec<String> = task.blocked_on.iter().map(|id| format!("#{id}")).collect();
        line.push_str(&format!(" ⛔{}", on.join(",")));
    }
    // The density mark lives here, where it can't be read past as it was in an
    // edit's output. The size only: the findings are in `task show`.
    if let Some(chars) = task.sprawl_chars {
        line.push_str(&format!(" [sprawl {}]", thousands(chars)));
    }
    if task.assignee.kind != AssigneeKind::Nobody {
        line.push_str(&format!(" ({})", task.assignee.label()));
    }
    line
}

/// `18.2K`, `940`: rounded, because the question is whether to rewrite, not
/// arithmetic.
pub fn thousands(chars: u32) -> String {
    if chars < 1_000 {
        return chars.to_string();
    }
    format!("{:.1}K", f64::from(chars) / 1_000.0)
}
