//! التسوية البنكية: كشف لكل صندوق/بنك، مع بنود مُسوّاة وأخرى معلّقة،
//! والفرق يُحسب صريحًا (رصيد الدفاتر + معلّقات مقابل رصيد الكشف).
use super::audit;
use super::money::{self, render};
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use rust_decimal::Decimal;
use serde::Deserialize;

const LINE_KINDS: &[&str] = &["deposit", "withdrawal", "adjustment", "charge", "interest"];

#[derive(Deserialize)]
pub struct ListQuery {
    company_id: i64,
    fund_id: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewStatement {
    company_id: i64,
    fund_id: i64,
    period_start: String,
    period_end: String,
    closing_balance: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewLine {
    company_id: i64,
    kind: String,
    amount: String,
    memo: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClearLine {
    company_id: i64,
    cleared: bool,
}

pub async fn list(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ListQuery>,
) -> Response {
    let result = (|| -> Result<Vec<serde_json::Value>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT s.id,s.fund_id,f.name,s.period_start,s.period_end,s.closing_balance
             FROM finance_bank_statements s JOIN finance_funds f ON f.id=s.fund_id
             WHERE s.company_id=?1 AND (?2 IS NULL OR s.fund_id=?2)
             ORDER BY s.period_end DESC, s.id DESC",
        )?;
        let rows = stmt.query_map(params![scope.company_id, q.fund_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, fund_id, fund_name, start, end, closing) = row?;
            let closing = money::from_db(&closing, "رصيد الكشف")?;
            let (cleared, pending) = line_totals(&db, id)?;
            out.push(serde_json::json!({
                "id": id, "fund_id": fund_id, "fund_name": fund_name,
                "period_start": start, "period_end": end,
                "statement_balance": render(closing),
                "cleared_total": render(cleared), "pending_total": render(pending),
                "difference": render(closing - cleared - pending)
            }));
        }
        Ok(out)
    })();
    match result {
        Ok(items) => Json(items).into_response(),
        Err(e) => error_response(e),
    }
}

fn line_totals(
    db: &rusqlite::Connection,
    statement_id: i64,
) -> Result<(Decimal, Decimal), FinanceError> {
    let mut stmt =
        db.prepare("SELECT amount,cleared FROM finance_bankrec_lines WHERE statement_id=?1")?;
    let rows = stmt.query_map([statement_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? == 1))
    })?;
    let (mut cleared, mut pending) = (Decimal::ZERO, Decimal::ZERO);
    for row in rows {
        let (amount, is_cleared) = row?;
        let value = money::from_db(&amount, "بند تسوية")?;
        if is_cleared {
            cleared += value;
        } else {
            pending += value;
        }
    }
    Ok((cleared, pending))
}

pub async fn create(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<NewStatement>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        for day in [&n.period_start, &n.period_end] {
            if chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").is_err() {
                return Err(FinanceError::Invalid("تاريخ فترة الكشف غير صالح.".into()));
            }
        }
        if n.period_start > n.period_end {
            return Err(FinanceError::Invalid("بداية الكشف بعد نهايته.".into()));
        }
        let closing = money::parse(&n.closing_balance, "رصيد الكشف")?;
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        scope.require_admin()?;
        let fund: Option<String> = tx
            .query_row(
                "SELECT kind FROM finance_funds WHERE id=?1 AND company_id=?2 AND archived_at IS NULL",
                params![n.fund_id, scope.company_id],
                |r| r.get(0),
            )
            .optional()?;
        match fund.as_deref() {
            Some("bank") => {}
            Some(_) => {
                return Err(FinanceError::Invalid(
                    "التسوية البنكية تخص البنوك لا الصناديق النقدية.".into(),
                ));
            }
            None => return Err(FinanceError::NotFound("الحساب البنكي")),
        }
        tx.execute(
            "INSERT INTO finance_bank_statements
             (company_id,fund_id,period_start,period_end,closing_balance)
             VALUES(?1,?2,?3,?4,?5)",
            params![
                scope.company_id,
                n.fund_id,
                n.period_start,
                n.period_end,
                render(closing)
            ],
        )?;
        let id = tx.last_insert_rowid();
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "bankrec.create",
                entity: "finance_bank_statements",
                entity_id: Some(id.to_string()),
                payload: Some(
                    serde_json::json!({"fund_id": n.fund_id, "closing": render(closing)}),
                ),
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

pub async fn add_line(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Path(statement_id): Path<i64>,
    Json(n): Json<NewLine>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        if !LINE_KINDS.contains(&n.kind.as_str()) {
            return Err(FinanceError::Invalid("نوع بند التسوية غير مدعوم.".into()));
        }
        let amount = money::parse(&n.amount, "مبلغ البند")?;
        if n.memo.trim().is_empty() || n.memo.chars().count() > 200 {
            return Err(FinanceError::Invalid(
                "بيان البند مطلوب حتى 200 حرف.".into(),
            ));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        scope.require_admin()?;
        let owned: Option<i64> = tx
            .query_row(
                "SELECT id FROM finance_bank_statements WHERE id=?1 AND company_id=?2",
                params![statement_id, scope.company_id],
                |r| r.get(0),
            )
            .optional()?;
        if owned.is_none() {
            return Err(FinanceError::NotFound("كشف التسوية"));
        }
        // السحب والمصاريف تُخزَّن سالبة، الإيداعات موجبة — إشارة واحدة صريحة.
        let signed = match n.kind.as_str() {
            "withdrawal" | "charge" => -amount,
            _ => amount,
        };
        tx.execute(
            "INSERT INTO finance_bankrec_lines(statement_id,kind,amount,memo) VALUES(?1,?2,?3,?4)",
            params![statement_id, n.kind, render(signed), n.memo.trim()],
        )?;
        let id = tx.last_insert_rowid();
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "bankrec.line",
                entity: "finance_bankrec_lines",
                entity_id: Some(id.to_string()),
                payload: Some(
                    serde_json::json!({"statement": statement_id, "amount": render(signed)}),
                ),
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

pub async fn clear_line(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Path((statement_id, line_id)): Path<(i64, i64)>,
    Json(req): Json<ClearLine>,
) -> Response {
    let result = (|| -> Result<(), FinanceError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, req.company_id)?;
        scope.require_admin()?;
        let changed = tx.execute(
            "UPDATE finance_bankrec_lines SET cleared=?1
             WHERE id=?2 AND statement_id=?3
               AND statement_id IN (SELECT id FROM finance_bank_statements
                                    WHERE company_id=?4 AND id=?3)",
            params![
                i64::from(req.cleared),
                line_id,
                statement_id,
                scope.company_id
            ],
        )?;
        if changed == 0 {
            return Err(FinanceError::NotFound("بند التسوية"));
        }
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "bankrec.clear",
                entity: "finance_bankrec_lines",
                entity_id: Some(line_id.to_string()),
                payload: Some(serde_json::json!({"cleared": req.cleared})),
            },
        )?;
        tx.commit()?;
        Ok(())
    })();
    match result {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => error_response(e),
    }
}
