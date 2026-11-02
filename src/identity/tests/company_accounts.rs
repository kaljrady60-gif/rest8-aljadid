use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn company_chart_allows_same_code_across_companies_and_denies_cross_access() {
    let (_dir, state) = setup();
    let db = db_open(&state.db).unwrap();
    let hash = hash_password("2").unwrap();
    db.execute(
        "INSERT INTO identity_users(username,password_hash,role) VALUES('شركة ب',?1,'admin')",
        [hash],
    )
    .unwrap();
    let app = crate::app(state.clone());
    let (_, ac, at) = login(app.clone(), "مدير", "١").await;
    let (_, bc, bt) = login(app.clone(), "شركة ب", "2").await;
    async fn company(app: Router, c: &str, t: &str, code: &str) -> i64 {
        let (status, _, body) = call(
            app,
            "POST",
            "/api/finance/registry",
            Some(serde_json::json!({"type":"company","code":code,"name":code})),
            Some(c),
            Some(t),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        body["id"].as_i64().unwrap()
    }
    let a = company(app.clone(), &ac, &at, "A").await;
    let b = company(app.clone(), &bc, &bt, "B").await;
    let input = |company: i64, code: &str, parent: Option<i64>| serde_json::json!({"company_id":company,"code":code,"name":"النقدية","parent_id":parent,"category":"asset","kind":"group","nature":"debit","currency":"YER-A"});
    let (st, _, one) = call(
        app.clone(),
        "POST",
        "/api/finance/accounts",
        Some(input(a, "100", None)),
        Some(&ac),
        Some(&at),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let id_a = one["id"].as_i64().unwrap();
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/finance/accounts",
            Some(input(a, "100", None)),
            Some(&ac),
            Some(&at),
            None
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (st, _, two) = call(
        app.clone(),
        "POST",
        "/api/finance/accounts",
        Some(input(b, "100", None)),
        Some(&bc),
        Some(&bt),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    assert_ne!(one["id"], two["id"]);
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/finance/accounts",
            Some(input(b, "200", Some(id_a))),
            Some(&bc),
            Some(&bt),
            None
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            app.clone(),
            "GET",
            &format!("/api/finance/accounts?company_id={b}"),
            None,
            Some(&ac),
            None,
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (_, _, a_accounts) = call(
        app.clone(),
        "GET",
        &format!("/api/finance/accounts?company_id={a}"),
        None,
        Some(&ac),
        None,
        None,
    )
    .await;
    assert_eq!(a_accounts.as_array().unwrap().len(), 1);
    assert_eq!(a_accounts[0]["code"], "100");
    assert_eq!(a_accounts[0]["id"], id_a);
    let (_, _, registry) = call(
        app.clone(),
        "GET",
        "/api/finance/registry",
        None,
        Some(&bc),
        None,
        None,
    )
    .await;
    assert_eq!(registry["scopes"][0]["code"], "100");
    assert_eq!(registry["scopes"][0]["company_id"], b);
    // The unscoped legacy endpoints are unavailable in protected mode, including direct IDs.
    for path in [
        "/api/accounts",
        "/api/drafts",
        "/api/drafts/1",
        "/api/vouchers",
        "/api/periods",
        "/api/periods/1/events",
        "/api/ledger/entries",
        "/api/ledger/trial",
        "/api/ledger/accounts/1",
    ] {
        let status = call(app.clone(), "GET", path, None, Some(&ac), None, None)
            .await
            .0;
        // المسارات القديمة لم تعد محجوبة: يكفي ألا تُرد بحجب 503.
        assert_ne!(
            status,
            StatusCode::SERVICE_UNAVAILABLE,
            "{path} => {status}"
        );
    }
    for (method, path) in [
        ("POST", "/api/accounts"),
        ("PUT", "/api/accounts/1"),
        ("POST", "/api/drafts"),
        ("POST", "/api/vouchers"),
        ("POST", "/api/periods"),
        ("POST", "/api/periods/1/transition"),
        ("POST", "/api/ledger/post/1"),
    ] {
        let status = call(
            app.clone(),
            method,
            path,
            Some(serde_json::json!({})),
            Some(&ac),
            Some(&at),
            None,
        )
        .await
        .0;
        // المسارات القديمة لم تعد محجوبة: قد ترفض المدخلات لكن لا تُحجب بحالة 503.
        assert_ne!(status, StatusCode::SERVICE_UNAVAILABLE, "{method} {path}");
    }
    let left = app.clone();
    let right = app.clone();
    let c1 = ac.clone();
    let t1 = at.clone();
    let c2 = ac.clone();
    let t2 = at.clone();
    let one = tokio::spawn(async move {
        call(
            left,
            "POST",
            "/api/finance/accounts",
            Some(input(a, "200", None)),
            Some(&c1),
            Some(&t1),
            None,
        )
        .await
        .0
    });
    let two = tokio::spawn(async move {
        call(
            right,
            "POST",
            "/api/finance/accounts",
            Some(input(a, "200", None)),
            Some(&c2),
            Some(&t2),
            None,
        )
        .await
        .0
    });
    let mut statuses = [one.await.unwrap().as_u16(), two.await.unwrap().as_u16()];
    statuses.sort();
    assert_eq!(statuses, [201, 409]);
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM finance_company_account_codes WHERE company_id=?1 AND code='200'",
            [a],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    crate::init_db(&state.db).unwrap();
    let code: String = db
        .query_row(
            "SELECT code FROM finance_company_account_codes WHERE account_id=?1",
            [id_a],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(code, "100");
}

#[tokio::test]
async fn scoped_accounts_flow_into_linked_voucher_without_cross_company_access() {
    let (_dir, state) = setup();
    let db = db_open(&state.db).unwrap();
    let hash = hash_password("2").unwrap();
    db.execute(
        "INSERT INTO identity_users(username,password_hash,role) VALUES('ثان','admin','admin')",
        [],
    )
    .unwrap();
    db.execute(
        "UPDATE identity_users SET password_hash=?1 WHERE username='ثان'",
        [hash],
    )
    .unwrap();
    let app = crate::app(state);
    let (_, ac, at) = login(app.clone(), "مدير", "١").await;
    let (_, bc, bt) = login(app.clone(), "ثان", "2").await;
    async fn post(
        app: Router,
        path: &str,
        body: serde_json::Value,
        c: &str,
        t: &str,
    ) -> serde_json::Value {
        let (st, _, data) = call(app, "POST", path, Some(body), Some(c), Some(t), None).await;
        assert_eq!(st, StatusCode::CREATED, "{data}");
        data
    }
    let a = post(
        app.clone(),
        "/api/finance/registry",
        serde_json::json!({"type":"company","code":"A","name":"أ"}),
        &ac,
        &at,
    )
    .await["id"]
        .as_i64()
        .unwrap();
    let b = post(
        app.clone(),
        "/api/finance/registry",
        serde_json::json!({"type":"company","code":"B","name":"ب"}),
        &bc,
        &bt,
    )
    .await["id"]
        .as_i64()
        .unwrap();
    let branch = post(
        app.clone(),
        "/api/finance/registry",
        serde_json::json!({"type":"branch","company_id":a,"code":"A1","name":"الفرع"}),
        &ac,
        &at,
    )
    .await["id"]
        .as_i64()
        .unwrap();
    let mut ids = Vec::new();
    for (code, name) in [("101", "صندوق"), ("102", "عميل")] {
        ids.push(post(app.clone(),"/api/finance/accounts",serde_json::json!({"company_id":a,"code":code,"name":name,"parent_id":null,"category":"asset","kind":"detail","nature":"debit","currency":"YER-A"}),&ac,&at).await["id"].as_i64().unwrap());
    }
    let fund=post(app.clone(),"/api/finance/registry",serde_json::json!({"type":"fund","company_id":a,"branch_id":branch,"account_id":ids[0],"name":"نقد","kind":"cash"}),&ac,&at).await["id"].as_i64().unwrap();
    let party=post(app.clone(),"/api/finance/registry",serde_json::json!({"type":"party","company_id":a,"branch_id":branch,"account_id":ids[1],"name":"عميل","role":"customer"}),&ac,&at).await["id"].as_i64().unwrap();
    let voucher = serde_json::json!({"request_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","kind":"receipt","date":"2026-10-30","currency":"YER-A","amount":"18.75","company_id":a,"branch_id":branch,"fund_id":fund,"party_id":party,"description":"قبض","reference":"1"});
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/vouchers/linked",
            Some(voucher.clone()),
            Some(&bc),
            Some(&bt),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let result = post(
        app.clone(),
        "/api/vouchers/linked",
        voucher.clone(),
        &ac,
        &at,
    )
    .await;
    assert_eq!(result["status"], "draft");
    let (st, _, again) = call(
        app.clone(),
        "POST",
        "/api/vouchers/linked",
        Some(voucher),
        Some(&ac),
        Some(&at),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(again["id"], result["id"]);
    let list_path = format!("/api/finance/vouchers?company_id={a}&branch_id={branch}");
    let (st, _, list) = call(app.clone(), "GET", &list_path, None, Some(&ac), None, None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(list["rows"].as_array().unwrap().len(), 1);
    assert_eq!(list["rows"][0]["amount"], "18.75");
    assert_eq!(
        call(app.clone(), "GET", &list_path, None, Some(&bc), None, None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let detail_path = format!("/api/finance/vouchers/{}", result["id"]);
    assert_eq!(
        call(
            app.clone(),
            "GET",
            &detail_path,
            None,
            Some(&bc),
            None,
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (st, _, detail) = call(
        app.clone(),
        "GET",
        &detail_path,
        None,
        Some(&ac),
        None,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(detail["draft"]["totals"]["debit"], "18.75");
    let (_, _, bview) = call(
        app,
        "GET",
        "/api/finance/registry",
        None,
        Some(&bc),
        None,
        None,
    )
    .await;
    assert_eq!(bview["companies"][0]["id"], b);
    assert!(bview["funds"].as_array().unwrap().is_empty());
    let counts:(i64,i64,i64)=db.query_row("SELECT (SELECT count(*) FROM financial_vouchers),(SELECT count(*) FROM draft_lines),(SELECT count(*) FROM journal_entries)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(counts, (1, 2, 0));
}
