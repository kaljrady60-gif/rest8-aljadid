//! توحيد بيانات القيود القديمة على الصيغة المهنية المرجعية.
//!
//! لا يُعاد كتابة بيان من صنع المستخدم بلا سبب: تُعالج فقط البيانات التي لا تحمل
//! هوية مستند (لا رقم ولا شرطة طويلة)، وتُضاف لها هوية مستند من رقم القيد وسنته،
//! مع الاحتفاظ بنص المستخدم كما هو بعد علامة الشرطة.
use super::audit;
use super::narrative;
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use rusqlite::{TransactionBehavior, params};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizeInput {
    company_id: i64,
    #[serde(default)]
    dry_run: bool,
}

/// يشتق نوع المستند من نص البيان — لا يخترع نوعًا لا يشبه النص.
fn label_for(text: &str) -> &'static str {
    let t = text;
    if t.contains("مرتجع") {
        return "مرتجع بيع";
    }
    if t.contains("مبيعات") || t.contains("بيع") || t.contains("إيرادات") || t.contains("خدمات") {
        return "بيع";
    }
    if t.contains("تحصيل") {
        return "تحصيل";
    }
    if t.contains("مشتريات") || t.contains("شراء") {
        return "شراء";
    }
    if t.contains("سداد") {
        return "سداد";
    }
    if t.contains("رواتب") {
        return "رواتب";
    }
    if t.contains("إيجار")
        || t.contains("كهرباء")
        || t.contains("اتصالات")
        || t.contains("مصاريف")
        || t.contains("مصروف")
    {
        return "مصروف";
    }
    if t.contains("إشعار") {
        return "إشعار";
    }
    if t.contains("زكاة") {
        return "استحقاق الزكاة";
    }
    "قيد"
}

/// هل البيان يحمل هوية مستند بالفعل؟ الشكل المرجعي: «<نوع> <رمز-سنة-تسلسل> — <طرف>».
fn already_professional(text: &str) -> bool {
    if !text.contains(" — ") {
        return false;
    }
    let head = text.split(" — ").next().unwrap_or("");
    let mut parts = head.split_whitespace();
    let _label = parts.next();
    match parts.next() {
        Some(code) => {
            let mut chunks = code.split('-');
            let a = chunks.next().unwrap_or("");
            let b = chunks.next().unwrap_or("");
            let c = chunks.next().unwrap_or("");
            !a.is_empty()
                && a.chars().all(|ch| ch.is_ascii_alphabetic())
                && b.len() == 4
                && b.chars().all(|ch| ch.is_ascii_digit())
                && !c.is_empty()
                && c.chars().all(|ch| ch.is_ascii_digit())
        }
        None => false,
    }
}

/// «مبيعات نقدية — الشهر 3» ← «بيع JE-2026-0042 — مبيعات نقدية · الشهر 3»
fn rebuild(description: &str, entry_id: i64, year: &str) -> String {
    let body = narrative::normalize(description).replace(" — ", " · ");
    narrative::compose(
        label_for(&body),
        &format!("JE-{year}-{entry_id:04}"),
        &body,
    )
}

pub async fn normalize_all(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(input): Json<NormalizeInput>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, input.company_id)?;
        scope.require_admin()?;
        // القيود المرحّلة غير قابلة للتعديل بحكم تصميمنا (حارس على مستوى القاعدة).
        // لذلك يُحوَّل بيان المسودات فقط، ويُبلَّغ عن المرحّلة كـ«مقفلة» بصراحة.
        let rows: Vec<(i64, String, String, i64)> = {
            let mut stmt = tx.prepare(
                "SELECT id, COALESCE(description,''), substr(COALESCE(entry_date,''),1,4),
                        COALESCE(finalized,0)
                 FROM journal_entries WHERE company_id=?1 ORDER BY id",
            )?;
            let mapped = stmt.query_map([scope.company_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?;
            mapped.collect::<Result<Vec<_>, _>>()?
        };
        let mut scanned = 0_usize;
        let mut updated = 0_usize;
        let mut skipped = 0_usize;
        let mut locked = 0_usize;
        let mut sample: Vec<serde_json::Value> = Vec::new();
        for (id, description, year, finalized) in rows {
            scanned += 1;
            if description.trim().is_empty() || already_professional(&description) {
                skipped += 1;
                continue;
            }
            if finalized == 1 {
                locked += 1;
                continue;
            }
            let year = if year.len() == 4 { year } else { "0000".to_string() };
            let fresh = rebuild(&description, id, &year);
            if fresh == description {
                skipped += 1;
                continue;
            }
            if !input.dry_run {
                // الرأس والمسودة معًا كي لا يختلف البيان بين الشاشتين.
                tx.execute(
                    "UPDATE journal_entries SET description=?2 WHERE id=?1",
                    params![id, fresh],
                )?;
                tx.execute(
                    "UPDATE drafts SET description=?2
                     WHERE id=(SELECT draft_id FROM journal_entries WHERE id=?1)",
                    params![id, fresh],
                )?;
            }
            updated += 1;
            if sample.len() < 8 {
                sample.push(serde_json::json!({"id": id, "before": description, "after": fresh}));
            }
        }
        if !input.dry_run && updated > 0 {
            audit::record(
                &tx,
                audit::Entry {
                    company_id: Some(scope.company_id),
                    actor_id: Some(actor.id),
                    action: "journal.normalize",
                    entity: "journal_entries",
                    entity_id: None,
                    payload: Some(serde_json::json!({"updated": updated, "scanned": scanned})),
                },
            )?;
        }
        tx.commit()?;
        Ok(serde_json::json!({
            "scanned": scanned,
            "updated": updated,
            "skipped": skipped,
            "locked_posted": locked,
            "dry_run": input.dry_run,
            "sample": sample
        }))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}
