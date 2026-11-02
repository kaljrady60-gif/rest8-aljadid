//! اختبارات الموجة الثانية: الاعتماد، الأطراف، الشيكات، الإقفال السنوي، التصدير.
use super::*;

/// جلسة لمستخدم ثانٍ (مُعتمد) على نفس القاعدة.
fn second_admin(db: &rusqlite::Connection) -> (String, String) {
    db.execute(
        "INSERT OR IGNORE INTO identity_users(username,password_hash,role) VALUES('__approver__','x','admin')",
        [],
    )
    .expect("user");
    let id: i64 = db
        .query_row(
            "SELECT id FROM identity_users WHERE username='__approver__'",
            [],
            |r| r.get(0),
        )
        .expect("id");
    let token = crate::identity::random_hex().expect("token");
    let csrf = crate::identity::random_hex().expect("csrf");
    db.execute(
        "INSERT INTO identity_sessions(token_hash,user_id,csrf,expires_at) VALUES(?1,?2,?3,?4)",
        params![
            crate::identity::token_hash(&token),
            id,
            csrf,
            chrono::Utc::now().timestamp() + 3600
        ],
    )
    .expect("session");
    db.execute(
        "INSERT OR IGNORE INTO finance_company_admins(company_id,user_id,granted_by) VALUES(1,?1,?1)",
        params![id],
    )
    .expect("grant");
    (token, csrf)
}

fn journal_body() -> serde_json::Value {
    serde_json::json!({
        "company_id": 1, "description": "قيد اعتماد", "date": "2026-06-01", "currency": "YER",
        "lines": [
            {"account_id": 1, "account": "1110", "debit": "100", "credit": "0"},
            {"account_id": 2, "account": "3100", "debit": "0", "credit": "100"}
        ]
    })
}

#[tokio::test]
async fn approval_policy_blocks_posting_until_approved_by_another_user() {
    let (f, token, csrf) = setup_authed();
    let db = db_open(&f.path).expect("open");
    let (app2, csrf2) = second_admin(&db);
    drop(db);

    let (status, _) = call_auth(
        f.app.clone(),
        "POST",
        "/api/finance/approval/policy",
        Some(serde_json::json!({"company_id":1,"require_approval":true})),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, created) = call_auth(
        f.app.clone(),
        "POST",
        "/api/finance/journals",
        Some(journal_body()),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_i64().expect("id");

    // بلا اعتماد: الترحيل مرفوض بسياسة الشركة.
    let (status, body) = call_auth(
        f.app.clone(),
        "POST",
        &format!("/api/finance/journals/{id}/post"),
        Some(serde_json::json!({"company_id":1})),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    // المُنشئ يرسل، ثم يحاول اعتماد نفسه: مرفوض.
    let (status, _) = call_auth(
        f.app.clone(),
        "POST",
        &format!("/api/finance/journals/{id}/submit"),
        Some(serde_json::json!({"company_id":1})),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call_auth(
        f.app.clone(),
        "POST",
        &format!("/api/finance/journals/{id}/approve"),
        Some(serde_json::json!({"company_id":1})),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "فصل المُنشئ عن المُعتمد");

    // مُعتمد آخر يعتمد ثم يُرحّل.
    let (status, _) = call_auth(
        f.app.clone(),
        "POST",
        &format!("/api/finance/journals/{id}/approve"),
        Some(serde_json::json!({"company_id":1,"note":"مطابق للمستند"})),
        &app2,
        &csrf2,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = call_auth(
        f.app.clone(),
        "POST",
        &format!("/api/finance/journals/{id}/post"),
        Some(serde_json::json!({"company_id":1})),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, flow) = call_auth(
        f.app,
        "GET",
        "/api/finance/approval?company_id=1",
        None,
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(flow[0]["state"], "posted");
}

#[tokio::test]
async fn received_check_collection_posts_a_balanced_entry() {
    let (f, token, csrf) = setup_authed();
    let (status, created) = call_auth(
        f.app.clone(),
        "POST",
        "/api/finance/checks",
        Some(serde_json::json!({
            "company_id":1,"direction":"received","number":"CH-1","bank":"بنك تجاري",
            "drawer":"عميل أ","amount":"1500.00","currency":"YER","due_date":"2026-06-10"
        })),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_i64().expect("check id");

    // شيك مكرر بنفس البنك والاتجاه يُرفض.
    let (status, _) = call_auth(
        f.app.clone(),
        "POST",
        "/api/finance/checks",
        Some(serde_json::json!({
            "company_id":1,"direction":"received","number":"CH-1","bank":"بنك تجاري",
            "drawer":"عميل أ","amount":"1500.00","currency":"YER","due_date":"2026-06-10"
        })),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, moved) = call_auth(
        f.app.clone(),
        "POST",
        &format!("/api/finance/checks/{id}/status"),
        Some(serde_json::json!({"company_id":1,"status":"collected"})),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    let entry = moved["entry_id"].as_i64().expect("entry");
    assert!(entry > 0);

    let (status, trial) = call_auth(
        f.app.clone(),
        "GET",
        "/api/finance/statements?company_id=1&report=trial",
        None,
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(trial["totals"]["balanced"], true);

    // التحصيل مرة ثانية يُرفض.
    let (status, _) = call_auth(
        f.app,
        "POST",
        &format!("/api/finance/checks/{id}/status"),
        Some(serde_json::json!({"company_id":1,"status":"collected"})),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn year_end_closing_zeroes_result_accounts_and_is_irreversible() {
    let (f, token, csrf) = setup_authed();
    for (description, debit_account, credit_account, amount) in
        [("إيراد", 1, 3, "1000"), ("مصروف", 4, 1, "400")]
    {
        let (status, _) = call_auth(
            f.app.clone(),
            "POST",
            "/api/finance/journals",
            Some(serde_json::json!({
                "company_id":1,"description":description,"date":"2026-04-01","currency":"YER",
                "lines":[
                    {"account_id":debit_account,"account":"x","debit":amount,"credit":"0"},
                    {"account_id":credit_account,"account":"y","debit":"0","credit":amount}
                ]
            })),
            &token,
            &csrf,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let id: i64 = {
            let (_, list) = call_auth(
                f.app.clone(),
                "GET",
                "/api/finance/journals?company_id=1&status=draft",
                None,
                &token,
                &csrf,
            )
            .await;
            list.as_array().expect("list")[0]["id"]
                .as_i64()
                .expect("draft id")
        };
        let (status, _) = call_auth(
            f.app.clone(),
            "POST",
            &format!("/api/finance/journals/{id}/post"),
            Some(serde_json::json!({"company_id":1})),
            &token,
            &csrf,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    let (status, closed) = call_auth(
        f.app.clone(),
        "POST",
        "/api/finance/closing",
        Some(serde_json::json!({"company_id":1,"fiscal_year":2026,"retained_account_id":2})),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{closed}");
    assert_eq!(closed["net_income"], "600.0000");

    // بعد الإقفال: صافي الربح السنوي صفر لأن حسابات النتيجة أُغلقت.
    let (_, income) = call_auth(
        f.app.clone(),
        "GET",
        "/api/finance/statements?company_id=1&report=income",
        None,
        &token,
        &csrf,
    )
    .await;
    assert_eq!(income["net_profit"], "0.0000");

    // الإقفال مكرر: مرفوض، والسجل غير قابل للتعديل.
    let (status, _) = call_auth(
        f.app.clone(),
        "POST",
        "/api/finance/closing",
        Some(serde_json::json!({"company_id":1,"fiscal_year":2026})),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, list) = call_auth(
        f.app,
        "GET",
        "/api/finance/closing?company_id=1",
        None,
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list[0]["fiscal_year"], 2026);
}

#[tokio::test]
async fn csv_export_contains_bom_and_report_rows() {
    let (f, token, csrf) = setup_authed();
    let request = Request::builder()
        .uri("/api/finance/export?company_id=1&kind=income")
        .header("cookie", format!("smart_session={token}"))
        .body(Body::empty())
        .expect("request");
    let response = f.app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let text = String::from_utf8(bytes.to_vec()).expect("utf8");
    assert!(content_type.starts_with("text/csv"));
    assert!(text.starts_with('\u{feff}'), "BOM مطلوب لـ Excel");
    assert!(text.contains("التصنيف"));
    let _ = csrf;
}

#[tokio::test]
async fn parties_require_a_detail_account_and_unique_link() {
    let (f, token, csrf) = setup_authed();
    let (status, created) = call_auth(
        f.app.clone(),
        "POST",
        "/api/finance/parties",
        Some(serde_json::json!({
            "company_id":1,"name":"مورد الخليج","role":"supplier","account_id":4
        })),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let (status, list) = call_auth(
        f.app.clone(),
        "GET",
        "/api/finance/parties?company_id=1&role=supplier",
        None,
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().expect("list").len(), 1);
    // نفس الحساب لطرف آخر: مرفوض.
    let (status, _) = call_auth(
        f.app,
        "POST",
        "/api/finance/parties",
        Some(serde_json::json!({
            "company_id":1,"name":"مورد ثانٍ","role":"supplier","account_id":4
        })),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}
