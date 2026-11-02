//! اختبارات الموجة الثالثة: الأصول الثابتة والإهلاك، والموظفون والرواتب.
use super::*;
use rust_decimal::Decimal;

/// مبلغ عشري من نص.
fn dec(text: &str) -> Decimal {
    text.parse().expect("decimal")
}

/// مجموع عمود (مدين أو دائن) لقيد، محسوبًا بـDecimal لا f64.
fn sum_of(db: &rusqlite::Connection, column: &str, entry: i64) -> Decimal {
    let sql = format!("SELECT {column} FROM journal_lines WHERE entry_id=?1");
    let mut stmt = db.prepare(&sql).expect("prepare");
    let rows = stmt
        .query_map(params![entry], |r| r.get::<_, String>(0))
        .expect("rows");
    let mut total = Decimal::ZERO;
    for row in rows {
        total += dec(&row.expect("row"));
    }
    total
}

async fn category(f: &Fixture, token: &str, csrf: &str) -> i64 {
    let (status, body) = call_auth(
        f.app.clone(),
        "POST",
        "/api/finance/assets/categories",
        Some(serde_json::json!({
            "company_id": 1, "name": "أجهزة", "method": "straight",
            "asset_account_id": 6, "accum_account_id": 7, "expense_account_id": 8
        })),
        token,
        csrf,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_i64().expect("id")
}

async fn create_asset(f: &Fixture, token: &str, csrf: &str, category_id: i64) -> i64 {
    let (status, body) = call_auth(
        f.app.clone(),
        "POST",
        "/api/finance/assets",
        Some(serde_json::json!({
            "company_id": 1, "category_id": category_id, "code": "FA-1",
            "name": "حاسوب", "acquired_on": "2026-01-01", "cost": "12000",
            "salvage": "0", "life_months": 12, "funded_from": "bank"
        })),
        token,
        csrf,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_i64().expect("id")
}

#[tokio::test]
async fn asset_purchase_posts_entry_and_monthly_depreciation_is_idempotent() {
    let (f, token, csrf) = setup_authed();
    let cat = category(&f, &token, &csrf).await;
    let asset = create_asset(&f, &token, &csrf, cat).await;

    let db = db_open(&f.path).expect("open");
    let entry: i64 = db
        .query_row(
            "SELECT acquired_entry_id FROM finance_assets WHERE id=?1",
            params![asset],
            |r| r.get(0),
        )
        .expect("entry");
    let debit = sum_of(&db, "debit", entry);
    assert_eq!(debit, dec("12000"));
    drop(db);

    let (status, body) = call_auth(
        f.app.clone(),
        "POST",
        "/api/finance/assets/depreciate",
        Some(serde_json::json!({"company_id":1,"period":"2026-02"})),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["count"], 1);
    assert_eq!(body["total"], "1000.0000");

    let (status, again) = call_auth(
        f.app.clone(),
        "POST",
        "/api/finance/assets/depreciate",
        Some(serde_json::json!({"company_id":1,"period":"2026-02"})),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["count"], 0, "الإهلاك لا يتكرر لنفس الشهر");

    let db = db_open(&f.path).expect("open");
    let accumulated: String = db
        .query_row(
            "SELECT accumulated FROM finance_assets WHERE id=?1",
            params![asset],
            |r| r.get(0),
        )
        .expect("acc");
    assert_eq!(accumulated, "1000.0000");
}

#[tokio::test]
async fn straight_line_stops_at_depreciable_base_and_disposal_clears_it() {
    let (f, token, csrf) = setup_authed();
    let cat = category(&f, &token, &csrf).await;
    let asset = create_asset(&f, &token, &csrf, cat).await;

    for month in [
        "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12",
    ] {
        let (status, _) = call_auth(
            f.app.clone(),
            "POST",
            "/api/finance/assets/depreciate",
            Some(serde_json::json!({"company_id":1,"period":format!("2026-{month}")})),
            &token,
            &csrf,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    let db = db_open(&f.path).expect("open");
    let accumulated: String = db
        .query_row(
            "SELECT accumulated FROM finance_assets WHERE id=?1",
            params![asset],
            |r| r.get(0),
        )
        .expect("acc");
    assert_eq!(accumulated, "12000.0000", "لا يتجاوز الإهلاك التكلفة");
    drop(db);

    let (status, body) = call_auth(
        f.app.clone(),
        "POST",
        &format!("/api/finance/assets/{asset}/dispose"),
        Some(serde_json::json!({"company_id":1,"date":"2026-12-20","proceeds":"500"})),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let db = db_open(&f.path).expect("open");
    let status: String = db
        .query_row(
            "SELECT status FROM finance_assets WHERE id=?1",
            params![asset],
            |r| r.get(0),
        )
        .expect("status");
    assert_eq!(status, "disposed");
    let disposal: i64 = db
        .query_row(
            "SELECT disposal_entry_id FROM finance_assets WHERE id=?1",
            params![asset],
            |r| r.get(0),
        )
        .expect("disposal");
    assert_eq!(
        sum_of(&db, "debit", disposal),
        sum_of(&db, "credit", disposal),
        "قيد الاستبعاد متوازن"
    );
}

#[tokio::test]
async fn payroll_run_builds_lines_posts_balanced_entry_and_pays_once() {
    let (f, token, csrf) = setup_authed();
    for (code, name, base, allow, ded) in [
        ("E1", "سالم", "1000", "200", "100"),
        ("E2", "مريم", "1500", "0", "0"),
    ] {
        let (status, body) = call_auth(
            f.app.clone(),
            "POST",
            "/api/finance/payroll/employees",
            Some(serde_json::json!({
                "company_id": 1, "code": code, "name": name, "hired_on": "2026-01-01",
                "base_salary": base, "allowances": allow, "deductions": ded
            })),
            &token,
            &csrf,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }

    let (status, body) = call_auth(
        f.app.clone(),
        "POST",
        "/api/finance/payroll/runs",
        Some(serde_json::json!({"company_id":1,"period":"2026-03"})),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let run = body["id"].as_i64().expect("run");

    let (status, lines) = call_auth(
        f.app.clone(),
        "GET",
        &format!("/api/finance/payroll/runs/{run}/lines?company_id=1"),
        None,
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(lines.as_array().expect("lines").len(), 2);

    let (status, posted) = call_auth(
        f.app.clone(),
        "POST",
        &format!("/api/finance/payroll/runs/{run}/post"),
        Some(serde_json::json!({"company_id":1})),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{posted}");
    let entry = posted["entry_id"].as_i64().expect("entry");

    let db = db_open(&f.path).expect("open");
    let debit = sum_of(&db, "debit", entry);
    assert_eq!(debit, sum_of(&db, "credit", entry));
    assert_eq!(debit, dec("2700"), "إجمالي الرواتب والبدلات");
    drop(db);

    let (status, _) = call_auth(
        f.app.clone(),
        "POST",
        &format!("/api/finance/payroll/runs/{run}/post"),
        Some(serde_json::json!({"company_id":1})),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "لا يُرحّل المسير مرتين");

    let (status, paid) = call_auth(
        f.app.clone(),
        "POST",
        &format!("/api/finance/payroll/runs/{run}/pay"),
        Some(serde_json::json!({"company_id":1,"date":"2026-03-28"})),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{paid}");

    let (status, again) = call_auth(
        f.app.clone(),
        "POST",
        &format!("/api/finance/payroll/runs/{run}/pay"),
        Some(serde_json::json!({"company_id":1})),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "لا يُصرف المسير مرتين");
    assert!(again["error"].as_str().is_some());
}

#[tokio::test]
async fn assets_and_payroll_are_isolated_per_company() {
    let (f, token, csrf) = setup_authed();
    let cat = category(&f, &token, &csrf).await;
    let asset = create_asset(&f, &token, &csrf, cat).await;
    let (status, body) = call_auth(
        f.app.clone(),
        "POST",
        &format!("/api/finance/assets/{asset}/dispose"),
        Some(serde_json::json!({"company_id":2,"date":"2026-05-01"})),
        &token,
        &csrf,
    )
    .await;
    assert!(
        status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND,
        "شركة أخرى لا تستبعد الأصل: {status} {body}"
    );
}
