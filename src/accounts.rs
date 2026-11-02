//! Experimental account directory. This is not an accounting ledger or a posting engine.
use crate::{AppState, db_open, fail};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct AccountInput {
    code: String,
    name: String,
    parent_id: Option<i64>,
    category: String,
    kind: String,
    nature: String,
    currency: String,
}
#[derive(Debug, Serialize)]
pub struct Account {
    id: i64,
    code: String,
    name: String,
    parent_id: Option<i64>,
    category: String,
    kind: String,
    nature: String,
    currency: String,
    company_id: Option<i64>,
}
fn validate(a: &AccountInput) -> Result<(), &'static str> {
    let code = a.code.trim();
    let name = a.name.trim();
    if code.is_empty()
        || code.chars().count() > 30
        || !code
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err("رقم الحساب مطلوب (حتى 30 حرفًا أو رقمًا، والنقاط والشرطات فقط).");
    }
    if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
        return Err("اسم الحساب مطلوب ولا يزيد على 100 حرف دون رموز تحكم.");
    }
    if !["asset", "liability", "equity", "revenue", "expense"].contains(&a.category.as_str()) {
        return Err("تصنيف الحساب غير صالح.");
    }
    if !["group", "detail"].contains(&a.kind.as_str()) {
        return Err("نوع الحساب غير صالح.");
    }
    if !["debit", "credit"].contains(&a.nature.as_str()) {
        return Err("طبيعة الحساب غير صالحة.");
    }
    if !crate::domain::supported_currency(&a.currency) {
        return Err("العملة غير مدعومة في هذه النسخة التجريبية.");
    }
    if a.parent_id.is_some_and(|id| id <= 0) {
        return Err("معرّف الحساب الأب غير صالح.");
    }
    Ok(())
}
pub async fn list(State(s): State<AppState>) -> Response {
    let result = (|| -> rusqlite::Result<Vec<Account>> {
        let db = db_open(&s.db)?;
        let mut st = db.prepare("SELECT a.id,COALESCE(c.code,a.code),a.name,a.parent_id,a.category,a.kind,a.nature,a.currency,s.company_id FROM accounts a LEFT JOIN finance_account_scopes s ON s.account_id=a.id LEFT JOIN finance_company_account_codes c ON c.account_id=a.id ORDER BY COALESCE(c.code,a.code),a.id")?;
        let rows = st.query_map([], |r| {
            Ok(Account {
                id: r.get(0)?,
                code: r.get(1)?,
                name: r.get(2)?,
                parent_id: r.get(3)?,
                category: r.get(4)?,
                kind: r.get(5)?,
                nature: r.get(6)?,
                currency: r.get(7)?,
                company_id: r.get(8)?,
            })
        })?;
        rows.collect()
    })();
    match result {
        Ok(accounts) => Json(accounts).into_response(),
        Err(e) => {
            tracing::error!(%e,"account list failed");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "تعذر تحميل دليل الحسابات. راجع سجل الخادم.",
            )
        }
    }
}
pub async fn create(
    State(s): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(a): Json<AccountInput>,
) -> Response {
    if let Err(msg) = validate(&a) {
        return fail(StatusCode::UNPROCESSABLE_ENTITY, msg);
    }
    let result = (|| -> Result<i64, CreateError> {
        let mut db = db_open(&s.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !s.preview_no_login && !crate::identity::is_current_admin(&tx, &actor)? {
            return Err(CreateError::Unauthorized);
        }
        if let Some(parent) = a.parent_id {
            let p: Option<(String, String)> = tx
                .query_row(
                    "SELECT kind,category FROM accounts WHERE id=?1",
                    [parent],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            match p {
                None => return Err(CreateError::Invalid("الحساب الأب غير موجود.")),
                Some((kind, _)) if kind != "group" => {
                    return Err(CreateError::Invalid("لا يمكن إضافة فرع تحت حساب تفصيلي."));
                }
                Some((_, category)) if category != a.category => {
                    return Err(CreateError::Invalid(
                        "يجب أن يتطابق تصنيف الحساب مع الحساب الأب في هذه النسخة.",
                    ));
                }
                _ => {}
            }
        }
        let duplicate: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE code=?1)",
            [a.code.trim()],
            |r| r.get(0),
        )?;
        if duplicate {
            return Err(CreateError::Conflict);
        }
        tx.execute("INSERT INTO accounts(code,name,parent_id,category,kind,nature,currency) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![a.code.trim(),a.name.trim(),a.parent_id,a.category,a.kind,a.nature,a.currency])?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    })();
    match result {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response(),
        Err(CreateError::Invalid(msg)) => fail(StatusCode::UNPROCESSABLE_ENTITY, msg),
        Err(CreateError::Conflict) => fail(StatusCode::CONFLICT, "رقم الحساب مستخدم بالفعل."),
        Err(CreateError::Unauthorized) => fail(
            StatusCode::FORBIDDEN,
            "انتهت الجلسة أو أُلغيت صلاحية المدير قبل حفظ الحساب.",
        ),
        Err(CreateError::Storage(e)) => {
            tracing::error!(%e,"account create failed");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "تعذر حفظ الحساب. راجع سجل الخادم.",
            )
        }
    }
}
// Limited rename only. The expected value prevents silently overwriting a concurrent rename.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameInput {
    name: String,
    expected_name: String,
}
pub async fn rename(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(a): Json<RenameInput>,
) -> Response {
    if id <= 0 {
        return fail(StatusCode::BAD_REQUEST, "معرّف الحساب غير صالح.");
    }
    let name = a.name.trim();
    if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
        return fail(
            StatusCode::UNPROCESSABLE_ENTITY,
            "اسم الحساب مطلوب ولا يزيد على 100 حرف دون رموز تحكم.",
        );
    }
    let result = (|| -> Result<(usize, bool), RenameError> {
        let mut db = db_open(&s.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !s.preview_no_login && !crate::identity::is_current_admin(&tx, &actor)? {
            return Err(RenameError::Unauthorized);
        }
        let changed = tx.execute(
            "UPDATE accounts SET name=?1 WHERE id=?2 AND name=?3",
            params![name, id, a.expected_name],
        )?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1)",
            [id],
            |r| r.get(0),
        )?;
        tx.commit()?;
        Ok((changed, exists))
    })();
    match result {
        Ok((1, _)) => Json(serde_json::json!({"id":id,"name":name})).into_response(),
        Ok((0, false)) => fail(StatusCode::NOT_FOUND, "الحساب غير موجود."),
        Ok((0, true)) => fail(
            StatusCode::CONFLICT,
            "تغير اسم الحساب منذ فتح النافذة؛ حدّث الدليل وراجع الاسم قبل إعادة المحاولة.",
        ),
        Ok(_) => fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "تعذّر التحقق من نتيجة التعديل.",
        ),
        Err(RenameError::Unauthorized) => fail(
            StatusCode::FORBIDDEN,
            "انتهت الجلسة أو أُلغيت صلاحية المدير قبل تعديل الحساب.",
        ),
        Err(RenameError::Storage(e)) => {
            tracing::error!(%e,id,"account rename failed");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "تعذر تعديل الحساب. راجع سجل الخادم.",
            )
        }
    }
}
#[derive(Debug)]
enum CreateError {
    Unauthorized,
    Invalid(&'static str),
    Conflict,
    Storage(rusqlite::Error),
}
impl From<rusqlite::Error> for CreateError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Storage(e)
    }
}
#[derive(Debug)]
enum RenameError {
    Unauthorized,
    Storage(rusqlite::Error),
}
impl From<rusqlite::Error> for RenameError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> AccountInput {
        AccountInput {
            code: "100".into(),
            name: "الأصول".into(),
            parent_id: None,
            category: "asset".into(),
            kind: "group".into(),
            nature: "debit".into(),
            currency: "YER".into(),
        }
    }
    #[test]
    fn validates_fields() {
        let mut a = input();
        assert!(validate(&a).is_ok());
        a.code = " ".into();
        assert!(validate(&a).is_err());
        a.code = "١٠٠".into();
        assert!(validate(&a).is_ok());
        a.name = "x\n".into();
        assert!(validate(&a).is_ok());
        a.name = "x\ny".into();
        assert!(validate(&a).is_err());
        a.name = "x".into();
        a.category = "bad".into();
        assert!(validate(&a).is_err());
    }
}

#[cfg(test)]
mod transaction_tests;

mod scoped;
pub use scoped::{create_scoped, list_scoped};
