//! What a refused request body says back, driven through the real router.
//!
//! ⚠ **A rule the service will not state is half a rule.** `priority` is the one
//! key a filing may not leave out, and `null` — *unassessed, nobody has judged
//! this* — is a legal answer to it. Serde's own refusal names the field and
//! stops: it reads as *you must pick a level*, which is the opposite of the
//! design, and the escape it never mentions is the whole reason the field is
//! required rather than defaulted. #724 set the bar on the other side of this —
//! an unknown holder answers 400, names it, and says how to find a real one.
//!
//! Who actually reads these: not the CLI (clap refuses first) and not the app
//! (the button stays disabled), so this is the bare-API path — a hand-written
//! script, or **a phone still running a pre-`5dce9b6` bundle**, which posts
//! without the key and gets whatever this file pins.
//!
//! A refused body never reaches the database — an extractor answers before the
//! handler runs — so these take a lazy pool pointed at nothing, the way
//! `tests/access.rs` does. Two tests are deliberately the other way round and
//! say so: `a_change_may_still_leave_priority_out` uses the unreachable pool as
//! its evidence, and `filing_a_whole_task_still_works` takes the real one.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::MySqlPool;
use tasks::config::{AuthConfig, Config};
use tasks::routes;
use tasks::state::AppState;
use tower::ServiceExt;

const TOKEN: &str = "test-agent-token";
const SESSION: &str = "sess-wire";

fn config(database_url: &str) -> Config {
    Config {
        database_url: database_url.into(),
        bind_addr: "127.0.0.1:0".into(),
        static_dir: None,
        auth: Some(AuthConfig {
            session_secret: "test-secret".into(),
            nc_base_url: "https://dash.example".into(),
            nc_internal_url: None,
            nc_client_id: "id".into(),
            nc_client_secret: "secret".into(),
            nc_redirect_uri: "https://tasks.example/auth/callback".into(),
            allowed_users: vec!["pippijn".into()],
        }),
        agent_token: Some(TOKEN.into()),
    }
}

/// An app whose pool is never dialled — every request below is refused first.
fn app() -> axum::Router {
    let cfg = config("mysql://unused:unused@127.0.0.1:1/unused");
    let pool = MySqlPool::connect_lazy(&cfg.database_url).expect("a lazy pool");
    routes::router(AppState::new(cfg, pool, reqwest::Client::new()))
}

async fn post(app: &axum::Router, path: &str, body: &str) -> (StatusCode, String) {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .method("POST")
                .header("Authorization", format!("Bearer {TOKEN}"))
                .header("X-Session-Id", SESSION)
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .expect("the router answered");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 64 * 1024)
        .await
        .expect("a body");
    (status, String::from_utf8_lossy(&bytes).to_string())
}

/// The point of the whole file: both legal answers, in the refusal.
#[tokio::test]
async fn filing_without_priority_names_both_legal_answers() {
    let (status, body) = post(&app(), "/api/tasks", r#"{"subject":"Something"}"#).await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a body this service can name the fault in is a 400, not serde's 422: {body}"
    );
    assert!(body.contains("priority"), "name the key: {body}");
    assert!(
        body.contains("P0") && body.contains("P4"),
        "say what a level looks like: {body}"
    );
    assert!(
        body.contains("null") && body.contains("unassessed"),
        "THE ESCAPE. Without it the message reads as `you must pick a level`, \
         which is the opposite of the rule: {body}"
    );
}

/// The refusal is this service's shape, so a client parses one error format.
#[tokio::test]
async fn a_refused_body_answers_in_the_services_own_error_shape() {
    let (_, body) = post(&app(), "/api/tasks", r#"{"subject":"Something"}"#).await;
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("JSON back: {body}");
    assert!(
        parsed.get("error").and_then(|e| e.as_str()).is_some(),
        "every other error here is {{\"error\": ...}}: {body}"
    );
}

/// ⚠ **The ablation.** Delete the missing-key arm and the test above fails —
/// but so would a version that answered the same 400 to *everything*, which
/// would be worse than serde: it would name a fault the caller does not have.
/// A key that is present and wrong must still be refused for what it is.
#[tokio::test]
async fn a_priority_that_is_present_and_wrong_is_refused_for_that() {
    let (status, body) = post(
        &app(),
        "/api/tasks",
        r#"{"subject":"Something","priority":"P9"}"#,
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        !body.contains("is required"),
        "the key WAS given — telling this caller it is required sends them to \
         check the one thing they got right: {body}"
    );
    assert!(
        body.contains("P9") || body.contains("unknown") || body.contains("expected"),
        "say what was wrong with the value given: {body}"
    );
}

/// Absence still means *leave it alone* on a change, and that contrast is what
/// makes the filing rule legible rather than an inconsistency. A `PATCH` that
/// mentions no priority is a normal edit, not a refusal.
#[tokio::test]
async fn a_change_may_still_leave_priority_out() {
    let app = app();
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/tasks/1")
                .method("PATCH")
                .header("Authorization", format!("Bearer {TOKEN}"))
                .header("X-Session-Id", SESSION)
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"status":"done"}"#))
                .unwrap(),
        )
        .await
        .expect("the router answered");
    // ⚠ **500 is the PASS here, and it is asserted rather than merely allowed.**
    // The body got through extraction and the handler then went looking for a
    // database that is not there — which is exactly the evidence wanted, since
    // nothing downstream of the extractor can run until the extractor lets go.
    // `assert_ne!(BAD_REQUEST)` would have said the same thing about a 401,
    // i.e. it would still pass on the day this stopped reaching the handler at
    // all.
    assert_eq!(
        res.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "an edit that leaves priority alone never reached the handler — the \
         required-key rule has leaked onto the wrong body type"
    );
}

/// ⚠ **A gate on the extractor itself.** Everything above is about REFUSALS,
/// and an extractor that refused every body would pass all of it. This is the
/// one test here that needs the database, because accepting a filing means
/// writing it.
#[tokio::test]
async fn filing_a_whole_task_still_works() {
    let pool = common::fresh_db().await;
    let app = routes::router(AppState::new(
        config(&common::test_db_url()),
        pool,
        reqwest::Client::new(),
    ));

    let (status, body) = post(
        &app,
        "/api/tasks",
        r#"{"subject":"A filing that says how urgent it is","priority":"P3"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let task: serde_json::Value = serde_json::from_str(&body).expect("the task back");
    assert_eq!(task["priority"], "P3");
    assert_eq!(task["subject"], "A filing that says how urgent it is");
}
