use super::*;

#[tokio::test]
async fn authenticated_company_registry_and_grants_are_scoped() {
    let (_dir, state) = setup();
    let db = db_open(&state.db).unwrap();
    let hash = hash_password("2").unwrap();
    db.execute(
        "INSERT INTO identity_users(username,password_hash,role) VALUES('مدير ب',?1,'admin')",
        [&hash],
    )
    .unwrap();
    let user_b = db.last_insert_rowid();
    let app = crate::app(state);
    let (status, a_cookie, a_csrf) = login(app.clone(), "مدير", "١").await;
    assert_eq!(status, StatusCode::OK);
    let (status, b_cookie, b_csrf) = login(app.clone(), "مدير ب", "2").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, a) = call(
        app.clone(),
        "POST",
        "/api/finance/registry",
        Some(serde_json::json!({"type":"company","code":"a","name":"شركة أ"})),
        Some(&a_cookie),
        Some(&a_csrf),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let company_a = a["id"].as_i64().unwrap();
    let (status, _, branch) = call(
        app.clone(),
        "POST",
        "/api/finance/registry",
        Some(
            serde_json::json!({"type":"branch","company_id":company_a,"code":"a1","name":"فرع أ"}),
        ),
        Some(&a_cookie),
        Some(&a_csrf),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let branch_a = branch["id"].as_i64().unwrap();
    let (status, _, before) = call(
        app.clone(),
        "GET",
        "/api/finance/registry",
        None,
        Some(&b_cookie),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(before["companies"].as_array().unwrap().is_empty());
    assert_eq!(call(app.clone(), "POST", "/api/finance/registry",
        Some(serde_json::json!({"type":"branch","company_id":company_a,"code":"intrude","name":"تجاوز"})),
        Some(&b_cookie), Some(&b_csrf), None).await.0, StatusCode::FORBIDDEN);
    let grant = serde_json::json!({"company_id":company_a,"branch_id":branch_a,"user_id":user_b,"role":"reader","reason":"صلاحية قراءة اختبارية"});
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/finance/grants",
            Some(grant.clone()),
            Some(&a_cookie),
            Some(&a_csrf),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, _, after) = call(
        app.clone(),
        "GET",
        "/api/finance/registry",
        None,
        Some(&b_cookie),
        None,
        None,
    )
    .await;
    assert_eq!(after["companies"][0]["id"], company_a);
    assert_eq!(after["branches"][0]["id"], branch_a);
    let mut revoke = grant;
    revoke["role"] = serde_json::Value::Null;
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/finance/grants",
            Some(revoke),
            Some(&a_cookie),
            Some(&a_csrf),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, _, after_revoke) = call(
        app,
        "GET",
        "/api/finance/registry",
        None,
        Some(&b_cookie),
        None,
        None,
    )
    .await;
    assert!(after_revoke["companies"].as_array().unwrap().is_empty());
}
