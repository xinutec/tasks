//! Auth routes: Nextcloud identity login, restricted to an explicit
//! allow-list. Sessions are stateless, so logout is clearing the cookie. All
//! three routes 404 when auth is unconfigured (local dev).

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
        // hostPort, and the VPN is the transport gate.
        .same_site(SameSite::Lax)
        .max_age(time::Duration::days(7))
        .build()
}

/// Same-site paths only, judged where a browser lands: the WHATWG parser drops
/// tabs and newlines and folds `\` to `/`, so `/\t/host` is `//host`. Returns
/// the path as that parser resolves it.
pub fn validate_return_to(return_to: Option<&str>) -> String {
    let site = url::Url::parse("https://site.invalid/").expect("a fixed URL");
    match return_to
        .filter(|p| p.starts_with('/'))
        .map(|p| site.join(p))
    {
        Some(Ok(dest)) if dest.origin() == site.origin() => {
            dest[url::Position::BeforePath..].to_string()
        }
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
) -> Response {
    let Some(auth) = app.cfg.auth.as_ref() else {
        return sign_in_problem(StatusCode::NOT_FOUND, "Sign-in is not set up here.");
    };
    let state = app.create_oauth_state(q.return_to, false);
    // The same value goes two ways: in the URL, where the provider is supposed
    // to hand it back, and in a cookie, where nobody but this browser can. See
    // `state_to_consume` for what the cookie may and may not do.
    (
        jar.add(pending_cookie(state.clone())),
        Redirect::to(&identity::authorize_url(auth, &state)),
    )
        .into_response()
}

/// What a callback may do, judged from the `state` in its URL and the one this
/// browser's cookie carries.
#[derive(Debug, PartialEq, Eq)]
pub enum Answer<'a> {
    /// The URL names the sign-in: exchange its code.
    Exchange(&'a str),
    /// The URL's `state` is empty: drop its code and authorize this sign-in
    /// again.
    Retry(&'a str),
    Refuse(&'static str),
}

/// What this callback may do.
///
/// ⚠⚠ **Nextcloud loses the `state` it was given.** It stashes the value in its
/// PHP session (`LoginRedirectorController`) and reads it back at the redirect
/// (`ClientFlowLoginController`); a sign-in that crosses its login page in
/// between comes back as `state=&code=…`.
///
/// ⚠ **An empty `state` is retried, never trusted (RFC 6749 §10.12).** Without
/// it nothing ties the URL's code to this browser, so a page that makes the
/// browser follow a callback while a sign-in is pending could sign it in as
/// someone else. The retry finds Nextcloud signed in, so `state` survives it.
///
/// When both are present they must agree; disagreeing means this callback is
/// answering somebody else's attempt.
pub fn state_to_consume<'a>(from_url: &'a str, from_cookie: &'a str) -> Answer<'a> {
    match (from_url, from_cookie) {
        ("", "") => Answer::Refuse("This sign-in did not start here, or it took too long."),
        (url, cookie) if !url.is_empty() && !cookie.is_empty() && url != cookie => {
            Answer::Refuse("This link answers a different sign-in attempt.")
        }
        ("", cookie) => Answer::Retry(cookie),
        (url, _) => Answer::Exchange(url),
    }
}

/// A sign-in that could not be finished, drawn for the BROWSER looking at it.
///
/// ⚠ **A navigation endpoint must not answer in JSON.** `/auth/callback` is
/// somewhere a browser is *sent*; `{"error":"…"}` on a phone screen reads as the
/// app being broken, where the sentence says "try again".
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

    let from_url = q.state.unwrap_or_default();
    let (state, retry) = match state_to_consume(&from_url, &from_cookie) {
        Answer::Exchange(state) => (state, false),
        Answer::Retry(state) => (state, true),
        Answer::Refuse(said) => return sign_in_problem(StatusCode::UNAUTHORIZED, said),
    };
    let Some(pending) = app.consume_oauth_state(state) else {
        return sign_in_problem(
            StatusCode::UNAUTHORIZED,
            "This sign-in did not start here, or it took too long.",
        );
    };
    if retry {
        if pending.retried {
            return sign_in_problem(
                StatusCode::UNAUTHORIZED,
                "Nextcloud lost this sign-in twice.",
            );
        }
        // The code is not exchanged: without `state` nothing ties it to this
        // browser.
        tracing::info!("sign-in callback without state — authorizing again");
        let state = app.create_oauth_state(pending.return_to, true);
        return (
            jar.add(pending_cookie(state.clone())),
            Redirect::to(&identity::authorize_url(auth, &state)),
        )
            .into_response();
    }
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

/// POST /logout → clear the cookie (sessions are stateless); 204, the page reloads
/// itself.
pub async fn logout(
    State(app): State<AppState>,
    jar: CookieJar,
) -> Result<(CookieJar, StatusCode), AppError> {
    app.cfg.auth.as_ref().ok_or(AppError::NotFound)?;
    Ok((
        jar.remove(Cookie::from(COOKIE_NAME)),
        StatusCode::NO_CONTENT,
    ))
}
