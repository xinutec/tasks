//! What the DB integration tests share: the database they need, or a failure.
//!
//! These tests are the only check on this repository's SQL. The queries are
//! runtime strings — `sqlx::query_as` rather than the compile-time macros —
//! so running them **is** the check on them.
//!
//! ⚠ **Unset means failure, not skip.** Returning early after an `eprintln!`
//! nobody sees is what let a bare `cargo test` in `life` report `ok. 6 passed`
//! in 0.00s with none of the SQL exercised, looking exactly like a run that had
//! done the work. A test that silently passes when it cannot run is worse than
//! no test, because it reports coverage it is not providing.

use sqlx::MySqlPool;

/// The test database, or a failure that says how to get one.
pub(crate) fn test_db_url() -> String {
    std::env::var("TASKS_TEST_DATABASE_URL").unwrap_or_else(|_| {
        panic!(
            "TASKS_TEST_DATABASE_URL is unset, and these tests are the only check \
             on the SQL — so this is a failure rather than a skip.\n\
             \n\
             Run the whole gate, which supplies a throwaway MariaDB itself:\n\
             \x20   nix run ../dev-lint#gate -- . gate.json\n\
             \n\
             Or just this suite against one:\n\
             \x20   nix develop --command nix run ../dev-lint#with-test-db -- \\\n\
             \x20     --database tasks --user tasks --password tasks --port 3321 \\\n\
             \x20     --url-env TASKS_TEST_DATABASE_URL -- cargo test -- --test-threads=1"
        )
    })
}

/// A migrated pool, emptied first.
///
/// ⚠ **Truncated rather than dropped and recreated**, and children before
/// parents: `task_events` has a foreign key into `tasks`, and `tasks` one into
/// `sessions`. The suite runs single-threaded against one database, so leaving
/// a previous test's rows behind would make every count assertion depend on the
/// order the files happened to run in.
pub(crate) async fn fresh_db() -> MySqlPool {
    let pool = tasks::db::connect(&test_db_url())
        .await
        .expect("connecting to the test database");
    tasks::db::migrate(&pool).await.expect("migrating");
    // Written out rather than looped over a list of names: sqlx 0.9 takes only
    // `&'static str`, and asserting a formatted string safe to save three lines
    // is exactly the audit that bound exists to force.
    for statement in [
        // Before `task_events`, which it hangs off. The FK cascades, so this is
        // belt and braces — but the order this list is written in is the
        // documentation of the shape, and a reader should not have to check a
        // migration to know which way the arrows point.
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
    // ⚠ **The id sequence is deliberately NOT reset.** An `ALTER TABLE …
    // AUTO_INCREMENT` here would let a test assert on `#1`, and a test that
    // depends on the id it is about to be given is one that breaks the day
    // another test is added above it. Every assertion below names the id it was
    // handed. (It is also DDL dev-lint's schema reader cannot parse, which
    // silently disables its absence checks for the whole repository.)
    pool
}
