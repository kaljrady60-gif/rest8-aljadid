//! Financial periods gate posting; local actor is rechecked in the write transaction.
use crate::{AppState, db_open, fail};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

pub fn init_schema(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS financial_periods(
          id INTEGER PRIMARY KEY, name TEXT NOT NULL, start_date TEXT NOT NULL,
          end_date TEXT NOT NULL, status TEXT NOT NULL CHECK(status IN ('open','closed')),
          created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE TABLE IF NOT EXISTS period_events(
          id INTEGER PRIMARY KEY, period_id INTEGER NOT NULL REFERENCES financial_periods(id) ON DELETE RESTRICT,
          event TEXT NOT NULL CHECK(event IN ('created','closed','reopened')),
          occurred_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE TRIGGER IF NOT EXISTS financial_periods_no_delete BEFORE DELETE ON financial_periods
          BEGIN SELECT RAISE(ABORT,'financial periods cannot be deleted'); END;
        CREATE TRIGGER IF NOT EXISTS financial_periods_no_dates_update BEFORE UPDATE OF name,start_date,end_date ON financial_periods
          BEGIN SELECT RAISE(ABORT,'period identity and dates are immutable'); END;
        CREATE TRIGGER IF NOT EXISTS period_events_no_update BEFORE UPDATE ON period_events
          BEGIN SELECT RAISE(ABORT,'period history is immutable'); END;
        CREATE TRIGGER IF NOT EXISTS period_events_no_delete BEFORE DELETE ON period_events
          BEGIN SELECT RAISE(ABORT,'period history is immutable'); END;
    "#)?;
    let has: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('journal_entries') WHERE name='period_id')",
        [],
        |r| r.get(0),
    )?;
    if !has {
        db.execute_batch("ALTER TABLE journal_entries ADD COLUMN period_id INTEGER REFERENCES financial_periods(id) ON DELETE RESTRICT;")?;
    }
    let has_count: bool=db.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('period_events') WHERE name='unposted_count')",[],|r|r.get(0))?;
    if !has_count {
        db.execute_batch("ALTER TABLE period_events ADD COLUMN unposted_count INTEGER;")?;
    }
    let has_actor: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('period_events') WHERE name='actor_id')",
        [],
        |r| r.get(0),
    )?;
    if !has_actor {
        db.execute_batch("ALTER TABLE period_events ADD COLUMN actor_id INTEGER REFERENCES identity_users(id) ON DELETE RESTRICT;")?;
    }
    Ok(())
}
fn valid_date(date: &str) -> bool {
    date.len() == 10
        && NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .is_ok_and(|d| d.format("%Y-%m-%d").to_string() == date)
}
#[derive(Serialize)]
struct Period {
    id: i64,
    name: String,
    start_date: String,
    end_date: String,
    status: String,
    posted: i64,
    unposted: i64,
    unassigned: i64,
    events: i64,
}
pub async fn list(State(s): State<AppState>) -> Response {
    let result = (|| -> rusqlite::Result<Vec<Period>> {
        let db = db_open(&s.db)?;
        let mut st=db.prepare("SELECT p.id,p.name,p.start_date,p.end_date,p.status,(SELECT COUNT(*) FROM journal_entries e WHERE e.period_id=p.id AND e.finalized=1),(SELECT COUNT(*) FROM drafts d WHERE d.date BETWEEN p.start_date AND p.end_date AND NOT EXISTS (SELECT 1 FROM journal_entries e WHERE e.draft_id=d.id AND e.finalized=1)),(SELECT COUNT(*) FROM journal_entries e WHERE e.period_id IS NULL AND e.finalized=1 AND e.entry_date BETWEEN p.start_date AND p.end_date),(SELECT COUNT(*) FROM period_events ev WHERE ev.period_id=p.id) FROM financial_periods p ORDER BY p.start_date DESC")?;
        let rows = st.query_map([], |r| {
            Ok(Period {
                id: r.get(0)?,
                name: r.get(1)?,
                start_date: r.get(2)?,
                end_date: r.get(3)?,
                status: r.get(4)?,
                posted: r.get(5)?,
                unposted: r.get(6)?,
                unassigned: r.get(7)?,
                events: r.get(8)?,
            })
        })?;
        rows.collect()
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!(%e,"period list failed");
            fail(StatusCode::INTERNAL_SERVER_ERROR, "تعذر تحميل الفترات.")
        }
    }
}
#[derive(Serialize)]
struct PeriodEvent {
    id: i64,
    event: String,
    occurred_at: String,
    unposted_count: Option<i64>,
    actor: Option<String>,
}
pub async fn events(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    if id <= 0 {
        return fail(StatusCode::BAD_REQUEST, "معرّف الفترة غير صالح.");
    }
    let result = (|| -> Result<Option<Vec<PeriodEvent>>, rusqlite::Error> {
        let db = db_open(&s.db)?;
        let exists: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM financial_periods WHERE id=?1)",
            [id],
            |r| r.get(0),
        )?;
        if !exists {
            return Ok(None);
        }
        let mut st = db.prepare(
            "SELECT e.id,e.event,e.occurred_at,e.unposted_count,u.username FROM period_events e LEFT JOIN identity_users u ON u.id=e.actor_id WHERE e.period_id=?1 ORDER BY e.id",
        )?;
        let records = st
            .query_map([id], |r| {
                Ok(PeriodEvent {
                    id: r.get(0)?,
                    event: r.get(1)?,
                    occurred_at: r.get(2)?,
                    unposted_count: r.get(3)?,
                    actor: r.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(records))
    })();
    match result {
        Ok(Some(rows)) => Json(rows).into_response(),
        Ok(None) => fail(StatusCode::NOT_FOUND, "الفترة غير موجودة."),
        Err(e) => {
            tracing::error!(%e,"period events read failed");
            fail(StatusCode::INTERNAL_SERVER_ERROR, "تعذر تحميل سجل الفترة.")
        }
    }
}
#[derive(Deserialize)]
pub struct PeriodInput {
    name: String,
    start_date: String,
    end_date: String,
}
pub async fn create(
    State(s): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(p): Json<PeriodInput>,
) -> Response {
    if !s.experimental_posting {
        return fail(StatusCode::FORBIDDEN, "إدارة الفترات الاختبارية معطلة.");
    }
    if p.name.trim().is_empty()
        || p.name.chars().count() > 100
        || p.name.chars().any(char::is_control)
    {
        return fail(
            StatusCode::UNPROCESSABLE_ENTITY,
            "أدخل اسم فترة لا يزيد عن 100 حرف.",
        );
    }
    if !valid_date(&p.start_date) || !valid_date(&p.end_date) || p.start_date > p.end_date {
        return fail(
            StatusCode::UNPROCESSABLE_ENTITY,
            "أدخل تواريخ صحيحة مرتبة للفترة.",
        );
    }
    enum Error {
        Overlap,
        Unauthorized,
        Storage(rusqlite::Error),
    }
    let result = (|| -> Result<i64, Error> {
        let mut db = db_open(&s.db).map_err(Error::Storage)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(Error::Storage)?;
        if !crate::identity::is_current_admin(&tx, &actor).map_err(Error::Storage)? {
            return Err(Error::Unauthorized);
        }
        let overlap:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM financial_periods WHERE start_date<=?1 AND end_date>=?2)",params![p.end_date,p.start_date],|r|r.get(0)).map_err(Error::Storage)?;
        if overlap {
            return Err(Error::Overlap);
        }
        tx.execute("INSERT INTO financial_periods(name,start_date,end_date,status) VALUES(?1,?2,?3,'open')",params![p.name.trim(),p.start_date,p.end_date]).map_err(Error::Storage)?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO period_events(period_id,event,actor_id) VALUES(?1,'created',?2)",
            params![id, actor.id],
        )
        .map_err(Error::Storage)?;
        tx.commit().map_err(Error::Storage)?;
        Ok(id)
    })();
    match result {
        Ok(id) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"id":id,"status":"open"})),
        )
            .into_response(),
        Err(Error::Unauthorized) => fail(
            StatusCode::UNAUTHORIZED,
            "انتهت الجلسة أو أُلغيت الصلاحية قبل إنشاء الفترة.",
        ),
        Err(Error::Overlap) => fail(
            StatusCode::CONFLICT,
            "الفترة تتداخل مع فترة مسجلة، شاملة يوم البداية والنهاية.",
        ),
        Err(Error::Storage(e)) => {
            tracing::error!(%e,"period creation failed");
            fail(StatusCode::INTERNAL_SERVER_ERROR, "تعذر إنشاء الفترة.")
        }
    }
}
#[derive(Deserialize)]
pub struct Transition {
    expected_status: String,
    approved: bool,
    allow_unposted: bool,
}
pub async fn transition(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(input): Json<Transition>,
) -> Response {
    if !s.experimental_posting {
        return fail(StatusCode::FORBIDDEN, "إدارة الفترات الاختبارية معطلة.");
    }
    if id <= 0 || !["open", "closed"].contains(&input.expected_status.as_str()) || !input.approved {
        return fail(
            StatusCode::UNPROCESSABLE_ENTITY,
            "حدد الحالة المتوقعة وأكد العملية صراحة.",
        );
    }
    enum Error {
        Missing,
        Unauthorized,
        Stale,
        Unposted(i64),
        Storage(rusqlite::Error),
    }
    let result = (|| -> Result<&'static str, Error> {
        let mut db = db_open(&s.db).map_err(Error::Storage)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(Error::Storage)?;
        if !crate::identity::is_current_admin(&tx, &actor).map_err(Error::Storage)? {
            return Err(Error::Unauthorized);
        }
        let old: Option<(String, String, String)> = tx
            .query_row(
                "SELECT status,start_date,end_date FROM financial_periods WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(Error::Storage)?;
        let (status, start, end) = old.ok_or(Error::Missing)?;
        if status != input.expected_status {
            return Err(Error::Stale);
        }
        let (next, event, unposted_count) = if status == "open" {
            let count:i64=tx.query_row("SELECT COUNT(*) FROM drafts d WHERE d.date BETWEEN ?1 AND ?2 AND NOT EXISTS(SELECT 1 FROM journal_entries e WHERE e.draft_id=d.id AND e.finalized=1)",params![start,end],|r|r.get(0)).map_err(Error::Storage)?;
            if count > 0 && !input.allow_unposted {
                return Err(Error::Unposted(count));
            }
            ("closed", "closed", Some(count))
        } else {
            ("open", "reopened", None)
        };
        tx.execute(
            "UPDATE financial_periods SET status=?1 WHERE id=?2",
            params![next, id],
        )
        .map_err(Error::Storage)?;
        tx.execute(
            "INSERT INTO period_events(period_id,event,unposted_count,actor_id) VALUES(?1,?2,?3,?4)",
            params![id, event, unposted_count,actor.id],
        )
        .map_err(Error::Storage)?;
        tx.commit().map_err(Error::Storage)?;
        Ok(next)
    })();
    match result {Ok(status)=>Json(serde_json::json!({"status":status})).into_response(),Err(Error::Unauthorized)=>fail(StatusCode::UNAUTHORIZED,"انتهت الجلسة أو أُلغيت الصلاحية قبل تغيير الفترة."),Err(Error::Missing)=>fail(StatusCode::NOT_FOUND,"الفترة غير موجودة."),Err(Error::Stale)=>fail(StatusCode::CONFLICT,"تغيرت حالة الفترة؛ حدّث القائمة وأعد المراجعة."),Err(Error::Unposted(n))=>(StatusCode::CONFLICT,Json(serde_json::json!({"error":"توجد مسودات غير مرحلة؛ راجعها أو أكد الإقفال رغم وجودها.","unposted":n}))).into_response(),Err(Error::Storage(e))=>{tracing::error!(%e,"period transition failed");fail(StatusCode::INTERNAL_SERVER_ERROR,"تعذر تغيير حالة الفترة.")}}
}
// Called inside the same IMMEDIATE transaction as posting to avoid racing a close.
pub fn open_period(db: &Connection, date: &str) -> Result<Option<i64>, rusqlite::Error> {
    db.query_row("SELECT p.id FROM financial_periods p WHERE p.status='open' AND ?1 BETWEEN p.start_date AND p.end_date AND NOT EXISTS(SELECT 1 FROM finance_period_context c WHERE c.period_id=p.id)",[date],|r|r.get(0)).optional()
}

#[cfg(test)]
mod tests;

mod scoped;
pub use scoped::{create as create_scoped, list as list_scoped, transition as transition_scoped};
