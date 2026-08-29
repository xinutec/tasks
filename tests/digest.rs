//! The index a prompt receives.
//!
//! This is the one file in the repository whose assertions are about *cost*.
//! Every other test asks whether the app is correct; these ask whether it is
//! still cheap, because the failure that produced this whole project was not a
//! wrong answer — it was a correct answer that cost 86 kB to deliver 3.9 kB, on
//! every turn, and nothing anywhere said so.

use chrono::{TimeZone, Utc};
use tasks::digest::{FOCUS_HINT_LINES, MAX_BYTES, PILE_LINES};
use tasks::tasks::types::{Assignee, AssigneeKind, Priority, Status, Task};

/// The digest of a session that has not focused on anything — which is every
/// session almost all of the time, and the shape every assertion below about
/// cost is about. A focus is passed explicitly where it is the subject.
fn render(tasks: &[Task]) -> String {
    tasks::digest::render(tasks, None)
}

fn task(id: u64, subject: &str, status: Status, assignee: Assignee) -> Task {
    let at = Utc.with_ymd_and_hms(2026, 8, 8, 12, 0, 0).unwrap();
    Task {
        id,
        subject: subject.to_string(),
        status,
        priority: None,
        due: None,
        escalated_to: None,
        overdue: false,
        blocked_on: Vec::new(),
        blocked: false,
        assignee,
        detailed: false,
        sprawl_chars: None,
        filed_by: None,
        created_at: at,
        updated_at: at,
        closed_at: None,
    }
}

fn open(id: u64, subject: &str) -> Task {
    task(id, subject, Status::Open, Assignee::nobody())
}

fn person(id: &str) -> Assignee {
    Assignee {
        kind: AssigneeKind::Person,
        id: Some(id.to_string()),
        name: Some(id.to_string()),
    }
}

fn held(id: u64, subject: &str, by: &str) -> Task {
    task(
        id,
        subject,
        Status::Open,
        Assignee {
            kind: AssigneeKind::Session,
            id: Some(by.to_string()),
            name: Some(by.to_string()),
        },
    )
}

#[test]
fn nothing_open_says_nothing_at_all() {
    // Silence is the contract: a hook prints this verbatim, and a line saying
    // "0 open tasks" would be a per-turn cost for the absence of work.
    assert_eq!(render(&[]), "");
}

#[test]
fn one_line_per_task_and_the_line_is_the_subject() {
    let out = render(&[
        open(1, "Stop walking every transcript on the request path"),
        open(2, "The picture button is ugly on the left"),
    ]);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3, "one header and two tasks: {out}");
    assert_eq!(
        lines[1],
        "- [ ] **#1** Stop walking every transcript on the request path"
    );
    assert!(lines[0].starts_with("2 open task(s)"), "{}", lines[0]);
}

#[test]
fn nothing_groups_the_list_and_no_heading_is_spent() {
    // The repository was dropped in `0004`, and with it the group headings. This
    // asserts the cost, not the absence of a feature: a heading is a whole line
    // in the one place that is re-sent on every turn, and re-grouping under some
    // other key would spend it again.
    let out = render(&[open(1, "a"), open(2, "b"), open(3, "c")]);
    assert_eq!(out.lines().count(), 4, "one header and three tasks: {out}");
    assert!(!out.contains("across"), "{out}");
}

#[test]
fn work_in_hand_is_counted_and_marked() {
    let mut doing = open(2, "being worked on");
    doing.status = Status::Doing;
    let out = render(&[open(1, "waiting"), doing]);
    assert!(out.contains("1 in progress"), "{out}");
    assert!(out.contains("- [>] **#2** being worked on"), "{out}");
}

#[test]
fn a_holder_is_named_and_nobody_is_not() {
    let mut held = open(2, "yours");
    held.assignee = person("pippijn");
    let out = render(&[open(1, "in the pile"), held]);
    assert!(out.contains("- [ ] **#1** in the pile\n"), "{out}");
    assert!(out.contains("- [ ] **#2** yours (pippijn)"), "{out}");
    assert!(!out.contains("(nobody)"), "{out}");
}

/// The pile is the one part of a digest that nothing else bounds.
///
/// ⚠ **[`MAX_BYTES`] is not this guard.** It is a runaway stop at 25 kB — some
/// two hundred lines in every conversation before it says a word — and it is
/// per session, which is the wrong denominator for the pile: an unheld task is
/// charged to *every* session on *every* turn, so one filed line costs as many
/// prompts as there are live conversations. The affordability argument in
/// `README.md` was measured at *3 unheld of 134 open*, and it is conditional on
/// a number nothing was keeping down — two days after the cutover the recall
/// session's digest was 5 pile lines against its own 3.
#[test]
fn the_pile_is_bounded_however_long_it_gets() {
    let long: Vec<Task> = (1..=40)
        .map(|id| open(id, "left for whoever picks it up"))
        .collect();
    let out = render(&long);
    let shown = out.lines().filter(|l| l.starts_with("- [")).count();
    assert_eq!(shown, PILE_LINES, "the pile was recited in full:\n{out}");
    assert!(out.contains("35 more in the pile"), "{out}");
    // Still counted, so the header does not under-report the work there is.
    assert!(out.starts_with("40 open task(s)"), "{out}");
}

/// Ablation for the test above: without the cap the same input is 40 lines,
/// every one of them in every conversation.
#[test]
fn the_cap_is_what_makes_the_test_above_pass() {
    // The same forty subjects, held instead of piled: all forty render, so the
    // fixture is large enough to have been capped and the cap is what stopped
    // it above rather than the byte budget or the fixture's own size.
    let mine: Vec<Task> = (1..=40).map(|id| held(id, "mine", "recall")).collect();
    let out = render(&mine);
    assert_eq!(
        out.lines().filter(|l| l.starts_with("- [")).count(),
        40,
        "the cap is not the pile's: it hid work in hand\n{out}"
    );
}

/// The cap is the pile's alone.
///
/// A session is accountable for what it holds, and a plate that quietly stops
/// listing is how a task is forgotten by the one conversation that agreed to do
/// it. Growth there is a backlog to work off — `task list --mine` says how much
/// — not a cost to spread over everybody else.
#[test]
fn what_a_session_holds_is_never_hidden_by_the_pile() {
    let mut tasks: Vec<Task> = (1..=30).map(|id| open(id, "in the pile")).collect();
    tasks.extend((100..=120).map(|id| held(id, "on my plate", "recall")));
    let out = render(&tasks);
    for id in 100..=120 {
        assert!(
            out.contains(&format!("**#{id}**")),
            "own task {id} was hidden:\n{out}"
        );
    }
}

#[test]
fn a_pile_short_enough_to_read_is_shown_whole() {
    let out = render(&[open(1, "a"), open(2, "b"), open(3, "c")]);
    assert!(!out.contains("in the pile"), "a notice for nothing: {out}");
    assert_eq!(out.lines().count(), 4, "{out}");
}

/// The order is the id, and the cap does not reshuffle it.
///
/// Own-first was the obvious way to write the cap and is not what this does:
/// grouping is a line spent to say what the holder already says, and `render`
/// promises one flat list.
#[test]
fn the_cap_keeps_the_list_in_id_order() {
    let mut tasks: Vec<Task> = vec![held(2, "mine", "recall"), held(9, "mine", "recall")];
    tasks.extend((1..=8).filter(|id| *id != 2).map(|id| open(id, "pile")));
    tasks.sort_by_key(|t| t.id);
    let out = render(&tasks);
    let ids: Vec<u64> = out
        .lines()
        .filter_map(|l| l.split("**#").nth(1))
        .filter_map(|l| l.split("**").next())
        .filter_map(|l| l.parse().ok())
        .collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "the cap reordered the list:\n{out}");
    assert!(ids.contains(&9), "a held task fell off the end:\n{out}");
}

/// The property the whole service exists to hold.
#[test]
fn the_digest_stays_inside_its_budget_however_many_tasks_there_are() {
    // Held, not piled: the pile has its own cap now, and a fixture that trips
    // that one first would leave the byte budget unexercised while still
    // passing. This budget is the guard on a session's OWN plate, which is the
    // half nothing else bounds.
    let many: Vec<Task> = (1..=4000)
        .map(|id| {
            held(
                id,
                "a subject of a length that a real one might plausibly reach",
                "recall",
            )
        })
        .collect();
    let out = render(&many);
    assert!(
        out.len() <= MAX_BYTES + 400,
        "digest is {} bytes for 4000 tasks",
        out.len()
    );
    // And it says so rather than truncating quietly: a list that silently stops
    // reads as a list that has ended.
    assert!(out.contains("not shown"), "no notice of the omission");
    assert!(
        out.contains("over its"),
        "the notice does not say why: {}",
        out.lines().last().unwrap_or_default()
    );
}

/// Ablation for the test above: without the budget the same input is enormous.
/// Kept because a cost assertion that cannot fail is the failure mode this
/// project has hit twice.
#[test]
fn the_budget_is_what_makes_the_test_above_pass() {
    let many: Vec<Task> = (1..=4000)
        .map(|id| {
            held(
                id,
                "a subject of a length that a real one might plausibly reach",
                "recall",
            )
        })
        .collect();
    let unbudgeted: usize = many
        .iter()
        .map(|t| t.subject.len() + t.id.to_string().len() + 12)
        .sum();
    assert!(
        unbudgeted > MAX_BYTES * 8,
        "the fixture is too small to prove anything: {unbudgeted} bytes"
    );
}

/// The filer never reaches a prompt.
///
/// ⚠ **`filed_by` was added so a session can rule a pile task out without
/// opening it — in a LIST, which is fetched when somebody has just asked.** The
/// digest is not that; it is the per-turn cost, and most open tasks are in the
/// pile, so a word on each of them is a per-task charge levied on every session
/// forever. That is the exact shape this file exists to refuse, and it would
/// arrive wearing a good argument, which is why the guard is a test rather than
/// a comment.
#[test]
fn the_digest_never_says_who_filed_a_task() {
    let mut task = open(1, "Left for whoever picks it up");
    let silent = render(std::slice::from_ref(&task));
    task.filed_by = Some("observe".into());
    let told = render(std::slice::from_ref(&task));
    assert_eq!(
        told,
        silent,
        "the filer reached a prompt, at {} bytes a turn",
        told.len() - silent.len()
    );
}

#[test]
fn the_header_countermands_the_built_in_task_tools() {
    // Not decoration, and not a doc's job: Claude Code emits "consider using
    // TaskCreate…" on every turn, and `docs/for-sessions.md` is read once. A
    // session that skims will do as the repeated instruction says — which is to
    // write into the store that cost 527 kB a turn and is what this replaced.
    // The counter has to be in the digest because the digest is the only thing
    // that is also there every turn.
    let out = render(&[open(1, "Something")]);
    let head = out.lines().next().expect("a header");
    assert!(head.contains("TaskCreate"), "{head}");
    assert!(head.contains("task add"), "{head}");

    // And it is in the HEADER, so it is paid for once per turn rather than once
    // per task. A per-task cost is the shape this service exists to refuse.
    assert!(
        !out.lines().skip(1).any(|line| line.contains("TaskCreate")),
        "the warning is repeated per task:\n{out}"
    );
}

/// ⚠ **A rank costs nothing until somebody sets one**, which is the only reason
/// this is allowed in the file that reaches every prompt on every turn.
///
/// Almost every task is unranked and always will be, so the ordinary line must
/// be byte-for-byte what it was before the column existed. A default of `P2` —
/// the shape rejected in `migrations/0005_priority.sql` — would have spent five
/// bytes a line, on every line, in every conversation, to say nothing.
#[test]
fn an_unranked_task_costs_exactly_what_it_did_before() {
    let unranked = open(1, "in the pile");
    let plain = render(std::slice::from_ref(&unranked));
    assert_eq!(
        plain.lines().nth(1),
        Some("- [ ] **#1** in the pile"),
        "an unranked line grew:\n{plain}"
    );

    let mut ranked = unranked;
    ranked.priority = Some(Priority::P0);
    let out = render(&[ranked]);
    assert!(out.contains("- [ ] **#1** in the pile [P0]"), "{out}");
    // The same fixture twice, so the difference IS the marker: five bytes — a
    // space, two brackets, two characters — spent only where somebody ranked
    // something. Stated as a number because this file's assertions are about
    // cost.
    assert_eq!(out.len() - plain.len(), 5, "{out}");
}

/// `render` does not sort, and must not start.
///
/// The one ordering in the service is `repo::list`'s `ORDER BY`, so a digest
/// receives its tasks already ranked. A second sort here would be a second rule
/// to keep true, and it would fight the pile cap — which trims the tail of
/// whatever order it is handed.
#[test]
fn the_order_is_the_one_render_was_handed() {
    let mut urgent = held(9, "ranked but last in the list", "recall");
    urgent.priority = Some(Priority::P0);
    let out = render(&[held(1, "first", "recall"), urgent]);
    let ids: Vec<u64> = out
        .lines()
        .filter_map(|l| l.split("**#").nth(1))
        .filter_map(|l| l.split("**").next())
        .filter_map(|l| l.parse().ok())
        .collect();
    assert_eq!(ids, vec![1, 9], "render reordered by priority:\n{out}");
}

/// `P4` is *"kept as a record rather than a plan; it may never happen"* — the
/// CLI's own definition. Reciting it on every turn contradicts it: it is the
/// one rank the filer has said is not a plan, pushed at the holder more often
/// than anything they chose to do.
///
/// Measured 2026-08-17, before this: the `life` session's digest was 1112 bytes
/// of which **100%** was its P3/P4 tail — 12 of its 13 open tasks were P4.
#[test]
fn a_task_kept_as_a_record_is_counted_and_not_recited() {
    let mut parked = held(2, "whole-house inventory, some day", "life");
    parked.priority = Some(Priority::P4);
    let out = render(&[held(1, "the actual work", "life"), parked]);

    assert!(out.contains("**#1**"), "the plan went missing:\n{out}");
    assert!(
        !out.contains("**#2**"),
        "a P4 was recited into every turn:\n{out}"
    );
}

/// Rule 2 of every trim in this service: the party paying for it is told it
/// happened. The head still counts the parked task, so the discrepancy between
/// "2 open" and one line has to be explained on the page where it appears.
#[test]
fn what_is_parked_is_said_and_still_counted() {
    let mut parked = held(2, "some day", "life");
    parked.priority = Some(Priority::P4);
    let out = render(&[held(1, "the actual work", "life"), parked]);

    assert!(out.starts_with("2 open task(s)"), "{out}");
    assert!(out.contains("1 at P4"), "the trim went silent:\n{out}");
    assert!(out.contains("task list"), "no way back to it:\n{out}");
}

/// ⚠ **The effective rank, not the chosen one** — the same rule
/// `focus::breaks_through` follows. A deadline inside the week raises a task to
/// `P0` without anything being written, and reading `priority` here would let
/// this trim bury exactly the task the escalation exists to raise.
#[test]
fn a_parked_task_a_deadline_has_raised_is_recited() {
    let mut raised = held(2, "parked until the date got close", "life");
    raised.priority = Some(Priority::P4);
    raised.escalated_to = Some(Priority::P0);
    let out = render(&[raised]);
    assert!(
        out.contains("**#2**"),
        "an escalation was buried by the P4 trim:\n{out}"
    );
}

/// Overdue is its own arm rather than a consequence, for the reason
/// `breaks_through` gives: a task can be past its date with no rank at all, and
/// a deadline that has already passed is the one thing that must not go quiet.
#[test]
fn a_parked_task_past_its_date_is_recited() {
    let mut late = held(2, "parked and now late", "life");
    late.priority = Some(Priority::P4);
    late.overdue = true;
    let out = render(&[late]);
    assert!(out.contains("**#2**"), "an overdue task went quiet:\n{out}");
}

/// A focus is a session saying what it is working on, in so many words. If it
/// names a parked task then that is the task it means, and a default trim must
/// not overrule something typed on purpose.
#[test]
fn a_focus_that_names_a_parked_task_shows_it() {
    let mut parked = held(2, "some day, but today", "life");
    parked.priority = Some(Priority::P4);
    let focus = tasks::tasks::focus::Focus {
        tasks: [2].into_iter().collect(),
        until: Utc.with_ymd_and_hms(2026, 8, 8, 16, 0, 0).unwrap(),
    };
    let out = tasks::digest::render(&[parked], Some(&focus));
    assert!(
        out.contains("**#2**"),
        "a focus was overruled by the P4 trim:\n{out}"
    );
}

/// The trim costs nothing on the sessions that have no parked work, which is
/// most of them. Same argument as the priority marker above: stated as a
/// number, because this file's assertions are about cost.
#[test]
fn nothing_parked_costs_nothing() {
    let mut ordinary = held(1, "ordinary work", "health");
    ordinary.priority = Some(Priority::P2);
    let out = render(std::slice::from_ref(&ordinary));
    assert!(
        !out.contains("P4"),
        "a session with no P4 paid for it:\n{out}"
    );
    assert_eq!(out.lines().count(), 2, "{out}");
}

/// A session carrying 49 recited lines pays for all 49 on every turn and is
/// working on two of them. `focus` is the only thing in the service that lets
/// it say which two, and until this existed the digest never named it: the
/// feature was reachable only from `task focus --help`, which nothing prompts
/// anybody to run. Measured across every transcript on the machine, `focus`
/// with real ids appeared in one episode, by one session, ever.
#[test]
fn a_session_carrying_too_much_is_told_focus_exists() {
    let many: Vec<Task> = (1..=FOCUS_HINT_LINES as u64 + 1)
        .map(|id| held(id, "work", "health"))
        .collect();
    let out = render(&many);
    assert!(out.contains("task focus"), "no hint at the floor:\n{out}");
}

/// The header is the one line every session pays for on **every** turn, and
/// this module's rule is to resist growing it. A conditional line is the only
/// defensible form: it must cost exactly nothing on the sessions below the
/// floor, which is most of them.
#[test]
fn a_session_below_the_floor_pays_nothing_for_the_hint() {
    let few: Vec<Task> = (1..=FOCUS_HINT_LINES as u64)
        .map(|id| held(id, "work", "health"))
        .collect();
    let out = render(&few);
    assert!(
        !out.contains("focus"),
        "a short digest paid for the hint:\n{out}"
    );
}

/// ⚠ **What the session HOLDS, not what the page shows.** The pile is a
/// handover channel with a different denominator, capped at `PILE_LINES` and
/// charged to everybody — and `focus` is not the remedy for it. Counting it
/// here would tell a session with three of its own to go and focus.
#[test]
fn the_pile_does_not_push_a_session_over_the_floor() {
    let mut tasks: Vec<Task> = (1..=FOCUS_HINT_LINES as u64)
        .map(|id| held(id, "mine", "health"))
        .collect();
    tasks.extend((100..=140).map(|id| open(id, "unheld")));
    let out = render(&tasks);
    assert!(
        !out.contains("task focus"),
        "the pile was counted against the holder:\n{out}"
    );
}

/// A task the digest did not recite is one the session is not paying for, so it
/// cannot be part of the argument that it is paying too much. This is what makes
/// the floor a statement about cost rather than about backlog size.
#[test]
fn what_was_never_recited_does_not_count_toward_the_floor() {
    let mut tasks: Vec<Task> = (1..=FOCUS_HINT_LINES as u64)
        .map(|id| held(id, "mine", "health"))
        .collect();
    for id in 100..=140 {
        let mut parked = held(id, "some day", "health");
        parked.priority = Some(Priority::P4);
        tasks.push(parked);
    }
    let out = render(&tasks);
    assert!(
        !out.contains("task focus"),
        "parked work was counted as cost:\n{out}"
    );
}

/// Telling a session that has already focused to focus is noise, and worse, it
/// contradicts the notice directly above it — which says how to *end* the thing
/// the hint would be recommending.
#[test]
fn a_session_that_has_already_focused_is_not_told_to() {
    let many: Vec<Task> = (1..=FOCUS_HINT_LINES as u64 + 1)
        .map(|id| held(id, "work", "health"))
        .collect();
    let focus = tasks::tasks::focus::Focus {
        tasks: (1..=20).collect(),
        until: Utc.with_ymd_and_hms(2026, 8, 8, 16, 0, 0).unwrap(),
    };
    let out = tasks::digest::render(&many, Some(&focus));
    assert_eq!(
        out.matches("task focus").count(),
        1,
        "the hint doubled up on the focus notice:\n{out}"
    );
}

/// The marker that makes a critique unignorable, in the one channel that repeats.
///
/// ⚠ **This module argues that a doc cannot win an argument with a per-turn
/// reminder, and that cuts both ways.** A density read's findings used to be the
/// tail of a successful edit: said once, to a session in the middle of something
/// else, and gone with its scrollback. Of the 43 tasks read more than once in
/// the 5.6 days to 2026-08-29, 28 only ever grew.
mod sprawl {
    use super::*;

    fn flagged(chars: u32) -> Task {
        let mut task = open(1, "a body that got away");
        task.sprawl_chars = Some(chars);
        task
    }

    #[test]
    fn a_flagged_body_says_so_in_the_prompt() {
        let digest = render(&[flagged(18_162)]);
        assert!(digest.contains("[sprawl 18.2K]"), "{digest}");
    }

    #[test]
    fn a_body_with_nothing_outstanding_costs_nothing() {
        // Nearly every task. The marker is allowed in the one place every
        // session pays for on every turn only because it is free when absent.
        let plain = open(1, "a body that got away");
        let quiet = render(std::slice::from_ref(&plain));
        assert!(!quiet.contains("sprawl"), "{quiet}");
        assert!(
            render(&[flagged(18_162)]).len() > quiet.len(),
            "the marker has to cost something when it is there, or it is not there"
        );
    }

    #[test]
    fn the_words_never_reach_the_prompt() {
        // The findings are prose and belong in `task show`. A digest that
        // carried them would be the accretion it is warning about, one level up
        // — so what a flag costs a prompt is bounded by the marker, whatever the
        // model wrote. `Task` has no field for the words, which is what makes
        // that structural; this pins the size it buys.
        let plain = render(&[open(1, "a body that got away")]);
        let flagged = render(&[flagged(18_162)]);
        let cost = flagged.len() - plain.len();
        assert!(cost <= 20, "a marker grew into content: {cost} bytes");
        assert_eq!(
            flagged.lines().count(),
            plain.lines().count(),
            "one line per task, and a flag does not buy a second"
        );
    }
}
