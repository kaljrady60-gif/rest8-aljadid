//! Company chart uses the existing accounts table and a business code scoped to a company.
//! Internal accounts.code is an opaque unique key; historic account codes remain unchanged.
use super::*;
use crate::identity::Principal;
use axum::extract::Query;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedInput {
    company_id: i64,
    code: String,
    name: String,
    parent_id: Option<i64>,
    category: String,
    kind: String,
    nature: String,
    currency: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyQuery {
    company_id: i64,
}

fn public_error(e: CreateError) -> Response {
    match e {
        CreateError::Unauthorized => fail(StatusCode::FORBIDDEN, "لا يوجد إسناد يسمح بهذه العملية."),
        CreateError::Invalid(message) => fail(StatusCode::UNPROCESSABLE_ENTITY, message),
        CreateError::Conflict => fail(StatusCode::CONFLICT, "رقم الحساب مستخدم في هذه الشركة."),
        CreateError::Storage(e) => {
            tracing::error!(%e,"scoped account failure");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "تعذر حفظ الحساب أو قراءة الدليل.",
            )
        }
    }
}

pub async fn create_scoped(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<Principal>,
    Json(i): Json<ScopedInput>,
) -> Response {
    let a = AccountInput {
        code: i.code,
        name: i.name,
        parent_id: i.parent_id,
        category: i.category,
        kind: i.kind,
        nature: i.nature,
        currency: i.currency,
    };
    if let Err(message) = validate(&a) {
        return fail(StatusCode::UNPROCESSABLE_ENTITY, message);
    }
    let result = (|| -> Result<i64, CreateError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !state.preview_no_login
            && (!crate::identity::is_current_admin(&tx, &actor)?
                || !crate::finance_registry::access::company_admin(&tx, actor.id, i.company_id)?)
        {
            return Err(CreateError::Unauthorized);
        }
        let company_exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM finance_companies WHERE id=?1)",
            [i.company_id],
            |r| r.get(0),
        )?;
        if !company_exists {
            return Err(CreateError::Invalid("الشركة غير موجودة."));
        }
        if let Some(parent) = a.parent_id {
            let p: Option<(String,String)> = tx.query_row(
                "SELECT a.kind,a.category FROM accounts a JOIN finance_account_scopes s ON s.account_id=a.id WHERE a.id=?1 AND s.company_id=?2",
                params![parent,i.company_id], |r|Ok((r.get(0)?,r.get(1)?)),
            ).optional()?;
            if !matches!(p,Some((ref kind,ref category)) if kind=="group" && *category==a.category)
            {
                return Err(CreateError::Invalid(
                    "الحساب الأب يجب أن يكون رئيسيًا من الشركة والتصنيف نفسيهما.",
                ));
            }
        }
        let duplicate:bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM finance_company_account_codes WHERE company_id=?1 AND code=?2)",params![i.company_id,a.code.trim()],|r|r.get(0))?;
        if duplicate {
            return Err(CreateError::Conflict);
        }
        let mut random = [0u8; 16];
        getrandom::fill(&mut random)
            .map_err(|_| CreateError::Invalid("تعذر إنشاء معرّف الحساب الآمن."))?;
        let key = format!(
            "__org_{}",
            random
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        tx.execute("INSERT INTO accounts(code,name,parent_id,category,kind,nature,currency) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![key,a.name.trim(),a.parent_id,a.category,a.kind,a.nature,a.currency])?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO finance_account_scopes(company_id,account_id) VALUES(?1,?2)",
            params![i.company_id, id],
        )?;
        tx.execute("INSERT INTO finance_company_account_codes(company_id,account_id,code) VALUES(?1,?2,?3)",params![i.company_id,id,a.code.trim()])?;
        tx.commit()?;
        Ok(id)
    })();
    match result {
        Ok(id) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"id":id,"company_id":i.company_id,"code":a.code.trim()})),
        )
            .into_response(),
        Err(e) => public_error(e),
    }
}

pub async fn list_scoped(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<Principal>,
    Query(q): Query<CompanyQuery>,
) -> Response {
    let result = (|| -> Result<Vec<Account>, CreateError> {
        let db = db_open(&state.db)?;
        let tx = db.unchecked_transaction()?;
        if !state.preview_no_login
            && (!crate::identity::is_current_user(&tx, &actor)?
                || !crate::finance_registry::access::company_read(&tx, actor.id, q.company_id)?)
        {
            return Err(CreateError::Unauthorized);
        }
        let rows = {
            let mut st=tx.prepare("SELECT a.id,c.code,a.name,a.parent_id,a.category,a.kind,a.nature,a.currency,c.company_id FROM finance_company_account_codes c JOIN accounts a ON a.id=c.account_id WHERE c.company_id=?1 ORDER BY c.code,a.id")?;
            let rows = st.query_map([q.company_id], |r| {
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
            rows.collect::<Result<Vec<_>, _>>()?
        };
        tx.commit()?;
        Ok(rows)
    })();
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => public_error(e),
    }
}
