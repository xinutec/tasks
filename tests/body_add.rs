//! Adding to a body instead of replacing it.
//!
//! ⚠ **These exist because the absence of this cost two bodies in one
//! afternoon.** Until 2026-08-15 the only way to write prose onto a task was
//! `--body`, which replaces — so recording an outcome meant reading the old
//! text out, concatenating by hand, and sending the whole thing back. Twice
//! that day the read half was skipped by the session that maintains this tool:
//! once caught by the collapse guard, and once at 52% kept, which is under the
//! threshold and which nothing catches. The second one was only noticed because
//! the writer went looking.
//!
//! Two properties are under test, and the second is the one that could not be
//! got by fixing this in the client:
//!
//! 1. **The old body survives, whole**, with exactly one blank line at the
//!    seam — a note butted straight onto prose becomes its first sentence.
//! 2. **The addition is resolved against the body inside the transaction that
//!    reads it.** A client-side read-concatenate-PATCH would lose one of two
//!    concurrent additions, and this store has sessions editing the same task
//!    eleven seconds apart on record.

mod common;

use tasks::tasks::repo::{self, Change, NewTask};
use tasks::tasks::types::{Actor, Priority, Ranking};

fn pippijn() -> Actor {
    Actor::Person("pippijn".into())
}

fn dev_lint() -> Actor {
    Actor::Person("dev-lint".into())
}

async fn file(pool: &sqlx::MySqlPool, body: &str) -> u64 {
    repo::create(
        pool,
        NewTask {
            subject: "a task with a filing worth keeping".into(),
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

async fn stored(pool: &sqlx::MySqlPool, id: u64) -> String {
    repo::get(pool, id)
        .await
        .expect("reading")
        .expect("there")
        .body
}

fn prepend(text: &str) -> Change {
    Change {
        prepend: Some(text.into()),
        ..Change::default()
    }
}

fn append(text: &str) -> Change {
    Change {
        append: Some(text.into()),
        ..Change::default()
    }
}

fn refusal(e: tasks::error::AppError) -> String {
    match e {
        tasks::error::AppError::BadRequest(msg) => msg,
        other => panic!("expected a bad request, got {other:?}"),
    }
}

#[tokio::test]
async fn prepending_keeps_every_word_of_what_was_there() {
    let pool = common::fresh_db().await;
    let filing = "## The filing\n\nEverything anybody knew when this was raised.";
    let id = file(&pool, filing).await;

    repo::update(&pool, id, prepend("DONE in a2c3ab6."), &pippijn())
        .await
        .expect("prepending");

    assert_eq!(
        stored(&pool, id).await,
        format!("DONE in a2c3ab6.\n\n{filing}")
    );
}

#[tokio::test]
async fn appending_puts_it_under() {
    let pool = common::fresh_db().await;
    let filing = "## The filing\n\nWhat was known at the time.";
    let id = file(&pool, filing).await;

    repo::update(&pool, id, append("Then this happened."), &pippijn())
        .await
        .expect("appending");

    assert_eq!(
        stored(&pool, id).await,
        format!("{filing}\n\nThen this happened.")
    );
}

#[tokio::test]
async fn one_blank_line_at_the_seam_whatever_was_typed() {
    // ⚠ Not cosmetic. Markdown joins adjacent lines into one paragraph, so a
    // one-line note butted onto a body that opens with prose silently becomes
    // the first sentence OF that prose — two claims read as one.
    let pool = common::fresh_db().await;
    let id = file(&pool, "\n\n  The filing, indented and padded.\n\n\n").await;

    repo::update(&pool, id, prepend("DONE.\n\n\n"), &pippijn())
        .await
        .expect("prepending");

    let now = stored(&pool, id).await;
    // ⚠ **The indentation survives.** Only newlines are eaten at the seam:
    // whitespace inside a line is markdown content — two trailing spaces are a
    // hard break, leading spaces set continuation — so a `trim` here would edit
    // what the task says while claiming to keep it. The first version of
    // `joined` did exactly that and this assertion is what caught it.
    assert_eq!(now, "DONE.\n\n  The filing, indented and padded.\n\n\n");
    assert!(
        !now.contains("\n\n\n  The"),
        "more than one blank line: {now:?}"
    );
}

#[tokio::test]
async fn appending_does_not_restyle_the_top_it_never_touched() {
    // The seam is trimmed; the far end is left exactly as it was. Otherwise an
    // append reports characters moved at the other end of the body, which reads
    // as an edit nobody made.
    let pool = common::fresh_db().await;
    let id = file(&pool, "\n\nThe filing, with a blank line above it.").await;

    repo::update(&pool, id, append("And this."), &pippijn())
        .await
        .expect("appending");

    assert_eq!(
        stored(&pool, id).await,
        "\n\nThe filing, with a blank line above it.\n\nAnd this."
    );
}

#[tokio::test]
async fn both_at_once() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "the middle").await;

    repo::update(
        &pool,
        id,
        Change {
            prepend: Some("on top".into()),
            append: Some("underneath".into()),
            ..Change::default()
        },
        &pippijn(),
    )
    .await
    .expect("both");

    assert_eq!(
        stored(&pool, id).await,
        "on top\n\nthe middle\n\nunderneath"
    );
}

#[tokio::test]
async fn prepending_to_a_task_filed_with_no_prose() {
    // No seam to normalise, so no leading blank line: most tasks are filed with
    // an empty body and this is the first thing written to them.
    let pool = common::fresh_db().await;
    let id = file(&pool, "").await;

    repo::update(
        &pool,
        id,
        prepend("The first thing anybody wrote."),
        &pippijn(),
    )
    .await
    .expect("prepending");

    assert_eq!(stored(&pool, id).await, "The first thing anybody wrote.");
}

#[tokio::test]
async fn additions_compose_one_after_another() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "the filing").await;

    repo::update(&pool, id, prepend("mine"), &pippijn())
        .await
        .expect("first");
    repo::update(&pool, id, prepend("theirs"), &dev_lint())
        .await
        .expect("second");

    let now = stored(&pool, id).await;
    assert_eq!(now, "theirs\n\nmine\n\nthe filing");
}

#[tokio::test]
async fn adding_waits_for_whoever_is_already_writing_the_body() {
    // ⚠ **The property that made this server-side, and it needs a LOCK rather
    // than merely a transaction.** Both updates read the body and write a
    // version built from it. Under InnoDB's REPEATABLE READ a plain `SELECT` is
    // a non-locking snapshot, so being inside a transaction is NOT enough: two
    // would both read `the filing`, and whichever committed second would store
    // a body that had never contained the other's text. `SELECT … FOR UPDATE`
    // is what makes the second wait and then read what the first wrote.
    //
    // ⚠ **Two earlier versions of this test passed against the BROKEN code**,
    // which is why it is shaped so awkwardly:
    //
    // * A `tokio::join!` of two updates passed three times out of three —
    //   `join!` polls the first until it yields, and a local MariaDB is fast
    //   enough that the two rarely overlap where it matters.
    // * Asserting that the addition merely BLOCKS passed too. It does block —
    //   on the `UPDATE`, not the read — and then writes the body it built from
    //   text that had already been replaced. Blocking is not the property; not
    //   losing the other write is.
    //
    // So this drives the interleaving by hand and asserts on the TEXT.
    let pool = common::fresh_db().await;
    let id = file(&pool, "the filing").await;

    // Another conversation, holding the row and not yet committed.
    let mut theirs = pool.begin().await.expect("their transaction");
    sqlx::query("SELECT body FROM tasks WHERE id = ? FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *theirs)
        .await
        .expect("their read");

    let mine = tokio::spawn({
        let pool = pool.clone();
        async move { repo::update(&pool, id, prepend("mine"), &pippijn()).await }
    });

    // The precondition, asserted rather than assumed: if this addition had
    // already finished, everything below would pass without proving anything —
    // it would simply be the sequential case again.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        !mine.is_finished(),
        "the addition did not wait for the lock"
    );

    // Their write lands first, and it is what a correct addition must build on.
    sqlx::query("UPDATE tasks SET body = ? WHERE id = ?")
        .bind("theirs\n\nthe filing")
        .bind(id)
        .execute(&mut *theirs)
        .await
        .expect("their write");
    theirs.commit().await.expect("their commit");

    mine.await.expect("joining").expect("my addition");

    let now = stored(&pool, id).await;
    assert!(now.contains("mine"), "my addition was lost: {now:?}");
    // ⚠ The assertion that fails without `FOR UPDATE`: an addition that read
    // the body before their commit builds from `the filing` alone and stores a
    // version their text was never in.
    assert!(now.contains("theirs"), "their write was lost: {now:?}");
}

#[tokio::test]
async fn the_collapse_guard_cannot_fire_on_something_that_only_grows() {
    // The guard refuses a body that keeps under a quarter of itself. Adding to
    // one can never do that, so `--replace-body` is never needed here — and if
    // it ever were, the guard would be refusing the operation that exists to
    // stop bodies being lost.
    let pool = common::fresh_db().await;
    let long = "word ".repeat(600); // 3,000 characters: well over the threshold
    let id = file(&pool, &long).await;

    repo::update(&pool, id, prepend("x"), &pippijn())
        .await
        .expect("a growing body was refused");

    assert!(stored(&pool, id).await.contains(long.trim()));
}

#[tokio::test]
async fn what_it_replaced_is_still_recoverable() {
    // An addition is an edit like any other: it leaves a revision, so a prepend
    // somebody regrets is one `task undo` away.
    let pool = common::fresh_db().await;
    let id = file(&pool, "the filing").await;

    repo::update(&pool, id, prepend("DONE."), &pippijn())
        .await
        .expect("prepending");

    let was = repo::previous(&pool, id, &pippijn())
        .await
        .expect("reading")
        .expect("a revision");
    assert_eq!(was.body, "the filing");
}

#[tokio::test]
async fn the_history_counts_an_addition_as_what_it_is() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "the filing").await;

    repo::update(&pool, id, prepend("DONE."), &pippijn())
        .await
        .expect("prepending");

    let detail = repo::get(&pool, id)
        .await
        .expect("reading")
        .expect("there")
        .events
        .into_iter()
        .find(|e| e.kind == "edited")
        .and_then(|e| e.detail)
        .expect("an edit with nothing said about it");
    // Grew rather than shrank, which is the whole point and is visible to the
    // next reader without opening anything.
    assert_eq!(detail, "body 10 → 17 chars");
}

#[tokio::test]
async fn replacing_and_adding_at_once_is_refused_as_a_contradiction() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "the filing").await;

    let said = refusal(
        repo::update(
            &pool,
            id,
            Change {
                body: Some("a whole new body".into()),
                prepend: Some("and this on top".into()),
                ..Change::default()
            },
            &pippijn(),
        )
        .await
        .expect_err("--body with --prepend"),
    );

    assert!(said.contains("--body"), "{said}");
    assert!(said.contains("--prepend"), "{said}");
    assert_eq!(
        stored(&pool, id).await,
        "the filing",
        "the body moved anyway"
    );
}

#[tokio::test]
async fn adding_nothing_is_refused_rather_than_quietly_doing_nothing() {
    // ⚠ These fields are filled from heredocs, variables and piped commands.
    // The way one arrives empty is that whatever was meant to produce the text
    // produced none, and reporting success is how that goes unnoticed.
    let pool = common::fresh_db().await;
    let id = file(&pool, "the filing").await;

    for change in [prepend("   \n  "), append("")] {
        let said = refusal(
            repo::update(&pool, id, change, &pippijn())
                .await
                .expect_err("an empty addition"),
        );
        assert!(said.contains("nothing to add"), "{said}");
    }

    assert_eq!(stored(&pool, id).await, "the filing");
}
