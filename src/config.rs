//! Runtime configuration from the environment.
//!
//! Auth is *inert unless configured* (the memview/recall pattern): the
//! Nextcloud login wall activates only when `SESSION_SECRET` +
//! `NC_CLIENT_ID` + `NC_CLIENT_SECRET` are all set, so local dev on the Mac
//! serves open and only the isis deployment raises the wall.

use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct Config {
    /// Full `mysql://` DSN for this app's own database.
    pub database_url: String,
    pub bind_addr: String,
    /// Directory of the built Angular bundle to serve (SPA fallback). Unset →
    /// API-only (dev, where `ng serve` proxies).
    pub static_dir: Option<String>,

    /// Nextcloud OAuth2 (identity-only). None → the browser wall is disabled.
    pub auth: Option<AuthConfig>,

    /// The shared secret a Claude session presents to act on its own tasks.
    ///
    /// ⚠ **It authenticates the machine, not the session.** Every session on
    /// the Mac reads the same value out of the same file, so one holding it can
    /// act as another by declaring a different `X-Session-Id`. That is not a
    /// boundary being lost — they run as one user on one machine and can read
    /// each other's transcripts anyway — but it must not be described as
    /// per-session authentication, because a later change might rely on that.
    ///
    /// None → the agent API is closed entirely, which is what a browser-only
    /// deployment wants and what the tests use to prove the wall exists.
    pub agent_token: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AuthConfig {
    /// HMAC key for signing session cookies and OAuth state.
    pub session_secret: String,
    /// Base URL of the Nextcloud instance as the *browser* reaches it, no
    /// trailing slash.
    pub nc_base_url: String,
    /// Server-side base URL for token/userinfo calls (cluster-internal Service
    /// DNS on isis, where the pod can't hairpin to the node's public IP).
    /// Requests here carry a Host header for `nc_base_url`'s host.
    pub nc_internal_url: Option<String>,
    pub nc_client_id: String,
    pub nc_client_secret: String,
    pub nc_redirect_uri: String,
    /// Nextcloud user ids permitted to sign in. Fail-closed: an empty list is
    /// rejected at startup rather than treated as "everybody".
    pub allowed_users: Vec<String>,
}

fn env(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("missing required env var {key}"))
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// An env var that is absent when empty as well as when unset.
///
/// Kubernetes supplies an optional secret reference as an empty string rather
/// than by omitting the variable, so `Some("")` is what an unconfigured
/// deployment actually produces — and an empty agent token that compared equal
/// to an empty header would admit everybody.
fn env_set(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let auth = match env_set("SESSION_SECRET") {
            Some(session_secret) => {
                let allowed_users = env("ALLOWED_USERS")?
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>();
                Some(AuthConfig {
                    session_secret,
                    nc_base_url: env("NC_BASE_URL")?.trim_end_matches('/').to_string(),
                    nc_internal_url: env_set("NC_INTERNAL_URL")
                        .map(|u| u.trim_end_matches('/').to_string()),
                    nc_client_id: env("NC_CLIENT_ID")?,
                    nc_client_secret: env("NC_CLIENT_SECRET")?,
                    nc_redirect_uri: env("NC_REDIRECT_URI")?,
                    allowed_users,
                })
            }
            None => None,
        };

        Ok(Self {
            database_url: env("DATABASE_URL")?,
            bind_addr: env_or("BIND_ADDR", "0.0.0.0:8092"),
            static_dir: env_set("STATIC_DIR"),
            auth,
            agent_token: env_set("AGENT_TOKEN"),
        })
    }

    /// Whether a Nextcloud user id is permitted to use the app. With auth
    /// unconfigured every request is the local owner, so this is `true`.
    pub fn is_allowed(&self, user_id: &str) -> bool {
        match &self.auth {
            Some(a) => a.allowed_users.iter().any(|u| u == user_id),
            None => true,
        }
    }
}
