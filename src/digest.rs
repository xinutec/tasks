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
//! **Who it selects for is a fourth rule**: a session sees its own open tasks
//! and the pile, never what another conversation holds. That selection is
//! [`crate::tasks::repo::Filter::digest_for`] and the reasoning lives with it,
//! including why the pile may not be dropped.
//!
//! **A fifth rule arrived with the ranks: `P4` is counted and never recited.**
//! It is defined as *"kept as a record rather than a plan; it may never
//! happen"*, and putting that in front of a session on every turn contradicts
//! its own definition. Measured 2026-08-17, the `life` session's entire
//! 1112-byte digest was its P3/P4 tail: 12 of 13 open tasks were `P4`, so it
//! paid for a parked wishlist every turn and for nothing else. See `parked`,
//! which also records why `P3` is not treated the same way.
//!
//! **A session can narrow this further, for a few hours, and only itself.**
//! `task focus 849 850 --for 4h` — see [`crate::tasks::focus`], which is the
//! only thing in the service that hides an *open* task and carries the three
//! rules that make that safe. The two trims here compose in one direction:
//! focus runs first and the pile cap runs on what survives, or the five pile
//! lines would be spent on tasks the focus then hid.
//!
//! **And it advertises itself once, above a floor.** A session carrying more
//! than [`FOCUS_HINT_LINES`] recited lines of its own is told `focus` exists.
//! This is the only feature the digest advertises, and it is here on the same
//! ground the TaskCreate/TaskUpdate line stands on: a doc cannot win an
//! argument with a per-turn reminder. Measured 2026-08-18, `task focus` with
//! real ids appeared in one episode across every transcript on the machine,
//! by one session, while another carried 49 lines on every turn.
//!
//! The selection lives in [`Filter::digest_for`](crate::tasks::repo::Filter),
//! not here — this module is handed a list and renders it. Which is why the
//! render tests can stay about cost.

use crate::tasks::focus::{self, Focus};
use crate::tasks::types::{AssigneeKind, Priority, Status, Task};

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

/// How many recited lines of its own a session carries before the digest says
/// `focus` exists.
///
/// ⚠ **Recited lines it HOLDS, not open tasks.** A task that was trimmed —
/// parked at `P4`, or past the pile cap — is one the session is not paying for,
/// so it cannot be part of the argument that it is paying too much. And the
/// pile is charged to everybody rather than to the holder, with `PILE_LINES`
/// as its remedy; counting it here would tell a session with three of its own
/// to go and focus on them.
///
/// **Twelve, from the distribution rather than from taste.** Measured
/// 2026-08-18, once `P4` stopped being recited: 49 lines, 15, then 10, 10, 10,
/// and a tail of 6, 6, 5, 4, 3, 3, 1. The break is between 15 and 10, so this
/// fires for the two sessions genuinely paying and costs the other ten nothing.
/// An earlier reading of the same fleet put the floor here too and would have
/// caught five — that was the distribution *before* the `P4` trim, and it is
/// why this number is written down with its date.
///
/// ⚠ **The hint is a capability, not advice.** It says what `focus` does and
/// leaves the decision alone. A digest that tells a session to hide its work is
/// pushing exactly the sessions most likely to hide something they should be
/// doing, and it has no way of knowing which two tasks matter.
pub const FOCUS_HINT_LINES: usize = 12;

/// Whether a task is kept as a record rather than recited as a plan.
///
/// `P4` is defined in `task --help` as *"kept as a record rather than a plan;
/// it may never happen"*. Putting that in front of a session on every turn
/// contradicts the definition: it is the one rank whose filer has said it is
/// not work, pushed at the holder more often than anything they chose to do.
/// Measured before this existed, the `life` session's whole 1112-byte digest
/// was its P3/P4 tail — 12 of 13 open tasks were P4.
///
/// ⚠ **The effective rank, not the chosen one**, for the reason
/// [`focus::breaks_through`] gives: a deadline inside the week raises a task
/// without anything being written, and reading `priority` here would bury
/// exactly the task the escalation exists to raise. Overdue is its own arm for
/// the same reason it is one there — a date that has already passed must not go
/// quiet, whatever rank it was filed at.
///
/// ⚠ **P3 is deliberately not here.** It means a workaround exists and is in
/// use, which is still a plan, and the eleven P3s the `home` session was
/// carrying get done *because* they are read. Hide the rank and filing at P3
/// becomes filing into a drawer, so the pressure inverts and everything is
/// ranked P2 to stay visible — a change to what the ranks mean rather than to
/// what a page shows.
fn parked(task: &Task) -> bool {
    !task.overdue && task.urgency() == Some(Priority::P4)
}

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
    let mut parked_hidden = 0usize;
    // Counted as lines are selected rather than from `tasks`, because the floor
    // below is about what this prompt costs and a trimmed task costs nothing.
    let mut own = 0usize;
    let mut selected: Vec<&Task> = Vec::with_capacity(tasks.len());
    for task in tasks {
        let unheld = task.assignee.kind == AssigneeKind::Nobody;
        // A focus names its tasks in so many words, so it outranks both trims
        // below: if a session has said it is working on something parked, that
        // is the task it means, and a default must not overrule it.
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
    // ⚠ **Counted, never silent** — the third trim to follow the rule, and the
    // reason the head still says the full number: a page that says "13 open"
    // above one line has to explain itself where the discrepancy appears.
    // Costs nothing at all on a session with nothing parked, which is most of
    // them, and that is what makes it allowable in the one place every session
    // pays on every turn.
    if parked_hidden > 0 {
        out.push(format!(
            "⚠ {parked_hidden} at P4 not shown — kept as a record; `task list`."
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
    // ⚠ **Last, and never beside a focus.** A session that has already focused
    // has the notice above telling it how to *end* the thing this would be
    // recommending, and saying both is noise that contradicts itself.
    //
    // This is the one place the digest advertises a feature, and it is here
    // because a doc cannot win an argument with a per-turn reminder — the same
    // ground the TaskCreate/TaskUpdate line in the header stands on. `focus`
    // was reachable only from `task focus --help`, and across every transcript
    // on the machine it had been used with real ids once, by one session, while
    // one conversation carried 49 lines on every turn.
    if focus.is_none() && own > FOCUS_HINT_LINES {
        out.push(format!(
            "⚠ {own} of these are yours — `task focus <id>… --for 4h` recites only what you name."
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
    // ⚠ **The one place this can be said where it cannot evaporate.** A density
    // read's findings used to exist only as the tail of a successful edit, in
    // the transcript of whichever session happened to make it — and were read
    // past: of the 43 tasks read more than once in the 5.6 days to 2026-08-29,
    // 28 only ever grew. This module already argues that a doc cannot win an
    // argument with a per-turn reminder; that cuts both ways, and this is the
    // reminder.
    //
    // ⚠ **The size, not the words.** The findings are prose and belong in `task
    // show`; a number is what makes the marker worth reading, and it is the one
    // thing about a sprawling body that is not a matter of taste. Costs nothing
    // on a task with nothing outstanding, which is nearly all of them.
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
/// Rounded because the precision is not the point — the reader is deciding
/// whether a body has got away from them, and `18162` invites arithmetic where
/// `18.2K` invites a rewrite. Under a thousand it is printed as it is: a body
/// that small is not what this marks, and `0.9K` would be a claim about
/// precision the number does not have.
pub fn thousands(chars: u32) -> String {
    if chars < 1_000 {
        return chars.to_string();
    }
    format!("{:.1}K", f64::from(chars) / 1_000.0)
}
