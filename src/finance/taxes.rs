//! الضرائب والرسوم: تعريف ضريبة بنسبة بالأساس-نقطة (bp) وحسابها بدقة عشرية تامة.
//!
//! لماذا الأساس-النقطة: النسبة تُخزَّن عددًا صحيحًا (1500 = 15%) فلا يقع أي خطأ تمثيل
//! عائم في القيمة المخزّنة، والحساب كله على `Decimal` ثم تقريب HALF_UP لمنازل المال.
use super::audit;
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use rust_decimal::Decimal;
use rust_decimal::RoundingStrategy;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

pub const KINDS: &[&str] = &["vat", "sales", "purchase", "withholding"];
const SCALE: u32 = 4;
const MAX_BP: i64 = 10_000;

#[derive(Serialize)]
pub struct Tax {
    id: i64,
    code: String,
    name: String,
    kind: String,
    rate_bp: i64,
    account_id: i64,
    is_inclusive: bool,
    archived_at: Option<String>,
}

#[derive(Deserialize)]
pub struct ListQuery {
    company_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewTax {
    company_id: i64,
    code: String,
    name: String,
    kind: String,
    rate_bp: i64,
    account_id: i64,
    #[serde(default)]
    is_inclusive: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveRequest {
    company_id: i64,
    id: i64,
}

#[derive(Deserialize)]
pub struct ComputeQuery {
    company_id: i64,
    tax_id: i64,
    amount: String,
}

/// يثبّت المبلغ على منازل المال الأربع (SCALE) بلا فقدان قيمة.
///
/// السبب: قسمة `Decimal` قد تُنتج منازل أقل (100.00 بدل 100.0000)، فيختلف النص المخزَّن
/// عن النص المحسوب ويُفسد أي مقارنة نصية لاحقًا. التوحيد هنا يمنع هذا الصنف من الأخطاء جذريًا.
fn canonical(value: Decimal) -> Decimal {
    let mut out = value.round_dp_with_strategy(SCALE, RoundingStrategy::MidpointAwayFromZero);
    out.rescale(SCALE);
    out
}

/// يقسم المبلغ إلى وعاء وضريبة. عند `inclusive` يكون المجموع مساويًا للمبلغ تمامًا.
pub fn split(amount: Decimal, rate_bp: i64, inclusive: bool) -> (Decimal, Decimal) {
    if amount <= Decimal::ZERO || rate_bp <= 0 {
        return (canonical(amount), canonical(Decimal::ZERO));
    }
    let rate = Decimal::from(rate_bp) / Decimal::from(MAX_BP);
    if inclusive {
        let base = canonical(amount / (Decimal::ONE + rate));
        let tax = canonical(amount - base);
        (base, tax)
    } else {
        (canonical(amount), canonical(amount * rate))
    }
}

pub async fn list(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ListQuery>,
) -> Response {
    let result = (|| -> Result<Vec<Tax>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT id,code,name,kind,rate_bp,account_id,is_inclusive,archived_at
             FROM finance_taxes WHERE company_id=?1
             ORDER BY archived_at IS NOT NULL, code",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            Ok(Tax {
                id: r.get(0)?,
                code: r.get(1)?,
                name: r.get(2)?,
                kind: r.get(3)?,
                rate_bp: r.get(4)?,
                account_id: r.get(5)?,
                is_inclusive: r.get::<_, i64>(6)? != 0,
                archived_at: r.get(7)?,
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
    Json(n): Json<NewTax>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        let code = n.code.trim();
        let name = n.name.trim();
        if code.is_empty() || code.chars().count() > 20 {
            return Err(FinanceError::Invalid(
                "رمز الضريبة مطلوب حتى 20 حرفًا.".into(),
            ));
        }
        if name.is_empty() || name.chars().count() > 120 {
            return Err(FinanceError::Invalid(
                "اسم الضريبة مطلوب حتى 120 حرفًا.".into(),
            ));
        }
        if !KINDS.contains(&n.kind.as_str()) {
            return Err(FinanceError::Invalid("نوع الضريبة غير مدعوم.".into()));
        }
        if !(0..=MAX_BP).contains(&n.rate_bp) {
            return Err(FinanceError::Invalid(
                "النسبة يجب أن تكون بين 0% و100%.".into(),
            ));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        scope.require_admin()?;
        let account_ok: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1)",
            [n.account_id],
            |r| r.get(0),
        )?;
        if !account_ok {
            return Err(FinanceError::Invalid(
                "حساب الضريبة غير موجود في الدليل.".into(),
            ));
        }
        tx.execute(
            "INSERT INTO finance_taxes(company_id,code,name,kind,rate_bp,account_id,is_inclusive)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                scope.company_id,
                code,
                name,
                n.kind,
                n.rate_bp,
                n.account_id,
                i64::from(n.is_inclusive)
            ],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(_, _) => FinanceError::Conflict("رمز الضريبة مستخدم."),
            other => FinanceError::Storage(other),
        })?;
        let id = tx.last_insert_rowid();
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "tax.create",
                entity: "finance_taxes",
                entity_id: Some(id.to_string()),
                payload: Some(
                    serde_json::json!({"code": code, "rate_bp": n.rate_bp, "kind": n.kind}),
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
            "UPDATE finance_taxes SET archived_at=CURRENT_TIMESTAMP
             WHERE id=?1 AND company_id=?2 AND archived_at IS NULL",
            params![req.id, scope.company_id],
        )?;
        if changed == 0 {
            return Err(FinanceError::NotFound("الضريبة"));
        }
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "tax.archive",
                entity: "finance_taxes",
                entity_id: Some(req.id.to_string()),
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

/// معاينة احتساب: تُعيد الوعاء والضريبة والإجمالي بلا كتابة أي بيانات.
pub async fn compute(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ComputeQuery>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let amount = Decimal::from_str(q.amount.trim())
            .map_err(|_| FinanceError::Invalid("المبلغ ليس رقمًا صالحًا.".into()))?;
        if amount.is_sign_negative() {
            return Err(FinanceError::Invalid("المبلغ يجب أن يكون موجبًا.".into()));
        }
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let tax = db
            .query_row(
                "SELECT rate_bp,is_inclusive,name FROM finance_taxes
                 WHERE id=?1 AND company_id=?2 AND archived_at IS NULL",
                params![q.tax_id, scope.company_id],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)? != 0,
                        r.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or(FinanceError::NotFound("الضريبة"))?;
        let (base, value) = split(amount, tax.0, tax.1);
        Ok(serde_json::json!({
            "tax": tax.2,
            "rate_bp": tax.0,
            "inclusive": tax.1,
            "base": super::money::render(base),
            "tax_value": super::money::render(value),
            "total": super::money::render(base + value),
        }))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}
