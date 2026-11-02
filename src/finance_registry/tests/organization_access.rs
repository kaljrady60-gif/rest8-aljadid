use super::*;

#[tokio::test]
async fn registry_and_linked_creation_check_explicit_grants() {
    let (_dir, state, preview) = setup(true);
    build_registry(&preview).await;
    let db = db_open(&state.db).unwrap();
    let actor = crate::identity::test_admin(&db);
    let actor_id = actor.id;
    let protected = AppState {
        preview_no_login: false,
        ..state.clone()
    };
    let app = Router::new()
        .route("/registry", axum::routing::get(list).post(create))
        .route(
            "/linked",
            axum::routing::post(crate::vouchers::create_linked),
        )
        .layer(axum::Extension(actor))
        .with_state(protected);

    let (status, empty) = req(app.clone(), "GET", "/registry", None).await;
    assert_eq!(status, StatusCode::OK);
    for field in ["companies", "branches", "scopes", "parties", "funds"] {
        assert_eq!(empty[field].as_array().unwrap().len(), 0, "{field}");
    }
    assert_eq!(
        req(
            app.clone(),
            "POST",
            "/linked",
            Some(linked(&"1".repeat(32)))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        req(
            app.clone(),
            "POST",
            "/registry",
            Some(serde_json::json!({"type":"branch","company_id":1,"code":"x","name":"x"}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );

    db.execute("INSERT INTO finance_branch_grants(company_id,branch_id,user_id,role,granted_by) VALUES(1,1,?1,'reader',?1)", [actor_id]).unwrap();
    let (_, visible) = req(app.clone(), "GET", "/registry", None).await;
    assert_eq!(visible["companies"].as_array().unwrap().len(), 1);
    assert_eq!(visible["branches"].as_array().unwrap().len(), 1);
    assert_eq!(visible["funds"].as_array().unwrap().len(), 1);
    assert_eq!(visible["parties"].as_array().unwrap().len(), 1);
    assert_eq!(visible["companies"][0]["id"], 1);
    assert_eq!(
        req(
            app.clone(),
            "POST",
            "/linked",
            Some(linked(&"2".repeat(32)))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    db.execute(
        "UPDATE finance_branch_grants SET role='operator' WHERE user_id=?1",
        [actor_id],
    )
    .unwrap();
    assert_eq!(
        req(
            app.clone(),
            "POST",
            "/linked",
            Some(linked(&"3".repeat(32)))
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let mut wrong = linked(&"4".repeat(32));
    wrong["company_id"] = 2.into();
    wrong["branch_id"] = 2.into();
    wrong["fund_id"] = 2.into();
    assert_eq!(
        req(app.clone(), "POST", "/linked", Some(wrong)).await.0,
        StatusCode::FORBIDDEN
    );
    db.execute(
        "DELETE FROM finance_branch_grants WHERE user_id=?1",
        [actor_id],
    )
    .unwrap();
    let (_, empty_again) = req(app.clone(), "GET", "/registry", None).await;
    assert!(empty_again["companies"].as_array().unwrap().is_empty());
    assert_eq!(
        req(app, "POST", "/linked", Some(linked(&"5".repeat(32))))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn company_and_branch_creation_assign_explicit_grants_atomically() {
    let (_dir, state, _preview) = setup(true);
    let db = db_open(&state.db).unwrap();
    let actor = crate::identity::test_admin(&db);
    let actor_id = actor.id;
    let protected = AppState {
        preview_no_login: false,
        ..state
    };
    let app = Router::new()
        .route("/registry", axum::routing::get(list).post(create))
        .layer(axum::Extension(actor))
        .with_state(protected);
    let (status, company) = req(
        app.clone(),
        "POST",
        "/registry",
        Some(serde_json::json!({"type":"company","code":"C","name":"شركة"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let c = company["id"].as_i64().unwrap();
    assert!(access::company_admin(&db, actor_id, c).unwrap());
    let (status, branch) = req(
        app.clone(),
        "POST",
        "/registry",
        Some(serde_json::json!({"type":"branch","company_id":c,"code":"B","name":"فرع"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let b = branch["id"].as_i64().unwrap();
    assert!(access::branch_write(&db, actor_id, c, b).unwrap());
    let (_, visible) = req(app.clone(), "GET", "/registry", None).await;
    assert_eq!(visible["companies"][0]["id"], c);
    assert_eq!(visible["branches"][0]["id"], b);
    db.execute_batch("CREATE TRIGGER fail_grant BEFORE INSERT ON finance_company_admins BEGIN SELECT RAISE(ABORT,'test rollback'); END;").unwrap();
    assert_eq!(
        req(
            app,
            "POST",
            "/registry",
            Some(serde_json::json!({"type":"company","code":"rollback","name":"اختبار"}))
        )
        .await
        .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM finance_companies WHERE code='rollback'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn explicit_grants_are_authorized_audited_idempotent_and_atomic() {
    let (_dir, state, _) = setup(true);
    let db = db_open(&state.db).unwrap();
    let actor = crate::identity::test_admin(&db);
    db.execute(
        "INSERT INTO identity_users(username,password_hash,role) VALUES('reader','test','viewer')",
        [],
    )
    .unwrap();
    let target = db.last_insert_rowid();
    let protected = AppState {
        preview_no_login: false,
        ..state
    };
    let app = Router::new()
        .route("/registry", axum::routing::post(create))
        .route("/grants", axum::routing::post(change_grant))
        .layer(axum::Extension(actor))
        .with_state(protected);
    let (_, company) = req(
        app.clone(),
        "POST",
        "/registry",
        Some(serde_json::json!({"type":"company","code":"C","name":"شركة"})),
    )
    .await;
    let c = company["id"].as_i64().unwrap();
    let (_, branch) = req(
        app.clone(),
        "POST",
        "/registry",
        Some(serde_json::json!({"type":"branch","company_id":c,"code":"B","name":"فرع"})),
    )
    .await;
    let b = branch["id"].as_i64().unwrap();
    let body = serde_json::json!({"company_id":c,"branch_id":b,"user_id":target,"role":"reader","reason":"إسناد عمل"});
    let (status, changed) = req(app.clone(), "POST", "/grants", Some(body.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(changed["changed"], true);
    let (_, retry) = req(app.clone(), "POST", "/grants", Some(body.clone())).await;
    assert_eq!(retry["changed"], false);
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM finance_grant_events WHERE user_id=?1",
            [target],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    let mut missing = body.clone();
    missing.as_object_mut().unwrap().remove("role");
    assert_eq!(
        req(app.clone(), "POST", "/grants", Some(missing)).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut wrong = body.clone();
    wrong["branch_id"] = 999.into();
    wrong["role"] = "admin".into();
    assert_eq!(
        req(app.clone(), "POST", "/grants", Some(wrong)).await.0,
        StatusCode::FORBIDDEN
    );
    db.execute_batch("CREATE TRIGGER fail_grant_event BEFORE INSERT ON finance_grant_events WHEN NEW.new_role='operator' BEGIN SELECT RAISE(ABORT,'test rollback'); END;").unwrap();
    let mut update = body.clone();
    update["role"] = "operator".into();
    assert_eq!(
        req(app.clone(), "POST", "/grants", Some(update)).await.0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let role: String = db
        .query_row(
            "SELECT role FROM finance_branch_grants WHERE user_id=?1",
            [target],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(role, "reader");
    let mut revoke = body;
    revoke["role"] = serde_json::Value::Null;
    assert_eq!(
        req(app, "POST", "/grants", Some(revoke)).await.0,
        StatusCode::OK
    );
    assert!(!access::branch_write(&db, target, c, b).unwrap());
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM finance_grant_events WHERE user_id=?1",
            [target],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
    assert!(db.execute("DELETE FROM finance_grant_events", []).is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_branch_revocation_and_voucher_are_serialized() {
    let (_dir, state, preview) = setup(true);
    build_registry(&preview).await;
    let db = db_open(&state.db).unwrap();
    let actor = crate::identity::test_admin(&db);
    let actor_id = actor.id;
    db.execute(
        "INSERT INTO finance_company_admins(company_id,user_id,granted_by) VALUES(1,?1,?1)",
        [actor_id],
    )
    .unwrap();
    db.execute("INSERT INTO finance_branch_grants(company_id,branch_id,user_id,role,granted_by) VALUES(1,1,?1,'admin',?1)", [actor_id]).unwrap();
    let protected = AppState {
        preview_no_login: false,
        ..state
    };
    let app = Router::new()
        .route("/grants", axum::routing::post(change_grant))
        .route(
            "/linked",
            axum::routing::post(crate::vouchers::create_linked),
        )
        .layer(axum::Extension(actor))
        .with_state(protected);
    let revoke = serde_json::json!({"company_id":1,"branch_id":1,"user_id":actor_id,"role":null,"reason":"إلغاء الاختبار"});
    let r1 = tokio::spawn(req(app.clone(), "POST", "/grants", Some(revoke)));
    let r2 = tokio::spawn(req(
        app.clone(),
        "POST",
        "/linked",
        Some(linked(&"6".repeat(32))),
    ));
    let (revoke, voucher) = (r1.await.unwrap(), r2.await.unwrap());
    assert_eq!(revoke.0, StatusCode::OK);
    assert!([StatusCode::CREATED, StatusCode::FORBIDDEN].contains(&voucher.0));
    let counts: (i64,i64,i64) = db.query_row("SELECT (SELECT COUNT(*) FROM financial_vouchers),(SELECT COUNT(*) FROM drafts),(SELECT COUNT(*) FROM finance_voucher_context)", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    let saved = i64::from(voucher.0 == StatusCode::CREATED);
    assert_eq!(counts, (saved, saved, saved));
    assert_eq!(
        req(app, "POST", "/linked", Some(linked(&"7".repeat(32))))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn upgrading_existing_registry_never_assigns_users_or_changes_old_records() {
    let (_dir, state, preview) = setup(true);
    build_registry(&preview).await;
    let db = db_open(&state.db).unwrap();
    let actor = crate::identity::test_admin(&db);
    let before: (i64, i64, i64) = db.query_row("SELECT (SELECT COUNT(*) FROM finance_companies),(SELECT COUNT(*) FROM finance_branches),(SELECT COUNT(*) FROM finance_account_scopes)", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    crate::init_db(&state.db).unwrap();
    let after: (i64, i64, i64, i64, i64) = db.query_row("SELECT (SELECT COUNT(*) FROM finance_companies),(SELECT COUNT(*) FROM finance_branches),(SELECT COUNT(*) FROM finance_account_scopes),(SELECT COUNT(*) FROM finance_company_admins),(SELECT COUNT(*) FROM finance_branch_grants)", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).unwrap();
    assert_eq!(after, (before.0, before.1, before.2, 0, 0));
    assert!(!access::company_admin(&db, actor.id, 1).unwrap());
    assert!(!access::branch_write(&db, actor.id, 1, 1).unwrap());
}
