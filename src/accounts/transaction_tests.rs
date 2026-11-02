use super::*;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::{post, put},
};
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn revoked_admin_cannot_create_or_rename_account() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("accounts.sqlite");
    crate::init_db(&path).unwrap();
    let db = db_open(&path).unwrap();
    db.execute("INSERT INTO accounts(id,code,name,category,kind,nature,currency) VALUES(1,'1','أصلي','asset','detail','debit','YER')", []).unwrap();
    let actor = crate::identity::test_admin(&db);
    let state = AppState {
        db: Arc::new(path),
        experimental_posting: false,
        preview_no_login: false,
    };
    let app = Router::new()
        .route("/accounts", post(create))
        .route("/accounts/{id}", put(rename))
        .layer(axum::Extension(actor))
        .with_state(state);
    db.execute("DELETE FROM identity_sessions", []).unwrap();
    let requests = [
        (
            "POST",
            "/accounts",
            serde_json::json!({"code":"2","name":"جديد","parent_id":null,"category":"asset","kind":"detail","nature":"debit","currency":"YER"}),
        ),
        (
            "PUT",
            "/accounts/1",
            serde_json::json!({"name":"معدّل","expected_name":"أصلي"}),
        ),
    ];
    for (method, uri, body) in requests {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let count: i64 = db
        .query_row("SELECT count(*) FROM accounts", [], |r| r.get(0))
        .unwrap();
    let name: String = db
        .query_row("SELECT name FROM accounts WHERE id=1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(name, "أصلي");
}
