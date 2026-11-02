use super::*;
use crate::identity::session::cookie_name;

#[test]
fn preview_cookie_is_opt_in_partitioned_and_logout_matches() {
    let ordinary = cookie_value("abc", SESSION_SECONDS, true, false);
    assert!(ordinary.starts_with("smart_session=abc;"));
    assert!(ordinary.contains("SameSite=Strict; Secure"));
    assert!(!ordinary.contains("Partitioned"));
    let preview = cookie_value("abc", SESSION_SECONDS, true, true);
    assert!(preview.starts_with("smart_preview_session=abc;"));
    assert!(preview.contains("SameSite=None; Secure; Partitioned"));
    let logout = cookie_value("", 0, true, true);
    assert!(logout.starts_with("smart_preview_session=;"));
    assert!(logout.contains("Max-Age=0; SameSite=None; Secure; Partitioned"));
}

#[test]
fn cookie_parser_ignores_unrelated_malformed_items_but_rejects_ambiguous_session() {
    let mut headers = HeaderMap::new();
    assert!(matches!(inspect_cookie(&headers), SessionCookie::Missing));
    let token = "a".repeat(64);
    let name = cookie_name();
    headers.insert(
        header::COOKIE,
        HeaderValue::from_str(&format!("unrelated; {name}={token}; other=ok")).unwrap(),
    );
    assert!(matches!(inspect_cookie(&headers), SessionCookie::Valid(value) if value == token));
    headers.insert(
        header::COOKIE,
        HeaderValue::from_str(&format!("{name}=short")).unwrap(),
    );
    assert!(matches!(inspect_cookie(&headers), SessionCookie::Invalid));
    headers.insert(
        header::COOKIE,
        HeaderValue::from_str(&format!("{name}={token}; {name}={token}")).unwrap(),
    );
    assert!(matches!(inspect_cookie(&headers), SessionCookie::Invalid));
}

#[tokio::test]
async fn session_diagnostic_never_returns_the_cookie_or_token() {
    let (_tmp, state) = setup();
    let app = crate::app(state);
    let token = "a".repeat(64);
    let cases = [
        (None, "missing"),
        (Some(format!("{}=bad", cookie_name())), "invalid"),
        (Some(format!("{}={token}", cookie_name())), "not-current"),
    ];
    for (cookie, expected) in cases {
        let (status, headers, body) = call(
            app.clone(),
            "GET",
            "/api/auth/me",
            None,
            cookie.as_deref(),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(headers.get("x-smart-session-state").unwrap(), expected);
        assert!(!format!("{headers:?}{body}").contains(&token));
    }
}

#[test]
fn bootstrap_is_single_and_keeps_legacy_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.sqlite");
    crate::init_db(&path).unwrap();
    let mut db = db_open(&path).unwrap();
    db.execute("INSERT INTO accounts(code,name,category,kind,nature,currency) VALUES('100','حساب','asset','detail','debit','YER')",[]).unwrap();
    bootstrap_admin(&mut db, "أحمد", "1").unwrap();
    assert!(bootstrap_admin(&mut db, "آخر", "2").is_err());
    let hash: String = db
        .query_row("SELECT password_hash FROM identity_users", [], |r| r.get(0))
        .unwrap();
    assert!(hash.starts_with("$argon2id$"));
    assert!(!hash.contains("password=1"));
    assert!(verify_password("1", &hash));
    assert!(!verify_password("2", &hash));
    assert_eq!(
        db.query_row("SELECT count(*) FROM accounts", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    crate::init_db(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM identity_users", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
