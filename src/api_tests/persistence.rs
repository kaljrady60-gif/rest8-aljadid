use super::*;

#[tokio::test]
async fn new_drafts_use_directory_and_do_not_trust_client_names() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("directory.sqlite");
    init_db(&path).unwrap();
    let db = db_open(&path).unwrap();
    for (code, name, kind, currency) in [
        ("1", "نقدية", "detail", "YER"),
        ("2", "مبيعات", "detail", "YER"),
        ("3", "مجموعة", "group", "YER"),
        ("4", "دولار", "detail", "USD"),
    ] {
        db.execute("INSERT INTO accounts(code,name,category,kind,nature,currency) VALUES(?1,?2,'asset',?3,'debit',?4)",params![code,name,kind,currency]).unwrap();
    }
    let actor = crate::identity::test_admin(&db);
    let app = Router::new()
        .route("/api/drafts", get(list).post(create))
        .route("/api/drafts/{id}", get(draft_detail))
        .layer(axum::Extension(actor))
        .with_state(AppState {
            db: Arc::new(path),
            experimental_posting: false,
            preview_no_login: false,
        });
    let body = serde_json::json!({"description":"دليل","date":"2026-10-01","currency":"YER","lines":[{"account_id":1,"account":"اسم مزور","debit":"12.5","credit":""},{"account_id":2,"account":"","debit":"","credit":"12.5"}]});
    async fn send(app: Router, body: serde_json::Value) -> StatusCode {
        app.oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/drafts")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
    }
    assert_eq!(send(app.clone(), body.clone()).await, StatusCode::CREATED);
    for id in [0, 3, 4, 999] {
        let mut bad = body.clone();
        bad["lines"][0]["account_id"] = id.into();
        assert_eq!(
            send(app.clone(), bad).await,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let mut old = body.clone();
    old["lines"][0]
        .as_object_mut()
        .unwrap()
        .remove("account_id");
    assert_eq!(
        send(app.clone(), old).await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut imbalance = body.clone();
    imbalance["lines"][1]["credit"] = "10".into();
    assert_eq!(
        send(app.clone(), imbalance).await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM drafts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    db.execute("UPDATE accounts SET name='تغير الاسم' WHERE id=1", [])
        .unwrap();
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
    let detail: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(detail["lines"][0]["account"], "نقدية");
    assert_eq!(detail["lines"][0]["account_id"], 1);
    assert!(db.execute("DELETE FROM accounts WHERE id=1", []).is_err());
    async fn detail_status(app: Router) -> StatusCode {
        app.oneshot(
            Request::builder()
                .uri("/api/drafts/1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
    }
    db.execute("UPDATE drafts SET lines_count=3 WHERE id=1", [])
        .unwrap();
    assert_eq!(
        detail_status(app.clone()).await,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    db.execute("UPDATE drafts SET lines_count=2 WHERE id=1", [])
        .unwrap();
    db.execute(
        "UPDATE draft_lines SET line_no=9 WHERE draft_id=1 AND line_no=1",
        [],
    )
    .unwrap();
    assert_eq!(
        detail_status(app.clone()).await,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    db.execute(
        "UPDATE draft_lines SET line_no=1 WHERE draft_id=1 AND line_no=9",
        [],
    )
    .unwrap();
    db.execute_batch("PRAGMA foreign_keys=OFF; UPDATE draft_lines SET account_id=999 WHERE draft_id=1 AND line_no=0; PRAGMA foreign_keys=ON;").unwrap();
    assert_eq!(
        detail_status(app.clone()).await,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    db.execute(
        "UPDATE draft_lines SET account_id=1 WHERE draft_id=1 AND line_no=0",
        [],
    )
    .unwrap();
    db.execute("UPDATE drafts SET lines_count=NULL WHERE id=1", [])
        .unwrap();
    assert_eq!(detail_status(app).await, StatusCode::INTERNAL_SERVER_ERROR);
}
#[tokio::test]
async fn migration_preserves_legacy_drafts_and_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("old.sqlite");
    let db = db_open(&path).unwrap();
    db.execute_batch("CREATE TABLE drafts(id INTEGER PRIMARY KEY, description TEXT NOT NULL,date TEXT NOT NULL,currency TEXT NOT NULL,total TEXT NOT NULL,lines_json TEXT NOT NULL,created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);").unwrap();
    db.execute_batch("CREATE TABLE accounts (id INTEGER PRIMARY KEY, code TEXT NOT NULL UNIQUE, name TEXT NOT NULL, parent_id INTEGER REFERENCES accounts(id), category TEXT NOT NULL, kind TEXT NOT NULL, nature TEXT NOT NULL, currency TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP); INSERT INTO accounts(code,name,category,kind,nature,currency) VALUES('100','حساب قديم','asset','detail','debit','YER');").unwrap();
    let legacy = r#"[{"account":"نقدية قديمة","debit":"5","credit":""},{"account":"إيرادات قديمة","debit":"","credit":"5"}]"#;
    db.execute("INSERT INTO drafts(description,date,currency,total,lines_json) VALUES('قديم','2026-01-01','YER','5',?1)",[legacy]).unwrap();
    init_db(&path).unwrap();
    init_db(&path).unwrap();
    let account_name: String = db
        .query_row("SELECT name FROM accounts WHERE code='100'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(account_name, "حساب قديم");
    let stored_legacy: String = db
        .query_row("SELECT lines_json FROM drafts WHERE id=1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stored_legacy, legacy);
    let app = Router::new()
        .route("/api/drafts/{id}", get(draft_detail))
        .with_state(AppState {
            db: Arc::new(path),
            experimental_posting: false,
            preview_no_login: false,
        });
    let r = app
        .oneshot(
            Request::builder()
                .uri("/api/drafts/1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let d: serde_json::Value =
        serde_json::from_slice(&to_bytes(r.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(d["lines"][0]["account"], "نقدية قديمة");
    assert!(d["lines"][0]["account_id"].is_null());
}
