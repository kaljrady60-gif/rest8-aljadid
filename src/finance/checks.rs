//! إدارة الشيكات: وارد (من عميل) وصادر (لمورد)، مع دورة حياة كاملة
//! وترحيل محاسبي عند التحصيل عبر `gl::post_entry` (مصدر واحد للترحيل).
use super::audit;
use super::gl;
use super::money::{self, render};
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use rust_decimal::Decimal;
use serde::Deserialize;

/// (الاتجاه، المبلغ، العملة، الاستحقاق، الصندوق، الطرف، الرقم، القيد)
type CheckRow = (
    String,
    String,
    String,
    String,
    Option<i64>,
    Option<i64>,
    String,
    Option<i64>,
);

#[derive(Deserialize)]
pub struct ListQuery {
    company_id: i64,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewCheck {
    company_id: i64,
    direction: String,
    number: String,
    bank: String,
    drawer: String,
    amount: String,
    currency: String,
    due_date: String,
    #[serde(default)]
    fund_id: Option<i64>,
    #[serde(default)]
    party_id: Option<i64>,
    #[serde(default)]
    notes: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Move {
    company_id: i64,
    status: String,
    #[serde(default)]
    fund_id: Option<i64>,
}

const ALLOWED: [(&str, &[&str]); 2] = [
    (
        "received",
        &[
            "registered",
            "deposited",
            "collected",
            "bounced",
            "endorsed",
            "cancelled",
        ],
    ),
    (
        "issued",
        &[
            "registered",
            "deposited",
            "collected",
            "bounced",
            "cancelled",
        ],
    ),
];

fn validate(n: &NewCheck) -> Result<Decimal, FinanceError> {
    if n.direction != "received" && n.direction != "issued" {
        return Err(FinanceError::Invalid(
            "اتجاه الشيك يجب أن يكون واردًا أو صادرًا.".into(),
        ));
    }
    for (value, label, max) in [
        (&n.number, "رقم الشيك", 40usize),
        (&n.bank, "البنك", 80),
        (&n.drawer, "اسم الساحب", 120),
    ] {
        if value.trim().is_empty() || value.chars().count() > max {
            return Err(FinanceError::Invalid(format!(
                "{label} مطلوب وبحد {max} حرفًا."
            )));
        }
    }
    if !crate::generated::contracts::supported_currency(&n.currency) {
        return Err(FinanceError::Invalid("العملة غير مدعومة.".into()));
    }
    if chrono::NaiveDate::parse_from_str(&n.due_date, "%Y-%m-%d").is_err() {
        return Err(FinanceError::Invalid("تاريخ الاستحقاق غير صالح.".into()));
    }
    let amount = money::parse(&n.amount, "مبلغ الشيك")?;
    if amount <= Decimal::ZERO {
        return Err(FinanceError::Invalid(
            "مبلغ الشيك يجب أن يكون أكبر من صفر.".into(),
        ));
    }
    Ok(amount)
}

pub async fn list(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ListQuery>,
) -> Response {
    let result = (|| -> Result<Vec<serde_json::Value>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let filter = q.status.clone().unwrap_or_else(|| "all".into());
        if filter != "all"
            && ![
                "registered",
                "deposited",
                "collected",
                "bounced",
                "endorsed",
                "cancelled",
            ]
            .contains(&filter.as_str())
        {
            return Err(FinanceError::Invalid("حالة تصفية غير مدعومة.".into()));
        }
        let mut stmt = db.prepare(
            "SELECT c.id,c.direction,c.number,c.bank,c.drawer,c.amount,c.currency,c.due_date,
                    c.status,c.entry_id,c.notes,p.name,f.name
             FROM finance_checks c
             LEFT JOIN finance_parties p ON p.id=c.party_id
             LEFT JOIN finance_funds f ON f.id=c.fund_id
             WHERE c.company_id=?1 ORDER BY c.due_date, c.id LIMIT 400",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?,
                "direction": r.get::<_, String>(1)?,
                "number": r.get::<_, String>(2)?,
                "bank": r.get::<_, String>(3)?,
                "drawer": r.get::<_, String>(4)?,
                "amount": r.get::<_, String>(5)?,
                "currency": r.get::<_, String>(6)?,
                "due_date": r.get::<_, String>(7)?,
                "status": r.get::<_, String>(8)?,
                "entry_id": r.get::<_, Option<i64>>(9)?,
                "notes": r.get::<_, Option<String>>(10)?,
                "party": r.get::<_, Option<String>>(11)?,
                "fund": r.get::<_, Option<String>>(12)?
            }))
        })?;
        let mut items = Vec::new();
        for row in rows {
            let item = row?;
            if filter != "all" && item["status"] != filter.as_str() {
                continue;
            }
            items.push(item);
        }
        Ok(items)
    })();
    match result {
        Ok(items) => Json(items).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn create(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<NewCheck>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        let amount = validate(&n)?;
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        let branch: i64 = tx
            .query_row(
                "SELECT id FROM finance_branches WHERE company_id=?1 ORDER BY id LIMIT 1",
                [scope.company_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| FinanceError::Invalid("لا يوجد فرع للشركة.".into()))?;
        if let Some(fund) = n.fund_id {
            let owned: Option<i64> = tx
                .query_row(
                    "SELECT id FROM finance_funds WHERE id=?1 AND company_id=?2",
                    params![fund, scope.company_id],
                    |r| r.get(0),
                )
                .optional()?;
            if owned.is_none() {
                return Err(FinanceError::Invalid("الصندوق ليس من هذه الشركة.".into()));
            }
        }
        tx.execute(
            "INSERT INTO finance_checks
             (company_id,branch_id,fund_id,party_id,direction,number,bank,drawer,amount,currency,due_date,status,notes,created_by)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'registered',?12,?13)",
            params![
                scope.company_id,
                branch,
                n.fund_id,
                n.party_id,
                n.direction,
                n.number.trim(),
                n.bank.trim(),
                n.drawer.trim(),
                render(amount),
                n.currency,
                n.due_date,
                n.notes.as_deref(),
                actor.id
            ],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(_, _) => {
                FinanceError::Conflict("رقم الشيك مسجّل مسبقًا لنفس البنك والاتجاه.")
            }
            other => FinanceError::Storage(other),
        })?;
        let id = tx.last_insert_rowid();
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "check.create",
                entity: "finance_checks",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({
                    "direction": n.direction, "amount": render(amount), "due_date": n.due_date
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

/// ترحيل أثر التحصيل: وارد ⇒ مدين الصندوق/دائن طرف أو «شيكات برسم التحصيل»،
/// وصادر ⇒ مدين طرف/دائن الصندوق.
fn post_collection(
    tx: &rusqlite::Connection,
    actor_id: i64,
    company_id: i64,
    check: &(
        String,
        String,
        String,
        String,
        Option<i64>,
        Option<i64>,
        String,
    ),
) -> Result<i64, FinanceError> {
    let (direction, amount_raw, currency, due_date, fund_id, party_id, number) = check.clone();
    let amount = money::from_db(&amount_raw, "مبلغ الشيك")?;
    let ctx = gl::context(tx, company_id, &due_date)?;
    let fund_account: Option<i64> = match fund_id {
        Some(id) => tx
            .query_row(
                "SELECT account_id FROM finance_funds WHERE id=?1 AND company_id=?2",
                params![id, company_id],
                |r| r.get(0),
            )
            .optional()?,
        None => None,
    };
    let fund_account = fund_account
        .or(gl::account_by_code(tx, company_id, "1120")?)
        .or(gl::account_by_code(tx, company_id, "1110")?)
        .ok_or_else(|| FinanceError::Invalid("لا يوجد حساب نقدي أو بنكي للترحيل.".into()))?;
    let party_account: Option<i64> = match party_id {
        Some(id) => tx
            .query_row(
                "SELECT account_id FROM finance_parties WHERE id=?1 AND company_id=?2",
                params![id, company_id],
                |r| r.get(0),
            )
            .optional()?,
        None => None,
    };
    let counter = match party_account {
        Some(account) => account,
        None => gl::account_by_code(tx, company_id, "1130")?.unwrap_or(fund_account),
    };
    let lines = if direction == "received" {
        vec![
            (fund_account, amount, Decimal::ZERO),
            (counter, Decimal::ZERO, amount),
        ]
    } else {
        vec![
            (counter, amount, Decimal::ZERO),
            (fund_account, Decimal::ZERO, amount),
        ]
    };
    let description = format!(
        "تحصيل شيك {} رقم {}",
        if direction == "received" {
            "وارد"
        } else {
            "صادر"
        },
        number
    );
    gl::post_entry(
        tx,
        ctx,
        &due_date,
        &description,
        &currency,
        &lines,
        Some(actor_id),
        "check.collected",
    )
}

pub async fn move_status(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Path(id): Path<i64>,
    Json(m): Json<Move>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, m.company_id)?;
        let row: Option<CheckRow> = tx
            .query_row(
                "SELECT direction,amount,currency,due_date,fund_id,party_id,number,entry_id
                 FROM finance_checks WHERE id=?1 AND company_id=?2",
                params![id, scope.company_id],
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
                    ))
                },
            )
            .optional()?;
        let (direction, amount, currency, due_date, fund_id, party_id, number, entry_id) =
            row.ok_or(FinanceError::NotFound("الشيك"))?;
        let allowed = ALLOWED
            .iter()
            .find(|(dir, _)| *dir == direction)
            .map(|(_, states)| *states)
            .unwrap_or(&[]);
        if !allowed.contains(&m.status.as_str()) {
            return Err(FinanceError::Invalid("حالة غير مدعومة لهذا الاتجاه.".into()));
        }
        let entry = if m.status == "collected" {
            if entry_id.is_some() {
                return Err(FinanceError::Conflict("الشيك محصّل مسبقًا."));
            }
            let fund = m.fund_id.or(fund_id);
            let posted = post_collection(
                &tx,
                actor.id,
                scope.company_id,
                &(
                    direction, amount, currency, due_date, fund, party_id, number,
                ),
            )?;
            Some(posted)
        } else {
            None
        };
        tx.execute(
            "UPDATE finance_checks SET status=?1, fund_id=COALESCE(?2, fund_id), entry_id=COALESCE(?3, entry_id)
             WHERE id=?4",
            params![m.status, m.fund_id, entry, id],
        )?;
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "check.status",
                entity: "finance_checks",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({"status": m.status, "entry_id": entry})),
            },
        )?;
        tx.commit()?;
        Ok(serde_json::json!({"id": id, "status": m.status, "entry_id": entry}))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}
