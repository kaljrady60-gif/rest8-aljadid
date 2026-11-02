//! الكيانات المساندة للمبيعات: مجموعات، مندوبون، محصّلون، بطاقات، أنواع، أسباب إرجاع، أجهزة، عملاء نقد.
use super::common;
use crate::AppState;
use crate::db_open;
use crate::finance::audit;
use crate::finance::scope::{FinanceError, Scope, error_response};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, params};
use serde_json::Value;

#[derive(serde::Deserialize)]
pub struct ListQuery {
    pub company_id: i64,
}

fn text(body: &Value, key: &str) -> String {
    body.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn int(body: &Value, key: &str) -> i64 {
    body.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn money_field(body: &Value, key: &str) -> Result<i64, FinanceError> {
    match body.get(key).and_then(Value::as_str) {
        Some(raw) if !raw.trim().is_empty() => common::parse_money(raw, key),
        _ => Ok(int(body, key)),
    }
}

/// كل الكيانات في نداء واحد: شاشة التهيئة تُبنى بلا عشرات الطلبات.
pub async fn list_all(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ListQuery>,
) -> Response {
    let result = (|| -> Result<Value, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let cid = scope.company_id;
        let mut out = serde_json::Map::new();
        out.insert("groups".into(), rows(&db, cid,
            "SELECT id,code,name,discount_bp,status FROM sales_groups WHERE company_id=?1 ORDER BY name",
            |r| Ok(serde_json::json!({"id":r.get::<_,i64>(0)?,"code":r.get::<_,String>(1)?,"name":r.get::<_,String>(2)?,"discount_bp":r.get::<_,i64>(3)?,"status":r.get::<_,String>(4)?})))?);
        out.insert("reps".into(), rows(&db, cid,
            "SELECT id,number,name,COALESCE(phone,''),commission_bp,status FROM sales_reps WHERE company_id=?1 ORDER BY number",
            |r| Ok(serde_json::json!({"id":r.get::<_,i64>(0)?,"number":r.get::<_,String>(1)?,"name":r.get::<_,String>(2)?,"phone":r.get::<_,String>(3)?,"commission_bp":r.get::<_,i64>(4)?,"status":r.get::<_,String>(5)?})))?);
        out.insert("collectors".into(), rows(&db, cid,
            "SELECT id,number,name,COALESCE(phone,''),commission_bp,status FROM sales_collectors WHERE company_id=?1 ORDER BY number",
            |r| Ok(serde_json::json!({"id":r.get::<_,i64>(0)?,"number":r.get::<_,String>(1)?,"name":r.get::<_,String>(2)?,"phone":r.get::<_,String>(3)?,"commission_bp":r.get::<_,i64>(4)?,"status":r.get::<_,String>(5)?})))?);
        out.insert("cards".into(), rows(&db, cid,
            "SELECT id,name,network_limit_minor,off_network_limit_minor,commission_bp,status FROM sales_cards WHERE company_id=?1 ORDER BY name",
            |r| Ok(serde_json::json!({"id":r.get::<_,i64>(0)?,"name":r.get::<_,String>(1)?,"network_limit":common::money(r.get::<_,i64>(2)?),"off_network_limit":common::money(r.get::<_,i64>(3)?),"commission_bp":r.get::<_,i64>(4)?,"status":r.get::<_,String>(5)?})))?);
        out.insert("types".into(), rows(&db, cid,
            "SELECT id,code,name,kind,COALESCE(category,''),status FROM sales_types WHERE company_id=?1 ORDER BY code",
            |r| Ok(serde_json::json!({"id":r.get::<_,i64>(0)?,"code":r.get::<_,String>(1)?,"name":r.get::<_,String>(2)?,"kind":r.get::<_,String>(3)?,"category":r.get::<_,String>(4)?,"status":r.get::<_,String>(5)?})))?);
        out.insert("reasons".into(), rows(&db, cid,
            "SELECT id,code,name,COALESCE(category,''),status FROM sales_reasons WHERE company_id=?1 ORDER BY code",
            |r| Ok(serde_json::json!({"id":r.get::<_,i64>(0)?,"code":r.get::<_,String>(1)?,"name":r.get::<_,String>(2)?,"category":r.get::<_,String>(3)?,"status":r.get::<_,String>(4)?})))?);
        out.insert("machines".into(), rows(&db, cid,
            "SELECT id,number,name,COALESCE(cashbox_id,0),status FROM sales_pos_machines WHERE company_id=?1 ORDER BY number",
            |r| Ok(serde_json::json!({"id":r.get::<_,i64>(0)?,"number":r.get::<_,String>(1)?,"name":r.get::<_,String>(2)?,"cashbox_id":r.get::<_,i64>(3)?,"status":r.get::<_,String>(4)?})))?);
        out.insert("cash".into(), rows(&db, cid,
            "SELECT id,name,COALESCE(phone,''),balance_minor,status FROM sales_cash_customers WHERE company_id=?1 ORDER BY name",
            |r| Ok(serde_json::json!({"id":r.get::<_,i64>(0)?,"name":r.get::<_,String>(1)?,"phone":r.get::<_,String>(2)?,"balance":common::money(r.get::<_,i64>(3)?),"status":r.get::<_,String>(4)?})))?);
        Ok(Value::Object(out))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

fn rows<F>(db: &rusqlite::Connection, cid: i64, sql: &str, map: F) -> Result<Value, FinanceError>
where
    F: Fn(&rusqlite::Row<'_>) -> rusqlite::Result<Value>,
{
    let mut stmt = db.prepare(sql)?;
    let items = stmt
        .query_map([cid], map)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Array(items))
}

/// جدول وترتيب الحقول المسموح بها لكل نوع — قائمة بيضاء صريحة.
fn table_of(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "groups" => "sales_groups",
        "reps" => "sales_reps",
        "collectors" => "sales_collectors",
        "cards" => "sales_cards",
        "types" => "sales_types",
        "reasons" => "sales_reasons",
        "machines" => "sales_pos_machines",
        "cash" => "sales_cash_customers",
        _ => return None,
    })
}

pub async fn save(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Path(kind): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let result = (|| -> Result<i64, FinanceError> {
        let table = table_of(&kind).ok_or(FinanceError::NotFound("نوع الكيان"))?;
        let company_id = int(&body, "company_id");
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, company_id)?;
        if !scope.is_admin && !crate::identity::is_current_admin(&tx, &actor)? {
            return Err(FinanceError::Forbidden);
        }
        let id = int(&body, "id");
        let cid = scope.company_id;
        let saved = match kind.as_str() {
            "groups" => upsert(&tx, table, id, cid,
                &[("code", text(&body, "code")), ("name", text(&body, "name"))],
                &[("discount_bp", int(&body, "discount_bp"))])?,
            "reps" | "collectors" => upsert(&tx, table, id, cid,
                &[("number", text(&body, "number")), ("name", text(&body, "name")), ("phone", text(&body, "phone"))],
                &[("commission_bp", int(&body, "commission_bp"))])?,
            "cards" => upsert(&tx, table, id, cid,
                &[("name", text(&body, "name"))],
                &[("network_limit_minor", money_field(&body, "network_limit")?),
                  ("off_network_limit_minor", money_field(&body, "off_network_limit")?),
                  ("commission_bp", int(&body, "commission_bp"))])?,
            "types" => upsert(&tx, table, id, cid,
                &[("code", text(&body, "code")), ("name", text(&body, "name")),
                  ("kind", if text(&body, "kind").is_empty() { "cash".into() } else { text(&body, "kind") }),
                  ("category", text(&body, "category"))], &[])?,
            "reasons" => upsert(&tx, table, id, cid,
                &[("code", text(&body, "code")), ("name", text(&body, "name")),
                  ("category", text(&body, "category"))], &[])?,
            "machines" => upsert(&tx, table, id, cid,
                &[("number", text(&body, "number")), ("name", text(&body, "name"))],
                &[("cashbox_id", int(&body, "cashbox_id"))])?,
            "cash" => upsert(&tx, table, id, cid,
                &[("name", text(&body, "name")), ("phone", text(&body, "phone"))], &[])?,
            _ => return Err(FinanceError::NotFound("نوع الكيان")),
        };
        audit::record(&tx, audit::Entry {
            company_id: Some(cid), actor_id: Some(actor.id),
            action: "sales.entity.save", entity: "sales", entity_id: Some(saved.to_string()),
            payload: Some(serde_json::json!({"kind": kind})),
        })?;
        tx.commit()?;
        Ok(saved)
    })();
    match result {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response(),
        Err(e) => error_response(e),
    }
}

fn upsert(
    tx: &rusqlite::Connection,
    table: &str,
    id: i64,
    cid: i64,
    texts: &[(&str, String)],
    ints: &[(&str, i64)],
) -> Result<i64, FinanceError> {
    if id > 0 {
        let mut assignments: Vec<String> = Vec::new();
        let mut values: Vec<rusqlite::types::Value> = Vec::new();
        for (name, value) in texts {
            assignments.push(format!("{name}=?"));
            values.push(rusqlite::types::Value::Text(value.clone()));
        }
        for (name, value) in ints {
            assignments.push(format!("{name}=?"));
            values.push(rusqlite::types::Value::Integer(*value));
        }
        if assignments.is_empty() {
            return Err(FinanceError::Invalid("لا حقول للتعديل.".into()));
        }
        values.push(rusqlite::types::Value::Integer(id));
        values.push(rusqlite::types::Value::Integer(cid));
        let sql = format!(
            "UPDATE {table} SET {} WHERE id=? AND company_id=?",
            assignments.join(",")
        );
        let changed = tx.execute(&sql, rusqlite::params_from_iter(values))?;
        if changed == 0 {
            return Err(FinanceError::NotFound("السجل"));
        }
        return Ok(id);
    }
    let mut columns = vec!["company_id".to_string()];
    let mut marks: Vec<String> = vec!["?".into()];
    let mut values: Vec<rusqlite::types::Value> =
        vec![rusqlite::types::Value::Integer(cid)];
    for (name, value) in texts {
        columns.push((*name).into());
        marks.push("?".into());
        values.push(rusqlite::types::Value::Text(value.clone()));
    }
    for (name, value) in ints {
        columns.push((*name).into());
        marks.push("?".into());
        values.push(rusqlite::types::Value::Integer(*value));
    }
    let sql = format!(
        "INSERT INTO {table}({}) VALUES({})",
        columns.join(","),
        marks.join(",")
    );
    tx.execute(&sql, rusqlite::params_from_iter(values))?;
    Ok(tx.last_insert_rowid())
}

pub async fn archive(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Path((kind, id)): Path<(String, i64)>,
    Json(body): Json<Value>,
) -> Response {
    let result = (|| -> Result<(), FinanceError> {
        let table = table_of(&kind).ok_or(FinanceError::NotFound("نوع الكيان"))?;
        let mut db = db_open(&state.db)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let scope = Scope::resolve(&state, &tx, &actor, int(&body, "company_id"))?;
        let status: Option<String> = tx
            .query_row(
                &format!("SELECT status FROM {table} WHERE id=?1 AND company_id=?2"),
                params![id, scope.company_id],
                |r| r.get(0),
            )
            .optional()?;
        let next = match status.as_deref() {
            Some("active") => "archived",
            Some("archived") => "active",
            _ => return Err(FinanceError::NotFound("السجل")),
        };
        tx.execute(
            &format!("UPDATE {table} SET status=?1 WHERE id=?2"),
            params![next, id],
        )?;
        tx.commit()?;
        Ok(())
    })();
    match result {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => error_response(e),
    }
}
