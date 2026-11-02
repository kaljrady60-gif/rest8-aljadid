//! مصدر واحد لترحيل قيد إلى دفتر الأستاذ.
//!
//! كل مسار يُنشئ قيدًا محاسبيًا (قيود اليومية، الشيكات، الإقفال السنوي) يمرّ من هنا،
//! فلا تتكرر بنية `drafts/draft_lines/journal_entries/journal_lines` في أكثر من موضع،
//! ويبقى احترام مشغّلات عدم قابلية التعديل في مكان واحد.
use super::audit;
use super::scope::FinanceError;
use rusqlite::{Connection, OptionalExtension, params};
use rust_decimal::Decimal;

/// سطر قيد: (معرّف الحساب، مدين، دائن).
pub type Line = (i64, Decimal, Decimal);

/// سياق الترحيل: الشركة والفرع والفترة.
#[derive(Debug, Clone, Copy)]
pub struct Ctx {
    pub company_id: i64,
    pub branch_id: i64,
    pub period_id: i64,
}

fn text(value: Decimal) -> String {
    format!("{value:.4}")
}

/// يحوّل تاريخًا إلى الفترة المفتوحة التي تغطيه، أو يرفض بوضوح.
pub fn period_for(db: &Connection, company_id: i64, date: &str) -> Result<i64, FinanceError> {
    let period: Option<i64> = db
        .query_row(
            "SELECT p.id FROM financial_periods p
             JOIN finance_period_context c ON c.period_id=p.id
             WHERE c.company_id=?1 AND p.status='open' AND ?2 BETWEEN p.start_date AND p.end_date",
            params![company_id, date],
            |r| r.get(0),
        )
        .optional()?;
    period.ok_or_else(|| {
        FinanceError::Invalid("لا توجد فترة مفتوحة لهذه الشركة تغطي تاريخ القيد.".into())
    })
}

/// سياق كامل لتاريخ محدد.
pub fn context(db: &Connection, company_id: i64, date: &str) -> Result<Ctx, FinanceError> {
    let branch_id: Option<i64> = db
        .query_row(
            "SELECT id FROM finance_branches WHERE company_id=?1 ORDER BY id LIMIT 1",
            [company_id],
            |r| r.get(0),
        )
        .optional()?;
    let branch_id = branch_id.ok_or_else(|| FinanceError::Invalid("لا يوجد فرع للشركة.".into()))?;
    Ok(Ctx {
        company_id,
        branch_id,
        period_id: period_for(db, company_id, date)?,
    })
}

/// يُنشئ مسودة + سطورها، ثم قيدًا مرحّلًا بنفس السطور، ويعيد معرّف القيد.
///
/// الترتيب مقصود: الإدخال بلا ترحيل، ثم السطور، ثم القفل — لأن مُشغّل القاعدة
/// يمنع إضافة سطور إلى قيد مرحّل (خطأ كشفه الاختبار الفعلي وأُصلح هنا مركزيًا).
pub fn post_entry(
    db: &Connection,
    ctx: Ctx,
    date: &str,
    description: &str,
    currency: &str,
    lines: &[Line],
    actor_id: Option<i64>,
    action: &str,
) -> Result<i64, FinanceError> {
    if lines.len() < 2 {
        return Err(FinanceError::Invalid("القيد يحتاج سطرين على الأقل.".into()));
    }
    let mut debit = Decimal::ZERO;
    let mut credit = Decimal::ZERO;
    for (_, d, c) in lines {
        if *d < Decimal::ZERO || *c < Decimal::ZERO {
            return Err(FinanceError::Invalid("لا تُقبل قيم سالبة في القيد.".into()));
        }
        if d.is_zero() && c.is_zero() {
            return Err(FinanceError::Invalid("سطر بلا مبلغ.".into()));
        }
        debit += *d;
        credit += *c;
    }
    if debit != credit {
        return Err(FinanceError::Invalid("القيد غير متوازن.".into()));
    }
    let total = text(debit);
    let empty = "[]".to_string();
    db.execute(
        "INSERT INTO drafts(description,date,currency,total,lines_json,lines_count,company_id)
         VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            description,
            date,
            currency,
            total,
            empty,
            lines.len() as i64,
            ctx.company_id
        ],
    )?;
    let draft_id = db.last_insert_rowid();
    for (index, (account, d, c)) in lines.iter().enumerate() {
        db.execute(
            "INSERT INTO draft_lines(draft_id,line_no,account_id,account_snapshot,debit,credit)
             VALUES(?1,?2,?3,(SELECT code || ' · ' || name FROM accounts WHERE id=?3),?4,?5)",
            params![draft_id, index as i64, account, text(*d), text(*c)],
        )?;
    }
    db.execute(
        "INSERT INTO journal_entries(draft_id,description,entry_date,currency,total,finalized,posted_by,period_id,company_id,branch_id)
         VALUES(?1,?2,?3,?4,?5,0,?6,?7,?8,?9)",
        params![
            draft_id,
            description,
            date,
            currency,
            total,
            actor_id,
            ctx.period_id,
            ctx.company_id,
            ctx.branch_id
        ],
    )?;
    let entry_id = db.last_insert_rowid();
    for (index, (account, d, c)) in lines.iter().enumerate() {
        db.execute(
            "INSERT INTO journal_lines(entry_id,line_no,account_id,account_snapshot,debit,credit)
             VALUES(?1,?2,?3,(SELECT code || ' · ' || name FROM accounts WHERE id=?3),?4,?5)",
            params![entry_id, index as i64, account, text(*d), text(*c)],
        )?;
    }
    db.execute(
        "UPDATE journal_entries SET finalized=1 WHERE id=?1",
        [entry_id],
    )?;
    audit::record(
        db,
        audit::Entry {
            company_id: Some(ctx.company_id),
            actor_id,
            action,
            entity: "journal_entries",
            entity_id: Some(entry_id.to_string()),
            payload: Some(serde_json::json!({
                "draft_id": draft_id, "total": total, "lines": lines.len(), "date": date
            })),
        },
    )?;
    Ok(entry_id)
}

/// معرّف حساب تفصيلي في دليل الشركة برمزه، أو `None`.
pub fn account_by_code(
    db: &Connection,
    company_id: i64,
    code: &str,
) -> Result<Option<i64>, FinanceError> {
    Ok(db
        .query_row(
            "SELECT account_id FROM finance_company_account_codes WHERE company_id=?1 AND code=?2",
            params![company_id, code],
            |r| r.get(0),
        )
        .optional()?)
}
