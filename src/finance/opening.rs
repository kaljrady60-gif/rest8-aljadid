//! الأرصدة الافتتاحية: تُدخل مرة واحدة قبل أي ترحيل، ويجب أن تتوازن بالعملة.
//! بعد وجود قيود مرحّلة تُصبح القراءة فقط — حماية لسلامة الأرصدة.
use super::audit;
use super::money::{self, render};
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{TransactionBehavior, params};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct OpeningLine {
    account_id: i64,
    account_code: String,
    account_name: String,
    currency: String,
    debit: String,
    credit: String,
}

#[derive(Deserialize)]
pub struct OpeningQuery {
    company_id: i64,
    currency: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpeningInput {
    company_id: i64,
    currency: String,
    lines: Vec<OpeningRow>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpeningRow {
    account_id: i64,
    debit: String,
    credit: String,
}

/// هل توجد قيود مرحّلة لهذه الشركة؟ (وقتها لا يُسمح بتعديل الافتتاحي)
fn has_posted(db: &rusqlite::Connection, company_id: i64) -> Result<bool, FinanceError> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM journal_entries WHERE company_id=?1 AND finalized=1)",
        [company_id],
        |r| r.get(0),
    )?;
    Ok(exists)
}

pub async fn list(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<OpeningQuery>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let currency = q.currency.unwrap_or_else(|| "YER".into());
        if !crate::generated::contracts::supported_currency(&currency) {
            return Err(FinanceError::Invalid("العملة غير مدعومة.".into()));
        }
        let mut stmt = db.prepare(
            "SELECT o.account_id,c.code,a.name,o.currency,o.debit,o.credit
             FROM finance_opening_balances o
             JOIN accounts a ON a.id=o.account_id
             LEFT JOIN finance_company_account_codes c ON c.account_id=a.id AND c.company_id=o.company_id
             WHERE o.company_id=?1 AND o.currency=?2 ORDER BY c.code",
        )?;
        let rows = stmt.query_map(params![scope.company_id, currency], |r| {
            Ok(OpeningLine {
                account_id: r.get(0)?,
                account_code: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                account_name: r.get(2)?,
                currency: r.get(3)?,
                debit: r.get(4)?,
                credit: r.get(5)?,
            })
        })?;
        let lines: Vec<OpeningLine> = rows.collect::<Result<Vec<_>, _>>()?;
        let mut debit = Decimal::ZERO;
        let mut credit = Decimal::ZERO;
        for line in &lines {
            debit += money::from_db(&line.debit, "مدين افتتاحي")?;
            credit += money::from_db(&line.credit, "دائن افتتاحي")?;
        }
        Ok(serde_json::json!({
            "company_id": scope.company_id,
            "currency": currency,
            "locked": has_posted(&db, scope.company_id)?,
            "totals": {"debit": render(debit), "credit": render(credit),
                       "balanced": debit == credit},
            "lines": lines,
        }))
    })();
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn save(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(input): Json<OpeningInput>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        if !crate::generated::contracts::supported_currency(&input.currency) {
            return Err(FinanceError::Invalid("العملة غير مدعومة.".into()));
        }
        if input.lines.len() > crate::generated::contracts::limits::MAX_LINES_PER_ENTRY * 4 {
            return Err(FinanceError::Invalid("عدد سطور الافتتاحي كبير جدًا.".into()));
        }
        let mut parsed = Vec::with_capacity(input.lines.len());
        let mut debit_total = Decimal::ZERO;
        let mut credit_total = Decimal::ZERO;
        for row in &input.lines {
            let debit = money::parse(&row.debit, "مدين افتتاحي")?;
            let credit = money::parse(&row.credit, "دائن افتتاحي")?;
            if debit > Decimal::ZERO && credit > Decimal::ZERO {
                return Err(FinanceError::Invalid(
                    "لا يجوز أن يكون السطر مدينًا ودائنًا معًا.".into(),
                ));
            }
            if debit == Decimal::ZERO && credit == Decimal::ZERO {
                continue;
            }
            debit_total += debit;
            credit_total += credit;
            parsed.push((row.account_id, debit, credit));
        }
        if debit_total != credit_total {
            return Err(FinanceError::Invalid(format!(
                "الأرصدة الافتتاحية غير متوازنة: مدين {} مقابل دائن {}.",
                render(debit_total),
                render(credit_total)
            )));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, input.company_id)?;
        scope.require_admin()?;
        if has_posted(&tx, scope.company_id)? {
            return Err(FinanceError::Conflict(
                "لا يمكن تعديل الأرصدة الافتتاحية بعد وجود قيود مرحّلة.",
            ));
        }
        for (account_id, _, _) in &parsed {
            if !scope.owns_account(&tx, *account_id)? {
                return Err(FinanceError::Invalid(
                    "حساب افتتاحي لا ينتمي إلى هذه الشركة.".into(),
                ));
            }
        }
        tx.execute(
            "DELETE FROM finance_opening_balances WHERE company_id=?1 AND currency=?2",
            params![scope.company_id, input.currency],
        )?;
        for (account_id, debit, credit) in &parsed {
            tx.execute(
                "INSERT INTO finance_opening_balances
                 (company_id,account_id,currency,debit,credit,created_by)
                 VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    scope.company_id,
                    account_id,
                    input.currency,
                    render(*debit),
                    render(*credit),
                    // معاينة مفتوحة بلا مستخدم حقيقي: لا نخزّن معرّفًا وهميًا (FK).
                    (actor.id > 0).then_some(actor.id)
                ],
            )?;
        }
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "opening.save",
                entity: "finance_opening_balances",
                entity_id: Some(input.currency.clone()),
                payload: Some(serde_json::json!({
                    "rows": parsed.len(), "debit": render(debit_total), "currency": input.currency
                })),
            },
        )?;
        tx.commit()?;
        Ok(serde_json::json!({
            "saved": parsed.len(), "debit": render(debit_total), "credit": render(credit_total)
        }))
    })();
    match result {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(e) => error_response(e),
    }
}
