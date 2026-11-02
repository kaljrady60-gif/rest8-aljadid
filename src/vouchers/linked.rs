//! Scoped draft vouchers: atomic linkage to the existing journal draft, never bank execution.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkedInput {
    request_id: String,
    kind: String,
    date: String,
    currency: String,
    amount: String,
    company_id: i64,
    branch_id: i64,
    fund_id: i64,
    party_id: i64,
    description: String,
    reference: String,
}

pub async fn create(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(input): Json<LinkedInput>,
) -> Response {
    let result = (|| -> Result<(i64, i64, bool), Error> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !state.preview_no_login && !crate::identity::is_current_admin(&tx, &actor)? {
            return Err(Error::Unauthorized);
        }
        if !state.preview_no_login
            && !crate::finance_registry::access::branch_write(
                &tx,
                actor.id,
                input.company_id,
                input.branch_id,
            )?
        {
            return Err(Error::Unauthorized);
        }
        let linked: Option<(i64, i64, String, String, String)> = tx.query_row(
            "SELECT f.account_id,p.account_id,p.name,fa.currency,pa.currency
             FROM finance_funds f JOIN finance_parties p ON p.company_id=f.company_id AND p.branch_id=f.branch_id
             JOIN accounts fa ON fa.id=f.account_id JOIN accounts pa ON pa.id=p.account_id
             WHERE f.id=?1 AND p.id=?2 AND f.company_id=?3 AND f.branch_id=?4",
            params![input.fund_id,input.party_id,input.company_id,input.branch_id],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
        ).optional()?;
        let (cash_account_id, other_account_id, party_name, fund_currency, party_currency) = linked
            .ok_or_else(|| {
                Error::Invalid("الصندوق والطرف يجب أن يتبعا الشركة والفرع المختارين.".into())
            })?;
        if fund_currency != input.currency || party_currency != input.currency {
            return Err(Error::Invalid(
                "عملة السند يجب أن تطابق عملة حساب الصندوق أو البنك والطرف؛ لا يوجد تحويل صرف هنا."
                    .into(),
            ));
        }
        let voucher = VoucherInput {
            request_id: input.request_id,
            kind: input.kind,
            date: input.date,
            currency: input.currency,
            amount: input.amount,
            cash_account_id,
            other_account_id,
            party_name,
            description: input.description,
            reference: input.reference,
        };
        validate(&voucher).map_err(|e| Error::Invalid(e.into()))?;
        let (id, draft_id, created, _) = save_voucher_in_tx(&tx, &voucher, Some(input.company_id))?;
        if created {
            tx.execute("INSERT INTO finance_voucher_context(voucher_id,company_id,branch_id,party_id,fund_id) VALUES(?1,?2,?3,?4,?5)",
                params![id,input.company_id,input.branch_id,input.party_id,input.fund_id])?;
        } else {
            let old: Option<(i64,i64,i64,i64)> = tx.query_row(
                "SELECT company_id,branch_id,party_id,fund_id FROM finance_voucher_context WHERE voucher_id=?1", [id],
                |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
            ).optional()?;
            if old
                != Some((
                    input.company_id,
                    input.branch_id,
                    input.party_id,
                    input.fund_id,
                ))
            {
                return Err(Error::Conflict);
            }
        }
        tx.commit()?;
        Ok((id, draft_id, created))
    })();
    match result {
        Ok((id,draft_id,created)) => (if created {StatusCode::CREATED} else {StatusCode::OK},
            Json(serde_json::json!({"id":id,"draft_id":draft_id,"created":created,"status":"draft","posting_available":false}))).into_response(),
        Err(Error::Invalid(message)) => fail(StatusCode::UNPROCESSABLE_ENTITY,message),
        Err(Error::Conflict) => fail(StatusCode::CONFLICT,"معرّف الحفظ مستخدم مع بيانات أو روابط مختلفة."),
        Err(Error::Unauthorized) => fail(StatusCode::FORBIDDEN,"انتهت جلسة المدير أو أُلغيت صلاحيتها."),
        Err(Error::Storage(e)) => { tracing::error!(%e,"linked voucher save failed"); fail(StatusCode::INTERNAL_SERVER_ERROR,"تعذر حفظ السند وروابطه؛ راجع السجل قبل إعادة المحاولة.") }
    }
}
