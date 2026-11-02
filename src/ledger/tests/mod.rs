use super::*;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
    routing::{get, post},
};
use std::sync::Arc;
use tower::ServiceExt;
fn setup(enabled: bool) -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.sqlite");
    crate::init_db(&path).unwrap();
    let db = db_open(&path).unwrap();
    db.execute("INSERT INTO financial_periods(name,start_date,end_date,status) VALUES('فترة اختبار','2026-01-01','2026-12-31','open')",[]).unwrap();
    for (id, code, name, currency) in [
        (1, "1", "نقدية", "YER"),
        (2, "2", "إيراد", "YER"),
        (3, "3", "دولار", "USD"),
        (4, "4", "مقابل", "USD"),
    ] {
        db.execute("INSERT INTO accounts(id,code,name,category,kind,nature,currency) VALUES(?1,?2,?3,'asset','detail','debit',?4)",params![id,code,name,currency]).unwrap();
    }
    (
        dir,
        AppState {
            db: Arc::new(path),
            experimental_posting: enabled,
            preview_no_login: false,
        },
    )
}
fn app(state: AppState) -> Router {
    let actor = crate::identity::test_admin(&db_open(&state.db).unwrap());
    Router::new()
        .route("/drafts", post(crate::create))
        .route("/post/{id}", post(super::post))
        .route("/trial", get(super::trial))
        .route("/entries", get(super::entries))
        .route("/accounts/{id}", get(super::account_ledger))
        .layer(axum::Extension(actor))
        .with_state(state)
}
async fn request(
    app: Router,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    let data = if let Some(body) = body {
        builder = builder.header("content-type", "application/json");
        body.to_string()
    } else {
        String::new()
    };
    let res = app
        .oneshot(builder.body(Body::from(data)).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}
async fn create_draft(app: Router, currency: &str, ids: [i64; 2], amount: &str) -> i64 {
    let (status,body)=request(app,"POST","/drafts",Some(serde_json::json!({"description":"اختبار","date":"2026-01-15","currency":currency,"lines":[{"account_id":ids[0],"debit":amount,"credit":""},{"account_id":ids[1],"debit":"","credit":amount}]}))).await;
    assert_eq!(status, StatusCode::CREATED);
    body["id"].as_i64().unwrap()
}

mod posting;
mod reports;

mod yemen;
