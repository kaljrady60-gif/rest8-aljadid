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
    let path = dir.path().join("test.sqlite");
    crate::init_db(&path).unwrap();
    let db = db_open(&path).unwrap();
    db.execute("INSERT INTO accounts(id,code,name,category,kind,nature,currency) VALUES(1,'100','النقدية','asset','detail','debit','YER'),(2,'200','العميل','asset','detail','debit','YER'),(3,'300','دولار','asset','detail','debit','USD'),(4,'400','رئيسي','asset','group','debit','YER')",[]).unwrap();
    let state = AppState {
        db: Arc::new(path),
        experimental_posting: false,
        preview_no_login: preview,
    };
    let router = crate::app(state.clone());
    (dir, state, router)
}
fn value(kind: &str, request_id: &str) -> serde_json::Value {
    serde_json::json!({"kind":kind,"request_id":request_id,"date":"2026-10-30","currency":"YER","amount":"19.25","cash_account_id":1,"other_account_id":2,"party_name":"أحمد","description":"تسوية حساب","reference":"مرجع-١"})
}
async fn request(
    app: Router,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.map(|v| v.to_string()).unwrap_or_default()))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}
#[tokio::test]
async fn receipt_payment_are_balanced_linked_and_idempotent() {
    let (_dir, state, app) = setup(true);
    let a = value("receipt", &"a".repeat(32));
    let b = value("payment", &"b".repeat(32));
    let (code, receipt) = request(app.clone(), "POST", "/api/vouchers", Some(a.clone())).await;
    assert_eq!(code, StatusCode::CREATED);
    let (code, replay) = request(app.clone(), "POST", "/api/vouchers", Some(a.clone())).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(receipt["id"], replay["id"]);
    assert_eq!(replay["created"], false);
    let mut altered = a;
    altered["amount"] = "30".into();
    assert_eq!(
        request(app.clone(), "POST", "/api/vouchers", Some(altered))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let (code, payment) = request(app.clone(), "POST", "/api/vouchers", Some(b)).await;
    assert_eq!(code, StatusCode::CREATED);
    for (v, first_side, second_side) in
        [(&receipt, "debit", "credit"), (&payment, "credit", "debit")]
    {
        let (code, draft) = request(
            app.clone(),
            "GET",
            &format!("/api/drafts/{}", v["draft_id"]),
            None,
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(draft["lines"][0][first_side], "19.25");
        assert_eq!(draft["lines"][1][second_side], "19.25");
        assert_eq!(draft["totals"]["debit"], "19.25");
        assert_eq!(draft["status"], "draft");
    }
    let (code, list) = request(app.clone(), "GET", "/api/vouchers", None).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(list["rows"].as_array().unwrap().len(), 2);
    assert_eq!(list["has_more"], false);
    let db = db_open(&state.db).unwrap();
    let counts:(i64,i64,i64)=db.query_row("SELECT (SELECT count(*) FROM financial_vouchers),(SELECT count(*) FROM drafts),(SELECT count(*) FROM journal_entries)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(counts, (2, 2, 0));
}
#[tokio::test]
async fn rejects_invalid_input_without_partial_records_and_requires_auth_when_enabled() {
    let (_dir, state, app) = setup(true);
    for (key, change) in [
        ("amount", serde_json::json!("0")),
        ("amount", serde_json::json!("-2")),
        ("amount", serde_json::json!("0.00001")),
        ("other_account_id", serde_json::json!(1)),
        ("other_account_id", serde_json::json!(3)),
        ("other_account_id", serde_json::json!(4)),
        ("other_account_id", serde_json::json!(999)),
        ("date", serde_json::json!("2026-02-30")),
        ("party_name", serde_json::json!(" ")),
        ("kind", serde_json::json!("other")),
    ] {
        let mut v = value("receipt", &"a".repeat(32));
        v[key] = change;
        assert_eq!(
            request(app.clone(), "POST", "/api/vouchers", Some(v))
                .await
                .0,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{key}"
        );
    }
    let db = db_open(&state.db).unwrap();
    let count: (i64, i64) = db
        .query_row(
            "SELECT (SELECT count(*) FROM financial_vouchers),(SELECT count(*) FROM drafts)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(count, (0, 0));
    let protected = crate::app(AppState {
        preview_no_login: false,
        ..state
    });
    assert_eq!(
        request(
            protected,
            "POST",
            "/api/vouchers",
            Some(value("receipt", &"c".repeat(32)))
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn explicit_experimental_posting_flows_to_ledger_and_trial() {
    use axum::routing::{get, post};
    let (_dir, state, _) = setup(false);
    let actor = crate::identity::test_admin(&db_open(&state.db).unwrap());
    let state = AppState {
        experimental_posting: true,
        ..state
    };
    let app = Router::new()
        .route("/vouchers", get(super::list).post(super::create))
        .route("/periods", post(crate::periods::create))
        .route("/post/{id}", post(crate::ledger::post))
        .route("/trial", get(crate::ledger::trial))
        .route("/entries", get(crate::ledger::entries))
        .layer(axum::Extension(actor))
        .with_state(state);
    assert_eq!(request(app.clone(),"POST","/periods",Some(serde_json::json!({"name":"اختبار","start_date":"2026-01-01","end_date":"2026-12-31"}))).await.0,StatusCode::CREATED);
    let (_, v) = request(
        app.clone(),
        "POST",
        "/vouchers",
        Some(value("receipt", &"d".repeat(32))),
    )
    .await;
    let path = format!("/post/{}", v["draft_id"]);
    assert_eq!(
        request(
            app.clone(),
            "POST",
            &path,
            Some(serde_json::json!({"approved":false}))
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        request(app.clone(), "GET", "/entries?currency=YER", None)
            .await
            .1["movements"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        request(
            app.clone(),
            "POST",
            &path,
            Some(serde_json::json!({"approved":true}))
        )
        .await
        .0,
        StatusCode::CREATED
    );
    assert_eq!(
        request(
            app.clone(),
            "POST",
            &path,
            Some(serde_json::json!({"approved":true}))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (_, entries) = request(app.clone(), "GET", "/entries?currency=YER", None).await;
    assert_eq!(entries["movements"].as_array().unwrap().len(), 2);
    let (_, trial) = request(app.clone(), "GET", "/trial?currency=YER", None).await;
    assert_eq!(trial["rows"].as_array().unwrap().len(), 2);
    let (_, replay) = request(
        app.clone(),
        "POST",
        "/vouchers",
        Some(value("receipt", &"d".repeat(32))),
    )
    .await;
    assert_eq!(replay["status"], "posted_experimental");
    let (_, list) = request(app, "GET", "/vouchers", None).await;
    assert_eq!(list["rows"][0]["status"], "posted_experimental");
}
#[tokio::test]
async fn failed_voucher_insert_rolls_back_draft_and_lines() {
    let (_dir, state, app) = setup(true);
    let db = db_open(&state.db).unwrap();
    db.execute_batch("CREATE TRIGGER fail_test BEFORE INSERT ON financial_vouchers BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    assert_eq!(
        request(
            app,
            "POST",
            "/api/vouchers",
            Some(value("receipt", &"e".repeat(32)))
        )
        .await
        .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let counts:(i64,i64,i64)=db.query_row("SELECT (SELECT count(*) FROM financial_vouchers),(SELECT count(*) FROM drafts),(SELECT count(*) FROM draft_lines)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(counts, (0, 0, 0));
}
#[tokio::test]
async fn revoked_session_is_rechecked_inside_write_transaction() {
    use axum::routing::post;
    let (_dir, state, _) = setup(false);
    let db = db_open(&state.db).unwrap();
    let actor = crate::identity::test_admin(&db);
    let app = Router::new()
        .route("/vouchers", post(super::create))
        .layer(axum::Extension(actor))
        .with_state(state);
    db.execute("DELETE FROM identity_sessions", []).unwrap();
    assert_eq!(
        request(
            app,
            "POST",
            "/vouchers",
            Some(value("receipt", &"f".repeat(32)))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let count: i64 = db
        .query_row("SELECT count(*) FROM drafts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}
#[tokio::test]
async fn corrupt_voucher_draft_fails_closed_and_does_not_report_empty_list() {
    let (_dir, state, app) = setup(true);
    let (_, v) = request(
        app.clone(),
        "POST",
        "/api/vouchers",
        Some(value("receipt", &"1".repeat(32))),
    )
    .await;
    let db = db_open(&state.db).unwrap();
    db.execute(
        "UPDATE draft_lines SET debit='19.24' WHERE draft_id=?1 AND line_no=0",
        [v["draft_id"].as_i64().unwrap()],
    )
    .unwrap();
    assert_eq!(
        request(app, "GET", "/api/vouchers", None).await.0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
}
#[tokio::test]
async fn list_reports_when_more_than_one_hundred_exist() {
    let (_dir, _state, app) = setup(true);
    for i in 0..101 {
        let token = format!("{i:032x}");
        assert_eq!(
            request(
                app.clone(),
                "POST",
                "/api/vouchers",
                Some(value("receipt", &token))
            )
            .await
            .0,
            StatusCode::CREATED
        );
    }
    let (status, list) = request(app, "GET", "/api/vouchers", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["rows"].as_array().unwrap().len(), 100);
    assert_eq!(list["has_more"], true);
}

mod pagination;
