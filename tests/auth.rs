//! Post-login redirect allowlist — must not become an open redirect.

use tasks::routes::auth::validate_return_to;

#[test]
fn allows_internal_paths() {
    assert_eq!(validate_return_to(Some("/t/4")), "/t/4");
    assert_eq!(validate_return_to(Some("/?repo=memview")), "/?repo=memview");
}

#[test]
fn rejects_open_redirects_and_falls_back_to_root() {
    assert_eq!(validate_return_to(Some("//evil.example")), "/");
    // Browsers fold `\` to `/` in URLs, so `/\evil` is `//evil` in disguise.
    assert_eq!(validate_return_to(Some("/\\evil.example")), "/");
    assert_eq!(validate_return_to(Some("https://evil.example")), "/");
    assert_eq!(validate_return_to(Some("evil")), "/");
    assert_eq!(validate_return_to(None), "/");
}

/// Which pending sign-in a callback answers, and what it may do with it.
///
/// ⚠ The security-relevant half of the sign-in, and a pure function so it can
/// be pinned here: the live flow needs Nextcloud, which no test has.
mod which_signin {
    use tasks::routes::auth::{Answer, state_to_consume};

    #[test]
    fn the_url_is_used_when_the_provider_hands_it_back() {
        // An identity provider that behaves: cookie and URL agree.
        assert_eq!(state_to_consume("abc", "abc"), Answer::Exchange("abc"));
        // No cookie: a state this browser never carried. `/login` mints one
        // for anyone, so finding it in the store proves nothing (login CSRF).
        assert!(matches!(state_to_consume("abc", ""), Answer::Refuse(_)));
    }

    /// ⚠ **The live case, and never trusted (RFC 6749 §10.12).** With `state`
    /// empty nothing ties the URL's code to this browser: anyone's code would
    /// do. The cookie names the sign-in to retry, never one to finish.
    #[test]
    fn a_dropped_state_is_retried_never_exchanged() {
        assert_eq!(state_to_consume("", "abc"), Answer::Retry("abc"));
    }

    #[test]
    fn nothing_to_go_on_is_refused() {
        assert!(matches!(state_to_consume("", ""), Answer::Refuse(_)));
    }

    /// ⚠ **Two different attempts must not be spliced.** A callback naming one
    /// sign-in, arriving in a browser holding another, is answering somebody
    /// else's.
    #[test]
    fn a_callback_for_another_attempt_is_refused() {
        assert!(matches!(state_to_consume("abc", "def"), Answer::Refuse(_)));
    }
}

/// The callback itself, through the router: what a browser is sent when
/// Nextcloud drops `state`.
mod retry {
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use tasks::config::{AuthConfig, Config};
    use tasks::routes;
    use tasks::state::AppState;
    use tower::ServiceExt;

    pub(super) fn app() -> axum::Router {
        let cfg = Config {
            database_url: String::new(),
            bind_addr: "127.0.0.1:0".into(),
            static_dir: None,
            auth: Some(AuthConfig {
                session_secret: "test-secret".into(),
                nc_base_url: "https://dash.example".into(),
                nc_internal_url: None,
                nc_client_id: "id".into(),
                nc_client_secret: "secret".into(),
                nc_redirect_uri: "https://tasks.example/auth/callback".into(),
                allowed_users: vec!["allowed-user".into()],
            }),
            agent_token: None,
        };
        let pool = sqlx::MySqlPool::connect_lazy("mysql://unused@127.0.0.1/unused")
            .expect("a lazy pool never connects");
        routes::router(AppState::new(cfg, pool, reqwest::Client::new()))
    }

    /// One GET, with the sign-in cookie if given: the status, where it points,
    /// and the sign-in cookie it sets.
    pub(super) async fn get(
        app: &axum::Router,
        uri: &str,
        signin: Option<&str>,
    ) -> (StatusCode, String, String) {
        let mut req = Request::builder().uri(uri);
        if let Some(value) = signin {
            req = req.header(header::COOKIE, format!("signin={value}"));
        }
        let res = app
            .clone()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .expect("the router answered");
        let location = res
            .headers()
            .get(header::LOCATION)
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default();
        let cookie = res
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .filter_map(|v| v.strip_prefix("signin="))
            .map(|v| v.split(';').next().unwrap_or("").to_string())
            .find(|v| !v.is_empty())
            .unwrap_or_default();
        (res.status(), location, cookie)
    }

    pub(super) fn state_in(location: &str) -> String {
        url::Url::parse(location)
            .expect("an absolute authorize URL")
            .query_pairs()
            .find(|(k, _)| k == "state")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default()
    }

    /// ⚠ **The code is dropped, not exchanged.** An exchange would reach
    /// `dash.example` and answer 502; a redirect to authorize means it never
    /// left. The fresh `state` rides in both the URL and the cookie.
    #[tokio::test]
    async fn a_swallowed_state_is_retried_never_trusted() {
        let app = app();
        let (_, to, first) = get(&app, "/login?return_to=/t/5", None).await;
        assert_eq!(state_in(&to), first, "login sets what it sends");

        let (status, to, again) =
            get(&app, "/auth/callback?state=&code=attackers", Some(&first)).await;
        assert!(status.is_redirection(), "{status}");
        assert!(to.starts_with("https://dash.example/"), "{to}");
        let fresh = state_in(&to);
        assert_eq!(fresh, again, "the retry's state is the cookie's");
        assert_ne!(fresh, first, "a retry is a new attempt");
    }

    #[tokio::test]
    async fn a_second_miss_is_refused() {
        let app = app();
        let (_, _, first) = get(&app, "/login", None).await;
        let (_, _, again) = get(&app, "/auth/callback?state=&code=a", Some(&first)).await;
        assert!(!again.is_empty(), "the first miss was not retried");
        let (status, to, _) = get(&app, "/auth/callback?state=&code=b", Some(&again)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{to}");
    }

    /// The retried attempt is spent like any other: its first `state` is no
    /// longer one a callback can name.
    #[tokio::test]
    async fn the_first_attempt_is_spent_by_its_retry() {
        let app = app();
        let (_, _, first) = get(&app, "/login", None).await;
        let (retried, _, _) = get(&app, "/auth/callback?state=&code=a", Some(&first)).await;
        assert!(
            retried.is_redirection(),
            "the miss was not retried: {retried}"
        );
        let (status, _, _) = get(&app, &format!("/auth/callback?state={first}&code=a"), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
}

/// The proof `exchange_code` demands is made only by a present `state` equal to
/// the stored one.
#[test]
fn the_proof_is_made_only_by_a_present_matching_state() {
    use tasks::nextcloud::identity::StateMatched;
    assert!(StateMatched::check("n0nce", "n0nce").is_some());
    assert!(StateMatched::check("", "").is_none());
    assert!(StateMatched::check("", "n0nce").is_none());
    assert!(StateMatched::check("nonce", "n0nce").is_none());
    assert!(StateMatched::check("n0nc", "n0nce").is_none());
}

/// Login CSRF: someone signs in up to Nextcloud on their own machine, keeps
/// the state and code, and sends a victim's browser to the callback. That
/// browser carries no sign-in cookie, so the state is refused before any code
/// is exchanged.
mod another_browser {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn a_state_this_browser_never_carried_is_refused() {
        let app = super::retry::app();
        let (_, to, _) = super::retry::get(&app, "/login", None).await;
        let state = super::retry::state_in(&to);
        // No sign-in cookie: the victim's browser never started this sign-in.
        let res = app
            .oneshot(
                Request::get(format!("/auth/callback?state={state}&code=theirs"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("the router answered");
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8_lossy(&body);
        // Refused for the state, before the code went anywhere: an exchange
        // attempt would answer that Nextcloud failed instead.
        assert!(body.contains("did not start in this browser"), "{body}");
    }
}
