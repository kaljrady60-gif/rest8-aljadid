//! اختبارات الموجة الرابعة: الفروع وتقييد التقارير بها، وأوراق الطباعة.
use super::*;

async fn new_branch(f: &Fixture, token: &str, csrf: &str, code: &str) -> i64 {
    let (status, body) = call_auth(
        f.app.clone(),
        "POST",
        "/api/finance/branches",
        Some(serde_json::json!({"company_id":1,"code":code,"name":format!("فرع {code}")})),
        token,
        csrf,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_i64().expect("id")
}

async fn post_journal(f: &Fixture, token: &str, csrf: &str, branch: Option<i64>, amount: &str) {
    let (status, created) = call_auth(
        f.app.clone(),
        "POST",
        "/api/finance/journals",
        Some(serde_json::json!({
            "company_id": 1, "description": "قيد فرع", "date": "2026-06-01",
            "currency": "YER",
            "lines": [
                {"account_id": 1, "account": "1110", "debit": amount, "credit": "0"},
                {"account_id": 2, "account": "3100", "debit": "0", "credit": amount}
            ]
        })),
        token,
        csrf,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let draft = created["id"].as_i64().expect("draft");
    let mut body = serde_json::json!({"company_id":1});
    if let Some(id) = branch {
        body["branch_id"] = serde_json::json!(id);
    }
    let (status, posted) = call_auth(
        f.app.clone(),
        "POST",
        &format!("/api/finance/journals/{draft}/post"),
        Some(body),
        token,
        csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{posted}");
}

#[tokio::test]
async fn branch_filter_limits_reports_and_branch_list_counts_entries() {
    let (f, token, csrf) = setup_authed();
    let second = new_branch(&f, &token, &csrf, "B2").await;
    post_journal(&f, &token, &csrf, None, "1000").await; // الفرع الافتراضي
    post_journal(&f, &token, &csrf, Some(second), "2500").await;

    let (status, all) = call_auth(
        f.app.clone(),
        "GET",
        "/api/finance/statements?company_id=1&report=trial",
        None,
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{all}");
    assert_eq!(all["totals"]["debit"], "3500.0000");

    let (status, only) = call_auth(
        f.app.clone(),
        "GET",
        &format!("/api/finance/statements?company_id=1&report=trial&branch_id={second}"),
        None,
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{only}");
    assert_eq!(only["totals"]["debit"], "2500.0000");

    let (status, branches) = call_auth(
        f.app.clone(),
        "GET",
        "/api/finance/branches?company_id=1",
        None,
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let list = branches.as_array().expect("branches").clone();
    assert_eq!(list.len(), 2);
    let second_row = list.iter().find(|b| b["id"] == second).expect("second");
    assert_eq!(second_row["entries"], 1);
    assert_eq!(second_row["debit"], "2500.0000");
}

#[tokio::test]
async fn branch_must_belong_to_the_company() {
    let (f, token, csrf) = setup_authed();
    let (status, body) = call_auth(
        f.app.clone(),
        "POST",
        "/api/finance/journals",
        Some(serde_json::json!({
            "company_id": 1, "description": "قيد", "date": "2026-06-01", "currency": "YER",
            "lines": [
                {"account_id": 1, "account": "1110", "debit": "100", "credit": "0"},
                {"account_id": 2, "account": "3100", "debit": "0", "credit": "100"}
            ]
        })),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let draft = body["id"].as_i64().expect("draft");
    // الفرع 2 يخص الشركة 2 في عُهدة الاختبار.
    let (status, blocked) = call_auth(
        f.app.clone(),
        "POST",
        &format!("/api/finance/journals/{draft}/post"),
        Some(serde_json::json!({"company_id":1,"branch_id":2})),
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{blocked}");
    assert!(blocked["error"].as_str().unwrap_or("").contains("الفرع"));
}

#[tokio::test]
async fn print_sheet_renders_rtl_a4_page_with_real_totals() {
    let (f, token, csrf) = setup_authed();
    post_journal(&f, &token, &csrf, None, "1750").await;
    let request = Request::builder()
        .method("GET")
        .uri("/api/finance/print?company_id=1&report=trial")
        .header("cookie", format!("smart_session={token}"))
        .body(Body::empty())
        .expect("request");
    let response = f.app.clone().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let html = String::from_utf8(bytes.to_vec()).expect("utf8");
    assert!(html.contains("dir=\"rtl\""), "صفحة عربية RTL");
    assert!(html.contains("size: A4"), "مقاس الطباعة A4");
    assert!(html.contains("1750"), "أرقام التقرير الحقيقية");
    assert!(html.contains("window.print()"), "زر طباعة/حفظ PDF");
    assert!(html.contains("ميزان المراجعة"), "عنوان التقرير");
}

#[tokio::test]
async fn print_sheet_rejects_unknown_report_and_foreign_company() {
    let (f, token, csrf) = setup_authed();
    let (status, _) = call_auth(
        f.app.clone(),
        "GET",
        "/api/finance/print?company_id=1&report=unknown",
        None,
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = call_auth(
        f.app.clone(),
        "GET",
        "/api/finance/print?company_id=2&report=trial",
        None,
        &token,
        &csrf,
    )
    .await;
    assert!(
        status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND,
        "شركة غير مسندة مرفوضة: {status}"
    );
}

#[tokio::test]
async fn journal_detail_returns_lines_and_respects_company_scope() {
    let (f, token, csrf) = setup_authed();
    post_journal(&f, &token, &csrf, None, "900").await;
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
    let id = list.as_array().expect("list")[0]["id"]
        .as_i64()
        .expect("id");

    let (status, detail) = call_auth(
        f.app.clone(),
        "GET",
        &format!("/api/finance/journals/{id}?company_id=1"),
        None,
        &token,
        &csrf,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["lines"].as_array().expect("lines").len(), 2);
    assert_eq!(detail["posted"], true);
    assert!(detail["entry_id"].as_i64().is_some());

    let (status, _) = call_auth(
        f.app.clone(),
        "GET",
        &format!("/api/finance/journals/{id}?company_id=2"),
        None,
        &token,
        &csrf,
    )
    .await;
    assert!(
        status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND,
        "شركة أخرى لا ترى تفاصيل القيد: {status}"
    );
}
