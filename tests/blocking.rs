//! What blocks what, and the rule that carries.
//!
//! Pippijn, 2026-08-11: *"If something is blocked, it should get a ticket number
//! that it's blocked on. It can be the same, but not higher priority than the
//! thing it's blocked on."* And, separately: *"If A blocks B then B can't block
//! A."*
//!
//! ⚠ **The rule is what makes the link worth storing.** A dependency you can
//! only read is a note; one that constrains the rank is a check. Claiming *do
//! this next* about something you cannot start is the single move that inflates
//! a scale — everything downstream drifts up while the thing actually holding it
//! sits at `P3` — and it is the one shape a machine can catch.

mod common;

use tasks::tasks::repo::{self, Change, Filter, NewTask};
use tasks::tasks::types::{Actor, Priority, Ranking, Status};

fn pippijn() -> Actor {
    Actor::Person("pippijn".into())
}

fn filed(subject: &str, priority: Option<Priority>) -> NewTask {
    NewTask {
        subject: subject.into(),
        // The check ran: these file through the service the way a session does.
        checked: true,
        body: String::new(),
        priority: priority.map_or(Ranking::Unassessed, Ranking::At),
        due: None,
        blocked_on: Vec::new(),
        assignee: None,
    }
}

async fn file(pool: &sqlx::MySqlPool, subject: &str, priority: Option<Priority>) -> u64 {
    repo::create(pool, filed(subject, priority), &pippijn())
        .await
        .expect("filing")
        .id
}

async fn block(
    pool: &sqlx::MySqlPool,
    id: u64,
    on: &[u64],
) -> std::result::Result<(), tasks::error::AppError> {
    repo::update(
        pool,
        id,
        Change {
            blocked_on: Some(on.to_vec()),
            ..Default::default()
        },
        &pippijn(),
    )
    .await
    .map(|_| ())
}

async fn rank(
    pool: &sqlx::MySqlPool,
    id: u64,
    p: Priority,
) -> std::result::Result<(), tasks::error::AppError> {
    repo::update(
        pool,
        id,
        Change {
            priority: Some(p),
            ..Default::default()
        },
        &pippijn(),
    )
    .await
    .map(|_| ())
}

fn refusal(e: tasks::error::AppError) -> String {
    match e {
        tasks::error::AppError::BadRequest(msg) => msg,
        other => panic!("expected a bad request, got {other:?}"),
    }
}

#[tokio::test]
async fn a_task_can_wait_on_more_than_one_thing() {
    // ⚠ The first cut of this feature was ONE column, chosen because no open
    // task named two blockers. That measured the absence of the feature rather
    // than the shape of the work — there was nowhere to record even one — and
    // with a single slot the workaround for a second is prose, which is the
    // staleness the table replaced. Pippijn caught it before it shipped.
    let pool = common::fresh_db().await;
    let one = file(&pool, "the first thing", None).await;
    let two = file(&pool, "the second thing", None).await;
    let waiting = file(&pool, "needs both", None).await;

    block(&pool, waiting, &[one, two]).await.expect("blocking");
    let task = repo::list(&pool, &Filter::default())
        .await
        .expect("listing")
        .into_iter()
        .find(|t| t.id == waiting)
        .expect("the task");
    assert_eq!(task.blocked_on, vec![one, two]);
    assert!(task.blocked, "it waits on two open tasks");
}

#[tokio::test]
async fn a_blocked_task_may_not_outrank_what_blocks_it() {
    let pool = common::fresh_db().await;
    let blocker = file(&pool, "do this first", Some(Priority::P3)).await;
    let waiting = file(&pool, "waits for it", None).await;
    block(&pool, waiting, &[blocker]).await.expect("blocking");

    let msg = refusal(
        rank(&pool, waiting, Priority::P1)
            .await
            .expect_err("a blocked task was ranked above its blocker"),
    );
    assert!(msg.contains(&format!("#{blocker}")), "{msg}");
    // Equal is allowed — that is the half of the rule that is easy to lose.
    rank(&pool, waiting, Priority::P3)
        .await
        .expect("equal ranks are allowed");
}

/// The end that would otherwise slip through.
///
/// Ranking the blocked task up is the obvious violation. Ranking the BLOCKER
/// down leaves the same inconsistency by the other door, and nothing about the
/// edit looks wrong at the time.
#[tokio::test]
async fn a_blocker_may_not_be_demoted_below_what_waits_for_it() {
    let pool = common::fresh_db().await;
    let blocker = file(&pool, "do this first", Some(Priority::P1)).await;
    let waiting = file(&pool, "waits for it", Some(Priority::P1)).await;
    block(&pool, waiting, &[blocker]).await.expect("blocking");

    let msg = refusal(
        rank(&pool, blocker, Priority::P3)
            .await
            .expect_err("a blocker was demoted below the task waiting for it"),
    );
    assert!(msg.contains(&format!("#{waiting}")), "{msg}");
}

/// With several blockers the bound is the LEAST urgent open one.
///
/// That is the one that decides when the work can actually start: waiting on a
/// `P1` and a `P3`, you are waiting for the `P3`.
#[tokio::test]
async fn several_blockers_bind_at_the_least_urgent_of_them() {
    let pool = common::fresh_db().await;
    let urgent = file(&pool, "soon", Some(Priority::P1)).await;
    let later = file(&pool, "not soon", Some(Priority::P4)).await;
    let waiting = file(&pool, "waits for both", None).await;
    block(&pool, waiting, &[urgent, later])
        .await
        .expect("blocking");

    rank(&pool, waiting, Priority::P2)
        .await
        .expect_err("P2 beat a P4 blocker");
    rank(&pool, waiting, Priority::P4)
        .await
        .expect("the least urgent blocker is the bound");
}

#[tokio::test]
async fn a_closed_blocker_stops_constraining_anything() {
    // A finished dependency must not hold a rank down for ever. The link stays —
    // it is a fact about how the work went — but its effect ends.
    let pool = common::fresh_db().await;
    let blocker = file(&pool, "was in the way", Some(Priority::P4)).await;
    let waiting = file(&pool, "waited for it", None).await;
    block(&pool, waiting, &[blocker]).await.expect("blocking");
    rank(&pool, waiting, Priority::P0)
        .await
        .expect_err("an open P4 blocker allowed a P0");

    repo::update(
        &pool,
        blocker,
        Change {
            status: Some(Status::Done),
            ..Default::default()
        },
        &pippijn(),
    )
    .await
    .expect("finishing the blocker");

    rank(&pool, waiting, Priority::P0)
        .await
        .expect("a closed blocker still constrained the task");
    let task = repo::get(&pool, waiting)
        .await
        .expect("reading")
        .expect("a task");
    assert_eq!(
        task.task.blocked_on,
        vec![blocker],
        "the link was discarded"
    );
    assert!(
        !task.task.blocked,
        "a closed blocker still reads as blocking"
    );
}

#[tokio::test]
async fn nothing_may_block_itself() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "alone", None).await;
    refusal(
        block(&pool, id, &[id])
            .await
            .expect_err("a task was blocked on itself"),
    );
}

#[tokio::test]
async fn if_a_blocks_b_then_b_may_not_block_a() {
    // Pippijn's words, 2026-08-11. The two-task case is the one anybody thinks
    // of; the test below is the one that actually needs the graph walk.
    let pool = common::fresh_db().await;
    let a = file(&pool, "A", None).await;
    let b = file(&pool, "B", None).await;
    block(&pool, b, &[a]).await.expect("B waits for A");

    let msg = refusal(
        block(&pool, a, &[b])
            .await
            .expect_err("A and B were made to wait for each other"),
    );
    assert!(msg.contains("loop"), "{msg}");
}

/// ⚠ **The case a one-step check misses.**
///
/// `A → B → C → A` arrives as three separate edits, each of which looks fine on
/// its own: nothing in the third edit mentions A. Only a walk over the whole
/// graph refuses it, which is why `no_cycle` is breadth-first rather than a
/// single lookup.
#[tokio::test]
async fn a_longer_loop_is_refused_too() {
    let pool = common::fresh_db().await;
    let a = file(&pool, "A", None).await;
    let b = file(&pool, "B", None).await;
    let c = file(&pool, "C", None).await;
    block(&pool, a, &[b]).await.expect("A waits for B");
    block(&pool, b, &[c]).await.expect("B waits for C");

    refusal(
        block(&pool, c, &[a])
            .await
            .expect_err("a three-task loop was accepted"),
    );
}

#[tokio::test]
async fn a_blocker_that_does_not_exist_is_named_rather_than_a_500() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "waits for a ghost", None).await;
    let msg = refusal(
        block(&pool, id, &[999_999])
            .await
            .expect_err("a nonexistent blocker was accepted"),
    );
    assert!(msg.contains("999999"), "{msg}");
}

#[tokio::test]
async fn an_empty_list_is_how_a_task_stops_being_blocked() {
    // And is why there is no `--unblock` flag: `[]` is a value, not an absence.
    let pool = common::fresh_db().await;
    let blocker = file(&pool, "in the way", None).await;
    let waiting = file(&pool, "waiting", None).await;
    block(&pool, waiting, &[blocker]).await.expect("blocking");
    block(&pool, waiting, &[]).await.expect("unblocking");

    let task = repo::get(&pool, waiting)
        .await
        .expect("reading")
        .expect("a task");
    assert!(task.task.blocked_on.is_empty());
    assert!(!task.task.blocked);
    // Both moves are in the history: it is a change to the plan, not a detail.
    let moves: Vec<&str> = task
        .events
        .iter()
        .filter(|e| e.kind == "blocked")
        .filter_map(|e| e.detail.as_deref())
        .collect();
    assert_eq!(
        moves,
        vec![
            format!("nothing → #{blocker}").as_str(),
            format!("#{blocker} → nothing").as_str()
        ]
    );
}

#[tokio::test]
async fn re_stating_the_same_blockers_writes_no_history() {
    let pool = common::fresh_db().await;
    let blocker = file(&pool, "in the way", None).await;
    let waiting = file(&pool, "waiting", None).await;
    block(&pool, waiting, &[blocker]).await.expect("blocking");

    let again = repo::update(
        &pool,
        waiting,
        Change {
            blocked_on: Some(vec![blocker]),
            ..Default::default()
        },
        &pippijn(),
    )
    .await
    .expect("re-stating");
    assert!(again.changed.is_empty(), "{:?}", again.changed);
}

/// ⚠ **A rank and a block that are legal together must be accepted together.**
///
/// Checking as each field is written would refuse this: at the moment the rank
/// lands, the task has no blockers and the check passes; at the moment the block
/// lands, the rank is already `P1` and the blocker is `P1` — also fine. But an
/// implementation that checked after the FIRST write against the OLD value of
/// the second would reject a change that is consistent when it commits.
#[tokio::test]
async fn ranking_and_blocking_in_one_change_is_judged_on_the_result() {
    let pool = common::fresh_db().await;
    let blocker = file(&pool, "first", Some(Priority::P1)).await;
    let waiting = file(&pool, "second", Some(Priority::P4)).await;

    repo::update(
        &pool,
        waiting,
        Change {
            priority: Some(Priority::P1),
            blocked_on: Some(vec![blocker]),
            ..Default::default()
        },
        &pippijn(),
    )
    .await
    .expect("a consistent pair was refused");
}

/// The deadline twin of the rank rule.
///
/// ⚠ **This one is arithmetic, not judgement.** A task cannot be finished before
/// the thing it is waiting for, so a due date earlier than an open blocker's is
/// wrong however anybody feels about it — no threshold, no "soon", nothing to
/// calibrate. Equal is allowed: both landing on the same day is tight, not
/// impossible.
mod deadlines {
    use super::*;
    use chrono::{Duration, NaiveDate, Utc};

    /// ⚠ **Relative, because a hardcoded future date is a test with an expiry
    /// date.** As first written these were literals — `2026-09-01` for "not
    /// overdue", `2026-08-12` for "has a deadline" — both true the afternoon
    /// they were written, both false within a month, and the second one broke
    /// the same day, when a deadline inside a week started raising the rank.
    fn day(days: i64) -> NaiveDate {
        (Utc::now() + Duration::days(days)).date_naive()
    }

    async fn due(
        pool: &sqlx::MySqlPool,
        id: u64,
        days: i64,
    ) -> std::result::Result<(), tasks::error::AppError> {
        repo::update(
            pool,
            id,
            Change {
                due: Some(day(days)),
                ..Default::default()
            },
            &pippijn(),
        )
        .await
        .map(|_| ())
    }

    #[tokio::test]
    async fn a_task_may_not_be_due_before_what_blocks_it() {
        let pool = common::fresh_db().await;
        let blocker = file(&pool, "must happen first", None).await;
        let waiting = file(&pool, "cannot precede it", None).await;
        block(&pool, waiting, &[blocker]).await.expect("blocking");
        due(&pool, blocker, 60).await.expect("the blocker");

        let msg = refusal(
            due(&pool, waiting, 30)
                .await
                .expect_err("a task was due before its blocker"),
        );
        assert!(msg.contains(&format!("#{blocker}")), "{msg}");
        due(&pool, waiting, 60)
            .await
            .expect("the same day is allowed");
    }

    #[tokio::test]
    async fn a_blocker_may_not_be_pushed_past_what_waits_for_it() {
        // The other door, and the one nothing about the edit looks wrong at.
        let pool = common::fresh_db().await;
        let blocker = file(&pool, "must happen first", None).await;
        let waiting = file(&pool, "waits for it", None).await;
        block(&pool, waiting, &[blocker]).await.expect("blocking");
        due(&pool, blocker, 30).await.expect("the blocker");
        due(&pool, waiting, 40).await.expect("the dependent");

        let msg = refusal(
            due(&pool, blocker, 90)
                .await
                .expect_err("a blocker was pushed past its dependent"),
        );
        assert!(msg.contains(&format!("#{waiting}")), "{msg}");
    }

    #[tokio::test]
    async fn a_deadline_is_recorded_and_can_be_taken_off_again() {
        let pool = common::fresh_db().await;
        let id = file(&pool, "has a date", None).await;
        due(&pool, id, 60).await.expect("setting");

        let task = repo::get(&pool, id).await.expect("read").expect("a task");
        assert_eq!(task.task.due, Some(day(60)));
        assert!(!task.task.overdue, "a date two months out is not overdue");

        repo::update(
            &pool,
            id,
            Change {
                clear_due: true,
                ..Default::default()
            },
            &pippijn(),
        )
        .await
        .expect("clearing");
        let task = repo::get(&pool, id).await.expect("read").expect("a task");
        assert_eq!(task.task.due, None);
        let moves: Vec<&str> = task
            .events
            .iter()
            .filter(|e| e.kind == "due")
            .filter_map(|e| e.detail.as_deref())
            .collect();
        let d = day(60);
        assert_eq!(moves, vec![format!("none → {d}"), format!("{d} → none")]);
    }

    #[tokio::test]
    async fn a_day_that_has_passed_reads_as_overdue() {
        // Decided by the DATABASE's clock, not the caller's — one clock, so the
        // CLI, the app and the digest cannot disagree about which day it is.
        let pool = common::fresh_db().await;
        let id = file(&pool, "late", None).await;
        due(&pool, id, -2000).await.expect("setting");
        let task = repo::get(&pool, id).await.expect("read").expect("a task");
        assert!(task.task.overdue, "a date years back is not overdue?");
    }

    #[tokio::test]
    async fn a_closed_blocker_stops_constraining_the_date_too() {
        let pool = common::fresh_db().await;
        let blocker = file(&pool, "was first", None).await;
        let waiting = file(&pool, "waited", None).await;
        block(&pool, waiting, &[blocker]).await.expect("blocking");
        due(&pool, blocker, 120).await.expect("the blocker");
        due(&pool, waiting, 30)
            .await
            .expect_err("an open blocker allowed an earlier date");

        repo::update(
            &pool,
            blocker,
            Change {
                status: Some(Status::Done),
                ..Default::default()
            },
            &pippijn(),
        )
        .await
        .expect("finishing the blocker");
        due(&pool, waiting, 30)
            .await
            .expect("a closed blocker still held the date back");
    }

    /// ⚠ **A FAR deadline must not reorder anything** — which is a narrower
    /// claim than this test started with.
    ///
    /// As first written it said *a deadline* must not reorder, with a fixture
    /// due tomorrow, and it was correct until Pippijn added the rule that a
    /// deadline inside a week raises the rank (`mod escalation`). What survives
    /// is the half still worth defending: a date far enough out is evidence for
    /// a rank rather than a substitute for one, and leaves the order alone. The
    /// near half is now a stated rule rather than arithmetic overriding a
    /// decision, which is the whole difference.
    #[tokio::test]
    async fn a_far_deadline_does_not_move_a_task_up_the_list() {
        let pool = common::fresh_db().await;
        let urgent = file(&pool, "ranked P1, no date", Some(Priority::P1)).await;
        let dated = file(&pool, "ranked P4, due in three months", Some(Priority::P4)).await;
        due(&pool, dated, 90).await.expect("setting");

        let order: Vec<u64> = repo::list(&pool, &Filter::default())
            .await
            .expect("listing")
            .into_iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(
            order,
            vec![urgent, dated],
            "a deadline silently overrode a ranking decision"
        );
    }
}

/// A deadline inside the week raises the rank — Pippijn, 2026-08-11.
///
/// ⚠ **This is the one thing a deadline is allowed to reorder**, and it does not
/// contradict `a_deadline_does_not_move_a_task_up_the_list` above: that pins
/// that a FAR date changes nothing. The earlier refusal was about arithmetic
/// overriding a human decision; a rule Pippijn states IS the decision, and it is
/// also the case where `P0`'s own test ("every hour it stays open costs more")
/// starts being true, since the hours are what is being spent.
mod escalation {
    use super::*;
    use chrono::{Duration, Utc};

    async fn due_in(
        pool: &sqlx::MySqlPool,
        id: u64,
        days: i64,
    ) -> std::result::Result<(), tasks::error::AppError> {
        let day = (Utc::now() + Duration::days(days)).date_naive();
        repo::update(
            pool,
            id,
            Change {
                due: Some(day),
                ..Default::default()
            },
            &pippijn(),
        )
        .await
        .map(|_| ())
    }

    async fn one(pool: &sqlx::MySqlPool, id: u64) -> tasks::tasks::types::Task {
        repo::list(pool, &Filter::default())
            .await
            .expect("listing")
            .into_iter()
            .find(|t| t.id == id)
            .expect("the task")
    }

    #[tokio::test]
    async fn inside_the_week_a_task_sorts_as_p0_whatever_it_was_set_to() {
        let pool = common::fresh_db().await;
        let id = file(&pool, "due in three days", Some(Priority::P4)).await;
        due_in(&pool, id, 3).await.expect("setting");

        let task = one(&pool, id).await;
        assert_eq!(task.escalated_to, Some(Priority::P0));
        // ⚠ The STORED rank is untouched. Nothing writes P0 into the row: a job
        // that did would edit history nobody asked for and need a scheduler to
        // be right. This is recomputed from the date every time it is read.
        assert_eq!(task.priority, Some(Priority::P4), "the stored rank moved");
    }

    /// ⚠ **The boundary, because "less than a week" has two readings.**
    #[tokio::test]
    async fn exactly_a_week_out_is_not_yet_inside_it() {
        let pool = common::fresh_db().await;
        let seven = file(&pool, "due in seven days", Some(Priority::P2)).await;
        let six = file(&pool, "due in six days", Some(Priority::P2)).await;
        due_in(&pool, seven, 7).await.expect("setting");
        due_in(&pool, six, 6).await.expect("setting");

        assert_eq!(one(&pool, seven).await.escalated_to, None);
        assert_eq!(one(&pool, six).await.escalated_to, Some(Priority::P0));
    }

    #[tokio::test]
    async fn a_task_already_p0_is_not_reported_as_raised() {
        // Nothing was raised, and saying otherwise would invite a client to draw
        // a difference that does not exist.
        let pool = common::fresh_db().await;
        let id = file(&pool, "already urgent", Some(Priority::P0)).await;
        due_in(&pool, id, 1).await.expect("setting");
        assert_eq!(one(&pool, id).await.escalated_to, None);
    }

    #[tokio::test]
    async fn an_overdue_task_is_raised_too() {
        let pool = common::fresh_db().await;
        let id = file(&pool, "late", Some(Priority::P3)).await;
        due_in(&pool, id, -5).await.expect("setting");
        let task = one(&pool, id).await;
        assert_eq!(task.escalated_to, Some(Priority::P0));
        assert!(task.overdue);
    }

    #[tokio::test]
    async fn a_task_with_no_deadline_is_never_raised() {
        let pool = common::fresh_db().await;
        let id = file(&pool, "no date at all", Some(Priority::P4)).await;
        assert_eq!(one(&pool, id).await.escalated_to, None);
    }

    /// The raise has to reach the ORDER BY, not only the projection.
    ///
    /// A version that reported `escalated_to` and sorted on the stored rank
    /// would draw a `P0!` marker at the bottom of the list, which is worse than
    /// not marking it at all: the reader would believe the order.
    #[tokio::test]
    async fn the_raise_is_what_the_list_is_sorted_by() {
        let pool = common::fresh_db().await;
        let ordinary = file(&pool, "filed first, ranked P1", Some(Priority::P1)).await;
        let soon = file(
            &pool,
            "filed second, P4 but due in two days",
            Some(Priority::P4),
        )
        .await;
        due_in(&pool, soon, 2).await.expect("setting");

        let order: Vec<u64> = repo::list(&pool, &Filter::default())
            .await
            .expect("listing")
            .into_iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(
            order,
            vec![soon, ordinary],
            "the raised task did not reach the top"
        );
    }
}
