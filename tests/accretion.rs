//! How much a body has grown since anything last made it smaller.
//!
//! ⚠ **The unit is characters since the last consolidation, and both halves of
//! that matter.** A count of edits cannot tell three typo fixes from three
//! two-thousand-character dumps, and an absolute size cannot tell a long body
//! somebody has just rewritten from a short one that has doubled since anyone
//! read it. Measured 2026-08-23 over nine days of history: 667 body-changing
//! edits, of which 459 were exact appends or prepends adding 791,400
//! characters, against 183 rewrites removing 26,593. #982 ran 42 consecutive
//! growing edits, 2,795 → 100,382 characters, without once being consolidated.
//!
//! What this number feeds is advice, never a refusal — see `density.rs`. These
//! tests pin the arithmetic, which is the half that has to be exact.

mod common;

use tasks::tasks::repo::{self, Change, NewTask};
use tasks::tasks::types::{Actor, Priority, Ranking};

fn pippijn() -> Actor {
    Actor::Person("pippijn".into())
}

async fn file(pool: &sqlx::MySqlPool, body: &str) -> u64 {
    repo::create(
        pool,
        NewTask {
            subject: "a task with a body".into(),
            // The check ran: these file through the service the way a session does.
            checked: true,
            body: body.into(),
            priority: Ranking::At(Priority::P2),
            due: None,
            blocked_on: Vec::new(),
            assignee: None,
        },
        &pippijn(),
    )
    .await
    .expect("filing")
    .id
}

/// A body of exactly `n` characters, so every assertion below is arithmetic
/// rather than a count of what happens to have been typed.
fn text(n: usize) -> String {
    "x".repeat(n)
}

/// The growth this edit reported, which is absent unless it moved text.
async fn edit_to(pool: &sqlx::MySqlPool, id: u64, body: &str) -> usize {
    repo::update(
        pool,
        id,
        Change {
            body: Some(body.into()),
            replace_body: true,
            ..Change::default()
        },
        &pippijn(),
    )
    .await
    .expect("editing")
    .replaced
    .expect("an edit that moved text says what it landed on")
    .accreted
}

#[tokio::test]
async fn growth_is_counted_from_the_body_that_was_filed() {
    let pool = common::fresh_db().await;
    let id = file(&pool, &text(100)).await;

    assert_eq!(edit_to(&pool, id, &text(300)).await, 200);
    assert_eq!(edit_to(&pool, id, &text(700)).await, 600);
}

#[tokio::test]
async fn a_shrink_reports_nothing_accreted() {
    let pool = common::fresh_db().await;
    let id = file(&pool, &text(100)).await;
    edit_to(&pool, id, &text(3000)).await;

    // The consolidation itself is the answer to the warning, so the moment it
    // lands there is nothing outstanding to warn about.
    assert_eq!(edit_to(&pool, id, &text(500)).await, 0);
}

#[tokio::test]
async fn what_a_rewrite_swept_up_is_not_counted_again() {
    let pool = common::fresh_db().await;
    let id = file(&pool, &text(100)).await;
    edit_to(&pool, id, &text(5000)).await;
    edit_to(&pool, id, &text(800)).await;

    // 4,900 characters of accretion were consolidated away. Counting them a
    // second time would warn about a body somebody has just rewritten, which is
    // the one state this must be silent in.
    assert_eq!(edit_to(&pool, id, &text(1000)).await, 200);
}

#[tokio::test]
async fn an_edit_that_left_the_body_alone_does_not_end_a_run() {
    let pool = common::fresh_db().await;
    let id = file(&pool, &text(100)).await;
    edit_to(&pool, id, &text(1000)).await;

    // A subject edit stores a revision like any other update, and its body is
    // unchanged rather than smaller. Reading that as a consolidation would let
    // any run be reset by renaming the task.
    repo::update(
        &pool,
        id,
        Change {
            subject: Some("renamed, and not consolidated".into()),
            ..Change::default()
        },
        &pippijn(),
    )
    .await
    .expect("renaming");

    assert_eq!(edit_to(&pool, id, &text(1600)).await, 1500);
}

#[tokio::test]
async fn each_task_is_counted_on_its_own() {
    let pool = common::fresh_db().await;
    let one = file(&pool, &text(100)).await;
    let two = file(&pool, &text(100)).await;
    edit_to(&pool, one, &text(4000)).await;

    // The query is keyed on the task, and a fleet-wide `ORDER BY event_id` with
    // the `WHERE` left off would read the neighbour's growth as this one's.
    assert_eq!(edit_to(&pool, two, &text(300)).await, 200);
}
