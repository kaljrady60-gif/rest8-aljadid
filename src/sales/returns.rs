//! مرتجعات البيع: لا تُنشأ من الصفر بل من فاتورة مرحّلة، والكمية المرتجعة تراكميًا لا تتجاوز الأصل.
use super::common;
use super::invoices::{self, LineIn, NewInvoice};
use crate::AppState;
use crate::finance::scope::{FinanceError, Scope, error_response};
use crate::finance::audit;
use crate::db_open;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct ReturnLine {
    pub line_id: i64,
    pub qty: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewReturn {
    pub company_id: i64,
    pub invoice_id: i64,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub post_now: Option<bool>,
    /// سطور مختارة؛ غيابها يعني إرجاع كل الفاتورة.
    #[serde(default)]
    pub lines: Vec<ReturnLine>,
}

pub async fn create(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(input): Json<NewReturn>,
) -> Response {
    let result = (|| -> Result<(i64, Option<i64>), FinanceError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, input.company_id)?;
        if !scope.is_admin && !crate::identity::is_current_admin(&tx, &actor)? {
            return Err(FinanceError::Forbidden);
        }
        let original: Option<(String, i64, String, String)> = tx
            .query_row(
                "SELECT status,party_id,invoice_date,currency FROM sales_invoices
                 WHERE company_id=?1 AND id=?2",
                params![scope.company_id, input.invoice_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let (status, party_id, _date, currency) = original.ok_or(FinanceError::NotFound("الفاتورة"))?;
        if status != "posted" {
            return Err(FinanceError::Conflict(
                "المرتجع يُنشأ من فاتورة مرحّلة فقط.".into(),
            ));
        }
        let mut stmt = tx.prepare(
            "SELECT l.id,l.item_name,l.qty_milli,l.price_minor,l.discount_minor
             FROM sales_invoice_lines l WHERE l.invoice_id=?1 ORDER BY l.line_no",
        )?;
        let originals = stmt
            .query_map([input.invoice_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        if originals.is_empty() {
            return Err(FinanceError::Invalid("الفاتورة بلا سطور.".into()));
        }
        // ما سبق إرجاعه لكل سطر من فواتير المرتجع السابقة.
        let returned: Vec<(i64, i64)> = {
            let mut stmt = tx.prepare(
                "SELECT o.line_no, COALESCE(SUM(r.qty_milli),0)
                 FROM sales_invoices ri
                 JOIN sales_invoice_lines r ON r.invoice_id=ri.id
                 JOIN sales_invoice_lines o ON o.invoice_id=ri.original_invoice_id
                 WHERE ri.original_invoice_id=?1 AND ri.status='posted' AND o.line_no=r.line_no
                 GROUP BY o.line_no",
            )?;
            stmt.query_map([input.invoice_id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut lines = Vec::new();
        for (index, (line_id, name, qty, price, discount)) in originals.iter().enumerate() {
            let already = returned
                .iter()
                .find(|(no, _)| *no == index as i64)
                .map(|(_, q)| *q)
                .unwrap_or(0);
            let allowed = qty - already;
            let requested = match input.lines.iter().find(|l| l.line_id == *line_id) {
                Some(sel) => match sel.qty.as_deref() {
                    Some(raw) if !raw.trim().is_empty() => common::parse_qty(raw, "المرتجع")?,
                    _ => allowed,
                },
                None => {
                    if input.lines.is_empty() {
                        allowed
                    } else {
                        0
                    }
                }
            };
            if requested > allowed {
                return Err(FinanceError::Invalid(format!(
                    "الكمية المرتجعة من «{name}» تتجاوز المتاح ({})",
                    common::qty_text(allowed.max(0))
                )));
            }
            if requested <= 0 {
                continue;
            }
            lines.push(LineIn {
                item_name: name.clone(),
                qty: common::qty_text(requested),
                price: common::money(*price),
                discount: Some(common::money(*discount)),
            });
        }
        if lines.is_empty() {
            return Err(FinanceError::Invalid("لا توجد كميات صالحة للإرجاع.".into()));
        }
        let date = input
            .date
            .clone()
            .filter(|d| d.len() == 10)
            .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());
        let draft = NewInvoice {
            company_id: scope.company_id,
            party_id,
            invoice_date: date,
            currency: Some(currency),
            description: input.reason.clone(),
            tax_bp: Some(0),
            discount: None,
            rep_id: None,
            pos_machine_id: None,
            note: input.reason.clone(),
            lines,
        };
        let id = invoices::insert_invoice(&tx, scope, actor.id, "sale_return", &draft, Some(input.invoice_id))?;
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "sales.return.create",
                entity: "sales_invoices",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({"original": input.invoice_id})),
            },
        )?;
        tx.commit()?;
        let entry = if input.post_now.unwrap_or(true) {
            Some(invoices::post_invoice(&state, &actor, scope.company_id, id)?)
        } else {
            None
        };
        Ok((id, entry))
    })();
    match result {
        Ok((id, entry)) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"id": id, "journal_entry_id": entry})),
        )
            .into_response(),
        Err(e) => error_response(e),
    }
}
