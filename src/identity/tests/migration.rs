use super::*;

#[test]
fn financial_transaction_rechecks_session_role_and_expiry() {
    let (_dir, state) = setup();
    let db = db_open(&state.db).unwrap();
    let principal = test_admin(&db);
    assert!(is_current_admin(&db, &principal).unwrap());
    db.execute(
        "UPDATE identity_users SET role='viewer' WHERE id=?1",
        [principal.id],
    )
    .unwrap();
    assert!(!is_current_admin(&db, &principal).unwrap());
    db.execute(
        "UPDATE identity_users SET role='admin' WHERE id=?1",
        [principal.id],
    )
    .unwrap();
    db.execute(
        "UPDATE identity_sessions SET expires_at=0 WHERE token_hash=?1",
        [&principal.token_hash],
    )
    .unwrap();
    assert!(!is_current_admin(&db, &principal).unwrap());
}
#[test]
fn migration_keeps_posted_and_period_history_with_unknown_actor() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.sqlite");
    let db = db_open(&path).unwrap();
    db.execute_batch("CREATE TABLE drafts(id INTEGER PRIMARY KEY,description TEXT NOT NULL,date TEXT NOT NULL,currency TEXT NOT NULL,total TEXT NOT NULL,lines_json TEXT NOT NULL,created_at TEXT DEFAULT CURRENT_TIMESTAMP); INSERT INTO drafts(id,description,date,currency,total,lines_json) VALUES(1,'قديم','2026-01-01','YER','2','[]'); CREATE TABLE journal_entries(id INTEGER PRIMARY KEY,draft_id INTEGER NOT NULL UNIQUE REFERENCES drafts(id),description TEXT NOT NULL,entry_date TEXT NOT NULL,currency TEXT NOT NULL,total TEXT NOT NULL,finalized INTEGER NOT NULL DEFAULT 0,created_at TEXT DEFAULT CURRENT_TIMESTAMP); INSERT INTO journal_entries(id,draft_id,description,entry_date,currency,total,finalized) VALUES(1,1,'قديم','2026-01-01','YER','2',1); CREATE TABLE financial_periods(id INTEGER PRIMARY KEY,name TEXT NOT NULL,start_date TEXT NOT NULL,end_date TEXT NOT NULL,status TEXT NOT NULL,created_at TEXT DEFAULT CURRENT_TIMESTAMP); INSERT INTO financial_periods(id,name,start_date,end_date,status) VALUES(1,'قديمة','2026-01-01','2026-12-31','closed'); CREATE TABLE period_events(id INTEGER PRIMARY KEY,period_id INTEGER NOT NULL REFERENCES financial_periods(id),event TEXT NOT NULL,occurred_at TEXT DEFAULT CURRENT_TIMESTAMP); INSERT INTO period_events(id,period_id,event) VALUES(1,1,'created');").unwrap();
    drop(db);
    crate::init_db(&path).unwrap();
    crate::init_db(&path).unwrap();
    let db = db_open(&path).unwrap();
    let old: (String, Option<i64>, Option<i64>) = db
        .query_row(
            "SELECT total,period_id,posted_by FROM journal_entries WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(old, ("2".into(), None, None));
    assert_eq!(
        db.query_row("SELECT actor_id FROM period_events WHERE id=1", [], |r| {
            r.get::<_, Option<i64>>(0)
        })
        .unwrap(),
        None
    );
    let violations: i64 = db
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0);
}
#[tokio::test]
async fn voucher_guard_requires_csrf_and_blocks_unscoped_route() {
    let (_dir, state) = setup();
    let app = crate::app(state.clone());
    let db = db_open(&state.db).unwrap();
    db.execute("INSERT INTO accounts(id,code,name,category,kind,nature,currency) VALUES(1,'A','الصندوق','asset','detail','debit','YER'),(2,'B','المقابل','asset','detail','debit','YER')",[]).unwrap();
    let voucher = serde_json::json!({"request_id":"0123456789abcdef0123456789abcdef","kind":"receipt","date":"2026-10-30","currency":"YER","amount":"1","cash_account_id":1,"other_account_id":2,"party_name":"طرف","description":"اختبار","reference":""});
    let (status, cookie, csrf) = login(app.clone(), "مدير", "١").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/vouchers",
            Some(voucher.clone()),
            Some(&cookie),
            None,
            Some("http://localhost")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/vouchers",
            Some(voucher.clone()),
            Some(&cookie),
            Some(&csrf),
            Some("https://attacker.test")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/vouchers",
            Some(voucher),
            Some(&cookie),
            Some(&csrf),
            Some("http://localhost")
        )
        .await
        .0,
        StatusCode::CREATED
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM financial_vouchers", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
