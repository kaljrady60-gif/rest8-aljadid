//! الهيكل الإداري: أقسام الشركة شجرية (قسم داخل قسم) بلا دورات ولا تداخل بين الشركات.
//!
//! الحماية من الدورات ليست تحقّقًا سطحيًا: عند كل ربط نتصعّد سلسلة الآباء فعليًا،
//! فإن وُجد القسم نفسه في السلسلة رُفض الربط. الشجرة تبقى دائمًا محدودة العمق.
use super::audit;
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

pub const MAX_DEPTH: usize = 10;

#[derive(Serialize)]
pub struct Department {
    id: i64,
    code: String,
    name: String,
    parent_id: Option<i64>,
    manager_name: Option<String>,
    depth: i64,
    archived_at: Option<String>,
}

#[derive(Deserialize)]
pub struct ListQuery {
    company_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewDepartment {
    company_id: i64,
    code: String,
    name: String,
    #[serde(default)]
    parent_id: Option<i64>,
    #[serde(default)]
    manager_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveRequest {
    company_id: i64,
    id: i64,
}

pub fn validate_code(code: &str) -> Result<(), FinanceError> {
    if code.is_empty() || code.chars().count() > 20 {
        return Err(FinanceError::Invalid("رمز القسم مطلوب حتى 20 حرفًا.".into()));
    }
    if !code
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(FinanceError::Invalid(
            "الرمز: حروف لاتينية وأرقام و- و_ فقط.".into(),
        ));
    }
    Ok(())
}

pub fn validate_name(name: &str) -> Result<(), FinanceError> {
    if name.trim().is_empty() || name.chars().count() > 120 {
        return Err(FinanceError::Invalid(
            "اسم القسم مطلوب حتى 120 حرفًا.".into(),
        ));
    }
    Ok(())
}

/// هل يخلق ربط `parent` بالقسم `node` دورة؟ (سلسلة الآباء تشمل القسم نفسه)
pub fn makes_cycle(ancestry: &[i64], node: i64) -> bool {
    ancestry.contains(&node)
}

fn chain(db: &rusqlite::Connection, company_id: i64, start: i64) -> Result<Vec<i64>, FinanceError> {
    let mut out = Vec::new();
    let mut cursor = Some(start);
    while let Some(id) = cursor {
        if out.len() > MAX_DEPTH {
            return Err(FinanceError::Invalid(
                "تجاوزت الشجرة حدّ العمق المسموح.".into(),
            ));
        }
        out.push(id);
        cursor = db
            .query_row(
                "SELECT parent_id FROM finance_departments WHERE id=?1 AND company_id=?2",
                params![id, company_id],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten();
    }
    Ok(out)
}

pub async fn list(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ListQuery>,
) -> Response {
    let result = (|| -> Result<Vec<Department>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT id,code,name,parent_id,manager_name,depth,archived_at
             FROM finance_departments WHERE company_id=?1
             ORDER BY archived_at IS NOT NULL, depth, code",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            Ok(Department {
                id: r.get(0)?,
                code: r.get(1)?,
                name: r.get(2)?,
                parent_id: r.get(3)?,
                manager_name: r.get(4)?,
                depth: r.get(5)?,
                archived_at: r.get(6)?,
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
    Json(n): Json<NewDepartment>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        let code = n.code.trim();
        let name = n.name.trim();
        validate_code(code)?;
        validate_name(name)?;
        let manager = n
            .manager_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if manager.is_some_and(|m| m.chars().count() > 120) {
            return Err(FinanceError::Invalid("اسم المسؤول حتى 120 حرفًا.".into()));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        scope.require_admin()?;
        let (parent_id, depth) = match n.parent_id {
            None => (None, 0_i64),
            Some(parent) => {
                let parent_depth: i64 = tx
                    .query_row(
                        "SELECT depth FROM finance_departments
                         WHERE id=?1 AND company_id=?2 AND archived_at IS NULL",
                        params![parent, scope.company_id],
                        |r| r.get(0),
                    )
                    .optional()?
                    .ok_or(FinanceError::Invalid("القسم الأب غير صالح.".into()))?;
                if parent_depth + 1 >= MAX_DEPTH as i64 {
                    return Err(FinanceError::Invalid(
                        "تجاوزت الشجرة حدّ العمق المسموح.".into(),
                    ));
                }
                (Some(parent), parent_depth + 1)
            }
        };
        tx.execute(
            "INSERT INTO finance_departments(company_id,code,name,parent_id,manager_name,depth)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![scope.company_id, code, name, parent_id, manager, depth],
        )
        .map_err(|e| match e {
            // القيد الوحيد (رمز مكرر) وحده يعني تعارضًا؛ أي فشل آخر يبقى خطأ تخزين حقيقيًا
            // حتى لا يُخفى سبب مثل جدول مفقود أو مفتاح أجنبي خاطئ.
            rusqlite::Error::SqliteFailure(err, _)
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                FinanceError::Conflict("رمز القسم مستخدم.")
            }
            other => FinanceError::Storage(other),
        })?;
        let id = tx.last_insert_rowid();
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "department.create",
                entity: "finance_departments",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({"code": code, "depth": depth})),
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
        let chain_ids = chain(&tx, scope.company_id, req.id)?;
        if !makes_cycle(&chain_ids, req.id) {
            return Err(FinanceError::NotFound("القسم"));
        }
        let children: i64 = tx.query_row(
            "SELECT COUNT(*) FROM finance_departments
             WHERE parent_id=?1 AND company_id=?2 AND archived_at IS NULL",
            params![req.id, scope.company_id],
            |r| r.get(0),
        )?;
        if children > 0 {
            return Err(FinanceError::Conflict(
                "لا يمكن أرشفة قسم له أقسام فرعية نشطة.",
            ));
        }
        let changed = tx.execute(
            "UPDATE finance_departments SET archived_at=CURRENT_TIMESTAMP
             WHERE id=?1 AND company_id=?2 AND archived_at IS NULL",
            params![req.id, scope.company_id],
        )?;
        if changed == 0 {
            return Err(FinanceError::NotFound("القسم"));
        }
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "department.archive",
                entity: "finance_departments",
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
