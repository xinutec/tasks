//! Refusing the write that is not an edit: a session takes `--json`'s
//! `detailed` boolean for the prose and writes `True` over the body.
//!
//! Half of these pin what is refused, half what is not: a guard that fires on
//! ordinary rewrites gets switched off. The key one is
//! [`putting_back_a_body_an_edit_had_grown`]: undo sends a shorter body, and
//! blocking it would block the only recovery.

mod common;

use tasks::tasks::repo::{self, Change, NewTask};
use tasks::tasks::types::{Actor, Priority, Ranking};

fn human() -> Actor {
    Actor::Person("user".into())
}

async fn file(pool: &sqlx::MySqlPool, body: &str) -> u64 {
    repo::create(
        pool,
        NewTask {
            subject: "a task with prose behind it".into(),
            checked: true,
            not_duplicate_of: Vec::new(),
            body: body.into(),
            priority: Ranking::At(Priority::P2),
            due: None,
            blocked_on: Vec::new(),
            assignee: None,
            spare: None,
        },
        &human(),
    )
    .await
    .expect("filing")
    .id
}

/// Longer than a body has to be before it is guarded at all.
fn prose(chars: usize) -> String {
    "word ".repeat(chars / 5)
}

fn body(text: &str) -> Change {
    Change {
        body: Some(text.into()),
        ..Change::default()
    }
}

fn refusal(e: tasks::error::AppError) -> String {
    match e {
        tasks::error::AppError::BadRequest(msg) => msg,
        other => panic!("expected a bad request, got {other:?}"),
    }
}

async fn stored(pool: &sqlx::MySqlPool, id: u64) -> String {
    repo::get(pool, id)
        .await
        .expect("reading")
        .expect("there")
        .body
}

#[tokio::test]
async fn a_long_body_may_not_be_replaced_by_almost_nothing() {
    let pool = common::fresh_db().await;
    let was = prose(3000);
    let id = file(&pool, &was).await;

    let said = refusal(
        repo::update(&pool, id, body("True"), &human())
            .await
            .expect_err("4 characters replaced 3,000"),
    );

    // Both sizes, because the number that makes the case is the one lost.
    assert!(said.contains("4 characters"), "{said}");
    assert!(said.contains("3000-character"), "{said}");
    // And the way through, or a refusal is just an obstacle.
    assert!(said.contains("--replace-body"), "{said}");
    assert!(said.contains("--body"), "{said}");
    assert_eq!(stored(&pool, id).await, was, "the body moved anyway");
}

#[tokio::test]
async fn the_refusal_counts_one_character_as_one() {
    let pool = common::fresh_db().await;
    let id = file(&pool, &prose(3000)).await;

    let said = refusal(
        repo::update(&pool, id, body("x"), &human())
            .await
            .expect_err("one character replaced 3,000"),
    );

    assert!(said.contains("1 character of"), "{said}");
}

#[tokio::test]
async fn a_refused_edit_leaves_no_trace() {
    let pool = common::fresh_db().await;
    let id = file(&pool, &prose(3000)).await;
    let before = repo::get(&pool, id).await.expect("reading").expect("there");

    repo::update(&pool, id, body(""), &human())
        .await
        .expect_err("a 3,000-character body was emptied");

    // ⚠ Refused inside the transaction, possibly after the subject was written: it
    // must roll back, or a rejected edit moves half the task and `restorable` offers
    // to undo a change nobody made.
    let after = repo::get(&pool, id).await.expect("reading").expect("there");
    assert_eq!(after.events.len(), before.events.len(), "history moved");
    assert!(!after.restorable, "a refused edit left a revision behind");
}

#[tokio::test]
async fn a_subject_beside_a_refused_body_does_not_survive_it() {
    let pool = common::fresh_db().await;
    let id = file(&pool, &prose(3000)).await;

    repo::update(
        &pool,
        id,
        Change {
            subject: Some("renamed on the way past".into()),
            body: Some("True".into()),
            ..Change::default()
        },
        &human(),
    )
    .await
    .expect_err("the body collapsed");

    let after = repo::get(&pool, id).await.expect("reading").expect("there");
    assert_eq!(after.task.subject, "a task with prose behind it");
}

#[tokio::test]
async fn saying_so_is_how_a_body_is_emptied_on_purpose() {
    let pool = common::fresh_db().await;
    let id = file(&pool, &prose(3000)).await;

    repo::update(
        &pool,
        id,
        Change {
            body: Some(String::new()),
            replace_body: true,
            ..Change::default()
        },
        &human(),
    )
    .await
    .expect("--replace-body was given and refused anyway");

    assert_eq!(stored(&pool, id).await, "");
}

#[tokio::test]
async fn putting_back_a_body_an_edit_had_grown() {
    let pool = common::fresh_db().await;
    let short = prose(600);
    let id = file(&pool, &short).await;

    // The edit that grows it. Nothing objects to this.
    repo::update(&pool, id, body(&prose(4000)), &human())
        .await
        .expect("growing a body");

    // ⚠ This undo looks exactly like the mistake (600 characters replacing 4,000);
    // it passes because whoever restores has just read what they restore.
    let was = repo::previous(&pool, id, &human())
        .await
        .expect("a revision")
        .expect("there");
    repo::update(
        &pool,
        id,
        Change {
            subject: Some(was.subject),
            body: Some(was.body),
            replace_body: true,
            ..Change::default()
        },
        &human(),
    )
    .await
    .expect("an undo was refused");

    assert_eq!(stored(&pool, id).await, short, "the undo did not land");
}

#[tokio::test]
async fn a_short_body_is_not_worth_guarding() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "a one-line reminder, and no more than that").await;

    // Every proportion the guard cares about, and none of it applies: below the
    // size where losing the text costs anything worth a question.
    repo::update(&pool, id, body("x"), &human())
        .await
        .expect("a short body was guarded");

    assert_eq!(stored(&pool, id).await, "x");
}

#[tokio::test]
async fn an_ordinary_rewrite_is_not_a_collapse() {
    let pool = common::fresh_db().await;
    let id = file(&pool, &prose(3000)).await;

    // Half of it, which is what putting the conclusion on top and cutting the
    // history under it actually looks like. The guard has to sit far enough out
    // that this never meets it.
    let now = prose(1500);
    repo::update(&pool, id, body(&now), &human())
        .await
        .expect("a genuine rewrite was refused");

    assert_eq!(stored(&pool, id).await, now);
}

#[tokio::test]
async fn the_history_says_how_much_of_a_body_moved() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "the body as first written").await;

    repo::update(&pool, id, body("shorter"), &human())
        .await
        .expect("editing");

    // ⚠ The reply goes with the writer's scrollback; this is the copy the NEXT
    // reader gets, or the history says `edited body` and nothing about a body
    // reduced to four characters.
    let detail = repo::get(&pool, id)
        .await
        .expect("reading")
        .expect("there")
        .events
        .into_iter()
        .find(|e| e.kind == "edited")
        .and_then(|e| e.detail)
        .expect("an edit with nothing said about it");
    assert_eq!(detail, "body 25 → 7 chars");
}
