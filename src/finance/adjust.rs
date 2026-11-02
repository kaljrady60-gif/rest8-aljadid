//! التسويات المحاسبية ومراجعة الوثائق: مسار قرار موحّد (معلّق → معتمد/مرفوض)
//! بلا أي أثر على الأرصدة حتى الاعتماد، وكل قرار يُقيَّد في سجل التدقيق.
use super::audit;
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

pub const ADJUST_KINDS: &[&str] = &["accrual", "prepaid", "depreciation", "provision", "reclass"];
pub const REVIEW_DOC_TYPES: &[&str] = &["invoice", "receipt", "contract", "statement", "other"];

#[derive(Serialize)]
pub struct Adjustment {
    id: i64,
    kind: String,
    description: String,
    amount: String,
    currency: String,
    account_id: i64,
    offset_account_id: i64,
    status: String,
    archived_at: Option<String>,
}

#[derive(Serialize)]
pub struct Review {
    id: i64,
    doc_type: String,
    doc_ref: String,
    note: Option<String>,
    status: String,
    decided_at: Option<String>,
}

#[derive(Deserialize)]
pub struct CompanyQuery {
    company_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewAdjustment {
    company_id: i64,
    kind: String,
    description: String,
    amount: String,
    currency: String,
    account_id: i64,
    offset_account_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewReview {
    company_id: i64,
    doc_type: String,
    doc_ref: String,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decide {
    company_id: i64,
    id: i64,
    approve: bool,
}

fn account_ok(db: &rusqlite::Connection, id: i64) -> Result<bool, FinanceError> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1)",
        [id],
        |r| r.get(0),
    )?)
}

pub async fn adjustments(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<CompanyQuery>,
) -> Response {
    let result = (|| -> Result<Vec<Adjustment>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT id,kind,description,amount,currency,account_id,offset_account_id,status,archived_at
             FROM finance_adjustments WHERE company_id=?1
             ORDER BY archived_at IS NOT NULL, id DESC LIMIT 200",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            Ok(Adjustment {
                id: r.get(0)?,
                kind: r.get(1)?,
                description: r.get(2)?,
                amount: r.get(3)?,
                currency: r.get(4)?,
                account_id: r.get(5)?,
                offset_account_id: r.get(6)?,
                status: r.get(7)?,
                archived_at: r.get(8)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })();
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn create_adjustment(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<NewAdjustment>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        if !ADJUST_KINDS.contains(&n.kind.as_str()) {
            return Err(FinanceError::Invalid("نوع التسوية غير مدعوم.".into()));
        }
        let description = n.description.trim();
        if description.is_empty() || description.chars().count() > 200 {
            return Err(FinanceError::Invalid(
                "بيان التسوية مطلوب حتى 200 حرف.".into(),
            ));
        }
        let amount = Decimal::from_str(n.amount.trim())
            .map_err(|_| FinanceError::Invalid("المبلغ ليس رقمًا صالحًا.".into()))?;
        if amount <= Decimal::ZERO {
            return Err(FinanceError::Invalid(
                "المبلغ يجب أن يكون أكبر من صفر.".into(),
            ));
        }
        if n.account_id == n.offset_account_id {
            return Err(FinanceError::Invalid(
                "حساب التسوية وحساب المقابل يجب أن يختلفا.".into(),
            ));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        if !account_ok(&tx, n.account_id)? || !account_ok(&tx, n.offset_account_id)? {
            return Err(FinanceError::Invalid(
                "أحد حسابي التسوية غير موجود في الدليل.".into(),
            ));
        }
        tx.execute(
            "INSERT INTO finance_adjustments
             (company_id,kind,description,amount,currency,account_id,offset_account_id)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                scope.company_id,
                n.kind,
                description,
                super::money::render(amount),
                n.currency,
                n.account_id,
                n.offset_account_id
            ],
        )?;
        let id = tx.last_insert_rowid();
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "adjustment.create",
                entity: "finance_adjustments",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({"kind": n.kind, "amount": n.amount})),
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

pub async fn decide_adjustment(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(req): Json<Decide>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, req.company_id)?;
        scope.require_admin()?;
        let status: Option<String> = tx
            .query_row(
                "SELECT status FROM finance_adjustments
                 WHERE id=?1 AND company_id=?2 AND archived_at IS NULL",
                params![req.id, scope.company_id],
                |r| r.get(0),
            )
            .optional()?;
        match status.as_deref() {
            None => return Err(FinanceError::NotFound("التسوية")),
            Some("pending") => {}
            Some(_) => return Err(FinanceError::Conflict("التسوية حُسمت مسبقًا.")),
        }
        tx.execute(
            "UPDATE finance_adjustments
             SET status=?3, decided_by=?4, decided_at=CURRENT_TIMESTAMP
             WHERE id=?1 AND company_id=?2",
            params![
                req.id,
                scope.company_id,
                if req.approve { "approved" } else { "rejected" },
                actor.id
            ],
        )?;
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: if req.approve {
                    "adjustment.approve"
                } else {
                    "adjustment.reject"
                },
                entity: "finance_adjustments",
                entity_id: Some(req.id.to_string()),
                payload: None,
            },
        )?;
        let decided: (String, Option<String>) = tx.query_row(
            "SELECT status, decided_at FROM finance_adjustments WHERE id=?1 AND company_id=?2",
            params![req.id, scope.company_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let new_status = decided.0.clone();
        tx.commit()?;
        Ok(serde_json::json!({
            "id": req.id,
            "entity": "adjustment",
            "status": new_status,
            "decided_at": decided.1
        }))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn reviews(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<CompanyQuery>,
) -> Response {
    let result = (|| -> Result<Vec<Review>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT id,doc_type,doc_ref,note,status,decided_at
             FROM finance_doc_reviews WHERE company_id=?1 ORDER BY id DESC LIMIT 200",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            Ok(Review {
                id: r.get(0)?,
                doc_type: r.get(1)?,
                doc_ref: r.get(2)?,
                note: r.get(3)?,
                status: r.get(4)?,
                decided_at: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })();
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn create_review(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<NewReview>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        if !REVIEW_DOC_TYPES.contains(&n.doc_type.as_str()) {
            return Err(FinanceError::Invalid("نوع الوثيقة غير مدعوم.".into()));
        }
        let reference = n.doc_ref.trim();
        if reference.is_empty() || reference.chars().count() > 60 {
            return Err(FinanceError::Invalid(
                "مرجع الوثيقة مطلوب حتى 60 حرفًا.".into(),
            ));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        tx.execute(
            "INSERT INTO finance_doc_reviews(company_id,doc_type,doc_ref,note)
             VALUES(?1,?2,?3,?4)",
            params![
                scope.company_id,
                n.doc_type,
                reference,
                n.note.as_deref().map(str::trim)
            ],
        )?;
        let id = tx.last_insert_rowid();
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "docreview.create",
                entity: "finance_doc_reviews",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({"doc_type": n.doc_type, "ref": reference})),
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

pub async fn decide_review(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(req): Json<Decide>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, req.company_id)?;
        scope.require_admin()?;
        let changed = tx.execute(
            "UPDATE finance_doc_reviews
             SET status=?3, decided_by=?4, decided_at=CURRENT_TIMESTAMP
             WHERE id=?1 AND company_id=?2 AND status='pending'",
            params![
                req.id,
                scope.company_id,
                if req.approve { "approved" } else { "rejected" },
                actor.id
            ],
        )?;
        if changed == 0 {
            return Err(FinanceError::Conflict(
                "الوثيقة محسومة مسبقًا أو غير موجودة.",
            ));
        }
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: if req.approve {
                    "docreview.approve"
                } else {
                    "docreview.reject"
                },
                entity: "finance_doc_reviews",
                entity_id: Some(req.id.to_string()),
                payload: None,
            },
        )?;
        let decided: (String, Option<String>) = tx.query_row(
            "SELECT status, decided_at FROM finance_doc_reviews WHERE id=?1 AND company_id=?2",
            params![req.id, scope.company_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let new_status = decided.0.clone();
        tx.commit()?;
        Ok(serde_json::json!({
            "id": req.id,
            "entity": "doc_review",
            "status": new_status,
            "decided_at": decided.1
        }))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}
