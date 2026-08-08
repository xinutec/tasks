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
