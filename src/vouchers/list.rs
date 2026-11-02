//! Cursor-based voucher history. Each returned voucher is validated within one read snapshot.
use super::*;
use axum::extract::{Query, rejection::QueryRejection};
use rust_decimal::Decimal;
use std::str::FromStr;

#[derive(Serialize)]
struct VoucherView {
    id: i64,
    draft_id: i64,
    kind: String,
    party_name: String,
    reference: String,
    voucher_no: Option<String>,
    date: String,
    currency: String,
    amount: String,
    description: String,
    cash_account: String,
    other_account: String,
    status: &'static str,
}
pub(super) fn amount(s: &str) -> Result<Decimal, Box<dyn std::error::Error>> {
    if s.trim().is_empty() {
        Ok(Decimal::ZERO)
    } else {
        Ok(Decimal::from_str(s.trim())?)
    }
}
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoucherListQuery {
    q: Option<String>,
    kind: Option<String>,
    currency: Option<String>,
    from: Option<String>,
    to: Option<String>,
    before: Option<i64>,
    ceiling: Option<i64>,
}

impl VoucherListQuery {
    fn validate(&mut self) -> Result<(), &'static str> {
        for field in [
            &mut self.q,
            &mut self.kind,
            &mut self.currency,
            &mut self.from,
            &mut self.to,
        ] {
            *field = field
                .take()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }
        if self
            .q
            .as_ref()
            .is_some_and(|s| s.chars().count() > 120 || s.chars().any(char::is_control))
        {
            return Err("نص البحث غير صالح؛ الحد ١٢٠ حرفًا دون محارف تحكم.");
        }
        if self
            .kind
            .as_deref()
            .is_some_and(|s| !["receipt", "payment"].contains(&s))
        {
            return Err("نوع السند غير صالح.");
        }
        if self
            .currency
            .as_deref()
            .is_some_and(|s| !domain::supported_currency(s))
        {
            return Err("العملة غير مدعومة.");
        }
        for date in [&self.from, &self.to].into_iter().flatten() {
            if date.len() != 10 || chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
                return Err("تاريخ التصفية غير صالح.");
            }
        }
        if self
            .from
            .as_ref()
            .zip(self.to.as_ref())
            .is_some_and(|(a, b)| a > b)
        {
            return Err("تاريخ البداية بعد تاريخ النهاية.");
        }
        if self.ceiling.is_some_and(|x| x < 0)
            || self.before.is_some_and(|x| x <= 0)
            || self.before.is_some() && self.ceiling.is_none()
            || self.before.zip(self.ceiling).is_some_and(|(b, c)| b > c)
        {
            return Err("مؤشر صفحة السندات غير صالح؛ أعد تحميل الصفحة الأولى.");
        }
        Ok(())
    }
}

pub async fn list(
    State(state): State<AppState>,
    query: Result<Query<VoucherListQuery>, QueryRejection>,
) -> Response {
    let Query(mut filters) = match query {
        Ok(query) => query,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "معلمات تصفية السندات غير صالحة."),
    };
    if let Err(message) = filters.validate() {
        return fail(StatusCode::BAD_REQUEST, message);
    }
    let result = (|| -> Result<(Vec<VoucherView>, bool, i64), Box<dyn std::error::Error>> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction()?;
        // A high-water mark prevents newly inserted vouchers shifting later pages.
        // It is not a persistent snapshot across HTTP requests.
        let ceiling = match filters.ceiling {
            Some(c) => c,
            None => tx.query_row(
                "SELECT COALESCE(MAX(id),0) FROM financial_vouchers",
                [],
                |r| r.get(0),
            )?,
        };
        let raw: Vec<(i64, i64, String, String, String, Option<String>)> = {
            let mut st = tx.prepare(
                "SELECT v.id,v.draft_id,v.kind,v.party_name,v.reference,v.voucher_no
                 FROM financial_vouchers v LEFT JOIN drafts d ON d.id=v.draft_id
                 WHERE v.id<=?1 AND (?2 IS NULL OR v.id<?2)
                   AND (?3 IS NULL OR v.kind=?3) AND (?4 IS NULL OR d.currency=?4)
                   AND (?5 IS NULL OR d.date>=?5) AND (?6 IS NULL OR d.date<=?6)
                   AND (?7 IS NULL OR instr(lower(CAST(v.id AS TEXT)),lower(?7))>0
                     OR instr(lower(v.party_name),lower(?7))>0
                     OR instr(lower(v.reference),lower(?7))>0
                     OR instr(lower(d.description),lower(?7))>0
                     OR EXISTS(SELECT 1 FROM draft_lines l WHERE l.draft_id=v.draft_id
                               AND instr(lower(l.account_snapshot),lower(?7))>0))
                 ORDER BY v.id DESC LIMIT 101",
            )?;
            let rows = st.query_map(
                params![
                    ceiling,
                    filters.before,
                    filters.kind,
                    filters.currency,
                    filters.from,
                    filters.to,
                    filters.q
                ],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )?;
            rows.collect::<Result<_, _>>()?
        };
        let has_more = raw.len() > 100;
        let mut out = Vec::new();
        for (id, draft_id, kind, party_name, reference, voucher_no) in raw.into_iter().take(100) {
            let d = read_draft(&tx, draft_id)?
                .ok_or_else(|| std::io::Error::other("voucher draft missing"))?;
            if d.lines.len() != 2
                || d.lines[0].account_id.is_none()
                || d.lines[1].account_id.is_none()
                || d.lines[0].account_id == d.lines[1].account_id
            {
                return Err(std::io::Error::other("invalid voucher lines").into());
            }
            let l = &d.lines[0];
            let n = &d.lines[1];
            let shape = match kind.as_str() {
                "receipt" => {
                    amount(&l.debit)? > Decimal::ZERO
                        && amount(&l.credit)?.is_zero()
                        && amount(&n.credit)? > Decimal::ZERO
                        && amount(&n.debit)?.is_zero()
                }
                "payment" => {
                    amount(&l.credit)? > Decimal::ZERO
                        && amount(&l.debit)?.is_zero()
                        && amount(&n.debit)? > Decimal::ZERO
                        && amount(&n.credit)?.is_zero()
                }
                _ => false,
            };
            if !shape {
                return Err(std::io::Error::other("invalid voucher direction").into());
            }
            out.push(VoucherView {
                id,
                draft_id,
                kind,
                party_name,
                reference,
                voucher_no,
                date: d.date,
                currency: d.currency,
                amount: d.totals.debit,
                description: d.description,
                cash_account: l.account.clone(),
                other_account: n.account.clone(),
                status: d.status,
            });
        }
        tx.commit()?;
        Ok((out, has_more, ceiling))
    })();
    match result {
        Ok((rows, has_more, ceiling)) => {
            let next_before = if has_more {
                rows.last().map(|row| row.id)
            } else {
                None
            };
            Json(serde_json::json!({"rows":rows,"has_more":has_more,"ceiling":ceiling,"next_before":next_before})).into_response()
        }
        Err(e) => {
            tracing::error!(%e,"voucher list validation failed");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "تعذر قراءة السندات أو التحقق من القيود المرتبطة؛ راجع سجل الخادم.",
            )
        }
    }
}
