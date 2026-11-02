//! إضافات المالية (١): العملات · مجموعات الفروع · الإقرارات الضريبية · الإقفال الموحّد.
//!
//! قاعدة عامة في هذا الملف: كل قراءة تمرّ بعزل الشركة عبر `Scope::resolve`،
//! وكل كتابة تُسجَّل في سجل التدقيق، والأرقام المالية تُجمع على `Decimal` لا على `f64`.
use super::audit;
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{TransactionBehavior, params};
use rust_decimal::Decimal;
use rust_decimal::RoundingStrategy;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

pub(super) fn money(value: Decimal) -> String {
    let mut out = value.round_dp_with_strategy(4, RoundingStrategy::MidpointAwayFromZero);
    out.rescale(4);
    super::money::render(out)
}

/// تحويل مبلغ نصي إلى عشري موجب — مشترك بين شاشات المالية.
pub(super) fn amount_of(raw: &str, label: &str) -> Result<Decimal, FinanceError> {
    let value = Decimal::from_str(raw.trim())
        .map_err(|_| FinanceError::Invalid(format!("{label} ليس رقمًا صالحًا.")))?;
    if value.is_sign_negative() {
        return Err(FinanceError::Invalid(format!("{label} يجب أن يكون موجبًا.")));
    }
    Ok(value)
}

// ─────────────────────────────── العملات ───────────────────────────────
#[derive(Serialize)]
pub struct Currency {
    id: i64,
    code: String,
    name: String,
    symbol: Option<String>,
    decimals: i64,
    archived_at: Option<String>,
}

#[derive(Deserialize)]
pub struct CurrencyQuery {
    company_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewCurrency {
    company_id: i64,
    code: String,
    name: String,
    #[serde(default)]
    symbol: Option<String>,
    #[serde(default = "two")]
    decimals: i64,
}

fn two() -> i64 {
    2
}

pub async fn currencies(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<CurrencyQuery>,
) -> Response {
    let result = (|| -> Result<Vec<Currency>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT id,code,name,symbol,decimals,archived_at FROM finance_currencies
             WHERE company_id=?1 ORDER BY archived_at IS NOT NULL, code",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            Ok(Currency {
                id: r.get(0)?,
                code: r.get(1)?,
                name: r.get(2)?,
                symbol: r.get(3)?,
                decimals: r.get(4)?,
                archived_at: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })();
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn create_currency(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<NewCurrency>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        let code = n.code.trim().to_uppercase();
        if code.len() != 3 || !code.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(FinanceError::Invalid(
                "رمز العملة ثلاثي وفق ISO، مثل YER.".into(),
            ));
        }
        if n.name.trim().is_empty() {
            return Err(FinanceError::Invalid("اسم العملة مطلوب.".into()));
        }
        if !(0..=6).contains(&n.decimals) {
            return Err(FinanceError::Invalid("منازل العملة بين 0 و6.".into()));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        scope.require_admin()?;
        tx.execute(
            "INSERT INTO finance_currencies(company_id,code,name,symbol,decimals)
             VALUES(?1,?2,?3,?4,?5)",
            params![scope.company_id, code, n.name.trim(), n.symbol, n.decimals],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(_, _) => FinanceError::Conflict("رمز العملة مستخدم."),
            other => FinanceError::Storage(other),
        })?;
        let id = tx.last_insert_rowid();
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "currency.create",
                entity: "finance_currencies",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({"code": code, "decimals": n.decimals})),
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

// ─────────────────────────── مجموعات الفروع ───────────────────────────
#[derive(Serialize)]
pub struct BranchGroup {
    id: i64,
    code: String,
    name: String,
    note: Option<String>,
    members: Vec<serde_json::Value>,
    archived_at: Option<String>,
}

#[derive(Deserialize)]
pub struct GroupQuery {
    company_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewGroup {
    company_id: i64,
    code: String,
    name: String,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    branch_ids: Vec<i64>,
}

pub async fn groups(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<GroupQuery>,
) -> Response {
    let result = (|| -> Result<Vec<BranchGroup>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT id,code,name,note,archived_at FROM finance_branch_groups
             WHERE company_id=?1 ORDER BY archived_at IS NOT NULL, code",
        )?;
        let base = stmt
            .query_map([scope.company_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut out = Vec::with_capacity(base.len());
        for (id, code, name, note, archived) in base {
            let mut ms = db.prepare(
                "SELECT b.id,b.code,b.name FROM finance_branch_group_members m
                 JOIN finance_branches b ON b.id=m.branch_id WHERE m.group_id=?1 ORDER BY b.code",
            )?;
            let members = ms
                .query_map([id], |r| {
                    Ok(serde_json::json!({
                        "id": r.get::<_, i64>(0)?,
                        "code": r.get::<_, String>(1)?,
                        "name": r.get::<_, String>(2)?
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            out.push(BranchGroup {
                id,
                code,
                name,
                note,
                members,
                archived_at: archived,
            });
        }
        Ok(out)
    })();
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn create_group(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<NewGroup>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        if n.code.trim().is_empty() || n.name.trim().is_empty() {
            return Err(FinanceError::Invalid("رمز المجموعة واسمها مطلوبان.".into()));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        scope.require_admin()?;
        tx.execute(
            "INSERT INTO finance_branch_groups(company_id,code,name,note) VALUES(?1,?2,?3,?4)",
            params![scope.company_id, n.code.trim(), n.name.trim(), n.note],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(err, _)
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                FinanceError::Conflict("رمز المجموعة مستخدم.")
            }
            other => FinanceError::Storage(other),
        })?;
        let id = tx.last_insert_rowid();
        for branch in &n.branch_ids {
            let owned: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM finance_branches WHERE id=?1 AND company_id=?2)",
                params![branch, scope.company_id],
                |r| r.get(0),
            )?;
            if !owned {
                return Err(FinanceError::Invalid(
                    "فرع غير تابع لهذه الشركة في المجموعة.".into(),
                ));
            }
            tx.execute(
                "INSERT OR IGNORE INTO finance_branch_group_members(group_id,branch_id) VALUES(?1,?2)",
                params![id, branch],
            )?;
        }
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "branch_group.create",
                entity: "finance_branch_groups",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({
                    "code": n.code.trim(), "branches": n.branch_ids.len()
                })),
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

// ───────────────────────── الإقرارات الضريبية ─────────────────────────
#[derive(Deserialize)]
pub struct ReturnQuery {
    company_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewReturn {
    company_id: i64,
    #[serde(default = "vat")]
    kind: String,
    period_from: String,
    period_to: String,
    #[serde(default)]
    note: Option<String>,
}

fn vat() -> String {
    "vat".into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnId {
    company_id: i64,
    id: i64,
}

/// يجمع ضريبة المخرجات والمدخلات من روابط الضرائب الفعلية داخل الفترة.
fn totals_for(
    db: &rusqlite::Connection,
    company_id: i64,
    from: &str,
    to: &str,
) -> Result<(Decimal, Decimal), FinanceError> {
    let mut stmt = db.prepare(
        "SELECT l.direction, l.tax_amount FROM finance_tax_links l
         WHERE l.company_id=?1 AND l.archived_at IS NULL
           AND l.doc_type='journal'
           AND l.created_at >= ?2 AND l.created_at < ?3",
    )?;
    let rows = stmt.query_map(params![company_id, from, to], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let (mut output, mut input) = (Decimal::ZERO, Decimal::ZERO);
    for row in rows {
        let (direction, amount) = row?;
        let value = Decimal::from_str(amount.trim()).unwrap_or(Decimal::ZERO);
        if direction == "output" {
            output += value;
        } else {
            input += value;
        }
    }
    Ok((output, input))
}

pub async fn tax_returns(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ReturnQuery>,
) -> Response {
    let result = (|| -> Result<Vec<serde_json::Value>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT id,kind,period_from,period_to,output_tax,input_tax,net_tax,status,note,submitted_at
             FROM finance_tax_returns WHERE company_id=?1 ORDER BY period_from DESC, id DESC",
        )?;
        let rows = stmt
            .query_map([scope.company_id], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "kind": r.get::<_, String>(1)?,
                    "period_from": r.get::<_, String>(2)?,
                    "period_to": r.get::<_, String>(3)?,
                    "output_tax": r.get::<_, String>(4)?,
                    "input_tax": r.get::<_, String>(5)?,
                    "net_tax": r.get::<_, String>(6)?,
                    "status": r.get::<_, String>(7)?,
                    "note": r.get::<_, Option<String>>(8)?,
                    "submitted_at": r.get::<_, Option<String>>(9)?
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })();
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => error_response(e),
    }
}

/// معاينة إقرار لفترة قبل حفظه: لا تكتب شيئًا.
pub async fn preview_return(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<PreviewQuery>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let (output, input) = totals_for(&db, scope.company_id, &q.from, &q.to)?;
        Ok(serde_json::json!({
            "period_from": q.from,
            "period_to": q.to,
            "output_tax": money(output),
            "input_tax": money(input),
            "net_tax": money(output - input),
            "direction": if output >= input { "payable" } else { "refundable" }
        }))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

#[derive(Deserialize)]
pub struct PreviewQuery {
    company_id: i64,
    from: String,
    to: String,
}

pub async fn create_return(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<NewReturn>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        if n.period_from.as_str() >= n.period_to.as_str() {
            return Err(FinanceError::Invalid(
                "بداية الفترة يجب أن تسبق نهايتها.".into(),
            ));
        }
        if !["vat", "sales", "purchase", "withholding"].contains(&n.kind.as_str()) {
            return Err(FinanceError::Invalid("نوع إقرار غير مدعوم.".into()));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        scope.require_admin()?;
        let (output, input) = totals_for(&tx, scope.company_id, &n.period_from, &n.period_to)?;
        tx.execute(
            "INSERT INTO finance_tax_returns(company_id,kind,period_from,period_to,output_tax,input_tax,net_tax,note)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                scope.company_id,
                n.kind,
                n.period_from,
                n.period_to,
                money(output),
                money(input),
                money(output - input),
                n.note
            ],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(err, _)
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                FinanceError::Conflict("يوجد إقرار لهذه الفترة والنوع.")
            }
            other => FinanceError::Storage(other),
        })?;
        let id = tx.last_insert_rowid();
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "tax_return.create",
                entity: "finance_tax_returns",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({
                    "from": n.period_from, "to": n.period_to,
                    "output": money(output), "input": money(input)
                })),
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

/// تقديم الإقرار: انتقال حالة نهائي مع أثر تدقيقي، ولا يُقدَّم مرتين.
pub async fn submit_return(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(req): Json<ReturnId>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, req.company_id)?;
        scope.require_admin()?;
        let changed = tx.execute(
            "UPDATE finance_tax_returns SET status='submitted', submitted_at=CURRENT_TIMESTAMP
             WHERE id=?1 AND company_id=?2 AND status='draft'",
            params![req.id, scope.company_id],
        )?;
        if changed == 0 {
            return Err(FinanceError::Conflict("الإقرار مقدَّم مسبقًا أو غير موجود."));
        }
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "tax_return.submit",
                entity: "finance_tax_returns",
                entity_id: Some(req.id.to_string()),
                payload: None,
            },
        )?;
        tx.commit()?;
        Ok(serde_json::json!({"id": req.id, "status": "submitted"}))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

// ─────────────────────────── الإقفال الموحّد ───────────────────────────
/// لوحة واحدة تجمع ما يمنع الإقفال: فترات مفتوحة، مسودات غير مرحّلة،
/// إقرارات غير مقدَّمة، وموازنات غير معتمدة. لا كتابة — قراءة فقط.
pub async fn close_hub(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ReturnQuery>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let cid = scope.company_id;
        let open_periods: i64 = db.query_row(
            "SELECT COUNT(*) FROM financial_periods p
             JOIN finance_period_context c ON c.period_id=p.id
             WHERE c.company_id=?1 AND p.status='open'",
            [cid],
            |r| r.get(0),
        )?;
        let unposted: i64 = db.query_row(
            "SELECT COUNT(*) FROM journal_entries WHERE company_id=?1 AND finalized=0",
            [cid],
            |r| r.get(0),
        )?;
        let draft_docs: i64 = db.query_row(
            "SELECT COUNT(*) FROM finance_notes WHERE company_id=?1 AND status='draft'",
            [cid],
            |r| r.get(0),
        )?;
        let pending_returns: i64 = db.query_row(
            "SELECT COUNT(*) FROM finance_tax_returns WHERE company_id=?1 AND status='draft'",
            [cid],
            |r| r.get(0),
        )?;
        let approved_budgets: i64 = db.query_row(
            "SELECT COUNT(*) FROM finance_budgets WHERE company_id=?1 AND status='approved'",
            [cid],
            |r| r.get(0),
        )?;
        let closed_years: i64 = db.query_row(
            "SELECT COUNT(*) FROM finance_closings WHERE company_id=?1",
            [cid],
            |r| r.get(0),
        )
        .unwrap_or(0);
        let blockers: Vec<serde_json::Value> = [
            (open_periods, "فترة مالية ما زالت مفتوحة"),
            (unposted, "قيود مسودة لم تُرحّل"),
            (draft_docs, "إشعارات مسودة لم تُرحّل"),
            (pending_returns, "إقرارات ضريبية لم تُقدَّم"),
        ]
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, label)| serde_json::json!({"count": n, "reason": label}))
        .collect();
        Ok(serde_json::json!({
            "open_periods": open_periods,
            "unposted_entries": unposted,
            "draft_notes": draft_docs,
            "pending_tax_returns": pending_returns,
            "approved_budgets": approved_budgets,
            "closed_years": closed_years,
            "blockers": blockers,
            "ready": blockers.is_empty()
        }))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}
