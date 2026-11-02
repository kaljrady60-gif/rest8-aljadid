use super::*;

#[tokio::test]
async fn post_requires_opt_in_and_confirmation_and_prevents_duplicates() {
    let (_dir, state) = setup(false);
    let off = app(state.clone());
    let id = create_draft(off.clone(), "YER", [1, 2], "0.1").await;
    assert_eq!(
        request(
            off,
            "POST",
            &format!("/post/{id}"),
            Some(serde_json::json!({"approved":true}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let enabled = app(AppState {
        experimental_posting: true,
        preview_no_login: false,
        ..state.clone()
    });
    assert_eq!(
        request(
            enabled.clone(),
            "POST",
            &format!("/post/{id}"),
            Some(serde_json::json!({"approved":false}))
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        request(
            enabled.clone(),
            "POST",
            "/post/999",
            Some(serde_json::json!({"approved":true}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (status, response) = request(
        enabled.clone(),
        "POST",
        &format!("/post/{id}"),
        Some(serde_json::json!({"approved":true})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(response["entry_id"], 1);
    let (status, response) = request(
        enabled.clone(),
        "POST",
        &format!("/post/{id}"),
        Some(serde_json::json!({"approved":true})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(response["entry_id"], 1);
    let db = db_open(&state.db).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM journal_entries", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(
        db.execute("UPDATE journal_entries SET total='1' WHERE id=1", [])
            .is_err()
    );
    assert!(
        db.execute("DELETE FROM journal_entries WHERE id=1", [])
            .is_err()
    );
    assert!(
        db.execute("UPDATE journal_lines SET debit='1' WHERE entry_id=1", [])
            .is_err()
    );
    assert!(
        db.execute("DELETE FROM journal_lines WHERE entry_id=1", [])
            .is_err()
    );
    assert!(db.execute("INSERT INTO journal_lines(entry_id,line_no,account_id,account_snapshot,debit,credit) VALUES(1,2,1,'x','1','')",[]).is_err());
}
#[tokio::test]
async fn reports_use_posted_balanced_entries_separately_by_currency() {
    let (_dir, state) = setup(true);
    let app = app(state);
    let yer = create_draft(app.clone(), "YER", [1, 2], "0.1").await;
    let other = create_draft(app.clone(), "USD", [3, 4], "7").await;
    create_draft(app.clone(), "YER", [1, 2], "500").await; // unposted, excluded
    for id in [yer, other] {
        assert_eq!(
            request(
                app.clone(),
                "POST",
                &format!("/post/{id}"),
                Some(serde_json::json!({"approved":true}))
            )
            .await
            .0,
            StatusCode::CREATED
        );
    }
    let (status, trial) = request(app.clone(), "GET", "/trial?currency=YER", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(trial["debit_total"], "0.1");
    assert_eq!(trial["credit_total"], "0.1");
    assert_eq!(trial["rows"].as_array().unwrap().len(), 2);
    let (_, usd) = request(app.clone(), "GET", "/trial?currency=USD", None).await;
    assert_eq!(usd["debit_total"], "7");
    let (_, before) = request(
        app.clone(),
        "GET",
        "/trial?currency=YER&to=2026-01-01",
        None,
    )
    .await;
    assert_eq!(before["debit_total"], "0");
    assert_eq!(
        request(
            app.clone(),
            "GET",
            "/trial?currency=YER&from=2026-01-01",
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(app.clone(), "GET", "/trial?currency=YER&account_id=1", None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let (_, one) = request(
        app.clone(),
        "GET",
        "/entries?currency=YER&account_id=1",
        None,
    )
    .await;
    assert_eq!(one["movements"].as_array().unwrap().len(), 1);
    assert_eq!(
        request(
            app,
            "GET",
            "/entries?currency=YER&from=2026-02-01&to=2026-01-01",
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}
#[tokio::test]
async fn corrupt_or_legacy_drafts_are_not_posted_and_corrupt_ledger_is_not_reported() {
    let (_dir, state) = setup(true);
    let app = app(state.clone());
    let id = create_draft(app.clone(), "YER", [1, 2], "4").await;
    let db = db_open(&state.db).unwrap();
    let legacy = r#"[{"account":"قديم 1","debit":"1","credit":""},{"account":"قديم 2","debit":"","credit":"1"}]"#;
    db.execute("INSERT INTO drafts(description,date,currency,total,lines_json) VALUES('قديم','2026-01-15','YER','1',?1)",[legacy]).unwrap();
    let old_id = db.last_insert_rowid();
    assert_eq!(
        request(
            app.clone(),
            "POST",
            &format!("/post/{old_id}"),
            Some(serde_json::json!({"approved":true}))
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    db.execute("UPDATE drafts SET total='5' WHERE id=?1", [id])
        .unwrap();
    assert_eq!(
        request(
            app.clone(),
            "POST",
            &format!("/post/{id}"),
            Some(serde_json::json!({"approved":true}))
        )
        .await
        .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM journal_entries", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    db.execute("UPDATE drafts SET total='4' WHERE id=?1", [id])
        .unwrap();
    assert_eq!(
        request(
            app.clone(),
            "POST",
            &format!("/post/{id}"),
            Some(serde_json::json!({"approved":true}))
        )
        .await
        .0,
        StatusCode::CREATED
    );
    // Simulate external corruption in the isolated test database.
    db.execute_batch(
        "DROP TRIGGER journal_lines_no_update; UPDATE journal_lines SET line_no=9 WHERE line_no=1;",
    )
    .unwrap();
    assert_eq!(
        request(app.clone(), "GET", "/trial?currency=YER", None)
            .await
            .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    db.execute("UPDATE journal_lines SET line_no=1 WHERE line_no=9", [])
        .unwrap();
    db.execute_batch("PRAGMA foreign_keys=OFF; UPDATE journal_lines SET account_id=999 WHERE line_no=0; PRAGMA foreign_keys=ON;").unwrap();
    assert_eq!(
        request(app.clone(), "GET", "/trial?currency=YER", None)
            .await
            .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    db.execute("UPDATE journal_lines SET account_id=1 WHERE line_no=0", [])
        .unwrap();
    db.execute_batch(
        "DROP TRIGGER journal_lines_no_delete; DELETE FROM journal_lines WHERE line_no=1;",
    )
    .unwrap();
    assert_eq!(
        request(app.clone(), "GET", "/trial?currency=YER", None)
            .await
            .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        request(app, "GET", "/entries?currency=YER&account_id=1", None)
            .await
            .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
}
