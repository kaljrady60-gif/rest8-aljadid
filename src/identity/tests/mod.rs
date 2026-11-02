use super::*;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use std::sync::Arc;
use tower::ServiceExt;

fn setup() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.sqlite");
    crate::init_db(&path).unwrap();
    let state = AppState {
        db: Arc::new(path),
        experimental_posting: true,
        preview_no_login: false,
    };
    let mut db = db_open(&state.db).unwrap();
    bootstrap_admin(&mut db, "مدير", "١").unwrap();
    (dir, state)
}
async fn call(
    app: Router,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
    cookie: Option<&str>,
    csrf: Option<&str>,
    origin: Option<&str>,
) -> (StatusCode, HeaderMap, serde_json::Value) {
    let mut b = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "localhost");
    if let Some(v) = cookie {
        b = b.header(header::COOKIE, v)
    }
    if let Some(v) = csrf {
        b = b.header("x-csrf-token", v)
    }
    if let Some(v) = origin {
        b = b.header(header::ORIGIN, v)
    }
    let body = if let Some(v) = body {
        b = b.header(header::CONTENT_TYPE, "application/json");
        v.to_string()
    } else {
        String::new()
    };
    let res = app
        .oneshot(b.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let data = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    // استجابة بلا جسم (204/422/…) ليست خطأ في الاختبار نفسه.
    let value = serde_json::from_slice(&data).unwrap_or(serde_json::Value::Null);
    (status, headers, value)
}
async fn login(app: Router, user: &str, password: &str) -> (StatusCode, String, String) {
    let (status, headers, body) = call(
        app,
        "POST",
        "/api/auth/login",
        Some(serde_json::json!({"username":user,"password":password})),
        None,
        None,
        Some("http://localhost"),
    )
    .await;
    (
        status,
        headers
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap()
            .to_string(),
        body["csrf"].as_str().unwrap_or("").to_string(),
    )
}

mod company_accounts;
mod company_periods;
mod migration;
mod organization;
mod permissions;
mod sessions;
