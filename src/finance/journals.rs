//! القيود اليومية المرتبطة بالشركة: مسودات محميّة بالنطاق، وترحيل ذرّي.
//! هذا هو بديل المسار القديم `/api/drafts` الذي كان يعمل بلا عزل شركة.
use super::audit;
use super::money::{self, render};
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open, domain};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use rust_decimal::Decimal;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct ListQuery {
    company_id: i64,
    status: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewJournal {
    company_id: i64,
    description: String,
    date: String,
    currency: String,
    lines: Vec<domain::LineInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostRequest {
    company_id: i64,
    #[serde(default)]
    dimensions: Vec<i64>,
    /// فرع القيد؛ عند غيابه يُسجَّل على أول فرع للشركة.
    #[serde(default)]
    branch_id: Option<i64>,
}

pub async fn list(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ListQuery>,
) -> Response {
    let result = (|| -> Result<Vec<serde_json::Value>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let status = q.status.as_deref().unwrap_or("all");
        if !["all", "draft", "posted"].contains(&status) {
            return Err(FinanceError::Invalid("حالة تصفية غير مدعومة.".into()));
        }
        let mut stmt = db.prepare(
            "SELECT d.id,d.description,d.date,d.currency,d.total,
                    EXISTS(SELECT 1 FROM journal_entries e WHERE e.draft_id=d.id AND e.finalized=1)
             FROM drafts d WHERE d.company_id=?1
             ORDER BY d.date DESC, d.id DESC LIMIT 200",
        )?;
        let rows = stmt.query_map([scope.company_id], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?, "description": r.get::<_, String>(1)?,
                "date": r.get::<_, String>(2)?, "currency": r.get::<_, String>(3)?,
                "total": r.get::<_, String>(4)?,
                "posted": r.get::<_, i64>(5)? == 1
            }))
        })?;
        let mut items = Vec::new();
        for row in rows {
            let item = row?;
            let posted = item["posted"].as_bool().unwrap_or(false);
            let keep = match status {
                "draft" => !posted,
                "posted" => posted,
                _ => true,
            };
            if keep {
                items.push(item);
            }
        }
        Ok(items)
    })();
    match result {
        Ok(items) => Json(items).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn create(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(input): Json<NewJournal>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        // العميل يرسل معرّف الحساب؛ نُكمل وسمه من الدليل قبل التحقق الموحّد.
        let mut lines = input.lines.clone();
        {
            let db = db_open(&state.db)?;
            for line in &mut lines {
                if line.account.trim().is_empty()
                    && let Some(id) = line.account_id
                {
                    let label: Option<String> = db
                        .query_row(
                            "SELECT code || ' · ' || name FROM accounts WHERE id=?1",
                            [id],
                            |r| r.get(0),
                        )
                        .optional()?;
                    if let Some(label) = label {
                        line.account = label;
                    }
                }
            }
        }
        let totals = domain::validate(&domain::DraftInput {
            description: input.description.clone(),
            date: input.date.clone(),
            currency: input.currency.clone(),
            lines,
        })
        .map_err(FinanceError::Invalid)?;
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, input.company_id)?;
        if !scope.is_admin && !crate::identity::is_current_admin(&tx, &actor)? {
            return Err(FinanceError::Forbidden);
        }
        let mut accounts = Vec::with_capacity(input.lines.len());
        for line in &input.lines {
            let id = line
                .account_id
                .ok_or_else(|| FinanceError::Invalid("كل سطر يحتاج حسابًا.".into()))?;
            if !scope.owns_account(&tx, id)? {
                return Err(FinanceError::Invalid(
                    "حساب من خارج دليل هذه الشركة.".into(),
                ));
            }
            let (kind, currency): (String, String) = tx.query_row(
                "SELECT kind,currency FROM accounts WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            if kind != "detail" {
                return Err(FinanceError::Invalid(
                    "القيود تُسجَّل على حسابات تفصيلية فقط.".into(),
                ));
            }
            if currency != input.currency {
                return Err(FinanceError::Invalid(
                    "عملة الحساب تختلف عن عملة القيد.".into(),
                ));
            }
            accounts.push(id);
        }
        tx.execute(
            "INSERT INTO drafts(description,date,currency,total,lines_json,lines_count,company_id)
             VALUES(?1,?2,?3,?4,'[]',?5,?6)",
            params![
                input.description.trim(),
                input.date,
                input.currency,
                totals.debit,
                input.lines.len() as i64,
                scope.company_id
            ],
        )?;
        let draft_id = tx.last_insert_rowid();
        for (index, line) in input.lines.iter().enumerate() {
            let debit = money::parse(&line.debit, "مدين")?;
            let credit = money::parse(&line.credit, "دائن")?;
            let snapshot: String = tx.query_row(
                "SELECT name FROM accounts WHERE id=?1",
                [accounts[index]],
                |r| r.get(0),
            )?;
            tx.execute(
                "INSERT INTO draft_lines(draft_id,line_no,account_id,account_snapshot,debit,credit)
                 VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    draft_id,
                    index as i64,
                    accounts[index],
                    snapshot,
                    render(debit),
                    render(credit)
                ],
            )?;
        }
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "journal.create",
                entity: "drafts",
                entity_id: Some(draft_id.to_string()),
                payload: Some(serde_json::json!({"total": totals.debit, "lines": accounts.len()})),
            },
        )?;
        tx.commit()?;
        Ok(draft_id)
    })();
    match result {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn post(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Path(draft_id): Path<i64>,
    Json(req): Json<PostRequest>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        if !state.experimental_posting {
            return Err(FinanceError::Forbidden);
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, req.company_id)?;
        if !scope.is_admin && !crate::identity::is_current_admin(&tx, &actor)? {
            return Err(FinanceError::Forbidden);
        }
        let exists: Option<(Option<i64>, String, String)> = tx
            .query_row(
                "SELECT company_id,currency,total FROM drafts WHERE id=?1",
                [draft_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (company_id, currency, total) = exists.ok_or(FinanceError::NotFound("المسودة"))?;
        if company_id != Some(scope.company_id) {
            return Err(FinanceError::Forbidden);
        }
        let already: Option<i64> = tx
            .query_row(
                "SELECT id FROM journal_entries WHERE draft_id=?1 AND finalized=1",
                [draft_id],
                |r| r.get(0),
            )
            .optional()?;
        if already.is_some() {
            return Err(FinanceError::Conflict("المسودة مرحّلة مسبقًا."));
        }
        let draft = crate::drafts::read_draft(&tx, draft_id)
            .map_err(|_| FinanceError::Invalid("مسودة تالفة.".into()))?
            .ok_or(FinanceError::NotFound("المسودة"))?;
        // متغيرات الأستاذ تحكم الترحيل فعليًا هنا: الفترة والاعتماد والترحيل الرجعي.
        let flags = super::settings5::ledger_flags(&tx, scope.company_id);
        let period: Option<i64> = tx
            .query_row(
                "SELECT p.id FROM financial_periods p
                 JOIN finance_period_context c ON c.period_id=p.id
                 WHERE c.company_id=?1 AND p.status='open' AND ?2 BETWEEN p.start_date AND p.end_date",
                params![scope.company_id, draft.date],
                |r| r.get(0),
            )
            .optional()?;
        if period.is_none() && flags.require_period {
            return Err(FinanceError::Invalid(
                "لا توجد فترة مفتوحة لهذه الشركة تغطي تاريخ القيد.".into(),
            ));
        }
        // الترحيل الرجعي: تاريخ يسبق أحدث فترة مفتوحة (فترة أقدم ما زالت مفتوحة).
        if !flags.allow_backdated {
            let newest_open: Option<String> = tx
                .query_row(
                    "SELECT MAX(p.start_date) FROM financial_periods p
                     JOIN finance_period_context c ON c.period_id=p.id
                     WHERE c.company_id=?1 AND p.status='open'",
                    [scope.company_id],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            if let Some(start) = newest_open
                && draft.date.as_str() < start.as_str()
            {
                return Err(FinanceError::Invalid(
                    "الترحيل بتاريخ يسبق أحدث فترة مفتوحة غير مسموح حسب متغيرات الأستاذ.".into(),
                ));
            }
        }
        super::dimensions::ensure_active(&tx, scope.company_id, &req.dimensions)?;
        // سياسة الاعتماد: لا ترحيل قبل اعتماد مُعتمد مختلف عن المُرسل.
        super::approval::gate(&tx, scope.company_id, draft_id)?;
        if flags.require_approval {
            super::approval::require_approved(&tx, draft_id)?;
        }
        // الإدخال بلا ترحيل ثم قفل بعد السطور: مشغّل القاعدة يمنع إضافة سطور لقيد مرحّل.
        let branch_id = super::branches::ensure_branch(&tx, scope.company_id, req.branch_id)?
            .unwrap_or(default_branch(&tx, scope.company_id)?);
        tx.execute(
            "INSERT INTO journal_entries(draft_id,description,entry_date,currency,total,finalized,posted_by,period_id,company_id,branch_id)
             VALUES(?1,?2,?3,?4,?5,0,?6,?7,?8,?9)",
            params![draft_id, draft.description, draft.date, currency, total, actor.id, period, scope.company_id, branch_id],
        )?;
        let entry_id = tx.last_insert_rowid();
        let mut debit = Decimal::ZERO;
        let mut credit = Decimal::ZERO;
        for (index, line) in draft.lines.iter().enumerate() {
            let account_id = line
                .account_id
                .ok_or_else(|| FinanceError::Invalid("سطر بلا حساب.".into()))?;
            let d = money::parse(&line.debit, "مدين")?;
            let c = money::parse(&line.credit, "دائن")?;
            debit += d;
            credit += c;
            tx.execute(
                "INSERT INTO journal_lines(entry_id,line_no,account_id,account_snapshot,debit,credit)
                 VALUES(?1,?2,?3,?4,?5,?6)",
                params![entry_id, index as i64, account_id, line.account, render(d), render(c)],
            )?;
            for dimension in &req.dimensions {
                tx.execute(
                    "INSERT INTO finance_entry_dimensions(entry_id,line_no,dimension_id,company_id)
                     VALUES(?1,?2,?3,?4)",
                    params![entry_id, index as i64, dimension, scope.company_id],
                )?;
            }
        }
        if debit != credit {
            return Err(FinanceError::Invalid("القيد غير متوازن.".into()));
        }
        tx.execute(
            "UPDATE journal_entries SET finalized=1 WHERE id=?1",
            params![entry_id],
        )?;
        super::approval::mark_posted(&tx, scope.company_id, draft_id, actor.id)?;
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "journal.post",
                entity: "journal_entries",
                entity_id: Some(entry_id.to_string()),
                payload: Some(serde_json::json!({
                    "draft_id": draft_id,
                    "total": render(debit),
                    // الأعلام الفعّالة وقت الترحيل تبقى في السجل: أي إعداد حكم القيد.
                    "require_period": flags.require_period,
                    "require_approval": flags.require_approval,
                    "allow_backdated": flags.allow_backdated,
                    "period_id": period,
                })),
            },
        )?;
        tx.commit()?;
        Ok(entry_id)
    })();
    match result {
        Ok(id) => Json(serde_json::json!({"entry_id": id})).into_response(),
        Err(e) => error_response(e),
    }
}

/// أول فرع للشركة — يُستخدم حين لا يُحدَّد فرع عند الترحيل.
fn default_branch(db: &rusqlite::Connection, company_id: i64) -> Result<i64, FinanceError> {
    db.query_row(
        "SELECT id FROM finance_branches WHERE company_id=?1 ORDER BY id LIMIT 1",
        [company_id],
        |r| r.get(0),
    )
    .optional()?
    .ok_or_else(|| FinanceError::Invalid("لا يوجد فرع لهذه الشركة.".into()))
}

/// تفاصيل قيد واحد (مسودة أو مرحّل) بسطوره وأبعاده — تُغذّي نافذة التفاصيل.
pub async fn detail(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    axum::extract::Path(id): axum::extract::Path<i64>,
    Query(q): Query<ListQuery>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let draft: Option<(String, String, String, String)> = db
            .query_row(
                "SELECT description,date,currency,total FROM drafts WHERE id=?1 AND company_id=?2",
                params![id, scope.company_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let (description, date, currency, total) =
            draft.ok_or_else(|| FinanceError::NotFound("القيد غير موجود لهذه الشركة."))?;
        let mut stmt = db.prepare(
            "SELECT line_no,COALESCE(account_snapshot,''),debit,credit
             FROM draft_lines WHERE draft_id=?1 ORDER BY line_no",
        )?;
        let rows = stmt.query_map([id], |r| {
            Ok(serde_json::json!({
                "line_no": r.get::<_, i64>(0)?,
                "account": r.get::<_, String>(1)?,
                "debit": r.get::<_, String>(2)?,
                "credit": r.get::<_, String>(3)?
            }))
        })?;
        let lines = rows.collect::<Result<Vec<_>, _>>()?;
        let entry: Option<i64> = db
            .query_row(
                "SELECT id FROM journal_entries WHERE draft_id=?1 AND finalized=1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        let dimensions: Vec<String> = match entry {
            Some(entry_id) => {
                let mut stmt = db.prepare(
                    "SELECT DISTINCT d.code FROM finance_entry_dimensions x
                     JOIN finance_dimensions d ON d.id=x.dimension_id
                     WHERE x.entry_id=?1 ORDER BY d.code",
                )?;
                let rows = stmt.query_map([entry_id], |r| r.get::<_, String>(0))?;
                rows.collect::<Result<Vec<_>, _>>()?
            }
            None => Vec::new(),
        };
        Ok(serde_json::json!({
            "id": id, "description": description, "date": date, "currency": currency,
            "total": total, "posted": entry.is_some(), "entry_id": entry,
            "lines": lines, "dimensions": dimensions
        }))
    })();
    match result {
        Ok(body) => Json(body).into_response(),
        Err(e) => error_response(e),
    }
}
