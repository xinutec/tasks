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
use tasks::tasks::types::{Actor, Priority, Status};

fn pippijn() -> Actor {
    Actor::Person("pippijn".into())
}

fn filed(subject: &str, priority: Option<Priority>) -> NewTask {
    NewTask {
        subject: subject.into(),
        body: String::new(),
        priority,
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
