//! Auth routes: Nextcloud identity login, restricted to an explicit
//! allow-list. Copied from `messages`; sessions are stateless here, so
//! logout is just clearing the cookie. All three routes 404 when auth is
//! unconfigured (local dev — there is nothing to log in to).

use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::Deserialize;

use crate::error::AppError;
use crate::nextcloud::identity;
use crate::session::{COOKIE_NAME, UserSession, create_session};
use crate::state::{AppState, OAUTH_TTL_SECS};

/// Names the sign-in this browser started, so the callback can be matched to it
/// even when the identity provider loses the `state` it was given.
const PENDING_COOKIE: &str = "signin";

fn session_cookie(value: String) -> Cookie<'static> {
    Cookie::build((COOKIE_NAME, value))
        .path("/")
        .http_only(true)
        // Not `Secure`: the isis deployment is plain http on the wg0
        // hostPort (the VPN is the transport gate), matching recall.
        .same_site(SameSite::Lax)
        .max_age(time::Duration::days(7))
        .build()
}

/// Only allow same-site internal paths as a post-login redirect target.
///
/// ⚠ **The backslash matters.** Browsers fold `\` to `/` inside a URL, so
/// `/\evil.example` is `//evil.example` in disguise — a protocol-relative URL,
/// and an open redirect out of a signed-in flow. `life`'s version of this
/// function rejects both; memview's checks only `//` and is the weaker of the
/// two. This is life's.
pub fn validate_return_to(return_to: Option<&str>) -> String {
    match return_to {
        Some(p) if p.starts_with('/') && !p[1..].starts_with(['/', '\\']) => p.to_string(),
        _ => "/".to_string(),
    }
}

#[derive(Deserialize)]
pub struct LoginQuery {
    return_to: Option<String>,
}

/// The cookie that says *this browser started a sign-in, and it was this one*.
///
/// ⚠ **`Lax`, and it must not be `Strict`.** The callback arrives as a
/// top-level navigation from the identity provider's origin, which is
/// cross-site: a `Strict` cookie is withheld on exactly that hop, so the one
/// request this exists for would be the one request without it. `Lax` is sent
/// on top-level GET navigations, which is what a callback is.
fn pending_cookie(state: String) -> Cookie<'static> {
    Cookie::build((PENDING_COOKIE, state))
        .path("/")
        .http_only(true)
        // Not `Secure`, for the reason `session_cookie` gives.
        .same_site(SameSite::Lax)
        .max_age(time::Duration::seconds(OAUTH_TTL_SECS))
        .build()
}

/// GET /login → redirect to NC's OAuth2 authorize endpoint.
pub async fn login(
    State(app): State<AppState>,
    jar: CookieJar,
    Query(q): Query<LoginQuery>,
) -> Result<(CookieJar, Redirect), AppError> {
    let auth = app.cfg.auth.as_ref().ok_or(AppError::NotFound)?;
    let state = app.create_oauth_state(q.return_to);
    // The same value goes two ways: in the URL, where the provider is supposed
    // to hand it back, and in a cookie, where nobody but this browser can. See
    // `state_to_consume` for why the second is not redundant.
    Ok((
        jar.add(pending_cookie(state.clone())),
        Redirect::to(&identity::authorize_url(auth, &state)),
    ))
}

/// Which pending sign-in this callback is for: the one named in the URL, or —
/// when the provider dropped it — the one this browser is carrying.
///
/// ⚠⚠ **Nextcloud loses the `state` it was given, so the URL cannot be the only
/// source.** Observed 2026-08-30: `authorize` is handed 48 hex characters and
/// the callback arrives as `state=&code=…`. Nextcloud stashes the value in its
/// PHP session (`LoginRedirectorController.php:95`) and reads it back at the
/// redirect (`ClientFlowLoginController.php:325`); a sign-in that crosses its
/// login page in between comes back empty, and the whole flow was unusable.
///
/// ⚠ **What the cookie is worth, and what it is not.** `state` exists to prove
/// the callback belongs to a flow *this browser* began. A `HttpOnly` cookie
/// proves that directly and cannot be read or set across origins, so for the
/// common case it is the stronger of the two. What it does NOT do is prove the
/// callback belongs to *this* attempt when the URL says nothing: a caller who
/// can make the browser follow a callback URL of their choosing, while a
/// sign-in is pending, can have that pending sign-in spend itself on their
/// authorization code. Bounded by [`OAUTH_TTL_SECS`], single-use, and behind
/// both the VPN and the Nextcloud allow-list — and accepted deliberately,
/// because the alternative on this deployment is a flow that never completes.
///
/// When both are present they must agree; disagreeing means this callback is
/// answering somebody else's attempt, and that is refused rather than guessed.
pub fn state_to_consume<'a>(
    from_url: &'a str,
    from_cookie: &'a str,
) -> Result<&'a str, &'static str> {
    match (from_url, from_cookie) {
        ("", "") => Err("This sign-in did not start here, or it took too long."),
        (url, cookie) if !url.is_empty() && !cookie.is_empty() && url != cookie => {
            Err("This link answers a different sign-in attempt.")
        }
        ("", cookie) => Ok(cookie),
        (url, _) => Ok(url),
    }
}

/// A sign-in that could not be finished, drawn for the BROWSER looking at it.
///
/// ⚠ **A navigation endpoint must not answer in JSON.** `/auth/callback` is
/// somewhere a browser is *sent*; nothing calls it as an API. On 2026-08-30 a
/// dropped `state` put `{"error":"…"}` on the screen of a phone, and a
/// recoverable "try again" was read as the application being broken. The
/// sentence had been right the whole time — only its content type was wrong.
fn sign_in_problem(status: StatusCode, said: &str) -> Response {
    let said = said
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let body = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>Sign-in did not finish</title><style>\
         body{{font:16px/1.5 system-ui,-apple-system,sans-serif;margin:0;\
         min-height:100vh;display:grid;place-items:center;padding:1.5rem;color:#1a1a1a}}\
         main{{max-width:26rem}}h1{{font-size:1.2rem;margin:0 0 .5rem}}\
         p{{margin:0 0 1.5rem;color:#555}}\
         a{{display:inline-block;padding:.65rem 1.1rem;border-radius:.5rem;\
         background:#1b6ac9;color:#fff;text-decoration:none}}\
         </style></head><body><main><h1>Sign-in did not finish</h1>\
         <p>{said}</p><a href=\"/login\">Try again</a></main></body></html>"
    );
    (
        status,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        body,
    )
        .into_response()
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

/// GET /auth/callback → exchange code, read identity, ENFORCE the
/// allow-list, then mint our stateless session cookie.
pub async fn callback(
    State(app): State<AppState>,
    jar: CookieJar,
    Query(q): Query<CallbackQuery>,
) -> Response {
    let Some(auth) = app.cfg.auth.as_ref() else {
        // Auth unconfigured is local dev, where there is nothing to log in to.
        return AppError::NotFound.into_response();
    };
    // Cleared whatever happens: a pending sign-in is spent by being answered,
    // and leaving it set would let a second callback consume it.
    let from_cookie = jar
        .get(PENDING_COOKIE)
        .map(|c| c.value().to_string())
        .unwrap_or_default();
    let jar = jar.remove(Cookie::from(PENDING_COOKIE));

    let state = match state_to_consume(&q.state.unwrap_or_default(), &from_cookie) {
        Ok(state) => state.to_string(),
        Err(said) => return sign_in_problem(StatusCode::UNAUTHORIZED, said),
    };
    let Some(pending) = app.consume_oauth_state(&state) else {
        return sign_in_problem(
            StatusCode::UNAUTHORIZED,
            "This sign-in did not start here, or it took too long.",
        );
    };
    let Some(code) = q.code else {
        return sign_in_problem(
            StatusCode::BAD_REQUEST,
            "Nextcloud sent no authorization code back.",
        );
    };

    let token = match identity::exchange_code(&app.http, auth, &code).await {
        Ok(token) => token,
        Err(e) => {
            // Logged in full, shown as one sentence: the detail here is about
            // Nextcloud's reply and means nothing to whoever is reading it.
            tracing::error!("exchanging the authorization code failed: {e:#}");
            return sign_in_problem(
                StatusCode::BAD_GATEWAY,
                "Nextcloud would not exchange the sign-in code.",
            );
        }
    };
    let nc_user = match identity::fetch_user(&app.http, auth, &token).await {
        Ok(user) => user,
        Err(e) => {
            tracing::error!("reading the Nextcloud identity failed: {e:#}");
            return sign_in_problem(
                StatusCode::BAD_GATEWAY,
                "Nextcloud would not say who you are.",
            );
        }
    };

    if !app.cfg.is_allowed(&nc_user.id) {
        tracing::warn!(
            "denied login for non-allowed Nextcloud user {:?}",
            nc_user.id
        );
        return sign_in_problem(
            StatusCode::FORBIDDEN,
            "That Nextcloud account is not allowed to use this.",
        );
    }

    let user = UserSession {
        user_id: nc_user.id,
        display_name: nc_user.display_name,
    };
    let signed = create_session(&auth.session_secret, &user);
    let dest = validate_return_to(pending.return_to.as_deref());
    (jar.add(session_cookie(signed)), Redirect::to(&dest)).into_response()
}

/// POST /logout → clear the cookie (sessions are stateless).
pub async fn logout(
    State(app): State<AppState>,
    jar: CookieJar,
) -> Result<(CookieJar, Redirect), AppError> {
    app.cfg.auth.as_ref().ok_or(AppError::NotFound)?;
    Ok((jar.remove(Cookie::from(COOKIE_NAME)), Redirect::to("/")))
}
