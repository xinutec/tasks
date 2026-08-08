//! Who gets in, and as whom.
//!
//! Driven through the real router in-process, because the person/session split
//! is a security property and should not rest on reading the code. None of
//! these requests reaches the database — the pool is lazy and the handlers here
//! answer before touching it — which is deliberate: an access rule that can only
//! be tested with a server running is one that stops being tested.

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

/// A configured app: the login wall up, the agent door open.
fn app(agent_token: Option<&str>) -> axum::Router {
    let cfg = Config {
        // Never connected to: `connect_lazy` dials on first use, and no request
        // in this file gets that far.
        database_url: "mysql://unused:unused@127.0.0.1:1/unused".into(),
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
        agent_token: agent_token.map(str::to_string),
    };
    let pool = MySqlPool::connect_lazy(&cfg.database_url).expect("a lazy pool");
    routes::router(AppState::new(cfg, pool, reqwest::Client::new()))
}

async fn send(app: &axum::Router, req: Request<Body>) -> (StatusCode, String) {
    let res = app.clone().oneshot(req).await.expect("the router answered");
    let status = res.status();
    let body = axum::body::to_bytes(res.into_body(), 64 * 1024)
        .await
        .expect("a body");
    (status, String::from_utf8_lossy(&body).to_string())
}

fn get(path: &str) -> axum::http::request::Builder {
    Request::builder().uri(path).method("GET")
}

#[tokio::test]
async fn no_credential_gets_nothing() {
    let app = app(Some(TOKEN));
    let (status, _) = send(&app, get("/api/me").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_wrong_token_gets_nothing() {
    let app = app(Some(TOKEN));
    let (status, _) = send(
        &app,
        get("/api/me")
            .header("Authorization", "Bearer not-the-token")
            .header("X-Session-Id", "sess-1")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_token_without_a_session_id_gets_nothing() {
    // The token says which machine; the header says which conversation. One
    // without the other cannot be filed against anybody, and a write filed
    // against nobody is exactly what the history must never contain.
    let app = app(Some(TOKEN));
    let (status, body) = send(
        &app,
        get("/api/me")
            .header("Authorization", format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // And it says which half is missing. Refusing a *correct* token with "not
    // authenticated" sent somebody to re-check the one thing that was right,
    // which is how `task list` came to look like a broken token.
    assert!(body.contains("X-Session-Id"), "{body}");
    assert!(!body.contains("not authenticated"), "{body}");
}

#[tokio::test]
async fn no_credential_still_just_says_no() {
    // The other half of the pair above: nothing was offered, so there is no
    // remedy to name and the generic answer is the honest one.
    let app = app(Some(TOKEN));
    let (status, body) = send(&app, get("/api/me").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("not authenticated"), "{body}");
}

#[tokio::test]
async fn a_nameless_token_does_not_fall_through_to_a_cookie() {
    // It must not borrow a signed-in browser's identity to get in: that is the
    // same misattribution `a_session_credential_beats_a_cookie_that_rode_along`
    // guards from the other side.
    let app = app(Some(TOKEN));
    let cookie = create_session(
        SECRET,
        &UserSession {
            user_id: "pippijn".into(),
            display_name: "Pippijn".into(),
        },
    );
    let (status, _) = send(
        &app,
        get("/api/me")
            .header("Cookie", format!("session={cookie}"))
            .header("Authorization", format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_unset_token_closes_the_agent_door_entirely() {
    // A deployment that forgot the secret must not admit an empty bearer.
    let app = app(None);
    for offered in ["Bearer ", "Bearer x"] {
        let (status, _) = send(
            &app,
            get("/api/me")
                .header("Authorization", offered)
                .header("X-Session-Id", "sess-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "admitted {offered:?}");
    }
}

#[tokio::test]
async fn a_session_is_answered_as_itself() {
    let app = app(Some(TOKEN));
    let (status, body) = send(
        &app,
        get("/api/me")
            .header("Authorization", format!("Bearer {TOKEN}"))
            .header("X-Session-Id", "sess-1")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"kind\":\"session\""), "{body}");
    assert!(body.contains("sess-1"), "{body}");
}

#[tokio::test]
async fn the_person_is_answered_as_the_person() {
    let app = app(Some(TOKEN));
    let cookie = create_session(
        SECRET,
        &UserSession {
            user_id: "pippijn".into(),
            display_name: "Pippijn".into(),
        },
    );
    let (status, body) = send(
        &app,
        get("/api/me")
            .header("Cookie", format!("session={cookie}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"kind\":\"person\""), "{body}");
}

#[tokio::test]
async fn a_session_credential_beats_a_cookie_that_rode_along() {
    // Otherwise a session driven from a signed-in machine would file its
    // history under Pippijn's name, and the record would be a fiction.
    let app = app(Some(TOKEN));
    let cookie = create_session(
        SECRET,
        &UserSession {
            user_id: "pippijn".into(),
            display_name: "Pippijn".into(),
        },
    );
    let (status, body) = send(
        &app,
        get("/api/me")
            .header("Cookie", format!("session={cookie}"))
            .header("Authorization", format!("Bearer {TOKEN}"))
            .header("X-Session-Id", "sess-1")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"kind\":\"session\""), "{body}");
}

#[tokio::test]
async fn a_session_may_not_rename_another() {
    let app = app(Some(TOKEN));
    let (status, _) = send(
        &app,
        Request::builder()
            .uri("/api/sessions/sess-2")
            .method("PATCH")
            .header("Authorization", format!("Bearer {TOKEN}"))
            .header("X-Session-Id", "sess-1")
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"name":"stolen"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn healthz_answers_without_a_credential() {
    // kubelet has none, and a probe behind the wall is a pod that never
    // becomes ready and a deploy that fails for the wrong reason.
    let app = app(Some(TOKEN));
    let (status, body) = send(&app, get("/healthz").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "ok");
}
