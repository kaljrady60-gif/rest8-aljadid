//! الموظفون والرواتب: قائمة موظفين، مسير شهري، ترحيل المسير، ثم صرفه من صندوق.
//!
//! المسير يُبنى من الموظفين النشطين لحظة الإنشاء (لقطة ثابتة)، ثم يُرحّل قيدًا
//! واحدًا متوازنًا عبر `gl::post_entry`: مصروف الرواتب مقابل مستحقات الموظفين
//! والاستقطاعات. ولا يُسمح بمسيرين لنفس الشركة والشهر.
use super::audit;
use super::gl;
use super::money::{self, render};
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use rust_decimal::Decimal;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct CompanyQuery {
    company_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewEmployee {
    company_id: i64,
    code: String,
    name: String,
    #[serde(default)]
    job_title: Option<String>,
    hired_on: String,
    base_salary: String,
    #[serde(default)]
    allowances: Option<String>,
    #[serde(default)]
    deductions: Option<String>,
    #[serde(default)]
    currency: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewRun {
    company_id: i64,
    /// الشهر بصيغة `YYYY-MM`.
    period: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayRun {
    company_id: i64,
    #[serde(default)]
    fund_id: Option<i64>,
    #[serde(default)]
    date: Option<String>,
}

fn amount_or_zero(raw: Option<&str>, field: &str) -> Result<Decimal, FinanceError> {
    match raw {
        Some(text) => money::parse(text, field),
        None => Ok(Decimal::ZERO),
    }
}

/// أول حساب متاح من قائمة رموز مقترحة.
fn pick(
    tx: &rusqlite::Connection,
    company_id: i64,
    codes: &[&str],
    label: &str,
) -> Result<i64, FinanceError> {
    for code in codes {
        if let Some(id) = gl::account_by_code(tx, company_id, code)? {
            return Ok(id);
        }
    }
    Err(FinanceError::Invalid(format!(
        "لا يوجد حساب مناسب لـ{label}؛ أضفه إلى دليل الحسابات."
    )))
}

pub async fn employees(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<CompanyQuery>,
) -> Response {
    let result = (|| -> Result<Vec<serde_json::Value>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT id,code,name,job_title,hired_on,base_salary,allowances,deductions,currency,status
             FROM finance_employees WHERE company_id=?1 ORDER BY code LIMIT 400",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            let base = r.get::<_, String>(5)?;
            let allowances = r.get::<_, String>(6)?;
            let deductions = r.get::<_, String>(7)?;
            let gross = money::from_db(&base, "الراتب").unwrap_or(Decimal::ZERO)
                + money::from_db(&allowances, "البدلات").unwrap_or(Decimal::ZERO);
            let net = gross - money::from_db(&deductions, "الاستقطاعات").unwrap_or(Decimal::ZERO);
            Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?,
                "code": r.get::<_, String>(1)?,
                "name": r.get::<_, String>(2)?,
                "job_title": r.get::<_, Option<String>>(3)?,
                "hired_on": r.get::<_, String>(4)?,
                "base_salary": base,
                "allowances": allowances,
                "deductions": deductions,
                "gross": render(gross),
                "net": render(net),
                "currency": r.get::<_, String>(8)?,
                "status": r.get::<_, String>(9)?
            }))
        })?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    })();
    match result {
        Ok(items) => Json(items).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn create_employee(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<NewEmployee>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        if n.code.trim().len() < 2 || n.name.trim().len() < 2 {
            return Err(FinanceError::Invalid("رمز الموظف واسمه مطلوبان.".into()));
        }
        let base = money::parse(&n.base_salary, "الراتب الأساسي")?;
        if base <= Decimal::ZERO {
            return Err(FinanceError::Invalid(
                "الراتب الأساسي يجب أن يكون أكبر من صفر.".into(),
            ));
        }
        let allowances = amount_or_zero(n.allowances.as_deref(), "البدلات")?;
        let deductions = amount_or_zero(n.deductions.as_deref(), "الاستقطاعات")?;
        if deductions >= base + allowances {
            return Err(FinanceError::Invalid(
                "الاستقطاعات لا يجوز أن تساوي الصافي أو تتجاوزه.".into(),
            ));
        }
        let currency = n.currency.clone().unwrap_or_else(|| "YER".into());
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        let branch: i64 = tx
            .query_row(
                "SELECT id FROM finance_branches WHERE company_id=?1 ORDER BY id LIMIT 1",
                [scope.company_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| FinanceError::Invalid("لا يوجد فرع للشركة.".into()))?;
        tx.execute(
            "INSERT INTO finance_employees
             (company_id,branch_id,code,name,job_title,hired_on,base_salary,allowances,deductions,currency,status)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'active')",
            params![
                scope.company_id,
                branch,
                n.code.trim(),
                n.name.trim(),
                n.job_title.as_deref(),
                n.hired_on,
                render(base),
                render(allowances),
                render(deductions),
                currency
            ],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(_, _) => {
                FinanceError::Conflict("رمز الموظف مستخدم مسبقًا.")
            }
            other => FinanceError::Storage(other),
        })?;
        let id = tx.last_insert_rowid();
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "employee.create",
                entity: "finance_employees",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({"code": n.code.trim(), "base": render(base)})),
            },
        )?;
        tx.commit()?;
        Ok(id)
    })();
    match result {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn runs(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<CompanyQuery>,
) -> Response {
    let result = (|| -> Result<Vec<serde_json::Value>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT r.id,r.period,r.status,r.gross_total,r.deduction_total,r.net_total,r.currency,
                    r.entry_id,r.payment_entry_id,
                    (SELECT COUNT(*) FROM finance_payroll_lines l WHERE l.run_id=r.id)
             FROM finance_payroll_runs r WHERE r.company_id=?1 ORDER BY r.period DESC LIMIT 120",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?,
                "period": r.get::<_, String>(1)?,
                "status": r.get::<_, String>(2)?,
                "gross": r.get::<_, String>(3)?,
                "deductions": r.get::<_, String>(4)?,
                "net": r.get::<_, String>(5)?,
                "currency": r.get::<_, String>(6)?,
                "entry_id": r.get::<_, Option<i64>>(7)?,
                "payment_entry_id": r.get::<_, Option<i64>>(8)?,
                "employees": r.get::<_, i64>(9)?
            }))
        })?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    })();
    match result {
        Ok(items) => Json(items).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn run_lines(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Path(id): Path<i64>,
    Query(q): Query<CompanyQuery>,
) -> Response {
    let result = (|| -> Result<Vec<serde_json::Value>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let owned: Option<i64> = db
            .query_row(
                "SELECT id FROM finance_payroll_runs WHERE id=?1 AND company_id=?2",
                params![id, scope.company_id],
                |r| r.get(0),
            )
            .optional()?;
        if owned.is_none() {
            return Err(FinanceError::NotFound("المسير غير موجود لهذه الشركة."));
        }
        let mut stmt = db.prepare(
            "SELECT employee_id,employee_name,base_salary,allowances,deductions,net
             FROM finance_payroll_lines WHERE run_id=?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([id], |r| {
            Ok(serde_json::json!({
                "employee_id": r.get::<_, i64>(0)?,
                "name": r.get::<_, String>(1)?,
                "base": r.get::<_, String>(2)?,
                "allowances": r.get::<_, String>(3)?,
                "deductions": r.get::<_, String>(4)?,
                "net": r.get::<_, String>(5)?
            }))
        })?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    })();
    match result {
        Ok(items) => Json(items).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn create_run(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<NewRun>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        let period = n.period.trim();
        if period.len() != 7 || !period.contains('-') {
            return Err(FinanceError::Invalid(
                "صيغة الشهر يجب أن تكون YYYY-MM.".into(),
            ));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        let branch: i64 = tx
            .query_row(
                "SELECT id FROM finance_branches WHERE company_id=?1 ORDER BY id LIMIT 1",
                [scope.company_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| FinanceError::Invalid("لا يوجد فرع للشركة.".into()))?;
        let mut stmt = tx.prepare(
            "SELECT id,code,name,base_salary,allowances,deductions,currency
             FROM finance_employees WHERE company_id=?1 AND status='active' ORDER BY code",
        )?;
        let rows = stmt
            .query_map([scope.company_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        if rows.is_empty() {
            return Err(FinanceError::Invalid(
                "لا يوجد موظفون نشطون لبناء المسير.".into(),
            ));
        }
        let mut gross_total = Decimal::ZERO;
        let mut ded_total = Decimal::ZERO;
        let mut currency = "YER".to_string();
        let mut lines = Vec::new();
        for (id, code, name, base_raw, allow_raw, ded_raw, cur) in rows {
            let base = money::from_db(&base_raw, "الراتب")?;
            let allowances = money::from_db(&allow_raw, "البدلات")?;
            let deductions = money::from_db(&ded_raw, "الاستقطاعات")?;
            let gross = base + allowances;
            let net = gross - deductions;
            if net < Decimal::ZERO {
                return Err(FinanceError::Invalid(format!(
                    "صافي راتب الموظف {code} سالب؛ راجع الاستقطاعات."
                )));
            }
            gross_total += gross;
            ded_total += deductions;
            currency = cur;
            lines.push((id, name, base, allowances, deductions, net));
        }
        tx.execute(
            "INSERT INTO finance_payroll_runs
             (company_id,branch_id,period,status,gross_total,deduction_total,net_total,currency,created_by)
             VALUES(?1,?2,?3,'draft',?4,?5,?6,?7,?8)",
            params![
                scope.company_id,
                branch,
                period,
                render(gross_total),
                render(ded_total),
                render(gross_total - ded_total),
                currency,
                actor.id
            ],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(_, _) => {
                FinanceError::Conflict("يوجد مسير رواتب لهذا الشهر مسبقًا.")
            }
            other => FinanceError::Storage(other),
        })?;
        let run_id = tx.last_insert_rowid();
        for (id, name, base, allowances, deductions, net) in lines {
            tx.execute(
                "INSERT INTO finance_payroll_lines
                 (run_id,employee_id,employee_name,base_salary,allowances,deductions,net)
                 VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    run_id,
                    id,
                    name,
                    render(base),
                    render(allowances),
                    render(deductions),
                    render(net)
                ],
            )?;
        }
        tx.commit()?;
        Ok(run_id)
    })();
    match result {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn post_run(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Path(id): Path<i64>,
    Json(n): Json<CompanyQuery>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        let row: Option<(String, String, String, String, String, String)> = tx
            .query_row(
                "SELECT period,status,gross_total,deduction_total,net_total,currency
                 FROM finance_payroll_runs WHERE id=?1 AND company_id=?2",
                params![id, scope.company_id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .optional()?;
        let (period, status, gross_raw, ded_raw, net_raw, currency) =
            row.ok_or_else(|| FinanceError::NotFound("المسير غير موجود لهذه الشركة."))?;
        if status != "draft" {
            return Err(FinanceError::Conflict("المسير مُرحّل أو مدفوع مسبقًا."));
        }
        let gross = money::from_db(&gross_raw, "إجمالي الرواتب")?;
        if gross <= Decimal::ZERO {
            return Err(FinanceError::Invalid("المسير بلا مبالغ.".into()));
        }
        let deductions = money::from_db(&ded_raw, "الاستقطاعات")?;
        let net = money::from_db(&net_raw, "الصافي")?;
        let expense = pick(&tx, scope.company_id, &["5200", "5100"], "مصروف الرواتب")?;
        let payable = pick(
            &tx,
            scope.company_id,
            &["2200", "2400", "2100"],
            "رواتب مستحقة",
        )?;
        let deductions_account = pick(
            &tx,
            scope.company_id,
            &["2300", "2400", "2200"],
            "الاستقطاعات",
        )?;
        let date = format!("{period}-28");
        let ctx = gl::context(&tx, scope.company_id, &date)?;
        let mut lines = vec![
            (expense, gross, Decimal::ZERO),
            (payable, Decimal::ZERO, net),
        ];
        if deductions > Decimal::ZERO {
            lines.push((deductions_account, Decimal::ZERO, deductions));
        }
        let entry_id = gl::post_entry(
            &tx,
            ctx,
            &date,
            &format!("مسير رواتب {period}"),
            &currency,
            &lines,
            Some(actor.id),
            "payroll.post",
        )?;
        tx.execute(
            "UPDATE finance_payroll_runs SET status='posted', entry_id=?1 WHERE id=?2",
            params![entry_id, id],
        )?;
        tx.commit()?;
        Ok(entry_id)
    })();
    match result {
        Ok(entry) => Json(serde_json::json!({"entry_id": entry})).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn pay_run(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Path(id): Path<i64>,
    Json(n): Json<PayRun>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        let row: Option<(String, String, String, String)> = tx
            .query_row(
                "SELECT period,status,net_total,currency FROM finance_payroll_runs
                 WHERE id=?1 AND company_id=?2",
                params![id, scope.company_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let (period, status, net_raw, currency) =
            row.ok_or_else(|| FinanceError::NotFound("المسير غير موجود لهذه الشركة."))?;
        if status != "posted" {
            return Err(FinanceError::Conflict("لا يُصرف مسير قبل ترحيله."));
        }
        let net = money::from_db(&net_raw, "صافي الرواتب")?;
        let payable = pick(
            &tx,
            scope.company_id,
            &["2200", "2400", "2100"],
            "رواتب مستحقة",
        )?;
        let fund_account: i64 = match n.fund_id {
            Some(fund) => tx
                .query_row(
                    "SELECT account_id FROM finance_funds WHERE id=?1 AND company_id=?2",
                    params![fund, scope.company_id],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or_else(|| FinanceError::Invalid("الصندوق ليس من هذه الشركة.".into()))?,
            None => pick(&tx, scope.company_id, &["1120", "1110"], "الصندوق أو البنك")?,
        };
        let date = n.date.clone().unwrap_or_else(|| format!("{period}-28"));
        let ctx = gl::context(&tx, scope.company_id, &date)?;
        let entry_id = gl::post_entry(
            &tx,
            ctx,
            &date,
            &format!("صرف رواتب {period}"),
            &currency,
            &[
                (payable, net, Decimal::ZERO),
                (fund_account, Decimal::ZERO, net),
            ],
            Some(actor.id),
            "payroll.pay",
        )?;
        tx.execute(
            "UPDATE finance_payroll_runs SET status='paid', payment_entry_id=?1 WHERE id=?2",
            params![entry_id, id],
        )?;
        tx.commit()?;
        Ok(entry_id)
    })();
    match result {
        Ok(entry) => Json(serde_json::json!({"entry_id": entry})).into_response(),
        Err(e) => error_response(e),
    }
}
