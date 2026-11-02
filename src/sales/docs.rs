//! عروض الأسعار وطلبات العملاء: مستندان بلا أثر محاسبي حتى التحويل إلى فاتورة.
use super::common;
use super::invoices::{LineIn, NewInvoice};
use crate::AppState;
use crate::db_open;
use crate::finance::audit;
use crate::finance::scope::{FinanceError, Scope, error_response};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct ListQuery {
    pub company_id: i64,
    /// quote | order
    pub kind: String,
}

#[derive(Deserialize)]
pub struct DocLine {
    pub item_name: String,
    pub qty: String,
    pub price: String,
    #[serde(default)]
    pub discount: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewDoc {
    pub company_id: i64,
    pub kind: String,
    pub party_id: i64,
    pub date: String,
    #[serde(default)]
    pub valid_until: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub rep_id: Option<i64>,
    #[serde(default)]
    pub note: Option<String>,
    pub lines: Vec<DocLine>,
}

fn table(kind: &str) -> Result<(&'static str, &'static str, &'static str), FinanceError> {
    match kind {
        "quote" => Ok(("sales_quotes", "sales_quote_lines", "QT")),
        "order" => Ok(("sales_orders", "sales_order_lines", "OR")),
        _ => Err(FinanceError::Invalid("نوع مستند غير مدعوم.".into())),
    }
}

pub async fn list(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ListQuery>,
) -> Response {
    let result = (|| -> Result<Vec<serde_json::Value>, FinanceError> {
        let (head, _lines, _prefix) = table(&q.kind)?;
        let date_col = if q.kind == "quote" { "quote_date" } else { "order_date" };
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let sql = format!(
            "SELECT d.id,d.number,d.{date_col},p.name,d.currency,d.total_minor,d.status,
                    COALESCE(d.note,''),COALESCE(d.invoice_id,0)
             FROM {head} d JOIN finance_parties p ON p.id=d.party_id
             WHERE d.company_id=?1 ORDER BY d.id DESC LIMIT 200"
        );
        let mut stmt = db.prepare(&sql)?;
        let items = stmt
            .query_map([scope.company_id], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "number": r.get::<_, String>(1)?,
                    "date": r.get::<_, String>(2)?,
                    "party": r.get::<_, String>(3)?,
                    "currency": r.get::<_, String>(4)?,
                    "total": common::money(r.get::<_, i64>(5)?),
                    "status": r.get::<_, String>(6)?,
                    "note": r.get::<_, String>(7)?,
                    "invoice_id": r.get::<_, i64>(8)?,
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(items)
    })();
    match result {
        Ok(items) => Json(serde_json::json!({"items": items})).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn create(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(input): Json<NewDoc>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        let (head, lines_table, prefix) = table(&input.kind)?;
        if input.lines.is_empty() {
            return Err(FinanceError::Invalid("المستند يحتاج سطرًا واحدًا على الأقل.".into()));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, input.company_id)?;
        let (_account, party_currency) =
            common::party_account(&tx, scope.company_id, input.party_id)?;
        let currency = input
            .currency
            .clone()
            .filter(|c| !c.trim().is_empty())
            .unwrap_or(party_currency);
        let number = common::next_number(&tx, scope.company_id, prefix, &input.date)?;
        let mut total = 0_i64;
        let mut prepared = Vec::new();
        for line in &input.lines {
            let qty = common::parse_qty(&line.qty, "السطر")?;
            let price = common::parse_money(&line.price, "السعر")?;
            let discount = match line.discount.as_deref() {
                Some(raw) if !raw.trim().is_empty() => common::parse_money(raw, "الخصم")?,
                _ => 0,
            };
            let net = common::line_net(qty, price, discount);
            total += net;
            prepared.push((line.item_name.trim().to_string(), qty, price, discount, net));
        }
        if input.kind == "quote" {
            tx.execute(
                "INSERT INTO sales_quotes(company_id,number,quote_date,valid_until,party_id,currency,total_minor,status,note)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,'draft',?8)",
                params![scope.company_id, number, input.date, input.valid_until, input.party_id, currency, total, input.note],
            )?;
        } else {
            tx.execute(
                "INSERT INTO sales_orders(company_id,number,order_date,party_id,rep_id,currency,total_minor,status,note)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,'draft',?8)",
                params![scope.company_id, number, input.date, input.party_id, input.rep_id, currency, total, input.note],
            )?;
        }
        let doc_id = tx.last_insert_rowid();
        for (index, (name, qty, price, discount, net)) in prepared.iter().enumerate() {
            let sql = format!(
                "INSERT INTO {lines_table}({}_id,line_no,item_name,qty_milli,price_minor,discount_minor,net_minor)
                 VALUES(?1,?2,?3,?4,?5,?6,?7)",
                if input.kind == "quote" { "quote" } else { "order" }
            );
            tx.execute(&sql, params![doc_id, index as i64, name, qty, price, discount, net])?;
        }
        audit::record(&tx, audit::Entry {
            company_id: Some(scope.company_id), actor_id: Some(actor.id),
            action: "sales.doc.create", entity: head, entity_id: Some(doc_id.to_string()),
            payload: Some(serde_json::json!({"kind": input.kind, "total": common::money(total)})),
        })?;
        tx.commit()?;
        Ok(doc_id)
    })();
    match result {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response(),
        Err(e) => error_response(e),
    }
}

#[derive(Deserialize)]
pub struct ConvertBody {
    pub company_id: i64,
}

/// تحويل عرض/طلب إلى فاتورة مسودة بنفس السطور والمبالغ — في معاملة واحدة.
pub async fn convert(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Path((kind, id)): Path<(String, i64)>,
    Json(body): Json<ConvertBody>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        let (head, lines_table, _) = table(&kind)?;
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, body.company_id)?;
        let row: Option<(String, i64, String, Option<String>, i64)> = tx
            .query_row(
                &format!(
                    "SELECT status,party_id,currency,{},COALESCE(invoice_id,0) FROM {head}
                     WHERE company_id=?1 AND id=?2",
                    if kind == "quote" { "valid_until" } else { "NULL" }
                ),
                params![scope.company_id, id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?;
        let (status, party_id, currency, valid_until, invoice_id) =
            row.ok_or(FinanceError::NotFound("المستند"))?;
        if invoice_id > 0 {
            return Err(FinanceError::Conflict("المستند محوّل مسبقًا.".into()));
        }
        if status == "cancelled" {
            return Err(FinanceError::Conflict("المستند ملغى.".into()));
        }
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        if let Some(until) = valid_until
            && !until.trim().is_empty()
            && until.as_str() < today.as_str()
        {
            return Err(FinanceError::Invalid("انتهت صلاحية عرض السعر.".into()));
        }
        let mut stmt = tx.prepare(&format!(
            "SELECT item_name,qty_milli,price_minor,discount_minor FROM {lines_table}
             WHERE {}_id=?1 ORDER BY line_no",
            if kind == "quote" { "quote" } else { "order" }
        ))?;
        let lines = stmt
            .query_map([id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        if lines.is_empty() {
            return Err(FinanceError::Invalid("المستند بلا سطور.".into()));
        }
        let invoice_lines: Vec<LineIn> = lines
            .into_iter()
            .map(|(name, qty, price, discount)| LineIn {
                item_name: name,
                qty: common::qty_text(qty),
                price: common::money(price),
                discount: Some(common::money(discount)),
            })
            .collect();
        let input = NewInvoice {
            company_id: scope.company_id,
            party_id,
            invoice_date: today,
            currency: Some(currency),
            description: None,
            tax_bp: Some(0),
            discount: None,
            rep_id: None,
            pos_machine_id: None,
            note: Some(format!("محوّل من {kind} #{id}")),
            lines: invoice_lines,
        };
        let invoice = super::invoices::insert_invoice(&tx, scope, actor.id, "sale", &input, None)?;
        let new_status = if kind == "quote" { "converted" } else { "invoiced" };
        tx.execute(
            &format!("UPDATE {head} SET status=?1, invoice_id=?2 WHERE id=?3"),
            params![new_status, invoice, id],
        )?;
        tx.commit()?;
        Ok(invoice)
    })();
    match result {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({"invoice_id": id}))).into_response(),
        Err(e) => error_response(e),
    }
}
