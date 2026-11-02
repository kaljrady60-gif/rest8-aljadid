use super::*;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
    routing::{get, post},
};
use std::sync::Arc;
use tower::ServiceExt;
fn setup() -> (tempfile::TempDir, AppState, Router) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("periods.sqlite");
    crate::init_db(&path).unwrap();
    let state = AppState {
        db: Arc::new(path),
        experimental_posting: true,
        preview_no_login: false,
    };
    let actor = crate::identity::test_admin(&db_open(&state.db).unwrap());
    let app = Router::new()
        .route("/periods", get(list).post(create))
        .route("/periods/{id}/transition", post(transition))
        .route("/periods/{id}/events", get(events))
        .route("/drafts", post(crate::create))
        .route("/post/{id}", post(crate::ledger::post))
        .layer(axum::Extension(actor))
        .with_state(state.clone());
    (dir, state, app)
}
async fn req(
    app: Router,
    method: &str,
    url: &str,
    data: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut b = Request::builder().method(method).uri(url);
    let body = if let Some(v) = data {
        b = b.header("content-type", "application/json");
        v.to_string()
    } else {
        String::new()
    };
    let r = app
        .oneshot(b.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = r.status();
    let bytes = to_bytes(r.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}
async fn mkperiod(app: Router, start: &str, end: &str) -> (StatusCode, serde_json::Value) {
    req(
        app,
        "POST",
        "/periods",
        Some(serde_json::json!({"name":"فترة اختبار","start_date":start,"end_date":end})),
    )
    .await
}
async fn change(
    app: Router,
    id: i64,
    status: &str,
    allow: bool,
) -> (StatusCode, serde_json::Value) {
    req(
        app,
        "POST",
        &format!("/periods/{id}/transition"),
        Some(serde_json::json!({"expected_status":status,"approved":true,"allow_unposted":allow})),
    )
    .await
}
#[tokio::test]
async fn periods_are_nonoverlapping_and_dates_are_valid() {
    let (_dir, _state, app) = setup();
    assert_eq!(
        mkperiod(app.clone(), "2026-02-30", "2026-03-01").await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        mkperiod(app.clone(), "2026-02-01", "2026-01-01").await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        mkperiod(app.clone(), "2026-01-01", "2026-01-31").await.0,
        StatusCode::CREATED
    );
    assert_eq!(
        mkperiod(app.clone(), "2026-01-31", "2026-02-28").await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        mkperiod(app.clone(), "2026-02-01", "2026-02-28").await.0,
        StatusCode::CREATED
    );
    assert_eq!(
        mkperiod(app.clone(), "2025-12-30", "2026-03-01").await.0,
        StatusCode::CONFLICT
    );
    let (_, rows) = req(app, "GET", "/periods", None).await;
    assert_eq!(rows.as_array().unwrap().len(), 2);
}
#[tokio::test]
async fn posting_requires_open_period_and_period_transitions_keep_history() {
    let (_dir, state, app) = setup();
    let db = db_open(&state.db).unwrap();
    for (code, name) in [("1", "نقدية"), ("2", "إيراد")] {
        db.execute("INSERT INTO accounts(code,name,category,kind,nature,currency) VALUES(?1,?2,'asset','detail','debit','YER')",params![code,name]).unwrap();
    }
    let (status,d)=req(app.clone(),"POST","/drafts",Some(serde_json::json!({"description":"اختبار","date":"2026-01-15","currency":"YER","lines":[{"account_id":1,"debit":"5","credit":""},{"account_id":2,"debit":"","credit":"5"}]}))).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = d["id"].as_i64().unwrap();
    let approve = Some(serde_json::json!({"approved":true}));
    assert_eq!(
        req(app.clone(), "POST", &format!("/post/{id}"), approve.clone())
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        mkperiod(app.clone(), "2026-01-01", "2026-01-31").await.0,
        StatusCode::CREATED
    );
    let (status, r) = change(app.clone(), 1, "open", false).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(r["unposted"], 1);
    assert_eq!(change(app.clone(), 1, "open", true).await.0, StatusCode::OK);
    assert_eq!(
        req(app.clone(), "POST", &format!("/post/{id}"), approve.clone())
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        change(app.clone(), 1, "open", true).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        change(app.clone(), 1, "closed", false).await.0,
        StatusCode::OK
    );
    assert_eq!(
        req(app.clone(), "POST", &format!("/post/{id}"), approve)
            .await
            .0,
        StatusCode::CREATED
    );
    let (_, rows) = req(app.clone(), "GET", "/periods", None).await;
    assert_eq!(rows[0]["posted"], 1);
    assert_eq!(rows[0]["unposted"], 0);
    assert_eq!(rows[0]["unassigned"], 0);
    assert_eq!(
        db.query_row("SELECT period_id FROM journal_entries", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    let (_, events) = req(app, "GET", "/periods/1/events", None).await;
    assert_eq!(events.as_array().unwrap().len(), 3);
    assert_eq!(events[1]["unposted_count"], 1);
    assert!(
        db.execute("UPDATE period_events SET event='closed'", [])
            .is_err()
    );
    assert!(db.execute("DELETE FROM financial_periods", []).is_err());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_close_and_post_never_allow_posting_after_close() {
    let (_dir, state, app) = setup();
    let db = db_open(&state.db).unwrap();
    for code in ["1", "2"] {
        db.execute("INSERT INTO accounts(code,name,category,kind,nature,currency) VALUES(?1,?1,'asset','detail','debit','YER')",[code]).unwrap();
    }
    mkperiod(app.clone(), "2026-01-01", "2026-01-31").await;
    req(app.clone(),"POST","/drafts",Some(serde_json::json!({"description":"اختبار","date":"2026-01-15","currency":"YER","lines":[{"account_id":1,"debit":"5","credit":""},{"account_id":2,"debit":"","credit":"5"}]}))).await;
    let a = app.clone();
    let b = app.clone();
    let x = tokio::spawn(async move { change(a, 1, "open", true).await });
    let y = tokio::spawn(async move {
        req(
            b,
            "POST",
            "/post/1",
            Some(serde_json::json!({"approved":true})),
        )
        .await
    });
    let (closed, posted) = tokio::join!(x, y);
    assert_eq!(closed.unwrap().0, StatusCode::OK);
    assert!([StatusCode::CREATED, StatusCode::UNPROCESSABLE_ENTITY].contains(&posted.unwrap().0));
    let status: String = db
        .query_row("SELECT status FROM financial_periods WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(status, "closed");
    let (s, _) = req(
        app,
        "POST",
        "/post/1",
        Some(serde_json::json!({"approved":true})),
    )
    .await;
    assert!([StatusCode::CONFLICT, StatusCode::UNPROCESSABLE_ENTITY].contains(&s));
}
#[test]
fn upgrades_pre_period_postings_without_guessing_their_period() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.sqlite");
    crate::init_db(&path).unwrap();
    let db = db_open(&path).unwrap();
    db.execute("INSERT INTO drafts(description,date,currency,total,lines_json) VALUES('قديم','2026-01-15','YER','2','[]')",[]).unwrap();
    db.execute("INSERT INTO journal_entries(draft_id,description,entry_date,currency,total,finalized) VALUES(1,'قديم','2026-01-15','YER','2',1)",[]).unwrap();
    // Simulate schema from the immediately preceding version while preserving the existing entry.
    db.execute_batch("DROP TRIGGER journal_entries_no_update; ALTER TABLE journal_entries DROP COLUMN period_id; DROP TABLE financial_periods; DROP TABLE period_events;").unwrap();
    crate::init_db(&path).unwrap();
    crate::init_db(&path).unwrap();
    let value: Option<i64> = db
        .query_row(
            "SELECT period_id FROM journal_entries WHERE id=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(value, None);
    let date: String = db
        .query_row(
            "SELECT entry_date FROM journal_entries WHERE id=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(date, "2026-01-15");
}
#[tokio::test]
async fn period_name_and_transition_boundaries() {
    let (_dir, _state, app) = setup();
    for name in ["".to_owned(), "x".repeat(101), "a\nb".to_owned()] {
        let (status, body) = req(
            app.clone(),
            "POST",
            "/periods",
            Some(
                serde_json::json!({"name":name,"start_date":"2028-01-01","end_date":"2028-01-01"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(body["error"].as_str().unwrap().contains("اسم فترة"));
    }
    let (status,_)=req(app.clone(),"POST","/periods",Some(serde_json::json!({"name":"x".repeat(100),"start_date":"2028-01-01","end_date":"2028-01-01"}))).await;
    assert_eq!(status, StatusCode::CREATED);
    for (id, state, approved) in [(0, "open", true), (1, "invalid", true), (1, "open", false)] {
        let (status,body)=req(app.clone(),"POST",&format!("/periods/{id}/transition"),Some(serde_json::json!({"expected_status":state,"approved":approved,"allow_unposted":false}))).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(body["error"].as_str().unwrap().contains("أكد العملية"));
    }
    // A single-day period is valid and can close without overriding when there are no drafts.
    assert_eq!(change(app, 1, "open", false).await.0, StatusCode::OK);
}
#[tokio::test]
async fn revoked_actor_cannot_close_period_or_leave_an_event() {
    let (_dir, state, app) = setup();
    let (status, p) = mkperiod(app.clone(), "2030-01-01", "2030-12-31").await;
    assert_eq!(status, StatusCode::CREATED);
    let id = p["id"].as_i64().unwrap();
    let db = db_open(&state.db).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM period_events WHERE actor_id IS NOT NULL",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    db.execute("DELETE FROM identity_sessions", []).unwrap();
    let (status, _) = req(
        app,
        "POST",
        &format!("/periods/{id}/transition"),
        Some(serde_json::json!({"expected_status":"open","approved":true,"allow_unposted":true})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        db.query_row(
            "SELECT status FROM financial_periods WHERE id=?1",
            [id],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "open"
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM period_events", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
