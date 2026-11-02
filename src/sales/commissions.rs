//! العمولات: خطط وقواعد واحتساب دوري، ثم ترحيل استحقاق عبر محرّك القيود نفسه.
use super::common;
use crate::AppState;
use crate::db_open;
use crate::finance::journals::{self, NewJournal};
use crate::finance::narrative;
use crate::finance::scope::{FinanceError, Scope, error_response};
use crate::domain::LineInput;
use axum::Json;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use rusqlite::{TransactionBehavior, params};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
pub struct ListQuery {
    pub company_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewPlan {
    pub company_id: i64,
    #[serde(default)]
    pub id: Option<i64>,
    pub name: String,
    pub percent_bp: i64,
    #[serde(default)]
    pub basis: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewRule {
    pub company_id: i64,
    #[serde(default)]
    pub id: Option<i64>,
    pub name: String,
    pub rate_bp: i64,
    #[serde(default)]
    pub basis: Option<String>,
    #[serde(default)]
    pub applies_to: Option<String>,
    #[serde(default)]
    pub min_amount: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeriodBody {
    pub company_id: i64,
    pub period: String,
}

pub async fn list(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ListQuery>,
) -> Response {
    let result = (|| -> Result<Value, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let cid = scope.company_id;
        let mut out = serde_json::Map::new();
        let mut stmt = db.prepare(
            "SELECT id,name,percent_bp,basis,active FROM sales_commission_plans
             WHERE company_id=?1 ORDER BY id",
        )?;
        out.insert("plans".into(), Value::Array(
            stmt.query_map([cid], |r| Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?, "name": r.get::<_, String>(1)?,
                "percent_bp": r.get::<_, i64>(2)?, "basis": r.get::<_, String>(3)?,
                "active": r.get::<_, i64>(4)? == 1,
            })))?.collect::<Result<Vec<_>, _>>()?));
        let mut stmt = db.prepare(
            "SELECT id,name,basis,applies_to,rate_bp,min_amount_minor,active
             FROM sales_commission_rules WHERE company_id=?1 ORDER BY id",
        )?;
        out.insert("rules".into(), Value::Array(
            stmt.query_map([cid], |r| Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?, "name": r.get::<_, String>(1)?,
                "basis": r.get::<_, String>(2)?, "applies_to": r.get::<_, String>(3)?,
                "rate_bp": r.get::<_, i64>(4)?,
                "min_amount": common::money(r.get::<_, i64>(5)?),
                "active": r.get::<_, i64>(6)? == 1,
            })))?.collect::<Result<Vec<_>, _>>()?));
        let mut stmt = db.prepare(
            "SELECT id,period,subject,basis,base_minor,rate_bp,amount_minor,COALESCE(note,'')
             FROM sales_commission_calcs WHERE company_id=?1 ORDER BY period DESC, id DESC LIMIT 200",
        )?;
        out.insert("calcs".into(), Value::Array(
            stmt.query_map([cid], |r| Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?, "period": r.get::<_, String>(1)?,
                "subject": r.get::<_, String>(2)?, "basis": r.get::<_, String>(3)?,
                "base": common::money(r.get::<_, i64>(4)?),
                "rate_bp": r.get::<_, i64>(5)?,
                "amount": common::money(r.get::<_, i64>(6)?),
                "note": r.get::<_, String>(7)?,
            })))?.collect::<Result<Vec<_>, _>>()?));
        Ok(Value::Object(out))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn save_plan(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(input): Json<NewPlan>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        if !(0..=10_000).contains(&input.percent_bp) {
            return Err(FinanceError::Invalid("نسبة الخطة خارج المدى.".into()));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, input.company_id)?;
        let basis = input.basis.clone().unwrap_or_else(|| "sale".into());
        let id = match input.id {
            Some(id) if id > 0 => {
                tx.execute(
                    "UPDATE sales_commission_plans SET name=?1,percent_bp=?2,basis=?3
                     WHERE id=?4 AND company_id=?5",
                    params![input.name.trim(), input.percent_bp, basis, id, scope.company_id],
                )?;
                id
            }
            _ => {
                tx.execute(
                    "INSERT INTO sales_commission_plans(company_id,name,percent_bp,basis,active)
                     VALUES(?1,?2,?3,?4,1)",
                    params![scope.company_id, input.name.trim(), input.percent_bp, basis],
                )?;
                tx.last_insert_rowid()
            }
        };
        tx.commit()?;
        Ok(id)
    })();
    match result {
        Ok(id) => Json(serde_json::json!({"id": id})).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn save_rule(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(input): Json<NewRule>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        if !(0..=10_000).contains(&input.rate_bp) {
            return Err(FinanceError::Invalid("نسبة القاعدة خارج المدى.".into()));
        }
        let basis = input.basis.clone().unwrap_or_else(|| "sale".into());
        let applies = input.applies_to.clone().unwrap_or_else(|| "rep".into());
        if !["sale", "collection"].contains(&basis.as_str()) {
            return Err(FinanceError::Invalid("أساس عمولة غير مدعوم.".into()));
        }
        if !["rep", "collector", "group"].contains(&applies.as_str()) {
            return Err(FinanceError::Invalid("جهة عمولة غير مدعومة.".into()));
        }
        let min = match input.min_amount.as_deref() {
            Some(raw) if !raw.trim().is_empty() => common::parse_money(raw, "الحد الأدنى")?,
            _ => 0,
        };
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, input.company_id)?;
        let id = match input.id {
            Some(id) if id > 0 => {
                tx.execute(
                    "UPDATE sales_commission_rules SET name=?1,basis=?2,applies_to=?3,rate_bp=?4,min_amount_minor=?5
                     WHERE id=?6 AND company_id=?7",
                    params![input.name.trim(), basis, applies, input.rate_bp, min, id, scope.company_id],
                )?;
                id
            }
            _ => {
                tx.execute(
                    "INSERT INTO sales_commission_rules(company_id,name,basis,applies_to,rate_bp,min_amount_minor,active)
                     VALUES(?1,?2,?3,?4,?5,?6,1)",
                    params![scope.company_id, input.name.trim(), basis, applies, input.rate_bp, min],
                )?;
                tx.last_insert_rowid()
            }
        };
        tx.commit()?;
        Ok(id)
    })();
    match result {
        Ok(id) => Json(serde_json::json!({"id": id})).into_response(),
        Err(e) => error_response(e),
    }
}

fn valid_period(period: &str) -> bool {
    let bytes = period.as_bytes();
    bytes.len() == 7 && bytes[4] == b'-' && period[..4].chars().all(|c| c.is_ascii_digit())
        && period[5..].chars().all(|c| c.is_ascii_digit())
}

/// احتساب العمولات لدورة شهرية: الأساس = صافي مبيعات المندوب المرحّلة في الشهر.
pub async fn calculate(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(input): Json<PeriodBody>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        if !valid_period(&input.period) {
            return Err(FinanceError::Invalid("صيغة الدورة يجب أن تكون YYYY-MM.".into()));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, input.company_id)?;
        let cid = scope.company_id;
        tx.execute(
            "DELETE FROM sales_commission_calcs WHERE company_id=?1 AND period=?2",
            params![cid, input.period],
        )?;
        let reps: Vec<(i64, String, i64)> = {
            let mut stmt = tx.prepare(
                "SELECT id,name,commission_bp FROM sales_reps
                 WHERE company_id=?1 AND status='active' AND commission_bp>0 ORDER BY id",
            )?;
            stmt.query_map([cid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut created = 0_i64;
        for (rep_id, name, rate) in reps {
            let base: i64 = tx.query_row(
                "SELECT COALESCE(SUM(CASE WHEN invoice_type='sale' THEN total_minor ELSE -total_minor END),0)
                 FROM sales_invoices
                 WHERE company_id=?1 AND rep_id=?2 AND status='posted'
                   AND substr(invoice_date,1,7)=?3",
                params![cid, rep_id, input.period],
                |r| r.get(0),
            )?;
            if base <= 0 {
                continue;
            }
            let amount = base * rate / 10_000;
            tx.execute(
                "INSERT INTO sales_commission_calcs(company_id,period,subject,subject_id,basis,base_minor,rate_bp,amount_minor,note,created_by)
                 VALUES(?1,?2,?3,?4,'sale',?5,?6,?7,?8,?9)",
                params![cid, input.period, name, rep_id, base, rate, amount, "احتساب آلي", actor.id],
            )?;
            created += 1;
        }
        tx.commit()?;
        Ok(created)
    })();
    match result {
        Ok(n) => Json(serde_json::json!({"calculated": n})).into_response(),
        Err(e) => error_response(e),
    }
}

/// ترحيل استحقاق العمولة: مصروف عمولات / عمولات مستحقة — عبر محرّك القيود.
pub async fn accrue(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(input): Json<PeriodBody>,
) -> Response {
    let result = (|| -> Result<Option<i64>, FinanceError> {
        if !valid_period(&input.period) {
            return Err(FinanceError::Invalid("صيغة الدورة يجب أن تكون YYYY-MM.".into()));
        }
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, input.company_id)?;
        let cid = scope.company_id;
        let total: i64 = db.query_row(
            "SELECT COALESCE(SUM(amount_minor),0) FROM sales_commission_calcs
             WHERE company_id=?1 AND period=?2",
            params![cid, input.period],
            |r| r.get(0),
        )?;
        if total <= 0 {
            return Err(FinanceError::Invalid("لا توجد عمولات محتسبة لهذه الدورة.".into()));
        }
        // إن لم يوجد حساب عمولات في الدليل نُنشئه تلقائيًا (مصروف 5000، والتزام 2000).
        let mut db = db;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let expense = match common::commission_expense_account(&tx, cid) {
            Some(id) => id,
            None => super::customers::provision_account(
                &tx, cid, "5000", "عمولات مبيعات", "expense", "debit")?,
        };
        let payable = match common::commission_payable_account(&tx, cid) {
            Some(id) => id,
            None => super::customers::provision_account(
                &tx, cid, "2000", "عمولات مستحقة", "liability", "credit")?,
        };
        tx.commit()?;
        let db = db_open(&state.db)?;
        let currency: String = db.query_row(
            "SELECT currency FROM accounts WHERE id=?1",
            [expense],
            |r| r.get(0),
        )?;
        let date = format!("{}-28", input.period);
        let description = narrative::compose("استحقاق عمولات", &input.period, "");
        let lines = vec![
            LineInput {
                account: common::account_label(&db, cid, expense),
                account_id: Some(expense),
                debit: common::money(total),
                credit: "0".into(),
            },
            LineInput {
                account: common::account_label(&db, cid, payable),
                account_id: Some(payable),
                debit: "0".into(),
                credit: common::money(total),
            },
        ];
        drop(db);
        let draft = journals::create_draft(
            &state,
            &actor,
            &NewJournal {
                company_id: cid,
                description,
                date,
                currency,
                lines,
            },
        )?;
        let entry = journals::post_draft(&state, &actor, cid, draft, &[], None)?;
        Ok(Some(entry))
    })();
    match result {
        Ok(entry) => Json(serde_json::json!({"journal_entry_id": entry})).into_response(),
        Err(e) => error_response(e),
    }
}
