//! تقارير المبيعات: أعمار الذمم، ملخّص تحليلي، وإقفال وردية POS.
use super::common;
use crate::AppState;
use crate::db_open;
use crate::finance::scope::{FinanceError, Scope, error_response};
use axum::Json;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use rusqlite::params;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
pub struct RangeQuery {
    pub company_id: i64,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
}

#[derive(Deserialize)]
pub struct ShiftQuery {
    pub company_id: i64,
    pub machine_id: i64,
    pub date: String,
}

fn today() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

fn days_between(from: &str, to: &str) -> i64 {
    match (
        chrono::NaiveDate::parse_from_str(from, "%Y-%m-%d"),
        chrono::NaiveDate::parse_from_str(to, "%Y-%m-%d"),
    ) {
        (Ok(a), Ok(b)) => (b - a).num_days().max(0),
        _ => 0,
    }
}

/// أعمار الذمم: أرصدة الفواتير غير المسدّدة موزّعة على 0-30/31-60/61-90/90+.
pub async fn aging(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<RangeQuery>,
) -> Response {
    let result = (|| -> Result<Value, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT p.name, i.invoice_date, i.total_minor - i.paid_minor
             FROM sales_invoices i JOIN finance_parties p ON p.id=i.party_id
             WHERE i.company_id=?1 AND i.status='posted' AND i.total_minor > i.paid_minor
             ORDER BY p.name",
        )?;
        let rows = stmt
            .query_map([scope.company_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let now = today();
        let mut parties: Vec<(String, [i64; 4])> = Vec::new();
        for (name, date, due) in rows {
            let age = days_between(&date, &now);
            let bucket = if age <= 30 {
                0
            } else if age <= 60 {
                1
            } else if age <= 90 {
                2
            } else {
                3
            };
            match parties.iter_mut().find(|(n, _)| *n == name) {
                Some((_, buckets)) => buckets[bucket] += due,
                None => {
                    let mut buckets = [0_i64; 4];
                    buckets[bucket] = due;
                    parties.push((name, buckets));
                }
            }
        }
        let items: Vec<Value> = parties
            .iter()
            .map(|(name, b)| {
                serde_json::json!({
                    "party": name,
                    "b0": common::money(b[0]), "b30": common::money(b[1]),
                    "b60": common::money(b[2]), "b90": common::money(b[3]),
                    "total": common::money(b.iter().sum::<i64>()),
                })
            })
            .collect();
        let grand: i64 = parties.iter().map(|(_, b)| b.iter().sum::<i64>()).sum();
        Ok(serde_json::json!({"items": items, "total": common::money(grand)}))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

/// ملخّص تحليلي: إجماليات، توزيع شهري، أفضل العملاء، أداء المندوبين، المرتجعات.
pub async fn summary(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<RangeQuery>,
) -> Response {
    let result = (|| -> Result<Value, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let from = q.from.clone().unwrap_or_else(|| "0000-01-01".into());
        let to = q.to.clone().unwrap_or_else(|| "9999-12-31".into());
        let cid = scope.company_id;
        let (sales, returns, drafts): (i64, i64, i64) = db.query_row(
            "SELECT
                COALESCE(SUM(CASE WHEN invoice_type='sale' AND status='posted' THEN total_minor END),0),
                COALESCE(SUM(CASE WHEN invoice_type='sale_return' AND status='posted' THEN total_minor END),0),
                COALESCE(SUM(CASE WHEN status='draft' THEN 1 ELSE 0 END),0)
             FROM sales_invoices WHERE company_id=?1 AND invoice_date BETWEEN ?2 AND ?3",
            params![cid, from, to],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let mut stmt = db.prepare(
            "SELECT substr(invoice_date,1,7) AS m,
                    COALESCE(SUM(CASE WHEN invoice_type='sale' THEN total_minor ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN invoice_type='sale_return' THEN total_minor ELSE 0 END),0)
             FROM sales_invoices WHERE company_id=?1 AND status='posted' AND invoice_date BETWEEN ?2 AND ?3
             GROUP BY m ORDER BY m",
        )?;
        let months: Vec<Value> = stmt
            .query_map(params![cid, from, to], |r| {
                Ok(serde_json::json!({
                    "month": r.get::<_, String>(0)?,
                    "sales": common::money(r.get::<_, i64>(1)?),
                    "returns": common::money(r.get::<_, i64>(2)?),
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut stmt = db.prepare(
            "SELECT p.name, SUM(i.total_minor) t
             FROM sales_invoices i JOIN finance_parties p ON p.id=i.party_id
             WHERE i.company_id=?1 AND i.status='posted' AND i.invoice_type='sale'
               AND i.invoice_date BETWEEN ?2 AND ?3
             GROUP BY p.name ORDER BY t DESC LIMIT 10",
        )?;
        let top: Vec<Value> = stmt
            .query_map(params![cid, from, to], |r| {
                Ok(serde_json::json!({
                    "party": r.get::<_, String>(0)?,
                    "total": common::money(r.get::<_, i64>(1)?),
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut stmt = db.prepare(
            "SELECT COALESCE(r.name,'بلا مندوب'), SUM(i.total_minor)
             FROM sales_invoices i LEFT JOIN sales_reps r ON r.id=i.rep_id
             WHERE i.company_id=?1 AND i.status='posted' AND i.invoice_type='sale'
               AND i.invoice_date BETWEEN ?2 AND ?3
             GROUP BY r.id ORDER BY 2 DESC",
        )?;
        let reps: Vec<Value> = stmt
            .query_map(params![cid, from, to], |r| {
                Ok(serde_json::json!({
                    "rep": r.get::<_, String>(0)?,
                    "total": common::money(r.get::<_, i64>(1)?),
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(serde_json::json!({
            "sales": common::money(sales),
            "returns": common::money(returns),
            "net": common::money(sales - returns),
            "drafts": drafts,
            "months": months, "top_parties": top, "reps": reps,
        }))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

/// إقفال وردية: النقد المتوقع = عهود فتح + مبيعات نقدية − مرتجعات نقدية، مقابل المسلَّم.
pub async fn shift(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ShiftQuery>,
) -> Response {
    let result = (|| -> Result<Value, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let cid = scope.company_id;
        let machine: Option<String> = db
            .query_row(
                "SELECT name FROM sales_pos_machines WHERE company_id=?1 AND id=?2",
                params![cid, q.machine_id],
                |r| r.get(0),
            )
            .ok();
        let machine = machine.ok_or(FinanceError::NotFound("الجهاز"))?;
        let opened: i64 = db.query_row(
            "SELECT COALESCE(SUM(amount_minor),0) FROM sales_pos_custody
             WHERE company_id=?1 AND machine_id=?2 AND custody_date=?3 AND kind IN ('open','handover')",
            params![cid, q.machine_id, q.date],
            |r| r.get(0),
        )?;
        let delivered: i64 = db.query_row(
            "SELECT COALESCE(SUM(amount_minor),0) FROM sales_pos_custody
             WHERE company_id=?1 AND machine_id=?2 AND custody_date=?3 AND kind='close'",
            params![cid, q.machine_id, q.date],
            |r| r.get(0),
        )?;
        let (sales, returns, count): (i64, i64, i64) = db.query_row(
            "SELECT
                COALESCE(SUM(CASE WHEN invoice_type='sale' THEN total_minor END),0),
                COALESCE(SUM(CASE WHEN invoice_type='sale_return' THEN total_minor END),0),
                COUNT(*)
             FROM sales_invoices
             WHERE company_id=?1 AND pos_machine_id=?2 AND status='posted' AND invoice_date=?3",
            params![cid, q.machine_id, q.date],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let expected = opened + sales - returns;
        Ok(serde_json::json!({
            "machine": machine, "date": q.date,
            "opened": common::money(opened),
            "delivered": common::money(delivered),
            "sales": common::money(sales),
            "returns": common::money(returns),
            "invoices": count,
            "expected": common::money(expected),
            "difference": common::money(delivered - expected),
        }))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}
