//! What is standing in the tracker, counted against a real database.
//!
//! ⚠ **Six conditional aggregates in one statement, and every one of them can
//! be wrong in a way that looks right.** A `SUM(condition)` that never matches
//! reports 0, which is exactly what an empty tracker reports; a `COUNT(*)` over
//! a join to `task_blocks` reports edges as tasks; `<= 'P1'` on a string column
//! is doing lexicographic comparison and works only because `P0` and `P1` sort
//! before `P2`. None of those fail loudly. So each column gets a fixture that
//! makes it differ from the others.
//!
//! ⚠ **The macros are the point of half of this.** `still_open!` and `due_soon!`
//! are shared with the lists so a graph and a digest cannot disagree; a test that
//! spelled the conditions out again would pass while they drifted apart.

mod common;

use chrono::{Duration, Utc};
use sqlx::MySqlPool;
use tasks::tasks::repo::{self, Change, NewTask};
use tasks::tasks::types::{Actor, Assignee, AssigneeKind, Priority, Ranking, Status};
use tasks::tasks::work;

/// A session has to exist before it can hold anything — the service refuses an
/// unknown holder rather than inventing one, which is the same guard that stops
/// a typo silently parking work where nobody looks.
async fn seen(pool: &MySqlPool) {
    tasks::sessions::touch(pool, "s-1", Some("tasks"))
        .await
        .expect("recording a session");
}

async fn file(pool: &MySqlPool, subject: &str, assignee: Option<Assignee>) -> u64 {
    repo::create(
        pool,
        NewTask {
            subject: subject.into(),
            checked: true,
            body: String::new(),
            priority: Ranking::Unassessed,
            due: None,
            blocked_on: Vec::new(),
            assignee,
        },
        &Actor::Person("pippijn".into()),
    )
    .await
    .expect("filing")
    .id
}

fn held() -> Assignee {
    Assignee {
        kind: AssigneeKind::Session,
        id: Some("s-1".into()),
        name: None,
    }
}

#[tokio::test]
async fn an_empty_tracker_counts_zero_of_everything() {
    let pool = common::fresh_db().await;
    seen(&pool).await;
    let now = work::standing(&pool).await.expect("counting");
    assert_eq!(now.open, 0);
    assert_eq!(now.unheld, 0);
    assert_eq!(now.sprawling, 0);
}

#[tokio::test]
async fn a_closed_task_is_not_standing() {
    // `still_open!` — open and doing, and nothing else. A count that used
    // `status <> 'done'` would keep the dropped ones, which is the exact bug
    // `digest.rs` records: one edit, nothing fails, the numbers are just wrong.
    let pool = common::fresh_db().await;
    seen(&pool).await;
    let open = file(&pool, "still to do", Some(held())).await;
    let gone = file(&pool, "dropped", Some(held())).await;
    repo::update(
        &pool,
        gone,
        Change {
            status: Some(Status::Dropped),
            ..Default::default()
        },
        &Actor::Session("s-1".into()),
    )
    .await
    .expect("dropping");

    let now = work::standing(&pool).await.expect("counting");
    assert_eq!(
        now.open, 1,
        "a dropped task is closed and must not be counted"
    );
    let _ = open;
}

#[tokio::test]
async fn the_pile_is_counted_apart_from_the_held() {
    // Two numbers that move independently: a growing pile with a flat `open` is
    // work arriving and reaching nobody, and neither figure says that alone.
    let pool = common::fresh_db().await;
    seen(&pool).await;
    // ⚠ `None` here is NOT the pile: `repo::create` falls back to the FILING
    // actor, so an omitted holder means "mine". The pile has to be asked for.
    file(&pool, "mine", Some(held())).await;
    file(&pool, "spare", Some(Assignee::nobody())).await;
    file(&pool, "spare too", Some(Assignee::nobody())).await;

    let now = work::standing(&pool).await.expect("counting");
    assert_eq!(now.open, 3);
    assert_eq!(now.unheld, 2);
}

#[tokio::test]
async fn urgent_is_the_rank_the_list_sorts_by_not_the_one_somebody_typed() {
    // ⚠ The half a `priority IN ('P0','P1')` count would miss. A deadline inside
    // the week RAISES a task to P0 at read time and nothing is written, so a
    // count against the stored column disagrees with every list in the service.
    let pool = common::fresh_db().await;
    seen(&pool).await;
    let ranked = file(&pool, "somebody said P1", Some(held())).await;
    repo::update(
        &pool,
        ranked,
        Change {
            priority: Some(Priority::P1),
            ..Default::default()
        },
        &Actor::Session("s-1".into()),
    )
    .await
    .expect("ranking");

    let soon = file(&pool, "due in three days, unranked", Some(held())).await;
    repo::update(
        &pool,
        soon,
        Change {
            due: Some((Utc::now() + Duration::days(3)).date_naive()),
            ..Default::default()
        },
        &Actor::Session("s-1".into()),
    )
    .await
    .expect("dating");

    file(&pool, "ordinary", Some(held())).await;

    let now = work::standing(&pool).await.expect("counting");
    assert_eq!(
        now.urgent, 2,
        "the raised task was not counted, so this disagrees with the sort"
    );
}

#[tokio::test]
async fn a_task_is_counted_once_however_many_things_block_it() {
    // ⚠ **The join that would report edges as tasks.** `task_blocks` has a row
    // per edge, so counting over a join multiplies a task by its blockers — one
    // task waiting on two things would read as two blocked tasks.
    let pool = common::fresh_db().await;
    seen(&pool).await;
    let first = file(&pool, "blocker one", Some(held())).await;
    let second = file(&pool, "blocker two", Some(held())).await;
    let waiting = file(&pool, "waits on both", Some(held())).await;
    repo::update(
        &pool,
        waiting,
        Change {
            blocked_on: Some(vec![first, second]),
            ..Default::default()
        },
        &Actor::Session("s-1".into()),
    )
    .await
    .expect("blocking");

    let now = work::standing(&pool).await.expect("counting");
    assert_eq!(
        now.blocked, 1,
        "two edges were counted as two blocked tasks"
    );
    assert_eq!(now.open, 3, "the total must not multiply either");
}

#[tokio::test]
async fn a_blocker_that_closed_stops_blocking() {
    // The link is KEPT when a blocker closes — it is a record of how the work
    // went — so a count on the mere presence of `task_blocks` rows would report
    // work as stuck forever.
    let pool = common::fresh_db().await;
    seen(&pool).await;
    let blocker = file(&pool, "the blocker", Some(held())).await;
    let waiting = file(&pool, "waiting", Some(held())).await;
    repo::update(
        &pool,
        waiting,
        Change {
            blocked_on: Some(vec![blocker]),
            ..Default::default()
        },
        &Actor::Session("s-1".into()),
    )
    .await
    .expect("blocking");
    assert_eq!(work::standing(&pool).await.expect("counting").blocked, 1);

    repo::update(
        &pool,
        blocker,
        Change {
            status: Some(Status::Done),
            ..Default::default()
        },
        &Actor::Session("s-1".into()),
    )
    .await
    .expect("finishing");
    assert_eq!(
        work::standing(&pool).await.expect("counting").blocked,
        0,
        "a finished blocker still counted as blocking"
    );
}

#[tokio::test]
async fn the_sprawl_backlog_is_the_number_this_module_was_built_for() {
    // `0014` put a critique on the task and a mark in every holder's digest.
    // Whether that works is this count falling, and until now the only way to
    // ask was a hand-filtered `--all --json`.
    use tasks::tasks::checks::{self, Kind, Outcome, Run};
    let pool = common::fresh_db().await;
    seen(&pool).await;
    let id = file(&pool, "a body that grew", Some(held())).await;
    assert_eq!(work::standing(&pool).await.expect("counting").sprawling, 0);

    checks::record(
        &pool,
        "s-1",
        &Run {
            kind: Kind::Density,
            task_id: Some(id),
            input_chars: 18_162,
            accreted: Some(5_991),
            elapsed_ms: 33_735,
            outcome: Outcome::Spoke,
            subject_key: None,
            said: Some("the conclusion is at the bottom".into()),
        },
    )
    .await
    .expect("recording the read");

    assert_eq!(
        work::standing(&pool).await.expect("counting").sprawling,
        1,
        "a flagged body is not being counted, so nothing charts whether 0014 works"
    );
}
