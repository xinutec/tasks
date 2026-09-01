//! MariaDB connection pool. This app's own database — NC is never written to.
//!
//! Copied from `~/Code/life/src/db.rs`, which is where both non-obvious parts
//! were worked out and where any correction to them should also land.

use anyhow::{Context, Result};
use sqlx::MySqlPool;
use sqlx::mysql::MySqlPoolOptions;

pub async fn connect(database_url: &str) -> Result<MySqlPool> {
    let pool = MySqlPoolOptions::new()
        .max_connections(8)
        // Pin every connection's session zone to UTC. Columns written by the DB
        // clock (`NOW()`, `DEFAULT CURRENT_TIMESTAMP`) are read back with
        // `.and_utc()`, which asserts UTC — so the session zone MUST be UTC or
        // those instants drift by the server's offset. Without this the code is
        // correct only because the container happens to run UTC; here it's
        // correct by construction, matching the Rust-side `.naive_utc()` writes.
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                sqlx::query("SET time_zone = '+00:00'")
                    .execute(conn)
                    .await?;
                Ok(())
            })
        })
        .connect(database_url)
        .await
        .context("connecting to MariaDB")?;
    Ok(pool)
}

const MIGRATION_LOCK: &str = "tasks_migrations";
/// Long enough to outlast a real migration, short enough to fail loudly if a
/// previous holder wedged.
const MIGRATION_LOCK_TIMEOUT_SECS: i32 = 60;

/// Apply embedded migrations from `migrations/`. Idempotent; safe on every boot,
/// and safe when several processes boot **at the same time**.
///
/// sqlx does not take a cross-connection lock before applying migrations on MySQL,
/// so two processes starting together both see an empty `_sqlx_migrations`, both
/// apply version 1, and one dies with `1062 Duplicate entry '1' for key 'PRIMARY'`.
/// That surfaced as a flaky test suite (each DB test binary migrates on start), but
/// it would equally break a second backend replica at boot. A MySQL named lock —
/// connection-scoped, and auto-released if the holder dies — serialises the whole
/// migrate step; whoever gets in second finds the work already done and no-ops.
pub async fn migrate(pool: &MySqlPool) -> Result<()> {
    let mut conn = pool
        .acquire()
        .await
        .context("acquiring migration lock conn")?;

    let got: Option<i64> = sqlx::query_scalar("SELECT GET_LOCK(?, ?)")
        .bind(MIGRATION_LOCK)
        .bind(MIGRATION_LOCK_TIMEOUT_SECS)
        .fetch_one(&mut *conn)
        .await
        .context("taking the migration lock")?;
    // 1 = acquired, 0 = timed out, NULL = error. Only 1 means we may migrate.
    if got != Some(1) {
        anyhow::bail!(
            "could not acquire the '{MIGRATION_LOCK}' lock within {MIGRATION_LOCK_TIMEOUT_SECS}s \
             (another process may be migrating, or holding it wedged)"
        );
    }

    // Migrate on the pool while `conn` holds the lock — the lock is what serialises
    // us, so the migrator itself can use whatever connection it likes. Run to
    // completion, then release whatever happened: an early `?` here would hold the
    // lock until the connection dropped and stall every other booter.
    let migrated = sqlx::migrate!()
        .run(pool)
        .await
        .context("running migrations");

    let released = sqlx::query("SELECT RELEASE_LOCK(?)")
        .bind(MIGRATION_LOCK)
        .execute(&mut *conn)
        .await
        .context("releasing the migration lock");

    migrated?;
    released?;
    Ok(())
}
