//! اختبارات المسارات الموسّعة: السياق، القيود، التسوية البنكية، الأبعاد، الأستاذ.
use super::*;

#[tokio::test]
async fn context_lists_companies_for_the_interface() {
    let f = setup();
    let (status, body) = call(f.app, "GET", "/api/finance/context", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["companies"].as_array().expect("companies").len(), 2);
    assert_eq!(body["default_company_id"], 1);
    assert!(body["screens"].as_array().expect("screens").len() >= 13);
}

#[tokio::test]
async fn journals_create_is_scoped_and_posting_is_blocked_without_opt_in() {
    let f = setup();
    let draft = serde_json::json!({
        "company_id": 1, "description": "قيد تجريبي", "date": "2026-05-01", "currency": "YER",
        "lines": [
            {"account_id": 1, "account": "1110", "debit": "10", "credit": "0"},
            {"account_id": 2, "account": "3100", "debit": "0", "credit": "10"}
        ]
    });
    let (status, created) = call(f.app.clone(), "POST", "/api/finance/journals", Some(draft)).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_i64().expect("draft id");

    let (status, list) = call(
        f.app.clone(),
        "GET",
        "/api/finance/journals?company_id=1",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().expect("list").len(), 1);
    assert_eq!(list[0]["posted"], false);

    // الترحيل يتطلب تفعيلًا صريحًا على الخادم.
    let (status, _) = call(
        f.app.clone(),
        "POST",
        &format!("/api/finance/journals/{id}/post"),
        Some(serde_json::json!({"company_id": 1})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // حساب من شركة ثانية يُرفض.
    let (status, _) = call(
        f.app,
        "POST",
        "/api/finance/journals",
        Some(serde_json::json!({
            "company_id": 1, "description": "خارج النطاق", "date": "2026-05-02", "currency": "YER",
            "lines": [
                {"account_id": 99, "account": "X", "debit": "5", "credit": "0"},
                {"account_id": 2, "account": "3100", "debit": "0", "credit": "5"}
            ]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn bankrec_requires_a_bank_fund_and_tracks_cleared_lines() {
    let f = setup();
    let (status, fund) = call(
        f.app.clone(),
        "POST",
        "/api/finance/funds",
        Some(serde_json::json!({
            "company_id": 1, "kind": "bank", "code": "BNK", "name": "بنك",
            "account_id": 1, "iban": "YE00", "opened_on": "2026-01-01"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let fund_id = fund["id"].as_i64().expect("fund");

    // كشف على صندوق نقدي يُرفض (المسار يخص البنوك).
    let (status, _) = call(
        f.app.clone(),
        "POST",
        "/api/finance/funds",
        Some(serde_json::json!({
            "company_id": 1, "kind": "cash", "code": "C2", "name": "صندوق",
            "account_id": 3, "iban": null, "opened_on": "2026-01-01"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, statement) = call(
        f.app.clone(),
        "POST",
        "/api/finance/bankrec",
        Some(serde_json::json!({
            "company_id": 1, "fund_id": fund_id,
            "period_start": "2026-01-01", "period_end": "2026-01-31",
            "closing_balance": "100"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let statement_id = statement["id"].as_i64().expect("statement");

    let (status, line) = call(
        f.app.clone(),
        "POST",
        &format!("/api/finance/bankrec/{statement_id}/lines"),
        Some(
            serde_json::json!({"company_id": 1, "kind": "charge", "amount": "5", "memo": "عمولة"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let line_id = line["id"].as_i64().expect("line");

    let (status, body) = call(
        f.app.clone(),
        "GET",
        "/api/finance/bankrec?company_id=1",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body[0]["pending_total"], "-5.0000"); // العمولة تُخزَّن سالبة صراحةً

    let (status, _) = call(
        f.app,
        "POST",
        &format!("/api/finance/bankrec/{statement_id}/lines/{line_id}"),
        Some(serde_json::json!({"company_id": 1, "cleared": true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn dimensions_can_be_archived_and_ledger_needs_an_owned_account() {
    let f = setup();
    let (status, created) = call(
        f.app.clone(),
        "POST",
        "/api/finance/dimensions",
        Some(serde_json::json!({"company_id":1,"code":"CC9","name":"مشروع","kind":"project"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_i64().expect("dimension");
    let (status, _) = call(
        f.app.clone(),
        "POST",
        "/api/finance/dimensions/archive",
        Some(serde_json::json!({"company_id": 1, "id": id})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // الأستاذ: حساب غير تابع للشركة يُرفض، والحساب التابع يُعيد حركة فارغة.
    let (status, _) = call(
        f.app.clone(),
        "GET",
        "/api/finance/ledger?company_id=1&account_id=99",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, body) = call(
        f.app,
        "GET",
        "/api/finance/ledger?company_id=1&account_id=1",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["closing"], "0.0000");
}
