//! تصدير التقارير إلى CSV (UTF-8 مع BOM ليفتح صحيحًا في Excel العربي)
//! وطباعة نصية منظّمة. المصدر نفسه تقارير `statements` المحسوبة من القيود المرحّلة.
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::extract::{Query, State};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct ExportQuery {
    company_id: i64,
    #[serde(default = "default_kind")]
    kind: String,
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    branch_id: Option<i64>,
}

fn default_kind() -> String {
    "trial".into()
}

fn cell(value: &str) -> String {
    let cleaned = value.replace('"', "\"\"");
    if cleaned.contains(',') || cleaned.contains('"') || cleaned.contains('\n') {
        format!("\"{cleaned}\"")
    } else {
        cleaned
    }
}

fn csv(rows: &[Vec<String>]) -> String {
    let mut out = String::from("\u{feff}");
    for row in rows {
        out.push_str(&row.iter().map(|c| cell(c)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    out
}

fn statement_rows(kind: &str, body: &serde_json::Value) -> Result<Vec<Vec<String>>, FinanceError> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    match kind {
        "trial" => {
            rows.push(vec![
                "الرمز".into(),
                "الحساب".into(),
                "مدين".into(),
                "دائن".into(),
                "الرصيد".into(),
            ]);
            for account in body["accounts"].as_array().cloned().unwrap_or_default() {
                rows.push(vec![
                    account["code"].as_str().unwrap_or_default().into(),
                    account["name"].as_str().unwrap_or_default().into(),
                    account["debit"].as_str().unwrap_or_default().into(),
                    account["credit"].as_str().unwrap_or_default().into(),
                    account["balance"].as_str().unwrap_or_default().into(),
                ]);
            }
            let totals = &body["totals"];
            rows.push(vec![
                "المجموع".into(),
                String::new(),
                totals["debit"].as_str().unwrap_or("0").into(),
                totals["credit"].as_str().unwrap_or("0").into(),
                String::new(),
            ]);
        }
        "income" | "balance" => {
            rows.push(vec![
                "التصنيف".into(),
                "الرمز".into(),
                "الحساب".into(),
                "الرصيد".into(),
            ]);
            for section in body["sections"].as_array().cloned().unwrap_or_default() {
                let category = section["category"].as_str().unwrap_or_default().to_string();
                for account in section["accounts"].as_array().cloned().unwrap_or_default() {
                    rows.push(vec![
                        category.clone(),
                        account["code"].as_str().unwrap_or_default().into(),
                        account["name"].as_str().unwrap_or_default().into(),
                        account["balance"].as_str().unwrap_or_default().into(),
                    ]);
                }
                rows.push(vec![
                    category,
                    String::new(),
                    "مجموع القسم".into(),
                    section["total"].as_str().unwrap_or("0").into(),
                ]);
            }
            let key = if kind == "income" {
                "net_profit"
            } else {
                "assets_total"
            };
            rows.push(vec![
                "الإجمالي".into(),
                String::new(),
                String::new(),
                body[key].as_str().unwrap_or("0").into(),
            ]);
        }
        "cashflow" => {
            rows.push(vec!["الرمز".into(), "الحساب".into(), "الصافي".into()]);
            for account in body["accounts"].as_array().cloned().unwrap_or_default() {
                rows.push(vec![
                    account["code"].as_str().unwrap_or_default().into(),
                    account["name"].as_str().unwrap_or_default().into(),
                    account["net"].as_str().unwrap_or_default().into(),
                ]);
            }
        }
        "general" => {
            rows.push(vec![
                "#".into(),
                "التاريخ".into(),
                "البيان".into(),
                "العملة".into(),
                "المبلغ".into(),
            ]);
            for entry in body["entries"].as_array().cloned().unwrap_or_default() {
                rows.push(vec![
                    entry["id"].to_string(),
                    entry["date"].as_str().unwrap_or_default().into(),
                    entry["description"].as_str().unwrap_or_default().into(),
                    entry["currency"].as_str().unwrap_or_default().into(),
                    entry["total"].as_str().unwrap_or_default().into(),
                ]);
            }
        }
        other => {
            return Err(FinanceError::Invalid(format!(
                "تصدير غير مدعوم للتقرير {other}."
            )));
        }
    }
    Ok(rows)
}

pub async fn export(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ExportQuery>,
) -> Response {
    let result = (|| -> Result<String, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let parsed = super::statements::StatementQuery {
            company_id: scope.company_id,
            report: q.kind.clone(),
            currency: q.currency.clone(),
            from: q.from.clone(),
            to: q.to.clone(),
            branch_id: q.branch_id,
        };
        let body = super::statements::compute(&state, &actor, parsed)?;
        let rows = statement_rows(&q.kind, &body)?;
        Ok(csv(&rows))
    })();
    match result {
        Ok(text) => {
            let mut response = text.into_response();
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/csv; charset=utf-8"),
            );
            response.headers_mut().insert(
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment; filename=\"report.csv\""),
            );
            response
        }
        Err(e) => error_response(e),
    }
}
