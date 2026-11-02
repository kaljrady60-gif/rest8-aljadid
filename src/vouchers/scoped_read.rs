//! Read-only linked vouchers with branch grants and a single SQLite snapshot.
use super::*;
use crate::identity::Principal;
use axum::extract::{Path, Query};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedQuery {
    company_id: i64,
    branch_id: i64,
    before: Option<i64>,
    ceiling: Option<i64>,
}
#[derive(Debug)]
enum ReadError {
    Unauthorized,
    Missing,
    Storage(Box<dyn std::error::Error>),
}
impl From<rusqlite::Error> for ReadError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Storage(Box::new(e))
    }
}
fn error(e: ReadError) -> Response {
    match e {
        ReadError::Unauthorized => fail(StatusCode::FORBIDDEN, "لا توجد صلاحية للفرع المحدد."),
        ReadError::Missing => fail(StatusCode::NOT_FOUND, "السند غير موجود."),
        ReadError::Storage(e) => {
            tracing::error!(%e,"scoped voucher read failed");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "تعذر قراءة السندات أو التحقق من بياناتها.",
            )
        }
    }
}
fn authorized(
    tx: &Connection,
    preview: bool,
    actor: &Principal,
    company: i64,
    branch: i64,
) -> Result<bool, ReadError> {
    Ok(preview
        || (crate::identity::is_current_user(tx, actor)?
            && crate::finance_registry::access::branch_read(tx, actor.id, company, branch)?))
}
pub async fn list(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<Principal>,
    Query(q): Query<ScopedQuery>,
) -> Response {
    if q.company_id <= 0
        || q.branch_id <= 0
        || q.before.is_some_and(|x| x <= 0)
        || q.ceiling.is_some_and(|x| x < 0)
        || q.before.is_some() && q.ceiling.is_none()
        || q.before.zip(q.ceiling).is_some_and(|(a, b)| a > b)
    {
        return fail(
            StatusCode::BAD_REQUEST,
            "نطاق الفرع أو مؤشر الصفحة غير صالح.",
        );
    }
    let result = (|| -> Result<serde_json::Value, ReadError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction()?;
        if !authorized(
            &tx,
            state.preview_no_login,
            &actor,
            q.company_id,
            q.branch_id,
        )? {
            return Err(ReadError::Unauthorized);
        }
        let ceiling = if let Some(c) = q.ceiling {
            c
        } else {
            tx.query_row("SELECT COALESCE(MAX(v.id),0) FROM financial_vouchers v JOIN finance_voucher_context c ON c.voucher_id=v.id WHERE c.company_id=?1 AND c.branch_id=?2",params![q.company_id,q.branch_id],|r|r.get(0))?
        };
        let vouchers: Vec<(i64, i64, String, String, String, Option<String>)> = {
            let mut st=tx.prepare("SELECT v.id,v.draft_id,v.kind,v.party_name,v.reference,v.voucher_no FROM financial_vouchers v JOIN finance_voucher_context c ON c.voucher_id=v.id WHERE c.company_id=?1 AND c.branch_id=?2 AND v.id<=?3 AND (?4 IS NULL OR v.id<?4) ORDER BY v.id DESC LIMIT 101")?;
            let rows = st
                .query_map(params![q.company_id, q.branch_id, ceiling, q.before], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
                })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let has_more = vouchers.len() > 100;
        let mut rows = Vec::new();
        for (id, draft_id, kind, party, reference, voucher_no) in vouchers.into_iter().take(100) {
            let draft = read_draft(&tx, draft_id)
                .map_err(ReadError::Storage)?
                .ok_or_else(|| ReadError::Storage("voucher draft missing".into()))?;
            if draft.lines.len() != 2
                || draft.lines[0].account_id.is_none()
                || draft.lines[1].account_id.is_none()
                || draft.lines[0].account_id == draft.lines[1].account_id
            {
                return Err(ReadError::Storage("invalid voucher lines".into()));
            }
            let first = &draft.lines[0];
            let second = &draft.lines[1];
            let parse = |s: &str| super::list::amount(s).map_err(ReadError::Storage);
            let valid = match kind.as_str() {
                "receipt" => {
                    parse(&first.debit)? > rust_decimal::Decimal::ZERO
                        && parse(&first.credit)?.is_zero()
                        && parse(&second.debit)?.is_zero()
                        && parse(&second.credit)? > rust_decimal::Decimal::ZERO
                }
                "payment" => {
                    parse(&first.credit)? > rust_decimal::Decimal::ZERO
                        && parse(&first.debit)?.is_zero()
                        && parse(&second.credit)?.is_zero()
                        && parse(&second.debit)? > rust_decimal::Decimal::ZERO
                }
                _ => false,
            };
            if !valid {
                return Err(ReadError::Storage("invalid voucher direction".into()));
            }
            rows.push(serde_json::json!({"id":id,"draft_id":draft_id,"kind":kind,"party_name":party,"reference":reference,"voucher_no":voucher_no,"date":draft.date,"currency":draft.currency,"amount":draft.totals.debit,"description":draft.description,"status":draft.status}));
        }
        let before = if has_more {
            rows.last().and_then(|r| r["id"].as_i64())
        } else {
            None
        };
        tx.commit()?;
        Ok(
            serde_json::json!({"company_id":q.company_id,"branch_id":q.branch_id,"rows":rows,"has_more":has_more,"ceiling":ceiling,"next_before":before}),
        )
    })();
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => error(e),
    }
}
pub async fn detail(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<Principal>,
    Path(id): Path<i64>,
) -> Response {
    if id <= 0 {
        return fail(StatusCode::BAD_REQUEST, "معرّف السند غير صالح.");
    }
    let result = (|| -> Result<serde_json::Value, ReadError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction()?;
        let context:Option<(i64,i64,i64)>=tx.query_row("SELECT c.company_id,c.branch_id,v.draft_id FROM financial_vouchers v JOIN finance_voucher_context c ON c.voucher_id=v.id WHERE v.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let (company, branch, draft_id) = context.ok_or(ReadError::Missing)?;
        if !authorized(&tx, state.preview_no_login, &actor, company, branch)? {
            return Err(ReadError::Missing);
        }
        let draft = read_draft(&tx, draft_id)
            .map_err(ReadError::Storage)?
            .ok_or_else(|| ReadError::Storage("voucher draft missing".into()))?;
        tx.commit()?;
        Ok(serde_json::json!({"id":id,"company_id":company,"branch_id":branch,"draft":draft}))
    })();
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => error(e),
    }
}
