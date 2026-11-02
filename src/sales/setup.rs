//! لقطة تهيئة واحدة لشاشة المبيعات: العملاء + الكيانات + المؤشرات + الترقيم القادم.
use super::common;
use crate::AppState;
use crate::db_open;
use crate::finance::scope::{FinanceError, Scope, error_response};
use axum::Json;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
pub struct SetupQuery {
    pub company_id: i64,
}

pub async fn get(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<SetupQuery>,
) -> Response {
    let result = (|| -> Result<Value, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let cid = scope.company_id;
        let mut stmt = db.prepare(
            "SELECT p.id,p.name,COALESCE(a.currency,'YER')
             FROM finance_parties p LEFT JOIN accounts a ON a.id=p.account_id
             WHERE p.company_id=?1 AND p.role IN ('customer','both') ORDER BY p.name",
        )?;
        let parties: Vec<Value> = stmt
            .query_map([cid], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "currency": r.get::<_, String>(2)?,
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let (invoices, posted, drafts, returns, quotes, orders): (i64, i64, i64, i64, i64, i64) =
            db.query_row(
                "SELECT
                   (SELECT COUNT(*) FROM sales_invoices WHERE company_id=?1),
                   (SELECT COUNT(*) FROM sales_invoices WHERE company_id=?1 AND status='posted'),
                   (SELECT COUNT(*) FROM sales_invoices WHERE company_id=?1 AND status='draft'),
                   (SELECT COUNT(*) FROM sales_invoices WHERE company_id=?1 AND invoice_type='sale_return'),
                   (SELECT COUNT(*) FROM sales_quotes WHERE company_id=?1),
                   (SELECT COUNT(*) FROM sales_orders WHERE company_id=?1)",
                [cid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )?;
        let sales_total: i64 = db.query_row(
            "SELECT COALESCE(SUM(CASE WHEN invoice_type='sale' THEN total_minor ELSE -total_minor END),0)
             FROM sales_invoices WHERE company_id=?1 AND status='posted'",
            [cid],
            |r| r.get(0),
        )?;
        let receivable: i64 = db.query_row(
            "SELECT COALESCE(SUM(total_minor - paid_minor),0) FROM sales_invoices
             WHERE company_id=?1 AND status='posted'",
            [cid],
            |r| r.get(0),
        )?;
        // حسابات الترحيل الفعلية: تُعرض للمالك ليعرف أين يُقيَّد البيع.
        let revenue = common::ensure_revenue_account(&db, cid).ok();
        let tax = common::tax_account(&db, cid);
        Ok(serde_json::json!({
            "company_id": cid,
            "parties": parties,
            "stats": {
                "invoices": invoices, "posted": posted, "drafts": drafts,
                "returns": returns, "quotes": quotes, "orders": orders,
                "sales_total": common::money(sales_total),
                "receivable": common::money(receivable),
            },
            "accounts": {
                "revenue": revenue.map(|id| common::account_label(&db, cid, id)),
                "tax": tax.map(|id| common::account_label(&db, cid, id)),
            }
        }))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}
