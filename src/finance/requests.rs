//! طلبات القيود والسندات: مسار طلب → تقديم → قرار، وجسم الطلب JSON مُتحقَّق.
//! وقراءة الفواتير المصورة: محلّل نصّي حتمي يستخرج التاريخ والمبالغ والعملة
//! والبنود من نص الفاتورة — بلا خدمة خارجية وبلا تخمين: ما لا يُستخرج يُعلن فشله.
use super::audit;
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open, fail};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{TransactionBehavior, params};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

pub const REQUEST_KINDS: &[&str] = &["journal", "voucher"];

#[derive(Serialize)]
pub struct RequestRow {
    id: i64,
    kind: String,
    title: String,
    payload: serde_json::Value,
    status: String,
    decided_at: Option<String>,
    created_at: String,
}

#[derive(Deserialize)]
pub struct CompanyQuery {
    company_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewRequest {
    company_id: i64,
    kind: String,
    title: String,
    payload: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    company_id: i64,
    id: i64,
    #[serde(default)]
    approve: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanInput {
    company_id: i64,
    text: String,
}

/// نتيجة تحليل فاتورة: كل حقل يُعلن صراحةً هل استُخرج أم لا.
#[derive(Serialize, Default)]
pub struct ScanResult {
    date: Option<String>,
    currency: Option<String>,
    total: Option<String>,
    tax: Option<String>,
    lines: Vec<ScanLine>,
    confidence: u8,
    notes: Vec<String>,
}

#[derive(Serialize)]
pub struct ScanLine {
    description: String,
    amount: String,
}

fn digits_only(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect()
}

/// يحوّل رقمًا مكتوبًا بفواصل أو مسافات إلى Decimal صالح.
fn parse_amount(raw: &str) -> Option<Decimal> {
    let cleaned = digits_only(&raw.replace(',', ""));
    if cleaned.is_empty() {
        return None;
    }
    Decimal::from_str(&cleaned).ok()
}

/// يستخرج أول تاريخ بصيغة YYYY-MM-DD أو DD/MM/YYYY أو DD-MM-YYYY.
pub fn extract_date(text: &str) -> Option<String> {
    let bytes: Vec<char> = text.chars().collect();
    for i in 0..bytes.len() {
        let window: String = bytes.iter().skip(i).take(10).collect();
        if window.len() < 8 {
            break;
        }
        let parts: Vec<&str> = window.split(['/', '-']).collect();
        if parts.len() == 3 {
            let (a, b, c) = (parts[0], parts[1], parts[2].get(..4).unwrap_or(parts[2]));
            let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
            if ok(a) && ok(b) && ok(c) {
                if a.len() == 4 && c.len() == 2 {
                    let iso = format!("{a}-{b:0>2}-{c:0>2}");
                    if chrono::NaiveDate::parse_from_str(&iso, "%Y-%m-%d").is_ok() {
                        return Some(iso);
                    }
                } else if a.len() == 2 && c.len() == 4 {
                    let iso = format!("{c}-{b:0>2}-{a:0>2}");
                    if chrono::NaiveDate::parse_from_str(&iso, "%Y-%m-%d").is_ok() {
                        return Some(iso);
                    }
                }
            }
        }
    }
    None
}

pub fn extract_currency(text: &str) -> Option<String> {
    for token in ["SAR", "USD", "EUR", "YER", "AED", "EGP", "GBP", "KWD"] {
        if text.to_uppercase().contains(token) {
            return Some(token.to_string());
        }
    }
    None
}


/// يزيل من السطر ما ليس مبلغًا: التواريخ وأرقام المستندات المركّبة.
/// بلا هذا التنظيف تُقرأ `12/10/2026` و`INV-2026-014` كمبالغ — وهو خطأ مالي خطير.
pub fn clean_line(line: &str) -> String {
    let mut kept = String::with_capacity(line.len());
    let mut token = String::new();
    let flush = |token: &mut String, kept: &mut String| {
        let has_digit = token.chars().any(|c| c.is_ascii_digit());
        let has_alpha = token.chars().any(|c| c.is_alphabetic());
        let date_like = token.matches(['/', '-']).count() >= 2;
        if !(has_digit && (has_alpha || date_like)) {
            kept.push_str(token);
            kept.push(' ');
        }
        token.clear();
    };
    for ch in line.chars() {
        if ch.is_whitespace() {
            if !token.is_empty() {
                flush(&mut token, &mut kept);
            }
        } else {
            token.push(ch);
        }
    }
    if !token.is_empty() {
        flush(&mut token, &mut kept);
    }
    kept
}

/// يبحث عن المبلغ المرتبط بكلمة مفتاحية (الإجمالي/الضريبة) وإلا يأخذ أكبر مبلغ.
pub fn extract_amounts(text: &str) -> (Option<String>, Option<String>, Vec<ScanLine>) {
    let mut total = None;
    let mut tax = None;
    let mut lines = Vec::new();
    for raw_line in text.lines() {
        let raw_trimmed = raw_line.trim();
        if raw_trimmed.is_empty() {
            continue;
        }
        let lower = raw_trimmed.to_lowercase();
        let cleaned = clean_line(raw_trimmed);
        let line = cleaned.trim();
        if line.is_empty() {
            continue;
        }
        let numbers: Vec<Decimal> = line
            .split_whitespace()
            .filter_map(parse_amount)
            .filter(|v| *v > Decimal::ZERO)
            .collect();
        if numbers.is_empty() {
            continue;
        }
        let value = numbers.last().copied().unwrap_or_default();
        if lower.contains("ضريبة") || lower.contains("vat") || lower.contains("tax") {
            tax = Some(super::money::render(value));
        } else if lower.contains("إجمالي")
            || lower.contains("اجمالي")
            || lower.contains("total")
            || lower.contains("المجموع")
        {
            total = Some(super::money::render(value));
        } else if !numbers.is_empty() && line.chars().any(|c| c.is_alphabetic()) {
            let label: String = raw_trimmed
                .split_whitespace()
                .filter(|w| !w.chars().any(|c| c.is_ascii_digit()))
                .collect::<Vec<_>>()
                .join(" ");
            lines.push(ScanLine {
                description: label.chars().take(80).collect(),
                amount: super::money::render(value),
            });
        }
    }
    if total.is_none() {
        let mut best = Decimal::ZERO;
        for line in text.lines() {
            for token in line.split_whitespace() {
                if let Some(v) = parse_amount(token).filter(|v| *v > best) {
                    best = v;
                }
            }
        }
        if best > Decimal::ZERO {
            total = Some(super::money::render(best));
        }
    }
    (total, tax, lines.into_iter().take(12).collect())
}

pub async fn list(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<CompanyQuery>,
) -> Response {
    let result = (|| -> Result<Vec<RequestRow>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT id,kind,title,payload,status,decided_at,created_at
             FROM finance_requests WHERE company_id=?1 ORDER BY id DESC LIMIT 200",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            let payload: String = r.get(3)?;
            Ok(RequestRow {
                id: r.get(0)?,
                kind: r.get(1)?,
                title: r.get(2)?,
                payload: serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null),
                status: r.get(4)?,
                decided_at: r.get(5)?,
                created_at: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })();
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn create(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<NewRequest>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        if !REQUEST_KINDS.contains(&n.kind.as_str()) {
            return Err(FinanceError::Invalid("نوع الطلب غير مدعوم.".into()));
        }
        let title = n.title.trim();
        if title.is_empty() || title.chars().count() > 160 {
            return Err(FinanceError::Invalid(
                "عنوان الطلب مطلوب حتى 160 حرفًا.".into(),
            ));
        }
        if !n.payload.is_object() {
            return Err(FinanceError::Invalid(
                "جسم الطلب يجب أن يكون كائن JSON.".into(),
            ));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        tx.execute(
            "INSERT INTO finance_requests(company_id,kind,title,payload,created_by)
             VALUES(?1,?2,?3,?4,?5)",
            params![
                scope.company_id,
                n.kind,
                title,
                n.payload.to_string(),
                actor.id
            ],
        )?;
        let id = tx.last_insert_rowid();
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "request.create",
                entity: "finance_requests",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({"kind": n.kind})),
            },
        )?;
        tx.commit()?;
        Ok(id)
    })();
    match result {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn submit(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(req): Json<Transition>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, req.company_id)?;
        let changed = tx.execute(
            "UPDATE finance_requests SET status='submitted'
             WHERE id=?1 AND company_id=?2 AND status='draft'",
            params![req.id, scope.company_id],
        )?;
        if changed == 0 {
            return Err(FinanceError::Conflict("الطلب ليس مسودة أو غير موجود."));
        }
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "request.submit",
                entity: "finance_requests",
                entity_id: Some(req.id.to_string()),
                payload: None,
            },
        )?;
        let status: String = tx.query_row(
            "SELECT status FROM finance_requests WHERE id=?1 AND company_id=?2",
            params![req.id, scope.company_id],
            |r| r.get(0),
        )?;
        tx.commit()?;
        Ok(serde_json::json!({
            "id": req.id,
            "entity": "request",
            "status": status,
            "decided_at": serde_json::Value::Null
        }))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn decide(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(req): Json<Transition>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, req.company_id)?;
        scope.require_admin()?;
        let changed = tx.execute(
            "UPDATE finance_requests
             SET status=?3, decided_by=?4, decided_at=CURRENT_TIMESTAMP
             WHERE id=?1 AND company_id=?2 AND status='submitted'",
            params![
                req.id,
                scope.company_id,
                if req.approve { "approved" } else { "rejected" },
                actor.id
            ],
        )?;
        if changed == 0 {
            return Err(FinanceError::Conflict("الطلب ليس مقدَّمًا أو غير موجود."));
        }
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: if req.approve {
                    "request.approve"
                } else {
                    "request.reject"
                },
                entity: "finance_requests",
                entity_id: Some(req.id.to_string()),
                payload: None,
            },
        )?;
        let decided: (String, Option<String>) = tx.query_row(
            "SELECT status, decided_at FROM finance_requests WHERE id=?1 AND company_id=?2",
            params![req.id, scope.company_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let new_status = decided.0.clone();
        tx.commit()?;
        Ok(serde_json::json!({
            "id": req.id,
            "entity": "request",
            "status": new_status,
            "decided_at": decided.1
        }))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

/// تحليل نص فاتورة إلى حقول معلنة — دالة نقية تُستخدم للنص ولنتيجة الذكاء الاصطناعي.
pub(super) fn analyse_text(text: &str) -> ScanResult {
    let mut result = ScanResult {
        date: extract_date(text),
        currency: extract_currency(text),
        ..ScanResult::default()
    };
    let (total, tax, lines) = extract_amounts(text);
    result.total = total;
    result.tax = tax;
    result.lines = lines;

    let mut score = 0_u8;
    if result.date.is_some() {
        score += 35;
    }
    if result.total.is_some() {
        score += 40;
    }
    if result.currency.is_some() {
        score += 10;
    }
    if !result.lines.is_empty() {
        score += 15;
    }
    result.confidence = score;
    if result.date.is_none() {
        result.notes.push("لم يُعثر على تاريخ صالح في النص.".into());
    }
    if result.total.is_none() {
        result.notes.push("لم يُعثر على إجمالٍ أو مبلغ واضح.".into());
    }
    if score < 50 {
        result
            .notes
            .push("الثقة منخفضة: راجع القيم يدويًا قبل إنشاء أي قيد.".into());
    }
    result
}

/// واجهات مبسّطة لمزوّد الرؤية: تُرجع الحقل مباشرة أو `None` بلا تخمين.
pub fn find_date_str(text: &str) -> Option<String> {
    extract_date(text)
}

pub fn find_currency_str(text: &str) -> Option<String> {
    extract_currency(text)
}

pub fn find_total_str(text: &str) -> Option<String> {
    extract_amounts(text).0
}

/// يحلّل نص فاتورة بلا كتابة أي بيانات — النتيجة معاينة فقط.
pub async fn scan(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(input): Json<ScanInput>,
) -> Response {
    let outcome = (|| -> Result<ScanResult, FinanceError> {
        let text = input.text.trim();
        if text.is_empty() {
            return Err(FinanceError::Invalid("ألصق نص الفاتورة أولًا.".into()));
        }
        if text.chars().count() > 20_000 {
            return Err(FinanceError::Invalid(
                "النص أطول من الحد المسموح (20000 حرف).".into(),
            ));
        }
        let db = db_open(&state.db)?;
        let _scope = Scope::resolve(&state, &db, &actor, input.company_id)?;
        Ok(analyse_text(text))
    })();
    match outcome {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageInput {
    company_id: i64,
    /// الصورة بترميز Base64 (بدون بادئة data:).
    image: String,
    #[serde(default)]
    mime: Option<String>,
}

/// قراءة فاتورة مصوّرة: يحوّلها مزوّد الرؤية الذي اختاره المستخدم إلى نص،
/// ثم يمشي عليها نفس المحلّل الحتمي — فلا تُخترع أرقام من غير مصدر.
pub async fn scan_image(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(input): Json<ImageInput>,
) -> Response {
    let prepared = (|| -> Result<(super::invoice_ai::Config, String, String), FinanceError> {
        let raw = input
            .image
            .trim()
            .rsplit(',')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if raw.len() < 64 {
            return Err(FinanceError::Invalid(
                "الصورة غير مقروءة أو فارغة؛ أعد اختيارها.".into(),
            ));
        }
        if raw.len() > 8 * 1024 * 1024 {
            return Err(FinanceError::Invalid(
                "حجم الصورة أكبر من 6 ميجابايت تقريبًا؛ اضغطها ثم أعد المحاولة.".into(),
            ));
        }
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, input.company_id)?;
        let (config, _) = super::ai_settings::load(&db, scope.company_id);
        let mime = input
            .mime
            .clone()
            .unwrap_or_else(|| "image/jpeg".to_string());
        Ok((config, raw, mime))
    })();
    let (config, image, mime) = match prepared {
        Ok(v) => v,
        Err(e) => return error_response(e),
    };

    let outcome = super::invoice_ai::read_invoice(config, image, mime).await;
    match outcome {
        Ok(reading) => {
            let mut result = analyse_text(&reading.text);
            if result.date.is_none() {
                if let Some(day) = reading.date {
                    result.date = Some(day);
                    result.confidence = result.confidence.saturating_add(20);
                }
            }
            if result.total.is_none() {
                if let Some(total) = reading.total {
                    result.total = Some(total);
                    result.confidence = result.confidence.saturating_add(25);
                }
            }
            if result.currency.is_none() {
                result.currency = reading.currency;
            }
            result.notes.push(format!(
                "قُرئت الصورة بواسطة {} ({}).",
                if reading.provider == "local" {
                    "النموذج المحلي"
                } else {
                    "النموذج السحابي"
                },
                reading.model
            ));
            let visible = reading.text.trim().replace('\n', " · ");
            if !visible.is_empty() {
                let short: String = visible.chars().take(220).collect();
                result.notes.push(format!("ما قرأه النموذج: {short}"));
            }
            for (scope, ok, reason) in &reading.attempts {
                if !ok {
                    result.notes.push(format!(
                        "تعذّر {} قبل ذلك: {}",
                        if scope == "local" { "المحلي" } else { "السحابي" },
                        reason.clone().unwrap_or_default()
                    ));
                }
            }
            Json(result).into_response()
        }
    Err(message) => fail(StatusCode::SERVICE_UNAVAILABLE, &message),
    }
}
