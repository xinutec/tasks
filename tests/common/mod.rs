//! What the DB tests share: the database, or a failure. The queries are runtime
//! strings, so running them is the only check on the SQL.
//!
//! ⚠ Unset means failure, not skip: an early return reports `ok` with none of
//! the SQL exercised.

use sqlx::MySqlPool;

/// The test database, or a failure that says how to get one.
pub(crate) fn test_db_url() -> String {
    std::env::var("TASKS_TEST_DATABASE_URL").unwrap_or_else(|_| {
        panic!(
            "TASKS_TEST_DATABASE_URL is unset, and these tests are the only check \
             on the SQL — so this is a failure rather than a skip.\n\
             \n\
             Run the whole gate, which supplies a throwaway MariaDB itself:\n\
             \x20   nix run \"git+file:../dev-lint?ref=HEAD#gate\" -- . gate.json\n\
             \n\
             Or just this suite against one:\n\
             \x20   nix develop --command nix run \"git+file:../dev-lint?ref=HEAD#with-test-db\" -- \\\n\
             \x20     --database tasks --user tasks --password tasks --port 3321 \\\n\
             \x20     --url-env TASKS_TEST_DATABASE_URL -- cargo test -- --test-threads=1"
        )
    })
}

/// A migrated pool, emptied first: truncated, children before parents, since the
/// suite shares one database and leftover rows would make counts depend on file
/// order.
pub(crate) async fn fresh_db() -> MySqlPool {
    let pool = tasks::db::connect(&test_db_url())
        .await
        .expect("connecting to the test database");
    tasks::db::migrate(&pool).await.expect("migrating");
    // Written out rather than looped over a list of names: sqlx 0.9 takes only
    // `&'static str`, and asserting a formatted string safe to save three lines
    // is exactly the audit that bound exists to force.
    for statement in [
        // Children before parents, so the list shows the shape. `check_run` has no keys
        // either way; emptied so counts in `tests/checks.rs` don't depend on file order.
        "DELETE FROM check_run",
        "DELETE FROM command_run",
        "DELETE FROM reported",
        "DELETE FROM task_revision",
        "DELETE FROM task_events",
        // Hangs off BOTH of the two below it, which is why it is above both.
        // `sessions.focus_until` goes with the session row and needs no line.
        "DELETE FROM session_focus",
        "DELETE FROM tasks",
        "DELETE FROM sessions",
    ] {
        sqlx::query(statement)
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("running {statement}: {e}"));
    }
    // ⚠ The id sequence is not reset, so no test can depend on the id it will get.
    // (The DDL would also break dev-lint's schema reader.)
    pool
}
