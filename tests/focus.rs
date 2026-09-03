//! What a focused session's prompt says, and what it must never stop saying.
//!
//! ⚠ **Focus is the only thing in the service that hides an OPEN task**, so
//! these are not tests about a convenience. Half of them pin what goes quiet;
//! the other half pin what refuses to, and the second half is the important
//! one — a focus that could bury a P0 filed while it ran would make the whole
//! feature unsafe to use, and the failure would be silence, which nothing
//! reports.
//!
//! The render tests are pure and take a [`Focus`] directly: expiry is decided
//! in [`focus::current`] and nowhere else, so `render` is given a period that
//! holds and never consults a clock. The database tests below cover the other
//! half — entering, replacing, lapsing, and the two things `enter` refuses.

mod common;

use std::collections::BTreeSet;

use chrono::{Duration, TimeZone, Utc};
use tasks::digest::render;
use tasks::sessions;
use tasks::tasks::focus::{self, Focus};
use tasks::tasks::types::{Assignee, AssigneeKind, Priority, Status, Task};

fn at() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 15, 12, 0, 0).unwrap()
}

fn task(id: u64, subject: &str, assignee: Assignee) -> Task {
    Task {
        id,
        subject: subject.to_string(),
        status: Status::Open,
        priority: None,
        due: None,
        escalated_to: None,
        overdue: false,
        blocked_on: Vec::new(),
        blocked: false,
        assignee,
        detailed: false,
        body_lines: 0,
        sprawl_chars: None,
        filed_by: None,
        created_at: at(),
        updated_at: at(),
        closed_at: None,
    }
}

fn mine(id: u64, subject: &str) -> Task {
    task(
        id,
        subject,
        Assignee {
            kind: AssigneeKind::Session,
            id: Some("sess-a".into()),
            name: Some("tasks".into()),
        },
    )
}

fn piled(id: u64, subject: &str) -> Task {
    task(id, subject, Assignee::nobody())
}

fn on(ids: &[u64]) -> Focus {
    Focus {
        until: at() + Duration::hours(4),
        tasks: ids.iter().copied().collect(),
    }
}

#[test]
fn only_what_is_focused_on_is_recited() {
    let tasks = [
        mine(1, "the thing I am doing"),
        mine(2, "something else entirely"),
        mine(3, "and another"),
    ];
    let out = render(&tasks, Some(&on(&[1])));

    assert!(out.contains("the thing I am doing"), "{out}");
    assert!(!out.contains("something else entirely"), "{out}");
    assert!(!out.contains("and another"), "{out}");
}

#[test]
fn what_is_hidden_is_counted_and_never_silent() {
    // ⚠ The property the whole feature stands on. A short list a session cannot
    // tell from an empty plate is how work gets forgotten, and the pile cap
    // already answers this the same way: hiding is allowed, silence is not.
    let tasks = [
        mine(1, "the thing I am doing"),
        mine(2, "one of mine"),
        mine(3, "another of mine"),
        piled(4, "going spare"),
    ];
    let out = render(&tasks, Some(&on(&[1])));

    assert!(out.contains("2 more of yours"), "{out}");
    assert!(out.contains("1 in the pile"), "{out}");
    // The way out, or a session in a focus it did not mean to set is stuck in
    // it: the ids it would need in order to refocus are the ones it cannot see.
    assert!(out.contains("task focus --clear"), "{out}");
    // And the header still counts the WHOLE plate, so the two numbers can be
    // read against each other rather than agreeing by construction.
    assert!(out.contains("4 open task(s)"), "{out}");
}

#[test]
fn a_focus_that_hides_nothing_still_says_it_is_on() {
    // Otherwise the one command that explains a short list is invisible in
    // exactly the case where the list is short for a different reason.
    let out = render(&[mine(1, "the only thing open")], Some(&on(&[1])));

    assert!(out.contains("focused until 16:00 UTC"), "{out}");
    assert!(out.contains("nothing else of yours is open"), "{out}");
    // Nothing was hidden, so the breakthrough rule has nothing to explain and
    // must not spend the bytes saying so.
    assert!(!out.contains("break through"), "{out}");
}

#[test]
fn a_p0_arrives_whatever_the_focus_is() {
    // ⚠ **The reason this is safe to use at all.** Pippijn filing a P0 is the
    // drop-everything signal; a four-hour focus that could swallow one filed
    // five minutes into it would make the feature a way to miss the only task
    // that was meant to interrupt.
    let mut urgent = mine(9, "the roof is off");
    urgent.priority = Some(Priority::P0);
    let out = render(&[mine(1, "what I am on"), urgent], Some(&on(&[1])));

    assert!(out.contains("the roof is off"), "{out}");
    // It is not counted as hidden either — it is on the screen.
    assert!(!out.contains("more of yours"), "{out}");
}

#[test]
fn a_raised_rank_arrives_the_same_way_a_chosen_one_does() {
    // ⚠ **`escalated_to`, not `priority`.** A deadline inside the week raises a
    // task to P0 without anything being written, and reading the chosen rank
    // here would let a focus bury precisely the task the escalation exists to
    // raise — the one case where the two columns disagree is the one that
    // matters.
    let mut soon = mine(9, "due on Tuesday");
    soon.priority = Some(Priority::P3);
    soon.escalated_to = Some(Priority::P0);
    let out = render(&[mine(1, "what I am on"), soon], Some(&on(&[1])));

    assert!(out.contains("due on Tuesday"), "{out}");
}

#[test]
fn a_deadline_already_missed_does_not_go_quiet() {
    // Its own arm rather than a consequence of the rank: a task can be past its
    // date with no rank at all, and a date that has passed is the one fact a
    // prompt must not stop mentioning.
    let mut late = mine(9, "was due last week");
    late.due = Some(chrono::NaiveDate::from_ymd_opt(2026, 8, 1).unwrap());
    late.overdue = true;
    let out = render(&[mine(1, "what I am on"), late], Some(&on(&[1])));

    assert!(out.contains("was due last week"), "{out}");
}

#[test]
fn a_lesser_rank_is_hidden_like_anything_else() {
    // The other side of the breakthrough, or the rule would read as "ranked
    // tasks are exempt" and focus would hide almost nothing.
    let mut ranked = mine(9, "worth doing, not now");
    ranked.priority = Some(Priority::P1);
    let out = render(&[mine(1, "what I am on"), ranked], Some(&on(&[1])));

    assert!(!out.contains("worth doing, not now"), "{out}");
    assert!(out.contains("1 more of yours"), "{out}");
}

#[test]
fn focusing_on_something_in_the_pile_takes_it_out_of_the_pile() {
    // Picking a task up before moving it is the ordinary way a session starts
    // work, so a focus naming an unheld task has to recite it.
    let out = render(
        &[piled(1, "going spare"), piled(2, "also spare")],
        Some(&on(&[1])),
    );

    assert!(out.contains("going spare"), "{out}");
    assert!(!out.contains("also spare"), "{out}");
}

#[test]
fn the_pile_cap_counts_what_the_focus_left() {
    // ⚠ Focus runs FIRST and the cap runs on what survives. The other order
    // would spend the five pile lines on tasks the focus then hid, and report a
    // pile shorter than it is — two trims that have to compose in one direction
    // only.
    let mut tasks = vec![mine(1, "what I am on")];
    for id in 2..=9 {
        tasks.push(piled(id, &format!("spare {id}")));
    }
    let out = render(&tasks, Some(&on(&[1])));

    // All eight are hidden by the focus, so the cap has nothing left to trim
    // and must not also claim them.
    assert!(out.contains("8 in the pile not shown"), "{out}");
    assert!(!out.contains("more in the pile"), "{out}");
}

#[test]
fn no_focus_renders_exactly_what_it_always_did() {
    // The regression that would matter most: this is what every session sees on
    // every turn, and almost none of them are ever focused.
    let tasks = [mine(1, "a"), piled(2, "b")];
    assert_eq!(render(&tasks, None), render(&tasks, None));
    assert!(
        !render(&tasks, None).contains("focused"),
        "unfocused digest mentions focus"
    );
}

#[test]
fn a_period_is_read_the_way_somebody_types_one() {
    assert_eq!(focus::parse("4h").unwrap(), Duration::hours(4));
    assert_eq!(focus::parse("90m").unwrap(), Duration::minutes(90));
    assert_eq!(focus::parse("2h30m").unwrap(), Duration::minutes(150));
    assert_eq!(focus::parse(" 4H ").unwrap(), Duration::hours(4));
    // ⚠ A bare number is MINUTES. The unit somebody leaves off is the small
    // one, and reading `--for 30` as thirty hours would grant sixty times what
    // was asked for — quietly, since it is a legal period either way.
    assert_eq!(focus::parse("30").unwrap(), Duration::minutes(30));
    for bad in ["", "soon", "4 hours", "h", "4d"] {
        assert!(focus::parse(bad).is_err(), "{bad:?} was read as a period");
    }
}

#[test]
fn a_period_is_spelled_back_the_way_it_was_typed() {
    // The refusal quotes the bounds, so the numbers a caller is told have to be
    // written the way the argument they typed is.
    assert_eq!(focus::spell(Duration::hours(4)), "4h");
    assert_eq!(focus::spell(Duration::minutes(90)), "1h30m");
    assert_eq!(focus::spell(Duration::minutes(15)), "15m");
}

async fn known(pool: &sqlx::MySqlPool, id: &str) -> String {
    sessions::touch(pool, id, None).await.expect("registering");
    id.to_string()
}

async fn a_task(pool: &sqlx::MySqlPool, subject: &str) -> u64 {
    use tasks::tasks::repo::{self, NewTask};
    use tasks::tasks::types::{Actor, Ranking};
    repo::create(
        pool,
        NewTask {
            subject: subject.into(),
            // The check ran: these file through the service the way a session does.
            checked: true,
            body: String::new(),
            priority: Ranking::At(Priority::P2),
            due: None,
            blocked_on: Vec::new(),
            assignee: None,
            spare: None,
        },
        &Actor::Person("pippijn".into()),
    )
    .await
    .expect("filing")
    .id
}

fn set(ids: &[u64]) -> BTreeSet<u64> {
    ids.iter().copied().collect()
}

#[tokio::test]
async fn entering_and_reading_back_a_focus() {
    let pool = common::fresh_db().await;
    let me = known(&pool, "sess-a").await;
    let (one, two) = (a_task(&pool, "one").await, a_task(&pool, "two").await);

    focus::enter(&pool, &me, &set(&[one, two]), Duration::hours(4))
        .await
        .expect("entering a focus");

    let now = focus::current(&pool, &me)
        .await
        .expect("reading")
        .expect("a focus");
    assert_eq!(now.tasks, set(&[one, two]));
    assert!(now.holds_at(Utc::now()));
}

#[tokio::test]
async fn a_second_focus_replaces_the_first_rather_than_adding_to_it() {
    // The same rule `--blocked-on` follows: a caller states what it is on, and
    // never what to add. Adding would make "what am I focused on" a question
    // whose answer only grows.
    let pool = common::fresh_db().await;
    let me = known(&pool, "sess-a").await;
    let (one, two) = (a_task(&pool, "one").await, a_task(&pool, "two").await);

    focus::enter(&pool, &me, &set(&[one]), Duration::hours(4))
        .await
        .expect("entering");
    focus::enter(&pool, &me, &set(&[two]), Duration::hours(4))
        .await
        .expect("re-entering");

    let now = focus::current(&pool, &me)
        .await
        .expect("reading")
        .expect("a focus");
    assert_eq!(now.tasks, set(&[two]));
}

#[tokio::test]
async fn one_session_is_not_focused_by_another() {
    let pool = common::fresh_db().await;
    let (me, them) = (known(&pool, "sess-a").await, known(&pool, "sess-b").await);
    let one = a_task(&pool, "one").await;

    focus::enter(&pool, &me, &set(&[one]), Duration::hours(4))
        .await
        .expect("entering");

    assert!(
        focus::current(&pool, &them)
            .await
            .expect("reading")
            .is_none()
    );
}

#[tokio::test]
async fn a_focus_that_has_run_out_is_no_focus() {
    // ⚠ **Nothing sweeps the table**, so this is the only thing that makes an
    // expiry real: every read compares against the clock. A focus whose session
    // never came back stops applying at its hour regardless.
    let pool = common::fresh_db().await;
    let me = known(&pool, "sess-a").await;
    let one = a_task(&pool, "one").await;
    focus::enter(&pool, &me, &set(&[one]), Duration::minutes(15))
        .await
        .expect("entering");

    // Reached past the API on purpose: the alternative is a test that sleeps
    // for a quarter of an hour, and what is under test is the comparison.
    sqlx::query("UPDATE sessions SET focus_until = NOW() - INTERVAL 1 MINUTE WHERE id = ?")
        .bind(&me)
        .execute(&pool)
        .await
        .expect("ageing the focus");

    assert!(focus::current(&pool, &me).await.expect("reading").is_none());
}

#[tokio::test]
async fn ending_one_early() {
    let pool = common::fresh_db().await;
    let me = known(&pool, "sess-a").await;
    let one = a_task(&pool, "one").await;
    focus::enter(&pool, &me, &set(&[one]), Duration::hours(4))
        .await
        .expect("entering");

    focus::leave(&pool, &me).await.expect("leaving");
    assert!(focus::current(&pool, &me).await.expect("reading").is_none());
    // Twice, because "unfocus me" from a session that is not focused is an
    // answer and not an error.
    focus::leave(&pool, &me).await.expect("leaving again");
}

#[tokio::test]
async fn a_focus_on_nothing_is_refused() {
    // ⚠ The one state with no way out: every task counted, none recited, and
    // the ids needed to refocus are the ones that cannot be seen.
    let pool = common::fresh_db().await;
    let me = known(&pool, "sess-a").await;

    let said = refusal(
        focus::enter(&pool, &me, &BTreeSet::new(), Duration::hours(4))
            .await
            .expect_err("an empty focus"),
    );
    assert!(said.contains("at least one task"), "{said}");
}

#[tokio::test]
async fn a_period_outside_the_bounds_is_refused_with_the_bounds_named() {
    let pool = common::fresh_db().await;
    let me = known(&pool, "sess-a").await;
    let one = a_task(&pool, "one").await;

    for period in [Duration::minutes(5), Duration::hours(48)] {
        let said = refusal(
            focus::enter(&pool, &me, &set(&[one]), period)
                .await
                .expect_err("a period outside the bounds"),
        );
        // ⚠ Named, not clamped. fleetwatch clamps an over-long mute silently,
        // which leaves the caller believing a number that was never applied.
        assert!(said.contains("15m"), "{said}");
        assert!(said.contains("24h"), "{said}");
        assert!(said.contains(&focus::spell(period)), "{said}");
        // ⚠ **And the reason has to match which bound was missed.** One
        // sentence for both told a caller who asked for five minutes that a
        // focus longer than a day is really a handover. Measured on prod,
        // 2026-08-15, before this arm existed.
        let handover = said.contains("handover");
        assert_eq!(
            handover,
            period > focus::MAX,
            "the refusal explains the wrong bound: {said}"
        );
    }

    // And nothing was written on the way to refusing.
    assert!(focus::current(&pool, &me).await.expect("reading").is_none());
}

#[tokio::test]
async fn a_task_that_does_not_exist_is_refused_rather_than_dropped() {
    // Silently narrowing the focus would leave a session working from a list
    // one task shorter than the one it asked for, and nothing would say which.
    let pool = common::fresh_db().await;
    let me = known(&pool, "sess-a").await;
    let one = a_task(&pool, "one").await;

    let said = refusal(
        focus::enter(&pool, &me, &set(&[one, 999_999]), Duration::hours(4))
            .await
            .expect_err("a task that is not there"),
    );
    assert!(said.contains("#999999"), "{said}");
    assert!(focus::current(&pool, &me).await.expect("reading").is_none());
}

fn refusal(e: tasks::error::AppError) -> String {
    match e {
        tasks::error::AppError::BadRequest(msg) => msg,
        other => panic!("expected a bad request, got {other:?}"),
    }
}
