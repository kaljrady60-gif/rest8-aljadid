//! الصناديق والبنوك: كل صندوق مرتبط بحساب تفصيلي في دليل الشركة،
//! ورصيده مشتق من القيود المرحّلة — لا يُخزَّن رصيد مستقل يتعارض معها.
use super::audit;
use super::money;
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Serialize)]
pub struct Fund {
    id: i64,
    kind: String,
    code: String,
    name: String,
    account_id: i64,
    account_label: String,
    currency: String,
    iban: Option<String>,
    opened_on: String,
    archived_at: Option<String>,
    balance: String,
}

#[derive(Deserialize)]
pub struct ListQuery {
    company_id: i64,
    kind: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewFund {
    company_id: i64,
    kind: String,
    code: String,
    name: String,
    account_id: i64,
    iban: Option<String>,
    opened_on: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveRequest {
    company_id: i64,
    fund_id: i64,
}

fn validate(n: &NewFund) -> Result<(), FinanceError> {
    if !crate::generated::contracts::funds::KINDS.contains(&n.kind.as_str()) {
        return Err(FinanceError::Invalid("نوع الصندوق غير مدعوم.".into()));
    }
    let code = n.code.trim();
    if code.is_empty() || code.len() > 20 {
        return Err(FinanceError::Invalid(
            "رمز الصندوق مطلوب وبحد أقصى 20 حرفًا.".into(),
        ));
    }
    let name = n.name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(FinanceError::Invalid(
            "اسم الصندوق مطلوب وبحد أقصى 120 حرفًا.".into(),
        ));
    }
    if chrono::NaiveDate::parse_from_str(&n.opened_on, "%Y-%m-%d").is_err() {
        return Err(FinanceError::Invalid("تاريخ الافتتاح غير صالح.".into()));
    }
    let iban = n.iban.as_deref().unwrap_or("").trim();
    if iban.len() > 34 {
        return Err(FinanceError::Invalid("رقم الحساب البنكي طويل جدًا.".into()));
    }
    Ok(())
}

pub async fn list(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ListQuery>,
) -> Response {
    let result = (|| -> Result<Vec<Fund>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let kind = q.kind.as_deref().unwrap_or("");
        if !kind.is_empty() && !crate::generated::contracts::funds::KINDS.contains(&kind) {
            return Err(FinanceError::Invalid("نوع الصندوق غير مدعوم.".into()));
        }
        let mut stmt = db.prepare(
            "SELECT f.id,f.kind,COALESCE(f.code,''),f.name,f.account_id,
                    COALESCE(c.code,'')||' · '||a.name,
                    COALESCE(f.currency,a.currency),f.iban,
                    COALESCE(f.opened_on,''),f.archived_at
             FROM finance_funds f
             JOIN accounts a ON a.id=f.account_id
             LEFT JOIN finance_company_account_codes c
                    ON c.account_id=a.id AND c.company_id=f.company_id
             WHERE f.company_id=?1 AND (?2='' OR f.kind=?2)
             ORDER BY f.kind, f.code, f.id",
        )?;
        let rows = stmt.query_map(params![scope.company_id, kind], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, Option<String>>(9)?,
            ))
        })?;
        let mut funds = Vec::new();
        for row in rows {
            let (
                id,
                kind,
                code,
                name,
                account_id,
                account_label,
                currency,
                iban,
                opened_on,
                archived,
            ) = row?;
            let balance = account_balance(&db, scope.company_id, account_id, &currency)?;
            funds.push(Fund {
                id,
                kind,
                code,
                name,
                account_id,
                account_label,
                currency,
                iban,
                opened_on,
                archived_at: archived,
                balance: money::render(balance),
            });
        }
        Ok(funds)
    })();
    match result {
        Ok(funds) => Json(funds).into_response(),
        Err(e) => error_response(e),
    }
}

/// رصيد الحساب من القيود المرحّلة للشركة: مدين − دائن.
fn account_balance(
    db: &rusqlite::Connection,
    company_id: i64,
    account_id: i64,
    currency: &str,
) -> Result<Decimal, FinanceError> {
    let mut stmt = db.prepare(
        "SELECT l.debit, l.credit FROM journal_lines l JOIN journal_entries e ON e.id=l.entry_id
         WHERE e.finalized=1 AND e.company_id=?1 AND e.currency=?2 AND l.account_id=?3",
    )?;
    let rows = stmt.query_map(params![company_id, currency, account_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut total = Decimal::ZERO;
    for row in rows {
        let (debit, credit) = row?;
        total += Decimal::from_str(&debit).unwrap_or(Decimal::ZERO)
            - Decimal::from_str(&credit).unwrap_or(Decimal::ZERO);
    }
    Ok(money::normalize(total))
}

pub async fn create(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<NewFund>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        validate(&n)?;
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        scope.require_admin()?;
        let branch_id: Option<i64> = tx
            .query_row(
                "SELECT id FROM finance_branches WHERE company_id=?1 ORDER BY id LIMIT 1",
                [scope.company_id],
                |r| r.get(0),
            )
            .optional()?;
        let branch_id = branch_id.ok_or(FinanceError::Invalid(
            "أنشئ فرعًا للشركة قبل تسجيل صندوق.".into(),
        ))?;
        let account: Option<(String, String, String)> = tx
            .query_row(
                "SELECT a.kind,a.currency,c.code FROM accounts a
                 JOIN finance_company_account_codes c ON c.account_id=a.id AND c.company_id=?2
                 WHERE a.id=?1",
                params![n.account_id, scope.company_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (kind, currency, _code) = match account {
            Some(found) => found,
            None => {
                return Err(FinanceError::Invalid(
                    "الحساب ليس من دليل هذه الشركة.".into(),
                ));
            }
        };
        if kind != "detail" {
            return Err(FinanceError::Invalid(
                "يجب ربط الصندوق بحساب تفصيلي لا رئيسي.".into(),
            ));
        }
        tx.execute(
            "INSERT INTO finance_funds
             (company_id,branch_id,kind,code,name,account_id,currency,iban,opened_on)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                scope.company_id,
                branch_id,
                n.kind,
                n.code.trim(),
                n.name.trim(),
                n.account_id,
                currency,
                n.iban.as_deref().map(str::trim).filter(|s| !s.is_empty()),
                n.opened_on
            ],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(_, _) => FinanceError::Conflict("رمز الصندوق مستخدم."),
            other => FinanceError::Storage(other),
        })?;
        let id = tx.last_insert_rowid();
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "fund.create",
                entity: "finance_funds",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({"kind": n.kind, "code": n.code.trim()})),
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

pub async fn archive(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(req): Json<ArchiveRequest>,
) -> Response {
    let result = (|| -> Result<(), FinanceError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, req.company_id)?;
        scope.require_admin()?;
        let changed = tx.execute(
            "UPDATE finance_funds SET archived_at=CURRENT_TIMESTAMP
             WHERE id=?1 AND company_id=?2 AND archived_at IS NULL",
            params![req.fund_id, scope.company_id],
        )?;
        if changed == 0 {
            return Err(FinanceError::NotFound("الصندوق"));
        }
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "fund.archive",
                entity: "finance_funds",
                entity_id: Some(req.fund_id.to_string()),
                payload: None,
            },
        )?;
        tx.commit()?;
        Ok(())
    })();
    match result {
        Ok(()) => Json(serde_json::json!({"archived": true})).into_response(),
        Err(e) => error_response(e),
    }
}
