//! سير اعتماد القيود: سياسة لكل شركة، ثم إرسال واعتماد أو رفض، مع فصل المُنشئ عن المُعتمد.
//!
//! الحالات: `pending` (مسودة) ← `submitted` ← `approved` | `rejected` ← `posted`.
//! الفرض في طبقة التطبيق لا في مُشغّل قاعدة، كي تمرّ القيود النظامية
//! (الشيكات والإقفال السنوي) دون تعطيل المحرك.
use super::audit;
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct PolicyQuery {
    company_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyInput {
    company_id: i64,
    require_approval: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    company_id: i64,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Deserialize)]
pub struct FlowQuery {
    company_id: i64,
    #[serde(default)]
    state: Option<String>,
}

/// هل تحتاج الشركة اعتمادًا قبل الترحيل؟
pub fn requires_approval(db: &Connection, company_id: i64) -> rusqlite::Result<bool> {
    let value: Option<i64> = db
        .query_row(
            "SELECT require_approval FROM finance_approval_policy WHERE company_id=?1",
            [company_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(value.unwrap_or(0) == 1)
}

/// حالة قيد في سير الاعتماد (`pending` إن لم يوجد صف).
pub fn state_of(
    db: &Connection,
    draft_id: i64,
) -> rusqlite::Result<(String, Option<i64>, Option<i64>)> {
    let row: Option<(String, Option<i64>, Option<i64>)> = db
        .query_row(
            "SELECT state,submitted_by,decided_by FROM finance_draft_flow WHERE draft_id=?1",
            [draft_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    Ok(row.unwrap_or(("pending".into(), None, None)))
}

/// بوابة الترحيل: تسمح دائمًا إن لم تُطلب سياسة اعتماد، وإلا فلابد من `approved`.
pub fn gate(db: &Connection, company_id: i64, draft_id: i64) -> Result<(), FinanceError> {
    if !requires_approval(db, company_id)? {
        return Ok(());
    }
    if state_of(db, draft_id)?.0 == "approved" {
        return Ok(());
    }
    Err(FinanceError::Conflict(
        "هذا القيد يحتاج اعتمادًا قبل الترحيل حسب سياسة الشركة.",
    ))
}

/// بوابة إجبارية: تُستخدم عندما يُشترط الاعتماد من متغيرات الأستاذ لا من سياسة الشركة.
///
/// منفصلة عن `gate` لأن `gate` تحترم سياسة الشركة (قد تكون معطّلة)؛ وهذه تفرض الاعتماد دائمًا.
pub fn require_approved(db: &Connection, draft_id: i64) -> Result<(), FinanceError> {
    if state_of(db, draft_id)?.0 == "approved" {
        return Ok(());
    }
    Err(FinanceError::Conflict(
        "متغيرات الأستاذ تشترط اعتماد القيد قبل ترحيله.",
    ))
}

fn touch(
    db: &Connection,
    company_id: i64,
    draft_id: i64,
    state: &str,
    actor: i64,
    note: Option<&str>,
) -> Result<(), FinanceError> {
    let stamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let submitted = state == "submitted";
    let decided = matches!(state, "approved" | "rejected");
    db.execute(
        "INSERT INTO finance_draft_flow(draft_id,company_id,state,submitted_by,submitted_at,decided_by,decided_at,note,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
         ON CONFLICT(draft_id) DO UPDATE SET
            state=excluded.state,
            submitted_by=COALESCE(excluded.submitted_by, finance_draft_flow.submitted_by),
            submitted_at=COALESCE(excluded.submitted_at, finance_draft_flow.submitted_at),
            decided_by=COALESCE(excluded.decided_by, finance_draft_flow.decided_by),
            decided_at=COALESCE(excluded.decided_at, finance_draft_flow.decided_at),
            note=excluded.note,
            updated_at=excluded.updated_at",
        params![
            draft_id,
            company_id,
            state,
            submitted.then_some(actor),
            submitted.then(|| stamp.clone()),
            decided.then_some(actor),
            decided.then(|| stamp.clone()),
            note,
            stamp
        ],
    )?;
    Ok(())
}

pub async fn get_policy(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<PolicyQuery>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let row: Option<(i64, String)> = db
            .query_row(
                "SELECT require_approval,updated_at FROM finance_approval_policy WHERE company_id=?1",
                [scope.company_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (required, updated) = match row {
            Some((value, at)) => (value == 1, Some(at)),
            None => (false, None),
        };
        Ok(serde_json::json!({
            "company_id": scope.company_id,
            "require_approval": required,
            "updated_at": updated,
            "is_admin": scope.is_admin
        }))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn set_policy(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(input): Json<PolicyInput>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, input.company_id)?;
        scope.require_admin()?;
        tx.execute(
            "INSERT INTO finance_approval_policy(company_id,require_approval,updated_by,updated_at)
             VALUES(?1,?2,?3,?4)
             ON CONFLICT(company_id) DO UPDATE SET
                require_approval=excluded.require_approval,
                updated_by=excluded.updated_by,
                updated_at=excluded.updated_at",
            params![
                scope.company_id,
                i64::from(input.require_approval),
                actor.id,
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
            ],
        )?;
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "approval.policy",
                entity: "finance_approval_policy",
                entity_id: Some(scope.company_id.to_string()),
                payload: Some(serde_json::json!({"require_approval": input.require_approval})),
            },
        )?;
        tx.commit()?;
        Ok(serde_json::json!({"require_approval": input.require_approval}))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn list_flow(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<FlowQuery>,
) -> Response {
    let result = (|| -> Result<Vec<serde_json::Value>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let state_filter = q.state.clone().unwrap_or_else(|| "all".into());
        if ![
            "all",
            "pending",
            "submitted",
            "approved",
            "rejected",
            "posted",
        ]
        .contains(&state_filter.as_str())
        {
            return Err(FinanceError::Invalid("حالة تصفية غير مدعومة.".into()));
        }
        let mut stmt = db.prepare(
            "SELECT d.id,d.description,d.date,d.currency,d.total,
                    CASE WHEN EXISTS(SELECT 1 FROM journal_entries e WHERE e.draft_id=d.id AND e.finalized=1)
                         THEN 'posted' ELSE COALESCE(f.state,'pending') END,
                    f.submitted_by, f.decided_by, f.note,
                    EXISTS(SELECT 1 FROM journal_entries e WHERE e.draft_id=d.id AND e.finalized=1)
             FROM drafts d
             LEFT JOIN finance_draft_flow f ON f.draft_id=d.id
             WHERE d.company_id=?1 AND d.date >= '2000-01-01'
             ORDER BY d.id DESC LIMIT 300",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            Ok(serde_json::json!({
                "draft_id": r.get::<_, i64>(0)?,
                "description": r.get::<_, String>(1)?,
                "date": r.get::<_, String>(2)?,
                "currency": r.get::<_, String>(3)?,
                "total": r.get::<_, String>(4)?,
                "state": r.get::<_, String>(5)?,
                "submitted_by": r.get::<_, Option<i64>>(6)?,
                "decided_by": r.get::<_, Option<i64>>(7)?,
                "note": r.get::<_, Option<String>>(8)?,
                "posted": r.get::<_, i64>(9)? == 1
            }))
        })?;
        let mut items = Vec::new();
        for row in rows {
            let item = row?;
            if state_filter != "all" && item["state"] != state_filter.as_str() {
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

async fn transition(
    state: AppState,
    actor: crate::identity::Principal,
    draft_id: i64,
    company_id: i64,
    target: &'static str,
    note: Option<String>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, company_id)?;
        let draft: Option<(Option<i64>, String)> = tx
            .query_row(
                "SELECT company_id,total FROM drafts WHERE id=?1",
                [draft_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (owner, _total) = draft.ok_or(FinanceError::NotFound("المسودة"))?;
        if owner != Some(scope.company_id) {
            return Err(FinanceError::Forbidden);
        }
        let posted: i64 = tx.query_row(
            "SELECT COUNT(*) FROM journal_entries WHERE draft_id=?1 AND finalized=1",
            [draft_id],
            |r| r.get(0),
        )?;
        if posted > 0 {
            return Err(FinanceError::Conflict("القيد مرحّل؛ لا يقبل تغيير الحالة."));
        }
        let (current, submitted_by, _) = state_of(&tx, draft_id)?;
        match target {
            "submitted" => {
                if !matches!(current.as_str(), "pending" | "rejected") {
                    return Err(FinanceError::Conflict("الحالة لا تسمح بالإرسال."));
                }
            }
            "approved" | "rejected" => {
                scope.require_admin()?;
                if current != "submitted" {
                    return Err(FinanceError::Conflict(
                        "الاعتماد أو الرفض يقتصر على القيود المُرسلة.",
                    ));
                }
                if submitted_by == Some(actor.id) && target == "approved" {
                    return Err(FinanceError::Forbidden);
                }
            }
            _ => return Err(FinanceError::Invalid("انتقال غير مدعوم.".into())),
        }
        touch(
            &tx,
            scope.company_id,
            draft_id,
            target,
            actor.id,
            note.as_deref(),
        )?;
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "approval.flow",
                entity: "drafts",
                entity_id: Some(draft_id.to_string()),
                payload: Some(serde_json::json!({"from": current, "to": target, "note": note})),
            },
        )?;
        tx.commit()?;
        Ok(serde_json::json!({"draft_id": draft_id, "state": target}))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn submit(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Path(draft_id): Path<i64>,
    Json(d): Json<Decision>,
) -> Response {
    transition(state, actor, draft_id, d.company_id, "submitted", d.note).await
}

pub async fn approve(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Path(draft_id): Path<i64>,
    Json(d): Json<Decision>,
) -> Response {
    transition(state, actor, draft_id, d.company_id, "approved", d.note).await
}

pub async fn reject(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Path(draft_id): Path<i64>,
    Json(d): Json<Decision>,
) -> Response {
    transition(state, actor, draft_id, d.company_id, "rejected", d.note).await
}

/// يُوسم القيد `posted` بعد نجاح الترحيل (يُستدعى من مسار الترحيل).
pub fn mark_posted(
    db: &Connection,
    company_id: i64,
    draft_id: i64,
    actor: i64,
) -> Result<(), FinanceError> {
    touch(db, company_id, draft_id, "posted", actor, None)
}

#[allow(dead_code)]
fn _status(_: StatusCode) {}
