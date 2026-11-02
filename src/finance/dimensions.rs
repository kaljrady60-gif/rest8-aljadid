//! مراكز التكلفة والمشاريع (أبعاد تحليلية) — تُربط بسطور القيود لا بالقيد كله.
use super::audit;
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{TransactionBehavior, params};
use serde::{Deserialize, Serialize};

const KINDS: &[&str] = &["cost_center", "project"];

#[derive(Serialize)]
pub struct Dimension {
    id: i64,
    code: String,
    name: String,
    kind: String,
    archived_at: Option<String>,
}

#[derive(Deserialize)]
pub struct ListQuery {
    company_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewDimension {
    company_id: i64,
    code: String,
    name: String,
    kind: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveRequest {
    company_id: i64,
    id: i64,
}

pub async fn list(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ListQuery>,
) -> Response {
    let result = (|| -> Result<Vec<Dimension>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT id,code,name,kind,archived_at FROM finance_dimensions
             WHERE company_id=?1 ORDER BY archived_at IS NOT NULL, code",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            Ok(Dimension {
                id: r.get(0)?,
                code: r.get(1)?,
                name: r.get(2)?,
                kind: r.get(3)?,
                archived_at: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })();
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn create(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<NewDimension>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        if !KINDS.contains(&n.kind.as_str()) {
            return Err(FinanceError::Invalid("نوع البُعد غير مدعوم.".into()));
        }
        let code = n.code.trim();
        if code.is_empty() || code.len() > 20 {
            return Err(FinanceError::Invalid("رمز البُعد مطلوب حتى 20 حرفًا.".into()));
        }
        if n.name.trim().is_empty() || n.name.chars().count() > 120 {
            return Err(FinanceError::Invalid(
                "اسم البُعد مطلوب حتى 120 حرفًا.".into(),
            ));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        scope.require_admin()?;
        tx.execute(
            "INSERT INTO finance_dimensions(company_id,code,name,kind) VALUES(?1,?2,?3,?4)",
            params![scope.company_id, code, n.name.trim(), n.kind],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(_, _) => FinanceError::Conflict("رمز البُعد مستخدم."),
            other => FinanceError::Storage(other),
        })?;
        let id = tx.last_insert_rowid();
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "dimension.create",
                entity: "finance_dimensions",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({"kind": n.kind, "code": code})),
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

pub async fn archive(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(req): Json<ArchiveRequest>,
) -> Response {
    let result = (|| -> Result<(), FinanceError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, req.company_id)?;
        scope.require_admin()?;
        let changed = tx.execute(
            "UPDATE finance_dimensions SET archived_at=CURRENT_TIMESTAMP
             WHERE id=?1 AND company_id=?2 AND archived_at IS NULL",
            params![req.id, scope.company_id],
        )?;
        if changed == 0 {
            return Err(FinanceError::NotFound("البُعد"));
        }
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "dimension.archive",
                entity: "finance_dimensions",
                entity_id: Some(req.id.to_string()),
                payload: None,
            },
        )?;
        tx.commit()?;
        Ok(())
    })();
    match result {
        Ok(()) => Json(serde_json::json!({"archived": true})).into_response(),
        Err(e) => error_response(e),
    }
}

/// يتحقق أن كل بُعد مذكور يخص الشركة وغير مؤرشف — يُستخدم عند ترحيل القيد.
pub fn ensure_active(
    db: &rusqlite::Connection,
    company_id: i64,
    ids: &[i64],
) -> Result<(), FinanceError> {
    for id in ids {
        let ok: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM finance_dimensions WHERE id=?1 AND company_id=?2 AND archived_at IS NULL)",
            params![id, company_id],
            |r| r.get(0),
        )?;
        if !ok {
            return Err(FinanceError::Invalid(
                "مركز تكلفة غير صالح أو مؤرشف.".into(),
            ));
        }
    }
    Ok(())
}
