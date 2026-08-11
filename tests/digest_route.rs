//! What the `/api/digest` ROUTE selects, driven through the real router.
//!
//! ⚠ **The filter being right is not the same as the route using it.**
//! `tests/tasks_db.rs` proves `Filter::digest_for` returns a session's own tasks
//! and the pile. What decides to *call* it is one `match` in `routes::api`, and
//! that line is the whole property: change it to `Filter::open_in` and every
//! other test in this repository still passes while every session silently goes
//! back to carrying every open task there is — 12,371 bytes a turn,
//! measured, against a few hundred. That is the same shape as the
//! `status <> 'done'` bug: one edit, nothing fails, the numbers are just wrong.
//!
//! So these go through HTTP. `tests/access.rs` drives the router with a lazy
//! pool because none of its requests reach the database; these do, so they take
//! the real one.
//!
//! `/api/tasks` is here for the same reason and not a different one. `task
//! list` defaults to the caller's own work plus the pile, and it gets there by
//! sending `pile=true`; a route that dropped the parameter would answer with a
//! session's bare plate, the pile would go quiet, and the five unit tests in
//! `src/bin/task.rs` — which only check what the CLI *sends* — would all still
//! pass.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::MySqlPool;
use tasks::config::{AuthConfig, Config};
use tasks::routes;
use tasks::session::{UserSession, create_session};
use tasks::sessions;
use tasks::state::AppState;
use tasks::tasks::repo::{self, NewTask};
use tasks::tasks::types::{Actor, Assignee, AssigneeKind, Ranking};
use tower::ServiceExt;

const SECRET: &str = "test-secret";
const TOKEN: &str = "test-agent-token";

/// The real router over the real test database.
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

/// The digest as a given session receives it.
async fn digest_as_session(app: &axum::Router, session: &str) -> String {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/digest")
                .method("GET")
                .header("Authorization", format!("Bearer {TOKEN}"))
                .header("X-Session-Id", session)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("the router answered");
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 64 * 1024)
        .await
        .expect("a body");
    String::from_utf8_lossy(&body).to_string()
}

/// The digest as Pippijn receives it, naming no session.
async fn digest_as_owner(app: &axum::Router) -> String {
    let cookie = create_session(
        SECRET,
        &UserSession {
            user_id: "pippijn".into(),
            display_name: "Pippijn".into(),
        },
    );
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/digest")
                .method("GET")
                .header("Cookie", format!("session={cookie}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("the router answered");
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 64 * 1024)
        .await
        .expect("a body");
    String::from_utf8_lossy(&body).to_string()
}

/// One task in each of the four holdings a digest has to tell apart.
async fn seed(pool: &MySqlPool) {
    for (id, name) in [("sess-1", "tasks"), ("sess-2", "memview")] {
        sessions::touch(pool, id, Some(name))
            .await
            .expect("recording a session");
    }
    for (subject, holder) in [
        (
            "MINE to do",
            Assignee {
                kind: AssigneeKind::Session,
                id: Some("sess-1".into()),
                name: None,
            },
        ),
        (
            "ANOTHER conversation has this",
            Assignee {
                kind: AssigneeKind::Session,
                id: Some("sess-2".into()),
                name: None,
            },
        ),
        (
            "PIPPIJN is holding this",
            Assignee {
                kind: AssigneeKind::Person,
                id: Some("pippijn".into()),
                name: None,
            },
        ),
        ("PILE, for whoever picks it up", Assignee::nobody()),
    ] {
        repo::create(
            pool,
            NewTask {
                subject: subject.into(),
                body: String::new(),
                priority: Ranking::Unassessed,
                due: None,
                blocked_on: Vec::new(),
                assignee: Some(holder),
            },
            &Actor::Person("pippijn".into()),
        )
        .await
        .expect("filing");
    }
}

#[tokio::test]
async fn the_route_hands_a_session_its_own_work_and_the_pile() {
    let pool = common::fresh_db().await;
    seed(&pool).await;
    let app = app(pool);

    let digest = digest_as_session(&app, "sess-1").await;
    assert!(digest.contains("MINE to do"), "own work missing:\n{digest}");
    assert!(
        digest.contains("PILE, for whoever picks it up"),
        "the pile fell out — it is the handover channel:\n{digest}"
    );
    assert!(
        !digest.contains("ANOTHER conversation"),
        "another session's work reached this prompt:\n{digest}"
    );
    assert!(
        !digest.contains("PIPPIJN is holding this"),
        "the person's own work reached a session's prompt:\n{digest}"
    );
}

#[tokio::test]
async fn the_route_selects_per_session_rather_than_once_for_everybody() {
    // Two sessions, one request each, from the same router and the same rows.
    // A route that had stopped filtering would give them identical bodies, and
    // both of the assertions above would still pass for `sess-1`.
    let pool = common::fresh_db().await;
    seed(&pool).await;
    let app = app(pool);

    let one = digest_as_session(&app, "sess-1").await;
    let two = digest_as_session(&app, "sess-2").await;
    assert_ne!(
        one, two,
        "two sessions received the same digest, so nothing is selecting per session"
    );
    assert!(
        two.contains("ANOTHER conversation"),
        "sess-2's own work:\n{two}"
    );
    assert!(
        !two.contains("MINE to do"),
        "sess-1's work reached sess-2:\n{two}"
    );
}

#[tokio::test]
async fn a_person_naming_no_session_still_sees_everything() {
    // `task digest` for measuring the cost, and the web app's own reading of it.
    // There is no "own" to narrow to, so narrowing would just hide rows.
    let pool = common::fresh_db().await;
    seed(&pool).await;
    let app = app(pool);

    let digest = digest_as_owner(&app).await;
    for subject in [
        "MINE to do",
        "ANOTHER conversation has this",
        "PIPPIJN is holding this",
        "PILE, for whoever picks it up",
    ] {
        assert!(digest.contains(subject), "{subject} missing:\n{digest}");
    }
}

/// The subjects `/api/tasks` answers with, for an arbitrary query string.
async fn list_as_session(app: &axum::Router, session: &str, query: &str) -> Vec<String> {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/tasks?{query}"))
                .method("GET")
                .header("Authorization", format!("Bearer {TOKEN}"))
                .header("X-Session-Id", session)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("the router answered");
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 256 * 1024)
        .await
        .expect("a body");
    serde_json::from_slice::<Vec<serde_json::Value>>(&body)
        .expect("a list of tasks")
        .into_iter()
        .map(|t| t["subject"].as_str().expect("a subject").to_string())
        .collect()
}

#[tokio::test]
async fn the_list_route_honours_the_pile_parameter() {
    // What a bare `task list` sends. If the route ignores `pile`, this returns
    // one task instead of two and a session stops being able to see work left
    // for whichever conversation is around — silently, since a shorter list
    // reads as less work rather than as a broken query.
    let pool = common::fresh_db().await;
    seed(&pool).await;
    let app = app(pool);

    let mine_and_pile = list_as_session(&app, "sess-1", "session=sess-1&pile=true").await;
    assert!(mine_and_pile.iter().any(|s| s == "MINE to do"));
    assert!(
        mine_and_pile
            .iter()
            .any(|s| s == "PILE, for whoever picks it up"),
        "the pile fell out of a bare `task list`: {mine_and_pile:?}"
    );
    assert!(
        !mine_and_pile.iter().any(|s| s.contains("ANOTHER")),
        "another session's work: {mine_and_pile:?}"
    );
}

#[tokio::test]
async fn mine_stays_strictly_mine() {
    // The narrower question has to keep answering narrowly: `--mine` sends no
    // `pile`, and the default for the parameter is what enforces that.
    let pool = common::fresh_db().await;
    seed(&pool).await;
    let app = app(pool);

    let mine = list_as_session(&app, "sess-1", "session=sess-1").await;
    assert_eq!(mine, vec!["MINE to do".to_string()], "not strictly mine");
}

#[tokio::test]
async fn all_is_still_reachable() {
    // `--all` sends no session at all. It is the one way left to ask what the
    // fleet is doing, so it must not be narrowed by the credential — which
    // names a session on every request the CLI makes.
    let pool = common::fresh_db().await;
    seed(&pool).await;
    let app = app(pool);

    let everything = list_as_session(&app, "sess-1", "").await;
    assert_eq!(everything.len(), 4, "{everything:?}");
}

/// A request names the session that sent it, without anybody typing it.
///
/// ⚠ **The header is the whole feature, and one line in each handler is what
/// uses it.** `sessions::touch(&app.db, id, None)` was the shape everywhere
/// until 2026-08-10, and it still compiles: drop the argument back to `None` and
/// every other test in this repository passes while every session goes back to
/// being a uuid. That is the same edit-one-line-fail-nothing shape as the filter
/// above, so it is tested through HTTP for the same reason.
mod naming {
    use super::*;

    async fn digest_called(app: &axum::Router, session: &str, called: Option<&str>) {
        let mut req = Request::builder()
            .uri("/api/digest")
            .method("GET")
            .header("Authorization", format!("Bearer {TOKEN}"))
            .header("X-Session-Id", session);
        if let Some(called) = called {
            req = req.header("X-Session-Name", called);
        }
        let res = app
            .clone()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .expect("the router answered");
        assert_eq!(res.status(), StatusCode::OK);
    }

    async fn stored_name(pool: &MySqlPool, session: &str) -> Option<String> {
        sessions::list(pool)
            .await
            .expect("the session list")
            .into_iter()
            .find(|s| s.id == session)
            .and_then(|s| s.name)
    }

    #[tokio::test]
    async fn a_session_is_named_by_the_request_it_sends() {
        let pool = common::fresh_db().await;
        let app = app(pool.clone());

        digest_called(&app, "sess-named", Some("memview")).await;
        assert_eq!(
            stored_name(&pool, "sess-named").await.as_deref(),
            Some("memview"),
            "the session stayed a uuid though its request said what it is called"
        );
    }

    #[tokio::test]
    async fn a_later_name_replaces_an_earlier_one() {
        // A conversation is renamed as its job changes, and the transcript's
        // newest line is the answer — so the service has to take the newer one
        // rather than keep the first thing it was ever told.
        let pool = common::fresh_db().await;
        let app = app(pool.clone());

        digest_called(&app, "sess-renamed", Some("scanner")).await;
        digest_called(&app, "sess-renamed", Some("observe")).await;
        assert_eq!(
            stored_name(&pool, "sess-renamed").await.as_deref(),
            Some("observe"),
            "a rename in Claude Code did not reach the service"
        );
    }

    #[tokio::test]
    async fn saying_nothing_keeps_the_name_a_session_has() {
        // The absent header is the common case, not an error: an older CLI, a
        // transcript that cannot be read, a session started outside Claude Code.
        // Blanking the name on those would empty the column on the next prompt.
        let pool = common::fresh_db().await;
        let app = app(pool.clone());

        digest_called(&app, "sess-quiet", Some("recall")).await;
        digest_called(&app, "sess-quiet", None).await;
        assert_eq!(
            stored_name(&pool, "sess-quiet").await.as_deref(),
            Some("recall"),
            "a request that said nothing erased the name"
        );
    }

    #[tokio::test]
    async fn a_person_reading_a_digest_cannot_name_the_session() {
        // Pippijn's browser can pass `?session=`, and it is not that
        // conversation. Whatever header it carries is about itself.
        let pool = common::fresh_db().await;
        let app = app(pool.clone());
        sessions::touch(&pool, "sess-watched", Some("health"))
            .await
            .expect("a named session");

        let cookie = create_session(
            SECRET,
            &UserSession {
                user_id: "pippijn".into(),
                display_name: "Pippijn".into(),
            },
        );
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/digest?session=sess-watched")
                    .method("GET")
                    .header("Cookie", format!("session={cookie}"))
                    .header("X-Session-Name", "not-its-name")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("the router answered");
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            stored_name(&pool, "sess-watched").await.as_deref(),
            Some("health"),
            "a reader renamed the session it was reading"
        );
    }
}

/// The pile, asked for on its own, through the route that answers it.
///
/// ⚠ **Paired with `tests/selection.rs` deliberately.** Those pin that the CLI
/// SENDS `unheld=true`; a route that parsed it and dropped it on the floor would
/// leave all four of them green while `task --pile` answered with every task
/// there is — which is the shape that produced this ticket in the first place:
/// a filter believed rather than checked, reporting 137 where there were 5.
mod pile {
    use super::*;

    async fn pile_list(app: &axum::Router, query: &str) -> Vec<String> {
        pile_tasks(app, query)
            .await
            .into_iter()
            .map(|t| t["subject"].as_str().unwrap_or_default().to_string())
            .collect()
    }

    /// ⚠ **Ids are not predictable across the suite.** `fresh_db` does not reset
    /// the counter, so the fourth task filed here is `#4` only in whichever test
    /// runs first — which passed alone and failed in the run, once.
    async fn pile_tasks(app: &axum::Router, query: &str) -> Vec<serde_json::Value> {
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tasks?{query}"))
                    .method("GET")
                    .header("Authorization", format!("Bearer {TOKEN}"))
                    .header("X-Session-Id", "sess-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("the router answered");
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), 256 * 1024)
            .await
            .expect("a body");
        serde_json::from_slice::<Vec<serde_json::Value>>(&body).expect("a task array")
    }

    #[tokio::test]
    async fn the_route_answers_with_the_unheld_and_nothing_else() {
        let pool = common::fresh_db().await;
        seed(&pool).await;
        let app = app(pool);

        let pile = pile_list(&app, "unheld=true").await;
        assert_eq!(
            pile,
            vec!["PILE, for whoever picks it up".to_string()],
            "the pile answer carried held work: {pile:?}"
        );
    }

    #[tokio::test]
    async fn a_session_id_does_not_narrow_the_pile_to_nothing() {
        // The CLI does not send one, but the parameter is public and a caller
        // may. Intersecting would answer with an empty list, which reads as an
        // empty pile — a wrong answer that looks like a fine one.
        let pool = common::fresh_db().await;
        seed(&pool).await;
        let app = app(pool);

        let pile = pile_list(&app, "unheld=true&session=sess-1").await;
        assert_eq!(
            pile,
            vec!["PILE, for whoever picks it up".to_string()],
            "a holder narrowed a question that has no holder: {pile:?}"
        );
    }

    #[tokio::test]
    async fn the_pile_is_not_what_a_bare_list_widens_to() {
        // `pile=true` widens to own-plus-unheld; `unheld=true` narrows to the
        // unheld. Confusing the two is exactly the miscount this closes.
        let pool = common::fresh_db().await;
        seed(&pool).await;
        let app = app(pool);

        let widened = pile_list(&app, "session=sess-1&pile=true").await;
        let narrowed = pile_list(&app, "unheld=true").await;
        assert!(widened.iter().any(|s| s == "MINE to do"), "{widened:?}");
        assert!(
            !narrowed.iter().any(|s| s == "MINE to do"),
            "the narrow question answered with the caller's own work: {narrowed:?}"
        );
    }

    /// The pile is open work by construction, and `--done` cannot widen it.
    ///
    /// ⚠ **Not a quirk of this query — a consequence of another rule.** Closing
    /// a task into the pile is refused outright ("a finished task with no holder
    /// reads as done by nobody"), so `closed AND unheld` is a state the service
    /// will not store. `--pile --done` therefore answers the same as `--pile`,
    /// and an empty pile stays empty however it is asked. Written after
    /// asserting the opposite and being told so by the service.
    #[tokio::test]
    async fn the_pile_never_holds_closed_work_however_it_is_asked() {
        let pool = common::fresh_db().await;
        seed(&pool).await;
        let app = app(pool.clone());
        let piled = pile_tasks(&app, "unheld=true").await;
        assert_eq!(piled.len(), 1, "the fixture");
        let id = piled[0]["id"].as_u64().expect("an id");

        // Closing it takes it on — the only way a pile task can close — so the
        // pile empties by the task gaining a holder rather than by hiding.
        repo::update(
            &pool,
            id,
            repo::Change {
                status: Some(tasks::tasks::types::Status::Dropped),
                ..Default::default()
            },
            &Actor::Person("pippijn".into()),
        )
        .await
        .expect("dropping it");

        assert!(
            pile_list(&app, "unheld=true").await.is_empty(),
            "a closed task stayed in the pile"
        );
        assert!(
            pile_list(&app, "unheld=true&done=true").await.is_empty(),
            "--done resurrected work that left the pile by being closed"
        );
    }
}
