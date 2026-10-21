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

/// Names the sign-in this browser started, since Nextcloud may lose `state`.
const PENDING_COOKIE: &str = "signin";

fn session_cookie(value: String) -> Cookie<'static> {
    Cookie::build((COOKIE_NAME, value))
        .path("/")
        .http_only(true)
        // Not `Secure`: isis serves plain http on the wg0 hostPort.
        .same_site(SameSite::Lax)
        .max_age(time::Duration::days(7))
        .build()
}

/// Same-site paths only, judged where a browser lands: the WHATWG parser drops
/// tabs and newlines and folds `\` to `/`, so `/\t/host` is `//host`. Returns
/// the path as that parser resolves it.
// dev-lint: sanitises redirect — resolved where the browser lands, and only the same origin passes
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

/// The cookie saying this browser started this sign-in.
///
/// ⚠ `Lax`, never `Strict`: the callback is a cross-site top-level navigation,
/// exactly the request a `Strict` cookie is withheld from.
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
    // The state goes in the URL, for the provider to hand back, and in a cookie
    // only this browser has.
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
    /// The URL's `state` is empty: drop its code and authorize again.
    Retry(&'a str),
    Refuse(&'static str),
}

/// What this callback may do.
///
/// ⚠ Nextcloud loses `state` when a sign-in crosses its login page (it keeps it
/// in the PHP session) and returns `state=&code=…`. That is retried, never
/// trusted (RFC 6749 §10.12): nothing ties that code to this browser. The retry
/// finds Nextcloud signed in, so `state` survives. Both present must agree.
pub fn state_to_consume<'a>(from_url: &'a str, from_cookie: &'a str) -> Answer<'a> {
    match (from_url, from_cookie) {
        ("", "") => Answer::Refuse("This sign-in did not start here, or it took too long."),
        ("", cookie) => Answer::Retry(cookie),
        // A state this browser doesn't carry is someone else's: `/login` mints one
        // for anyone, so the store having it proves nothing (login CSRF).
        (_, "") => {
            Answer::Refuse("This sign-in did not start in this browser, or it took too long.")
        }
        (url, cookie) if url != cookie => {
            Answer::Refuse("This link answers a different sign-in attempt.")
        }
        (url, _) => Answer::Exchange(url),
    }
}

/// A failed sign-in as a page, not JSON: a browser is sent here, and
/// `{"error":…}` on a phone reads as a broken app.
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

/// GET /auth/callback: exchange the code, read the identity, enforce the allow-list, set the session cookie.
pub async fn callback(
    State(app): State<AppState>,
    jar: CookieJar,
    Query(q): Query<CallbackQuery>,
) -> Response {
    let Some(auth) = app.cfg.auth.as_ref() else {
        return AppError::NotFound.into_response();
    };
    // Cleared whatever happens, so a second callback can't spend it.
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
        // Not exchanged: without `state` nothing ties it to this browser.
        tracing::info!("sign-in callback without state — authorizing again");
        let state = app.create_oauth_state(pending.return_to, true);
        return (
            jar.add(pending_cookie(state.clone())),
            Redirect::to(&identity::authorize_url(auth, &state)),
        )
            .into_response();
    }
    // Compared with the browser's copy: the store's would only repeat the key it was found by.
    let Some(proof) = identity::StateMatched::check(&from_url, &from_cookie) else {
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

    let token = match identity::exchange_code(&app.http, auth, &code, proof).await {
        Ok(token) => token,
        Err(e) => {
            // Logged in full; the reader gets one sentence.
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

/// POST /logout: clear the cookie (sessions are stateless); 204.
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
