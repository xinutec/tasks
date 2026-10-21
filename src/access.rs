//! Who is asking: the person in a browser, or a Claude session.
//!
//! ⚠ The actor comes from the credential, never the request body, so a
//! session cannot file history as the person.
//!
//! The person signs in through Nextcloud (an HMAC cookie, `session.rs`). A
//! session presents `AGENT_TOKEN` and names its conversation in `X-Session-Id`;
//! that token identifies the machine, not the conversation
//! ([`crate::config::Config::agent_token`]). With auth unconfigured every
//! request is the local owner, and the agent API stays closed unless its token
//! is set.

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

/// Header carrying what Claude Code calls that conversation.
pub const SESSION_NAME_HEADER: &str = "X-Session-Name";

/// The name a caller reports for itself. Not in [`Viewer`], which holds nothing
/// client-supplied; it only ever renames the sender's own row.
pub struct SeenAs(pub Option<String>);

impl<S: Send + Sync> FromRequestParts<S> for SeenAs {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(SeenAs(
            parts
                .headers
                .get(SESSION_NAME_HEADER)
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
                .filter(|s| !s.is_empty() && s.chars().count() <= 40)
                .map(str::to_string),
        ))
    }
}

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

/// Who every request is with auth unconfigured.
pub const LOCAL_OWNER: &str = "local";

fn local_owner() -> UserSession {
    UserSession {
        user_id: LOCAL_OWNER.into(),
        display_name: "Local".into(),
    }
}

/// What a caller's `Authorization` header amounted to.
enum Offered {
    /// No bearer token, or not ours. The cookie gets its turn.
    Nothing,
    /// Our token, on behalf of a named conversation.
    On(String),
    /// Our token without a conversation: refused, saying so, rather than a bare
    /// "not authenticated" that sends someone to re-check a correct token.
    Nameless,
}

const NAMELESS: &str = "the agent token is accepted, but nothing says which conversation \
                        it is for — send X-Session-Id, or use a `task` CLI new enough to \
                        read $CLAUDE_CODE_SESSION_ID";

fn agent(app: &AppState, parts: &Parts) -> Offered {
    let Some(expected) = app.cfg.agent_token.as_deref() else {
        return Offered::Nothing;
    };
    let offered = parts
        .headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let Some(offered) = offered else {
        return Offered::Nothing;
    };
    // Constant-time; `ct_eq` isn't for differing lengths, so check length first.
    if offered.len() != expected.len() || !bool::from(offered.as_bytes().ct_eq(expected.as_bytes()))
    {
        return Offered::Nothing;
    }
    parts
        .headers
        .get(SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map_or(Offered::Nameless, |s| Offered::On(s.to_string()))
}

fn resolve(app: &AppState, parts: &Parts) -> Result<Viewer, AppError> {
    // The token first, so a session in a signed-in browser isn't filed as the user.
    match agent(app, parts) {
        Offered::On(session) => return Ok(Viewer::Session(session)),
        Offered::Nameless => return Err(AppError::Unauthorized(NAMELESS)),
        Offered::Nothing => {}
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
    Err(AppError::Unauthorized("not authenticated"))
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
