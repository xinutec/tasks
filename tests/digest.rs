//! The index a prompt receives.
//!
//! This is the one file in the repository whose assertions are about *cost*.
//! Every other test asks whether the app is correct; these ask whether it is
//! still cheap, because the failure that produced this whole project was not a
//! wrong answer — it was a correct answer that cost 86 kB to deliver 3.9 kB, on
//! every turn, and nothing anywhere said so.

use chrono::{TimeZone, Utc};
use tasks::digest::{MAX_BYTES, render};
use tasks::tasks::types::{Assignee, AssigneeKind, Status, Task};

fn task(id: u64, subject: &str, status: Status, assignee: Assignee) -> Task {
    let at = Utc.with_ymd_and_hms(2026, 8, 8, 12, 0, 0).unwrap();
    Task {
        id,
        subject: subject.to_string(),
        status,
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

/// The property the whole service exists to hold.
#[test]
fn the_digest_stays_inside_its_budget_however_many_tasks_there_are() {
    let many: Vec<Task> = (1..=4000)
        .map(|id| {
            open(
                id,
                "a subject of a length that a real one might plausibly reach",
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
            open(
                id,
                "a subject of a length that a real one might plausibly reach",
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
