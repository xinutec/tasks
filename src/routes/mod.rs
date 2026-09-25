//! HTTP routing table.

pub mod api;
pub mod auth;
pub mod telemetry;

use axum::Router;
use axum::http::{HeaderValue, Response, header};
use axum::routing::{get, patch, post};
use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::state::AppState;

/// How long a static response may be reused without asking again.
///
/// ⚠ **`index.html` MUST REVALIDATE.** With no `Cache-Control` a client falls
/// back to heuristic freshness and may keep the page for days without asking.
/// The page names the content-hashed bundle, so the new `main-*.js` is never
/// fetched either and a deploy is invisible. `no-cache` means "ask first", not
/// "never keep": the `ETag` still makes the usual case a bodiless 304.
///
/// Everything else Angular emits carries a content hash in its NAME, so a new
/// build is a new URL — the one kind of response `immutable` is honest for.
///
/// Generic over the body because this `ServeDir` falls back to the `spa`
/// HANDLER, not a `ServeFile`, so its body type is not
/// `ServeFileSystemResponseBody`. The predicate only reads a header.
fn cache_control_for<B>(res: &Response<B>) -> Option<HeaderValue> {
    // ⚠ **An error is not an asset.** [`spa`] 404s a missing path that looks
    // like a file, by design and often, and a year of `immutable` on that is a
    // client that will not ask for the name again.
    //
    // ⚠ NOT `!is_success()`: that excludes **304 Not Modified**, which must
    // carry the headers a 200 would, or every revalidation becomes a re-fetch.
    if res.status().is_client_error() || res.status().is_server_error() {
        return None;
    }
    let is_html = res
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("text/html"));
    Some(if is_html {
        HeaderValue::from_static("no-cache")
    } else {
        HeaderValue::from_static("public, max-age=31536000, immutable")
    })
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/me", get(api::me))
        // The index a prompt receives. `text/plain`, because its one consumer
        // prints it, on the latency path of every prompt.
        .route("/digest", get(api::digest))
        // What the calling conversation is focused on. No id in the path: a
        // session may only focus itself, so the credential names it.
        .route(
            "/focus",
            get(api::read_focus)
                .post(api::start_focus)
                .delete(api::end_focus),
        )
        .route("/tasks", get(api::list).post(api::create))
        .route("/tasks/{id}", get(api::detail).patch(api::update))
        .route("/tasks/{id}/previous", get(api::previous))
        .route("/sessions", get(api::session_list))
        .route("/holders", get(api::holders))
        .route("/sessions/{id}", patch(api::rename))
        .route("/checks", get(api::checks_ran).post(api::check_ran))
        .route("/commands", get(api::commands_ran).post(api::command_ran))
        .route("/telemetry", post(telemetry::record))
        // ⚠ **`/api/*` must never reach the page.** Otherwise an unknown API path
        // falls through to the `ServeDir` fallback — `spa()` passes it, having
        // no dot in its last segment — and a caller asking for JSON gets
        // `200 text/html`, which reads as a client bug.
        .fallback(api::not_found);

    let app = Router::new()
        .route("/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/logout", post(auth::logout))
        .nest("/api", api);

    // Serve the built Angular bundle (single origin), SPA-fallback to
    // index.html so deep links load the shell. API-only when STATIC_DIR is
    // unset (dev: `ng serve` proxies).
    //
    // ⚠ **A missing FILE must 404, not fall back to the page.** The obvious
    // `ServeDir::fallback(ServeFile::new(index))` answers a missing font with
    // `200 text/html`, and the browser renders broken icons and reports
    // nothing. The test is a dot in the last path segment: `/t/1` is a route,
    // `/main-ABC123.js` a file. A heuristic, but enumerating the bundle's asset
    // names would change with every build.
    let app = if let Some(dir) = state.cfg.static_dir.clone() {
        let index = format!("{dir}/index.html");
        let serve = ServeDir::new(&dir).fallback(get(move |uri: axum::http::Uri| {
            let index = index.clone();
            async move { spa(&index, uri.path()) }
        }));
        // ⚠ The layer wraps the STATIC SERVICE ALONE, or API JSON would be
        // stamped `immutable` for a year.
        let serve = ServiceBuilder::new()
            .layer(SetResponseHeaderLayer::overriding(
                header::CACHE_CONTROL,
                cache_control_for,
            ))
            .service(serve);
        app.fallback_service(serve)
    } else {
        app
    };

    // One line per request: method, path, status, latency. Levels explicit:
    // TraceLayer logs under `tower_http`, which an `info` filter leaves at info,
    // so its default DEBUG would never emit a line.
    let trace = TraceLayer::new_for_http()
        .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
        .on_response(DefaultOnResponse::new().level(Level::INFO));

    // /healthz is registered AFTER the layer, so it is untraced: kubelet's
    // probes would bury the requests a person made.
    app.layer(trace)
        .route("/healthz", get(|| async { "ok" }))
        .with_state(state)
}

/// The SPA shell, or a 404 for something that was meant to be a file.
///
/// Public so `tests/serving.rs` can exercise it: its mistake is a 200, visible
/// only as missing icons in a browser.
pub fn spa(index: &str, path: &str) -> axum::response::Response {
    use axum::response::IntoResponse as _;

    if path
        .rsplit('/')
        .next()
        .is_some_and(|last| last.contains('.'))
    {
        return (axum::http::StatusCode::NOT_FOUND, "not found").into_response();
    }
    match std::fs::read_to_string(index) {
        Ok(page) => axum::response::Html(page).into_response(),
        Err(error) => {
            // A deployment with STATIC_DIR set and no index is misconfigured,
            // and saying so beats serving an empty page that looks like the app.
            tracing::error!("the app's index could not be read: {error}");
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "no index").into_response()
        }
    }
}
