use super::*;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use std::sync::Arc;
use tower::ServiceExt;

fn setup(preview: bool) -> (tempfile::TempDir, AppState, Router) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("finance.sqlite");
    crate::init_db(&path).unwrap();
    let db = db_open(&path).unwrap();
    for (id, currency) in [(1, "YER"), (2, "YER"), (3, "USD"), (4, "YER"), (5, "YER")] {
        db.execute("INSERT INTO accounts(id,code,name,category,kind,nature,currency) VALUES(?1,?2,?3,'asset','detail','debit',?4)",params![id,format!("{id}"),format!("حساب {id}"),currency]).unwrap();
    }
    let state = AppState {
        db: Arc::new(path),
        experimental_posting: false,
        preview_no_login: preview,
    };
    let app = crate::app(state.clone());
    (dir, state, app)
}
async fn req(
    app: Router,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.map(|v| v.to_string()).unwrap_or_default()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}
async fn registry(app: Router, body: serde_json::Value) -> (StatusCode, serde_json::Value) {
    req(app, "POST", "/api/finance/registry", Some(body)).await
}
async fn build_registry(app: &Router) {
    for body in [
        serde_json::json!({"type":"company","code":"c1","name":"شركة ١"}),
        serde_json::json!({"type":"company","code":"c2","name":"شركة ٢"}),
        serde_json::json!({"type":"branch","company_id":1,"code":"b1","name":"فرع ١"}),
        serde_json::json!({"type":"branch","company_id":2,"code":"b2","name":"فرع ٢"}),
        serde_json::json!({"type":"account_scope","company_id":1,"account_id":1}),
        serde_json::json!({"type":"account_scope","company_id":1,"account_id":2}),
        serde_json::json!({"type":"account_scope","company_id":1,"account_id":3}),
        serde_json::json!({"type":"account_scope","company_id":2,"account_id":4}),
        serde_json::json!({"type":"fund","company_id":1,"branch_id":1,"account_id":1,"name":"صندوق","kind":"cash"}),
        serde_json::json!({"type":"party","company_id":1,"branch_id":1,"account_id":2,"name":"عميل","role":"customer"}),
        serde_json::json!({"type":"fund","company_id":2,"branch_id":2,"account_id":4,"name":"بنك","kind":"bank"}),
    ] {
        assert_eq!(registry(app.clone(), body).await.0, StatusCode::CREATED);
    }
}
fn linked(token: &str) -> serde_json::Value {
    serde_json::json!({"request_id":token,"kind":"receipt","date":"2026-10-30","currency":"YER","amount":"12.50",
        "company_id":1,"branch_id":1,"fund_id":1,"party_id":1,"description":"قبض تجريبي","reference":"REF"})
}

#[tokio::test]
async fn scoped_setup_voucher_guards_and_atomic_rollback() {
    let (_dir, state, app) = setup(true);
    build_registry(&app).await;
    let (_, list) = req(app.clone(), "GET", "/api/finance/registry", None).await;
    assert_eq!(list["companies"].as_array().unwrap().len(), 2);
    assert_eq!(list["funds"].as_array().unwrap().len(), 2);
    for body in [
        serde_json::json!({"type":"branch","company_id":999,"code":"x","name":"فرع"}),
        serde_json::json!({"type":"fund","company_id":1,"branch_id":2,"account_id":3,"name":"اختلاط","kind":"cash"}),
        serde_json::json!({"type":"party","company_id":1,"branch_id":1,"account_id":4,"name":"اختلاط","role":"customer"}),
    ] {
        assert_eq!(
            registry(app.clone(), body).await.0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    assert_eq!(
        registry(
            app.clone(),
            serde_json::json!({"type":"account_scope","company_id":2,"account_id":1})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(registry(app.clone(),serde_json::json!({"type":"party","company_id":1,"branch_id":1,"account_id":1,"name":"تعارض","role":"customer"})).await.0,StatusCode::CONFLICT);
    let idempotent = linked(&"a".repeat(32));
    let (status, created) = req(
        app.clone(),
        "POST",
        "/api/vouchers/linked",
        Some(idempotent.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, replay) = req(
        app.clone(),
        "POST",
        "/api/vouchers/linked",
        Some(idempotent.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(created["id"], replay["id"]);
    let mut different = idempotent;
    different["amount"] = "13".into();
    assert_eq!(
        req(app.clone(), "POST", "/api/vouchers/linked", Some(different))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let draft_id = created["draft_id"].as_i64().unwrap();
    let (_, draft) = req(app.clone(), "GET", &format!("/api/drafts/{draft_id}"), None).await;
    assert_eq!(draft["lines"][0]["debit"], "12.50");
    assert_eq!(draft["lines"][1]["credit"], "12.50");
    let db = db_open(&state.db).unwrap();
    let context:(i64,i64,i64,i64)=db.query_row("SELECT company_id,branch_id,party_id,fund_id FROM finance_voucher_context WHERE voucher_id=?1",[created["id"].as_i64().unwrap()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
    assert_eq!(context, (1, 1, 1, 1));
    let mut wrong = linked(&"b".repeat(32));
    wrong["fund_id"] = 2.into();
    assert_eq!(
        req(app.clone(), "POST", "/api/vouchers/linked", Some(wrong))
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut wrong = linked(&"c".repeat(32));
    wrong["currency"] = "USD".into();
    assert_eq!(
        req(app.clone(), "POST", "/api/vouchers/linked", Some(wrong))
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let unscoped = serde_json::json!({"description":"خارج النطاق","date":"2026-10-30","currency":"YER","lines":[{"account_id":1,"debit":"1","credit":""},{"account_id":2,"debit":"","credit":"1"}]});
    assert_eq!(
        req(app.clone(), "POST", "/api/drafts", Some(unscoped))
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    db.execute_batch("CREATE TRIGGER fail_link BEFORE INSERT ON finance_voucher_context BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert_eq!(
        req(
            app.clone(),
            "POST",
            "/api/vouchers/linked",
            Some(linked(&"d".repeat(32)))
        )
        .await
        .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let counts:(i64,i64,i64)=db.query_row("SELECT (SELECT count(*) FROM financial_vouchers),(SELECT count(*) FROM drafts),(SELECT count(*) FROM finance_voucher_context)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(counts, (1, 1, 1));
    assert_eq!(
        req(
            app,
            "POST",
            &format!("/api/ledger/post/{draft_id}"),
            Some(serde_json::json!({"approved":true}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn registry_does_not_claim_legacy_used_accounts_or_allow_anonymous_in_protected_mode() {
    let (_dir, state, app) = setup(true);
    assert_eq!(
        registry(
            app.clone(),
            serde_json::json!({"type":"company","code":"c","name":"شركة"})
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let draft = serde_json::json!({"description":"قديم","date":"2026-10-30","currency":"YER","lines":[{"account_id":1,"debit":"1","credit":""},{"account_id":2,"debit":"","credit":"1"}]});
    assert_eq!(
        req(app.clone(), "POST", "/api/drafts", Some(draft)).await.0,
        StatusCode::CREATED
    );
    assert_eq!(
        registry(
            app,
            serde_json::json!({"type":"account_scope","company_id":1,"account_id":1})
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let protected = crate::app(AppState {
        preview_no_login: false,
        ..state
    });
    assert_eq!(
        req(protected.clone(), "GET", "/api/finance/registry", None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        req(
            protected,
            "POST",
            "/api/vouchers/linked",
            Some(linked(&"e".repeat(32)))
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_duplicate_linked_requests_save_once() {
    let (_dir, state, app) = setup(true);
    build_registry(&app).await;
    let body = linked(&"f".repeat(32));
    let a = tokio::spawn(req(
        app.clone(),
        "POST",
        "/api/vouchers/linked",
        Some(body.clone()),
    ));
    let b = tokio::spawn(req(app, "POST", "/api/vouchers/linked", Some(body)));
    let (a, b) = (a.await.unwrap(), b.await.unwrap());
    let mut statuses = [a.0.as_u16(), b.0.as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 201]);
    assert_eq!(a.1["id"], b.1["id"]);
    let db = db_open(&state.db).unwrap();
    let counts:(i64,i64,i64)=db.query_row("SELECT (SELECT count(*) FROM financial_vouchers),(SELECT count(*) FROM drafts),(SELECT count(*) FROM finance_voucher_context)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(counts, (1, 1, 1));
}

#[tokio::test]
async fn experimental_posting_rejects_linked_and_revoked_admin_cannot_write() {
    let (_dir, state, app) = setup(true);
    build_registry(&app).await;
    let (_, voucher) = req(
        app,
        "POST",
        "/api/vouchers/linked",
        Some(linked(&"9".repeat(32))),
    )
    .await;
    let db = db_open(&state.db).unwrap();
    let actor = crate::identity::test_admin(&db);
    let protected = AppState {
        preview_no_login: false,
        experimental_posting: true,
        ..state.clone()
    };
    let direct = Router::new()
        .route("/post/{id}", axum::routing::post(crate::ledger::post))
        .route("/registry", axum::routing::post(create))
        .route(
            "/linked",
            axum::routing::post(crate::vouchers::create_linked),
        )
        .layer(axum::Extension(actor))
        .with_state(protected);
    let (status, body) = req(
        direct.clone(),
        "POST",
        &format!("/post/{}", voucher["draft_id"]),
        Some(serde_json::json!({"approved":true})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"].as_str().unwrap().contains("عزل الشركة"));
    db.execute("DELETE FROM identity_sessions", []).unwrap();
    assert_eq!(
        req(
            direct.clone(),
            "POST",
            "/registry",
            Some(serde_json::json!({"type":"company","code":"no","name":"محظور"}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        req(direct, "POST", "/linked", Some(linked(&"8".repeat(32))))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let count: i64 = db
        .query_row("SELECT count(*) FROM journal_entries", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

mod organization_access;
