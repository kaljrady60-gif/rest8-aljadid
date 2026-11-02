use super::*;

#[tokio::test]
async fn server_search_filters_and_keyset_cover_older_vouchers_without_new_insert_shifts() {
    let (_dir, state, app) = setup(true);
    for i in 0..105 {
        let mut input = value(
            if i % 2 == 0 { "receipt" } else { "payment" },
            &format!("{i:032x}"),
        );
        if i == 0 {
            input["party_name"] = "طرف قديم".into();
            input["reference"] = "Older%_Reference".into();
            input["description"] = "معاملة قديمة".into();
            input["date"] = "2026-01-12".into();
        }
        assert_eq!(
            request(app.clone(), "POST", "/api/vouchers", Some(input))
                .await
                .0,
            StatusCode::CREATED
        );
    }
    let (status, first) = request(app.clone(), "GET", "/api/vouchers", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["rows"].as_array().unwrap().len(), 100);
    assert_eq!(first["ceiling"], 105);
    assert_eq!(first["next_before"], 6);
    assert_eq!(first["has_more"], true);
    assert_eq!(
        request(
            app.clone(),
            "POST",
            "/api/vouchers",
            Some(value("receipt", &"f".repeat(32)))
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let (_, older) = request(
        app.clone(),
        "GET",
        "/api/vouchers?before=6&ceiling=105",
        None,
    )
    .await;
    let ids: Vec<_> = older["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, [5, 4, 3, 2, 1]);
    assert_eq!(older["has_more"], false);
    assert!(older["next_before"].is_null());
    let (_, latest) = request(app.clone(), "GET", "/api/vouchers", None).await;
    assert_eq!(latest["ceiling"], 106);
    let (_, search) = request(
        app.clone(),
        "GET",
        "/api/vouchers?q=older%25_&kind=receipt&currency=YER&from=2026-01-01&to=2026-01-31",
        None,
    )
    .await;
    assert_eq!(search["rows"].as_array().unwrap().len(), 1);
    assert_eq!(search["rows"][0]["id"], 1);
    let encoded_party: String = "طرف قديم"
        .as_bytes()
        .iter()
        .map(|b| format!("%{b:02X}"))
        .collect();
    let (_, arabic) = request(
        app.clone(),
        "GET",
        &format!("/api/vouchers?q={encoded_party}"),
        None,
    )
    .await;
    assert_eq!(arabic["rows"].as_array().unwrap().len(), 1);
    assert_eq!(arabic["rows"][0]["id"], 1);
    let encoded_account: String = "النقدية"
        .as_bytes()
        .iter()
        .map(|b| format!("%{b:02X}"))
        .collect();
    let (_, account_search) = request(
        app.clone(),
        "GET",
        &format!("/api/vouchers?q={encoded_account}&to=2026-01-31"),
        None,
    )
    .await;
    assert_eq!(account_search["rows"][0]["id"], 1);
    let (_, wrong_kind) = request(
        app.clone(),
        "GET",
        "/api/vouchers?q=older&kind=payment",
        None,
    )
    .await;
    assert!(wrong_kind["rows"].as_array().unwrap().is_empty());
    let (_, identifier) =
        request(app.clone(), "GET", "/api/vouchers?q=1&to=2026-01-31", None).await;
    assert_eq!(identifier["rows"][0]["id"], 1);
    // Page 2 corruption fails closed rather than silently rendering bad money.
    let db = db_open(&state.db).unwrap();
    db.execute(
        "UPDATE draft_lines SET debit='19.24' WHERE draft_id=1 AND line_no=0",
        [],
    )
    .unwrap();
    assert_eq!(
        request(
            app.clone(),
            "GET",
            "/api/vouchers?before=6&ceiling=105",
            None
        )
        .await
        .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        request(app, "GET", "/api/vouchers?q=older", None).await.0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

#[tokio::test]
async fn list_rejects_bad_filters_and_requires_auth_in_protected_mode() {
    let (_dir, state, app) = setup(true);
    for query in [
        "kind=invalid",
        "currency=EUR",
        "from=2026-02-30",
        "from=2026-12-31&to=2026-01-01",
        "before=0&ceiling=5",
        "before=2",
        "before=6&ceiling=5",
        "ceiling=-1",
        "q=%00",
        "unknown=x",
        "before=abc&ceiling=5",
    ] {
        assert_eq!(
            request(app.clone(), "GET", &format!("/api/vouchers?{query}"), None)
                .await
                .0,
            StatusCode::BAD_REQUEST,
            "{query}"
        );
    }
    let long = "x".repeat(121);
    assert_eq!(
        request(app.clone(), "GET", &format!("/api/vouchers?q={long}"), None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let protected = crate::app(AppState {
        preview_no_login: false,
        ..state
    });
    assert_eq!(
        request(protected, "GET", "/api/vouchers?q=older", None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}
