use super::*;
use rusqlite::OptionalExtension;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RegistryInput {
    Company {
        code: String,
        name: String,
    },
    Branch {
        company_id: i64,
        code: String,
        name: String,
    },
    AccountScope {
        company_id: i64,
        account_id: i64,
    },
    Party {
        company_id: i64,
        branch_id: i64,
        account_id: i64,
        name: String,
        role: String,
    },
    Fund {
        company_id: i64,
        branch_id: i64,
        account_id: i64,
        name: String,
        kind: String,
    },
}
fn exists(db: &Connection, sql: &str, ids: &[i64]) -> rusqlite::Result<bool> {
    db.query_row(sql, rusqlite::params_from_iter(ids), |r| r.get(0))
}
fn scoped_account(
    db: &Connection,
    company: i64,
    branch: i64,
    account: i64,
    fund: bool,
) -> Result<(), Error> {
    if !exists(
        db,
        "SELECT EXISTS(SELECT 1 FROM finance_branches WHERE company_id=?1 AND id=?2)",
        &[company, branch],
    )? {
        return Err(Error::Invalid("الفرع لا يتبع الشركة المختارة."));
    }
    let info: Option<(String, String)> = db.query_row(
        "SELECT a.kind,a.category FROM accounts a JOIN finance_account_scopes s ON s.account_id=a.id WHERE s.company_id=?1 AND a.id=?2",
        params![company, account], |r| Ok((r.get(0)?, r.get(1)?)),
    ).optional()?;
    if !matches!(info, Some((ref kind, ref category)) if kind == "detail" && (!fund || category == "asset"))
    {
        return Err(Error::Invalid(
            "الحساب يجب أن يكون تفصيليًا تابعًا للشركة؛ حساب الصندوق أو البنك من الأصول.",
        ));
    }
    if exists(
        db,
        "SELECT EXISTS(SELECT 1 FROM finance_funds WHERE account_id=?1 UNION ALL SELECT 1 FROM finance_parties WHERE account_id=?1)",
        &[account],
    )? {
        return Err(Error::Conflict);
    }
    Ok(())
}

pub async fn create(
    State(state): State<AppState>,
    Extension(actor): Extension<Principal>,
    Json(input): Json<RegistryInput>,
) -> Response {
    let result = (|| -> Result<i64, Error> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !state.preview_no_login && !crate::identity::is_current_admin(&tx, &actor)? {
            return Err(Error::Unauthorized);
        }
        let id = match input {
            RegistryInput::Company { code, name } => {
                tx.execute(
                    "INSERT INTO finance_companies(code,name) VALUES(?1,?2)",
                    params![text(&code, 30)?, text(&name, 120)?],
                )
                .map_err(map_constraint)?;
                let company_id = tx.last_insert_rowid();
                if !state.preview_no_login {
                    tx.execute(
                        "INSERT INTO finance_company_admins(company_id,user_id,granted_by) VALUES(?1,?2,?2)",
                        params![company_id, actor.id],
                    )?;
                }
                company_id
            }
            RegistryInput::Branch {
                company_id,
                code,
                name,
            } => {
                if !state.preview_no_login && !access::company_admin(&tx, actor.id, company_id)? {
                    return Err(Error::Unauthorized);
                }
                if !exists(
                    &tx,
                    "SELECT EXISTS(SELECT 1 FROM finance_companies WHERE id=?1)",
                    &[company_id],
                )? {
                    return Err(Error::Invalid("الشركة غير موجودة."));
                }
                tx.execute(
                    "INSERT INTO finance_branches(company_id,code,name) VALUES(?1,?2,?3)",
                    params![company_id, text(&code, 30)?, text(&name, 120)?],
                )
                .map_err(map_constraint)?;
                let branch_id = tx.last_insert_rowid();
                if !state.preview_no_login {
                    tx.execute(
                        "INSERT INTO finance_branch_grants(company_id,branch_id,user_id,role,granted_by) VALUES(?1,?2,?3,'admin',?3)",
                        params![company_id, branch_id, actor.id],
                    )?;
                    tx.execute(
                        "INSERT INTO finance_grant_events(company_id,branch_id,user_id,actor_id,new_role,reason)
                         VALUES(?1,?2,?3,?3,'admin','إنشاء الفرع وإسناد المنشئ')",
                        params![company_id, branch_id, actor.id],
                    )?;
                }
                branch_id
            }
            RegistryInput::AccountScope {
                company_id,
                account_id,
            } => {
                // Old accounts are unassigned history; assigning them needs an explicit
                // audited migration workflow which does not exist yet.
                if !state.preview_no_login {
                    return Err(Error::Invalid(
                        "إسناد حساب سابق غير متاح قبل بناء إجراء مراجعة وتدقيق مخصص.",
                    ));
                }
                if !state.preview_no_login && !access::company_admin(&tx, actor.id, company_id)? {
                    return Err(Error::Unauthorized);
                }
                if !exists(
                    &tx,
                    "SELECT EXISTS(SELECT 1 FROM finance_companies WHERE id=?1)",
                    &[company_id],
                )? {
                    return Err(Error::Invalid("الشركة غير موجودة."));
                }
                let detail = exists(
                    &tx,
                    "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1 AND kind='detail')",
                    &[account_id],
                )?;
                let used = exists(
                    &tx,
                    "SELECT EXISTS(SELECT 1 FROM draft_lines WHERE account_id=?1 UNION ALL SELECT 1 FROM journal_lines WHERE account_id=?1)",
                    &[account_id],
                )?;
                if !detail || used {
                    return Err(Error::Invalid(
                        "يجب اختيار حساب تفصيلي لم يستخدم في مسودات أو قيود؛ لا تُنسب الحركة القديمة بالتخمين.",
                    ));
                }
                tx.execute(
                    "INSERT INTO finance_account_scopes(account_id,company_id) VALUES(?1,?2)",
                    params![account_id, company_id],
                )
                .map_err(map_constraint)?;
                tx.execute(
                    "INSERT INTO finance_company_account_codes(company_id,account_id,code) SELECT ?1,id,code FROM accounts WHERE id=?2",
                    params![company_id, account_id],
                ).map_err(map_constraint)?;
                account_id
            }
            RegistryInput::Party {
                company_id,
                branch_id,
                account_id,
                name,
                role,
            } => {
                if !state.preview_no_login
                    && (!access::company_admin(&tx, actor.id, company_id)?
                        || !access::branch_write(&tx, actor.id, company_id, branch_id)?)
                {
                    return Err(Error::Unauthorized);
                }
                if !["customer", "supplier", "both"].contains(&role.as_str()) {
                    return Err(Error::Invalid("نوع الطرف غير صالح."));
                }
                scoped_account(&tx, company_id, branch_id, account_id, false)?;
                tx.execute("INSERT INTO finance_parties(company_id,branch_id,account_id,name,role) VALUES(?1,?2,?3,?4,?5)",params![company_id,branch_id,account_id,text(&name,120)?,role]).map_err(map_constraint)?;
                tx.last_insert_rowid()
            }
            RegistryInput::Fund {
                company_id,
                branch_id,
                account_id,
                name,
                kind,
            } => {
                if !state.preview_no_login
                    && (!access::company_admin(&tx, actor.id, company_id)?
                        || !access::branch_write(&tx, actor.id, company_id, branch_id)?)
                {
                    return Err(Error::Unauthorized);
                }
                if !["cash", "bank"].contains(&kind.as_str()) {
                    return Err(Error::Invalid("نوع الصندوق أو البنك غير صالح."));
                }
                scoped_account(&tx, company_id, branch_id, account_id, true)?;
                tx.execute("INSERT INTO finance_funds(company_id,branch_id,account_id,name,kind) VALUES(?1,?2,?3,?4,?5)",params![company_id,branch_id,account_id,text(&name,120)?,kind]).map_err(map_constraint)?;
                tx.last_insert_rowid()
            }
        };
        tx.commit()?;
        Ok(id)
    })();
    match result {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({"id":id}))).into_response(),
        Err(e) => error_response(e),
    }
}
fn map_constraint(error: rusqlite::Error) -> Error {
    if matches!(error, rusqlite::Error::SqliteFailure(ref e, _) if e.code == rusqlite::ErrorCode::ConstraintViolation)
    {
        Error::Conflict
    } else {
        Error::Storage(error)
    }
}
