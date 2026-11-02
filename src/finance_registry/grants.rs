//! Explicit, audited branch grants. Not a substitute for system-wide enforcement.
use super::*;
use rusqlite::OptionalExtension;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantInput {
    company_id: i64,
    branch_id: i64,
    user_id: i64,
    // None explicitly revokes; it does not grant unlimited access.
    #[serde(deserialize_with = "required_role")]
    role: Option<String>,
    reason: String,
}

fn required_role<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(d)
}

pub async fn change_grant(
    State(state): State<AppState>,
    Extension(actor): Extension<Principal>,
    Json(input): Json<GrantInput>,
) -> Response {
    if state.preview_no_login {
        return fail(
            StatusCode::FORBIDDEN,
            "إدارة الإسناد غير متاحة في المعاينة المفتوحة.",
        );
    }
    let reason = input.reason.trim();
    if input.company_id <= 0
        || input.branch_id <= 0
        || input.user_id <= 0
        || reason.is_empty()
        || reason.chars().count() > 200
        || reason.chars().any(char::is_control)
        || input
            .role
            .as_deref()
            .is_some_and(|x| !["reader", "operator", "admin"].contains(&x))
    {
        return fail(
            StatusCode::UNPROCESSABLE_ENTITY,
            "بيانات الإسناد أو السبب غير صالحين.",
        );
    }
    let result = (|| -> Result<bool, Error> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !crate::identity::is_current_admin(&tx, &actor)?
            || !access::company_admin(&tx, actor.id, input.company_id)?
            || !access::branch_admin(&tx, actor.id, input.company_id, input.branch_id)?
        {
            return Err(Error::Unauthorized);
        }
        let target_exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM identity_users WHERE id=?1 AND active=1)",
            [input.user_id],
            |r| r.get(0),
        )?;
        if !target_exists {
            return Err(Error::Invalid("المستخدم غير موجود أو غير نشط."));
        }
        let old: Option<String> = tx.query_row(
            "SELECT role FROM finance_branch_grants WHERE company_id=?1 AND branch_id=?2 AND user_id=?3",
            params![input.company_id,input.branch_id,input.user_id], |r| r.get(0),
        ).optional()?;
        if old == input.role {
            tx.commit()?;
            return Ok(false);
        }
        if let Some(role) = input.role.as_deref() {
            tx.execute(
                "INSERT INTO finance_branch_grants(company_id,branch_id,user_id,role,granted_by)
                 VALUES(?1,?2,?3,?4,?5) ON CONFLICT(company_id,branch_id,user_id)
                 DO UPDATE SET role=excluded.role,granted_by=excluded.granted_by,granted_at=CURRENT_TIMESTAMP",
                params![input.company_id,input.branch_id,input.user_id,role,actor.id],
            )?;
        } else {
            tx.execute("DELETE FROM finance_branch_grants WHERE company_id=?1 AND branch_id=?2 AND user_id=?3",
                params![input.company_id,input.branch_id,input.user_id])?;
        }
        tx.execute(
            "INSERT INTO finance_grant_events(company_id,branch_id,user_id,actor_id,old_role,new_role,reason)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![input.company_id,input.branch_id,input.user_id,actor.id,old,input.role,reason],
        )?;
        tx.commit()?;
        Ok(true)
    })();
    match result {
        Ok(changed) => Json(serde_json::json!({"changed":changed})).into_response(),
        Err(e) => error_response(e),
    }
}
