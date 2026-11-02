//! Cash receipt/payment vouchers linked atomically to the single journal-draft path.
//! No separate ledger or balance is maintained here.
use crate::{AppState, SaveError, db_open, domain, fail, read_draft};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub fn init_schema(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS financial_vouchers (\
        id INTEGER PRIMARY KEY, draft_id INTEGER NOT NULL UNIQUE REFERENCES drafts(id) ON DELETE RESTRICT,\
        kind TEXT NOT NULL CHECK(kind IN ('receipt','payment')), party_name TEXT NOT NULL,\
        reference TEXT NOT NULL, request_id TEXT NOT NULL UNIQUE, request_hash BLOB NOT NULL,\
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);\
        CREATE TABLE IF NOT EXISTS voucher_sequences (\
        company_id INTEGER NOT NULL, kind TEXT NOT NULL, year TEXT NOT NULL,\
        last INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(company_id,kind,year));")?;
    // عمود الرقم التسلسلي للسندات القديمة (ترحيل آمن بلا فقد بيانات).
    let has_no: bool = db
        .prepare("PRAGMA table_info(financial_vouchers)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|name| name == "voucher_no");
    if !has_no {
        db.execute_batch("ALTER TABLE financial_vouchers ADD COLUMN voucher_no TEXT")?;
    }
    db.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_vouchers_no ON financial_vouchers(voucher_no)",
    )
}

/// الرقم التسلسلي التالي: `RV-2026-0001` للسندات المقبوضة و`PV-…` للمدفوعة.
/// التسلسل لكل شركة ولكل نوع ولكل سنة، ويُخصَّص داخل نفس معاملة الإدخال فلا يتكرر.
fn next_voucher_no(
    tx: &rusqlite::Transaction<'_>,
    company: i64,
    kind: &str,
    year: &str,
) -> rusqlite::Result<String> {
    tx.execute(
        "INSERT INTO voucher_sequences(company_id,kind,year,last) VALUES(?1,?2,?3,0)
         ON CONFLICT(company_id,kind,year) DO NOTHING",
        params![company, kind, year],
    )?;
    tx.execute(
        "UPDATE voucher_sequences SET last=last+1 WHERE company_id=?1 AND kind=?2 AND year=?3",
        params![company, kind, year],
    )?;
    let serial: i64 = tx.query_row(
        "SELECT last FROM voucher_sequences WHERE company_id=?1 AND kind=?2 AND year=?3",
        params![company, kind, year],
        |r| r.get(0),
    )?;
    let prefix = if kind == "receipt" { "RV" } else { "PV" };
    Ok(format!("{prefix}-{year}-{serial:04}"))
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VoucherInput {
    request_id: String,
    kind: String,
    date: String,
    currency: String,
    amount: String,
    cash_account_id: i64,
    other_account_id: i64,
    party_name: String,
    description: String,
    reference: String,
}
fn validate(i: &VoucherInput) -> Result<(), &'static str> {
    if i.request_id.len() != 32
        || !i
            .request_id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("معرّف الحفظ غير صالح؛ أعد فتح النافذة.");
    }
    if !["receipt", "payment"].contains(&i.kind.as_str()) {
        return Err("نوع السند غير صالح.");
    }
    if i.cash_account_id <= 0 || i.other_account_id <= 0 || i.cash_account_id == i.other_account_id
    {
        return Err("اختر حسابين تفصيليين مختلفين للنقدية والطرف.");
    }
    if i.party_name.trim().is_empty()
        || i.party_name.chars().count() > 120
        || i.party_name.chars().any(char::is_control)
    {
        return Err("اسم الطرف مطلوب ولا يزيد على 120 حرفًا دون رموز تحكم.");
    }
    if i.reference.chars().count() > 100 || i.reference.chars().any(char::is_control) {
        return Err("المرجع لا يزيد على 100 حرف دون رموز تحكم.");
    }
    Ok(())
}
#[derive(Debug)]
enum Error {
    Invalid(String),
    Conflict,
    Unauthorized,
    Storage(Box<dyn std::error::Error>),
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Storage(Box::new(e))
    }
}
impl From<SaveError> for Error {
    fn from(e: SaveError) -> Self {
        match e {
            SaveError::Invalid(s) => Self::Invalid(s),
            SaveError::Unauthorized => Self::Unauthorized,
            SaveError::Storage(e) => Self::Storage(e),
        }
    }
}

pub async fn create(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(i): Json<VoucherInput>,
) -> Response {
    if let Err(e) = validate(&i) {
        return fail(StatusCode::UNPROCESSABLE_ENTITY, e);
    }
    let result = (|| -> Result<(i64, i64, bool, bool), Error> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !state.preview_no_login && !crate::identity::is_current_admin(&tx, &actor)? {
            return Err(Error::Unauthorized);
        }
        let saved = save_voucher_in_tx(&tx, &i, None)?;
        let linked: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM finance_voucher_context WHERE voucher_id=?1)",
            [saved.0],
            |r| r.get(0),
        )?;
        if linked {
            return Err(Error::Conflict);
        }
        tx.commit()?;
        Ok(saved)
    })();
    match result {
        Ok((id, draft_id, created, posted)) => (
            if created {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            },
            Json(
                serde_json::json!({"id":id,"draft_id":draft_id,"status":if posted {"posted_experimental"} else {"draft"},"created":created}),
            ),
        )
            .into_response(),
        Err(Error::Invalid(e)) => fail(StatusCode::UNPROCESSABLE_ENTITY, e),
        Err(Error::Conflict) => fail(
            StatusCode::CONFLICT,
            "استخدم معرّف الحفظ نفسه مع بيانات مختلفة؛ أعد فتح السند.",
        ),
        Err(Error::Unauthorized) => fail(StatusCode::FORBIDDEN, "انتهت الجلسة أو تغيرت صلاحياتها قبل حفظ السند."),
        Err(Error::Storage(e)) => {
            tracing::error!(%e,"voucher save failed");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "تعذر حفظ السند؛ راجع القائمة قبل المحاولة مجددًا.",
            )
        }
    }
}
fn save_voucher_in_tx(
    tx: &rusqlite::Transaction<'_>,
    i: &VoucherInput,
    company: Option<i64>,
) -> Result<(i64, i64, bool, bool), Error> {
    let payload = serde_json::to_vec(i).map_err(|e| Error::Storage(Box::new(e)))?;
    let hash = Sha256::digest(&payload).to_vec();
    let existing: Option<(i64, i64, Vec<u8>, bool)> = tx
            .query_row(
                "SELECT v.id,v.draft_id,v.request_hash,EXISTS(SELECT 1 FROM journal_entries e WHERE e.draft_id=v.draft_id AND e.finalized=1) FROM financial_vouchers v WHERE v.request_id=?1",
                [&i.request_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
    if let Some((id, draft_id, previous, posted)) = existing {
        if previous != hash {
            return Err(Error::Conflict);
        }
        return Ok((id, draft_id, false, posted));
    }
    let receipt = i.kind == "receipt";
    let amount = i.amount.trim().to_owned();
    // البيان بالصيغة المرجعية للسندات: «الطرف — بيان السند».
    let narrative = crate::finance::narrative::compose_voucher(&i.party_name, &i.description);
    let mut draft = domain::DraftInput {
        description: narrative.clone(),
        date: i.date.clone(),
        currency: i.currency.clone(),
        lines: vec![
            domain::LineInput {
                account_id: Some(i.cash_account_id),
                account: String::new(),
                debit: if receipt {
                    amount.clone()
                } else {
                    String::new()
                },
                credit: if receipt {
                    String::new()
                } else {
                    amount.clone()
                },
            },
            domain::LineInput {
                account_id: Some(i.other_account_id),
                account: String::new(),
                debit: if receipt {
                    String::new()
                } else {
                    amount.clone()
                },
                credit: if receipt { amount } else { String::new() },
            },
        ],
    };
    let (draft_id, _) = crate::drafts::save_scoped_draft_in_tx(tx, &mut draft, company)?;
    let company_id = match company {
        Some(id) => id,
        None => 0,
    };
    let year = i.date.get(0..4).unwrap_or("0000").to_string();
    let voucher_no = next_voucher_no(tx, company_id, &i.kind, &year)?;
    // المرجع اليدوي يُحفظ كما هو، والرقم التسلسلي يُضاف دائمًا فلا يبقى سند بلا رقم.
    let reference = if i.reference.trim().is_empty() {
        voucher_no.clone()
    } else {
        i.reference.trim().to_string()
    };
    tx.execute("INSERT INTO financial_vouchers(draft_id,kind,party_name,reference,request_id,request_hash,voucher_no) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![draft_id,i.kind,i.party_name.trim(),reference,i.request_id,hash,voucher_no])?;
    let id = tx.last_insert_rowid();
    Ok((id, draft_id, true, false))
}

mod linked;
mod list;
mod scoped_read;
pub use linked::create as create_linked;
pub use list::list;
pub use scoped_read::{detail as scoped_detail, list as scoped_list};

#[cfg(test)]
mod tests;
