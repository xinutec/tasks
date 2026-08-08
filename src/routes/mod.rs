//! HTTP routing table.

pub mod api;
pub mod auth;

use axum::Router;
use axum::routing::{get, patch, post};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/me", get(api::me))
        // The index a prompt receives. `text/plain`, because its one consumer
        // prints it — a hook that had to parse JSON to print eight lines would
        // be carrying a parser on the latency path of every prompt.
        .route("/digest", get(api::digest))
        .route("/tasks", get(api::list).post(api::create))
        .route("/tasks/{id}", get(api::detail).patch(api::update))
        .route("/sessions", get(api::session_list))
        .route("/sessions/{id}", patch(api::rename))
        .route("/repos", get(api::repo_counts));

    let app = Router::new()
        .route("/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/logout", post(auth::logout))
        .nest("/api", api);

    // Serve the built Angular bundle (single origin), SPA-fallback to
    // index.html so deep links load the shell. API-only when STATIC_DIR is
    // unset (dev: `ng serve` proxies).
    let app = if let Some(dir) = state.cfg.static_dir.clone() {
        let serve = ServeDir::new(&dir).fallback(ServeFile::new(format!("{dir}/index.html")));
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
