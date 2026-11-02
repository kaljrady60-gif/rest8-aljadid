//! القوائم المالية: تُحسب دائمًا من القيود المرحّلة + الأرصدة الافتتاحية،
//! بعملة واحدة في كل مرة — لا تُجمع عملتان في رقم واحد أبدًا.
use super::money::{self, render};
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use rusqlite::{Connection, params};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
pub struct StatementQuery {
    pub company_id: i64,
    #[serde(default = "default_report")]
    pub report: String,
    pub currency: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    /// عند تحديده تُقيَّد كل التقارير بحركة هذا الفرع فقط.
    #[serde(default)]
    pub branch_id: Option<i64>,
}

fn default_report() -> String {
    "trial".into()
}

struct Line {
    account_id: i64,
    code: String,
    name: String,
    category: String,
    debit: Decimal,
    credit: Decimal,
}

fn check_dates(q: &StatementQuery) -> Result<(), FinanceError> {
    for day in [&q.from, &q.to].into_iter().flatten() {
        if chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").is_err() || day.len() != 10 {
            return Err(FinanceError::Invalid("تاريخ غير صالح.".into()));
        }
    }
    if q.from
        .as_ref()
        .zip(q.to.as_ref())
        .is_some_and(|(a, b)| a > b)
    {
        return Err(FinanceError::Invalid("بداية الفترة بعد نهايتها.".into()));
    }
    Ok(())
}

/// يُجمّع حركة القيود المرحّلة للشركة بعملة واحدة، مع دمج الأرصدة الافتتاحية عند الطلب.
fn movements(
    db: &Connection,
    company_id: i64,
    currency: &str,
    q: &StatementQuery,
    with_opening: bool,
) -> Result<Vec<Line>, FinanceError> {
    let from = q.from.clone().unwrap_or_else(|| "0000-01-01".into());
    let to = q.to.clone().unwrap_or_else(|| "9999-12-31".into());
    let mut stmt = db.prepare(
        "SELECT a.id, COALESCE(c.code,''), a.name, a.category, l.debit, l.credit
         FROM journal_lines l
         JOIN journal_entries e ON e.id=l.entry_id
         JOIN accounts a ON a.id=l.account_id
         LEFT JOIN finance_company_account_codes c ON c.account_id=a.id AND c.company_id=e.company_id
         WHERE e.finalized=1 AND e.company_id=?1 AND e.currency=?2
           AND e.entry_date>=?3 AND e.entry_date<=?4
           AND (?5 IS NULL OR e.branch_id=?5)",
    )?;
    let mut map: BTreeMap<i64, Line> = BTreeMap::new();
    let rows = stmt.query_map(params![company_id, currency, from, to, q.branch_id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
        ))
    })?;
    for row in rows {
        let (account_id, code, name, category, debit, credit) = row?;
        let line = map.entry(account_id).or_insert(Line {
            account_id,
            code,
            name,
            category,
            debit: Decimal::ZERO,
            credit: Decimal::ZERO,
        });
        line.debit += money::from_db(&debit, "مدين")?;
        line.credit += money::from_db(&credit, "دائن")?;
    }
    if with_opening {
        let mut stmt = db.prepare(
            "SELECT o.account_id, COALESCE(c.code,''), a.name, a.category, o.debit, o.credit
             FROM finance_opening_balances o
             JOIN accounts a ON a.id=o.account_id
             LEFT JOIN finance_company_account_codes c ON c.account_id=a.id AND c.company_id=o.company_id
             WHERE o.company_id=?1 AND o.currency=?2",
        )?;
        let rows = stmt.query_map(params![company_id, currency], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?;
        for row in rows {
            let (account_id, code, name, category, debit, credit) = row?;
            let line = map.entry(account_id).or_insert(Line {
                account_id,
                code,
                name,
                category,
                debit: Decimal::ZERO,
                credit: Decimal::ZERO,
            });
            line.debit += money::from_db(&debit, "مدين افتتاحي")?;
            line.credit += money::from_db(&credit, "دائن افتتاحي")?;
        }
    }
    Ok(map.into_values().collect())
}

fn group_by_category(lines: &[Line], categories: &[&str]) -> (Vec<serde_json::Value>, Decimal) {
    let mut items = Vec::new();
    let mut total = Decimal::ZERO;
    for category in categories {
        let mut subtotal = Decimal::ZERO;
        let mut children = Vec::new();
        for line in lines.iter().filter(|l| l.category == *category) {
            // طبيعة الحساب تُحدد إشارة الرصيد: أصول/مصروف مدين، غيرها دائن.
            let balance = if *category == "asset" || *category == "expense" {
                line.debit - line.credit
            } else {
                line.credit - line.debit
            };
            if balance == Decimal::ZERO {
                continue;
            }
            subtotal += balance;
            children.push(serde_json::json!({
                "account_id": line.account_id, "code": line.code,
                "name": line.name, "balance": render(balance)
            }));
        }
        total += subtotal;
        items.push(serde_json::json!({
            "category": category, "total": render(subtotal), "accounts": children
        }));
    }
    (items, total)
}

pub async fn get(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<StatementQuery>,
) -> Response {
    match compute(&state, &actor, q) {
        Ok(value) => Json(value).into_response(),
        Err(e) => error_response(e),
    }
}

/// الحساب الفعلي للتقارير — يُستدعى من المسار ومن التصدير.
pub fn compute(
    state: &AppState,
    actor: &crate::identity::Principal,
    q: StatementQuery,
) -> Result<serde_json::Value, FinanceError> {
    (|| -> Result<serde_json::Value, FinanceError> {
        check_dates(&q)?;
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(state, &db, actor, q.company_id)?;
        let currency = q.currency.clone().unwrap_or_else(|| "YER".into());
        if !crate::generated::contracts::supported_currency(&currency) {
            return Err(FinanceError::Invalid("العملة غير مدعومة.".into()));
        }
        let known = crate::generated::contracts::REPORTS
            .iter()
            .any(|r| r.id == q.report);
        if !known {
            return Err(FinanceError::Invalid("تقرير غير معروف.".into()));
        }
        let body = match q.report.as_str() {
            "income" => {
                let lines = movements(&db, scope.company_id, &currency, &q, false)?;
                let (items, profit) = {
                    let (rev, revenue_total) = group_by_category(&lines, &["revenue"]);
                    let (exp, expense_total) = group_by_category(&lines, &["expense"]);
                    let mut items = rev;
                    items.extend(exp);
                    let mut profit = revenue_total - expense_total;
                    if profit.is_zero() {
                        profit = Decimal::ZERO;
                    }
                    (items, profit)
                };
                serde_json::json!({"report":"income","currency":currency,
                    "sections":items,"net_profit":render(profit)})
            }
            "balance" => {
                let lines = movements(&db, scope.company_id, &currency, &q, true)?;
                let (assets, assets_total) = group_by_category(&lines, &["asset"]);
                let (liab_equity, liab_equity_total) =
                    group_by_category(&lines, &["liability", "equity"]);
                let (_, revenue_total) = group_by_category(&lines, &["revenue"]);
                let (_, expense_total) = group_by_category(&lines, &["expense"]);
                let profit = revenue_total - expense_total;
                let mut sections = assets;
                sections.extend(liab_equity);
                serde_json::json!({"report":"balance","currency":currency,
                    "sections":sections,"assets_total":render(assets_total),
                    "liabilities_equity_total":render(liab_equity_total + profit),
                    "retained_earnings":render(profit),
                    "balanced": assets_total == liab_equity_total + profit})
            }
            "cashflow" => {
                let lines = movements(&db, scope.company_id, &currency, &q, false)?;
                let cash_ids = cash_accounts(&db, scope.company_id)?;
                let mut inflow = Decimal::ZERO;
                let mut outflow = Decimal::ZERO;
                let mut rows = Vec::new();
                for line in lines.iter().filter(|l| cash_ids.contains(&l.account_id)) {
                    let net = line.debit - line.credit;
                    if net > Decimal::ZERO {
                        inflow += net;
                    } else {
                        outflow += -net;
                    }
                    rows.push(serde_json::json!({
                        "code": line.code, "name": line.name, "net": render(net)
                    }));
                }
                serde_json::json!({"report":"cashflow","currency":currency,
                    "inflow":render(inflow),"outflow":render(outflow),
                    "net":render(inflow - outflow),"accounts":rows})
            }
            "general" => {
                let mut stmt = db.prepare(
                    "SELECT e.id,e.entry_date,e.description,e.currency,e.total
                     FROM journal_entries e
                     WHERE e.finalized=1 AND e.company_id=?1 AND e.currency=?2
                       AND e.entry_date>=?3 AND e.entry_date<=?4
                       AND (?5 IS NULL OR e.branch_id=?5)
                     ORDER BY e.entry_date, e.id",
                )?;
                let from = q.from.clone().unwrap_or_else(|| "0000-01-01".into());
                let to = q.to.clone().unwrap_or_else(|| "9999-12-31".into());
                let rows = stmt.query_map(
                    params![scope.company_id, currency, from, to, q.branch_id],
                    |r| {
                    Ok(serde_json::json!({
                        "id": r.get::<_, i64>(0)?, "date": r.get::<_, String>(1)?,
                        "description": r.get::<_, String>(2)?, "currency": r.get::<_, String>(3)?,
                        "total": r.get::<_, String>(4)?
                    }))
                },
                )?;
                serde_json::json!({"report":"general","currency":currency,
                    "entries": rows.collect::<Result<Vec<_>, _>>()?})
            }
            _ => {
                let lines = movements(&db, scope.company_id, &currency, &q, true)?;
                let mut rows = Vec::new();
                let (mut debit_total, mut credit_total) = (Decimal::ZERO, Decimal::ZERO);
                for line in &lines {
                    debit_total += line.debit;
                    credit_total += line.credit;
                    rows.push(serde_json::json!({
                        "account_id": line.account_id, "code": line.code, "name": line.name,
                        "debit": render(line.debit), "credit": render(line.credit),
                        "balance": render(line.debit - line.credit)
                    }));
                }
                serde_json::json!({"report":"trial","currency":currency,"accounts":rows,
                    "totals":{"debit":render(debit_total),"credit":render(credit_total),
                              "balanced":debit_total==credit_total}})
            }
        };
        Ok(body)
    })()
}

/// حسابات النقد: حسابات الصناديق والبنوك المسجّلة للشركة (مصدر واحد للحقيقة).
fn cash_accounts(db: &Connection, company_id: i64) -> Result<Vec<i64>, FinanceError> {
    let mut stmt = db.prepare("SELECT account_id FROM finance_funds WHERE company_id=?1")?;
    let rows = stmt.query_map([company_id], |r| r.get::<_, i64>(0))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}
