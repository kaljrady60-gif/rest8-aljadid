//! الإقفال السنوي: تُصفَّر حسابات الإيرادات والمصروفات في حساب الأرباح المحتجزة،
//! ويُقفل سجل السنة نهائيًا مع منع التعديل، ويُسجَّل أثر التدقيق.
use super::audit;
use super::gl;
use super::money::{self, render};
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
pub struct ListQuery {
    company_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloseInput {
    company_id: i64,
    fiscal_year: i64,
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    retained_code: Option<String>,
    /// حساب الأرباح المحتجزة صراحةً (يتقدّم على البحث بالرمز).
    #[serde(default)]
    retained_account_id: Option<i64>,
    #[serde(default)]
    notes: Option<String>,
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
            "SELECT fiscal_year,net_income,entry_id,closed_at,notes
             FROM finance_year_closings WHERE company_id=?1 ORDER BY fiscal_year DESC",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            Ok(serde_json::json!({
                "fiscal_year": r.get::<_, i64>(0)?,
                "net_income": r.get::<_, String>(1)?,
                "entry_id": r.get::<_, i64>(2)?,
                "closed_at": r.get::<_, String>(3)?,
                "notes": r.get::<_, Option<String>>(4)?
            }))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })();
    match result {
        Ok(items) => Json(items).into_response(),
        Err(e) => error_response(e),
    }
}

/// الأرصدة الصافية لحسابات النتيجة خلال سنة (مدين − دائن لكل حساب).
fn pnl_balances(
    db: &rusqlite::Connection,
    company_id: i64,
    currency: &str,
    year: i64,
) -> Result<Vec<(i64, String, Decimal)>, FinanceError> {
    let from = format!("{year:04}-01-01");
    let to = format!("{year:04}-12-31");
    let mut stmt = db.prepare(
        "SELECT l.account_id, COALESCE(c.code,''), l.debit, l.credit
         FROM journal_lines l
         JOIN journal_entries e ON e.id=l.entry_id
         JOIN accounts a ON a.id=l.account_id
         LEFT JOIN finance_company_account_codes c ON c.account_id=a.id AND c.company_id=e.company_id
         WHERE e.finalized=1 AND e.company_id=?1 AND e.currency=?2
           AND a.category IN ('revenue','expense')
           AND e.entry_date BETWEEN ?3 AND ?4",
    )?;
    let mut map: BTreeMap<i64, (String, Decimal)> = BTreeMap::new();
    let rows = stmt.query_map(params![company_id, currency, from, to], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (id, code, debit, credit) = row?;
        let entry = map.entry(id).or_insert((code, Decimal::ZERO));
        entry.1 += money::from_db(&debit, "مدين")? - money::from_db(&credit, "دائن")?;
    }
    Ok(map
        .into_iter()
        .map(|(id, (code, balance))| (id, code, balance))
        .collect())
}

pub async fn close(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(input): Json<CloseInput>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        if !(1900..=2999).contains(&input.fiscal_year) {
            return Err(FinanceError::Invalid("سنة مالية غير صالحة.".into()));
        }
        let currency = input.currency.clone().unwrap_or_else(|| "YER".into());
        if !crate::generated::contracts::supported_currency(&currency) {
            return Err(FinanceError::Invalid("العملة غير مدعومة.".into()));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, input.company_id)?;
        scope.require_admin()?;
        let done: Option<i64> = tx
            .query_row(
                "SELECT id FROM finance_year_closings WHERE company_id=?1 AND fiscal_year=?2",
                params![scope.company_id, input.fiscal_year],
                |r| r.get(0),
            )
            .optional()?;
        if done.is_some() {
            return Err(FinanceError::Conflict("هذه السنة مقفلة مسبقًا."));
        }
        let balances = pnl_balances(&tx, scope.company_id, &currency, input.fiscal_year)?;
        if balances.is_empty() {
            return Err(FinanceError::Invalid(
                "لا توجد حركة إيرادات أو مصروفات لهذه السنة.".into(),
            ));
        }
        let retained = match input.retained_account_id {
            Some(id) => id,
            None => {
                let code = input.retained_code.clone().unwrap_or_else(|| "3200".into());
                let mut found = gl::account_by_code(&tx, scope.company_id, &code)?;
                for fallback in ["3200", "3900", "3100", "3000"] {
                    if found.is_some() {
                        break;
                    }
                    found = gl::account_by_code(&tx, scope.company_id, fallback)?;
                }
                if found.is_none() {
                    // آخر خيار: أي حساب تفصيلي من تصنيف حقوق الملكية في هذه الشركة.
                    found = tx
                        .query_row(
                            "SELECT a.id FROM accounts a
                             JOIN finance_company_account_codes c ON c.account_id=a.id AND c.company_id=?1
                             WHERE a.category='equity' AND a.kind='detail' ORDER BY a.code LIMIT 1",
                            [scope.company_id],
                            |r| r.get(0),
                        )
                        .optional()?;
                }
                found.ok_or_else(|| {
                    FinanceError::Invalid(
                        "لا يوجد حساب أرباح محتجزة؛ حدّده صراحةً أو أضف حسابًا من تصنيف حقوق الملكية."
                            .into(),
                    )
                })?
            }
        };
        let mut lines: Vec<gl::Line> = Vec::new();
        let mut net = Decimal::ZERO;
        for (account, _code, balance) in &balances {
            if balance.is_zero() {
                continue;
            }
            // ربح (رصيد دائن سالب بعد الطرح) ⇒ مدين الحساب، والعكس للمصروف.
            if *balance > Decimal::ZERO {
                lines.push((*account, Decimal::ZERO, *balance));
            } else {
                lines.push((*account, -*balance, Decimal::ZERO));
            }
            net -= *balance;
        }
        if net > Decimal::ZERO {
            lines.push((retained, Decimal::ZERO, net));
        } else if net < Decimal::ZERO {
            lines.push((retained, -net, Decimal::ZERO));
        }
        let date = format!("{:04}-12-31", input.fiscal_year);
        let ctx = gl::context(&tx, scope.company_id, &date)?;
        let entry_id = gl::post_entry(
            &tx,
            ctx,
            &date,
            &format!("إقفال السنة المالية {}", input.fiscal_year),
            &currency,
            &lines,
            Some(actor.id),
            "closing.year",
        )?;
        tx.execute(
            "INSERT INTO finance_year_closings(company_id,fiscal_year,entry_id,net_income,closed_by,notes)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                scope.company_id,
                input.fiscal_year,
                entry_id,
                render(net),
                actor.id,
                input.notes.as_deref()
            ],
        )?;
        // إقفال الفترة التي تنتهي في 31/12 إن كانت مفتوحة.
        tx.execute(
            "UPDATE financial_periods SET status='closed'
             WHERE status='open' AND end_date=?1
               AND id IN (SELECT period_id FROM finance_period_context WHERE company_id=?2)",
            params![date, scope.company_id],
        )?;
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "closing.confirmed",
                entity: "finance_year_closings",
                entity_id: Some(entry_id.to_string()),
                payload: Some(serde_json::json!({
                    "fiscal_year": input.fiscal_year, "net_income": render(net), "currency": currency
                })),
            },
        )?;
        tx.commit()?;
        Ok(serde_json::json!({
            "fiscal_year": input.fiscal_year,
            "entry_id": entry_id,
            "net_income": render(net),
            "accounts_closed": balances.len(),
            "currency": currency
        }))
    })();
    match result {
        Ok(v) => (StatusCode::CREATED, Json(v)).into_response(),
        Err(e) => error_response(e),
    }
}
