//! **`index.html` must revalidate; the hashed bundle may be kept forever; a
//! missing file says nothing about how long to keep it.**
//!
//! WHY THIS IS A TEST AND NOT A CURL. Six apps were given this header on
//! 2026-08-14 and the task was closed as "all fixed, deployed and curled". No
//! standing check went with it, so when three more names were measured on
//! 2026-09-07 — tasks among them, live since 2026-08-08 — nothing had ever
//! asked them the question. A header verified by hand once is a header nobody
//! is watching.
//!
//! What goes wrong without it: with no `Cache-Control` a client falls back to
//! HEURISTIC freshness, roughly a tenth of the document's age, and may keep
//! `index.html` for days without asking. That document names the content-hashed
//! bundle, so the new `main-*.js` is never fetched either and the deploy is
//! invisible. An Android `WebView` ran several builds behind for hours with a
//! missing button as the only symptom.
//!
//! No database: the static service answers before anything reaches the pool, so
//! these take a lazy pool pointed at nothing, the way `tests/access.rs` does.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use sqlx::MySqlPool;
use tasks::config::Config;
use tasks::routes;
use tasks::state::AppState;
use tower::ServiceExt;

/// A static dir shaped like a real `ng build` output: the document, and one
/// asset whose NAME carries the content hash.
struct StaticDir(std::path::PathBuf);

impl StaticDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "tasks-cache-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).expect("create static dir");
        std::fs::write(dir.join("index.html"), "<!doctype html><html></html>").expect("index");
        std::fs::write(dir.join("main-URXJXJF6.js"), "export {};").expect("bundle");
        Self(dir)
    }
}

impl Drop for StaticDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn cache_control(path: &str) -> (StatusCode, String) {
    let dir = StaticDir::new();
    let cfg = Config {
        database_url: "mysql://unused:unused@127.0.0.1:1/unused".into(),
        bind_addr: "127.0.0.1:0".into(),
        static_dir: Some(dir.0.to_string_lossy().into_owned()),
        // No login wall: the static service is not behind one, and adding an
        // auth config here would only invite a redirect to explain.
        auth: None,
        agent_token: None,
    };
    let pool = MySqlPool::connect_lazy(&cfg.database_url).expect("a lazy pool");
    let res = routes::router(AppState::new(cfg, pool, reqwest::Client::new()))
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let value = res
        .headers()
        .get(header::CACHE_CONTROL)
        .map(|v| v.to_str().unwrap().to_owned())
        .unwrap_or_default();
    (status, value)
}

#[tokio::test]
async fn the_document_is_asked_for_every_time() {
    let (status, cc) = cache_control("/index.html").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cc, "no-cache");
}

/// The path that actually matters on a phone. A deep link matches no route, so
/// it reaches the SPA fallback and is served the document — which means the
/// fallback needs the header just as much as the file does, and here it is a
/// different service entirely, not another arm of `ServeDir`.
#[tokio::test]
async fn a_deep_link_served_the_shell_revalidates_too() {
    let (status, cc) = cache_control("/t/1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cc, "no-cache");
}

/// The only kind of response `immutable` is honestly available for: a new build
/// is a new URL, so the old one can never be wrong.
#[tokio::test]
async fn the_content_hashed_bundle_may_be_kept() {
    let (status, cc) = cache_control("/main-URXJXJF6.js").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cc, "public, max-age=31536000, immutable");
}

/// ⚠ The case this app hits by DESIGN. `spa()` refuses a path whose last
/// segment has a dot, so a missing asset 404s rather than being handed the
/// shell — and a 404 stamped `immutable` would be a client that stops asking
/// for that name for a year. The header must be absent, not merely shorter.
#[tokio::test]
async fn a_missing_asset_is_not_told_how_long_to_keep_the_404() {
    let (status, cc) = cache_control("/media/nope.woff2").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(cc, "", "a 404 must carry no Cache-Control at all");
}

/// ⚠ **A 304 is not an error, and the difference is not cosmetic.** The guard
/// above was first written as `!status.is_success()`, which also caught
/// `304 Not Modified` — and a 304 must carry the headers a 200 would, so the
/// client can refresh what it already holds. Without them every revalidated
/// asset became a full re-fetch. In `messages` the symptom was a thread that
/// landed 271px above the bottom, because images arrived and grew the page
/// after it had scrolled: a scroll bug with no visible connection to a cache
/// header. This test is the cheap version of that accident.
#[tokio::test]
async fn a_revalidated_asset_is_still_told_it_may_be_kept() {
    let dir = StaticDir::new();
    let cfg = Config {
        database_url: "mysql://unused:unused@127.0.0.1:1/unused".into(),
        bind_addr: "127.0.0.1:0".into(),
        static_dir: Some(dir.0.to_string_lossy().into_owned()),
        auth: None,
        agent_token: None,
    };
    let pool = MySqlPool::connect_lazy(&cfg.database_url).expect("a lazy pool");
    let app = routes::router(AppState::new(cfg, pool, reqwest::Client::new()));

    let first = app
        .clone()
        .oneshot(
            Request::get("/main-URXJXJF6.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let etag = first
        .headers()
        .get(header::ETAG)
        .expect("ServeDir sends an ETag, which is what makes a 304 reachable")
        .clone();

    let second = app
        .oneshot(
            Request::get("/main-URXJXJF6.js")
                .header(header::IF_NONE_MATCH, &etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(
        second
            .headers()
            .get(header::CACHE_CONTROL)
            .map(|v| v.to_str().unwrap()),
        Some("public, max-age=31536000, immutable"),
    );
}
