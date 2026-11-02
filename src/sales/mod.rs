//! قسم المبيعات: وحدة مستقلة فوق نطاق المالية، تُرحّل عبر محرّك القيود نفسه.
pub mod commissions;
pub mod customers;
pub mod common;
pub mod docs;
pub mod entities;
pub mod invoices;
pub mod pos;
pub mod reports;
pub mod returns;
pub mod schema;
pub mod seed;
pub mod setup;

pub use schema::init_schema;

use crate::AppState;
use axum::Router;
use axum::routing::{get, post};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/sales/setup", get(setup::get))
        .route(
            "/api/sales/invoices",
            get(invoices::list).post(invoices::create),
        )
        .route("/api/sales/invoices/{id}", get(invoices::detail))
        .route("/api/sales/invoices/{id}/post", post(invoices::post))
        .route("/api/sales/invoices/{id}/cancel", post(invoices::cancel))
        .route("/api/sales/returns", post(returns::create))
        .route("/api/sales/customers", post(customers::create))
        .route("/api/sales/docs", get(docs::list).post(docs::create))
        .route("/api/sales/docs/{kind}/{id}/convert", post(docs::convert))
        .route("/api/sales/entities", get(entities::list_all))
        .route("/api/sales/entities/{kind}", post(entities::save))
        .route("/api/sales/entities/{kind}/{id}/archive", post(entities::archive))
        .route("/api/sales/commissions", get(commissions::list))
        .route("/api/sales/commissions/plans", post(commissions::save_plan))
        .route("/api/sales/commissions/rules", post(commissions::save_rule))
        .route("/api/sales/commissions/calculate", post(commissions::calculate))
        .route("/api/sales/commissions/accrue", post(commissions::accrue))
        .route("/api/sales/reports/aging", get(reports::aging))
        .route("/api/sales/reports/summary", get(reports::summary))
        .route("/api/sales/pos/shift", get(reports::shift))
        .route(
            "/api/sales/pos/custody",
            get(pos::custody_list).post(pos::custody_create),
        )
        .route("/api/sales/seed", post(seed::run))
}
