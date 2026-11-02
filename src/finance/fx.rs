//! أسعار الصرف: سعر واحد لكل (شركة، من، إلى، تاريخ) — لا تصحيح صامت ولا صفر.
//! التحويل يتم بعمليات عشرية صريحة ويُرفض إن غاب السعر.
use super::audit;
use super::money::{self, render};
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct Rate {
    id: i64,
    base: String,
    quote: String,
    rate: String,
    as_of: String,
}

#[derive(Deserialize)]
pub struct RatesQuery {
    company_id: i64,
    base: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewRate {
    company_id: i64,
    base: String,
    quote: String,
    rate: String,
    as_of: String,
}

#[derive(Deserialize)]
pub struct ConvertQuery {
    company_id: i64,
    base: String,
    quote: String,
    amount: String,
    as_of: Option<String>,
}

fn check_codes(base: &str, quote: &str) -> Result<(), FinanceError> {
    let supported = crate::generated::contracts::supported_currency;
    if !supported(base) || !supported(quote) {
        return Err(FinanceError::Invalid("العملة غير مدعومة.".into()));
    }
    if base == quote {
        return Err(FinanceError::Invalid(
            "لا حاجة لسعر صرف بين العملة ونفسها.".into(),
        ));
    }
    Ok(())
}

/// أحدث سعر معروف عند تاريخ معيّن (أو الأحدث إجمالًا).
pub fn latest(
    db: &rusqlite::Connection,
    company_id: i64,
    base: &str,
    quote: &str,
    as_of: Option<&str>,
) -> Result<Option<(Decimal, String)>, FinanceError> {
    let row: Option<(String, String)> = match as_of {
        Some(day) => db
            .query_row(
                "SELECT rate,as_of FROM finance_fx_rates
                 WHERE company_id=?1 AND base=?2 AND quote=?3 AND as_of<=?4
                 ORDER BY as_of DESC, id DESC LIMIT 1",
                params![company_id, base, quote, day],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?,
        None => db
            .query_row(
                "SELECT rate,as_of FROM finance_fx_rates
                 WHERE company_id=?1 AND base=?2 AND quote=?3
                 ORDER BY as_of DESC, id DESC LIMIT 1",
                params![company_id, base, quote],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?,
    };
    match row {
        Some((rate, day)) => Ok(Some((money::from_db(&rate, "سعر الصرف")?, day))),
        None => Ok(None),
    }
}

pub async fn list(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<RatesQuery>,
) -> Response {
    let result = (|| -> Result<Vec<Rate>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let base = q.base.as_deref().unwrap_or("");
        let mut stmt = db.prepare(
            "SELECT id,base,quote,rate,as_of FROM finance_fx_rates
             WHERE company_id=?1 AND (?2='' OR base=?2)
             ORDER BY as_of DESC, base, quote",
        )?;
        let rows = stmt.query_map(params![scope.company_id, base], |r| {
            Ok(Rate {
                id: r.get(0)?,
                base: r.get(1)?,
                quote: r.get(2)?,
                rate: r.get(3)?,
                as_of: r.get(4)?,
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
    Json(n): Json<NewRate>,
) -> Response {
    let result =
        (|| -> Result<i64, FinanceError> {
            check_codes(n.base.trim(), n.quote.trim())?;
            if chrono::NaiveDate::parse_from_str(&n.as_of, "%Y-%m-%d").is_err() {
                return Err(FinanceError::Invalid("تاريخ السعر غير صالح.".into()));
            }
            let rate = money::parse(&n.rate, "سعر الصرف")?;
            if rate <= Decimal::ZERO {
                return Err(FinanceError::Invalid(
                    "سعر الصرف يجب أن يكون أكبر من صفر.".into(),
                ));
            }
            let mut db = db_open(&state.db)?;
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
            scope.require_admin()?;
            tx.execute(
            "INSERT INTO finance_fx_rates(company_id,base,quote,rate,as_of) VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(company_id,base,quote,as_of) DO UPDATE SET rate=excluded.rate",
            params![scope.company_id, n.base.trim(), n.quote.trim(), render(rate), n.as_of],
        )?;
            let id = tx.last_insert_rowid();
            audit::record(
                &tx,
                audit::Entry {
                    company_id: Some(scope.company_id),
                    actor_id: Some(actor.id),
                    action: "fx.upsert",
                    entity: "finance_fx_rates",
                    entity_id: Some(id.to_string()),
                    payload: Some(serde_json::json!({
                        "base": n.base.trim(), "quote": n.quote.trim(),
                        "rate": render(rate), "as_of": n.as_of
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

pub async fn convert(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ConvertQuery>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        check_codes(q.base.trim(), q.quote.trim())?;
        let amount = money::parse(&q.amount, "المبلغ")?;
        let found = latest(
            &db,
            scope.company_id,
            q.base.trim(),
            q.quote.trim(),
            q.as_of.as_deref(),
        )?
        .or(latest(
            &db,
            scope.company_id,
            q.quote.trim(),
            q.base.trim(),
            q.as_of.as_deref(),
        )?
        .map(|(r, d)| (Decimal::ONE / r, d)));
        let (rate, day) = match found {
            Some(v) => v,
            None => {
                return Err(FinanceError::Invalid(
                    "لا يوجد سعر صرف مسجّل لهذا الزوج؛ سجّل السعر أولًا.".into(),
                ));
            }
        };
        Ok(serde_json::json!({
            "base": q.base.trim(), "quote": q.quote.trim(),
            "amount": render(amount), "rate": render(rate), "as_of": day,
            "result": render(money::normalize(amount * rate))
        }))
    })();
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => error_response(e),
    }
}
