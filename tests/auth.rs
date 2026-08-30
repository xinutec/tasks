//! Post-login redirect allowlist — must not become an open redirect.
//! The same three cases `life` and `memview` pin, for the same copied function.

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

/// Which pending sign-in a callback answers, when the identity provider may or
/// may not hand back the `state` it was given.
///
/// ⚠ This is the security-relevant half of the sign-in and it is a pure
/// function precisely so it can be pinned here: the live flow needs Nextcloud,
/// which no test has, and a decision nobody can exercise is a decision nobody
/// checks.
mod which_signin {
    use tasks::routes::auth::state_to_consume;

    #[test]
    fn the_url_is_used_when_the_provider_hands_it_back() {
        // An identity provider that behaves: cookie and URL agree, and either
        // would do. This is the path every other deployment takes.
        assert_eq!(state_to_consume("abc", "abc"), Ok("abc"));
        // No cookie — expired, or cleared between the two requests. The URL
        // still names a pending sign-in, and the in-memory store is what
        // decides whether it is a real one.
        assert_eq!(state_to_consume("abc", ""), Ok("abc"));
    }

    /// ⚠ **The live case.** Nextcloud is handed 48 hex characters and returns
    /// `state=`, so without this the flow cannot complete at all.
    #[test]
    fn the_cookie_carries_it_when_the_provider_drops_it() {
        assert_eq!(state_to_consume("", "abc"), Ok("abc"));
    }

    #[test]
    fn nothing_to_go_on_is_refused() {
        assert!(state_to_consume("", "").is_err());
    }

    /// ⚠ **Two different attempts must not be spliced.** A callback naming one
    /// sign-in, arriving in a browser holding another, is answering somebody
    /// else's — the one shape where guessing would be worse than refusing.
    #[test]
    fn a_callback_for_another_attempt_is_refused() {
        assert!(state_to_consume("abc", "def").is_err());
        // And it is refused rather than quietly preferring one of them.
        assert_ne!(state_to_consume("abc", "def"), Ok("abc"));
        assert_ne!(state_to_consume("abc", "def"), Ok("def"));
    }
}
