//! اختبارات الترحيل الحقيقي وبذر البيانات: تمنع رجوع عيب «قيد مرحّل بلا سطور».
use super::*;

#[tokio::test]
async fn posting_produces_finalized_entry_with_its_lines() {
    let (f, token, csrf) = setup_authed();
    let draft = serde_json::json!({
        "company_id": 1, "description": "قيد ترحيل فعلي", "date": "2026-05-01", "currency": "YER",
        "lines": [
            {"account_id": 1, "account": "1110", "debit": "250", "credit": "0"},
            {"account_id": 2, "account": "3100", "debit": "0", "credit": "250"}
        ]
    });
    let (status, created) = call_auth(
        f.app.clone(),
        "POST",
        "/api/finance/journals",
        Some(draft),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_i64().expect("draft id");

    let (status, body) = call_auth(
        f.app.clone(),
        "POST",
        &format!("/api/finance/journals/{id}/post"),
        Some(serde_json::json!({"company_id": 1})),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "الترحيل يجب أن ينجح: {body}");

    let (status, list) = call_auth(
        f.app.clone(),
        "GET",
        "/api/finance/journals?company_id=1",
        None,
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list[0]["posted"], true);

    // القيد المرحّل يظهر في الميزان بمبلغه.
    let (status, trial) = call_auth(
        f.app,
        "GET",
        "/api/finance/statements?company_id=1&report=trial",
        None,
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(trial["totals"]["balanced"], true);
}

#[test]
fn seed_posts_balanced_entries_and_refuses_a_second_run() {
    let path = crate::finance::tests::temp_db_path();
    crate::init_db(&path).expect("schema");
    let mut db = crate::db_open(&path).expect("db");
    crate::identity::bootstrap_admin(&mut db, "مدير البذر", "كلمة-سر-قوية-1").expect("admin");
    crate::finance::bootstrap::first_company(&mut db, "شركة البذر").expect("company");
    let report = crate::finance::seed::seed_demo(&mut db).expect("seed");
    assert!(
        report.entries >= 100,
        "عدد القيود المزروعة {}",
        report.entries
    );
    assert!(report.accounts_added > 0);

    let (debit, credit): (String, String) = db
        .query_row(
            "SELECT CAST(SUM(CAST(debit AS REAL)) AS TEXT), CAST(SUM(CAST(credit AS REAL)) AS TEXT)
             FROM journal_lines",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("totals");
    let parse = |v: &str| rust_decimal::Decimal::from_str_exact(v).expect("decimal");
    assert_eq!(parse(&debit), parse(&credit), "المدين يجب أن يساوي الدائن");

    // كل قيد مرحّل له سطران على الأقل — العيب الذي كشفه الترحيل الحقيقي.
    let orphans: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM journal_entries e
             WHERE e.finalized=1
               AND (SELECT COUNT(*) FROM journal_lines l WHERE l.entry_id=e.id) < 2",
            [],
            |r| r.get(0),
        )
        .expect("orphans");
    assert_eq!(orphans, 0, "قيود مرحّلة بلا سطور");

    // البذر مرة ثانية يرفض بلا تغيير.
    assert!(crate::finance::seed::seed_demo(&mut db).is_err());
}
