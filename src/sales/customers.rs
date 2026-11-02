//! إنشاء عميل: يُنشئ حساب ذمم تفصيليًا داخل دليل المالية ثم طرفًا بدور عميل.
//! لا يوجد جدول عملاء موازٍ — العملاء هم أطراف المالية.
use crate::AppState;
use crate::db_open;
use crate::finance::audit;
use crate::finance::scope::{FinanceError, Scope, error_response};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewCustomer {
    pub company_id: i64,
    pub name: String,
}


/// يُنشئ حسابًا تفصيليًا جديدًا تحت حساب أب (بالرمز المعروض للشركة) ويعيد معرّفه.
pub fn provision_account(
    tx: &rusqlite::Connection,
    company_id: i64,
    parent_code: &str,
    name: &str,
    category: &str,
    nature: &str,
) -> Result<i64, FinanceError> {
    let parent: Option<(i64, String)> = tx
        .query_row(
            "SELECT c.account_id, c.code FROM finance_company_account_codes c
             WHERE c.company_id=?1 AND c.code=?2",
            params![company_id, parent_code],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (parent_id, code_text) = parent.ok_or_else(|| {
        FinanceError::Invalid(format!("الحساب الأب {parent_code} غير موجود في الدليل."))
    })?;
    let currency: String = tx.query_row(
        "SELECT currency FROM accounts WHERE id=?1",
        [parent_id],
        |r| r.get(0),
    )?;
    let mut counter: i64 = 1;
    let (internal_code, display_code) = loop {
        let internal = format!("__c{company_id}_{parent_code}_{counter:03}");
        let display = format!("{code_text}-{counter:03}");
        let taken: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE code=?1)
                OR EXISTS(SELECT 1 FROM finance_company_account_codes WHERE company_id=?2 AND code=?3)",
            params![internal, company_id, display],
            |r| r.get(0),
        )?;
        if !taken {
            break (internal, display);
        }
        counter += 1;
        if counter > 1000 {
            return Err(FinanceError::Invalid("تعذّر توليد رمز حساب فرعي.".into()));
        }
    };
    tx.execute(
        "INSERT INTO accounts(code,name,parent_id,category,kind,nature,currency)
         VALUES(?1,?2,?3,?4,'detail',?5,?6)",
        params![internal_code, name, parent_id, category, nature, currency],
    )?;
    let account_id = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO finance_account_scopes(company_id,account_id) VALUES(?1,?2)",
        params![company_id, account_id],
    )?;
    tx.execute(
        "INSERT INTO finance_company_account_codes(company_id,account_id,code) VALUES(?1,?2,?3)",
        params![company_id, account_id, display_code],
    )?;
    Ok(account_id)
}

/// ينشئ حسابًا تفصيليًا تحت «العملاء» ثم يسجّل الطرف. يُعاد معرّف الطرف.
pub fn create_customer(
    tx: &rusqlite::Connection,
    company_id: i64,
    name: &str,
) -> Result<i64, FinanceError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(FinanceError::Invalid("اسم العميل غير صالح.".into()));
    }
    let parent: Option<(i64, String)> = tx
        .query_row(
            "SELECT c.account_id, c.code FROM finance_company_account_codes c
             WHERE c.company_id=?1 AND c.code='1200'",
            [company_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (parent_id, parent_code) = match parent {
        Some(found) => found,
        None => {
            let first: Option<(i64, String)> = tx
                .query_row(
                    "SELECT c.account_id, c.code FROM finance_company_account_codes c
                     JOIN accounts a ON a.id=c.account_id
                     WHERE c.company_id=?1 AND a.category='asset' AND a.kind='detail'
                     ORDER BY c.code LIMIT 1",
                    [company_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            first.ok_or_else(|| {
                FinanceError::Invalid("دليل الحسابات لا يحتوي حساب أصول تفصيليًا.".into())
            })?
        }
    };
    let currency: String = tx.query_row(
        "SELECT currency FROM accounts WHERE id=?1",
        [parent_id],
        |r| r.get(0),
    )?;
    // ترقيم فرعي متسلسل بلا تصادم: نزيد حتى يجد رمزًا حرًّا.
    let mut counter: i64 = tx.query_row(
        "SELECT COUNT(*)+1 FROM finance_parties WHERE company_id=?1",
        [company_id],
        |r| r.get(0),
    )?;
    let (internal_code, display_code) = loop {
        let internal = format!("__c{company_id}_C{counter:04}");
        let display = format!("{parent_code}-{counter:04}");
        let taken: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE code=?1)
                OR EXISTS(SELECT 1 FROM finance_company_account_codes WHERE company_id=?2 AND code=?3)",
            params![internal, company_id, display],
            |r| r.get(0),
        )?;
        if !taken {
            break (internal, display);
        }
        counter += 1;
        if counter > 100_000 {
            return Err(FinanceError::Invalid("تعذّر توليد رمز حساب للعميل.".into()));
        }
    };
    tx.execute(
        "INSERT INTO accounts(code,name,parent_id,category,kind,nature,currency)
         VALUES(?1,?2,?3,'asset','detail','debit',?4)",
        params![internal_code, name, parent_id, currency],
    )?;
    let account_id = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO finance_account_scopes(company_id,account_id) VALUES(?1,?2)",
        params![company_id, account_id],
    )?;
    tx.execute(
        "INSERT INTO finance_company_account_codes(company_id,account_id,code) VALUES(?1,?2,?3)",
        params![company_id, account_id, display_code],
    )?;
    let branch: i64 = tx.query_row(
        "SELECT id FROM finance_branches WHERE company_id=?1 ORDER BY id LIMIT 1",
        [company_id],
        |r| r.get(0),
    )?;
    tx.execute(
        "INSERT INTO finance_parties(company_id,branch_id,account_id,name,role)
         VALUES(?1,?2,?3,?4,'customer')",
        params![company_id, branch, account_id, name],
    )?;
    Ok(tx.last_insert_rowid())
}

pub async fn create(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(input): Json<NewCustomer>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, input.company_id)?;
        if !scope.is_admin && !crate::identity::is_current_admin(&tx, &actor)? {
            return Err(FinanceError::Forbidden);
        }
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM finance_parties WHERE company_id=?1 AND name=?2)",
            params![scope.company_id, input.name.trim()],
            |r| r.get(0),
        )?;
        if exists {
            return Err(FinanceError::Conflict("يوجد عميل بهذا الاسم.".into()));
        }
        let id = create_customer(&tx, scope.company_id, &input.name)?;
        audit::record(&tx, audit::Entry {
            company_id: Some(scope.company_id), actor_id: Some(actor.id),
            action: "sales.customer.create", entity: "finance_parties",
            entity_id: Some(id.to_string()), payload: None,
        })?;
        tx.commit()?;
        Ok(id)
    })();
    match result {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response(),
        Err(e) => error_response(e),
    }
}
