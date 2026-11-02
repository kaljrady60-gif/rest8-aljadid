//! المخططات والعمولات · بيانات الدول · الحسابات الوسيطة · متغيرات الأستاذ.
//! النسب تُخزَّن أساس-نقطة (1500 = 15%) فلا تمثيل عائم في أي قيمة محفوظة.
use super::audit;
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use rust_decimal::Decimal;
use rust_decimal::RoundingStrategy;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

pub const PLAN_KINDS: &[&str] = &["percent", "fixed", "tiered"];
pub const INTERMEDIATE_KINDS: &[&str] = &["clearing", "suspense", "pending"];
pub const SETTING_KEYS: &[&str] = &[
    "posting.require_period",
    "posting.require_approval",
    "posting.allow_backdated",
    "reports.round_scale",
    "reports.base_currency",
];

/// القيمة الفعّالة لكل مفتاح عندما لا يضبطه المستخدم — مصدر واحد للحقيقة.
pub const SETTING_DEFAULTS: &[(&str, &str)] = &[
    ("posting.require_period", "1"),
    ("posting.require_approval", "0"),
    ("posting.allow_backdated", "0"),
    ("reports.round_scale", "4"),
    ("reports.base_currency", "YER"),
];

#[derive(Serialize)]
pub struct Plan {
    id: i64,
    code: String,
    name: String,
    kind: String,
    rate_bp: i64,
    fixed_amount: String,
    account_id: Option<i64>,
    archived_at: Option<String>,
}

#[derive(Serialize)]
pub struct Country {
    id: i64,
    iso_code: String,
    name: String,
    currency: String,
    tax_rate_bp: i64,
    is_default: bool,
}

#[derive(Serialize)]
pub struct Intermediate {
    id: i64,
    code: String,
    name: String,
    kind: String,
    account_id: i64,
    archived_at: Option<String>,
}

#[derive(Deserialize)]
pub struct CompanyQuery {
    company_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewPlan {
    company_id: i64,
    code: String,
    name: String,
    kind: String,
    rate_bp: i64,
    #[serde(default)]
    fixed_amount: Option<String>,
    #[serde(default)]
    account_id: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewCountry {
    company_id: i64,
    iso_code: String,
    name: String,
    currency: String,
    #[serde(default)]
    tax_rate_bp: i64,
    #[serde(default)]
    is_default: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewIntermediate {
    company_id: i64,
    code: String,
    name: String,
    kind: String,
    account_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveReq {
    company_id: i64,
    id: i64,
}

#[derive(Deserialize)]
pub struct QuoteQuery {
    company_id: i64,
    plan_id: i64,
    amount: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetSetting {
    company_id: i64,
    key: String,
    value: String,
}

/// عمولة مخطط: نسبة على المبلغ + مبلغ ثابت، بتقريب HALF_UP لمنازل المال.
pub fn compute_commission(amount: Decimal, rate_bp: i64, fixed: Decimal) -> Decimal {
    let base = (amount * Decimal::from(rate_bp) / Decimal::from(10_000))
        .round_dp_with_strategy(4, RoundingStrategy::MidpointAwayFromZero);
    let mut out = base + fixed;
    out.rescale(4);
    out
}

pub async fn plans(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<CompanyQuery>,
) -> Response {
    let result = (|| -> Result<Vec<Plan>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT id,code,name,kind,rate_bp,fixed_amount,account_id,archived_at
             FROM finance_plans WHERE company_id=?1 ORDER BY archived_at IS NOT NULL, code",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            Ok(Plan {
                id: r.get(0)?,
                code: r.get(1)?,
                name: r.get(2)?,
                kind: r.get(3)?,
                rate_bp: r.get(4)?,
                fixed_amount: r.get(5)?,
                account_id: r.get(6)?,
                archived_at: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })();
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn create_plan(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<NewPlan>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        let code = n.code.trim();
        let name = n.name.trim();
        if code.is_empty() || code.chars().count() > 20 {
            return Err(FinanceError::Invalid(
                "رمز المخطط مطلوب حتى 20 حرفًا.".into(),
            ));
        }
        if name.is_empty() || name.chars().count() > 120 {
            return Err(FinanceError::Invalid(
                "اسم المخطط مطلوب حتى 120 حرفًا.".into(),
            ));
        }
        if !PLAN_KINDS.contains(&n.kind.as_str()) {
            return Err(FinanceError::Invalid("نوع المخطط غير مدعوم.".into()));
        }
        if !(0..=10_000).contains(&n.rate_bp) {
            return Err(FinanceError::Invalid("النسبة بين 0% و100%.".into()));
        }
        let fixed = match n.fixed_amount.as_deref() {
            None => Decimal::ZERO,
            Some(text) => Decimal::from_str(text.trim())
                .map_err(|_| FinanceError::Invalid("المبلغ الثابت ليس رقمًا صالحًا.".into()))?,
        };
        if fixed.is_sign_negative() {
            return Err(FinanceError::Invalid("المبلغ الثابت لا يكون سالبًا.".into()));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        scope.require_admin()?;
        tx.execute(
            "INSERT INTO finance_plans(company_id,code,name,kind,rate_bp,fixed_amount,account_id)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                scope.company_id,
                code,
                name,
                n.kind,
                n.rate_bp,
                super::money::render(fixed),
                n.account_id
            ],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(_, _) => FinanceError::Conflict("رمز المخطط مستخدم."),
            other => FinanceError::Storage(other),
        })?;
        let id = tx.last_insert_rowid();
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "plan.create",
                entity: "finance_plans",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({"code": code, "rate_bp": n.rate_bp})),
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

pub async fn quote(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<QuoteQuery>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let amount = Decimal::from_str(q.amount.trim())
            .map_err(|_| FinanceError::Invalid("المبلغ ليس رقمًا صالحًا.".into()))?;
        if amount.is_sign_negative() {
            return Err(FinanceError::Invalid("المبلغ لا يكون سالبًا.".into()));
        }
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let (rate_bp, fixed_text, name) = db
            .query_row(
                "SELECT rate_bp,fixed_amount,name FROM finance_plans
                 WHERE id=?1 AND company_id=?2 AND archived_at IS NULL",
                params![q.plan_id, scope.company_id],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or(FinanceError::NotFound("المخطط"))?;
        let fixed = Decimal::from_str(&fixed_text)
            .map_err(|_| FinanceError::Invalid("المبلغ الثابت المخزَّن تالف.".into()))?;
        let commission = compute_commission(amount, rate_bp, fixed);
        Ok(serde_json::json!({
            "plan": name,
            "amount": super::money::render(amount),
            "rate_bp": rate_bp,
            "commission": super::money::render(commission),
            "net": super::money::render(amount - commission)
        }))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn countries(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<CompanyQuery>,
) -> Response {
    let result = (|| -> Result<Vec<Country>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT id,iso_code,name,currency,tax_rate_bp,is_default
             FROM finance_countries WHERE company_id=?1 ORDER BY is_default DESC, iso_code",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            Ok(Country {
                id: r.get(0)?,
                iso_code: r.get(1)?,
                name: r.get(2)?,
                currency: r.get(3)?,
                tax_rate_bp: r.get(4)?,
                is_default: r.get::<_, i64>(5)? != 0,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })();
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn create_country(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<NewCountry>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        let iso = n.iso_code.trim().to_uppercase();
        if iso.len() != 2 || !iso.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(FinanceError::Invalid(
                "رمز الدولة حرفان لاتينيان (ISO 3166-1).".into(),
            ));
        }
        let name = n.name.trim();
        if name.is_empty() || name.chars().count() > 80 {
            return Err(FinanceError::Invalid(
                "اسم الدولة مطلوب حتى 80 حرفًا.".into(),
            ));
        }
        if n.currency.trim().is_empty() || n.currency.trim().len() > 8 {
            return Err(FinanceError::Invalid("رمز العملة مطلوب حتى 8 أحرف.".into()));
        }
        if !(0..=10_000).contains(&n.tax_rate_bp) {
            return Err(FinanceError::Invalid("نسبة الضريبة بين 0% و100%.".into()));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        scope.require_admin()?;
        if n.is_default {
            tx.execute(
                "UPDATE finance_countries SET is_default=0 WHERE company_id=?1",
                [scope.company_id],
            )?;
        }
        tx.execute(
            "INSERT INTO finance_countries(company_id,iso_code,name,currency,tax_rate_bp,is_default)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                scope.company_id,
                iso,
                name,
                n.currency.trim(),
                n.tax_rate_bp,
                i64::from(n.is_default)
            ],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(_, _) => FinanceError::Conflict("رمز الدولة مسجّل مسبقًا."),
            other => FinanceError::Storage(other),
        })?;
        let id = tx.last_insert_rowid();
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "country.create",
                entity: "finance_countries",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({"iso": iso, "tax_rate_bp": n.tax_rate_bp})),
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

pub async fn intermediate(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<CompanyQuery>,
) -> Response {
    let result = (|| -> Result<Vec<Intermediate>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT id,code,name,kind,account_id,archived_at FROM finance_intermediate
             WHERE company_id=?1 ORDER BY archived_at IS NOT NULL, code",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            Ok(Intermediate {
                id: r.get(0)?,
                code: r.get(1)?,
                name: r.get(2)?,
                kind: r.get(3)?,
                account_id: r.get(4)?,
                archived_at: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })();
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn create_intermediate(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<NewIntermediate>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        let code = n.code.trim();
        if code.is_empty() || code.chars().count() > 20 {
            return Err(FinanceError::Invalid(
                "رمز الحساب الوسيط مطلوب حتى 20 حرفًا.".into(),
            ));
        }
        if !INTERMEDIATE_KINDS.contains(&n.kind.as_str()) {
            return Err(FinanceError::Invalid("نوع الحساب الوسيط غير مدعوم.".into()));
        }
        if n.name.trim().is_empty() || n.name.chars().count() > 120 {
            return Err(FinanceError::Invalid(
                "اسم الحساب الوسيط مطلوب حتى 120 حرفًا.".into(),
            ));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        scope.require_admin()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1)",
            [n.account_id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(FinanceError::Invalid(
                "الحساب المرتبط غير موجود في الدليل.".into(),
            ));
        }
        tx.execute(
            "INSERT INTO finance_intermediate(company_id,code,name,kind,account_id)
             VALUES(?1,?2,?3,?4,?5)",
            params![scope.company_id, code, n.name.trim(), n.kind, n.account_id],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(_, _) => {
                FinanceError::Conflict("رمز الحساب الوسيط مستخدم.")
            }
            other => FinanceError::Storage(other),
        })?;
        let id = tx.last_insert_rowid();
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "intermediate.create",
                entity: "finance_intermediate",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({"code": code, "kind": n.kind})),
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

pub async fn archive(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    axum::extract::Path(table): axum::extract::Path<String>,
    Json(req): Json<ArchiveReq>,
) -> Response {
    let result = (|| -> Result<(), FinanceError> {
        let target = match table.as_str() {
            "plans" => "finance_plans",
            "intermediate" => "finance_intermediate",
            "countries" => "finance_countries",
            _ => return Err(FinanceError::Invalid("جدول أرشفة غير مدعوم.".into())),
        };
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, req.company_id)?;
        scope.require_admin()?;
        let sql = format!(
            "UPDATE {target} SET archived_at=CURRENT_TIMESTAMP
             WHERE id=?1 AND company_id=?2 AND archived_at IS NULL"
        );
        let changed = tx.execute(&sql, params![req.id, scope.company_id])?;
        if changed == 0 {
            return Err(FinanceError::NotFound("السجل"));
        }
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "archive",
                entity: target,
                entity_id: Some(req.id.to_string()),
                payload: None,
            },
        )?;
        tx.commit()?;
        Ok(())
    })();
    match result {
        Ok(()) => Json(serde_json::json!({"archived": true})).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn get_settings(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<CompanyQuery>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT key,value,updated_at FROM finance_ledger_settings WHERE company_id=?1",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            Ok(serde_json::json!({
                "key": r.get::<_, String>(0)?,
                "value": r.get::<_, String>(1)?,
                "updated_at": r.get::<_, String>(2)?
            }))
        })?;
        let list: Vec<serde_json::Value> = rows.collect::<Result<Vec<_>, _>>()?;
        // القيم الفعّالة: ما ضبطه المستخدم أو الافتراضي المعلن — حتى تعرض الواجهة نتيجة حقيقية.
        let effective: Vec<serde_json::Value> = SETTING_DEFAULTS
            .iter()
            .map(|(key, default)| {
                serde_json::json!({
                    "key": key,
                    "effective": setting_or(&db, scope.company_id, key, default)
                })
            })
            .collect();
        Ok(serde_json::json!({
            "allowed_keys": SETTING_KEYS,
            "settings": list,
            "effective": effective
        }))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

/// يقرأ إعدادًا واحدًا بقيمته الافتراضية — مصدر واحد للقراءة في كل المسارات.
pub fn setting_or(db: &rusqlite::Connection, company_id: i64, key: &str, default: &str) -> String {
    db.query_row(
        "SELECT value FROM finance_ledger_settings WHERE company_id=?1 AND key=?2",
        params![company_id, key],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
    .unwrap_or_else(|| default.to_string())
}

/// أعلام الترحيل الفعّالة لشركة: تُقرأ مرة واحدة في مسار الترحيل فلا تتكرر الاستعلامات.
#[derive(Debug, Clone, Copy)]
pub struct LedgerFlags {
    pub require_period: bool,
    pub require_approval: bool,
    pub allow_backdated: bool,
}

fn flag(db: &rusqlite::Connection, company_id: i64, key: &str, default: bool) -> bool {
    let fallback = if default { "1" } else { "0" };
    matches!(
        setting_or(db, company_id, key, fallback).trim(),
        "1" | "true" | "yes" | "on"
    )
}

/// يقرأ أعلام الترحيل الثلاثة بقيمها الفعّالة (المضبوطة أو الافتراضية المعلنة).
pub fn ledger_flags(db: &rusqlite::Connection, company_id: i64) -> LedgerFlags {
    LedgerFlags {
        require_period: flag(db, company_id, "posting.require_period", true),
        require_approval: flag(db, company_id, "posting.require_approval", false),
        allow_backdated: flag(db, company_id, "posting.allow_backdated", false),
    }
}

pub async fn set_setting(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(req): Json<SetSetting>,
) -> Response {
    let result = (|| -> Result<(), FinanceError> {
        if !SETTING_KEYS.contains(&req.key.as_str()) {
            return Err(FinanceError::Invalid("مفتاح إعداد غير معروف.".into()));
        }
        if req.value.chars().count() > 120 {
            return Err(FinanceError::Invalid("قيمة الإعداد طويلة جدًا.".into()));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, req.company_id)?;
        scope.require_admin()?;
        tx.execute(
            "INSERT INTO finance_ledger_settings(company_id,key,value,updated_at)
             VALUES(?1,?2,?3,CURRENT_TIMESTAMP)
             ON CONFLICT(company_id,key) DO UPDATE SET value=excluded.value, updated_at=CURRENT_TIMESTAMP",
            params![scope.company_id, req.key, req.value.trim()],
        )?;
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "ledgersettings.set",
                entity: "finance_ledger_settings",
                entity_id: Some(req.key.clone()),
                payload: Some(serde_json::json!({"value": req.value})),
            },
        )?;
        tx.commit()?;
        Ok(())
    })();
    match result {
        Ok(()) => Json(serde_json::json!({"saved": true})).into_response(),
        Err(e) => error_response(e),
    }
}
