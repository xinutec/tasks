//! Whose edit an undo would revert.
//!
//! ⚠ **These exist because it happened, to the session that built the store.**
//! `task undo` restores the one version a task keeps, whoever displaced it — so
//! it reverts *the* last edit, not *your* last edit. On 2026-08-15 a session
//! over-deleted #921's body, another appended to it eleven seconds later, and
//! the first ran `task undo` meaning to take back its own write. What it took
//! back was the other session's.
//!
//! What is under test here is the fact the CLI needs in order to refuse:
//! [`Revision::mine`], answered against the STORED identity rather than the
//! rendered label. The refusal itself is `tasks::tasks::undo`, whose wording is
//! tested beside it — this file is about getting the question right, because a
//! guard that answers "is this mine" wrongly is worse than none: it either
//! blocks a caller from its own work or waves through somebody else's.

mod common;

use tasks::sessions;
use tasks::tasks::repo::{self, Change, NewTask};
use tasks::tasks::types::{Actor, Priority, Ranking, Revision};
use tasks::tasks::undo;

fn session(id: &str) -> Actor {
    Actor::Session(id.into())
}

fn person(id: &str) -> Actor {
    Actor::Person(id.into())
}

/// A conversation the service has seen. Acting as one it has not is refused,
/// which is the rule that makes `sessions` the identity `mine` compares.
async fn known(pool: &sqlx::MySqlPool, id: &str) -> Actor {
    sessions::touch(pool, id, None).await.expect("registering");
    session(id)
}

async fn file(pool: &sqlx::MySqlPool, who: &Actor) -> u64 {
    repo::create(
        pool,
        NewTask {
            subject: "a task two conversations both touch".into(),
            // The check ran: these file through the service the way a session does.
            checked: true,
            body: "as first written".into(),
            priority: Ranking::At(Priority::P2),
            due: None,
            blocked_on: Vec::new(),
            assignee: None,
        },
        who,
    )
    .await
    .expect("filing")
    .id
}

async fn edit(pool: &sqlx::MySqlPool, id: u64, body: &str, who: &Actor) {
    repo::update(
        pool,
        id,
        Change {
            body: Some(body.into()),
            ..Change::default()
        },
        who,
    )
    .await
    .expect("editing");
}

async fn seen_by(pool: &sqlx::MySqlPool, id: u64, who: &Actor) -> Revision {
    repo::previous(pool, id, who)
        .await
        .expect("reading")
        .expect("a revision")
}

#[tokio::test]
async fn my_own_edit_is_mine_to_put_back() {
    let pool = common::fresh_db().await;
    let me = known(&pool, "sess-a").await;
    let id = file(&pool, &me).await;
    edit(&pool, id, "my rewrite", &me).await;

    let was = seen_by(&pool, id, &me).await;
    assert!(was.mine, "a caller's own edit did not read as theirs");
    assert!(!undo::needs_saying(&was));
}

#[tokio::test]
async fn another_conversations_edit_is_not() {
    let pool = common::fresh_db().await;
    let (me, them) = (known(&pool, "sess-a").await, known(&pool, "sess-b").await);
    let id = file(&pool, &me).await;
    edit(&pool, id, "their rewrite", &them).await;

    let was = seen_by(&pool, id, &me).await;
    assert!(!was.mine, "another session's edit read as the caller's own");
    assert!(undo::needs_saying(&was));
}

#[tokio::test]
async fn the_same_id_under_a_different_kind_is_a_different_actor() {
    let pool = common::fresh_db().await;
    // ⚠ A person and a session may hold the same string. Comparing ids alone
    // would make each look like the other, which is the failure that matters:
    // it waves an undo through as "yours".
    let (as_person, as_session) = (person("pippijn"), known(&pool, "pippijn").await);
    let id = file(&pool, &as_person).await;
    edit(&pool, id, "written by the session", &as_session).await;

    assert!(!seen_by(&pool, id, &as_person).await.mine);
    assert!(seen_by(&pool, id, &as_session).await.mine);
}

#[tokio::test]
async fn only_the_last_edit_counts_because_only_one_version_is_kept() {
    let pool = common::fresh_db().await;
    let (me, them) = (known(&pool, "sess-a").await, known(&pool, "sess-b").await);
    let id = file(&pool, &me).await;

    // The shape of the incident: mine, then theirs on top, seconds apart.
    edit(&pool, id, "my over-deletion", &me).await;
    edit(&pool, id, "their append on top of it", &them).await;

    // ⚠ The version waiting to come back is now MY text — which is exactly why
    // this reads as safe and is not. Restoring it reverts THEIR edit. `mine`
    // answers about the edit being reverted, never about the text returning.
    let was = seen_by(&pool, id, &me).await;
    assert_eq!(was.body, "my over-deletion", "the wrong version is waiting");
    assert!(!was.mine, "the edit being reverted was theirs");
    assert!(undo::needs_saying(&was));
}

#[tokio::test]
async fn undoing_an_undo_is_still_mine() {
    let pool = common::fresh_db().await;
    let me = known(&pool, "sess-a").await;
    let id = file(&pool, &me).await;
    edit(&pool, id, "first rewrite", &me).await;

    // Undo is an ordinary edit and leaves its own revision, so the second one
    // must not start refusing on the strength of the first.
    let was = seen_by(&pool, id, &me).await;
    edit(&pool, id, &was.body, &me).await;

    assert!(seen_by(&pool, id, &me).await.mine);
}

#[tokio::test]
async fn the_refusal_names_whom_and_when_and_the_way_through() {
    let pool = common::fresh_db().await;
    let (me, them) = (known(&pool, "sess-a").await, known(&pool, "dev-lint").await);
    let id = file(&pool, &me).await;
    edit(&pool, id, "their rewrite", &them).await;

    let said = undo::refusal(&seen_by(&pool, id, &me).await, id);

    // Whose, or the reader cannot judge whether to override.
    assert!(said.contains("dev-lint"), "{said}");
    // When, because a collision seconds old and an edit from last week want
    // different decisions.
    assert!(said.contains("UTC"), "{said}");
    // What to do instead, and how to insist.
    assert!(said.contains("--previous"), "{said}");
    assert!(said.contains("--anyway"), "{said}");
}
