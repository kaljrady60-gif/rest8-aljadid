//! ربط الضرائب بالقيود والسندات: كل قيد أو سند يمكن أن يحمل ضرائب بعدة اتجاهات،
//! والوعاء والضريبة يُحسبان هنا مرة واحدة بدقة عشرية ثم يُخزَّنان نصًّا موحّد المنازل.
use super::audit;
use super::money::render;
use super::scope::{error_response, FinanceError, Scope};
use super::taxes;
use crate::{db_open, AppState};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::str::FromStr;

pub const DOC_TYPES: &[&str] = &["journal", "voucher"];
pub const DIRECTIONS: &[&str] = &["output", "input"];

#[derive(Serialize)]
pub struct Link {
    id: i64,
    doc_type: String,
    doc_id: i64,
    tax_id: i64,
    tax_code: String,
    direction: String,
    base: String,
    rate_bp: i64,
    tax_amount: String,
    inclusive: bool,
    account_id: i64,
    doc_date: Option<String>,
    archived_at: Option<String>,
}

#[derive(Deserialize)]
pub struct LinksQuery {
    company_id: i64,
    doc_type: Option<String>,
    doc_id: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewLink {
    company_id: i64,
    doc_type: String,
    doc_id: i64,
    tax_id: i64,
    amount: String,
    direction: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unlink {
    company_id: i64,
    id: i64,
}

#[derive(Deserialize)]
pub struct ReportQuery {
    company_id: i64,
    from: Option<String>,
    to: Option<String>,
    direction: Option<String>,
}

/// سطر ملخّص ضريبي واحد بعد التجميع.
#[derive(Serialize)]
pub struct Summary {
    tax_id: i64,
    tax_code: String,
    direction: String,
    rate_bp: i64,
    base: String,
    tax_amount: String,
    links: i64,
}

/// تاريخ المستند الفعلي: القيد من `journal_entries`، والسند عبر مسودّته.
fn doc_date(db: &Connection, doc_type: &str, doc_id: i64) -> Result<Option<String>, FinanceError> {
    let date = if doc_type == "journal" {
        db.query_row(
            "SELECT entry_date FROM journal_entries WHERE id=?1",
            [doc_id],
            |r| r.get(0),
        )
        .optional()?
    } else {
        db.query_row(
            "SELECT je.entry_date FROM financial_vouchers v
             JOIN journal_entries je ON je.draft_id = v.draft_id
             WHERE v.id=?1",
            [doc_id],
            |r| r.get(0),
        )
        .optional()?
    };
    Ok(date)
}

fn doc_exists(db: &Connection, company_id: i64, doc_type: &str, doc_id: i64) -> rusqlite::Result<bool> {
    let found: bool = if doc_type == "journal" {
        db.query_row(
            "SELECT EXISTS(SELECT 1 FROM journal_entries WHERE id=?1 AND company_id=?2)",
            params![doc_id, company_id],
            |r| r.get(0),
        )?
    } else {
        db.query_row(
            "SELECT EXISTS(SELECT 1 FROM financial_vouchers v
                JOIN drafts d ON d.id = v.draft_id
                WHERE v.id=?1 AND d.company_id=?2)",
            params![doc_id, company_id],
            |r| r.get(0),
        )?
    };
    Ok(found)
}

/// سطر رابط خام قبل التجميع: (معرّف الضريبة، رمزها، الاتجاه، النسبة، الوعاء، الضريبة).
type Row = (i64, String, String, i64, Decimal, Decimal);

/// دلو تجميع لضريبة باتجاه واحد: (الرمز، النسبة، الوعاء، الضريبة، عدد الروابط).
type Bucket = (String, i64, Decimal, Decimal, i64);

/// تجميع روابط الضرائب في ملخّص لكل (ضريبة، اتجاه).
///
/// دالة نقية مستقلة عن قاعدة البيانات حتى تُختبر حسابيًا: التجميع بـ `Decimal` لا
/// بالفاصلة العائمة، والمخرجات تُرتَب بالنص الموحّد نفسه المستخدم في العرض.
fn aggregate(rows: &[Row]) -> Vec<Summary> {
    let mut buckets: BTreeMap<(i64, String), Bucket> = BTreeMap::new();
    for (tax_id, code, direction, rate_bp, base, tax) in rows {
        let entry = buckets
            .entry((*tax_id, direction.clone()))
            .or_insert_with(|| (code.clone(), *rate_bp, Decimal::ZERO, Decimal::ZERO, 0));
        entry.2 += *base;
        entry.3 += *tax;
        entry.4 += 1;
    }
    buckets
        .into_iter()
        .map(|((tax_id, direction), (code, rate_bp, base, tax, links))| Summary {
            tax_id,
            tax_code: code,
            direction,
            rate_bp,
            base: render(base),
            tax_amount: render(tax),
            links,
        })
        .collect()
}

pub async fn link(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<NewLink>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        if !DOC_TYPES.contains(&n.doc_type.as_str()) {
            return Err(FinanceError::Invalid(
                "نوع المستند يجب أن يكون قيدًا أو سندًا.".into(),
            ));
        }
        if !DIRECTIONS.contains(&n.direction.as_str()) {
            return Err(FinanceError::Invalid(
                "اتجاه الضريبة يجب أن يكون مخرجة أو مدخلة.".into(),
            ));
        }
        let amount = Decimal::from_str(n.amount.trim())
            .map_err(|_| FinanceError::Invalid("المبلغ ليس رقمًا صالحًا.".into()))?;
        if amount <= Decimal::ZERO {
            return Err(FinanceError::Invalid(
                "المبلغ يجب أن يكون أكبر من صفر.".into(),
            ));
        }
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        if !doc_exists(&tx, scope.company_id, &n.doc_type, n.doc_id)? {
            return Err(FinanceError::NotFound("المستند"));
        }
        let tax = tx
            .query_row(
                "SELECT code,rate_bp,account_id,is_inclusive FROM finance_taxes
                 WHERE id=?1 AND company_id=?2 AND archived_at IS NULL",
                params![n.tax_id, scope.company_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)? != 0,
                    ))
                },
            )
            .optional()?
            .ok_or(FinanceError::NotFound("الضريبة"))?;
        let (base, tax_amount) = taxes::split(amount, tax.1, tax.3);
        if tax_amount <= Decimal::ZERO {
            return Err(FinanceError::Invalid(
                "نسبة الضريبة صفر فلا يوجد مبلغ لربطه.".into(),
            ));
        }
        let inserted = tx.execute(
            "INSERT INTO finance_tax_links
             (company_id,doc_type,doc_id,tax_id,tax_code,direction,base,rate_bp,tax_amount,inclusive,account_id)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                scope.company_id,
                n.doc_type,
                n.doc_id,
                n.tax_id,
                tax.0,
                n.direction,
                render(base),
                tax.1,
                render(tax_amount),
                tax.3 as i64,
                tax.2
            ],
        );
        if let Err(e) = inserted {
            return Err(match e {
                rusqlite::Error::SqliteFailure(err, _)
                    if err.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    FinanceError::Conflict("هذه الضريبة مرتبطة بالمستند بهذا الاتجاه مسبقًا.")
                }
                other => FinanceError::Storage(other),
            });
        }
        let id = tx.last_insert_rowid();
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "tax.link",
                entity: "finance_tax_links",
                entity_id: Some(id.to_string()),
                payload: Some(serde_json::json!({
                    "doc_type": n.doc_type,
                    "doc_id": n.doc_id,
                    "tax_id": n.tax_id,
                    "direction": n.direction,
                    "tax": render(tax_amount),
                })),
            },
        )?;
        tx.commit()?;
        Ok(serde_json::json!({
            "id": id,
            "base": render(base),
            "rate_bp": tax.1,
            "tax": render(tax_amount),
            "total": render(base + tax_amount),
        }))
    })();
    match result {
        Ok(body) => (StatusCode::CREATED, Json(body)).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn links(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<LinksQuery>,
) -> Response {
    let result = (|| -> Result<Vec<Link>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let mut stmt = db.prepare(
            "SELECT id,doc_type,doc_id,tax_id,tax_code,direction,base,rate_bp,tax_amount,
                    inclusive,account_id,archived_at
             FROM finance_tax_links
             WHERE company_id=?1
               AND (?2 IS NULL OR doc_type=?2)
               AND (?3 IS NULL OR doc_id=?3)
             ORDER BY archived_at IS NOT NULL, id DESC LIMIT 200",
        )?;
        let mut rows = stmt
            .query_map(params![scope.company_id, q.doc_type, q.doc_id], |r| {
                Ok(Link {
                    id: r.get(0)?,
                    doc_type: r.get(1)?,
                    doc_id: r.get(2)?,
                    tax_id: r.get(3)?,
                    tax_code: r.get(4)?,
                    direction: r.get(5)?,
                    base: r.get(6)?,
                    rate_bp: r.get(7)?,
                    tax_amount: r.get(8)?,
                    inclusive: r.get::<_, i64>(9)? != 0,
                    account_id: r.get(10)?,
                    doc_date: None,
                    archived_at: r.get(11)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for row in rows.iter_mut() {
            row.doc_date = doc_date(&db, &row.doc_type, row.doc_id)?;
        }
        Ok(rows)
    })();
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn unlink(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(n): Json<Unlink>,
) -> Response {
    let result = (|| -> Result<(), FinanceError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, n.company_id)?;
        // نقرأ تفاصيل الرابط قبل حذفه: الأثر التفصيلي يبقى في سجل التدقيق.
        let row: Option<(String, i64, i64, String, String)> = tx
            .query_row(
                "SELECT doc_type,doc_id,tax_id,direction,tax_amount FROM finance_tax_links
                 WHERE id=?1 AND company_id=?2",
                params![n.id, scope.company_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?;
        let (doc_type, doc_id, tax_id, direction, tax_amount) =
            row.ok_or(FinanceError::NotFound("الرابط"))?;
        // الحذف (لا الأرشفة) لأن قيد التفرّد يخصّ الرابط الفعّال، فيجب أن يسمح بالربط من جديد.
        tx.execute(
            "DELETE FROM finance_tax_links WHERE id=?1 AND company_id=?2",
            params![n.id, scope.company_id],
        )?;
        audit::record(
            &tx,
            audit::Entry {
                company_id: Some(scope.company_id),
                actor_id: Some(actor.id),
                action: "tax.unlink",
                entity: "finance_tax_links",
                entity_id: Some(n.id.to_string()),
                payload: Some(serde_json::json!({
                    "doc_type": doc_type,
                    "doc_id": doc_id,
                    "tax_id": tax_id,
                    "direction": direction,
                    "tax": tax_amount
                })),
            },
        )?;
        tx.commit()?;
        Ok(())
    })();
    match result {
        Ok(()) => Json(serde_json::json!({"removed": true})).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn report(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ReportQuery>,
) -> Response {
    let result = (|| -> Result<Vec<Summary>, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        if let Some(dir) = q.direction.as_deref()
            && !DIRECTIONS.contains(&dir)
        {
            return Err(FinanceError::Invalid(
                "اتجاه الضريبة يجب أن يكون مخرجة أو مدخلة.".into(),
            ));
        }
        let mut stmt = db.prepare(
            "SELECT tax_id,tax_code,direction,rate_bp,base,tax_amount,doc_type,doc_id
             FROM finance_tax_links
             WHERE company_id=?1 AND archived_at IS NULL
             ORDER BY tax_code",
        )?;
        let raw = stmt
            .query_map([scope.company_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, i64>(7)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut rows = Vec::new();
        for (tax_id, code, direction, rate_bp, base, tax_amount, doc_type, doc_id) in raw {
            if q.direction.as_deref().is_some_and(|d| d != direction) {
                continue;
            }
            if q.from.is_some() || q.to.is_some() {
                let day = doc_date(&db, &doc_type, doc_id)?;
                let inside_from = q.from.as_deref().is_none_or(|f| day.as_deref().is_some_and(|d| d >= f));
                let inside_to = q.to.as_deref().is_none_or(|t| day.as_deref().is_some_and(|d| d <= t));
                if !inside_from || !inside_to {
                    continue;
                }
            }
            let base = Decimal::from_str(&base)
                .map_err(|_| FinanceError::Invalid("وعاء ضريبي مخزَّن بصيغة غير صالحة.".into()))?;
            let tax = Decimal::from_str(&tax_amount).map_err(|_| {
                FinanceError::Invalid("مبلغ ضريبي مخزَّن بصيغة غير صالحة.".into())
            })?;
            rows.push((tax_id, code, direction, rate_bp, base, tax));
        }
        Ok(aggregate(&rows))
    })();
    match result {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => error_response(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    fn d(text: &str) -> Decimal {
        Decimal::from_str(text).expect("decimal")
    }

    #[test]
    fn aggregate_sums_base_and_tax_per_tax_and_direction() {
        let rows = vec![
            (1, "VAT15".to_string(), "output".to_string(), 1500, d("1000"), d("150")),
            (1, "VAT15".to_string(), "output".to_string(), 1500, d("500.5"), d("75.075")),
            (2, "WHT5".to_string(), "input".to_string(), 500, d("2000"), d("100")),
        ];
        let out = aggregate(&rows);
        assert_eq!(out.len(), 2);
        let vat = out.iter().find(|s| s.tax_code == "VAT15").expect("vat");
        assert_eq!(vat.base, "1500.5000");
        assert_eq!(vat.tax_amount, "225.0750");
        assert_eq!(vat.links, 2);
        let wht = out.iter().find(|s| s.tax_code == "WHT5").expect("wht");
        assert_eq!(wht.tax_amount, "100.0000");
        assert_eq!(wht.direction, "input");
    }

    #[test]
    fn aggregate_keeps_directions_apart_for_the_same_tax() {
        let rows = vec![
            (7, "VAT".to_string(), "output".to_string(), 1500, d("100"), d("15")),
            (7, "VAT".to_string(), "input".to_string(), 1500, d("40"), d("6")),
        ];
        let out = aggregate(&rows);
        assert_eq!(out.len(), 2);
        let output = out.iter().find(|s| s.direction == "output").expect("output");
        let input = out.iter().find(|s| s.direction == "input").expect("input");
        assert_eq!(output.tax_amount, "15.0000");
        assert_eq!(output.base, "100.0000");
        assert_eq!(input.tax_amount, "6.0000");
        assert_eq!(input.base, "40.0000");
    }
}
