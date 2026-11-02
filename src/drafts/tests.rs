use super::*;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
    routing::post,
};
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn stale_session_cannot_create_journal_draft() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("test.sqlite");
    crate::init_db(&path).unwrap();
    let db = db_open(&path).unwrap();
    db.execute("INSERT INTO accounts(id,code,name,category,kind,nature,currency) VALUES(1,'1','نقدية','asset','detail','debit','YER'),(2,'2','مقابل','asset','detail','debit','YER')",[]).unwrap();
    let actor = crate::identity::test_admin(&db);
    let state = AppState {
        db: Arc::new(path),
        experimental_posting: false,
        preview_no_login: false,
    };
    let app = Router::new()
        .route("/drafts", post(create))
        .layer(axum::Extension(actor))
        .with_state(state);
    db.execute("DELETE FROM identity_sessions", []).unwrap();
    let payload = serde_json::json!({"description":"قيد اختبار","date":"2026-10-30","currency":"YER","lines":[{"account_id":1,"debit":"10","credit":""},{"account_id":2,"debit":"","credit":"10"}]});
    let request = Request::builder()
        .method("POST")
        .uri("/drafts")
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(std::str::from_utf8(&body).unwrap().contains("الجلسة"));
    assert_eq!(
        db.query_row("SELECT count(*) FROM drafts", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM draft_lines", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
