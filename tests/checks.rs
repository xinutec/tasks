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

use tasks::tasks::checks::{self, Kind, Outcome, Ran, Run};

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
            subject_key: None,
            said: None,
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
            subject_key: None,
            said: None,
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

/// Two runs of the same kind, one of which never answered.
fn spent(kind: Kind, elapsed_ms: u32, outcome: Outcome) -> Ran {
    Ran {
        ran_at: chrono::Utc::now(),
        kind,
        task_id: None,
        input_chars: 1_000,
        accreted: None,
        elapsed_ms,
        outcome,
    }
}

#[test]
fn a_timeout_counts_towards_the_latency_it_cost() {
    // ⚠ The bound is being judged from these numbers. Leaving the abandoned
    // call out of the spread is what would make a bound that fires look
    // comfortable — the run took the whole patience, and it is the worst case.
    let runs = vec![
        spent(Kind::Filing, 8_000, Outcome::Quiet),
        spent(Kind::Filing, 120_000, Outcome::Timeout),
    ];
    let [filing] = checks::tally(&runs).try_into().expect("one kind, one line");
    assert_eq!(filing.runs, 2);
    assert_eq!(filing.timeout, 1);
    assert_eq!(filing.worst_ms, 120_000);
}

#[test]
fn the_two_checks_are_never_folded_together() {
    // They answer different questions and have different bounds; one line for
    // both would average a 20-second body read into a filing's latency budget.
    let runs = vec![
        spent(Kind::Filing, 10_000, Outcome::Quiet),
        spent(Kind::Density, 60_000, Outcome::Spoke),
    ];
    let lines = checks::tally(&runs);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].kind, Kind::Filing);
    assert_eq!(lines[0].median_ms, 10_000);
    assert_eq!(lines[1].kind, Kind::Density);
    assert_eq!(lines[1].median_ms, 60_000);
}

#[test]
fn the_percentiles_are_nearest_rank_over_what_there_is() {
    // Ten runs, so p90 is the ninth: a definition that interpolates would
    // invent a latency nothing took, on a sample this small.
    let runs: Vec<Ran> = (1..=10)
        .map(|n| spent(Kind::Filing, n * 1_000, Outcome::Quiet))
        .collect();
    let [filing] = checks::tally(&runs).try_into().expect("one kind, one line");
    assert_eq!(filing.median_ms, 5_000);
    assert_eq!(filing.p90_ms, 9_000);
    assert_eq!(filing.worst_ms, 10_000);
}

#[tokio::test]
async fn what_is_read_back_is_the_window_asked_for() {
    let pool = common::fresh_db().await;
    for (session, kind) in [("s-1", Kind::Filing), ("s-2", Kind::Density)] {
        checks::record(
            &pool,
            session,
            &Run {
                kind,
                task_id: None,
                input_chars: 10,
                accreted: None,
                elapsed_ms: 1_000,
                outcome: Outcome::Quiet,
                subject_key: None,
                said: None,
            },
        )
        .await
        .expect("recording");
    }
    // Backdated past the window, so a query that ignored `days` would find it.
    sqlx::query("UPDATE check_run SET ran_at = NOW() - INTERVAL 30 DAY WHERE session = 's-2'")
        .execute(&pool)
        .await
        .expect("backdating one");

    let week = checks::recent(&pool, 7).await.expect("reading a week");
    assert_eq!(week.len(), 1);
    assert_eq!(week[0].kind, Kind::Filing);
    assert_eq!(
        checks::recent(&pool, 60)
            .await
            .expect("reading two months")
            .len(),
        2
    );
}

// The wiring, through the real router.
//
// ⚠ **The route table is the one line that can be wrong while every test above
// passes.** `record` and `recent` are proved by the tests over the pool; what
// decides whether a session's report ever reaches them is one entry in
// `routes::mod`, and a CLI that swallows its own failure — deliberately, since
// a check must never cost a write — would report nothing at all if it pointed
// at a path that 404s. That silence is exactly what this table exists to end.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::MySqlPool;
use tasks::config::{AuthConfig, Config};
use tasks::routes;
use tasks::session::{UserSession, create_session};
use tasks::state::AppState;
use tower::ServiceExt;

const SECRET: &str = "test-secret";
const TOKEN: &str = "test-agent-token";

fn app(pool: MySqlPool) -> axum::Router {
    let cfg = Config {
        database_url: String::new(),
        bind_addr: "127.0.0.1:0".into(),
        static_dir: None,
        auth: Some(AuthConfig {
            session_secret: SECRET.into(),
            nc_base_url: "https://dash.example".into(),
            nc_internal_url: None,
            nc_client_id: "id".into(),
            nc_client_secret: "secret".into(),
            nc_redirect_uri: "https://tasks.example/auth/callback".into(),
            allowed_users: vec!["pippijn".into()],
        }),
        agent_token: Some(TOKEN.into()),
    };
    routes::router(AppState::new(cfg, pool, reqwest::Client::new()))
}

const REPORTED: &str = r#"{"kind":"density","task_id":982,"input_chars":100382,
    "accreted":3412,"elapsed_ms":20500,"outcome":"spoke"}"#;

#[tokio::test]
async fn a_session_reports_a_run_and_reads_it_back() {
    let app = app(common::fresh_db().await);
    let posted = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/checks")
                .method("POST")
                .header("Authorization", format!("Bearer {TOKEN}"))
                .header("X-Session-Id", "sess-1")
                .header("Content-Type", "application/json")
                .body(Body::from(REPORTED))
                .unwrap(),
        )
        .await
        .expect("the router answered");
    assert_eq!(posted.status(), StatusCode::NO_CONTENT);

    let read = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/checks?days=1")
                .method("GET")
                .header("Authorization", format!("Bearer {TOKEN}"))
                .header("X-Session-Id", "sess-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("the router answered");
    assert_eq!(read.status(), StatusCode::OK);
    let body = axum::body::to_bytes(read.into_body(), 64 * 1024)
        .await
        .expect("a body");
    let runs: Vec<Ran> = serde_json::from_slice(&body).expect("rows this CLI could read");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].task_id, Some(982));
    assert_eq!(runs[0].outcome, Outcome::Spoke);
}

#[tokio::test]
async fn a_browser_has_no_check_to_report() {
    // A person never runs one: both are spawned by the CLI on the caller's
    // machine. Accepting the owner's would put rows in the table that no check
    // produced, which is the one thing that would make it unreadable.
    let app = app(common::fresh_db().await);
    let cookie = create_session(
        SECRET,
        &UserSession {
            user_id: "pippijn".into(),
            display_name: "Pippijn".into(),
        },
    );
    let refused = app
        .oneshot(
            Request::builder()
                .uri("/api/checks")
                .method("POST")
                .header("Cookie", format!("session={cookie}"))
                .header("Content-Type", "application/json")
                .body(Body::from(REPORTED))
                .unwrap(),
        )
        .await
        .expect("the router answered");
    assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
}

/// ⚠ **`--no-duplicate-check` may only overrule a refusal you have already
/// seen.** Measured over every transcript: 63 of 644 filings passed the flag on
/// the way IN and only 16 followed a refusal, so for 47 sessions the check never
/// ran and the trade the module rests on never happened.
#[tokio::test]
async fn a_skipped_check_is_licensed_only_by_a_refusal_of_that_subject() {
    let pool = common::fresh_db().await;
    let subject = "Compact MEMORY.md by merging files";

    assert!(
        !checks::refused_recently(&pool, "s-1", subject)
            .await
            .expect("asking"),
        "nothing has refused this session anything"
    );

    checks::record(
        &pool,
        "s-1",
        &Run {
            kind: Kind::Filing,
            task_id: None,
            input_chars: 16_000,
            accreted: None,
            elapsed_ms: 9_500,
            outcome: Outcome::Spoke,
            subject_key: Some(checks::subject_key(subject)),
            said: None,
        },
    )
    .await
    .expect("recording the refusal");

    assert!(
        checks::refused_recently(&pool, "s-1", subject)
            .await
            .expect("asking"),
        "the session that was refused may now overrule it"
    );
    assert!(
        !checks::refused_recently(&pool, "s-2", subject)
            .await
            .expect("asking"),
        "a refusal licenses the session that received it and no other"
    );
    assert!(
        !checks::refused_recently(&pool, "s-1", "something else entirely")
            .await
            .expect("asking"),
        "a refusal licenses the subject it refused and no other"
    );
}

/// ⚠ **A check that PASSED licenses nothing**, or every quiet filing would hand
/// out permission to skip the next one — which is the habit, with extra steps.
#[tokio::test]
async fn a_check_that_said_nothing_licenses_nothing() {
    let pool = common::fresh_db().await;
    let subject = "a subject nothing matched";
    checks::record(
        &pool,
        "s-quiet",
        &Run {
            kind: Kind::Filing,
            task_id: None,
            input_chars: 16_000,
            accreted: None,
            elapsed_ms: 7_100,
            outcome: Outcome::Quiet,
            subject_key: None,
            said: None,
        },
    )
    .await
    .expect("recording a quiet check");
    assert!(
        !checks::refused_recently(&pool, "s-quiet", subject)
            .await
            .expect("asking")
    );
}

/// The re-run is the SAME command, so it must not fail on capitalisation or a
/// stray space — the same rule `same_subject` follows.
#[test]
fn the_licence_ignores_what_a_retype_varies() {
    assert_eq!(
        checks::subject_key("  MEMORY.md Is Too Big  "),
        checks::subject_key("memory.md is too big")
    );
    assert_ne!(
        checks::subject_key("MEMORY.md is too big"),
        checks::subject_key("MEMORY.md is too big now")
    );
}

/// The flag a density read leaves behind, and the one edit that takes it away.
///
/// ⚠ **This is the half that used to not exist.** `check_run` recorded that a
/// read SPOKE and what it cost, and threw away what it said — so a finding
/// survived only as long as the transcript of whichever session ran the edit,
/// addressed to a session doing something else. Measured over the 5.6 days to
/// 2026-08-29: 229 of 268 reads spoke, and of the 43 tasks read more than once,
/// 28 only ever grew. These pin that the words outlive the tool result and that
/// exactly one thing retires them.
mod sprawl {
    use super::*;
    use sqlx::MySqlPool;
    use tasks::tasks::repo::{self, Change, NewTask};
    use tasks::tasks::types::{Actor, Ranking};

    async fn filed(pool: &MySqlPool, body: &str) -> u64 {
        repo::create(
            pool,
            NewTask {
                subject: "a body that grows".into(),
                checked: true,
                body: body.into(),
                priority: Ranking::Unassessed,
                due: None,
                blocked_on: Vec::new(),
                assignee: None,
                spare: None,
            },
            &Actor::Person("pippijn".into()),
        )
        .await
        .expect("filing")
        .id
    }

    async fn read_said(pool: &MySqlPool, id: u64, outcome: Outcome, said: Option<&str>) {
        checks::record(
            pool,
            "s-1",
            &Run {
                kind: Kind::Density,
                task_id: Some(id),
                input_chars: 18_162,
                accreted: Some(5_991),
                elapsed_ms: 33_735,
                outcome,
                subject_key: None,
                said: said.map(str::to_string),
            },
        )
        .await
        .expect("recording the read");
    }

    async fn rewrite(pool: &MySqlPool, id: u64, body: &str) {
        repo::update(
            pool,
            id,
            Change {
                body: Some(body.into()),
                replace_body: true,
                ..Default::default()
            },
            &Actor::Session("s-1".into()),
        )
        .await
        .expect("rewriting");
    }

    #[tokio::test]
    async fn what_a_read_said_outlives_the_tool_result() {
        let pool = common::fresh_db().await;
        let id = filed(&pool, &"x".repeat(4_000)).await;
        read_said(
            &pool,
            id,
            Outcome::Spoke,
            Some("Move 'Method note' bullets to a checklist AFTER 'What to do next'."),
        )
        .await;

        let detail = repo::get(&pool, id)
            .await
            .expect("reading")
            .expect("a task");
        assert_eq!(
            detail.sprawl_said.as_deref(),
            Some("Move 'Method note' bullets to a checklist AFTER 'What to do next'."),
            "the critique did not survive the call that recorded the run"
        );
        assert_eq!(
            detail.task.sprawl_chars,
            Some(18_162),
            "a digest line needs the number, and it is the size that was read"
        );
    }

    #[tokio::test]
    async fn a_shrinking_edit_is_what_retires_it() {
        let pool = common::fresh_db().await;
        let id = filed(&pool, &"x".repeat(4_000)).await;
        read_said(&pool, id, Outcome::Spoke, Some("this sprawls")).await;

        // Longer: a rewrite that adds is not a consolidation, and the flag has
        // to survive it or every append would clear its own warning.
        rewrite(&pool, id, &"y".repeat(5_000)).await;
        let still = repo::get(&pool, id)
            .await
            .expect("reading")
            .expect("a task");
        assert_eq!(
            still.task.sprawl_chars,
            Some(18_162),
            "growing the body cleared the flag about the body growing"
        );

        rewrite(&pool, id, &"z".repeat(1_200)).await;
        let done = repo::get(&pool, id)
            .await
            .expect("reading")
            .expect("a task");
        assert_eq!(
            done.sprawl_said, None,
            "a rewrite left the critique standing"
        );
        assert_eq!(done.task.sprawl_chars, None);
    }

    #[tokio::test]
    async fn a_timeout_must_not_retire_a_finding_nobody_addressed() {
        // 37 of 268 reads timed out. A timeout means the body was never judged,
        // so treating it as silence would let a slow model clear a flag — the
        // exact `Quiet` versus `Timeout` confusion this file opens with, now
        // with a write behind it.
        let pool = common::fresh_db().await;
        let id = filed(&pool, &"x".repeat(4_000)).await;
        read_said(&pool, id, Outcome::Spoke, Some("this sprawls")).await;

        for missed in [Outcome::Timeout, Outcome::Error] {
            read_said(&pool, id, missed, None).await;
            let after = repo::get(&pool, id)
                .await
                .expect("reading")
                .expect("a task");
            assert_eq!(
                after.sprawl_said.as_deref(),
                Some("this sprawls"),
                "{missed:?} retired a finding that was never re-judged"
            );
        }
    }

    #[tokio::test]
    async fn a_later_verdict_replaces_an_earlier_one() {
        let pool = common::fresh_db().await;
        let id = filed(&pool, &"x".repeat(4_000)).await;
        read_said(
            &pool,
            id,
            Outcome::Spoke,
            Some("the conclusion is at the bottom"),
        )
        .await;
        read_said(
            &pool,
            id,
            Outcome::Spoke,
            Some("section 3 is superseded by section 5"),
        )
        .await;

        let detail = repo::get(&pool, id)
            .await
            .expect("reading")
            .expect("a task");
        assert_eq!(
            detail.sprawl_said.as_deref(),
            Some("section 3 is superseded by section 5"),
            "this is the last thing said about the body, not a log of opinions"
        );

        // DENSE is a verdict about the body as it stands, and outranks the older
        // one. It cannot be summoned: the read only fires on 3,000 characters of
        // fresh accretion, so the cheapest route to it is making the body worse.
        read_said(&pool, id, Outcome::Quiet, None).await;
        let quiet = repo::get(&pool, id)
            .await
            .expect("reading")
            .expect("a task");
        assert_eq!(quiet.sprawl_said, None);
    }
}
