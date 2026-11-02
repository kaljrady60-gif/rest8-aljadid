//! إضافات المالية (٢): الإشعارات المدينة والدائنة · دورات الزكاة · الموازنات وتعديلاتها.
//!
//! كل ترحيل مالي هنا يمرّ عبر `gl::post_entry`: مصدر واحد لإنشاء القيد المرحّل،
//! فلا تتكرر قواعد التوازن والقفل وسجل التدقيق في ثلاثة أماكن.
use super::audit;
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::str::FromStr;

use super::extras::{amount_of, money};

#[derive(Deserialize)]
pub struct CompanyQuery {
    company_id: i64,
}

// ───────────────────── الإشعارات المدينة والدائنة ─────────────────────
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewNote {
    company_id: i64,
    kind: String,
    doc_ref: String,
    #[serde(default)]
    party_id: Option<i64>,
    note_date: String,
    amount: String,
    #[serde(default = "yer")]
    currency: String,
    #[serde(default)]
    reason: Option<String>,
}

fn yer() -> String {
    "YER".into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostNote {
    company_id: i64,
    id: i64,
    /// الحساب المقابل: إيرادات/مصروفات/مرتجع حسب نوع الإشعار.
    counter_account_id: i64,
}

pub async fn notes(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<CompanyQuery>,
) -> Response {
    let result = (|| -> Result<Vec<serde_json::Value>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT n.id,n.kind,n.doc_ref,n.note_date,n.amount,n.currency,n.reason,n.status,n.entry_id,
                    p.name
             FROM finance_notes n LEFT JOIN finance_parties p ON p.id=n.party_id
             WHERE n.company_id=?1 ORDER BY n.note_date DESC, n.id DESC",
        )?;
        let rows = stmt
            .query_map([scope.company_id], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "kind": r.get::<_, String>(1)?,
                    "doc_ref": r.get::<_, String>(2)?,
                    "date": r.get::<_, String>(3)?,
                    "amount": r.get::<_, String>(4)?,
                    "currency": r.get::<_, String>(5)?,
                    "reason": r.get::<_, Option<String>>(6)?,
                    "status": r.get::<_, String>(7)?,
                    "entry_id": r.get::<_, Option<i64>>(8)?,
                    "party": r.get::<_, Option<String>>(9)?
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })();
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn create_note(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<NewNote>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        if !["debit", "credit"].contains(&n.kind.as_str()) {
            return Err(FinanceError::Invalid(
                "نوع الإشعار: debit (مدين) أو credit (دائن).".into(),
            ));
        }
        if n.doc_ref.trim().is_empty() {
            return Err(FinanceError::Invalid("مرجع المستند مطلوب.".into()));
        }
        let amount = amount_of(&n.amount, "المبلغ")?;
        if amount.is_zero() {
            return Err(FinanceError::Invalid("المبلغ يجب أن يكون أكبر من صفر.".into()));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        tx.execute(
            "INSERT INTO finance_notes(company_id,kind,doc_ref,party_id,note_date,amount,currency,reason)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                scope.company_id,
                n.kind,
                n.doc_ref.trim(),
                n.party_id,
                n.note_date,
                money(amount),
                n.currency,
                n.reason
            ],
        )?;
        let id = tx.last_insert_rowid();
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "note.create",
                entity: "finance_notes",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({
                    "kind": n.kind, "ref": n.doc_ref.trim(), "amount": money(amount)
                })),
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

/// ترحيل الإشعار: ينشئ قيدًا متوازنًا فعليًا ويحوّل الحالة إلى posted.
pub async fn post_note(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(req): Json<PostNote>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, req.company_id)?;
        scope.require_admin()?;
        let row: Option<(String, String, Option<i64>, String, String, String)> = tx
            .query_row(
                "SELECT kind,note_date,party_id,amount,currency,status FROM finance_notes
                 WHERE id=?1 AND company_id=?2",
                params![req.id, scope.company_id],
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
        let (kind, date, party_id, amount, currency, status) =
            row.ok_or(FinanceError::NotFound("الإشعار"))?;
        if status == "posted" {
            return Err(FinanceError::Conflict("الإشعار مرحّل مسبقًا."));
        }
        let party_account: Option<i64> = match party_id {
            Some(pid) => tx
                .query_row(
                    "SELECT account_id FROM finance_parties WHERE id=?1 AND company_id=?2",
                    params![pid, scope.company_id],
                    |r| r.get(0),
                )
                .optional()?,
            None => None,
        };
        let party_account = party_account.ok_or_else(|| {
            FinanceError::Invalid("اربط الإشعار بطرف له حساب تفصيلي قبل الترحيل.".into())
        })?;
        let value = Decimal::from_str(amount.trim())
            .map_err(|_| FinanceError::Invalid("مبلغ الإشعار غير صالح.".into()))?;
        let ctx = super::gl::context(&tx, scope.company_id, &date)?;
        // إشعار مدين = زيادة على الطرف (مدين حساب الطرف) · دائن = تخفيض (دائن حساب الطرف).
        let lines = if kind == "debit" {
            vec![
                (party_account, value, Decimal::ZERO),
                (req.counter_account_id, Decimal::ZERO, value),
            ]
        } else {
            vec![
                (req.counter_account_id, value, Decimal::ZERO),
                (party_account, Decimal::ZERO, value),
            ]
        };
        let entry_id = super::gl::post_entry(
            &tx,
            ctx,
            &date,
            &{
                let party_name: String = match party_id {
                    Some(pid) => tx
                        .query_row(
                            "SELECT name FROM finance_parties WHERE id=?1",
                            [pid],
                            |r| r.get(0),
                        )
                        .unwrap_or_else(|_| String::new()),
                    None => String::new(),
                };
                super::narrative::compose(
                    super::narrative::document_label(if kind == "debit" {
                        "debit_note"
                    } else {
                        "credit_note"
                    }),
                    &format!("DN-{}/{}", req.id, &date),
                    &party_name,
                )
            },
            &currency,
            &lines,
            Some(actor.id),
            "note.post",
        )?;
        tx.execute(
            "UPDATE finance_notes SET status='posted', entry_id=?2 WHERE id=?1",
            params![req.id, entry_id],
        )?;
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "note.post",
                entity: "finance_notes",
                entity_id: Some(req.id.to_string()),
                payload: Some(serde_json::json!({
                    "entry_id": entry_id, "amount": amount, "kind": kind
                })),
            },
        )?;
        tx.commit()?;
        Ok(serde_json::json!({"id": req.id, "status": "posted", "entry_id": entry_id}))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

// ─────────────────────────── دورات الزكاة ───────────────────────────
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZakatRequest {
    company_id: i64,
    fiscal_year: i64,
    #[serde(default = "default_rate")]
    rate_bp: i64,
    #[serde(default)]
    expense_account_id: Option<i64>,
    #[serde(default)]
    payable_account_id: Option<i64>,
}

fn default_rate() -> i64 {
    250
}

/// وعاء الزكاة التقديري: مجموع الأصول الجارية ناقص الالتزامات الجارية من القيود المرحّلة.
/// تقدير صريح لا فتوى: النسبة والوعاء قابلان للضبط، والقيد الناتج مسودة قابلة للمراجعة.
fn zakat_base(
    db: &rusqlite::Connection,
    company_id: i64,
    year: i64,
) -> Result<Decimal, FinanceError> {
    let mut stmt = db.prepare(
        "SELECT a.category, COALESCE(SUM(CAST(jl.debit AS REAL)),0) - COALESCE(SUM(CAST(jl.credit AS REAL)),0)
         FROM journal_lines jl
         JOIN journal_entries je ON je.id=jl.entry_id
         JOIN accounts a ON a.id=jl.account_id
         WHERE je.company_id=?1 AND je.finalized=1
           AND substr(je.entry_date,1,4)=?2
         GROUP BY a.category",
    )?;
    let rows = stmt.query_map(params![company_id, year.to_string()], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
    })?;
    let mut base = Decimal::ZERO;
    for row in rows {
        let (kind, net) = row?;
        let value = Decimal::from_str(&format!("{net:.4}")).unwrap_or(Decimal::ZERO);
        match kind.as_str() {
            // الأصول تزيد الوعاء · الالتزامات تنقصه (الأصل مدين موجب، الالتزام دائن سالب فنجمعه).
            "asset" => base += value,
            "liability" => base += value,
            _ => {}
        }
    }
    if base.is_sign_negative() {
        base = Decimal::ZERO;
    }
    Ok(base)
}

pub async fn zakat_cycles(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<CompanyQuery>,
) -> Response {
    let result = (|| -> Result<Vec<serde_json::Value>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT id,fiscal_year,base,rate_bp,due,status,entry_id,created_at
             FROM finance_zakat_cycles WHERE company_id=?1 ORDER BY fiscal_year DESC",
        )?;
        let rows = stmt
            .query_map([scope.company_id], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "fiscal_year": r.get::<_, i64>(1)?,
                    "base": r.get::<_, String>(2)?,
                    "rate_bp": r.get::<_, i64>(3)?,
                    "due": r.get::<_, String>(4)?,
                    "status": r.get::<_, String>(5)?,
                    "entry_id": r.get::<_, Option<i64>>(6)?,
                    "created_at": r.get::<_, String>(7)?
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })();
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn compute_zakat(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ZakatQuery>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let base = zakat_base(&db, scope.company_id, q.fiscal_year)?;
        let rate = Decimal::from(q.rate_bp.clamp(0, 10_000)) / Decimal::from(10_000);
        Ok(serde_json::json!({
            "fiscal_year": q.fiscal_year,
            "base": money(base),
            "rate_bp": q.rate_bp,
            "due": money(base * rate)
        }))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

#[derive(Deserialize)]
pub struct ZakatQuery {
    company_id: i64,
    fiscal_year: i64,
    #[serde(default = "default_rate")]
    rate_bp: i64,
}

/// تحتسب الدورة وتخزّنها؛ وإن مُرِّر حسابان تُرحَّل فورًا بقيد متوازن.
pub async fn run_zakat(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(req): Json<ZakatRequest>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, req.company_id)?;
        scope.require_admin()?;
        let base = zakat_base(&tx, scope.company_id, req.fiscal_year)?;
        let rate = Decimal::from(req.rate_bp.clamp(0, 10_000)) / Decimal::from(10_000);
        let due = (base * rate).round_dp(4);
        tx.execute(
            "INSERT INTO finance_zakat_cycles(company_id,fiscal_year,base,rate_bp,due)
             VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(company_id,fiscal_year) DO UPDATE SET
               base=excluded.base, rate_bp=excluded.rate_bp, due=excluded.due
             WHERE finance_zakat_cycles.status='computed'",
            params![
                scope.company_id,
                req.fiscal_year,
                money(base),
                req.rate_bp,
                money(due)
            ],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(err, _)
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                FinanceError::Conflict("دورة هذه السنة مرحّلة ولا تُعاد.")
            }
            other => FinanceError::Storage(other),
        })?;
        let id: i64 = tx.query_row(
            "SELECT id FROM finance_zakat_cycles WHERE company_id=?1 AND fiscal_year=?2",
            params![scope.company_id, req.fiscal_year],
            |r| r.get(0),
        )?;
        let mut entry_id: Option<i64> = None;
        if let (Some(expense), Some(payable)) = (req.expense_account_id, req.payable_account_id) {
            if due > Decimal::ZERO {
                let date = format!("{}-12-31", req.fiscal_year);
                let ctx = super::gl::context(&tx, scope.company_id, &date)?;
                let lines = vec![
                    (expense, due, Decimal::ZERO),
                    (payable, Decimal::ZERO, due),
                ];
                let posted = super::gl::post_entry(
                    &tx,
                    ctx,
                    &date,
                    &super::narrative::compose(
                        super::narrative::document_label("zakat"),
                        &format!("ZK-{}", req.fiscal_year),
                        "الشركة",
                    ),
                    "YER",
                    &lines,
                    Some(actor.id),
                    "zakat.post",
                )?;
                tx.execute(
                    "UPDATE finance_zakat_cycles SET status='posted', entry_id=?2 WHERE id=?1",
                    params![id, posted],
                )?;
                entry_id = Some(posted);
            }
        }
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "zakat.run",
                entity: "finance_zakat_cycles",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({
                    "year": req.fiscal_year, "base": money(base), "due": money(due),
                    "entry_id": entry_id
                })),
            },
        )?;
        tx.commit()?;
        Ok(serde_json::json!({
            "id": id, "fiscal_year": req.fiscal_year,
            "base": money(base), "rate_bp": req.rate_bp, "due": money(due),
            "entry_id": entry_id
        }))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

// ──────────────────────────── الموازنات ────────────────────────────
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewBudget {
    company_id: i64,
    fiscal_year: i64,
    name: String,
    #[serde(default = "company_scope")]
    scope: String,
    #[serde(default)]
    department_id: Option<i64>,
    #[serde(default)]
    note: Option<String>,
    /// بنود الموازنة: حساب + شهر + مبلغ.
    #[serde(default)]
    lines: Vec<BudgetLine>,
}

fn company_scope() -> String {
    "company".into()
}

#[derive(Deserialize)]
pub struct BudgetLine {
    account_id: i64,
    month: i64,
    amount: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdjustBudget {
    company_id: i64,
    budget_id: i64,
    account_id: i64,
    month: i64,
    amount: String,
    #[serde(default)]
    reason: Option<String>,
}

pub async fn budgets(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<CompanyQuery>,
) -> Response {
    let result = (|| -> Result<Vec<serde_json::Value>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT b.id,b.fiscal_year,b.name,b.scope,b.status,b.note,d.name,
                    (SELECT COUNT(*) FROM finance_budget_lines l WHERE l.budget_id=b.id),
                    (SELECT COALESCE(SUM(CAST(l.amount AS REAL)),0) FROM finance_budget_lines l WHERE l.budget_id=b.id)
             FROM finance_budgets b LEFT JOIN finance_departments d ON d.id=b.department_id
             WHERE b.company_id=?1 ORDER BY b.fiscal_year DESC, b.id DESC",
        )?;
        let rows = stmt
            .query_map([scope.company_id], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "fiscal_year": r.get::<_, i64>(1)?,
                    "name": r.get::<_, String>(2)?,
                    "scope": r.get::<_, String>(3)?,
                    "status": r.get::<_, String>(4)?,
                    "note": r.get::<_, Option<String>>(5)?,
                    "department": r.get::<_, Option<String>>(6)?,
                    "lines": r.get::<_, i64>(7)?,
                    "total": r.get::<_, f64>(8)?
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })();
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn create_budget(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<NewBudget>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        if n.name.trim().is_empty() {
            return Err(FinanceError::Invalid("اسم الموازنة مطلوب.".into()));
        }
        if !["company", "department"].contains(&n.scope.as_str()) {
            return Err(FinanceError::Invalid("نطاق الموازنة غير مدعوم.".into()));
        }
        if n.scope == "department" && n.department_id.is_none() {
            return Err(FinanceError::Invalid(
                "موازنة الإدارة تحتاج قسمًا محددًا.".into(),
            ));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        scope.require_admin()?;
        tx.execute(
            "INSERT INTO finance_budgets(company_id,fiscal_year,name,scope,department_id,note)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                scope.company_id,
                n.fiscal_year,
                n.name.trim(),
                n.scope,
                n.department_id,
                n.note
            ],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(err, _)
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                FinanceError::Conflict("اسم الموازنة مستخدم لهذه السنة.")
            }
            other => FinanceError::Storage(other),
        })?;
        let id = tx.last_insert_rowid();
        for line in &n.lines {
            if !(1..=12).contains(&line.month) {
                return Err(FinanceError::Invalid("الشهر بين 1 و12.".into()));
            }
            let value = amount_of(&line.amount, "مبلغ البند")?;
            tx.execute(
                "INSERT INTO finance_budget_lines(budget_id,account_id,month,amount)
                 VALUES(?1,?2,?3,?4)
                 ON CONFLICT(budget_id,account_id,month) DO UPDATE SET amount=excluded.amount",
                params![id, line.account_id, line.month, money(value)],
            )?;
        }
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "budget.create",
                entity: "finance_budgets",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({
                    "year": n.fiscal_year, "lines": n.lines.len(), "scope": n.scope
                })),
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

#[derive(Deserialize)]
pub struct CompareQuery {
    company_id: i64,
    budget_id: i64,
}

/// مقارنة الموازنة بالفعل: لكل حساب مجموع الموازنة ومجموع الحركة المرحّلة، والفرق.
pub async fn compare_budget(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<CompareQuery>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let year: i64 = db
            .query_row(
                "SELECT fiscal_year FROM finance_budgets WHERE id=?1 AND company_id=?2",
                params![q.budget_id, scope.company_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(FinanceError::NotFound("الموازنة"))?;
        let mut stmt = db.prepare(
            "SELECT a.code, a.name, l.month, SUM(CAST(l.amount AS REAL))
             FROM finance_budget_lines l JOIN accounts a ON a.id=l.account_id
             WHERE l.budget_id=?1 GROUP BY a.id, l.month ORDER BY a.code, l.month",
        )?;
        let planned = stmt
            .query_map([q.budget_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, f64>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        // الفعلي يُحسب باتجاه طبيعة الحساب: المدين (دائن−مدين) والدائن (مدين−دائن) معكوسًا،
        // فلا يظهر بند إيراد سالبًا لمجرد أن الأستاذ يزيد بالدائن.
        let mut actual = db.prepare(
            "SELECT jl.account_id, a.nature,
                    COALESCE(SUM(CAST(jl.debit AS REAL)),0) - COALESCE(SUM(CAST(jl.credit AS REAL)),0)
             FROM journal_lines jl
             JOIN journal_entries je ON je.id=jl.entry_id
             JOIN accounts a ON a.id=jl.account_id
             WHERE je.company_id=?1 AND je.finalized=1 AND substr(je.entry_date,1,4)=?2
             GROUP BY jl.account_id",
        )?;
        let mut by_account = std::collections::HashMap::new();
        let rows = actual.query_map(params![scope.company_id, year.to_string()], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, f64>(2)?,
            ))
        })?;
        for row in rows {
            let (account, nature, net) = row?;
            let oriented = if nature == "credit" { -net } else { net };
            by_account.insert(account, oriented);
        }
        let mut codes = db.prepare("SELECT id, code, name FROM accounts")?;
        let mut lookup = std::collections::HashMap::new();
        let codes_rows = codes.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in codes_rows {
            let (id, code, name) = row?;
            lookup.insert(code.clone(), (id, name));
        }
        let mut lines = Vec::new();
        let mut planned_total = 0.0_f64;
        let mut actual_total = 0.0_f64;
        for (code, name, month, amount) in planned {
            let account_id = lookup.get(&code).map(|(id, _)| *id);
            let real = account_id
                .and_then(|id| by_account.get(&id).copied())
                .unwrap_or(0.0)
                / 12.0;
            planned_total += amount;
            actual_total += real;
            lines.push(serde_json::json!({
                "code": code, "name": name, "month": month,
                "planned": amount, "actual": real,
                "variance": real - amount
            }));
        }
        Ok(serde_json::json!({
            "budget_id": q.budget_id,
            "fiscal_year": year,
            "lines": lines,
            "planned_total": planned_total,
            "actual_total": actual_total,
            "variance_total": actual_total - planned_total
        }))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

/// تعديل بند موازنة: يحدّث المبلغ ويحفظ القيمة القديمة والسبب في جدول التعديلات.
pub async fn adjust_budget(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(req): Json<AdjustBudget>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let value = amount_of(&req.amount, "المبلغ")?;
        if !(1..=12).contains(&req.month) {
            return Err(FinanceError::Invalid("الشهر بين 1 و12.".into()));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, req.company_id)?;
        scope.require_admin()?;
        let status: String = tx
            .query_row(
                "SELECT status FROM finance_budgets WHERE id=?1 AND company_id=?2",
                params![req.budget_id, scope.company_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(FinanceError::NotFound("الموازنة"))?;
        if status == "closed" {
            return Err(FinanceError::Conflict("الموازنة مغلقة ولا تُعدّل."));
        }
        let old: Option<String> = tx
            .query_row(
                "SELECT amount FROM finance_budget_lines WHERE budget_id=?1 AND account_id=?2 AND month=?3",
                params![req.budget_id, req.account_id, req.month],
                |r| r.get(0),
            )
            .optional()?;
        let old_value = old.clone().unwrap_or_else(|| "0.0000".into());
        tx.execute(
            "INSERT INTO finance_budget_lines(budget_id,account_id,month,amount) VALUES(?1,?2,?3,?4)
             ON CONFLICT(budget_id,account_id,month) DO UPDATE SET amount=excluded.amount",
            params![req.budget_id, req.account_id, req.month, money(value)],
        )?;
        tx.execute(
            "INSERT INTO finance_budget_adjustments(budget_id,account_id,month,old_amount,new_amount,reason,actor_id)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                req.budget_id,
                req.account_id,
                req.month,
                old_value,
                money(value),
                req.reason,
                actor.id
            ],
        )?;
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "budget.adjust",
                entity: "finance_budget_lines",
                entity_id: Some(req.budget_id.to_string()),
                payload: Some(serde_json::json!({
                    "account_id": req.account_id, "month": req.month,
                    "old": old_value, "new": money(value)
                })),
            },
        )?;
        tx.commit()?;
        Ok(serde_json::json!({
            "budget_id": req.budget_id, "account_id": req.account_id,
            "month": req.month, "old": old_value, "new": money(value)
        }))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

/// سجل تعديلات موازنة (أثر تدقيقي مرئي للمستخدم لا للمطوّر فقط).
pub async fn budget_history(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<CompareQuery>,
) -> Response {
    let result = (|| -> Result<Vec<serde_json::Value>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT x.id, a.code, a.name, x.month, x.old_amount, x.new_amount, x.reason, x.created_at
             FROM finance_budget_adjustments x
             JOIN finance_budgets b ON b.id=x.budget_id
             JOIN accounts a ON a.id=x.account_id
             WHERE b.id=?1 AND b.company_id=?2 ORDER BY x.id DESC",
        )?;
        let rows = stmt
            .query_map(params![q.budget_id, scope.company_id], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "code": r.get::<_, String>(1)?,
                    "name": r.get::<_, String>(2)?,
                    "month": r.get::<_, i64>(3)?,
                    "old": r.get::<_, String>(4)?,
                    "new": r.get::<_, String>(5)?,
                    "reason": r.get::<_, Option<String>>(6)?,
                    "at": r.get::<_, String>(7)?
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })();
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => error_response(e),
    }
}
