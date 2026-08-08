//! tasks — the service. Loads config, connects, migrates, serves. All logic
//! lives in the `tasks` library crate.

use anyhow::Result;
use tasks::{config::Config, db, routes, state::AppState};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cfg = Config::from_env()?;
    match &cfg.auth {
        Some(auth) => {
            if auth.allowed_users.is_empty() {
                anyhow::bail!("ALLOWED_USERS is empty — refusing to start (would deny everyone)");
            }
            tracing::info!("auth enabled; allow-list: {:?}", auth.allowed_users);
        }
        None => tracing::warn!("auth NOT configured — serving open (dev mode)"),
    }
    if cfg.agent_token.is_none() {
        // Worth a line: with no token every Claude session is locked out, and
        // the symptom is a hook that prints nothing — which looks exactly like
        // having no tasks.
        tracing::warn!("AGENT_TOKEN not set — the agent API is closed; sessions cannot read tasks");
    }

    let pool = db::connect(&cfg.database_url).await?;
    db::migrate(&pool).await?;

    let http = reqwest::Client::builder().build()?;
    let bind_addr = cfg.bind_addr.clone();
    let app = routes::router(AppState::new(cfg, pool, http));

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("tasks listening on {bind_addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
