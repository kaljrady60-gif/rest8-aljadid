//! أدوات مشتركة لقسم المبيعات: تحويل المبالغ، الترقيم، وحلّ حسابات الترحيل.
use super::schema::{MONEY_SCALE, QTY_SCALE};
use crate::finance::scope::FinanceError;
use rusqlite::{Connection, OptionalExtension, params};

/// يعرض الوحدة الصغرى كنص عشري بمنزلتين (150250 → 1502.50).
pub fn money(minor: i64) -> String {
    let sign = if minor < 0 { "-" } else { "" };
    let abs = minor.abs();
    format!("{sign}{}.{:02}", abs / MONEY_SCALE, abs % MONEY_SCALE)
}

/// يقرأ نصًّا عشريًا (لا سالب) ويحوّله إلى وحدات صغرى بتقريب مصرفي.
pub fn parse_money(raw: &str, field: &str) -> Result<i64, FinanceError> {
    let text = raw.trim().replace(',', "");
    if text.is_empty() || text.len() > 24 {
        return Err(FinanceError::Invalid(format!("قيمة {field} غير صالحة.")));
    }
    let value: f64 = text
        .parse()
        .map_err(|_| FinanceError::Invalid(format!("قيمة {field} ليست رقمًا صالحًا.")))?;
    if value.is_nan() || value.is_infinite() || value < 0.0 {
        return Err(FinanceError::Invalid(format!(
            "قيمة {field} يجب أن تكون موجبة."
        )));
    }
    Ok((value * MONEY_SCALE as f64).round() as i64)
}

/// الكمية تُخزَّن بألف من الوحدة (1500 = 1.5).
pub fn parse_qty(raw: &str, field: &str) -> Result<i64, FinanceError> {
    let text = raw.trim().replace(',', "");
    let value: f64 = text
        .parse()
        .map_err(|_| FinanceError::Invalid(format!("كمية {field} ليست رقمًا صالحًا.")))?;
    if value.is_nan() || value.is_infinite() || value <= 0.0 {
        return Err(FinanceError::Invalid(format!(
            "كمية {field} يجب أن تكون أكبر من صفر."
        )));
    }
    Ok((value * QTY_SCALE as f64).round() as i64)
}

pub fn qty_text(qty_milli: i64) -> String {
    format!(
        "{}.{:03}",
        qty_milli / QTY_SCALE,
        (qty_milli % QTY_SCALE).abs()
    )
}

/// صافي السطر بالوحدات الصغرى: كمية × سعر − خصم السطر.
pub fn line_net(qty_milli: i64, price_minor: i64, discount_minor: i64) -> i64 {
    let gross = (qty_milli as i128 * price_minor as i128) / QTY_SCALE as i128;
    let net = gross - discount_minor as i128;
    net.max(0) as i64
}

/// ترقيم متسلسل لكل (شركة، نوع، سنة) داخل معاملة فورية.
pub fn next_number(
    tx: &Connection,
    company_id: i64,
    kind: &str,
    date: &str,
) -> Result<String, FinanceError> {
    let year: String = date.chars().take(4).collect();
    let year = if year.len() == 4 { year } else { "2026".into() };
    tx.execute(
        "INSERT INTO sales_sequences(company_id,kind,year,last) VALUES(?1,?2,?3,0)
         ON CONFLICT(company_id,kind,year) DO NOTHING",
        params![company_id, kind, year],
    )?;
    tx.execute(
        "UPDATE sales_sequences SET last=last+1 WHERE company_id=?1 AND kind=?2 AND year=?3",
        params![company_id, kind, year],
    )?;
    let last: i64 = tx.query_row(
        "SELECT last FROM sales_sequences WHERE company_id=?1 AND kind=?2 AND year=?3",
        params![company_id, kind, year],
        |r| r.get(0),
    )?;
    Ok(format!("{kind}-{year}-{last:04}"))
}

/// حساب الطرف (الذمم) وعملته — مصدر العملة الافتراضية للفاتورة.
pub fn party_account(
    db: &Connection,
    company_id: i64,
    party_id: i64,
) -> Result<(i64, String), FinanceError> {
    let row: Option<(i64, String, String)> = db
        .query_row(
            "SELECT p.account_id, COALESCE(a.currency,'YER'), p.role
             FROM finance_parties p LEFT JOIN accounts a ON a.id=p.account_id
             WHERE p.company_id=?1 AND p.id=?2",
            params![company_id, party_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let (account_id, currency, role) = row.ok_or(FinanceError::NotFound("العميل"))?;
    if role == "supplier" {
        return Err(FinanceError::Invalid(
            "الطرف مسجّل موردًا لا عميلًا.".into(),
        ));
    }
    Ok((account_id, currency))
}

fn first_detail(db: &Connection, company_id: i64, sql: &str) -> Option<i64> {
    db.query_row(sql, params![company_id], |r| r.get(0)).optional().ok().flatten()
}

/// حساب الإيرادات: أول حساب إيرادي تفصيلي في الشركة.
pub fn revenue_account(db: &Connection, company_id: i64) -> Result<i64, FinanceError> {
    first_detail(
        db,
        company_id,
        "SELECT a.id FROM accounts a
         JOIN finance_account_scopes s ON s.account_id=a.id AND s.company_id=?1
         WHERE a.kind='detail' AND a.category='revenue' ORDER BY a.code LIMIT 1",
    )
    .ok_or_else(|| FinanceError::Invalid("لا يوجد حساب إيرادات تفصيلي في الدليل.".into()))
}

/// حساب ضريبة المخرجات إن وُجد؛ غيابه يعني فاتورة بلا ضريبة لا خطأً.
pub fn tax_account(db: &Connection, company_id: i64) -> Option<i64> {
    first_detail(
        db,
        company_id,
        "SELECT a.id FROM accounts a
         JOIN finance_account_scopes s ON s.account_id=a.id AND s.company_id=?1
         WHERE a.kind='detail' AND a.category='liability'
           AND (a.name LIKE '%ضريب%' OR a.name LIKE '%ضرائب%' OR a.name LIKE '%مخرجات%')
         ORDER BY a.code LIMIT 1",
    )
}

/// حساب إيرادات مبيعات تفصيلي: يُفضّل اسمًا صريحًا، وإلا يُنشأ فرعًا تحت مجموعة 4100.
pub fn ensure_revenue_account(tx: &Connection, company_id: i64) -> Result<i64, FinanceError> {
    if let Some(id) = first_detail(
        tx,
        company_id,
        "SELECT a.id FROM accounts a
         JOIN finance_account_scopes s ON s.account_id=a.id AND s.company_id=?1
         WHERE a.kind='detail' AND a.category='revenue' AND a.name LIKE '%مبيعات%'
         ORDER BY a.code LIMIT 1",
    ) {
        return Ok(id);
    }
    let has_group: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM finance_company_account_codes
                       WHERE company_id=?1 AND code='4100')",
        [company_id],
        |r| r.get(0),
    )?;
    if has_group {
        return super::customers::provision_account(
            tx, company_id, "4100", "إيرادات المبيعات", "revenue", "credit");
    }
    revenue_account(tx, company_id)
}

pub fn account_label(db: &Connection, company_id: i64, id: i64) -> String {
    // رمز الشركة أولًا (1200-0001)؛ والرمز الداخلي احتياطًا.
    db.query_row(
        "SELECT COALESCE(
            (SELECT c.code FROM finance_company_account_codes c
              WHERE c.company_id=?1 AND c.account_id=?2),
            a.code) || ' · ' || a.name
         FROM accounts a WHERE a.id=?2",
        params![company_id, id],
        |r| r.get::<_, String>(0),
    )
    .unwrap_or_else(|_| "حساب غير معروف".into())
}

/// حساب المصروف/الاستحقاق للعمولات: أول حساب مصروف باسم عمولة.
pub fn commission_expense_account(db: &Connection, company_id: i64) -> Option<i64> {
    first_detail(
        db,
        company_id,
        "SELECT a.id FROM accounts a
         JOIN finance_account_scopes s ON s.account_id=a.id AND s.company_id=?1
         WHERE a.kind='detail' AND a.category='expense' AND a.name LIKE '%عمول%'
         ORDER BY a.code LIMIT 1",
    )
}

/// حساب الالتزام المقابل لاستحقاق العمولة.
pub fn commission_payable_account(db: &Connection, company_id: i64) -> Option<i64> {
    first_detail(
        db,
        company_id,
        "SELECT a.id FROM accounts a
         JOIN finance_account_scopes s ON s.account_id=a.id AND s.company_id=?1
         WHERE a.kind='detail' AND a.category='liability'
           AND (a.name LIKE '%عمول%' OR a.name LIKE '%مستحق%' OR a.name LIKE '%دائن%')
         ORDER BY a.code LIMIT 1",
    )
}
