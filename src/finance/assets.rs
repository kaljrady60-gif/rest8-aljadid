//! الأصول الثابتة: تصنيفات بطريقة إهلاك، أصول، إهلاك شهري دوري، واستبعاد.
//!
//! كل أثر محاسبي يمرّ من `gl::post_entry`: شراء الأصل، الإهلاك الشهري، والاستبعاد.
//! حساب الأصل ومجمعه ومصروف إهلاكه تُحدَّد في التصنيف، فلا تُثبَّت أرقام حسابات
//! في الشيفرة، وأي تصنيف بلا حسابات صحيحة يُرفض بوضوح.
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
pub struct NewCategory {
    company_id: i64,
    name: String,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    annual_rate: Option<String>,
    asset_account_id: i64,
    accum_account_id: i64,
    expense_account_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewAsset {
    company_id: i64,
    category_id: i64,
    code: String,
    name: String,
    acquired_on: String,
    cost: String,
    #[serde(default)]
    salvage: Option<String>,
    life_months: i64,
    #[serde(default)]
    funded_from: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Depreciate {
    company_id: i64,
    /// شهر الإهلاك بصيغة `YYYY-MM`.
    period: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dispose {
    company_id: i64,
    date: String,
    #[serde(default)]
    proceeds: Option<String>,
}

/// حسابات التصنيف الثلاثة بعد التحقق أنها من دليل الشركة.
struct Accounts {
    asset: i64,
    accum: i64,
    expense: i64,
}

fn accounts_of(
    tx: &rusqlite::Connection,
    scope: &Scope,
    category_id: i64,
) -> Result<(String, Accounts), FinanceError> {
    let row: Option<(String, String, i64, i64, i64)> = tx
        .query_row(
            "SELECT name, method, asset_account_id, accum_account_id, expense_account_id
             FROM finance_asset_categories WHERE id=?1 AND company_id=?2",
            params![category_id, scope.company_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    let (_name, method, asset, accum, expense) =
        row.ok_or_else(|| FinanceError::NotFound("تصنيف الأصل غير موجود لهذه الشركة."))?;
    for account in [asset, accum, expense] {
        if !scope.owns_account(tx, account)? {
            return Err(FinanceError::Invalid(
                "حسابات التصنيف يجب أن تكون من دليل حسابات الشركة.".into(),
            ));
        }
    }
    Ok((
        method,
        Accounts {
            asset,
            accum,
            expense,
        },
    ))
}

/// حساب مصدر التمويل عند الشراء.
fn funding_account(
    tx: &rusqlite::Connection,
    company_id: i64,
    source: &str,
) -> Result<i64, FinanceError> {
    let codes: &[&str] = match source {
        "cash" => &["1110"],
        "payable" => &["2100", "2400"],
        "capital" => &["3100"],
        _ => &["1120", "1110"],
    };
    for code in codes {
        if let Some(id) = gl::account_by_code(tx, company_id, code)? {
            return Ok(id);
        }
    }
    Err(FinanceError::Invalid(format!(
        "لا يوجد حساب مناسب لطريقة التمويل «{source}»."
    )))
}

pub async fn categories(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<CompanyQuery>,
) -> Response {
    let result = (|| -> Result<Vec<serde_json::Value>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT c.id,c.name,c.method,c.annual_rate,a.code,a.name,k.code,k.name,x.code,x.name
             FROM finance_asset_categories c
             JOIN accounts a ON a.id=c.asset_account_id
             JOIN accounts k ON k.id=c.accum_account_id
             JOIN accounts x ON x.id=c.expense_account_id
             WHERE c.company_id=?1 ORDER BY c.id",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?,
                "name": r.get::<_, String>(1)?,
                "method": r.get::<_, String>(2)?,
                "annual_rate": r.get::<_, String>(3)?,
                "asset_account": format!("{} · {}", r.get::<_, String>(4)?, r.get::<_, String>(5)?),
                "accum_account": format!("{} · {}", r.get::<_, String>(6)?, r.get::<_, String>(7)?),
                "expense_account": format!("{} · {}", r.get::<_, String>(8)?, r.get::<_, String>(9)?)
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

pub async fn create_category(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<NewCategory>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        if n.name.trim().len() < 2 {
            return Err(FinanceError::Invalid("اسم التصنيف قصير جدًا.".into()));
        }
        let method = n.method.clone().unwrap_or_else(|| "straight".into());
        if method != "straight" && method != "declining" {
            return Err(FinanceError::Invalid(
                "طريقة الإهلاك: قسط ثابت أو متناقص.".into(),
            ));
        }
        let rate = match n.annual_rate.as_deref() {
            Some(text) => money::parse(text, "نسبة الإهلاك")?,
            None => Decimal::ZERO,
        };
        if method == "declining" && (rate <= Decimal::ZERO || rate > Decimal::from(100)) {
            return Err(FinanceError::Invalid(
                "الإهلاك المتناقص يحتاج نسبة سنوية بين 0 و100.".into(),
            ));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        scope.require_admin()?;
        for account in [n.asset_account_id, n.accum_account_id, n.expense_account_id] {
            if !scope.owns_account(&tx, account)? {
                return Err(FinanceError::Invalid(
                    "حسابات التصنيف يجب أن تكون من دليل حسابات الشركة.".into(),
                ));
            }
        }
        tx.execute(
            "INSERT INTO finance_asset_categories
             (company_id,name,method,annual_rate,asset_account_id,accum_account_id,expense_account_id)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                scope.company_id,
                n.name.trim(),
                method,
                render(rate),
                n.asset_account_id,
                n.accum_account_id,
                n.expense_account_id
            ],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(_, _) => {
                FinanceError::Conflict("يوجد تصنيف بنفس الاسم.")
            }
            other => FinanceError::Storage(other),
        })?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    })();
    match result {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn list(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<CompanyQuery>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT s.id,s.code,s.name,c.name,s.acquired_on,s.cost,s.salvage,s.life_months,
                    s.accumulated,s.status,s.acquired_entry_id
             FROM finance_assets s JOIN finance_asset_categories c ON c.id=s.category_id
             WHERE s.company_id=?1 ORDER BY s.code LIMIT 400",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, String>(9)?,
                r.get::<_, Option<i64>>(10)?,
            ))
        })?;
        let mut items = Vec::new();
        let mut cost_sum = Decimal::ZERO;
        let mut acc_sum = Decimal::ZERO;
        for row in rows {
            let (
                id,
                code,
                name,
                category,
                acquired,
                cost_raw,
                salvage_raw,
                life,
                acc_raw,
                status,
                entry,
            ) = row?;
            let cost = money::from_db(&cost_raw, "تكلفة الأصل")?;
            let salvage = money::from_db(&salvage_raw, "القيمة التخريدية")?;
            let accumulated = money::from_db(&acc_raw, "مجمع الإهلاك")?;
            cost_sum += cost;
            acc_sum += accumulated;
            items.push(serde_json::json!({
                "id": id, "code": code, "name": name, "category": category,
                "acquired_on": acquired, "cost": render(cost), "salvage": render(salvage),
                "life_months": life, "accumulated": render(accumulated),
                "book_value": render(cost - accumulated),
                "monthly": render(monthly_amount(cost, salvage, life)),
                "status": status, "entry_id": entry
            }));
        }
        Ok(serde_json::json!({
            "items": items,
            "totals": { "cost": render(cost_sum), "accumulated": render(acc_sum),
                        "book_value": render(cost_sum - acc_sum) }
        }))
    })();
    match result {
        Ok(body) => Json(body).into_response(),
        Err(e) => error_response(e),
    }
}

/// القسط الشهري بطريقة القسط الثابت.
fn monthly_amount(cost: Decimal, salvage: Decimal, life_months: i64) -> Decimal {
    let base = (cost - salvage).max(Decimal::ZERO);
    if life_months <= 0 {
        return Decimal::ZERO;
    }
    // تقريب القسط إلى منزلتين نقديتين يمنع تراكم كسور صغيرة في مجمع الإهلاك.
    (base / Decimal::from(life_months)).round_dp(2).normalize()
}

pub async fn create(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<NewAsset>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        if n.code.trim().len() < 2 || n.name.trim().len() < 2 {
            return Err(FinanceError::Invalid("رمز الأصل واسمه مطلوبان.".into()));
        }
        if n.life_months <= 0 || n.life_months > 600 {
            return Err(FinanceError::Invalid(
                "العمر الإنتاجي بالأشهر يجب أن يكون بين 1 و600.".into(),
            ));
        }
        let cost = money::parse(&n.cost, "تكلفة الأصل")?;
        if cost <= Decimal::ZERO {
            return Err(FinanceError::Invalid(
                "تكلفة الأصل يجب أن تكون أكبر من صفر.".into(),
            ));
        }
        let salvage = match n.salvage.as_deref() {
            Some(text) => money::parse(text, "القيمة التخريدية")?,
            None => Decimal::ZERO,
        };
        if salvage >= cost {
            return Err(FinanceError::Invalid(
                "القيمة التخريدية يجب أن تقل عن التكلفة.".into(),
            ));
        }
        let source = n.funded_from.clone().unwrap_or_else(|| "bank".into());
        if !["bank", "cash", "payable", "capital"].contains(&source.as_str()) {
            return Err(FinanceError::Invalid("مصدر تمويل غير مدعوم.".into()));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        let (_method, accounts) = accounts_of(&tx, &scope, n.category_id)?;
        let ctx = gl::context(&tx, scope.company_id, &n.acquired_on)?;
        let counter = funding_account(&tx, scope.company_id, &source)?;
        let entry_id = gl::post_entry(
            &tx,
            ctx,
            &n.acquired_on,
            &format!("شراء أصل ثابت — {}", n.name.trim()),
            "YER",
            &[
                (accounts.asset, cost, Decimal::ZERO),
                (counter, Decimal::ZERO, cost),
            ],
            Some(actor.id),
            "asset.acquire",
        )?;
        tx.execute(
            "INSERT INTO finance_assets
             (company_id,branch_id,category_id,code,name,acquired_on,cost,salvage,life_months,
              funded_from,accumulated,status,acquired_entry_id,created_by)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'0','active',?11,?12)",
            params![
                scope.company_id,
                ctx.branch_id,
                n.category_id,
                n.code.trim(),
                n.name.trim(),
                n.acquired_on,
                render(cost),
                render(salvage),
                n.life_months,
                source,
                entry_id,
                actor.id
            ],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(_, _) => {
                FinanceError::Conflict("رمز الأصل مستخدم مسبقًا.")
            }
            other => FinanceError::Storage(other),
        })?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    })();
    match result {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response(),
        Err(e) => error_response(e),
    }
}

/// إهلاك شهر كامل: قيد واحد لكل أصل، ومجموع في سجل الإهلاك.
pub async fn depreciate(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<Depreciate>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let period = n.period.trim();
        if period.len() != 7 || !period.contains('-') {
            return Err(FinanceError::Invalid(
                "صيغة الشهر يجب أن تكون YYYY-MM.".into(),
            ));
        }
        let date = format!("{period}-01");
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        let ctx = gl::context(&tx, scope.company_id, &date)?;
        let mut stmt = tx.prepare(
            "SELECT s.id,s.code,s.name,s.cost,s.salvage,s.life_months,s.accumulated,
                    c.method,c.annual_rate,c.asset_account_id,c.accum_account_id,c.expense_account_id
             FROM finance_assets s JOIN finance_asset_categories c ON c.id=s.category_id
             WHERE s.company_id=?1 AND s.status='active' ORDER BY s.id",
        )?;
        let rows = stmt
            .query_map([scope.company_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, i64>(9)?,
                    r.get::<_, i64>(10)?,
                    r.get::<_, i64>(11)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        let mut posted = Vec::new();
        let mut total = Decimal::ZERO;
        for (
            id,
            code,
            name,
            cost_raw,
            salvage_raw,
            life,
            acc_raw,
            method,
            rate_raw,
            _a,
            accum,
            expense,
        ) in rows
        {
            let cost = money::from_db(&cost_raw, "تكلفة الأصل")?;
            let salvage = money::from_db(&salvage_raw, "القيمة التخريدية")?;
            let accumulated = money::from_db(&acc_raw, "مجمع الإهلاك")?;
            let base = (cost - salvage).max(Decimal::ZERO);
            let remaining = base - accumulated;
            if remaining <= Decimal::ZERO {
                continue;
            }
            let raw = if method == "declining" {
                let rate = money::from_db(&rate_raw, "نسبة الإهلاك")?;
                ((cost - accumulated) * rate / Decimal::from(1200))
                    .round_dp(2)
                    .normalize()
            } else {
                monthly_amount(cost, salvage, life)
            };
            let amount = raw.min(remaining);
            if amount <= Decimal::ZERO {
                continue;
            }
            let already: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM finance_asset_depreciations WHERE asset_id=?1 AND period=?2)",
                params![id, period],
                |r| r.get(0),
            )?;
            if already {
                continue;
            }
            let entry_id = gl::post_entry(
                &tx,
                ctx,
                &date,
                &format!("إهلاك {period} — {code} {name}"),
                "YER",
                &[
                    (expense, amount, Decimal::ZERO),
                    (accum, Decimal::ZERO, amount),
                ],
                Some(actor.id),
                "asset.depreciate",
            )?;
            let after = accumulated + amount;
            tx.execute(
                "UPDATE finance_assets SET accumulated=?1 WHERE id=?2",
                params![render(after), id],
            )?;
            tx.execute(
                "INSERT INTO finance_asset_depreciations
                 (company_id,asset_id,period,amount,accumulated_after,entry_id)
                 VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    scope.company_id,
                    id,
                    period,
                    render(amount),
                    render(after),
                    entry_id
                ],
            )?;
            total += amount;
            posted.push(serde_json::json!({
                "asset_id": id, "code": code, "amount": render(amount), "entry_id": entry_id
            }));
        }
        tx.commit()?;
        Ok(serde_json::json!({
            "period": period, "count": posted.len(), "total": render(total), "items": posted
        }))
    })();
    match result {
        Ok(body) => Json(body).into_response(),
        Err(e) => error_response(e),
    }
}

/// استبعاد أصل: إخراج التكلفة ومجمع الإهلاك، وإثبات المقابل، والفرق ربحًا أو خسارة.
pub async fn dispose(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Path(id): Path<i64>,
    Json(n): Json<Dispose>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        let proceeds = match n.proceeds.as_deref() {
            Some(text) => money::parse(text, "قيمة البيع")?,
            None => Decimal::ZERO,
        };
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        let row: Option<(i64, String, String, String, String, String)> = tx
            .query_row(
                "SELECT category_id,code,name,cost,accumulated,status FROM finance_assets
                 WHERE id=?1 AND company_id=?2",
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
        let (category_id, code, name, cost_raw, acc_raw, status) =
            row.ok_or_else(|| FinanceError::NotFound("الأصل غير موجود لهذه الشركة."))?;
        if status == "disposed" {
            return Err(FinanceError::Conflict("الأصل مستبعد مسبقًا."));
        }
        let (_, accounts) = accounts_of(&tx, &scope, category_id)?;
        let cost = money::from_db(&cost_raw, "تكلفة الأصل")?;
        let accumulated = money::from_db(&acc_raw, "مجمع الإهلاك")?;
        let book = cost - accumulated;
        let gain = proceeds - book;
        let ctx = gl::context(&tx, scope.company_id, &n.date)?;
        let mut lines = vec![
            (accounts.accum, accumulated, Decimal::ZERO),
            (accounts.asset, Decimal::ZERO, cost),
        ];
        if proceeds > Decimal::ZERO {
            let fund = funding_account(&tx, scope.company_id, "bank")?;
            lines.push((fund, proceeds, Decimal::ZERO));
        }
        if gain > Decimal::ZERO {
            let other = match gl::account_by_code(&tx, scope.company_id, "4900")? {
                Some(id) => id,
                None => gl::account_by_code(&tx, scope.company_id, "4100")?.ok_or_else(|| {
                    FinanceError::Invalid(
                        "لا يوجد حساب إيرادات أخرى (4900) ولا إيرادات مبيعات (4100) لأرباح الاستبعاد."
                            .into(),
                    )
                })?,
            };
            lines.push((other, Decimal::ZERO, gain));
        } else if gain < Decimal::ZERO {
            let loss =
                gl::account_by_code(&tx, scope.company_id, "5300")?.unwrap_or(accounts.expense);
            lines.push((loss, -gain, Decimal::ZERO));
        }
        let entry_id = gl::post_entry(
            &tx,
            ctx,
            &n.date,
            &format!("استبعاد أصل — {code} {name}"),
            "YER",
            &lines,
            Some(actor.id),
            "asset.dispose",
        )?;
        tx.execute(
            "UPDATE finance_assets SET status='disposed', disposed_on=?1, disposal_entry_id=?2 WHERE id=?3",
            params![n.date, entry_id, id],
        )?;
        tx.commit()?;
        Ok(entry_id)
    })();
    match result {
        Ok(entry) => Json(serde_json::json!({"entry_id": entry})).into_response(),
        Err(e) => error_response(e),
    }
}
