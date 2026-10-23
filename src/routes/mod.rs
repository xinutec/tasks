//! HTTP routing table.

pub mod api;
pub mod auth;
pub mod telemetry;

use axum::Router;
use axum::http::{HeaderValue, header};
use axum::routing::{get, patch, post};
use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::state::AppState;

/// A name carrying its content hash (`main-7GNM4EPX.js`, a font's `-ABCD1234`):
/// a new build is a new name, so it may be cached for ever.
fn is_content_hashed(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or("");
    let Some((stem, _ext)) = name.rsplit_once('.') else {
        return false;
    };
    let Some((_, hash)) = stem.rsplit_once('-') else {
        return false;
    };
    hash.len() == 8
        && hash
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

/// How long a static response may be reused. A hashed file is `immutable`.
/// Everything else revalidates (`no-cache`: ask first, then a 304 via the ETag):
/// the page names the bundle, and an icon, the manifest and the service worker's
/// files keep their names across builds, so a year of `immutable` pins the old
/// one on every phone. Without `Cache-Control` a WebView caches heuristically and
/// runs a build several deploys old. An error is never `immutable`, or the name is
/// not asked for again; a 304 keeps what a 200 would say.
async fn cache_headers(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let hashed = is_content_hashed(req.uri().path());
    let mut res = next.run(req).await;
    let failed = res.status().is_client_error() || res.status().is_server_error();
    let value = if hashed && !failed {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    res
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
            .layer(axum::middleware::from_fn(cache_headers))
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
