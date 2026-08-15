//! What an edit replaced, and getting it back.
//!
//! ⚠ **These exist because the loss happened.** 2026-08-14: a session rewrote
//! #25's body from a snapshot it had read three days earlier and never
//! re-read. `task_events` said the body had changed and could not say to what —
//! `detail` is one rendered line — so the only copy of the replaced text was in
//! the writer's own transcript. It was recovered by grep. That is luck.
//!
//! The property under test is that a task has a *previous version*: a complete
//! subject and body as they stood, restorable as a unit. Half of these pin the
//! shape of what is stored, and half pin what is NOT stored — a change that
//! moves no text must leave no revision, or `task undo` would put back a
//! version nobody replaced.

mod common;

use tasks::tasks::repo::{self, Change, NewTask};
use tasks::tasks::types::{Actor, Priority, Ranking, Status};

fn pippijn() -> Actor {
    Actor::Person("pippijn".into())
}

fn other() -> Actor {
    Actor::Person("someone-else".into())
}

async fn file(pool: &sqlx::MySqlPool, subject: &str, body: &str) -> u64 {
    repo::create(
        pool,
        NewTask {
            subject: subject.into(),
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

async fn edit(pool: &sqlx::MySqlPool, id: u64, change: Change) -> tasks::tasks::types::Updated {
    repo::update(pool, id, change, &other())
        .await
        .expect("editing")
}

fn body(text: &str) -> Change {
    Change {
        body: Some(text.into()),
        ..Change::default()
    }
}

#[tokio::test]
async fn a_task_nothing_has_overwritten_has_no_previous_version() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "as filed", "the first body").await;
    assert!(
        repo::previous(&pool, id, &pippijn())
            .await
            .expect("reading")
            .is_none(),
        "filing a task is not overwriting one"
    );
}

#[tokio::test]
async fn an_edited_body_leaves_the_one_it_replaced_behind() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "as filed", "the first body").await;
    edit(&pool, id, body("something else entirely")).await;

    let was = repo::previous(&pool, id, &pippijn())
        .await
        .expect("reading")
        .expect("an edit leaves a previous version");
    assert_eq!(was.body, "the first body");
    assert_eq!(was.subject, "as filed");
    assert_eq!(
        was.actor, "someone-else",
        "who displaced it, not who wrote it"
    );
}

#[tokio::test]
async fn a_subject_edit_snapshots_the_body_it_did_not_touch() {
    // ⚠ **The property that makes a revision restorable as a unit.** Storing
    // only the column that moved would leave `task undo` after a subject-only
    // edit with no body to put back — and putting back the body from some
    // earlier revision would restore a subject and a body that never coexisted.
    let pool = common::fresh_db().await;
    let id = file(&pool, "as filed", "the first body").await;
    edit(
        &pool,
        id,
        Change {
            subject: Some("renamed, body untouched".into()),
            ..Change::default()
        },
    )
    .await;

    let was = repo::previous(&pool, id, &pippijn())
        .await
        .expect("reading")
        .expect("a revision");
    assert_eq!(was.subject, "as filed");
    assert_eq!(
        was.body, "the first body",
        "the untouched half is stored too"
    );
}

#[tokio::test]
async fn one_update_changing_both_leaves_exactly_one_revision() {
    // A `task edit --subject --body` writes two `edited` events. Two revisions
    // would make the first `task undo` restore half a task and leave the other
    // half waiting for a second undo.
    let pool = common::fresh_db().await;
    let id = file(&pool, "as filed", "the first body").await;
    edit(
        &pool,
        id,
        Change {
            subject: Some("both moved".into()),
            body: Some("both moved, body too".into()),
            ..Change::default()
        },
    )
    .await;

    let kept: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM task_revision r JOIN task_events e ON e.id = r.event_id \
         WHERE e.task_id = ?",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("counting revisions");
    assert_eq!(kept.0, 1, "one update, one previous version");

    let was = repo::previous(&pool, id, &pippijn())
        .await
        .expect("reading")
        .expect("a revision");
    assert_eq!(
        (was.subject.as_str(), was.body.as_str()),
        ("as filed", "the first body")
    );
}

#[tokio::test]
async fn a_change_that_moves_no_text_leaves_no_revision() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "as filed", "the first body").await;
    for change in [
        Change {
            status: Some(Status::Doing),
            ..Change::default()
        },
        Change {
            priority: Some(Priority::P0),
            ..Change::default()
        },
        // The same body again: `update` compares before writing, so this is not
        // an edit and must not become a revision either.
        body("the first body"),
    ] {
        edit(&pool, id, change).await;
    }
    assert!(
        repo::previous(&pool, id, &pippijn())
            .await
            .expect("reading")
            .is_none(),
        "ranking, starting and re-saving identical prose overwrite nothing"
    );
}

#[tokio::test]
async fn the_newest_revision_is_the_one_that_comes_back() {
    // ⚠ **Pinned against `at`, not just against insertion order.** `task_events.at`
    // is a DATETIME at one-second resolution and a subject sweep writes several
    // edits inside one second, so ordering by time picks an arbitrary one of
    // them. Both events are forced to the same timestamp here to make that the
    // case under test rather than something the clock decides.
    let pool = common::fresh_db().await;
    let id = file(&pool, "as filed", "first").await;
    edit(&pool, id, body("second")).await;
    edit(&pool, id, body("third")).await;
    sqlx::query("UPDATE task_events SET at = '2026-01-01 00:00:00' WHERE task_id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .expect("flattening the timestamps");

    let was = repo::previous(&pool, id, &pippijn())
        .await
        .expect("reading")
        .expect("a revision");
    assert_eq!(was.body, "second", "the body the LAST edit replaced");
}

#[tokio::test]
async fn putting_a_version_back_is_an_edit_like_any_other() {
    // What `task undo` does: read the previous version, write it back. It must
    // leave its own revision, or an undo could not be undone — and it must go
    // through the ordinary path, so the history says it happened.
    let pool = common::fresh_db().await;
    let id = file(&pool, "as filed", "the first body").await;
    edit(&pool, id, body("the clobbering body")).await;

    let was = repo::previous(&pool, id, &pippijn())
        .await
        .expect("reading")
        .expect("a revision");
    edit(
        &pool,
        id,
        Change {
            subject: Some(was.subject.clone()),
            body: Some(was.body.clone()),
            ..Change::default()
        },
    )
    .await;

    let detail = repo::get(&pool, id)
        .await
        .expect("reading")
        .expect("the task");
    assert_eq!(detail.body, "the first body", "restored");

    let now = repo::previous(&pool, id, &pippijn())
        .await
        .expect("reading")
        .expect("a revision");
    assert_eq!(
        now.body, "the clobbering body",
        "undoing an undo puts the clobber back"
    );
}

#[tokio::test]
async fn an_edit_reports_the_text_it_landed_on() {
    // The half that refuses nothing: a writer is told, at the moment of the
    // write, whose text it just replaced and when. The loss this answers was a
    // session that believed a body was three days old when it had been
    // rewritten the day before by somebody else.
    let pool = common::fresh_db().await;
    let id = file(&pool, "as filed", "0123456789").await;
    let done = edit(&pool, id, body("shorter")).await;

    let replaced = done
        .replaced
        .expect("an edit that moved text says what it moved");
    assert_eq!(replaced.by, "pippijn", "who wrote what was displaced");
    assert_eq!((replaced.was, replaced.now), (10, 7));
}

#[tokio::test]
async fn a_change_that_moves_no_text_reports_nothing_displaced() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "as filed", "the first body").await;
    let done = edit(
        &pool,
        id,
        Change {
            status: Some(Status::Doing),
            ..Change::default()
        },
    )
    .await;
    assert!(
        done.replaced.is_none(),
        "starting a task displaces no prose, and saying so would be a lie about a write"
    );
}

#[tokio::test]
async fn the_task_says_whether_there_is_anything_to_put_back() {
    // The app offers the control off this flag alone. Fetching a revision to
    // find out would carry a whole second body to every reader who opens a task
    // and was never going to undo anything.
    let pool = common::fresh_db().await;
    let id = file(&pool, "as filed", "the first body").await;
    let before = repo::get(&pool, id)
        .await
        .expect("reading")
        .expect("the task");
    assert!(!before.restorable, "nothing has overwritten it yet");

    edit(&pool, id, body("something else")).await;
    let after = repo::get(&pool, id)
        .await
        .expect("reading")
        .expect("the task");
    assert!(after.restorable, "an edit leaves something to put back");
}

#[tokio::test]
async fn a_status_change_leaves_nothing_to_put_back() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "as filed", "the first body").await;
    edit(
        &pool,
        id,
        Change {
            status: Some(Status::Doing),
            ..Change::default()
        },
    )
    .await;
    let detail = repo::get(&pool, id)
        .await
        .expect("reading")
        .expect("the task");
    assert!(
        !detail.restorable,
        "starting a task overwrites no prose, and offering an undo would restore text nobody replaced"
    );
}
