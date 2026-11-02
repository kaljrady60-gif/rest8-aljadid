//! Journal draft reads, validation and atomic draft construction shared with vouchers.
use crate::{AppState, db_open, domain, fail};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use domain::{DraftInput, Totals};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

#[derive(Serialize)]
struct DraftView {
    id: i64,
    description: String,
    date: String,
    currency: String,
    total: String,
    status: &'static str,
}
#[derive(Serialize)]
pub(crate) struct DraftDetail {
    pub(crate) id: i64,
    pub(crate) description: String,
    pub(crate) date: String,
    pub(crate) currency: String,
    pub(crate) lines: Vec<domain::LineInput>,
    pub(crate) totals: Totals,
    pub(crate) status: &'static str,
}
#[derive(Serialize)]
struct Created {
    id: i64,
    totals: Totals,
    status: &'static str,
}
pub(crate) async fn list(State(s): State<AppState>) -> Response {
    let r = (|| -> Result<Vec<DraftView>, rusqlite::Error> {
        let db = db_open(&s.db)?;
        let mut st = db.prepare(
            "SELECT d.id,d.description,d.date,d.currency,d.total,EXISTS(SELECT 1 FROM journal_entries e WHERE e.draft_id=d.id AND e.finalized=1) FROM drafts d ORDER BY d.id DESC LIMIT 100",
        )?;
        let rows = st.query_map([], |r| {
            Ok(DraftView {
                id: r.get(0)?,
                description: r.get(1)?,
                date: r.get(2)?,
                currency: r.get(3)?,
                total: r.get(4)?,
                status: if r.get::<_, bool>(5)? {
                    "مرحّل تجريبيًا"
                } else {
                    "مسودة"
                },
            })
        })?;
        rows.collect()
    })();
    match r {
        Ok(x) => Json(x).into_response(),
        Err(e) => {
            tracing::error!(%e,"list failed");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "تعذر تحميل المسودات. راجع سجل الخادم.",
            )
        }
    }
}
pub(crate) fn read_draft(
    db: &Connection,
    id: i64,
) -> Result<Option<DraftDetail>, Box<dyn std::error::Error>> {
    let row = db
        .query_row(
            "SELECT description,date,currency,total,lines_json,lines_count FROM drafts WHERE id=?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                ))
            },
        )
        .optional()?;
    let Some((description, date, currency, stored_total, lines_json, lines_count)) = row else {
        return Ok(None);
    };
    let lines = match lines_count {
        Some(expected) => {
            if !(2..=50).contains(&expected) || lines_json != "[]" {
                return Err(std::io::Error::other("inconsistent draft line metadata").into());
            }
            let mut st=db.prepare("SELECT l.line_no,l.account_id,l.account_snapshot,l.debit,l.credit,a.id FROM draft_lines l LEFT JOIN accounts a ON a.id=l.account_id WHERE l.draft_id=?1 ORDER BY l.line_no")?;
            let rows = st.query_map([id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    domain::LineInput {
                        account_id: Some(r.get(1)?),
                        account: r.get(2)?,
                        debit: r.get(3)?,
                        credit: r.get(4)?,
                    },
                    r.get::<_, Option<i64>>(5)?,
                ))
            })?;
            let rows: Vec<_> = rows.collect::<Result<_, _>>()?;
            if rows.len() != expected as usize
                || rows
                    .iter()
                    .enumerate()
                    .any(|(i, (n, _, a))| *n != i as i64 || a.is_none())
            {
                return Err(std::io::Error::other(
                    "stored draft line count, order, or account reference mismatch",
                )
                .into());
            }
            rows.into_iter().map(|(_, line, _)| line).collect()
        }
        None => {
            let has_lines: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM draft_lines WHERE draft_id=?1)",
                [id],
                |r| r.get(0),
            )?;
            if has_lines {
                return Err(
                    std::io::Error::other("conflicting legacy and normalized draft lines").into(),
                );
            }
            let mut legacy: Vec<domain::LineInput> = serde_json::from_str(&lines_json)?;
            for line in &mut legacy {
                line.account_id = None;
            }
            legacy
        }
    };
    let input = DraftInput {
        description,
        date,
        currency,
        lines,
    };
    let totals = domain::validate(&input).map_err(std::io::Error::other)?;
    // المقارنة رقمية: صيغة المبلغ النصية قد تختلف في المنازل والمبلغ واحد.
    let stored_dec = stored_total
        .trim()
        .parse::<rust_decimal::Decimal>()
        .map_err(|_| std::io::Error::other("stored draft total is corrupt"))?;
    let totals_dec = totals
        .debit
        .trim()
        .parse::<rust_decimal::Decimal>()
        .map_err(|_| std::io::Error::other("computed draft total is corrupt"))?;
    if totals_dec != stored_dec {
        return Err(std::io::Error::other("stored draft total mismatch").into());
    }
    Ok(Some(DraftDetail {
        id,
        description: input.description,
        date: input.date,
        currency: input.currency,
        lines: input.lines,
        totals,
        status: if db.query_row(
            "SELECT EXISTS(SELECT 1 FROM journal_entries WHERE draft_id=?1 AND finalized=1)",
            [id],
            |r| r.get::<_, bool>(0),
        )? {
            "posted_experimental"
        } else {
            "draft"
        },
    }))
}

// Independent read path. Corrupt stored data is reported, never rendered as an empty draft.
pub(crate) async fn draft_detail(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    if id <= 0 {
        return fail(StatusCode::BAD_REQUEST, "رقم المسودة غير صالح.");
    }
    let result = db_open(&s.db)
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
        .and_then(|db| read_draft(&db, id));
    match result {
        Ok(Some(detail)) => Json(detail).into_response(),
        Ok(None) => fail(StatusCode::NOT_FOUND, "المسودة غير موجودة."),
        Err(e) => {
            tracing::error!(%e, id, "draft detail read or validation failed");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "تعذر عرض المسودة أو التحقق من بياناتها. راجع سجل الخادم.",
            )
        }
    }
}
#[derive(Debug)]
pub(crate) enum SaveError {
    Invalid(String),
    Unauthorized,
    Storage(Box<dyn std::error::Error>),
}
// Shared journal-draft construction: caller controls the transaction and commit.
pub(crate) fn save_draft_in_tx(
    tx: &rusqlite::Transaction<'_>,
    input: &mut DraftInput,
) -> Result<(i64, Totals), SaveError> {
    save_scoped_draft_in_tx(tx, input, None)
}

pub(crate) fn save_scoped_draft_in_tx(
    tx: &rusqlite::Transaction<'_>,
    input: &mut DraftInput,
    company: Option<i64>,
) -> Result<(i64, Totals), SaveError> {
    if !(2..=50).contains(&input.lines.len()) {
        return Err(SaveError::Invalid(
            "يجب أن يتكون القيد من سطرين إلى 50 سطرًا.".into(),
        ));
    }
    for (index, line) in input.lines.iter_mut().enumerate() {
        let id = line.account_id.filter(|x| *x > 0).ok_or_else(|| {
            SaveError::Invalid(format!("اختر حسابًا تفصيليًا من الدليل للسطر {}.", index + 1))
        })?;
        let owner: Option<i64> = tx
            .query_row(
                "SELECT company_id FROM finance_account_scopes WHERE account_id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| SaveError::Storage(Box::new(e)))?;
        if owner != company {
            return Err(SaveError::Invalid(format!(
                "نطاق الشركة للحساب في السطر {} لا يطابق المستند؛ لا تستخدم حسابًا مخصصًا في قيد غير مرتبط.",
                index + 1
            )));
        }
        let account: Option<(String, String, String)> = tx
            .query_row(
                "SELECT name,kind,currency FROM accounts WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|e| SaveError::Storage(Box::new(e)))?;
        match account {
            None => {
                return Err(SaveError::Invalid(format!(
                    "الحساب في السطر {} غير موجود في الدليل.",
                    index + 1
                )));
            }
            Some((_, kind, _)) if kind != "detail" => {
                return Err(SaveError::Invalid(format!(
                    "الحساب في السطر {} ليس تفصيليًا.",
                    index + 1
                )));
            }
            Some((_, _, currency)) if currency != input.currency => {
                return Err(SaveError::Invalid(format!(
                    "عملة حساب السطر {} لا تطابق عملة المسودة في هذه النسخة.",
                    index + 1
                )));
            }
            Some((name, _, _)) => line.account = name,
        }
    }
    let totals = domain::validate(input).map_err(SaveError::Invalid)?;
    tx.execute("INSERT INTO drafts (description,date,currency,total,lines_json,lines_count) VALUES (?1,?2,?3,?4,'[]',?5)",params![input.description.trim(),input.date,input.currency,totals.debit,input.lines.len() as i64]).map_err(|e|SaveError::Storage(Box::new(e)))?;
    let id = tx.last_insert_rowid();
    for (index, line) in input.lines.iter().enumerate() {
        let account_id = line
            .account_id
            .ok_or_else(|| SaveError::Invalid("معرّف الحساب مفقود بعد التحقق.".into()))?;
        tx.execute("INSERT INTO draft_lines(draft_id,line_no,account_id,account_snapshot,debit,credit) VALUES(?1,?2,?3,?4,?5,?6)",params![id,index as i64,account_id,line.account,line.debit,line.credit]).map_err(|e|SaveError::Storage(Box::new(e)))?;
    }
    Ok((id, totals))
}

// New drafts resolve account names from the account directory on the server. Historical
// free-text drafts remain readable; names saved with IDs are historical snapshots only.
pub(crate) async fn create(
    State(s): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(mut input): Json<DraftInput>,
) -> Response {
    if !(2..=50).contains(&input.lines.len()) {
        return fail(
            StatusCode::UNPROCESSABLE_ENTITY,
            "يجب أن يتكون القيد من سطرين إلى 50 سطرًا.",
        );
    }
    let r = (|| -> Result<(i64, Totals), SaveError> {
        let mut db = db_open(&s.db).map_err(|e| SaveError::Storage(Box::new(e)))?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| SaveError::Storage(Box::new(e)))?;
        if !s.preview_no_login
            && !crate::identity::is_current_admin(&tx, &actor)
                .map_err(|e| SaveError::Storage(Box::new(e)))?
        {
            return Err(SaveError::Unauthorized);
        }
        let (id, totals) = save_draft_in_tx(&tx, &mut input)?;
        tx.commit().map_err(|e| SaveError::Storage(Box::new(e)))?;
        Ok((id, totals))
    })();
    match r {
        Ok((id, totals)) => (
            StatusCode::CREATED,
            Json(Created {
                id,
                totals,
                status: "draft",
            }),
        )
            .into_response(),
        Err(SaveError::Invalid(e)) => fail(StatusCode::UNPROCESSABLE_ENTITY, e),
        Err(SaveError::Unauthorized) => fail(
            StatusCode::FORBIDDEN,
            "انتهت الجلسة أو تغيرت صلاحياتها قبل حفظ المسودة.",
        ),
        Err(SaveError::Storage(e)) => {
            tracing::error!(%e,"save failed");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "تعذر حفظ المسودة. راجع سجل الخادم.",
            )
        }
    }
}

#[cfg(test)]
mod tests;
