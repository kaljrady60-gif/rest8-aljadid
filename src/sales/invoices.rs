//! فواتير البيع: مسودة ← ترحيل ذرّي عبر محرّك القيود نفسه ← قفل نهائي.
use super::common;
use crate::AppState;
use crate::finance::journals::{self, NewJournal};
use crate::finance::scope::{FinanceError, Scope, error_response};
use crate::finance::{audit, narrative};
use crate::domain::LineInput;
use crate::db_open;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct ListQuery {
    pub company_id: i64,
    pub status: Option<String>,
    pub invoice_type: Option<String>,
}

#[derive(Deserialize)]
pub struct LineIn {
    pub item_name: String,
    pub qty: String,
    pub price: String,
    #[serde(default)]
    pub discount: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewInvoice {
    pub company_id: i64,
    pub party_id: i64,
    pub invoice_date: String,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tax_bp: Option<i64>,
    #[serde(default)]
    pub discount: Option<String>,
    #[serde(default)]
    pub rep_id: Option<i64>,
    #[serde(default)]
    pub pos_machine_id: Option<i64>,
    #[serde(default)]
    pub note: Option<String>,
    pub lines: Vec<LineIn>,
}

pub async fn list(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ListQuery>,
) -> Response {
    let result = (|| -> Result<Vec<serde_json::Value>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let status = q.status.clone().unwrap_or_else(|| "all".into());
        if !["all", "draft", "posted", "cancelled"].contains(&status.as_str()) {
            return Err(FinanceError::Invalid("حالة تصفية غير مدعومة.".into()));
        }
        let kind = q.invoice_type.clone().unwrap_or_else(|| "all".into());
        if !["all", "sale", "sale_return"].contains(&kind.as_str()) {
            return Err(FinanceError::Invalid("نوع فاتورة غير مدعوم.".into()));
        }
        let mut stmt = db.prepare(
            "SELECT i.id,i.number,i.invoice_type,i.invoice_date,p.name,i.currency,i.status,
                    i.total_minor,i.paid_minor,i.tax_minor,i.journal_entry_id,i.original_invoice_id,
                    COALESCE(r.name,''), COALESCE(m.name,'')
             FROM sales_invoices i
             JOIN finance_parties p ON p.id=i.party_id
             LEFT JOIN sales_reps r ON r.id=i.rep_id
             LEFT JOIN sales_pos_machines m ON m.id=i.pos_machine_id
             WHERE i.company_id=?1 ORDER BY i.invoice_date DESC, i.id DESC LIMIT 300",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?,
                "number": r.get::<_, String>(1)?,
                "invoice_type": r.get::<_, String>(2)?,
                "date": r.get::<_, String>(3)?,
                "party": r.get::<_, String>(4)?,
                "currency": r.get::<_, String>(5)?,
                "status": r.get::<_, String>(6)?,
                "total": common::money(r.get::<_, i64>(7)?),
                "paid": common::money(r.get::<_, i64>(8)?),
                "tax": common::money(r.get::<_, i64>(9)?),
                "journal_entry_id": r.get::<_, Option<i64>>(10)?,
                "original_invoice_id": r.get::<_, Option<i64>>(11)?,
                "rep": r.get::<_, String>(12)?,
                "machine": r.get::<_, String>(13)?,
            }))
        })?;
        let mut items = Vec::new();
        for row in rows {
            let item = row?;
            let keep_status = status == "all" || item["status"] == status.as_str();
            let keep_kind = kind == "all" || item["invoice_type"] == kind.as_str();
            if keep_status && keep_kind {
                items.push(item);
            }
        }
        Ok(items)
    })();
    match result {
        Ok(items) => Json(serde_json::json!({"items": items})).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn detail(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Path(id): Path<i64>,
    Query(q): Query<CompanyQuery>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let header: Option<serde_json::Value> = db
            .query_row(
                "SELECT i.id,i.number,i.invoice_type,i.invoice_date,p.name,i.currency,i.status,
                        i.subtotal_minor,i.discount_minor,i.tax_minor,i.total_minor,i.paid_minor,
                        i.description,i.note,i.journal_entry_id,i.original_invoice_id,i.draft_id
                 FROM sales_invoices i JOIN finance_parties p ON p.id=i.party_id
                 WHERE i.company_id=?1 AND i.id=?2",
                params![scope.company_id, id],
                |r| {
                    Ok(serde_json::json!({
                        "id": r.get::<_, i64>(0)?,
                        "number": r.get::<_, String>(1)?,
                        "invoice_type": r.get::<_, String>(2)?,
                        "date": r.get::<_, String>(3)?,
                        "party": r.get::<_, String>(4)?,
                        "currency": r.get::<_, String>(5)?,
                        "status": r.get::<_, String>(6)?,
                        "subtotal": common::money(r.get::<_, i64>(7)?),
                        "discount": common::money(r.get::<_, i64>(8)?),
                        "tax": common::money(r.get::<_, i64>(9)?),
                        "total": common::money(r.get::<_, i64>(10)?),
                        "paid": common::money(r.get::<_, i64>(11)?),
                        "description": r.get::<_, String>(12)?,
                        "note": r.get::<_, Option<String>>(13)?,
                        "journal_entry_id": r.get::<_, Option<i64>>(14)?,
                        "original_invoice_id": r.get::<_, Option<i64>>(15)?,
                        "draft_id": r.get::<_, Option<i64>>(16)?,
                    }))
                },
            )
            .optional()?;
        let mut header = header.ok_or(FinanceError::NotFound("الفاتورة"))?;
        let mut stmt = db.prepare(
            "SELECT id,line_no,item_name,qty_milli,price_minor,discount_minor,net_minor
             FROM sales_invoice_lines WHERE invoice_id=?1 ORDER BY line_no",
        )?;
        let lines = stmt
            .query_map([id], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "line_no": r.get::<_, i64>(1)?,
                    "item_name": r.get::<_, String>(2)?,
                    "qty": common::qty_text(r.get::<_, i64>(3)?),
                    "price": common::money(r.get::<_, i64>(4)?),
                    "discount": common::money(r.get::<_, i64>(5)?),
                    "net": common::money(r.get::<_, i64>(6)?),
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        header["lines"] = serde_json::Value::Array(lines);
        Ok(header)
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

#[derive(Deserialize)]
pub struct CompanyQuery {
    pub company_id: i64,
}

/// يحسب مجاميع الفاتورة من سطورها — الحساب على الخادم لا في المتصفح.
fn totals(
    lines: &[LineIn],
    doc_discount: i64,
    tax_bp: i64,
) -> Result<(Vec<(String, i64, i64, i64, i64)>, i64, i64, i64, i64), FinanceError> {
    if lines.is_empty() {
        return Err(FinanceError::Invalid("الفاتورة تحتاج سطرًا واحدًا على الأقل.".into()));
    }
    let mut parsed = Vec::new();
    let mut subtotal = 0_i64;
    for line in lines {
        let qty = common::parse_qty(&line.qty, "السطر")?;
        let price = common::parse_money(&line.price, "السعر")?;
        let discount = match line.discount.as_deref() {
            Some(raw) if !raw.trim().is_empty() => common::parse_money(raw, "خصم السطر")?,
            _ => 0,
        };
        let net = common::line_net(qty, price, discount);
        subtotal += net;
        parsed.push((line.item_name.trim().to_string(), qty, price, discount, net));
    }
    if doc_discount > subtotal {
        return Err(FinanceError::Invalid("خصم الفاتورة أكبر من الإجمالي.".into()));
    }
    let base = subtotal - doc_discount;
    let tax = base * tax_bp / 10_000;
    Ok((parsed, subtotal, doc_discount, tax, base + tax))
}

pub fn insert_invoice(
    tx: &rusqlite::Connection,
    scope: Scope,
    actor_id: i64,
    invoice_type: &str,
    input: &NewInvoice,
    original: Option<i64>,
) -> Result<i64, FinanceError> {
    let (account_id, currency) = common::party_account(tx, scope.company_id, input.party_id)?;
    let _ = account_id;
    let currency = input
        .currency
        .clone()
        .filter(|c| !c.trim().is_empty())
        .unwrap_or(currency);
    let doc_discount = match input.discount.as_deref() {
        Some(raw) if !raw.trim().is_empty() => common::parse_money(raw, "خصم الفاتورة")?,
        _ => 0,
    };
    let tax_bp = input.tax_bp.unwrap_or(0);
    if !(0..=10_000).contains(&tax_bp) {
        return Err(FinanceError::Invalid("نسبة الضريبة خارج المدى.".into()));
    }
    let (lines, subtotal, discount, tax, total) = totals(&input.lines, doc_discount, tax_bp)?;
    let prefix = if invoice_type == "sale_return" { "RT" } else { "INV" };
    let number = common::next_number(tx, scope.company_id, prefix, &input.invoice_date)?;
    let party_name: String = tx.query_row(
        "SELECT name FROM finance_parties WHERE id=?1",
        [input.party_id],
        |r| r.get(0),
    )?;
    let label = if invoice_type == "sale_return" { "مرتجع" } else { "بيع" };
    let description = input
        .description
        .clone()
        .filter(|d| !d.trim().is_empty())
        .unwrap_or_else(|| narrative::compose(label, &number, &party_name));
    tx.execute(
        "INSERT INTO sales_invoices(company_id,number,invoice_type,invoice_date,party_id,currency,
            status,description,subtotal_minor,discount_minor,tax_minor,total_minor,
            original_invoice_id,rep_id,pos_machine_id,note,created_by)
         VALUES(?1,?2,?3,?4,?5,?6,'draft',?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        params![
            scope.company_id,
            number,
            invoice_type,
            input.invoice_date,
            input.party_id,
            currency,
            description,
            subtotal,
            discount,
            tax,
            total,
            original,
            input.rep_id,
            input.pos_machine_id,
            input.note,
            actor_id
        ],
    )?;
    let invoice_id = tx.last_insert_rowid();
    for (index, (name, qty, price, disc, net)) in lines.iter().enumerate() {
        tx.execute(
            "INSERT INTO sales_invoice_lines(invoice_id,line_no,item_name,qty_milli,price_minor,discount_minor,net_minor)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![invoice_id, index as i64, name, qty, price, disc, net],
        )?;
    }
    Ok(invoice_id)
}

pub async fn create(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(input): Json<NewInvoice>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, input.company_id)?;
        scope.require_admin().or_else(|_| {
            if crate::identity::is_current_admin(&tx, &actor)? {
                Ok(())
            } else {
                Err(FinanceError::Forbidden)
            }
        })?;
        let id = insert_invoice(&tx, scope, actor.id, "sale", &input, None)?;
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "sales.invoice.create",
                entity: "sales_invoices",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({"party_id": input.party_id})),
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

/// يبني قيد الفاتورة ويمرّره على محرّك القيود القائم (لا قناة ترحيل موازية).
pub fn post_invoice(
    state: &AppState,
    actor: &crate::identity::Principal,
    company_id: i64,
    invoice_id: i64,
) -> Result<i64, FinanceError> {
    let mut db = db_open(&state.db)?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let _scope = Scope::resolve(state, &tx, actor, company_id)?;
    let row: Option<(String, String, i64, i64, i64, i64, String, i64, Option<i64>)> = tx
        .query_row(
            "SELECT status,number,total_minor,tax_minor,subtotal_minor,discount_minor,invoice_date,
                    party_id,original_invoice_id
             FROM sales_invoices WHERE company_id=?1 AND id=?2",
            params![company_id, invoice_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                ))
            },
        )
        .optional()?;
    let (status, number, total, tax, subtotal, discount, date, party_id, original) =
        row.ok_or(FinanceError::NotFound("الفاتورة"))?;
    // حجز الفاتورة قبل أي عمل خارجي: يمنع الترحيل المزدوج عند نقرتين متزامنتين.
    let claimed = tx.execute(
        "UPDATE sales_invoices SET draft_id=-1
         WHERE company_id=?1 AND id=?2 AND status='draft' AND COALESCE(draft_id,0)=0",
        params![company_id, invoice_id],
    )?;
    if claimed != 1 {
        return Err(FinanceError::Conflict(
            "الفاتورة قيد الترحيل الآن؛ أعد المحاولة بعد لحظات.".into(),
        ));
    }
    match status.as_str() {
        "posted" => return Err(FinanceError::Conflict("الفاتورة مرحّلة مسبقًا.")),
        "cancelled" => return Err(FinanceError::Conflict("الفاتورة ملغاة.")),
        _ => {}
    }
    let (party_account, currency) = common::party_account(&tx, company_id, party_id)?;
    let partner: String = tx.query_row(
        "SELECT name FROM finance_parties WHERE id=?1",
        [party_id],
        |r| r.get(0),
    )?;
    let is_return = original.is_some();
    let revenue = common::ensure_revenue_account(&tx, company_id)?;
    let base = (subtotal - discount).max(0);
    let mut lines = Vec::new();
    if is_return {
        // مرتجع: نعكس الاتجاه — مدين إيرادات (و ضريبة)، دائن ذمم العميل.
        lines.push(LineInput {
            account: common::account_label(&tx, company_id, revenue),
            account_id: Some(revenue),
            debit: common::money(base),
            credit: "0".into(),
        });
        if tax > 0
            && let Some(tax_acc) = common::tax_account(&tx, company_id)
        {
            lines.push(LineInput {
                account: common::account_label(&tx, company_id, tax_acc),
                account_id: Some(tax_acc),
                debit: common::money(tax),
                credit: "0".into(),
            });
        }
        lines.push(LineInput {
            account: common::account_label(&tx, company_id, party_account),
            account_id: Some(party_account),
            debit: "0".into(),
            credit: common::money(total),
        });
    } else {
        lines.push(LineInput {
            account: common::account_label(&tx, company_id, party_account),
            account_id: Some(party_account),
            debit: common::money(total),
            credit: "0".into(),
        });
        lines.push(LineInput {
            account: common::account_label(&tx, company_id, revenue),
            account_id: Some(revenue),
            debit: "0".into(),
            credit: common::money(base),
        });
        if tax > 0
            && let Some(tax_acc) = common::tax_account(&tx, company_id)
        {
            lines.push(LineInput {
                account: common::account_label(&tx, company_id, tax_acc),
                account_id: Some(tax_acc),
                debit: "0".into(),
                credit: common::money(tax),
            });
        }
    }
    let label = if is_return { "مرتجع" } else { "بيع" };
    let description = narrative::compose(label, &number, &partner);
    // ودائع القيد تُنشأ بأسماء الحسابات كما جاءت من الدليل، والتحقق الموحّد يُطبَّق في create_draft.
    if !crate::domain::supported_currency(&currency) {
        return Err(FinanceError::Invalid("عملة غير مدعومة للترحيل.".into()));
    }
    tx.commit()?;
    let draft_id = journals::create_draft(
        state,
        actor,
        &NewJournal {
            company_id,
            description,
            date,
            currency,
            lines,
        },
    )?;
    let entry_id = journals::post_draft(state, actor, company_id, draft_id, &[], None)?;
    let mut db = db_open(&state.db)?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(
        "UPDATE sales_invoices SET status='posted', journal_entry_id=?1, draft_id=?2
         WHERE company_id=?3 AND id=?4",
        params![entry_id, draft_id, company_id, invoice_id],
    )?;
    audit::record(
        &tx,
        audit::Entry {
            company_id: Some(company_id),
            actor_id: Some(actor.id),
            action: "sales.invoice.post",
            entity: "sales_invoices",
            entity_id: Some(invoice_id.to_string()),
            payload: Some(serde_json::json!({"journal_entry_id": entry_id})),
        },
    )?;
    tx.commit()?;
    Ok(entry_id)
}

pub async fn post(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Path(id): Path<i64>,
    Json(body): Json<CompanyQuery>,
) -> Response {
    match post_invoice(&state, &actor, body.company_id, id) {
        Ok(entry) => Json(serde_json::json!({"journal_entry_id": entry})).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn cancel(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Path(id): Path<i64>,
    Json(body): Json<CompanyQuery>,
) -> Response {
    let result = (|| -> Result<(), FinanceError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, body.company_id)?;
        let status: Option<String> = tx
            .query_row(
                "SELECT status FROM sales_invoices WHERE company_id=?1 AND id=?2",
                params![scope.company_id, id],
                |r| r.get(0),
            )
            .optional()?;
        match status.as_deref() {
            Some("draft") => {}
            Some("posted") => {
                return Err(FinanceError::Conflict(
                    "الفاتورة مرحّلة؛ أنشئ مرتجعًا بدل الإلغاء.".into(),
                ));
            }
            Some("cancelled") => return Err(FinanceError::Conflict("الفاتورة ملغاة مسبقًا.")),
            _ => return Err(FinanceError::NotFound("الفاتورة")),
        }
        tx.execute(
            "UPDATE sales_invoices SET status='cancelled' WHERE id=?1",
            [id],
        )?;
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "sales.invoice.cancel",
                entity: "sales_invoices",
                entity_id: Some(id.to_string()),
                payload: None,
            },
        )?;
        tx.commit()?;
        Ok(())
    })();
    match result {
        Ok(()) => Json(serde_json::json!({"status": "cancelled"})).into_response(),
        Err(e) => error_response(e),
    }
}
