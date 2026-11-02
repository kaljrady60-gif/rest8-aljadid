//! Company-scoped period management backed by the common financial_periods table.
use super::*;
use crate::identity::Principal;
use axum::{Extension, extract::Query};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyQuery {
    company_id: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeriodInput {
    company_id: i64,
    name: String,
    start_date: String,
    end_date: String,
    reason: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeriodChange {
    expected_status: String,
    approved: bool,
    allow_unposted: bool,
    reason: String,
}
#[derive(Debug)]
enum Error {
    Unauthorized,
    Missing,
    Conflict(&'static str),
    Invalid(&'static str),
    Storage(rusqlite::Error),
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Storage(e)
    }
}
fn err(e: Error) -> Response {
    match e {
        Error::Unauthorized => fail(
            StatusCode::FORBIDDEN,
            "لا توجد صلاحية إدارة الفترة لهذه الشركة.",
        ),
        Error::Missing => fail(StatusCode::NOT_FOUND, "الفترة غير موجودة."),
        Error::Conflict(m) => fail(StatusCode::CONFLICT, m),
        Error::Invalid(m) => fail(StatusCode::UNPROCESSABLE_ENTITY, m),
        Error::Storage(e) => {
            tracing::error!(%e,"company period failure");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "تعذر حفظ أو قراءة فترة الشركة.",
            )
        }
    }
}
fn valid_text(s: &str, max: usize) -> bool {
    let s = s.trim();
    !s.is_empty() && s.chars().count() <= max && !s.chars().any(char::is_control)
}
fn manager(tx: &Connection, actor: &Principal, company: i64) -> Result<bool, Error> {
    Ok(crate::identity::is_current_admin(tx, actor)?
        && crate::finance_registry::access::company_admin(tx, actor.id, company)?)
}
pub async fn list(
    State(state): State<AppState>,
    Extension(actor): Extension<Principal>,
    Query(q): Query<CompanyQuery>,
) -> Response {
    if q.company_id <= 0 {
        return fail(StatusCode::BAD_REQUEST, "الشركة غير صالحة.");
    }
    let result = (|| -> Result<serde_json::Value, Error> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction()?;
        if !state.preview_no_login
            && (!crate::identity::is_current_user(&tx, &actor)?
                || !crate::finance_registry::access::company_read(&tx, actor.id, q.company_id)?)
        {
            return Err(Error::Unauthorized);
        }
        let rows = {
            let mut st=tx.prepare("SELECT p.id,p.name,p.start_date,p.end_date,p.status FROM financial_periods p JOIN finance_period_context c ON c.period_id=p.id WHERE c.company_id=?1 ORDER BY p.start_date DESC,p.id DESC")?;
            let rows=st.query_map([q.company_id],|r|Ok(serde_json::json!({"id":r.get::<_,i64>(0)?,"name":r.get::<_,String>(1)?,"start_date":r.get::<_,String>(2)?,"end_date":r.get::<_,String>(3)?,"status":r.get::<_,String>(4)?})))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        tx.commit()?;
        Ok(serde_json::json!({"company_id":q.company_id,"periods":rows}))
    })();
    match result {
        Ok(x) => Json(x).into_response(),
        Err(e) => err(e),
    }
}
pub async fn create(
    State(state): State<AppState>,
    Extension(actor): Extension<Principal>,
    Json(p): Json<PeriodInput>,
) -> Response {
    if !state.experimental_posting || state.preview_no_login {
        return fail(
            StatusCode::FORBIDDEN,
            "إدارة الفترات التجريبية غير متاحة في المعاينة المفتوحة.",
        );
    }
    if p.company_id <= 0
        || !valid_text(&p.name, 100)
        || !valid_text(&p.reason, 200)
        || !valid_date(&p.start_date)
        || !valid_date(&p.end_date)
        || p.start_date > p.end_date
    {
        return err(Error::Invalid("بيانات الفترة أو سبب إنشائها غير صالح."));
    }
    let result = (|| -> Result<i64, Error> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !manager(&tx, &actor, p.company_id)? {
            return Err(Error::Unauthorized);
        }
        let overlap:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM financial_periods p JOIN finance_period_context c ON c.period_id=p.id WHERE c.company_id=?1 AND p.start_date<=?2 AND p.end_date>=?3)",params![p.company_id,p.end_date,p.start_date],|r|r.get(0))?;
        if overlap {
            return Err(Error::Conflict("تتداخل الفترة مع فترة أخرى للشركة."));
        }
        tx.execute("INSERT INTO financial_periods(name,start_date,end_date,status) VALUES(?1,?2,?3,'open')",params![p.name.trim(),p.start_date,p.end_date])?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO finance_period_context(period_id,company_id) VALUES(?1,?2)",
            params![id, p.company_id],
        )?;
        tx.execute(
            "INSERT INTO period_events(period_id,event,actor_id) VALUES(?1,'created',?2)",
            params![id, actor.id],
        )?;
        let event = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO finance_period_reasons(event_id,reason) VALUES(?1,?2)",
            params![event, p.reason.trim()],
        )?;
        tx.commit()?;
        Ok(id)
    })();
    match result {
        Ok(id) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"id":id,"status":"open"})),
        )
            .into_response(),
        Err(e) => err(e),
    }
}
pub async fn transition(
    State(state): State<AppState>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<i64>,
    Json(input): Json<PeriodChange>,
) -> Response {
    if !state.experimental_posting || state.preview_no_login {
        return fail(
            StatusCode::FORBIDDEN,
            "إدارة الفترات التجريبية غير متاحة في المعاينة المفتوحة.",
        );
    }
    if id <= 0
        || !input.approved
        || !["open", "closed"].contains(&input.expected_status.as_str())
        || !valid_text(&input.reason, 200)
    {
        return err(Error::Invalid("حدد الحالة المتوقعة والسبب وأكد العملية."));
    }
    let result = (|| -> Result<&'static str, Error> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row:Option<(i64,String,String,String)>=tx.query_row("SELECT c.company_id,p.status,p.start_date,p.end_date FROM financial_periods p JOIN finance_period_context c ON c.period_id=p.id WHERE p.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let (company, status, start, end) = row.ok_or(Error::Missing)?;
        if !manager(&tx, &actor, company)? {
            return Err(Error::Missing);
        }
        if status != input.expected_status {
            return Err(Error::Conflict("تغيرت حالة الفترة؛ أعد المراجعة."));
        }
        let (new, event, count) = if status == "open" {
            let count:i64=tx.query_row("SELECT COUNT(*) FROM financial_vouchers v JOIN finance_voucher_context c ON c.voucher_id=v.id JOIN drafts d ON d.id=v.draft_id WHERE c.company_id=?1 AND d.date BETWEEN ?2 AND ?3 AND NOT EXISTS(SELECT 1 FROM journal_entries e WHERE e.draft_id=d.id AND e.finalized=1)",params![company,start,end],|r|r.get(0))?;
            if count > 0 && !input.allow_unposted {
                return Err(Error::Conflict("توجد مسودات غير مرحّلة للشركة في الفترة."));
            }
            ("closed", "closed", Some(count))
        } else {
            ("open", "reopened", None)
        };
        tx.execute(
            "UPDATE financial_periods SET status=?1 WHERE id=?2",
            params![new, id],
        )?;
        tx.execute("INSERT INTO period_events(period_id,event,unposted_count,actor_id) VALUES(?1,?2,?3,?4)",params![id,event,count,actor.id])?;
        let eid = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO finance_period_reasons(event_id,reason) VALUES(?1,?2)",
            params![eid, input.reason.trim()],
        )?;
        tx.commit()?;
        Ok(new)
    })();
    match result {
        Ok(status) => Json(serde_json::json!({"status":status})).into_response(),
        Err(e) => err(e),
    }
}
