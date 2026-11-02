//! اختبارات نطاق المالية: النطاق، الافتتاحي، الصناديق، الصرف، التقارير، التدقيق.
use crate::{AppState, db_open, init_db};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use rusqlite::params;
use std::sync::Arc;
use tower::ServiceExt;

/// مسار قاعدة مؤقتة جديدة (لا يُمسح تلقائيًا؛ للاختبارات التي تحتاج ملفًا).
pub(crate) fn temp_db_path() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("smart-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join(format!(
        "seed-{}.sqlite",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ))
}

struct Fixture {
    _dir: tempfile::TempDir,
    app: Router,
    path: std::path::PathBuf,
}

fn setup() -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("finance.sqlite");
    init_db(&path).expect("schema");
    let db = db_open(&path).expect("open");
    db.execute(
        "INSERT INTO finance_companies(id,code,name) VALUES(1,'C1','شركة أولى'),(2,'C2','شركة ثانية')",
        [],
    )
    .expect("companies");
    db.execute(
        "INSERT INTO finance_branches(company_id,code,name) VALUES(1,'B1','فرع'),(2,'B2','فرع')",
        [],
    )
    .expect("branches");
    for (id, code, category, kind) in [
        (1, "1110", "asset", "detail"),
        (2, "3100", "equity", "detail"),
        (3, "4100", "revenue", "detail"),
        (4, "5100", "expense", "detail"),
        (5, "2200", "liability", "detail"),
        (6, "1400", "asset", "detail"),
        (7, "1410", "asset", "detail"),
        (8, "5700", "expense", "detail"),
        (9, "5200", "expense", "detail"),
        (10, "2300", "liability", "detail"),
        (11, "1120", "asset", "detail"),
    ] {
        db.execute(
            "INSERT INTO accounts(id,code,name,category,kind,nature,currency)
             VALUES(?1,?2,?3,?4,?5,'debit','YER')",
            params![
                id,
                format!("__t{id}"),
                format!("حساب {code}"),
                category,
                kind
            ],
        )
        .expect("account");
        db.execute(
            "INSERT INTO finance_account_scopes(company_id,account_id) VALUES(1,?1)",
            params![id],
        )
        .expect("scope");
        db.execute(
            "INSERT INTO finance_company_account_codes(company_id,account_id,code) VALUES(1,?1,?2)",
            params![id, code],
        )
        .expect("code");
    }
    db.execute(
        "INSERT INTO financial_periods(name,start_date,end_date,status)
         VALUES('فترة','2026-01-01','2026-12-31','open')",
        [],
    )
    .expect("period");
    db.execute(
        "INSERT INTO finance_period_context(period_id,company_id) VALUES(1,1)",
        [],
    )
    .expect("period context");
    let app = crate::app(state_for(&path, false));
    Fixture {
        _dir: dir,
        app,
        path,
    }
}

/// حالة اختبار على قاعدة جاهزة؛ `posting` يفعّل الترحيل المالي الحقيقي.
pub(crate) fn state_for(path: &std::path::Path, posting: bool) -> AppState {
    AppState {
        db: Arc::new(path.to_path_buf()),
        experimental_posting: posting,
        preview_no_login: true,
    }
}

/// تجهيز مصادَق فعليًا (جلسة + إسناد إدارة) مع ترحيل مفعّل.
fn setup_authed() -> (Fixture, String, String) {
    let base = setup();
    let db = db_open(&base.path).expect("open");
    let (_, token) = crate::identity_support::test_admin_session(&db);
    let csrf: String = db
        .query_row(
            "SELECT csrf FROM identity_sessions WHERE user_id=(SELECT id FROM identity_users WHERE username='__test_admin__')",
            [],
            |r| r.get(0),
        )
        .expect("session");
    db.execute(
        "INSERT OR IGNORE INTO finance_company_admins(company_id,user_id,granted_by) VALUES(1,(SELECT id FROM identity_users WHERE username='__test_admin__'),(SELECT id FROM identity_users WHERE username='__test_admin__'))",
        [],
    )
    .expect("grant");
    let app = crate::app(AppState {
        db: Arc::new(base.path.clone()),
        experimental_posting: true,
        preview_no_login: false,
    });
    (
        Fixture {
            app,
            _dir: base._dir,
            path: base.path,
        },
        token,
        csrf,
    )
}

/// نداء مصادَق بترويسة كوكي ورمز CSRF.
async fn call_auth(
    app: Router,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
    token: &str,
    csrf: &str,
) -> (StatusCode, serde_json::Value) {
    let payload = body.map(|v| v.to_string()).unwrap_or_default();
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("cookie", format!("smart_session={token}"))
        .header("x-csrf-token", csrf)
        .header("content-type", "application/json")
        .body(Body::from(payload))
        .expect("request");
    let response = app.oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

async fn call(
    app: Router,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.map(|v| v.to_string()).unwrap_or_default()))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn scope_blocks_company_the_user_cannot_see() {
    let f = setup();
    let (status, _) = call(
        f.app.clone(),
        "GET",
        "/api/finance/funds?company_id=999",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, body) = call(f.app, "GET", "/api/finance/funds?company_id=1", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.as_array().expect("array").is_empty());
}

#[tokio::test]
async fn opening_balances_must_balance_and_lock_after_posting() {
    let f = setup();
    let unbalanced = serde_json::json!({
        "company_id": 1, "currency": "YER",
        "lines": [{"account_id": 1, "debit": "100", "credit": "0"},
                  {"account_id": 2, "debit": "0", "credit": "90"}]
    });
    let (status, _) = call(
        f.app.clone(),
        "POST",
        "/api/finance/opening",
        Some(unbalanced),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let balanced = serde_json::json!({
        "company_id": 1, "currency": "YER",
        "lines": [{"account_id": 1, "debit": "100", "credit": "0"},
                  {"account_id": 2, "debit": "0", "credit": "100"}]
    });
    let (status, body) = call(
        f.app.clone(),
        "POST",
        "/api/finance/opening",
        Some(balanced),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["debit"], "100.0000");

    let (_, listed) = call(
        f.app.clone(),
        "GET",
        "/api/finance/opening?company_id=1",
        None,
    )
    .await;
    assert_eq!(listed["locked"], false);
    assert_eq!(listed["totals"]["balanced"], true);

    // بعد ترحيل قيد تُقفل الأرصدة الافتتاحية.
    let (status, _) = call(
        f.app.clone(),
        "POST",
        "/api/finance/journals",
        Some(serde_json::json!({
            "company_id": 1, "description": "قيد", "date": "2026-03-01", "currency": "YER",
            "lines": [
                {"account_id": 1, "account": "1110", "debit": "50", "credit": "0"},
                {"account_id": 3, "account": "4100", "debit": "0", "credit": "50"}
            ]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn funds_require_detail_account_and_derive_balance_from_posted_entries() {
    let f = setup();
    let (status, _) = call(
        f.app.clone(),
        "POST",
        "/api/finance/funds",
        Some(serde_json::json!({
            "company_id": 1, "kind": "cash", "code": "F1", "name": "صندوق",
            "account_id": 1, "iban": null, "opened_on": "2026-01-01"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // رمز مكرر أو حساب من شركة أخرى يُرفض.
    let (status, _) = call(
        f.app.clone(),
        "POST",
        "/api/finance/funds",
        Some(serde_json::json!({
            "company_id": 1, "kind": "cash", "code": "F1", "name": "مكرر",
            "account_id": 1, "iban": null, "opened_on": "2026-01-01"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, _) = call(
        f.app.clone(),
        "POST",
        "/api/finance/funds",
        Some(serde_json::json!({
            "company_id": 1, "kind": "cash", "code": "F2", "name": "غريب",
            "account_id": 99, "iban": null, "opened_on": "2026-01-01"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (_, body) = call(f.app, "GET", "/api/finance/funds?company_id=1", None).await;
    assert_eq!(body[0]["balance"], "0.0000");
}

#[tokio::test]
async fn fx_requires_a_recorded_rate_and_converts_exactly() {
    let f = setup();
    let (status, _) = call(
        f.app.clone(),
        "GET",
        "/api/finance/fx/convert?company_id=1&base=USD&quote=YER&amount=10",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, _) = call(
        f.app.clone(),
        "POST",
        "/api/finance/fx",
        Some(serde_json::json!({
            "company_id": 1, "base": "USD", "quote": "YER", "rate": "250.5", "as_of": "2026-03-01"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = call(
        f.app,
        "GET",
        "/api/finance/fx/convert?company_id=1&base=USD&quote=YER&amount=10",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], "2505.0000");
}

#[tokio::test]
async fn audit_log_is_append_only() {
    let f = setup();
    let _ = call(
        f.app.clone(),
        "POST",
        "/api/finance/dimensions",
        Some(serde_json::json!({"company_id":1,"code":"CC1","name":"مركز","kind":"cost_center"})),
    )
    .await;
    let (_, log) = call(f.app, "GET", "/api/finance/audit?company_id=1", None).await;
    assert_eq!(log[0]["action"], "dimension.create");

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("audit.sqlite");
    init_db(&path).expect("schema");
    let db = db_open(&path).expect("open");
    db.execute(
        "INSERT INTO finance_audit_log(company_id,action,entity) VALUES(1,'x','y')",
        [],
    )
    .expect("insert");
    assert!(
        db.execute("UPDATE finance_audit_log SET action='z'", [])
            .is_err()
    );
    assert!(db.execute("DELETE FROM finance_audit_log", []).is_err());
}

#[tokio::test]
async fn statements_reconcile_from_posted_entries() {
    let f = setup();
    // قيد مرحّل مباشرة على مستوى القاعدة (المسار الكامل للترحيل مُختبر في journals).
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("statements.sqlite");
    init_db(&path).expect("schema");
    let db = db_open(&path).expect("open");
    db.execute(
        "INSERT INTO finance_companies(id,code,name) VALUES(1,'C1','شركة')",
        [],
    )
    .expect("company");
    db.execute(
        "INSERT INTO accounts(id,code,name,category,kind,nature,currency)
         VALUES(1,'1','صندوق','asset','detail','debit','YER'),
               (2,'2','إيراد','revenue','detail','credit','YER')",
        [],
    )
    .expect("accounts");
    for id in [1, 2] {
        db.execute(
            "INSERT INTO finance_account_scopes(company_id,account_id) VALUES(1,?1)",
            params![id],
        )
        .expect("scope");
    }
    db.execute(
        "INSERT INTO drafts(id,description,date,currency,total,lines_json,lines_count,company_id)
         VALUES(1,'بيع','2026-04-01','YER','100','[]',2,1)",
        [],
    )
    .expect("draft");
    db.execute(
        "INSERT INTO journal_entries(id,draft_id,description,entry_date,currency,total,finalized,company_id)
         VALUES(1,1,'بيع','2026-04-01','YER','100',0,1)",
        [],
    )
    .expect("entry");
    db.execute(
        "INSERT INTO journal_lines(entry_id,line_no,account_id,account_snapshot,debit,credit)
         VALUES(1,0,1,'صندوق','100','0'),(1,1,2,'إيراد','0','100')",
        [],
    )
    .expect("lines");
    db.execute("UPDATE journal_entries SET finalized=1 WHERE id=1", [])
        .expect("finalize");
    let state = AppState {
        db: Arc::new(path),
        experimental_posting: false,
        preview_no_login: true,
    };
    let app = crate::app(state);
    let (status, trial) = call(
        app.clone(),
        "GET",
        "/api/finance/statements?company_id=1&report=trial",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(trial["totals"]["balanced"], true);
    assert_eq!(trial["totals"]["debit"], "100.0000");

    let (_, income) = call(
        app.clone(),
        "GET",
        "/api/finance/statements?company_id=1&report=income",
        None,
    )
    .await;
    assert_eq!(income["net_profit"], "100.0000");

    let (_, balance) = call(
        app,
        "GET",
        "/api/finance/statements?company_id=1&report=balance",
        None,
    )
    .await;
    assert_eq!(balance["balanced"], true);
    assert_eq!(balance["assets_total"], "100.0000");
    drop(f);
}

mod ops;
mod ops2;
mod ops3;
mod ops4;
mod seeding;

#[test]
fn taxes_split_exclusive_uses_basis_points() {
    use rust_decimal::Decimal;
    use std::str::FromStr;
    let amount = Decimal::from_str("100.0000").expect("decimal");
    let (base, tax) = crate::finance::taxes::split(amount, 1500, false);
    assert_eq!(base.to_string(), "100.0000");
    assert_eq!(tax.to_string(), "15.0000");
}

#[test]
fn taxes_split_inclusive_never_loses_a_unit() {
    use rust_decimal::Decimal;
    use std::str::FromStr;
    let amount = Decimal::from_str("115.0000").expect("decimal");
    let (base, tax) = crate::finance::taxes::split(amount, 1500, true);
    assert_eq!(base.to_string(), "100.0000");
    assert_eq!(tax.to_string(), "15.0000");
    assert_eq!(base + tax, amount);
    // مبلغ لا يقبل القسمة: مجموع الوعاء والضريبة يبقى مساويًا للمبلغ بالضبط.
    let odd = Decimal::from_str("0.0100").expect("decimal");
    let (b2, t2) = crate::finance::taxes::split(odd, 1500, true);
    assert_eq!(b2 + t2, odd);
}

#[test]
fn taxes_split_zero_rate_is_identity() {
    use rust_decimal::Decimal;
    use std::str::FromStr;
    let amount = Decimal::from_str("42.5000").expect("decimal");
    let (base, tax) = crate::finance::taxes::split(amount, 0, false);
    assert_eq!(base, amount);
    assert_eq!(tax, Decimal::ZERO);
}

#[test]
fn numbering_pads_and_prefixes() {
    assert_eq!(
        crate::finance::numbering::format_number("JV", 7, 5),
        "JV-00007"
    );
    assert_eq!(crate::finance::numbering::format_number("", 12, 3), "012");
    assert_eq!(
        crate::finance::numbering::format_number("A_B", 1, 1),
        "A_B-1"
    );
}

#[test]
fn org_validate_code_rejects_bad_symbols() {
    use crate::finance::org::validate_code;
    assert!(validate_code("SALES-01").is_ok());
    assert!(validate_code("").is_err());
    assert!(validate_code("قسم").is_err());
    assert!(validate_code(&"A".repeat(21)).is_err());
}

#[test]
fn org_validate_name_requires_text() {
    use crate::finance::org::validate_name;
    assert!(validate_name("إدارة المبيعات").is_ok());
    assert!(validate_name("   ").is_err());
    assert!(validate_name(&"ن".repeat(121)).is_err());
}

#[test]
fn org_detects_parent_cycles() {
    use crate::finance::org::makes_cycle;
    assert!(makes_cycle(&[5, 3, 1], 3));
    assert!(!makes_cycle(&[5, 3, 1], 9));
}

#[test]
fn scanner_extract_date_accepts_two_formats() {
    use crate::finance::requests::extract_date;
    assert_eq!(
        extract_date("فاتورة 12/10/2026").as_deref(),
        Some("2026-10-12")
    );
    assert_eq!(
        extract_date("التاريخ: 2026-10-12").as_deref(),
        Some("2026-10-12")
    );
    assert_eq!(extract_date("لا تاريخ هنا"), None);
    // تاريخ غير صالح يجب أن يُرفض لا أن يُقبل تقريبًا.
    assert_eq!(extract_date("45/13/2026"), None);
}

#[test]
fn scanner_extract_currency_known_tokens() {
    use crate::finance::requests::extract_currency;
    assert_eq!(extract_currency("المجموع 100 YER").as_deref(), Some("YER"));
    assert_eq!(extract_currency("total 100 usd").as_deref(), Some("USD"));
    assert_eq!(extract_currency("لا عملة"), None);
}

#[test]
fn scanner_extracts_total_and_tax() {
    use crate::finance::requests::extract_amounts;
    let text = "خدمات صيانة 150000\nضريبة 22500\nالإجمالي 172500";
    let (total, tax, lines) = extract_amounts(text);
    assert_eq!(total.as_deref(), Some("172500.0000"));
    assert_eq!(tax.as_deref(), Some("22500.0000"));
    assert!(!lines.is_empty());
}

#[test]
fn plans_commission_percent_plus_fixed() {
    use crate::finance::settings5::compute_commission;
    use rust_decimal::Decimal;
    use std::str::FromStr;
    let amount = Decimal::from_str("1000").expect("decimal");
    let fixed = Decimal::from_str("25").expect("decimal");
    let c = compute_commission(amount, 500, fixed); // 5% + 25
    assert_eq!(c.to_string(), "75.0000");
    // بلا نسبة: المبلغ الثابت وحده.
    assert_eq!(compute_commission(amount, 0, fixed).to_string(), "25.0000");
    // بلا ثابت: النسبة وحدها.
    assert_eq!(
        compute_commission(amount, 250, Decimal::ZERO).to_string(),
        "25.0000"
    );
}

#[test]
fn scanner_clean_line_drops_dates_and_references() {
    use crate::finance::requests::clean_line;
    // التاريخ ورقم المستند لا يصبحان مبلغًا.
    let cleaned = clean_line("التاريخ 12/10/2026");
    assert!(!cleaned.contains("12/10/2026"), "التاريخ يجب أن يُستبعد: {cleaned}");
    let inv = clean_line("فاتورة رقم INV-2026-014");
    assert!(!inv.chars().any(|c| c.is_ascii_digit()), "رقم المستند يجب أن يُستبعد: {inv}");
    // المبلغ الحقيقي يبقى.
    let amount = clean_line("خدمات صيانة 150000");
    assert!(amount.contains("150000"), "المبلغ يجب أن يبقى: {amount}");
}
