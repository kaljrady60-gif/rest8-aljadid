use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn periods_are_company_scoped_audited_and_closed_atomically() {
    let (_dir, state) = setup();
    let db = db_open(&state.db).unwrap();
    let hash = hash_password("2").unwrap();
    db.execute(
        "INSERT INTO identity_users(username,password_hash,role) VALUES('مستخدم ثان',?1,'admin')",
        [hash],
    )
    .unwrap();
    let app = crate::app(state.clone());
    let (_, ac, at) = login(app.clone(), "مدير", "١").await;
    let (_, bc, bt) = login(app.clone(), "مستخدم ثان", "2").await;
    async fn company(app: Router, c: &str, t: &str, code: &str) -> i64 {
        let (st, _, v) = call(
            app,
            "POST",
            "/api/finance/registry",
            Some(serde_json::json!({"type":"company","code":code,"name":code})),
            Some(c),
            Some(t),
            None,
        )
        .await;
        assert_eq!(st, StatusCode::CREATED);
        v["id"].as_i64().unwrap()
    }
    let a = company(app.clone(), &ac, &at, "A").await;
    let b = company(app.clone(), &bc, &bt, "B").await;
    let period = |company_id| serde_json::json!({"company_id":company_id,"name":"2026","start_date":"2026-01-01","end_date":"2026-12-31","reason":"تأسيس"});
    let (st, _, pa) = call(
        app.clone(),
        "POST",
        "/api/finance/periods",
        Some(period(a)),
        Some(&ac),
        Some(&at),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let aid = pa["id"].as_i64().unwrap();
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/finance/periods",
            Some(period(a)),
            Some(&ac),
            Some(&at),
            None
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/finance/periods",
            Some(period(b)),
            Some(&bc),
            Some(&bt),
            None
        )
        .await
        .0,
        StatusCode::CREATED
    );
    assert_eq!(
        call(
            app.clone(),
            "GET",
            &format!("/api/finance/periods?company_id={a}"),
            None,
            Some(&bc),
            None,
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (_, _, list) = call(
        app.clone(),
        "GET",
        &format!("/api/finance/periods?company_id={a}"),
        None,
        Some(&ac),
        None,
        None,
    )
    .await;
    assert_eq!(list["periods"].as_array().unwrap().len(), 1);
    let change = serde_json::json!({"expected_status":"open","approved":true,"allow_unposted":false,"reason":"إقفال مصرح"});
    let path = format!("/api/finance/periods/{aid}/transition");
    assert_eq!(
        call(
            app.clone(),
            "POST",
            &path,
            Some(change.clone()),
            Some(&bc),
            Some(&bt),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let c1 = ac.clone();
    let c2 = ac.clone();
    let t1 = at.clone();
    let t2 = at.clone();
    let path1 = path.clone();
    let path2 = path.clone();
    let l = app.clone();
    let r = app.clone();
    let c = change.clone();
    let left = tokio::spawn(async move {
        call(l, "POST", &path1, Some(c), Some(&c1), Some(&t1), None)
            .await
            .0
    });
    let right = tokio::spawn(async move {
        call(r, "POST", &path2, Some(change), Some(&c2), Some(&t2), None)
            .await
            .0
    });
    let mut statuses = [left.await.unwrap().as_u16(), right.await.unwrap().as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 409]);
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM period_events WHERE period_id=?1",
            [aid],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
    let reopen = serde_json::json!({"expected_status":"closed","approved":true,"allow_unposted":false,"reason":"تصحيح موثق"});
    db.execute_batch("CREATE TRIGGER reject_period_reason BEFORE INSERT ON finance_period_reasons WHEN NEW.reason='تصحيح موثق' BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert_eq!(
        call(
            app.clone(),
            "POST",
            &path,
            Some(reopen.clone()),
            Some(&ac),
            Some(&at),
            None
        )
        .await
        .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let status: String = db
        .query_row(
            "SELECT status FROM financial_periods WHERE id=?1",
            [aid],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "closed");
    db.execute_batch("DROP TRIGGER reject_period_reason")
        .unwrap();
    assert_eq!(
        call(app, "POST", &path, Some(reopen), Some(&ac), Some(&at), None)
            .await
            .0,
        StatusCode::OK
    );
    assert!(
        db.execute("UPDATE finance_period_reasons SET reason='x'", [])
            .is_err()
    );
    assert!(
        db.execute(
            "UPDATE finance_period_context SET company_id=?1 WHERE period_id=?2",
            params![b, aid]
        )
        .is_err()
    );
    assert_eq!(
        crate::periods::open_period(&db, "2026-06-01").unwrap(),
        None
    );
}
