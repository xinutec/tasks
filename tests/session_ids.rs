//! Which ids a conversation may be recorded under.
//!
//! Ids are not all UUIDs: a script names itself (`TASKS_SESSION=claude-sync`).
//! An id that is ANOTHER conversation's name is refused, because every
//! `--to <name>` would resolve to it instead of to the named conversation.

mod common;

use tasks::sessions;

const RECALL: &str = "d5c6955f-a76d-45af-b48e-f9ecc92225a0";

#[tokio::test]
async fn an_id_that_is_another_conversations_name_is_refused() {
    let pool = common::fresh_db().await;
    sessions::touch(&pool, RECALL, Some("recall"))
        .await
        .expect("the real one");
    let refused = sessions::touch(&pool, "recall", None)
        .await
        .expect_err("a phantom named after recall was recorded");
    let said = refused.to_string();
    assert!(said.contains(RECALL), "{said}");
    let rows: Vec<(String,)> = sqlx::query_as("SELECT id FROM sessions WHERE id = 'recall'")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(rows.is_empty(), "the refusal still wrote the row");
}

#[tokio::test]
async fn a_scripts_own_id_is_still_recorded() {
    let pool = common::fresh_db().await;
    sessions::touch(&pool, RECALL, Some("recall"))
        .await
        .unwrap();
    sessions::touch(&pool, "claude-sync", None)
        .await
        .expect("an id nobody is named is a script's own");
}

#[tokio::test]
async fn a_conversation_touching_itself_again_is_not_refused() {
    // Its own name is not "another conversation's".
    let pool = common::fresh_db().await;
    sessions::touch(&pool, "claude-sync", Some("claude-sync"))
        .await
        .unwrap();
    sessions::touch(&pool, "claude-sync", Some("claude-sync"))
        .await
        .unwrap();
}

#[tokio::test]
async fn the_migration_took_out_a_phantom_that_holds_nothing() {
    // Re-applied by hand after seeding, because `fresh_db` migrates an empty
    // table. The statement is the migration file itself.
    let pool = common::fresh_db().await;
    sessions::touch(&pool, RECALL, Some("recall"))
        .await
        .unwrap();
    sessions::touch(&pool, "claude-sync", None).await.unwrap();
    // The phantom, inserted directly: `touch` refuses it now.
    sqlx::query("INSERT INTO sessions (id) VALUES ('recall')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../migrations/0016_phantom_sessions.sql"))
        .execute(&pool)
        .await
        .unwrap();
    let left: Vec<(String,)> = sqlx::query_as("SELECT id FROM sessions ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    let left: Vec<String> = left.into_iter().map(|(id,)| id).collect();
    assert_eq!(left, vec!["claude-sync".to_string(), RECALL.to_string()]);
}

#[tokio::test]
async fn the_migration_leaves_a_phantom_that_still_holds_a_task() {
    // Deleting it would set the task's holder to NULL; it has to be moved first.
    let pool = common::fresh_db().await;
    sessions::touch(&pool, RECALL, Some("recall"))
        .await
        .unwrap();
    sqlx::query("INSERT INTO sessions (id) VALUES ('recall')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO tasks (subject, body, assignee_kind, assignee_session) \
         VALUES ('held by the phantom', '', 'session', 'recall')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!("../migrations/0016_phantom_sessions.sql"))
        .execute(&pool)
        .await
        .unwrap();
    let kept: Option<(String,)> = sqlx::query_as("SELECT id FROM sessions WHERE id = 'recall'")
        .fetch_optional(&pool)
        .await
        .unwrap();
    assert!(kept.is_some(), "a phantom holding a task was deleted");
}
