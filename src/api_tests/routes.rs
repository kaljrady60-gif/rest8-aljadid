use super::*;

#[tokio::test]
async fn open_preview_is_opt_in_and_posting_remains_disabled() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("test.sqlite");
    init_db(&path).unwrap();
    let state = AppState {
        db: Arc::new(path),
        experimental_posting: false,
        preview_no_login: true,
    };
    let router = app(state.clone());
    for uri in ["/api/auth/me", "/api/accounts", "/api/ledger/config"] {
        let response = router
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        if uri == "/api/auth/me" {
            let body: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                    .unwrap();
            assert_eq!(body["preview_no_login"], true);
        } else if uri == "/api/ledger/config" {
            let body: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                    .unwrap();
            assert_eq!(body["authentication"], false);
            assert_eq!(body["experimental_posting"], false);
        }
    }
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/ledger/post/1")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"approved":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let unsafe_combination = app(AppState {
        experimental_posting: true,
        ..state.clone()
    });
    let response = unsafe_combination
        .oneshot(
            Request::builder()
                .uri("/api/accounts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let protected = app(AppState {
        preview_no_login: false,
        ..state
    });
    let response = protected
        .oneshot(
            Request::builder()
                .uri("/api/accounts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_shell_and_auth_script_are_not_cached() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("test.sqlite");
    init_db(&path).unwrap();
    let router = app(AppState {
        db: Arc::new(path),
        experimental_posting: false,
        preview_no_login: false,
    });
    for uri in ["/", "/assets/auth.js?v=20261030-4"] {
        let response = router
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
    }
}

#[tokio::test]
async fn draft_details_preserve_lines_and_expose_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("test.sqlite");
    init_db(&path).unwrap();
    let db = db_open(&path).unwrap();
    let lines = r#"[{"account":"النقدية","debit":"12.50","credit":""},{"account":"الإيرادات","debit":"","credit":"12.5"}]"#;
    db.execute(
        "INSERT INTO drafts(description,date,currency,total,lines_json) VALUES(?1,?2,?3,?4,?5)",
        params!["اختبار", "2026-01-01", "YER", "12.5", lines],
    )
    .unwrap();
    db.execute(
        "INSERT INTO drafts(description,date,currency,total,lines_json) VALUES(?1,?2,?3,?4,?5)",
        params!["تالف", "2026-01-01", "YER", "12.5", "not json"],
    )
    .unwrap();
    let state = AppState {
        db: Arc::new(path),
        experimental_posting: false,
        preview_no_login: false,
    };
    let app = Router::new()
        .route("/api/drafts/{id}", get(draft_detail))
        .route("/api/accounts", get(accounts::list).post(accounts::create))
        .route("/api/accounts/{id}", axum::routing::put(accounts::rename))
        .with_state(state);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/drafts/1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(json["lines"][0]["account"], "النقدية");
    assert_eq!(json["totals"]["debit"], "12.5");
    for (id, expected) in [
        (0, StatusCode::BAD_REQUEST),
        (2, StatusCode::INTERNAL_SERVER_ERROR),
        (999, StatusCode::NOT_FOUND),
    ] {
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/drafts/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), expected);
    }
    db.execute("UPDATE drafts SET total='13' WHERE id=1", [])
        .unwrap();
    let r = app
        .oneshot(
            Request::builder()
                .uri("/api/drafts/1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::INTERNAL_SERVER_ERROR);
}
#[tokio::test]
async fn account_rename_rejects_stale_or_missing_data() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("accounts.sqlite");
    init_db(&path).unwrap();
    let db = db_open(&path).unwrap();
    db.execute("INSERT INTO accounts(code,name,category,kind,nature,currency) VALUES('100','الأصول','asset','group','debit','YER')",[]).unwrap();
    let actor = crate::identity::test_admin(&db);
    let app = Router::new()
        .route("/api/accounts/{id}", axum::routing::put(accounts::rename))
        .layer(axum::Extension(actor))
        .with_state(AppState {
            db: Arc::new(path),
            experimental_posting: false,
            preview_no_login: false,
        });
    async fn send(app: Router, id: i64, body: serde_json::Value) -> StatusCode {
        app.oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/accounts/{id}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
    }
    assert_eq!(
        send(
            app.clone(),
            1,
            serde_json::json!({"name":"أصول جديدة","expected_name":"الأصول"})
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        send(
            app.clone(),
            1,
            serde_json::json!({"name":"تغيير آخر","expected_name":"الأصول"})
        )
        .await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(
            app.clone(),
            999,
            serde_json::json!({"name":"اسم","expected_name":"x"})
        )
        .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            app.clone(),
            1,
            serde_json::json!({"name":" ","expected_name":"أصول جديدة"})
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        send(
            app,
            1,
            serde_json::json!({"name":"اسم","expected_name":"أصول جديدة","kind":"detail"})
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let stored: String = db
        .query_row("SELECT name FROM accounts WHERE id=1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stored, "أصول جديدة");
}
