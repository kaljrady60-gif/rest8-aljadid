//! سياق المالية للواجهة: الشركات المتاحة للمستخدم + الشاشات + التقارير.
//! نقطة واحدة تستهلكها الواجهة فلا تُكرَّر معرفة الشركة في كل سكربت.
use super::scope::{FinanceError, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Serialize)]
struct Company {
    id: i64,
    code: String,
    name: String,
}

pub async fn get(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let db = db_open(&state.db)?;
        let mut companies: Vec<Company> = Vec::new();
        if state.preview_no_login {
            let mut stmt = db.prepare("SELECT id,code,name FROM finance_companies ORDER BY id")?;
            let rows = stmt.query_map([], |r| {
                Ok(Company {
                    id: r.get(0)?,
                    code: r.get(1)?,
                    name: r.get(2)?,
                })
            })?;
            companies.extend(rows.collect::<Result<Vec<_>, _>>()?);
        } else {
            // الشركات التي يملك المستخدم عليها إسنادًا صريحًا فقط.
            let mut stmt = db.prepare(
                "SELECT DISTINCT c.id,c.code,c.name FROM finance_companies c
                 WHERE c.id IN (
                    SELECT company_id FROM finance_company_admins WHERE user_id=?1
                    UNION SELECT company_id FROM finance_branch_grants WHERE user_id=?1
                 ) ORDER BY c.id",
            )?;
            let rows = stmt.query_map([actor.id], |r| {
                Ok(Company {
                    id: r.get(0)?,
                    code: r.get(1)?,
                    name: r.get(2)?,
                })
            })?;
            companies.extend(rows.collect::<Result<Vec<_>, _>>()?);
        }
        let default_company = companies.first().map(|c| c.id);
        Ok(serde_json::json!({
            "companies": companies,
            "default_company_id": default_company,
            "preview": state.preview_no_login,
            "posting": state.experimental_posting,
            "screens": crate::generated::contracts::SCREENS.iter().map(|s| serde_json::json!({
                "id": s.id, "title": s.title, "api": s.api
            })).collect::<Vec<_>>(),
            "reports": crate::generated::contracts::REPORTS.iter().map(|r| serde_json::json!({
                "id": r.id, "title": r.title, "kind": r.kind
            })).collect::<Vec<_>>(),
            "currencies": crate::generated::contracts::CURRENCIES.iter().map(|c| serde_json::json!({
                "code": c.code, "label": c.label, "decimals": c.decimals
            })).collect::<Vec<_>>(),
        }))
    })();
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => error_response(e),
    }
}
