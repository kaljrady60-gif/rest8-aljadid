use super::*;

#[tokio::test]
async fn protected_routes_login_csrf_and_logout_are_enforced() {
    let (_dir, state) = setup();
    let app = crate::app(state.clone());
    for path in [
        "/api/accounts",
        "/api/drafts",
        "/api/periods",
        "/api/ledger/config",
        "/api/ledger/entries",
        "/api/auth/me",
    ] {
        assert_eq!(
            call(app.clone(), "GET", path, None, None, None, None)
                .await
                .0,
            StatusCode::UNAUTHORIZED,
            "{path}"
        );
    }
    assert_eq!(
        login(app.clone(), "مدير", "wrong").await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, cookie, csrf) = login(app.clone(), "مدير", "١").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!cookie.is_empty());
    assert_eq!(csrf.len(), 64);
    let db = db_open(&state.db).unwrap();
    let stored: Vec<u8> = db
        .query_row("SELECT token_hash FROM identity_sessions", [], |r| r.get(0))
        .unwrap();
    assert_ne!(stored, cookie.as_bytes());
    let (status, _, body) = call(
        app.clone(),
        "GET",
        "/api/auth/me",
        None,
        Some(&cookie),
        None,
        Some("http://localhost"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["username"], "مدير");
    let account = serde_json::json!({"code":"100","name":"النقدية","parent_id":null,"category":"asset","kind":"detail","nature":"debit","currency":"YER"});
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/accounts",
            Some(account.clone()),
            Some(&cookie),
            None,
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/accounts",
            Some(account.clone()),
            Some(&cookie),
            Some(&csrf),
            Some("https://attacker.test")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM accounts", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/accounts",
            Some(account),
            Some(&cookie),
            Some(&csrf),
            Some("http://localhost")
        )
        .await
        .0,
        StatusCode::CREATED
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM accounts", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/auth/logout",
            None,
            Some(&cookie),
            Some(&csrf),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(app, "GET", "/api/accounts", None, Some(&cookie), None, None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn viewer_cannot_modify_and_expired_sessions_fail() {
    let (_dir, state) = setup();
    let db = db_open(&state.db).unwrap();
    let hash = hash_password("x").unwrap();
    db.execute(
        "INSERT INTO identity_users(username,password_hash,role) VALUES('قارئ',?1,'viewer')",
        [hash],
    )
    .unwrap();
    let app = crate::app(state.clone());
    let (status, cookie, csrf) = login(app.clone(), "قارئ", "x").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        call(
            app.clone(),
            "GET",
            "/api/accounts",
            None,
            Some(&cookie),
            None,
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/periods",
            Some(
                serde_json::json!({"name":"فترة","start_date":"2026-01-01","end_date":"2026-12-31"})
            ),
            Some(&cookie),
            Some(&csrf),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    db.execute("UPDATE identity_sessions SET expires_at=0", [])
        .unwrap();
    assert_eq!(
        call(app, "GET", "/api/accounts", None, Some(&cookie), None, None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn password_guesses_are_temporarily_locked() {
    let (_dir, state) = setup();
    let app = crate::app(state.clone());
    for _ in 0..5 {
        assert_eq!(
            login(app.clone(), "مدير", "wrong").await.0,
            StatusCode::UNAUTHORIZED
        )
    }
    assert_eq!(
        login(app.clone(), "مدير", "١").await.0,
        StatusCode::UNAUTHORIZED
    );
    let db = db_open(&state.db).unwrap();
    db.execute("UPDATE identity_attempts SET locked_until=?1", [now() - 1])
        .unwrap();
    assert_eq!(login(app, "مدير", "١").await.0, StatusCode::OK);
}
