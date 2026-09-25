//! The only thing a prompt ever sees.
//!
//! This is re-serialised into a conversation on every turn, so it has to stay
//! cheap. A change that breaks one of these rules is a regression however good
//! it looks:
//!
//! 1. **One line per task, and the line is the subject** — enough to decide
//!    whether to open the task. No body, timestamps or history.
//! 2. **Only open tasks.** [`Filter::include_closed`](crate::tasks::repo::Filter)
//!    is never set on this path, and *open* is [`Status::is_open`], not "not
//!    done": a dropped task is closed too.
//! 3. **A budget, enforced.** Past [`MAX_BYTES`] the digest stops and says how
//!    many it left out.
//! 4. **A session sees its own tasks and the pile**, never another
//!    conversation's. That selection, and why the pile may not be dropped, is
//!    [`Filter::digest_for`](crate::tasks::repo::Filter::digest_for); this
//!    module is handed a list and renders it.
//! 5. **`P4` is counted, never recited** — see `parked`.
//!
//! A session can also narrow its own digest for a few hours with `task focus`
//! ([`crate::tasks::focus`], the only thing that hides an *open* task), and one
//! carrying more than [`FOCUS_HINT_LINES`] lines of its own is told it can.
//!
//! **Every trim is counted, never silent**: the head gives the full number, so
//! the difference is explained where it appears.

use crate::tasks::focus::{self, Focus};
use crate::tasks::types::{AssigneeKind, Priority, Status, Task};

/// A runaway stop, not a policy: an index this large has had content written
/// into its subjects.
pub const MAX_BYTES: usize = 25_000;

/// How many unheld tasks a digest recites before it starts counting them.
///
/// ⚠ **The pile has a different denominator.** A held task is in one
/// conversation's prompt; an unheld one is in *every* conversation's, every
/// turn. So a pile line costs as many prompts as there are live sessions, and
/// [`MAX_BYTES`] would stop the pile only long after it had become ruinous.
///
/// Small, because the pile is a handover channel, not a backlog: enough to
/// notice that something is waiting. Growing this spends every conversation's
/// context to save one `task list`.
pub const PILE_LINES: usize = 5;

/// How many recited lines of its own a session carries before the digest says
/// `focus` exists.
///
/// ⚠ **Recited lines it HOLDS, not open tasks.** A trimmed task costs the
/// session nothing, so it is no argument that it pays too much; and the pile is
/// charged to everybody, with [`PILE_LINES`] as its remedy.
///
/// Set at the break in the distribution of recited lines across sessions: the
/// few far above it are the ones genuinely paying, and the rest cost nothing.
///
/// ⚠ **The hint states a capability, not advice.** The digest cannot know which
/// tasks matter, and telling a session to hide its work pushes the sessions
/// most likely to hide something they should be doing.
pub const FOCUS_HINT_LINES: usize = 12;

/// Whether a task is counted rather than recited. `P4` is the level where
/// nothing is being paid today, so it gains least from being read every turn,
/// and a session with a parked wishlist would otherwise pay for it every turn.
///
/// ⚠ **Counted is not closed, and the notice must not say otherwise.** An open
/// task is work; a record is a CLOSED task, which `task list --done` still
/// finds. Wording that called this "kept as a record" led a session to leave
/// decided questions open.
///
/// ⚠ **The effective rank, not the chosen one**, and overdue is its own arm —
/// both for the reasons [`focus::breaks_through`] gives.
///
/// ⚠ **`P3` is deliberately not here.** It means a workaround is in use, which
/// is still a plan, and P3 work gets done *because* it is read. Hide it and
/// everything is ranked P2 to stay visible — a change to what the ranks mean.
fn parked(task: &Task) -> bool {
    !task.overdue && task.urgency() == Some(Priority::P4)
}

/// Render the index for a set of tasks, already filtered to the open ones.
///
/// One flat list in id order, which no trim reshuffles. No group headers: they
/// would cost a line per group every turn to say what the subject already says.
///
/// ⚠ **A focus is applied as given; its expiry is not checked here.**
/// [`focus::current`] decides whether it still holds and is the only reader of
/// the clock. The digest is cached and read later, so a renderer comparing
/// against its own `now` would answer for a moment that has passed — the same
/// reason a due date is printed as a date, never a countdown.
pub fn render(tasks: &[Task], focus: Option<&Focus>) -> String {
    if tasks.is_empty() {
        return String::new();
    }

    let doing = tasks.iter().filter(|t| t.status == Status::Doing).count();

    // Focus first, then the pile cap on what survives it: capping first would
    // spend the pile lines on tasks the focus then hides.
    let mut focus_hidden_own = 0usize;
    let mut focus_hidden_pile = 0usize;
    // The pile, trimmed in id order so the oldest survives: it is the one at
    // risk of being forgotten, and the newest is still remembered by its filer.
    let mut piled = 0usize;
    let mut pile_hidden = 0usize;
    let mut parked_hidden = 0usize;
    // Counted from what is selected, not from `tasks`: a trimmed task costs
    // this prompt nothing.
    let mut own = 0usize;
    let mut selected: Vec<&Task> = Vec::with_capacity(tasks.len());
    for task in tasks {
        let unheld = task.assignee.kind == AssigneeKind::Nobody;
        // A task the focus names outranks both trims below: a default must not
        // overrule what the session said it is working on.
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
    // ⚠ **The second sentence answers a per-turn reminder.** Claude Code
    // suggests TaskCreate/TaskUpdate every turn — the built-in store this
    // service replaced. A doc read once cannot outweigh an instruction repeated
    // every message, so the counter is repeated too, in the one channel we own.
    // Every session pays for this line every turn: resist adding to it.
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
        // Checked before pushing: going over and then reporting it would already
        // have cost what the budget prevents.
        if bytes + line.len() + 1 > MAX_BYTES {
            omitted = 1;
            continue;
        }
        bytes += line.len() + 1;
        out.push(line);
    }

    // ⚠ **A focus says what it hid and how to stop** — even when it hid
    // nothing, since it explains the shape of the list. A short list must not
    // read as an empty plate, and a session that does not know P0 still
    // arrives will run `task list` every turn to check.
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

    // Separate notices, never merged: different failures with different
    // remedies. Each costs nothing when there is nothing to report.
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
    // ⚠ **Last, and never beside a focus**, whose notice already says how to
    // end what this recommends. The only feature the digest advertises, on the
    // header's ground: reachable only from `--help`, `focus` went unused.
    if focus.is_none() && own > FOCUS_HINT_LINES {
        out.push(format!(
            "⚠ {own} of these are yours — `task focus <id>… --for 4h` recites only what you name."
        ));
    }
    out.join("\n")
}

/// One task as one line.
///
/// The holder is named in one word when it is somebody — work moving between
/// holders is what this system shows. `nobody` prints nothing, or the pile
/// would be a column of "(nobody)".
fn line(task: &Task) -> String {
    let mut line = format!("{} **#{}** {}", task.status.marker(), task.id, task.subject);
    // `repo::list` has already sorted by rank; this says why a line is where
    // it is. `escalated_to` first, because it is what the list sorted by, and
    // `!` says a deadline raised it above the rank chosen — `task show` gives
    // both.
    if let Some(raised) = task.escalated_to {
        line.push_str(&format!(" [{raised}!]"));
    } else if let Some(priority) = task.priority {
        line.push_str(&format!(" [{priority}]"));
    }
    // ⚠ **The date, not a countdown**: the line is cached and read later, and a
    // date is the same fact whenever it is read. OVERDUE is shouted because
    // nothing else on the line would show the change of state.
    if let Some(due) = task.due {
        if task.overdue {
            line.push_str(&format!(" OVERDUE {due}"));
        } else {
            line.push_str(&format!(" due {due}"));
        }
    }
    // Only while a blocker is still open, which is what `blocked` means.
    if task.blocked {
        let on: Vec<String> = task.blocked_on.iter().map(|id| format!("#{id}")).collect();
        line.push_str(&format!(" ⛔{}", on.join(",")));
    }
    // ⚠ **Here because here it cannot evaporate.** Density findings shown only
    // in the output of the edit that raised them were read past, and bodies
    // kept growing. The size, not the words: the findings belong in `task show`,
    // and a number is the one thing about a sprawling body that is not taste.
    if let Some(chars) = task.sprawl_chars {
        line.push_str(&format!(" [sprawl {}]", thousands(chars)));
    }
    if task.assignee.kind != AssigneeKind::Nobody {
        line.push_str(&format!(" ({})", task.assignee.label()));
    }
    line
}

/// A character count as a reader thinks of it: `18.2K`, `940`.
///
/// Rounded because the reader is deciding whether a body has got away from
/// them: `18162` invites arithmetic where `18.2K` invites a rewrite. Under a
/// thousand it prints as it is.
pub fn thousands(chars: u32) -> String {
    if chars < 1_000 {
        return chars.to_string();
    }
    format!("{:.1}K", f64::from(chars) / 1_000.0)
}
