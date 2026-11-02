use super::*;

#[tokio::test]
async fn two_yemen_note_types_remain_distinct_from_legacy_and_each_other() {
    let (_dir, state) = setup(true);
    let actor = crate::identity::test_admin(&db_open(&state.db).unwrap());
    let app = Router::new()
        .route("/accounts", post(crate::accounts::create))
        .route("/drafts", post(crate::create))
        .route(
            "/vouchers",
            post(crate::vouchers::create).get(crate::vouchers::list),
        )
        .route("/post/{id}", post(super::super::post))
        .route("/trial", get(super::super::trial))
        .route("/accounts/{id}", get(super::super::account_ledger))
        .layer(axum::Extension(actor))
        .with_state(state.clone());
    let mut pairs = Vec::new();
    for (tag, prefix) in [("YER-A", "A"), ("YER-S", "S")] {
        let mut ids = Vec::new();
        for suffix in [1, 2] {
            let body = serde_json::json!({"code":format!("{prefix}{suffix}"),"name":format!("تجربة {prefix}{suffix}"),"parent_id":null,"category":"asset","kind":"detail","nature":"debit","currency":tag});
            let (status, result) = request(app.clone(), "POST", "/accounts", Some(body)).await;
            assert_eq!(status, StatusCode::CREATED);
            ids.push(result["id"].as_i64().unwrap());
        }
        pairs.push(ids);
    }
    for (index, (currency, amount, token)) in [
        ("YER-A", "10.25", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ("YER-S", "20.75", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
    ]
    .into_iter()
    .enumerate()
    {
        let payload = serde_json::json!({"request_id":token,"kind":"receipt","date":"2026-01-15","currency":currency,"amount":amount,"cash_account_id":pairs[index][0],"other_account_id":pairs[index][1],"party_name":"اختبار","description":"سند اختباري","reference":""});
        let (status, voucher) = request(app.clone(), "POST", "/vouchers", Some(payload)).await;
        assert_eq!(status, StatusCode::CREATED);
        let draft = voucher["draft_id"].as_i64().unwrap();
        assert_eq!(
            request(
                app.clone(),
                "POST",
                &format!("/post/{draft}"),
                Some(serde_json::json!({"approved":true}))
            )
            .await
            .0,
            StatusCode::CREATED
        );
        let (status, trial) = request(
            app.clone(),
            "GET",
            &format!("/trial?currency={currency}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(trial["debit_total"], amount);
        assert_eq!(trial["credit_total"], amount);
        assert_eq!(trial["rows"].as_array().unwrap().len(), 2);
        let (status, history) = request(
            app.clone(),
            "GET",
            &format!("/vouchers?currency={currency}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(history["rows"].as_array().unwrap().len(), 1);
        assert_eq!(history["rows"][0]["currency"], currency);
    }
    // Legacy YER is kept as a distinct unclassified tag, not silently assigned to either issue.
    let legacy_id = create_draft(app.clone(), "YER", [1, 2], "30").await;
    assert_eq!(
        request(
            app.clone(),
            "POST",
            &format!("/post/{legacy_id}"),
            Some(serde_json::json!({"approved":true}))
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let (_, legacy) = request(app.clone(), "GET", "/trial?currency=YER", None).await;
    assert_eq!(legacy["debit_total"], "30");
    let (_, aden) = request(app.clone(), "GET", "/trial?currency=YER-A", None).await;
    let (_, sanaa) = request(app.clone(), "GET", "/trial?currency=YER-S", None).await;
    assert_eq!(aden["debit_total"], "10.25");
    assert_eq!(sanaa["debit_total"], "20.75");
    let wrong = serde_json::json!({"description":"خلط","date":"2026-01-15","currency":"YER-A","lines":[{"account_id":pairs[0][0],"debit":"1","credit":""},{"account_id":pairs[1][1],"debit":"","credit":"1"}]});
    assert_eq!(
        request(app.clone(), "POST", "/drafts", Some(wrong)).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        request(
            app.clone(),
            "GET",
            &format!("/accounts/{}?currency=YER-S", pairs[0][0]),
            None
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        request(app.clone(), "GET", "/vouchers?currency=YER-X", None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    // A second schema initialization leaves all codes and history untouched.
    crate::init_db(&state.db).unwrap();
    let db = db_open(&state.db).unwrap();
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM journal_entries WHERE currency IN ('YER-A','YER-S')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
    let legacy_count: i64 = db
        .query_row(
            "SELECT count(*) FROM journal_entries WHERE currency='YER'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(legacy_count, 1);
}
