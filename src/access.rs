//! Who is asking: the person in a browser, or a Claude session.
//!
//! ⚠ **The actor is derived from the credential, never from the request body.**
//! A write says what to change; it does not get to say who is changing it. That
//! is what stops a session filing history as though Pippijn had moved a task —
//! the one thing in this app that would make the record untrustworthy, and it is
//! prevented by there being no field to put it in.
//!
//! Two credentials, and they are not the same strength:
//!
//! * the **person** signs in through Nextcloud and carries an HMAC session
//!   cookie (`session.rs`);
//! * a **session** presents the shared `AGENT_TOKEN` and declares which
//!   conversation it is in `X-Session-Id`. That token authenticates the machine
//!   rather than the conversation — see [`crate::config::Config::agent_token`],
//!   which is where the consequence is written down.
//!
//! With auth unconfigured every request is the local owner, which is the dev
//! mode; `AGENT_TOKEN` is separate and stays closed unless set, so a dev server
//! is open to the desk and not to a stale script.

use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;
use axum_extra::extract::cookie::CookieJar;
use subtle::ConstantTimeEq;

use crate::error::AppError;
use crate::session::{COOKIE_NAME, UserSession, get_session};
use crate::state::AppState;
use crate::tasks::types::Actor;

/// Header naming which conversation an agent request is on behalf of.
pub const SESSION_HEADER: &str = "X-Session-Id";

#[derive(Clone, Debug)]
pub enum Viewer {
    Owner(UserSession),
    /// A Claude Code conversation, by the CLI's session id.
    Session(String),
}

impl Viewer {
    /// Who to file this change under.
    pub fn actor(&self) -> Actor {
        match self {
            Viewer::Owner(user) => Actor::Person(user.user_id.clone()),
            Viewer::Session(id) => Actor::Session(id.clone()),
        }
    }
}

fn local_owner() -> UserSession {
    UserSession {
        user_id: "local".into(),
        display_name: "Local".into(),
    }
}

fn agent(app: &AppState, parts: &Parts) -> Option<Viewer> {
    let expected = app.cfg.agent_token.as_deref()?;
    let offered = parts
        .headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))?;
    // Constant-time, and length-checked first: `ct_eq` on differing lengths is
    // not defined to be constant-time, and the length of a secret is itself
    // worth not leaking.
    if offered.len() != expected.len() || !bool::from(offered.as_bytes().ct_eq(expected.as_bytes()))
    {
        return None;
    }
    let session = parts
        .headers
        .get(SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    Some(Viewer::Session(session.to_string()))
}

fn resolve(app: &AppState, parts: &Parts) -> Result<Viewer, AppError> {
    // The agent credential is checked first, so a request that carries one is
    // filed against the session even from a browser that also holds a cookie —
    // otherwise a session driven from a signed-in machine would write history
    // under Pippijn's name.
    if let Some(viewer) = agent(app, parts) {
        return Ok(viewer);
    }
    let Some(auth) = &app.cfg.auth else {
        return Ok(Viewer::Owner(local_owner()));
    };
    let jar = CookieJar::from_headers(&parts.headers);
    if let Some(cookie) = jar.get(COOKIE_NAME)
        && let Some(user) = get_session(&auth.session_secret, cookie.value())
    {
        return Ok(Viewer::Owner(user));
    }
    Err(AppError::Unauthorized)
}

/// Extractor: the person or a session; 401 otherwise.
pub struct Access(pub Viewer);

impl<S> FromRequestParts<S> for Access
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app = AppState::from_ref(state);
        Ok(Access(resolve(&app, parts)?))
    }
}

/// Extractor: the person only; a session gets 403, no credential at all 401.
pub struct OwnerOnly(pub UserSession);

impl<S> FromRequestParts<S> for OwnerOnly
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app = AppState::from_ref(state);
        match resolve(&app, parts)? {
            Viewer::Owner(user) => Ok(OwnerOnly(user)),
            Viewer::Session(_) => Err(AppError::Forbidden),
        }
    }
}
