//! ترقيم المستندات: توالٍ ذرّي لكل (شركة، نوع مستند) لا يتكرر ولا يتخطى.
//!
//! الذرّية من SQLite نفسها: معاملة `IMMEDIATE` تُقفل الكتابة قبل القراءة، فلا يستطيع
//! طلبان أخذا الرقم نفسه مهما تزامنا — وهذا ما لا يضمنه العدّاد في الواجهة ولا في الذاكرة.
use super::audit;
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{TransactionBehavior, params};
use serde::{Deserialize, Serialize};

pub const DOC_TYPES: &[&str] = &[
    "journal",
    "receipt_voucher",
    "payment_voucher",
    "invoice_sales",
    "invoice_purchase",
    "asset",
    "payroll_run",
];

#[derive(Serialize)]
pub struct Sequence {
    doc_type: String,
    prefix: String,
    next_no: i64,
    width: i64,
    preview: String,
}

#[derive(Deserialize)]
pub struct ListQuery {
    company_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigureRequest {
    company_id: i64,
    doc_type: String,
    prefix: String,
    #[serde(default)]
    next_no: Option<i64>,
    #[serde(default)]
    width: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NextRequest {
    company_id: i64,
    doc_type: String,
}

pub fn format_number(prefix: &str, number: i64, width: i64) -> String {
    let width = width.clamp(1, 12) as usize;
    if prefix.is_empty() {
        format!("{number:0width$}")
    } else {
        format!("{prefix}-{number:0width$}")
    }
}

fn validate(doc_type: &str, prefix: &str) -> Result<(), FinanceError> {
    if !DOC_TYPES.contains(&doc_type) {
        return Err(FinanceError::Invalid("نوع المستند غير مدعوم.".into()));
    }
    if prefix.chars().count() > 12 {
        return Err(FinanceError::Invalid("البادئة حتى 12 حرفًا.".into()));
    }
    if !prefix
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(FinanceError::Invalid(
            "البادئة: حروف لاتينية وأرقام و- و_ فقط.".into(),
        ));
    }
    Ok(())
}

pub async fn list(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ListQuery>,
) -> Response {
    let result = (|| -> Result<Vec<Sequence>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT doc_type,prefix,next_no,width FROM finance_doc_sequences
             WHERE company_id=?1 ORDER BY doc_type",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            let prefix: String = r.get(1)?;
            let next_no: i64 = r.get(2)?;
            let width: i64 = r.get(3)?;
            Ok(Sequence {
                doc_type: r.get(0)?,
                preview: format_number(&prefix, next_no, width),
                prefix,
                next_no,
                width,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })();
    match result {
        Ok(rows) => {
            Json(serde_json::json!({"types": DOC_TYPES, "sequences": rows})).into_response()
        }
        Err(e) => error_response(e),
    }
}

pub async fn configure(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(req): Json<ConfigureRequest>,
) -> Response {
    let result = (|| -> Result<(), FinanceError> {
        validate(&req.doc_type, &req.prefix)?;
        let width = req.width.unwrap_or(5);
        if !(1..=12).contains(&width) {
            return Err(FinanceError::Invalid("عدد الخانات بين 1 و12.".into()));
        }
        if req.next_no.unwrap_or(1) < 1 {
            return Err(FinanceError::Invalid(
                "الرقم التالي يجب أن يكون 1 أو أكثر.".into(),
            ));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, req.company_id)?;
        scope.require_admin()?;
        // التهيئة لا تُنقص عدّادًا جرى استخدامه: نأخذ الأكبر بين المطلوب والحالي.
        tx.execute(
            "INSERT INTO finance_doc_sequences(company_id,doc_type,prefix,next_no,width,updated_at)
             VALUES(?1,?2,?3,?4,?5,CURRENT_TIMESTAMP)
             ON CONFLICT(company_id,doc_type) DO UPDATE SET
               prefix=excluded.prefix,
               width=excluded.width,
               next_no=MAX(finance_doc_sequences.next_no, excluded.next_no),
               updated_at=CURRENT_TIMESTAMP",
            params![
                scope.company_id,
                req.doc_type,
                req.prefix,
                req.next_no.unwrap_or(1),
                width
            ],
        )?;
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "numbering.configure",
                entity: "finance_doc_sequences",
                entity_id: Some(req.doc_type.clone()),
                payload: Some(serde_json::json!({"prefix": req.prefix, "width": width})),
            },
        )?;
        tx.commit()?;
        Ok(())
    })();
    match result {
        Ok(()) => Json(serde_json::json!({"saved": true})).into_response(),
        Err(e) => error_response(e),
    }
}

/// يصدر الرقم التالي ذرّيًا ويزيد العدّاد في المعاملة نفسها.
pub async fn next(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(req): Json<NextRequest>,
) -> Response {
    let result = (|| -> Result<String, FinanceError> {
        validate(&req.doc_type, "")?;
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, req.company_id)?;
        tx.execute(
            "INSERT INTO finance_doc_sequences(company_id,doc_type,prefix,next_no,width)
             VALUES(?1,?2,'',1,5)
             ON CONFLICT(company_id,doc_type) DO NOTHING",
            params![scope.company_id, req.doc_type],
        )?;
        let (prefix, next_no, width): (String, i64, i64) = tx.query_row(
            "SELECT prefix,next_no,width FROM finance_doc_sequences
             WHERE company_id=?1 AND doc_type=?2",
            params![scope.company_id, req.doc_type],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        tx.execute(
            "UPDATE finance_doc_sequences SET next_no=next_no+1, updated_at=CURRENT_TIMESTAMP
             WHERE company_id=?1 AND doc_type=?2",
            params![scope.company_id, req.doc_type],
        )?;
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "numbering.next",
                entity: "finance_doc_sequences",
                entity_id: Some(req.doc_type.clone()),
                payload: Some(serde_json::json!({"issued": next_no})),
            },
        )?;
        tx.commit()?;
        Ok(format_number(&prefix, next_no, width))
    })();
    match result {
        Ok(number) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"number": number})),
        )
            .into_response(),
        Err(e) => error_response(e),
    }
}
