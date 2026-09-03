//! Refusing the write that is not an edit.
//!
//! ⚠ **These exist because it happened.** 2026-08-15: a session wanted #900's
//! prose, read `--json`, and took `detailed` — the BOOLEAN that says whether
//! prose exists — for the prose itself. It then wrote the string `True` over
//! 3,109 characters. `task undo` got it back, which is the point of the
//! revision store, but nothing had tried to stop the write.
//!
//! The property under test is that a body which keeps almost nothing of the one
//! it replaces is refused unless somebody says they mean it. Half of these pin
//! what is refused; the other half pin what is NOT, because a guard that fires
//! on ordinary rewrites is one everybody learns to switch off. The most
//! important of those is [`putting_back_a_body_an_edit_had_grown`]: undo sends
//! a shorter body than the one it replaces, so a guard that cannot tell it
//! apart would block the only recovery there is.

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
            subject: "a task with prose behind it".into(),
            // The check ran: these file through the service the way a session does.
            checked: true,
            body: body.into(),
            priority: Ranking::At(Priority::P2),
            due: None,
            blocked_on: Vec::new(),
            assignee: None,
            spare: None,
        },
        &pippijn(),
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
        repo::update(&pool, id, body("True"), &pippijn())
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
        repo::update(&pool, id, body("x"), &pippijn())
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

    repo::update(&pool, id, body(""), &pippijn())
        .await
        .expect_err("a 3,000-character body was emptied");

    // ⚠ The refusal happens inside the transaction, after the row has been read
    // and possibly after a subject has been written. If that did not roll back,
    // a rejected edit would still move half the task and leave a history row
    // saying so — and `restorable` would offer to undo a change nobody made.
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
        &pippijn(),
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
        &pippijn(),
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
    repo::update(&pool, id, body(&prose(4000)), &pippijn())
        .await
        .expect("growing a body");

    // ⚠ **The undo now looks exactly like the mistake**: 600 characters
    // replacing 4,000, well under the share the guard wants kept. It is let
    // through because whoever restores has just read what they are restoring,
    // which is the one thing the collapsing write never did.
    let was = repo::previous(&pool, id, &pippijn())
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
        &pippijn(),
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
    repo::update(&pool, id, body("x"), &pippijn())
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
    repo::update(&pool, id, body(&now), &pippijn())
        .await
        .expect("a genuine rewrite was refused");

    assert_eq!(stored(&pool, id).await, now);
}

#[tokio::test]
async fn the_history_says_how_much_of_a_body_moved() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "the body as first written").await;

    repo::update(&pool, id, body("shorter"), &pippijn())
        .await
        .expect("editing");

    // ⚠ The reply to the writer already carried these numbers and then went
    // with their scrollback. This is the copy the NEXT reader gets — without
    // it, #900's history said `edited body` and gave no sign that a body had
    // been reduced to four characters.
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
