//! الطباعة وملف PDF: صفحة HTML مهيأة لمقاس A4 قابلة للطباعة أو الحفظ كـPDF
//! من المتصفح نفسه (بلا أي مكتبة مدفوعة أو خدمة خارجية).
//!
//! المصدر هو `statements::compute` — نفس محرك التقارير الذي يغذّي الشاشة والتصدير،
//! فلا تختلف الأرقام المطبوعة عن أرقام الشاشة أبدًا.
use super::scope::{FinanceError, Scope, error_response};
use super::statements::{StatementQuery, compute};
use crate::{AppState, db_open};
use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse, Response};
use rusqlite::OptionalExtension;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct PrintQuery {
    company_id: i64,
    #[serde(default)]
    report: Option<String>,
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    branch_id: Option<i64>,
}

const REPORTS: [(&str, &str); 5] = [
    ("trial", "ميزان المراجعة"),
    ("income", "قائمة الدخل"),
    ("balance", "الميزانية العمومية"),
    ("cashflow", "قائمة التدفق النقدي"),
    ("general", "دفتر اليومية العامة"),
];

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// تنسيق رقمي بمنزلتين للطباعة، عبر Decimal لا عبر أنواع الفاصلة العائمة.
fn num(value: &serde_json::Value) -> String {
    let raw = value.as_str().unwrap_or("0");
    match rust_decimal::Decimal::from_str_exact(raw.trim()) {
        Ok(parsed) => format!("{}", parsed.round_dp(2)),
        Err(_) => "0.00".into(),
    }
}

/// صفوف جدول مبسّط: (الرمز، الاسم، قيم إضافية).
fn rows_html(rows: &[Vec<String>], headers: &[&str]) -> String {
    let mut out = String::from("<table><thead><tr>");
    for header in headers {
        out.push_str(&format!("<th>{}</th>", escape(header)));
    }
    out.push_str("</tr></thead><tbody>");
    for row in rows {
        out.push_str("<tr>");
        for cell in row {
            out.push_str(&format!("<td>{cell}</td>"));
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table>");
    out
}

fn statement_rows(body: &serde_json::Value) -> (Vec<String>, Vec<Vec<String>>) {
    let headers: Vec<String>;
    let mut rows: Vec<Vec<String>> = Vec::new();
    match body["report"].as_str().unwrap_or("trial") {
        "income" | "balance" => {
            headers = ["البند", "الرصيد"].iter().map(|s| s.to_string()).collect();
            for section in body["sections"].as_array().cloned().unwrap_or_default() {
                let title = match section["category"].as_str().unwrap_or("") {
                    "revenue" => "الإيرادات",
                    "expense" => "المصروفات",
                    "asset" => "الأصول",
                    "liability" => "الالتزامات",
                    "equity" => "حقوق الملكية",
                    other => other,
                };
                rows.push(vec![
                    format!("<strong>{}</strong>", escape(title)),
                    String::new(),
                ]);
                for account in section["accounts"].as_array().cloned().unwrap_or_default() {
                    rows.push(vec![
                        escape(&format!(
                            "{} · {}",
                            account["code"].as_str().unwrap_or(""),
                            account["name"].as_str().unwrap_or("")
                        )),
                        num(&account["balance"]),
                    ]);
                }
                rows.push(vec![
                    format!("<em>مجموع {}</em>", escape(title)),
                    format!("<em>{}</em>", num(&section["total"])),
                ]);
            }
        }
        "cashflow" => {
            headers = ["الحساب", "الصافي"].iter().map(|s| s.to_string()).collect();
            for account in body["accounts"].as_array().cloned().unwrap_or_default() {
                rows.push(vec![
                    escape(&format!(
                        "{} · {}",
                        account["code"].as_str().unwrap_or(""),
                        account["name"].as_str().unwrap_or("")
                    )),
                    num(&account["net"]),
                ]);
            }
        }
        "general" => {
            headers = ["#", "التاريخ", "البيان", "المبلغ"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            for entry in body["entries"].as_array().cloned().unwrap_or_default() {
                rows.push(vec![
                    entry["id"].to_string(),
                    escape(entry["date"].as_str().unwrap_or("")),
                    escape(entry["description"].as_str().unwrap_or("")),
                    num(&entry["total"]),
                ]);
            }
        }
        _ => {
            headers = ["الرمز", "الحساب", "مدين", "دائن", "الرصيد"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            for account in body["accounts"].as_array().cloned().unwrap_or_default() {
                rows.push(vec![
                    escape(account["code"].as_str().unwrap_or("")),
                    escape(account["name"].as_str().unwrap_or("")),
                    num(&account["debit"]),
                    num(&account["credit"]),
                    num(&account["balance"]),
                ]);
            }
        }
    }
    (headers, rows)
}

fn summary_html(body: &serde_json::Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (key, label) in [
        ("net_profit", "صافي الربح"),
        ("assets_total", "إجمالي الأصول"),
        ("liabilities_equity_total", "الالتزامات وحقوق الملكية"),
        ("inflow", "التدفق الداخل"),
        ("outflow", "التدفق الخارج"),
        ("net", "الصافي النقدي"),
    ] {
        if body.get(key).is_some() {
            parts.push(format!(
                "<div class=\"kpi\"><span>{}</span><strong>{}</strong></div>",
                escape(label),
                num(&body[key])
            ));
        }
    }
    if let Some(totals) = body.get("totals") {
        parts.push(format!(
            "<div class=\"kpi\"><span>مجموع المدين</span><strong>{}</strong></div>",
            num(&totals["debit"])
        ));
        parts.push(format!(
            "<div class=\"kpi\"><span>مجموع الدائن</span><strong>{}</strong></div>",
            num(&totals["credit"])
        ));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("<section class=\"kpis\">{}</section>", parts.join(""))
    }
}

pub async fn sheet(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<PrintQuery>,
) -> Response {
    let result = (|| -> Result<String, FinanceError> {
        let report = q.report.clone().unwrap_or_else(|| "trial".into());
        let title = REPORTS
            .iter()
            .find(|(id, _)| *id == report)
            .map(|(_, title)| *title)
            .ok_or_else(|| FinanceError::Invalid("تقرير غير معروف.".into()))?;
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let company: String = db
            .query_row(
                "SELECT name FROM finance_companies WHERE id=?1",
                [scope.company_id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or_default();
        let branch_name: Option<String> = match q.branch_id {
            Some(id) => db
                .query_row(
                    "SELECT name FROM finance_branches WHERE id=?1 AND company_id=?2",
                    rusqlite::params![id, scope.company_id],
                    |r| r.get(0),
                )
                .optional()?,
            None => None,
        };
        let currency = q.currency.clone().unwrap_or_else(|| "YER".into());
        let body = compute(
            &state,
            &actor,
            StatementQuery {
                company_id: q.company_id,
                report: report.clone(),
                currency: Some(currency.clone()),
                from: q.from.clone(),
                to: q.to.clone(),
                branch_id: q.branch_id,
            },
        )?;
        let (headers, rows) = statement_rows(&body);
        let header_refs: Vec<&str> = headers.iter().map(|s| s.as_str()).collect();
        let table = rows_html(&rows, &header_refs);
        let period = match (q.from.as_deref(), q.to.as_deref()) {
            (Some(from), Some(to)) => format!("من {from} إلى {to}"),
            (Some(from), None) => format!("من {from}"),
            (None, Some(to)) => format!("حتى {to}"),
            (None, None) => "كل الفترات".to_string(),
        };
        let scope_label = branch_name
            .as_deref()
            .map(|name| format!("الفرع: {name}"))
            .unwrap_or_else(|| "كل الفروع".to_string());
        let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        Ok(format!(
            r#"<!doctype html><html lang="ar" dir="rtl"><head><meta charset="utf-8">
<title>{title} — {company}</title>
<style>
  @page {{ size: A4; margin: 14mm; }}
  * {{ box-sizing: border-box; }}
  body {{ font-family: "Segoe UI", "Noto Naskh Arabic", Tahoma, sans-serif; color: #0f172a; margin: 0; padding: 24px; }}
  header {{ display: flex; justify-content: space-between; align-items: flex-start; border-bottom: 2px solid #1d4ed8; padding-bottom: 12px; margin-bottom: 16px; }}
  header h1 {{ margin: 0 0 4px; font-size: 20px; }}
  header p {{ margin: 0; font-size: 12px; color: #475569; }}
  .meta {{ text-align: left; font-size: 12px; color: #475569; }}
  .kpis {{ display: flex; flex-wrap: wrap; gap: 10px; margin: 0 0 16px; }}
  .kpi {{ border: 1px solid #cbd5e1; border-radius: 8px; padding: 8px 14px; min-width: 150px; }}
  .kpi span {{ display: block; font-size: 11px; color: #64748b; }}
  .kpi strong {{ font-size: 15px; }}
  table {{ width: 100%; border-collapse: collapse; font-size: 12px; }}
  th, td {{ border-bottom: 1px solid #e2e8f0; padding: 6px 8px; text-align: right; }}
  th {{ background: #f1f5f9; }}
  footer {{ margin-top: 18px; display: flex; justify-content: space-between; font-size: 11px; color: #64748b; }}
  .sign {{ margin-top: 42px; display: flex; gap: 48px; font-size: 12px; }}
  .sign div {{ flex: 1; border-top: 1px solid #94a3b8; padding-top: 6px; text-align: center; }}
  .noprint {{ margin-bottom: 14px; }}
  .noprint button {{ font: inherit; padding: 8px 18px; border: 0; border-radius: 8px; background: #1d4ed8; color: #fff; cursor: pointer; }}
  @media screen {{ body {{ max-width: 860px; margin: 0 auto; background: #fff; box-shadow: 0 8px 30px rgba(15, 35, 80, .10); }} }}
  @media print {{ .noprint {{ display: none; }} body {{ padding: 0; max-width: none; }} }}
  thead {{ display: table-header-group; }}
  tr {{ page-break-inside: avoid; }}
</style></head><body>
<div class="noprint"><button onclick="window.print()">طباعة / حفظ PDF</button></div>
<header>
  <div><h1>{company}</h1><p>{title}</p></div>
  <div class="meta"><div>{period}</div><div>{scope_label} · العملة {currency}</div><div>أُنشئ في {stamp}</div></div>
</header>
{summary}
{table}
<div class="sign"><div>المحاسب</div><div>المدير المالي</div><div>المدقق</div></div>
<footer><span>المحاسب الذكي — مخرجات من محرك التقارير نفسه</span><span>صفحة A4</span></footer>
</body></html>"#,
            summary = summary_html(&body),
            table = table,
            currency = escape(&currency),
            period = escape(&period),
            scope_label = escape(&scope_label),
            stamp = escape(&stamp),
            company = escape(&company),
            title = escape(title),
        ))
    })();
    match result {
        Ok(html) => Html(html).into_response(),
        Err(e) => error_response(e),
    }
}
