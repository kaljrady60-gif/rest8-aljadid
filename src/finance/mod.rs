//! نطاق المالية الكامل: كل مسار هنا محمي بالجلسة + عزل الشركة.
//! التوجيه فقط في هذا الملف — التنفيذ في وحدات متخصصة تحت 300 سطر.
pub mod adjust;
pub mod approval;
pub mod assets;
pub mod audit;
pub mod bankrec;
pub mod bootstrap;
pub mod branches;
pub mod checks;
pub mod closing;
pub mod context;
pub mod demo;
pub mod dimensions;
pub mod exports;
pub mod extras;
pub mod ai_settings;
pub mod invoice_ai;
pub mod extras2;
pub mod funds;
pub mod fx;
pub mod gl;
pub mod journals;
pub mod ledger;
pub mod money;
pub mod narrative;
pub mod narrative_fix;
pub mod numbering;
pub mod opening;
pub mod org;
pub mod parties;
pub mod payroll;
pub mod print;
pub mod requests;
pub mod schema;
pub mod schema2;
pub mod schema3;
pub mod schema4;
pub mod schema5;
pub mod schema6;
pub mod schema7;
pub mod scope;
pub mod seed;
pub mod seed2;
pub mod settings5;
pub mod statements;
pub mod taxlink;
pub mod taxes;

use crate::AppState;
use axum::Router;
use axum::routing::{get, post};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/finance/journals",
            get(journals::list).post(journals::create),
        )
        .route("/api/finance/journals/{id}", get(journals::detail))
        .route("/api/finance/journals/{id}/post", post(journals::post))
        .route("/api/finance/journals/{id}/submit", post(approval::submit))
        .route(
            "/api/finance/journals/{id}/approve",
            post(approval::approve),
        )
        .route("/api/finance/journals/{id}/reject", post(approval::reject))
        .route("/api/finance/approval", get(approval::list_flow))
        .route(
            "/api/finance/approval/policy",
            get(approval::get_policy).post(approval::set_policy),
        )
        .route(
            "/api/finance/parties",
            get(parties::list).post(parties::create),
        )
        .route(
            "/api/finance/checks",
            get(checks::list).post(checks::create),
        )
        .route("/api/finance/checks/{id}/status", post(checks::move_status))
        .route(
            "/api/finance/closing",
            get(closing::list).post(closing::close),
        )
        .route("/api/finance/export", get(exports::export))
        .route(
            "/api/finance/journal/normalize",
            post(narrative_fix::normalize_all),
        )
        .route("/api/finance/ledger", get(ledger::entries))
        .route("/api/finance/trial", get(statements::get))
        .route("/api/finance/statements", get(statements::get))
        .route(
            "/api/finance/opening",
            get(opening::list).post(opening::save),
        )
        .route("/api/finance/funds", get(funds::list).post(funds::create))
        .route("/api/finance/funds/archive", post(funds::archive))
        .route("/api/finance/fx", get(fx::list).post(fx::create))
        .route("/api/finance/fx/convert", get(fx::convert))
        .route(
            "/api/finance/dimensions",
            get(dimensions::list).post(dimensions::create),
        )
        .route("/api/finance/dimensions/archive", post(dimensions::archive))
        .route(
            "/api/finance/bankrec",
            get(bankrec::list).post(bankrec::create),
        )
        .route("/api/finance/bankrec/{id}/lines", post(bankrec::add_line))
        .route(
            "/api/finance/bankrec/{id}/lines/{line_id}",
            post(bankrec::clear_line),
        )
        .route(
            "/api/finance/assets/categories",
            get(assets::categories).post(assets::create_category),
        )
        .route("/api/finance/assets/depreciate", post(assets::depreciate))
        .route("/api/finance/assets/{id}/dispose", post(assets::dispose))
        .route(
            "/api/finance/assets",
            get(assets::list).post(assets::create),
        )
        .route(
            "/api/finance/payroll/employees",
            get(payroll::employees).post(payroll::create_employee),
        )
        .route(
            "/api/finance/payroll/runs",
            get(payroll::runs).post(payroll::create_run),
        )
        .route(
            "/api/finance/payroll/runs/{id}/lines",
            get(payroll::run_lines),
        )
        .route(
            "/api/finance/payroll/runs/{id}/post",
            post(payroll::post_run),
        )
        .route("/api/finance/payroll/runs/{id}/pay", post(payroll::pay_run))
        .route(
            "/api/finance/branches",
            get(branches::list).post(branches::create),
        )
        .route("/api/finance/print", get(print::sheet))
        .route("/api/finance/audit", get(audit_tail))
        .route("/api/finance/context", get(context::get))
        // التسويات ومراجعة الوثائق
        .route(
            "/api/finance/adjustments",
            get(adjust::adjustments).post(adjust::create_adjustment),
        )
        .route(
            "/api/finance/adjustments/decide",
            post(adjust::decide_adjustment),
        )
        .route(
            "/api/finance/doc-reviews",
            get(adjust::reviews).post(adjust::create_review),
        )
        .route(
            "/api/finance/doc-reviews/decide",
            post(adjust::decide_review),
        )
        // المخططات والعمولات
        .route(
            "/api/finance/plans",
            get(settings5::plans).post(settings5::create_plan),
        )
        .route("/api/finance/plans/quote", get(settings5::quote))
        // بيانات الدول
        .route(
            "/api/finance/countries",
            get(settings5::countries).post(settings5::create_country),
        )
        // الحسابات الوسيطة
        .route(
            "/api/finance/intermediate",
            get(settings5::intermediate).post(settings5::create_intermediate),
        )
        // الأرشفة المشتركة
        .route("/api/finance/archive/{table}", post(settings5::archive))
        // متغيرات الأستاذ
        .route(
            "/api/finance/ledger-settings",
            get(settings5::get_settings).post(settings5::set_setting),
        )
        // الطلبات
        .route(
            "/api/finance/requests",
            get(requests::list).post(requests::create),
        )
        .route("/api/finance/requests/submit", post(requests::submit))
        .route("/api/finance/requests/decide", post(requests::decide))
        // قراءة الفواتير المصورة
        .route("/api/finance/scanner/parse", post(requests::scan))
        .route("/api/finance/scanner/image", post(requests::scan_image))
        .route(
            "/api/finance/scanner/status",
            get(ai_settings::get),
        )
        .route(
            "/api/finance/ai/settings",
            get(ai_settings::get).post(ai_settings::save),
        )
        .route("/api/finance/ai/models", get(ai_settings::models))
        .route("/api/finance/ai/probe", get(ai_settings::probe))
        // ربط الضرائب بالقيود والسندات
        .route(
            "/api/finance/tax-links",
            get(taxlink::links).post(taxlink::link),
        )
        .route("/api/finance/tax-links/unlink", post(taxlink::unlink))
        .route("/api/finance/tax-links/report", get(taxlink::report))
        .route("/api/finance/taxes", get(taxes::list).post(taxes::create))
        .route("/api/finance/taxes/archive", post(taxes::archive))
        .route("/api/finance/taxes/compute", get(taxes::compute))
        .route(
            "/api/finance/numbering",
            get(numbering::list).post(numbering::configure),
        )
        .route("/api/finance/numbering/next", post(numbering::next))
        .route("/api/finance/departments", get(org::list).post(org::create))
        .route("/api/finance/departments/archive", post(org::archive))
        // العملات ومجموعات الفروع
        .route(
            "/api/finance/currencies",
            get(extras::currencies).post(extras::create_currency),
        )
        .route(
            "/api/finance/branch-groups",
            get(extras::groups).post(extras::create_group),
        )
        // الإقرارات الضريبية
        .route(
            "/api/finance/tax-returns",
            get(extras::tax_returns).post(extras::create_return),
        )
        .route("/api/finance/tax-returns/preview", get(extras::preview_return))
        .route("/api/finance/tax-returns/submit", post(extras::submit_return))
        // الإقفال الموحّد
        .route("/api/finance/close-hub", get(extras::close_hub))
        // الإشعارات المدينة والدائنة
        .route(
            "/api/finance/notes",
            get(extras2::notes).post(extras2::create_note),
        )
        .route("/api/finance/notes/post", post(extras2::post_note))
        // الزكاة
        .route("/api/finance/zakat", get(extras2::zakat_cycles))
        .route("/api/finance/zakat/compute", get(extras2::compute_zakat))
        .route("/api/finance/zakat/run", post(extras2::run_zakat))
        // الموازنات
        .route(
            "/api/finance/budgets",
            get(extras2::budgets).post(extras2::create_budget),
        )
        .route("/api/finance/budgets/compare", get(extras2::compare_budget))
        .route("/api/finance/budgets/adjust", post(extras2::adjust_budget))
        .route("/api/finance/budgets/history", get(extras2::budget_history))
}

/// آخر أحداث سجل التدقيق للشركة.
async fn audit_tail(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    axum::extract::Query(q): axum::extract::Query<AuditQuery>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let outcome = (|| -> Result<Vec<serde_json::Value>, scope::FinanceError> {
        let db = crate::db_open(&state.db)?;
        let s = scope::Scope::resolve(&state, &db, &actor, q.company_id)?;
        audit::tail(&db, s.company_id, q.limit.unwrap_or(50))
    })();
    match outcome {
        Ok(rows) => axum::Json(rows).into_response(),
        Err(e) => scope::error_response(e),
    }
}

#[derive(serde::Deserialize)]
pub struct AuditQuery {
    company_id: i64,
    limit: Option<i64>,
}

/// تهيئة مخطط المالية — تُستدعى مرة عند بدء التشغيل بعد بقية المخططات.
pub fn init_schema(db: &rusqlite::Connection) -> rusqlite::Result<()> {
    schema::init(db)?;
    schema2::init(db)?;
    schema3::init(db)?;
    schema4::init(db)?;
    schema5::init(db)?;
    schema6::init(db)?;
    schema7::init(db)?;
    ai_settings::init(db)
}

#[cfg(test)]
mod tests;
