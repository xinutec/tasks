//! Filing past a duplicate refusal: only one this session was given, only by
//! naming what it named, and always on the record.
//!
//! Through the router, because the rules live in the service: a CLI that
//! skipped them, or a script calling the API, must meet the same answer.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use sqlx::MySqlPool;
use tasks::config::{AuthConfig, Config};
use tasks::routes;
use tasks::state::AppState;
use tasks::tasks::checks::{self, Kind, Outcome, Run};
use tower::ServiceExt;

const TOKEN: &str = "test-agent-token";
const SESSION: &str = "sess-overrule";
const SUBJECT: &str = "the harness renders every route from the model";

fn app(pool: MySqlPool) -> axum::Router {
    let cfg = Config {
        database_url: String::new(),
        bind_addr: "127.0.0.1:0".into(),
        static_dir: None,
        auth: Some(AuthConfig {
            session_secret: "test-secret".into(),
            nc_base_url: "https://dash.example".into(),
            nc_internal_url: None,
            nc_client_id: "id".into(),
            nc_client_secret: "secret".into(),
            nc_redirect_uri: "https://tasks.example/auth/callback".into(),
            allowed_users: vec!["user".into()],
        }),
        agent_token: Some(TOKEN.into()),
    };
    routes::router(AppState::new(cfg, pool, reqwest::Client::new()))
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .uri(uri)
        .method(method)
        .header("Authorization", format!("Bearer {TOKEN}"))
        .header("X-Session-Id", SESSION)
        .header("Content-Type", "application/json")
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
        .unwrap();
    let res = app.clone().oneshot(req).await.expect("the router answered");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 256 * 1024)
        .await
        .expect("a body");
    let said = match bytes.is_empty() {
        true => Value::Null,
        false => serde_json::from_slice(&bytes).expect("the service answers in JSON"),
    };
    (status, said)
}

/// A refusal of [`SUBJECT`] given to [`SESSION`], naming `matched`.
async fn refused(pool: &MySqlPool, kind: Kind, matched: Vec<u64>) {
    checks::record(
        pool,
        SESSION,
        &Run {
            kind,
            task_id: None,
            input_chars: 0,
            accreted: None,
            elapsed_ms: 0,
            outcome: Outcome::Spoke,
            subject_key: Some(checks::subject_key(SUBJECT)),
            matched,
            said: None,
        },
    )
    .await
    .expect("recording the refusal");
}

fn filing(not_duplicate_of: &[u64]) -> Value {
    json!({
        "subject": SUBJECT,
        "priority": "P3",
        "checked": false,
        "not_duplicate_of": not_duplicate_of,
    })
}

#[tokio::test]
async fn nothing_refused_means_nothing_to_overrule() {
    let app = app(common::fresh_db().await);
    let (status, said) = call(&app, "POST", "/api/tasks", Some(filing(&[7]))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{said}");
    assert!(said.to_string().contains("nothing has refused"), "{said}");
}

/// The licence says what the refusal named, so the CLI can tell a session
/// what it still has to judge.
#[tokio::test]
async fn the_licence_carries_what_the_refusal_named() {
    let pool = common::fresh_db().await;
    let app = app(pool.clone());
    let uri = format!("/api/checks/licence?subject={}", urlencode(SUBJECT));
    let (_, before) = call(&app, "GET", &uri, None).await;
    assert_eq!(before["matched"], Value::Null, "{before}");
    refused(&pool, Kind::Filing, vec![2037, 12]).await;
    let (status, after) = call(&app, "GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert_eq!(after["matched"], json!([12, 2037]), "{after}");
}

#[tokio::test]
async fn an_overrule_naming_what_was_named_files_and_is_recorded() {
    let pool = common::fresh_db().await;
    let app = app(pool.clone());
    refused(&pool, Kind::Filing, vec![2037]).await;
    let (status, filed) = call(&app, "POST", "/api/tasks", Some(filing(&[2037]))).await;
    assert_eq!(status, StatusCode::OK, "{filed}");
    let id = filed["id"].as_u64().expect("an id");
    let (_, shown) = call(&app, "GET", &format!("/api/tasks/{id}"), None).await;
    let overrode: Vec<&Value> = shown["events"]
        .as_array()
        .expect("events")
        .iter()
        .filter(|e| e["kind"] == "overrode")
        .collect();
    assert_eq!(overrode.len(), 1, "{shown}");
    assert!(
        overrode[0]["detail"]
            .as_str()
            .unwrap_or("")
            .contains("#2037"),
        "{shown}"
    );
}

/// Each named task is a judgment, so each must be named. A bare skip is
/// how a script files past every refusal without reading one.
#[tokio::test]
async fn an_overrule_must_name_every_task_the_refusal_named() {
    let pool = common::fresh_db().await;
    let app = app(pool.clone());
    refused(&pool, Kind::Filing, vec![2037, 12]).await;
    let (status, said) = call(&app, "POST", "/api/tasks", Some(filing(&[2037]))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{said}");
    assert!(said.to_string().contains("#12"), "{said}");
    let (status, said) = call(&app, "POST", "/api/tasks", Some(filing(&[]))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "an unnamed skip: {said}");
}

/// The exact-subject refusal offers the override too, so it must license it.
#[tokio::test]
async fn an_exact_subject_refusal_licenses_its_overrule() {
    let pool = common::fresh_db().await;
    let app = app(pool.clone());
    refused(&pool, Kind::Collision, vec![1206]).await;
    let (status, filed) = call(&app, "POST", "/api/tasks", Some(filing(&[1206]))).await;
    assert_eq!(status, StatusCode::OK, "{filed}");
}

fn urlencode(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes()).collect()
}

/// The CLI's half: whether a filing may skip the check, before anything is
/// sent. The service enforces the same rule; this is what keeps the CLI from
/// skipping a check the service would then refuse, or running one it needn't.
mod deciding {
    use tasks::tasks::checks::{Overrule, overrule};

    #[test]
    fn no_flag_always_checks_even_with_a_licence() {
        assert_eq!(overrule(false, &[], Some(&[2037])), Overrule::Check);
    }

    /// Asking to skip first time is harmless: the check runs.
    #[test]
    fn a_flag_with_nothing_refused_checks() {
        assert_eq!(overrule(true, &[2037], None), Overrule::Check);
    }

    #[test]
    fn naming_every_refused_task_skips() {
        assert_eq!(
            overrule(true, &[12, 2037], Some(&[2037, 12])),
            Overrule::Skip
        );
    }

    #[test]
    fn leaving_one_out_names_what_is_missing() {
        assert_eq!(
            overrule(true, &[2037], Some(&[2037, 12])),
            Overrule::Unnamed(vec![12])
        );
    }

    /// The bare `--no-duplicate-check` names nothing, so it can't skip a
    /// refusal that named something.
    #[test]
    fn a_bare_flag_cannot_skip_a_refusal_that_named_tasks() {
        assert_eq!(
            overrule(true, &[], Some(&[2037])),
            Overrule::Unnamed(vec![2037])
        );
    }

    /// A refusal recorded before matches were kept names nothing, so any
    /// override of it is complete.
    #[test]
    fn a_refusal_that_named_nothing_is_skipped_by_any_override() {
        assert_eq!(overrule(true, &[], Some(&[])), Overrule::Skip);
    }
}
