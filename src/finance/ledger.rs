//! دفتر الأستاذ للشركة: حركة حساب واحد بعملته، مع رصيد جارٍ دقيق.
use super::money::{self, render};
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, params};
use rust_decimal::Decimal;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct LedgerQuery {
    company_id: i64,
    /// عند غيابه نُعيد قائمة الحسابات بأرصدتها (فهرس الأستاذ).
    account_id: Option<i64>,
    from: Option<String>,
    to: Option<String>,
    #[serde(default)]
    branch_id: Option<i64>,
}

pub async fn entries(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<LedgerQuery>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        for day in [&q.from, &q.to].into_iter().flatten() {
            if chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").is_err() {
                return Err(FinanceError::Invalid("تاريخ غير صالح.".into()));
            }
        }
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let Some(account_id) = q.account_id else {
            return index(&db, &scope);
        };
        if !scope.owns_account(&db, account_id)? {
            return Err(FinanceError::Invalid(
                "الحساب ليس من دليل هذه الشركة.".into(),
            ));
        }
        let account_id = q.account_id.unwrap_or_default();
        let meta: Option<(String, String, String)> = db
            .query_row(
                "SELECT a.name,a.currency,COALESCE(c.code,'') FROM accounts a
                 LEFT JOIN finance_company_account_codes c
                        ON c.account_id=a.id AND c.company_id=?2 WHERE a.id=?1",
                params![account_id, scope.company_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (name, currency, code) = meta.ok_or(FinanceError::NotFound("الحساب"))?;
        let from = q.from.clone().unwrap_or_else(|| "0000-01-01".into());
        let to = q.to.clone().unwrap_or_else(|| "9999-12-31".into());
        let mut stmt = db.prepare(
            "SELECT e.entry_date,e.description,l.debit,l.credit,e.id
             FROM journal_lines l JOIN journal_entries e ON e.id=l.entry_id
             WHERE e.finalized=1 AND e.company_id=?1 AND e.currency=?2 AND l.account_id=?3
               AND e.entry_date>=?4 AND e.entry_date<=?5
               AND (?6 IS NULL OR e.branch_id=?6)
             ORDER BY e.entry_date, e.id, l.line_no",
        )?;
        let rows = stmt.query_map(
            params![
                scope.company_id,
                currency,
                account_id,
                from,
                to,
                q.branch_id
            ],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            },
        )?;
        let mut running = Decimal::ZERO;
        let mut lines = Vec::new();
        for row in rows {
            let (date, description, debit, credit, entry_id) = row?;
            let d = money::from_db(&debit, "مدين")?;
            let c = money::from_db(&credit, "دائن")?;
            running += d - c;
            lines.push(serde_json::json!({
                "entry_id": entry_id, "date": date, "description": description,
                "debit": render(d), "credit": render(c), "balance": render(running)
            }));
        }
        Ok(serde_json::json!({
            "account_id": account_id, "code": code, "name": name, "currency": currency,
            "opening": render(Decimal::ZERO), "closing": render(running), "lines": lines
        }))
    })();
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => error_response(e),
    }
}

/// فهرس الأستاذ: كل حسابات الشركة بعملاتها وأرصدتها (بلا تفاصيل الحركة).
fn index(db: &rusqlite::Connection, scope: &Scope) -> Result<serde_json::Value, FinanceError> {
    let mut stmt = db.prepare(
        "SELECT a.id,COALESCE(c.code,''),a.name,a.kind,a.currency
         FROM accounts a
         JOIN finance_company_account_codes c ON c.account_id=a.id AND c.company_id=?1
         ORDER BY c.code",
    )?;
    let rows = stmt.query_map([scope.company_id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    let mut accounts = Vec::new();
    for row in rows {
        let (id, code, name, kind, currency) = row?;
        let mut totals = db.prepare(
            "SELECT l.debit,l.credit FROM journal_lines l
             JOIN journal_entries e ON e.id=l.entry_id
             WHERE e.finalized=1 AND e.company_id=?1 AND l.account_id=?2",
        )?;
        let lines = totals.query_map(params![scope.company_id, id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let (mut debit, mut credit) = (Decimal::ZERO, Decimal::ZERO);
        for line in lines {
            let (d, c) = line?;
            debit += money::from_db(&d, "مدين")?;
            credit += money::from_db(&c, "دائن")?;
        }
        accounts.push(serde_json::json!({
            "account_id": id, "code": code, "name": name, "kind": kind, "currency": currency,
            "debit": render(debit), "credit": render(credit),
            "balance": render(debit - credit)
        }));
    }
    Ok(serde_json::json!({"company_id": scope.company_id, "accounts": accounts}))
}
