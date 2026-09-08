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
/// ⚠ **`index.html` MUST REVALIDATE.** With no `Cache-Control` at all — which
/// is what this served from 2026-08-08 until it was measured on 2026-09-07 — a
/// client falls back to HEURISTIC freshness, roughly a tenth of the document's
/// age, and may keep it for days without ever asking. The document names the
/// content-hashed bundle, so the new `main-*.js` is never fetched either and a
/// deploy is invisible: an Android `WebView` loaded messages' API and a whole
/// thread while running several builds behind, with a missing button as the
/// only symptom.
///
/// `no-cache` means "ask first", not "never keep" — the `ETag` still turns the
/// usual case into a 304 with no body.
///
/// Everything else Angular emits carries a content hash in its NAME, so a new
/// build is a new URL and the old one can never be wrong. Those are the one
/// kind of response `immutable` is honestly available for.
///
/// Generic over the body, and here that is not a nicety: this `ServeDir` falls
/// back to the `spa` HANDLER rather than a `ServeFile`, so its response body is
/// not `ServeFileSystemResponseBody` and the concrete signature the other apps
/// use would not compile. The predicate only ever reads a header.
fn cache_control_for<B>(res: &Response<B>) -> Option<HeaderValue> {
    // ⚠ **A 404 is not an asset**, and here that is not hypothetical: the
    // fallback above deliberately 404s a missing path that LOOKS like a file
    // (`/media/nope.woff2`), so this service answers 404 by design and often.
    // `SetResponseHeaderLayer::overriding` stamps whatever the service
    // returned, and a year of `immutable` on those is a client that will not
    // ask for that name again this year.
    //
    // ⚠ NOT `!is_success()`. That excludes **304 Not Modified**, which must
    // carry the headers a 200 would so the client can refresh what it already
    // holds. Stripping it made every revalidated image a full re-fetch, and the
    // arriving bytes grew the thread AFTER it had scrolled to the bottom —
    // `thread-scroll.spec.ts` caught it at 271px off, a symptom with no visible
    // connection to a cache header.
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
        // prints it — a hook that had to parse JSON to print eight lines would
        // be carrying a parser on the latency path of every prompt.
        .route("/digest", get(api::digest))
        // What the calling conversation is working on this afternoon. No id in
        // the path: a session may only focus itself, so the credential names
        // the only subject this route has.
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
        // ⚠ **`/api/*` must never reach the page.** Without this an unknown API
        // path falls through the nest to the `ServeDir` fallback below and comes
        // back `200 text/html` — the SPA shell, to a caller that asked for JSON.
        // It is the same defect the fallback's own comment is about, one level
        // up: `spa()` refuses a path whose last segment has a dot, and
        // `/api/nonsense` has none.
        //
        // It was true from the start and stayed invisible while every published
        // path existed. Retiring `/api/tasks/by/{session}/{number}` is what made
        // it reachable by a spelling the docs had published, and the symptom
        // pointed at the client: *"the service answered 200 OK with something
        // this CLI could not read"*.
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
    // wiring — `ServeDir::fallback(ServeFile::new(index))` — answers a missing
    // script or font with `200 text/html`, and a browser handed HTML where it
    // asked for a woff2 renders broken icons and reports nothing at all. Both
    // memview's console and this app shipped it; measured here against the live
    // deployment, `/media/nope.woff2` came back `200 text/html`.
    //
    // The test is a dot in the last path segment: `/t/1` is a route and
    // `/main-ABC123.js` is a file. It is a heuristic, and the alternative —
    // enumerating the bundle's own asset names — would have to be rebuilt
    // whenever `ng build` changes a hash.
    let app = if let Some(dir) = state.cfg.static_dir.clone() {
        let index = format!("{dir}/index.html");
        let serve = ServeDir::new(&dir).fallback(get(move |uri: axum::http::Uri| {
            let index = index.clone();
            async move { spa(&index, uri.path()) }
        }));
        // ⚠ The layer wraps the STATIC SERVICE ALONE. `health`'s first attempt
        // hooked every route and stamped a year of `immutable` onto API JSON,
        // which is this bug pointing the other way.
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

    // One line per request: method, path, status, latency. The levels are set
    // explicitly rather than left at the defaults — TraceLayer logs under the
    // `tower_http` target, so a filter of `info,tasks=debug` raises this crate
    // and leaves the layer at info, and taking the default DEBUG would ship a
    // layer that can never emit a line. memview paid for that once.
    let trace = TraceLayer::new_for_http()
        .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
        .on_response(DefaultOnResponse::new().level(Level::INFO));

    // /healthz is registered AFTER the layer, so it is deliberately untraced:
    // kubelet probes it about three times every twenty seconds, and logging that
    // buries the handful of requests a person actually made.
    app.layer(trace)
        .route("/healthz", get(|| async { "ok" }))
        .with_state(state)
}

/// The SPA shell, or a 404 for something that was meant to be a file.
///
/// Public so `tests/serving.rs` can exercise the rule directly: it is the one
/// piece of routing whose mistake is invisible — the wrong answer is a 200, and
/// the symptom appears in a browser as missing icons rather than as an error
/// anywhere.
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
