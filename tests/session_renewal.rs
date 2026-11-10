//! A used session cookie is re-minted to last a month from its last use.

use tasks::session::{UserSession, create_session, get_session, renewed, sign_value};

fn user() -> UserSession {
    UserSession {
        user_id: "user".into(),
        display_name: "User".into(),
    }
}

/// A cookie expiring at `x` (unix seconds), signed with `key`.
fn expiring_at(key: &str, x: i64) -> String {
    use base64::Engine;
    let claims = format!(r#"{{"u":"user","d":"User","x":{x}}}"#);
    sign_value(
        key,
        &base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims),
    )
}

#[test]
fn a_fresh_cookie_is_not_reminted() {
    assert!(renewed("k", &create_session("k", &user())).is_none());
}

#[test]
fn a_used_cookie_is_reminted_for_a_month_from_now() {
    let now = chrono::Utc::now().timestamp();
    let ten_days = expiring_at("k", now + 10 * 86_400);
    let fresh = renewed("k", &ten_days).expect("a cookie ten days from expiry is renewed");
    let got = get_session("k", &fresh).expect("the renewed cookie is valid");
    assert_eq!(got.user_id, "user");
    assert!(renewed("k", &fresh).is_none(), "and is fresh itself");
}

#[test]
fn an_expired_or_forged_cookie_is_never_reminted() {
    let now = chrono::Utc::now().timestamp();
    assert!(renewed("k", &expiring_at("k", now - 1)).is_none());
    assert!(renewed("k", &expiring_at("other", now + 86_400)).is_none());
}
