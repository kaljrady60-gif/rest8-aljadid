//! سجل تدقيق إلحاقي (append-only) لكل عملية مالية.
//!
//! الجدول محمي بمُشغِّلات تمنع التعديل والحذف، فلا يمكن طمس أثر.
use super::scope::FinanceError;
use rusqlite::{Connection, params};

pub struct Entry<'a> {
    pub company_id: Option<i64>,
    pub actor_id: Option<i64>,
    pub action: &'a str,
    pub entity: &'a str,
    pub entity_id: Option<String>,
    pub payload: Option<serde_json::Value>,
}

pub fn record(db: &Connection, e: Entry<'_>) -> Result<(), FinanceError> {
    let payload = e.payload.map(|v| v.to_string());
    db.execute(
        "INSERT INTO finance_audit_log(company_id,actor_id,action,entity,entity_id,payload)
         VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            e.company_id,
            e.actor_id,
            e.action,
            e.entity,
            e.entity_id,
            payload
        ],
    )?;
    Ok(())
}

/// قراءة السجل لشركة، الأحدث أولًا.
pub fn tail(
    db: &Connection,
    company_id: i64,
    limit: i64,
) -> Result<Vec<serde_json::Value>, FinanceError> {
    let limit = limit.clamp(1, 200);
    let mut stmt = db.prepare(
        "SELECT id,actor_id,action,entity,entity_id,payload,occurred_at
         FROM finance_audit_log WHERE company_id=?1 ORDER BY id DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![company_id, limit], |r| {
        Ok(serde_json::json!({
            "id": r.get::<_, i64>(0)?,
            "actor_id": r.get::<_, Option<i64>>(1)?,
            "action": r.get::<_, String>(2)?,
            "entity": r.get::<_, String>(3)?,
            "entity_id": r.get::<_, Option<String>>(4)?,
            "payload": r.get::<_, Option<String>>(5)?,
            "at": r.get::<_, String>(6)?,
        }))
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}
