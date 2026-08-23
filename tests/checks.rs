//! What a model check did, recorded — the instrument the two open questions
//! about this tool need.
//!
//! ⚠ **`Quiet` and `Timeout` are the pair worth a test.** Both leave the caller
//! with no advice and the write already done, so from outside they look
//! identical; counted together they would report a tool that runs and finds
//! nothing, when what happened is a tool that never ran. The classification is
//! the whole of that distinction, and it turns on an error's chain rather than
//! on its message.

mod common;

use tasks::tasks::checks::{self, Kind, Outcome, Run};

/// The real thing `call()` produces on the timeout path: a `tokio` `Elapsed`,
/// wrapped in the context the caller prints. Constructed rather than faked,
/// because what is being tested is that the wrapping does not hide it.
async fn timed_out() -> anyhow::Result<String> {
    use anyhow::Context;
    tokio::time::timeout(std::time::Duration::ZERO, std::future::pending::<()>())
        .await
        .with_context(|| "no answer in 120s".to_string())?;
    unreachable!("a pending future cannot complete")
}

#[tokio::test]
async fn a_timeout_is_not_a_quiet_check() {
    let said = timed_out().await;
    assert_eq!(checks::outcome(&said, false), Outcome::Timeout);
}

#[test]
fn a_failure_that_is_not_a_timeout_is_an_error() {
    let said: anyhow::Result<String> = Err(anyhow::anyhow!("no `claude` on PATH"));
    assert_eq!(checks::outcome(&said, false), Outcome::Error);
}

#[test]
fn an_answer_nobody_acted_on_is_quiet() {
    let said: anyhow::Result<String> = Ok("DENSE".into());
    assert_eq!(checks::outcome(&said, false), Outcome::Quiet);
}

#[test]
fn an_answer_that_named_something_spoke() {
    let said: anyhow::Result<String> = Ok("#982 says this already".into());
    assert_eq!(checks::outcome(&said, true), Outcome::Spoke);
}

/// One row per run, with the fields that only exist for one kind left out.
#[tokio::test]
async fn a_filing_check_is_recorded_before_there_is_a_task_to_name() {
    let pool = common::fresh_db().await;
    checks::record(
        &pool,
        "s-filing",
        &Run {
            kind: Kind::Filing,
            task_id: None,
            input_chars: 13_720,
            accreted: None,
            elapsed_ms: 24_000,
            outcome: Outcome::Quiet,
        },
    )
    .await
    .expect("recording a filing check");

    let row: (String, Option<u64>, Option<u32>, u32, String) = sqlx::query_as(
        "SELECT kind, task_id, accreted, elapsed_ms, outcome FROM check_run WHERE session = ?",
    )
    .bind("s-filing")
    .fetch_one(&pool)
    .await
    .expect("reading it back");
    assert_eq!(row.0, "filing");
    assert_eq!(row.1, None, "a filing check names no task: none exists yet");
    assert_eq!(row.2, None, "nothing accreted — that is the other kind");
    assert_eq!(row.3, 24_000);
    assert_eq!(row.4, "quiet");
}

#[tokio::test]
async fn a_density_run_keeps_what_crossed_the_sampler() {
    let pool = common::fresh_db().await;
    checks::record(
        &pool,
        "s-density",
        &Run {
            kind: Kind::Density,
            task_id: Some(982),
            input_chars: 100_382,
            accreted: Some(3_412),
            elapsed_ms: 20_500,
            outcome: Outcome::Spoke,
        },
    )
    .await
    .expect("recording a density read");
    // ⚠ There is no task #982 in this database, and the row lands anyway. The
    // table carries no foreign keys on purpose: an instrument that could be
    // refused — or removed — by the thing it measures is not one.

    let row: (String, Option<u64>, Option<u32>, String) =
        sqlx::query_as("SELECT kind, task_id, accreted, outcome FROM check_run")
            .fetch_one(&pool)
            .await
            .expect("reading it back");
    assert_eq!(
        row,
        ("density".into(), Some(982), Some(3_412), "spoke".into())
    );
}
