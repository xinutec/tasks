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

/// How long a static response may be reused without asking.
///
/// ⚠ `index.html` must revalidate (`no-cache`, still a bodiless 304 via `ETag`):
/// cached heuristically, it keeps naming the old hashed bundle and a deploy is
/// invisible. Everything else has a content hash in its name, so `immutable` is
/// honest there. Generic over the body because the fallback is the `spa` handler.
fn cache_control_for<B>(res: &Response<B>) -> Option<HeaderValue> {
    // Errors aren't assets: [`spa`] 404s missing files often, and a year of
    // `immutable` on one stops the client asking again. Not `!is_success()`,
    // which would drop the headers from a 304.
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
        // The prompt's index: plain text, on the latency path of every prompt.
        .route("/digest", get(api::digest))
        // A session may only focus itself, so the credential names it.
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
        .route("/checks/licence", get(api::check_licence))
        .route("/checks/overrules", get(api::check_overrules))
        .route("/commands", get(api::commands_ran).post(api::command_ran))
        .route("/telemetry", post(telemetry::record))
        // `/api/*` must never fall through to the page, or an unknown API path
        // answers `200 text/html`.
        .fallback(api::not_found);

    let app = Router::new()
        .route("/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/logout", post(auth::logout))
        .nest("/api", api);

    // The built Angular bundle, falling back to index.html for deep links;
    // API-only without STATIC_DIR (dev, where `ng serve` proxies).
    //
    // ⚠ A missing file must 404, not get the page, or a missing font renders as
    // broken icons with no error. A dot in the last segment marks a file: `/t/1`
    // is a route, `/main-ABC123.js` a file.
    let app = if let Some(dir) = state.cfg.static_dir.clone() {
        let index = format!("{dir}/index.html");
        let serve = ServeDir::new(&dir).fallback(get(move |uri: axum::http::Uri| {
            let index = index.clone();
            async move { spa(&index, uri.path()) }
        }));
        // Wraps the static service only, or API JSON would be cached as `immutable`.
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

    // One line per request. Levels are explicit: TraceLayer's default DEBUG never
    // shows under an `info` filter.
    let trace = TraceLayer::new_for_http()
        .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
        .on_response(DefaultOnResponse::new().level(Level::INFO));

    // /healthz is added after the layer, so probes don't bury real requests.
    app.layer(trace)
        .route("/healthz", get(|| async { "ok" }))
        .with_state(state)
}

/// The SPA shell, or 404 for a path that looks like a file.
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
            // STATIC_DIR without an index is a misconfiguration; say so.
            tracing::error!("the app's index could not be read: {error}");
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "no index").into_response()
        }
    }
}
