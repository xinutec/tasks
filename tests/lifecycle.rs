//! Which closed tasks a filing names that its filer recently closed.
//!
//! The case this exists for: a session closes a task, finds more of the same
//! work, and files it new instead of reopening. The duplicate check compares
//! wording and misses it when the two subjects share none; an id reference is
//! exact.
//!
//! The last test goes through the router: the query being right is not the same
//! as `create` calling it.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tasks::config::{AuthConfig, Config};
use tasks::routes;
use tasks::state::AppState;
use tower::ServiceExt;

use tasks::tasks::lifecycle::{self, mentioned};
use tasks::tasks::repo::{self, Change, NewTask};
use tasks::tasks::types::{Actor, Priority, Ranking, Status};

fn filer() -> Actor {
    Actor::Person("pippijn".into())
}

fn other() -> Actor {
    Actor::Person("someone-else".into())
}

async fn file(pool: &sqlx::MySqlPool, subject: &str) -> u64 {
    repo::create(
        pool,
        NewTask {
            subject: subject.into(),
            checked: true,
            body: String::new(),
            priority: Ranking::At(Priority::P2),
            due: None,
            blocked_on: Vec::new(),
            assignee: None,
            spare: None,
        },
        &filer(),
    )
    .await
    .expect("filing")
    .id
}

async fn set(pool: &sqlx::MySqlPool, id: u64, status: Status, by: &Actor) {
    repo::update(
        pool,
        id,
        Change {
            status: Some(status),
            ..Change::default()
        },
        by,
    )
    .await
    .expect("changing status");
}

async fn named(pool: &sqlx::MySqlPool, id: u64) -> Vec<u64> {
    lifecycle::closed_by(pool, &filer(), &[id])
        .await
        .expect("looking")
        .into_iter()
        .map(|closed| closed.id)
        .collect()
}

#[test]
fn only_the_hashed_spelling_is_a_reference() {
    // `## Why` is a heading and 8080 a port; `#89` twice is one reference.
    let text = "## Why\ncontinues #89 (see #1635, and #89 again) on port 8080. #x #";
    assert_eq!(mentioned(text), vec![89, 1635]);
}

#[tokio::test]
async fn a_task_the_filer_just_closed_is_named() {
    let pool = common::fresh_db().await;
    let done = file(&pool, "done work").await;
    set(&pool, done, Status::Done, &filer()).await;
    let dropped = file(&pool, "dropped work").await;
    set(&pool, dropped, Status::Dropped, &filer()).await;

    let found = lifecycle::closed_by(&pool, &filer(), &[done, dropped])
        .await
        .expect("looking");
    let statuses: Vec<(u64, Status)> = found.iter().map(|c| (c.id, c.status)).collect();
    assert_eq!(
        statuses,
        vec![(done, Status::Done), (dropped, Status::Dropped)]
    );
}

#[tokio::test]
async fn an_open_task_is_not_named() {
    let pool = common::fresh_db().await;
    let open = file(&pool, "still open").await;
    assert!(named(&pool, open).await.is_empty());
}

#[tokio::test]
async fn somebody_elses_close_is_not_named() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "closed by another").await;
    set(&pool, id, Status::Done, &other()).await;
    assert!(named(&pool, id).await.is_empty());
}

#[tokio::test]
async fn the_last_close_decides_not_the_first() {
    // The filer closed it, somebody reopened it and closed it again: whether it
    // continues is that closer's call now.
    let pool = common::fresh_db().await;
    let id = file(&pool, "closed twice").await;
    set(&pool, id, Status::Done, &filer()).await;
    set(&pool, id, Status::Open, &other()).await;
    set(&pool, id, Status::Done, &other()).await;
    assert!(named(&pool, id).await.is_empty());
}

#[tokio::test]
async fn a_close_older_than_a_day_is_not_named() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "closed long ago").await;
    set(&pool, id, Status::Done, &filer()).await;
    sqlx::query("UPDATE task_events SET at = NOW() - INTERVAL 2 DAY WHERE task_id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .expect("backdating");
    assert!(named(&pool, id).await.is_empty());
}

#[tokio::test]
async fn an_id_that_does_not_exist_is_not_named() {
    let pool = common::fresh_db().await;
    assert!(named(&pool, 999_999).await.is_empty());
}

#[test]
fn the_hint_names_both_halves_of_the_remedy() {
    let at = chrono::DateTime::parse_from_rfc3339("2026-09-26T10:00:00Z")
        .unwrap()
        .to_utc();
    let closed = lifecycle::Closed {
        id: 89,
        status: Status::Done,
        at,
    };
    let later = at + chrono::Duration::minutes(40);
    assert_eq!(
        lifecycle::reopen_hint(&closed, 1700, later),
        "#89 was closed (done) by you 40 min ago. If this continues it: \
         `task reopen 89` and `task drop 1700 --reason \"continues #89\"`."
    );
    let hint = lifecycle::reopen_hint(&closed, 1700, at + chrono::Duration::minutes(150));
    assert!(hint.contains("by you 2 h ago"), "{hint}");
}

const TOKEN: &str = "test-agent-token";

async fn send(app: &axum::Router, method: &str, uri: &str, body: &str) -> serde_json::Value {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .method(method)
                .header("Authorization", format!("Bearer {TOKEN}"))
                .header("X-Session-Id", "sess-a")
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .expect("the router answered");
    let status = res.status();
    let body = axum::body::to_bytes(res.into_body(), 256 * 1024)
        .await
        .expect("a body");
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    serde_json::from_slice(&body).expect("json")
}

#[tokio::test]
async fn filing_through_the_api_names_what_this_session_just_closed() {
    let pool = common::fresh_db().await;
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
            allowed_users: vec!["pippijn".into()],
        }),
        agent_token: Some(TOKEN.into()),
    };
    let app = routes::router(AppState::new(cfg, pool, reqwest::Client::new()));

    let first = send(
        &app,
        "POST",
        "/api/tasks",
        r#"{"subject":"the first bug","priority":"P2"}"#,
    )
    .await;
    let first = first["id"].as_u64().expect("an id");
    assert!(first_closed_is_absent(
        &send(
            &app,
            "POST",
            "/api/tasks",
            r#"{"subject":"unrelated","priority":"P2"}"#
        )
        .await
    ));
    send(
        &app,
        "PATCH",
        &format!("/api/tasks/{first}"),
        r#"{"status":"done"}"#,
    )
    .await;

    let filed = send(
        &app,
        "POST",
        "/api/tasks",
        &format!(r#"{{"subject":"four more bugs","priority":"P2","body":"found after #{first}"}}"#),
    )
    .await;
    assert_eq!(filed["closed"][0]["id"].as_u64(), Some(first), "{filed}");
    assert_eq!(filed["closed"][0]["status"], "done", "{filed}");
}

/// A filing that names nothing closed carries no `closed` key at all.
fn first_closed_is_absent(filed: &serde_json::Value) -> bool {
    filed.get("closed").is_none() && filed["id"].is_u64()
}

async fn change(
    pool: &sqlx::MySqlPool,
    id: u64,
    change: Change,
) -> Option<chrono::DateTime<chrono::Utc>> {
    repo::update(pool, id, change, &filer())
        .await
        .expect("changing")
        .unwritten
}

fn status(status: Status) -> Change {
    Change {
        status: Some(status),
        ..Change::default()
    }
}

fn body(text: &str) -> Change {
    Change {
        body: Some(text.into()),
        ..Change::default()
    }
}

#[tokio::test]
async fn closing_a_task_never_written_since_filing_is_named() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "never rewritten").await;
    assert!(change(&pool, id, status(Status::Done)).await.is_some());
}

#[tokio::test]
async fn a_drop_is_a_close_too() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "dropped unwritten").await;
    assert!(change(&pool, id, status(Status::Dropped)).await.is_some());
}

#[tokio::test]
async fn an_edit_before_the_close_satisfies_it() {
    // What `task done --note` does: the note lands, then the close.
    let pool = common::fresh_db().await;
    let id = file(&pool, "rewritten").await;
    change(&pool, id, body("what was found")).await;
    assert_eq!(change(&pool, id, status(Status::Done)).await, None);
}

#[tokio::test]
async fn an_edit_and_a_close_in_one_change_satisfy_it() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "rewritten as it closes").await;
    let both = Change {
        body: Some("what was found".into()),
        status: Some(Status::Done),
        ..Change::default()
    };
    assert_eq!(change(&pool, id, both).await, None);
}

#[tokio::test]
async fn an_edit_before_the_work_started_does_not() {
    // The plan was written, the work began, and the close said nothing of it.
    let pool = common::fresh_db().await;
    let id = file(&pool, "planned, then done silently").await;
    change(&pool, id, body("the plan")).await;
    change(&pool, id, status(Status::Doing)).await;
    assert!(change(&pool, id, status(Status::Done)).await.is_some());
}

#[tokio::test]
async fn an_edit_after_the_work_started_satisfies_it() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "started, rewritten, done").await;
    change(&pool, id, status(Status::Doing)).await;
    change(&pool, id, body("what was found")).await;
    assert_eq!(change(&pool, id, status(Status::Done)).await, None);
}

#[tokio::test]
async fn a_reopened_task_closed_again_unwritten_is_named() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "closed, reopened, closed").await;
    change(&pool, id, body("the first finding")).await;
    change(&pool, id, status(Status::Done)).await;
    change(&pool, id, status(Status::Open)).await;
    assert!(change(&pool, id, status(Status::Done)).await.is_some());
}

#[tokio::test]
async fn a_change_that_does_not_close_is_never_named() {
    let pool = common::fresh_db().await;
    let id = file(&pool, "only started").await;
    assert_eq!(change(&pool, id, status(Status::Doing)).await, None);
}

#[test]
fn the_rewrite_hint_says_how_long_the_text_has_stood() {
    let at = chrono::DateTime::parse_from_rfc3339("2026-09-20T10:00:00Z")
        .unwrap()
        .to_utc();
    assert_eq!(
        lifecycle::rewrite_hint(89, at, at + chrono::Duration::days(3)),
        "#89's text was last written 3 days ago, before this work. Closing is a rewrite: \
         `task edit 89 --body -` to say what was found."
    );
}
