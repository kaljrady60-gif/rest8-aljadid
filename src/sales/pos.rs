//! عهود الصندوق لأجهزة POS: فتح وردية، تسليم، إقفال — سجل نقدي مستقل عن القيود.
use super::common;
use crate::AppState;
use crate::db_open;
use crate::finance::scope::{FinanceError, Scope, error_response};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{TransactionBehavior, params};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
pub struct CustodyQuery {
    pub company_id: i64,
    #[serde(default)]
    pub machine_id: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewCustody {
    pub company_id: i64,
    pub machine_id: i64,
    pub cashier: String,
    pub date: String,
    pub amount: String,
    /// open | handover | close
    pub kind: String,
    #[serde(default)]
    pub note: Option<String>,
}

pub async fn custody_list(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<CustodyQuery>,
) -> Response {
    let result = (|| -> Result<Value, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT c.id,m.name,c.cashier,c.custody_date,c.amount_minor,c.kind,COALESCE(c.note,'')
             FROM sales_pos_custody c JOIN sales_pos_machines m ON m.id=c.machine_id
             WHERE c.company_id=?1 AND (?2 IS NULL OR c.machine_id=?2)
             ORDER BY c.custody_date DESC, c.id DESC LIMIT 200",
        )?;
        let items: Vec<Value> = stmt
            .query_map(params![scope.company_id, q.machine_id], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "machine": r.get::<_, String>(1)?,
                    "cashier": r.get::<_, String>(2)?,
                    "date": r.get::<_, String>(3)?,
                    "amount": common::money(r.get::<_, i64>(4)?),
                    "kind": r.get::<_, String>(5)?,
                    "note": r.get::<_, String>(6)?,
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(serde_json::json!({"items": items}))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn custody_create(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(input): Json<NewCustody>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        if !["open", "handover", "close"].contains(&input.kind.as_str()) {
            return Err(FinanceError::Invalid("نوع حركة عهدة غير مدعوم.".into()));
        }
        if input.cashier.trim().is_empty() {
            return Err(FinanceError::Invalid("اسم الكاشير مطلوب.".into()));
        }
        let amount = common::parse_money(&input.amount, "المبلغ")?;
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, input.company_id)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sales_pos_machines WHERE company_id=?1 AND id=?2)",
            params![scope.company_id, input.machine_id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(FinanceError::NotFound("الجهاز"));
        }
        tx.execute(
            "INSERT INTO sales_pos_custody(company_id,machine_id,cashier,custody_date,amount_minor,kind,note,created_by)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                scope.company_id, input.machine_id, input.cashier.trim(), input.date,
                amount, input.kind, input.note, actor.id
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    })();
    match result {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response(),
        Err(e) => error_response(e),
    }
}
