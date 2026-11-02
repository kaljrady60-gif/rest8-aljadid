//! Experimental, opt-in ledger with local actor checks; not a production accounting system.
use crate::{AppState, db_open, fail, read_draft};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Deserialize;

pub fn init_schema(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS journal_entries(
            id INTEGER PRIMARY KEY,
            draft_id INTEGER NOT NULL UNIQUE REFERENCES drafts(id) ON DELETE RESTRICT,
            description TEXT NOT NULL, entry_date TEXT NOT NULL, currency TEXT NOT NULL,
            total TEXT NOT NULL, finalized INTEGER NOT NULL DEFAULT 0 CHECK(finalized IN (0,1)),
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE TABLE IF NOT EXISTS journal_lines(
            entry_id INTEGER NOT NULL REFERENCES journal_entries(id) ON DELETE RESTRICT,
            line_no INTEGER NOT NULL CHECK(line_no>=0),
            account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
            account_snapshot TEXT NOT NULL, debit TEXT NOT NULL, credit TEXT NOT NULL,
            PRIMARY KEY(entry_id,line_no)
        );
        CREATE TRIGGER IF NOT EXISTS journal_entries_no_update BEFORE UPDATE ON journal_entries
            WHEN OLD.finalized=1 BEGIN SELECT RAISE(ABORT,'posted entries are immutable'); END;
        CREATE TRIGGER IF NOT EXISTS journal_entries_no_delete BEFORE DELETE ON journal_entries
            BEGIN SELECT RAISE(ABORT,'journal entries cannot be deleted'); END;
        CREATE TRIGGER IF NOT EXISTS journal_lines_no_update BEFORE UPDATE ON journal_lines
            BEGIN SELECT RAISE(ABORT,'journal lines are immutable'); END;
        CREATE TRIGGER IF NOT EXISTS journal_lines_no_delete BEFORE DELETE ON journal_lines
            BEGIN SELECT RAISE(ABORT,'journal lines cannot be deleted'); END;
        CREATE TRIGGER IF NOT EXISTS journal_lines_no_late_insert BEFORE INSERT ON journal_lines
            WHEN (SELECT finalized FROM journal_entries WHERE id=NEW.entry_id) != 0
            BEGIN SELECT RAISE(ABORT,'posted entries are immutable'); END;
    "#,
    )?;
    let has_actor: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('journal_entries') WHERE name='posted_by')",
        [],
        |r| r.get(0),
    )?;
    if !has_actor {
        db.execute_batch("ALTER TABLE journal_entries ADD COLUMN posted_by INTEGER REFERENCES identity_users(id) ON DELETE RESTRICT;")?;
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct Approval {
    approved: bool,
}
pub async fn config(State(s): State<AppState>) -> Json<serde_json::Value> {
    Json(
        serde_json::json!({"experimental_posting":s.experimental_posting,"authentication":!s.preview_no_login,"warning": if s.preview_no_login {"المعاينة مفتوحة دون تسجيل دخول، والبيانات مؤقتة والترحيل معطل؛ لا تستخدم بيانات حقيقية."} else {"تجربة فقط: الصلاحيات المحلية محدودة ولا يوجد عزل شركات أو إقفال مالي نهائي أو نسخ احتياطي، ولا تستخدم بيانات حقيقية"}}),
    )
}

pub async fn post(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(approval): Json<Approval>,
) -> Response {
    if !s.experimental_posting {
        return fail(StatusCode::FORBIDDEN, "الترحيل التجريبي معطل على الخادم.");
    }
    if id <= 0 {
        return fail(StatusCode::BAD_REQUEST, "رقم المسودة غير صالح.");
    }
    if !approval.approved {
        return fail(
            StatusCode::UNPROCESSABLE_ENTITY,
            "يلزم اعتماد صريح بعد مراجعة المسودة.",
        );
    }
    enum PostError {
        Missing,
        Unauthorized,
        Conflict(i64),
        Invalid(String),
        Storage(Box<dyn std::error::Error>),
    }
    let result = (|| -> Result<i64, PostError> {
        let mut db = db_open(&s.db).map_err(|e| PostError::Storage(Box::new(e)))?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| PostError::Storage(Box::new(e)))?;
        if !crate::identity::is_current_admin(&tx, &actor)
            .map_err(|e| PostError::Storage(Box::new(e)))?
        {
            return Err(PostError::Unauthorized);
        }
        let existing: Option<i64> = tx
            .query_row(
                "SELECT id FROM journal_entries WHERE draft_id=?1 AND finalized=1",
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| PostError::Storage(Box::new(e)))?;
        if let Some(existing) = existing {
            return Err(PostError::Conflict(existing));
        }
        let scoped: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM finance_voucher_context c JOIN financial_vouchers v ON v.id=c.voucher_id WHERE v.draft_id=?1)",
            [id], |r| r.get(0),
        ).map_err(|e| PostError::Storage(Box::new(e)))?;
        if scoped {
            return Err(PostError::Invalid(
                "لا يمكن ترحيل السند المرتبط حتى يكتمل عزل الشركة والفترة والاعتماد وسجل التدقيق."
                    .into(),
            ));
        }
        let draft = read_draft(&tx, id)
            .map_err(PostError::Storage)?
            .ok_or(PostError::Missing)?;
        let period_id = crate::periods::open_period(&tx, &draft.date)
            .map_err(|e| PostError::Storage(Box::new(e)))?
            .ok_or_else(|| {
                PostError::Invalid(
                    "لا توجد فترة مفتوحة تغطي تاريخ القيد؛ أنشئ الفترة أو راجع حالتها.".into(),
                )
            })?;
        if draft.lines.iter().any(|l| l.account_id.is_none()) {
            return Err(PostError::Invalid(
                "لا يمكن ترحيل مسودة قديمة غير مرتبطة بدليل الحسابات.".into(),
            ));
        }
        for line in &draft.lines {
            let account: Option<(String, String)> = tx
                .query_row(
                    "SELECT kind,currency FROM accounts WHERE id=?1",
                    [line.account_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(|e| PostError::Storage(Box::new(e)))?;
            if !matches!(account,Some((ref k,ref c)) if k=="detail" && *c==draft.currency) {
                return Err(PostError::Invalid(
                    "تغير أحد الحسابات أو عملته؛ راجع المسودة قبل الترحيل.".into(),
                ));
            }
        }
        tx.execute("INSERT INTO journal_entries(draft_id,description,entry_date,currency,total,period_id,posted_by) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![id,draft.description,draft.date,draft.currency,draft.totals.debit,period_id,actor.id]).map_err(|e|PostError::Storage(Box::new(e)))?;
        let entry = tx.last_insert_rowid();
        for (n, line) in draft.lines.iter().enumerate() {
            tx.execute("INSERT INTO journal_lines(entry_id,line_no,account_id,account_snapshot,debit,credit) VALUES(?1,?2,?3,?4,?5,?6)",params![entry,n as i64,line.account_id,line.account,line.debit,line.credit]).map_err(|e|PostError::Storage(Box::new(e)))?;
        }
        tx.execute(
            "UPDATE journal_entries SET finalized=1 WHERE id=?1",
            [entry],
        )
        .map_err(|e| PostError::Storage(Box::new(e)))?;
        tx.commit().map_err(|e| PostError::Storage(Box::new(e)))?;
        Ok(entry)
    })();
    match result {
        Ok(entry)=> (StatusCode::CREATED,Json(serde_json::json!({"entry_id":entry,"status":"posted_experimental"}))).into_response(),
        Err(PostError::Unauthorized)=>fail(StatusCode::UNAUTHORIZED,"انتهت الجلسة أو أُلغيت الصلاحية قبل الترحيل."),
        Err(PostError::Missing)=>fail(StatusCode::NOT_FOUND,"المسودة غير موجودة."),
        Err(PostError::Conflict(entry))=> (StatusCode::CONFLICT,Json(serde_json::json!({"error":"هذه المسودة مُرحّلة سابقًا؛ لم يُنشأ قيد مكرر.","entry_id":entry}))).into_response(),
        Err(PostError::Invalid(message))=>fail(StatusCode::UNPROCESSABLE_ENTITY,message),
        Err(PostError::Storage(e))=>{tracing::error!(%e,id,"experimental posting failed");fail(StatusCode::INTERNAL_SERVER_ERROR,"تعذر التحقق أو الترحيل؛ راجع سجل الخادم وقائمة القيود قبل إعادة المحاولة.")}
    }
}

mod reports;
#[cfg(test)]
use reports::{Filters, check_filters, check_report_size};
pub use reports::{account_ledger, entries, trial};
#[cfg(test)]
mod tests;
