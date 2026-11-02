//! الأطراف: عملاء وموردون مرتبطون بحسابات تفصيلية في دليل الشركة.
//! الطرف مرتبط بشركة واحدة (قرار المالك) ولا يُدمج حسابيًا مع غيره تلقائيًا.
use super::audit;
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct ListQuery {
    company_id: i64,
    #[serde(default)]
    role: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewParty {
    company_id: i64,
    name: String,
    role: String,
    account_id: i64,
    #[serde(default)]
    phone: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ListQuery>,
) -> Response {
    let result = (|| -> Result<Vec<serde_json::Value>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let role = q.role.clone().unwrap_or_else(|| "all".into());
        if !["all", "customer", "supplier", "other"].contains(&role.as_str()) {
            return Err(FinanceError::Invalid("تصنيف طرف غير مدعوم.".into()));
        }
        let mut stmt = db.prepare(
            "SELECT p.id,p.name,p.role,p.account_id,
                    (SELECT code || ' · ' || name FROM accounts WHERE id=p.account_id)
             FROM finance_parties p WHERE p.company_id=?1 ORDER BY p.name",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?,
                "name": r.get::<_, String>(1)?,
                "role": r.get::<_, String>(2)?,
                "account_id": r.get::<_, i64>(3)?,
                "account": r.get::<_, Option<String>>(4)?
            }))
        })?;
        let mut items = Vec::new();
        for row in rows {
            let item = row?;
            if role != "all" && item["role"] != role.as_str() {
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
    Json(n): Json<NewParty>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        if n.name.trim().is_empty() || n.name.chars().count() > 120 {
            return Err(FinanceError::Invalid(
                "اسم الطرف مطلوب وبحد 120 حرفًا.".into(),
            ));
        }
        if !["customer", "supplier", "other"].contains(&n.role.as_str()) {
            return Err(FinanceError::Invalid("تصنيف طرف غير مدعوم.".into()));
        }
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
        let owned: Option<String> = tx
            .query_row(
                "SELECT a.kind FROM accounts a
                 JOIN finance_company_account_codes c ON c.account_id=a.id AND c.company_id=?2
                 WHERE a.id=?1",
                params![n.account_id, scope.company_id],
                |r| r.get(0),
            )
            .optional()?;
        match owned.as_deref() {
            Some("detail") => {}
            Some(_) => {
                return Err(FinanceError::Invalid(
                    "اربط الطرف بحساب تفصيلي لا رئيسي.".into(),
                ));
            }
            None => {
                return Err(FinanceError::Invalid(
                    "الحساب ليس من دليل هذه الشركة.".into(),
                ));
            }
        }
        tx.execute(
            "INSERT INTO finance_parties(company_id,branch_id,account_id,name,role)
             VALUES(?1,?2,?3,?4,?5)",
            params![
                scope.company_id,
                branch,
                n.account_id,
                n.name.trim(),
                n.role
            ],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(_, _) => {
                FinanceError::Conflict("هذا الحساب مستخدم لطرف أو صندوق آخر.")
            }
            other => FinanceError::Storage(other),
        })?;
        let id = tx.last_insert_rowid();
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "party.create",
                entity: "finance_parties",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({
                    "name": n.name.trim(), "role": n.role, "phone": n.phone
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
