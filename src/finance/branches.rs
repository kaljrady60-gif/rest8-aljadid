//! الفروع: قائمة الفروع مع حجم الحركة في كل فرع، وإنشاء فرع جديد.
//!
//! الفرع يُسجَّل على كل قيد عند الترحيل (`journal_entries.branch_id`)، فتقارير
//! «موحّدة» أو «فرع واحد» تخرج من نفس البيانات بلا نسخ ولا تجميع مسبق.
use super::audit;
use super::money;
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use rust_decimal::Decimal;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct CompanyQuery {
    company_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewBranch {
    company_id: i64,
    code: String,
    name: String,
    #[serde(default)]
    city: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<CompanyQuery>,
) -> Response {
    let result = (|| -> Result<Vec<serde_json::Value>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT b.id,b.code,b.name,
                    (SELECT COUNT(*) FROM journal_entries e
                      WHERE e.company_id=b.company_id AND e.branch_id=b.id AND e.finalized=1)
             FROM finance_branches b WHERE b.company_id=?1 ORDER BY b.id",
        )?;
        let rows = stmt
            .query_map([scope.company_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        // الجمع بـDecimal لا بـf64 حتى تبقى الأرقام النقدية دقيقة.
        let mut items = Vec::new();
        for (id, code, name, entries) in rows {
            let mut total = Decimal::ZERO;
            let mut lines = db.prepare(
                "SELECT l.debit FROM journal_lines l JOIN journal_entries e ON e.id=l.entry_id
                 WHERE e.company_id=?1 AND e.branch_id=?2 AND e.finalized=1",
            )?;
            let values =
                lines.query_map(params![scope.company_id, id], |r| r.get::<_, String>(0))?;
            for value in values {
                total += money::from_db(&value?, "مدين")?;
            }
            items.push(serde_json::json!({
                "id": id, "code": code, "name": name, "entries": entries,
                "debit": money::render(total)
            }));
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
    Json(n): Json<NewBranch>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        if n.code.trim().len() < 2 || n.name.trim().len() < 2 {
            return Err(FinanceError::Invalid("رمز الفرع واسمه مطلوبان.".into()));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        scope.require_admin()?;
        let exists: Option<i64> = tx
            .query_row(
                "SELECT id FROM finance_branches WHERE company_id=?1 AND code=?2",
                params![scope.company_id, n.code.trim()],
                |r| r.get(0),
            )
            .optional()?;
        if exists.is_some() {
            return Err(FinanceError::Conflict("رمز الفرع مستخدم مسبقًا."));
        }
        tx.execute(
            "INSERT INTO finance_branches(company_id,code,name) VALUES(?1,?2,?3)",
            params![scope.company_id, n.code.trim(), n.name.trim()],
        )?;
        let id = tx.last_insert_rowid();
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "branch.create",
                entity: "finance_branches",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({
                    "code": n.code.trim(), "city": n.city.as_deref()
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

/// يتحقق أن الفرع (إن حُدد) يخص الشركة، ويعيده.
pub fn ensure_branch(
    db: &rusqlite::Connection,
    company_id: i64,
    branch_id: Option<i64>,
) -> Result<Option<i64>, FinanceError> {
    match branch_id {
        None => Ok(None),
        Some(id) => {
            let owned: Option<i64> = db
                .query_row(
                    "SELECT id FROM finance_branches WHERE id=?1 AND company_id=?2",
                    params![id, company_id],
                    |r| r.get(0),
                )
                .optional()?;
            if owned.is_none() {
                return Err(FinanceError::Invalid("الفرع ليس من هذه الشركة.".into()));
            }
            Ok(Some(id))
        }
    }
}
