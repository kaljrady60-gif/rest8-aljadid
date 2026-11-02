use crate::{AppState, db_open, domain, fail};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use rusqlite::{Connection, OptionalExtension, params};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, str::FromStr};

#[derive(Deserialize)]
pub struct Filters {
    pub(super) currency: Option<String>,
    pub(super) account_id: Option<i64>,
    pub(super) from: Option<String>,
    pub(super) to: Option<String>,
}
pub(super) fn check_filters(f: &Filters) -> Result<String, &'static str> {
    let currency = f.currency.as_deref().unwrap_or("YER");
    if !domain::supported_currency(currency) {
        return Err("العملة غير مدعومة.");
    }
    for d in [&f.from, &f.to].into_iter().flatten() {
        if chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err() || d.len() != 10 {
            return Err("التاريخ غير صالح.");
        }
    }
    if f.from
        .as_ref()
        .zip(f.to.as_ref())
        .is_some_and(|(a, b)| a > b)
    {
        return Err("بداية الفترة بعد نهايتها.");
    }
    if f.account_id.is_some_and(|x| x <= 0) {
        return Err("الحساب غير صالح.");
    }
    Ok(currency.to_owned())
}
#[derive(Serialize)]
pub(super) struct Movement {
    entry_id: i64,
    draft_id: i64,
    date: String,
    description: String,
    posted_by: Option<String>,
    account_id: i64,
    account_code: String,
    account_name: String,
    account_snapshot: String,
    debit: String,
    credit: String,
}
fn read_movements(
    db: &Connection,
    f: &Filters,
    currency: &str,
) -> Result<Vec<Movement>, Box<dyn std::error::Error>> {
    // Deliberately return an explicit error instead of silently truncating financial reports.
    let mut st=db.prepare("SELECT e.id,e.draft_id,e.entry_date,e.description,l.account_id,a.code,a.name,l.account_snapshot,l.debit,l.credit,u.username FROM journal_entries e JOIN journal_lines l ON l.entry_id=e.id JOIN accounts a ON a.id=l.account_id LEFT JOIN identity_users u ON u.id=e.posted_by WHERE e.finalized=1 AND e.currency=?1 AND (?2 IS NULL OR e.entry_date>=?2) AND (?3 IS NULL OR e.entry_date<=?3) AND (?4 IS NULL OR l.account_id=?4) ORDER BY e.entry_date,e.id,l.line_no LIMIT 100001")?;
    let rows = st.query_map(params![currency, f.from, f.to, f.account_id], |r| {
        Ok(Movement {
            entry_id: r.get(0)?,
            draft_id: r.get(1)?,
            date: r.get(2)?,
            description: r.get(3)?,
            posted_by: r.get(10)?,
            account_id: r.get(4)?,
            account_code: r.get(5)?,
            account_name: r.get(6)?,
            account_snapshot: r.get(7)?,
            debit: r.get(8)?,
            credit: r.get(9)?,
        })
    })?;
    let result: Vec<_> = rows.collect::<Result<_, _>>()?;
    check_report_size(result.len())?;
    Ok(result)
}
pub(super) fn check_report_size(count: usize) -> Result<(), Box<dyn std::error::Error>> {
    if count > 100_000 {
        return Err(
            std::io::Error::other("report exceeds 100000 movement limit; refine filters").into(),
        );
    }
    Ok(())
}
// The caller owns the snapshot so account metadata and movements cannot diverge.
pub(super) fn validated_movements_in_snapshot(
    db: &Connection,
    f: &Filters,
    currency: &str,
) -> Result<Vec<Movement>, Box<dyn std::error::Error>> {
    {
        let mut headers=db.prepare("SELECT id,description,entry_date,total,finalized FROM journal_entries WHERE currency=?1 AND (?2 IS NULL OR entry_date>=?2) AND (?3 IS NULL OR entry_date<=?3)")?;
        let entries = headers.query_map(params![currency, f.from, f.to], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, bool>(4)?,
            ))
        })?;
        for e in entries {
            let (id, description, date, total, finalized) = e?;
            if !finalized {
                return Err("unfinalized journal entry found".into());
            }
            let mut st=db.prepare("SELECT l.line_no,l.account_id,l.account_snapshot,l.debit,l.credit,a.id FROM journal_lines l LEFT JOIN accounts a ON a.id=l.account_id WHERE l.entry_id=?1 ORDER BY l.line_no")?;
            let lines = st
                .query_map([id], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        domain::LineInput {
                            account_id: Some(r.get(1)?),
                            account: r.get(2)?,
                            debit: r.get(3)?,
                            credit: r.get(4)?,
                        },
                        r.get::<_, Option<i64>>(5)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            if lines
                .iter()
                .enumerate()
                .any(|(n, (i, _, a))| *i != n as i64 || a.is_none())
            {
                return Err("journal line order or account reference mismatch".into());
            }
            let draft = domain::DraftInput {
                description,
                date,
                currency: currency.to_string(),
                lines: lines.into_iter().map(|(_, l, _)| l).collect(),
            };
            let actual = domain::validate(&draft).map_err(std::io::Error::other)?;
            // المقارنة رقمية لا نصية: 1200000 و 1200000.0000 مبلغ واحد، وصيغة التخزين ليست حكمًا.
            let stored_total = total
                .trim()
                .parse::<Decimal>()
                .map_err(|_| "stored journal total is corrupt")?;
            let computed_total = actual
                .debit
                .trim()
                .parse::<Decimal>()
                .map_err(|_| "computed journal total is corrupt")?;
            if computed_total != stored_total {
                return Err("journal total mismatch".into());
            }
        }
    }
    read_movements(db, f, currency)
}
fn validated_movements(
    db: &Connection,
    f: &Filters,
    currency: &str,
) -> Result<Vec<Movement>, Box<dyn std::error::Error>> {
    let tx = db.unchecked_transaction()?;
    let result = validated_movements_in_snapshot(&tx, f, currency)?;
    tx.commit()?;
    Ok(result)
}
pub async fn entries(State(s): State<AppState>, Query(f): Query<Filters>) -> Response {
    let currency = match check_filters(&f) {
        Ok(x) => x,
        Err(e) => return fail(StatusCode::BAD_REQUEST, e),
    };
    match db_open(&s.db)
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
        .and_then(|db| validated_movements(&db, &f, &currency))
    {
        Ok(rows) => Json(serde_json::json!({"currency":currency,"movements":rows})).into_response(),
        Err(e) => {
            tracing::error!(%e,"ledger report failed");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "تعذر تحميل الأستاذ أو تجاوز التقرير الحد؛ ضيّق الفترة وراجع سجل الخادم.",
            )
        }
    }
}
#[derive(Serialize)]
struct TrialRow {
    account_id: i64,
    code: String,
    name: String,
    debit_turnover: String,
    credit_turnover: String,
    debit_balance: String,
    credit_balance: String,
}
// The running balance includes all posted movements through the end date, even when
// the display starts later. No separate opening-balance module exists yet.
pub async fn account_ledger(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Query(f): Query<Filters>,
) -> Response {
    if id <= 0 || f.account_id.is_some() {
        return fail(StatusCode::BAD_REQUEST, "معرّف الحساب أو المرشح غير صالح.");
    }
    let currency = match check_filters(&f) {
        Ok(v) => v,
        Err(e) => return fail(StatusCode::BAD_REQUEST, e),
    };
    enum ReadError {
        Missing,
        Invalid,
        Storage(Box<dyn std::error::Error>),
    }
    let result = (|| -> Result<serde_json::Value, ReadError> {
        let db = db_open(&s.db).map_err(|e| ReadError::Storage(Box::new(e)))?;
        let tx = db
            .unchecked_transaction()
            .map_err(|e| ReadError::Storage(Box::new(e)))?;
        let account: Option<(String, String, String, String)> = tx
            .query_row(
                "SELECT code,name,currency,kind FROM accounts WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(|e| ReadError::Storage(Box::new(e)))?;
        let (code, name, account_currency, kind) = account.ok_or(ReadError::Missing)?;
        if account_currency != currency || kind != "detail" {
            return Err(ReadError::Invalid);
        }
        let query = Filters {
            currency: Some(currency.clone()),
            account_id: Some(id),
            from: None,
            to: f.to.clone(),
        };
        let movements =
            validated_movements_in_snapshot(&tx, &query, &currency).map_err(ReadError::Storage)?;
        tx.commit().map_err(|e| ReadError::Storage(Box::new(e)))?;
        let mut running = Decimal::ZERO;
        let mut opening = Decimal::ZERO;
        let mut rows = Vec::new();
        for m in movements {
            let parse = |x: &str| -> Result<Decimal, ReadError> {
                if x.trim().is_empty() {
                    Ok(Decimal::ZERO)
                } else {
                    Decimal::from_str(x.trim()).map_err(|e| ReadError::Storage(Box::new(e)))
                }
            };
            let debit = parse(&m.debit)?;
            let credit = parse(&m.credit)?;
            running = running
                .checked_add(debit)
                .and_then(|x| x.checked_sub(credit))
                .ok_or_else(|| ReadError::Storage("account balance overflow".into()))?;
            if f.from.as_ref().is_some_and(|date| m.date < *date) {
                opening = running;
                continue;
            }
            rows.push(serde_json::json!({"movement":m,"running_debit":if running>Decimal::ZERO {running.normalize().to_string()}else{"0".into()},"running_credit":if running<Decimal::ZERO {(-running).normalize().to_string()}else{"0".into()}}));
        }
        Ok(
            serde_json::json!({"account_id":id,"code":code,"name":name,"currency":currency,"opening_debit":if opening>Decimal::ZERO {opening.normalize().to_string()}else{"0".into()},"opening_credit":if opening<Decimal::ZERO {(-opening).normalize().to_string()}else{"0".into()},"closing_debit":if running>Decimal::ZERO {running.normalize().to_string()}else{"0".into()},"closing_credit":if running<Decimal::ZERO {(-running).normalize().to_string()}else{"0".into()},"rows":rows}),
        )
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(ReadError::Missing) => fail(StatusCode::NOT_FOUND, "الحساب غير موجود."),
        Err(ReadError::Invalid) => fail(
            StatusCode::UNPROCESSABLE_ENTITY,
            "اختر حسابًا تفصيليًا بعملة التقرير.",
        ),
        Err(ReadError::Storage(e)) => {
            tracing::error!(%e,id,"account ledger failed");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "تعذر التحقق من حركات الحساب أو تجاوز التقرير الحد؛ راجع سجل الخادم.",
            )
        }
    }
}
pub async fn trial(State(s): State<AppState>, Query(f): Query<Filters>) -> Response {
    if f.from.is_some() {
        return fail(
            StatusCode::BAD_REQUEST,
            "ميزان المراجعة يحسب الرصيد منذ أول قيد وحتى تاريخ النهاية؛ لا تقبل بداية فترة حاليًا.",
        );
    }
    if f.account_id.is_some() {
        return fail(
            StatusCode::BAD_REQUEST,
            "ميزان المراجعة يعرض جميع الحسابات، لا حسابًا واحدًا.",
        );
    }
    let currency = match check_filters(&f) {
        Ok(x) => x,
        Err(e) => return fail(StatusCode::BAD_REQUEST, e),
    };
    let result = (|| -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let db = db_open(&s.db)?;
        let movements = validated_movements(&db, &f, &currency)?;
        let mut balances: BTreeMap<i64, (String, String, Decimal, Decimal)> = BTreeMap::new();
        for m in movements {
            let parse = |x: &str| -> Result<Decimal, Box<dyn std::error::Error>> {
                if x.trim().is_empty() {
                    Ok(Decimal::ZERO)
                } else {
                    Ok(Decimal::from_str(x.trim())?)
                }
            };
            let row = balances.entry(m.account_id).or_insert((
                m.account_code,
                m.account_name,
                Decimal::ZERO,
                Decimal::ZERO,
            ));
            row.2 = row
                .2
                .checked_add(parse(&m.debit)?)
                .ok_or("debit overflow")?;
            row.3 = row
                .3
                .checked_add(parse(&m.credit)?)
                .ok_or("credit overflow")?;
        }
        let mut sum_debit = Decimal::ZERO;
        let mut sum_credit = Decimal::ZERO;
        let mut rows = Vec::new();
        for (id, (code, name, d, c)) in balances {
            let diff = d.checked_sub(c).ok_or("balance overflow")?;
            let (dr, cr) = if diff >= Decimal::ZERO {
                (diff, Decimal::ZERO)
            } else {
                (Decimal::ZERO, -diff)
            };
            sum_debit = sum_debit.checked_add(dr).ok_or("sum overflow")?;
            sum_credit = sum_credit.checked_add(cr).ok_or("sum overflow")?;
            rows.push(TrialRow {
                account_id: id,
                code,
                name,
                debit_turnover: d.normalize().to_string(),
                credit_turnover: c.normalize().to_string(),
                debit_balance: dr.normalize().to_string(),
                credit_balance: cr.normalize().to_string(),
            });
        }
        if sum_debit != sum_credit {
            return Err("trial balance mismatch; possible corruption or invalid filtering".into());
        }
        Ok(
            serde_json::json!({"currency":currency,"rows":rows,"debit_total":sum_debit.normalize().to_string(),"credit_total":sum_credit.normalize().to_string()}),
        )
    })();
    match result {
        Ok(data) => Json(data).into_response(),
        Err(e) => {
            tracing::error!(%e,"trial report failed");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "تعذر التحقق من ميزان المراجعة؛ راجع سجل الخادم.",
            )
        }
    }
}
