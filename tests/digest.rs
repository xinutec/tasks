//! The index a prompt receives.
//!
//! This is the one file in the repository whose assertions are about *cost*.
//! Every other test asks whether the app is correct; these ask whether it is
//! still cheap, because the failure that produced this whole project was not a
//! wrong answer — it was a correct answer that cost 86 kB to deliver 3.9 kB, on
//! every turn, and nothing anywhere said so.

use chrono::{TimeZone, Utc};
use tasks::digest::{MAX_BYTES, PILE_LINES, render};
use tasks::tasks::types::{Assignee, AssigneeKind, Priority, Status, Task};

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
