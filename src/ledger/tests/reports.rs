use super::*;

#[test]
fn filter_and_report_size_boundaries() {
    let mut f = Filters {
        currency: Some("YER".into()),
        account_id: None,
        from: Some("2026-01-01".into()),
        to: Some("2026-01-01".into()),
    };
    assert!(check_filters(&f).is_ok());
    f.from = Some("2026-1-1".into());
    assert_eq!(check_filters(&f).unwrap_err(), "التاريخ غير صالح.");
    f.from = Some("2026-02-30".into());
    assert_eq!(check_filters(&f).unwrap_err(), "التاريخ غير صالح.");
    assert!(check_report_size(99_999).is_ok());
    assert!(check_report_size(100_000).is_ok());
    assert!(check_report_size(100_001).is_err());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_approval_creates_only_one_entry() {
    let (_dir, state) = setup(true);
    let app = app(state.clone());
    let id = create_draft(app.clone(), "YER", [1, 2], "3").await;
    let url = format!("/post/{id}");
    let url2 = url.clone();
    let app2 = app.clone();
    let a = tokio::spawn(async move {
        request(
            app2,
            "POST",
            &url2,
            Some(serde_json::json!({"approved":true})),
        )
        .await
    });
    let b = tokio::spawn(async move {
        request(
            app,
            "POST",
            &url,
            Some(serde_json::json!({"approved":true})),
        )
        .await
    });
    let (a, b) = tokio::join!(a, b);
    let statuses = [a.unwrap().0, b.unwrap().0];
    assert!(statuses.contains(&StatusCode::CREATED));
    assert!(statuses.contains(&StatusCode::CONFLICT));
    let db = db_open(&state.db).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM journal_entries", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
#[tokio::test]
async fn account_running_balance_includes_prior_postings_and_checks_filters() {
    let (_dir, state) = setup(true);
    let app = app(state.clone());
    for (date, first_debit, amount) in [
        ("2026-01-01", true, "0.1"),
        ("2026-01-15", false, "0.3"),
        ("2026-02-01", true, "0.4"),
    ] {
        let lines = if first_debit {
            serde_json::json!([{"account_id":1,"debit":amount,"credit":""},{"account_id":2,"debit":"","credit":amount}])
        } else {
            serde_json::json!([{"account_id":1,"debit":"","credit":amount},{"account_id":2,"debit":amount,"credit":""}])
        };
        let (status,draft)=request(app.clone(),"POST","/drafts",Some(serde_json::json!({"description":"اختبار الرصيد","date":date,"currency":"YER","lines":lines}))).await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, _) = request(
            app.clone(),
            "POST",
            &format!("/post/{}", draft["id"]),
            Some(serde_json::json!({"approved":true})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (status, data) = request(
        app.clone(),
        "GET",
        "/accounts/1?currency=YER&from=2026-01-15&to=2026-02-01",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(data["opening_debit"], "0.1");
    assert_eq!(data["opening_credit"], "0");
    assert_eq!(data["rows"].as_array().unwrap().len(), 2);
    assert_eq!(data["rows"][0]["running_credit"], "0.2");
    assert_eq!(data["rows"][1]["running_debit"], "0.2");
    assert_eq!(data["closing_debit"], "0.2");
    assert_eq!(data["closing_credit"], "0");
    let (_, empty) = request(
        app.clone(),
        "GET",
        "/accounts/1?currency=YER&from=2026-03-01",
        None,
    )
    .await;
    assert_eq!(empty["opening_debit"], "0.2");
    assert!(empty["rows"].as_array().unwrap().is_empty());
    assert_eq!(empty["closing_debit"], "0.2");
    let (_, early) = request(
        app.clone(),
        "GET",
        "/accounts/1?currency=YER&to=2025-12-31",
        None,
    )
    .await;
    assert_eq!(early["closing_debit"], "0");
    assert_eq!(
        request(app.clone(), "GET", "/accounts/999?currency=YER", None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(app.clone(), "GET", "/accounts/3?currency=YER", None)
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        request(
            app.clone(),
            "GET",
            "/accounts/1?currency=YER&from=2026-02-01&to=2026-01-01",
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            app.clone(),
            "GET",
            "/accounts/1?currency=YER&account_id=2",
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let db = db_open(&state.db).unwrap();
    db.execute_batch("DROP TRIGGER journal_lines_no_update; UPDATE journal_lines SET debit='not-money' WHERE entry_id=1 AND line_no=0").unwrap();
    assert_eq!(
        request(app, "GET", "/accounts/1?currency=YER&from=2026-01-15", None)
            .await
            .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
}
#[tokio::test]
async fn revoked_session_cannot_post_or_leave_partial_entry() {
    let (_dir, state) = setup(true);
    let app = app(state.clone());
    let id = create_draft(app.clone(), "YER", [1, 2], "5").await;
    let db = db_open(&state.db).unwrap();
    db.execute("DELETE FROM identity_sessions", []).unwrap();
    let (status, _) = request(
        app,
        "POST",
        &format!("/post/{id}"),
        Some(serde_json::json!({"approved":true})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM journal_entries", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM journal_lines", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn account_name_and_movements_share_the_read_snapshot() {
    let (_dir, state) = setup(true);
    let app = app(state.clone());
    let id = create_draft(app.clone(), "YER", [1, 2], "5").await;
    assert_eq!(
        request(
            app,
            "POST",
            &format!("/post/{id}"),
            Some(serde_json::json!({"approved":true}))
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let reader = db_open(&state.db).unwrap();
    let tx = reader.unchecked_transaction().unwrap();
    let name: String = tx
        .query_row("SELECT name FROM accounts WHERE id=1", [], |r| r.get(0))
        .unwrap();
    let writer = db_open(&state.db).unwrap();
    writer
        .execute("UPDATE accounts SET name='اسم جديد' WHERE id=1", [])
        .unwrap();
    let filter = Filters {
        currency: Some("YER".into()),
        account_id: Some(1),
        from: None,
        to: None,
    };
    let movements =
        crate::ledger::reports::validated_movements_in_snapshot(&tx, &filter, "YER").unwrap();
    let row = serde_json::to_value(&movements[0]).unwrap();
    assert_eq!(row["account_name"], name);
    assert_eq!(row["account_name"], "نقدية");
    tx.commit().unwrap();
    let changed: String = reader
        .query_row("SELECT name FROM accounts WHERE id=1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(changed, "اسم جديد");
}
