//! بيانات مبيعات تجريبية واقعية (تُنشأ مرة واحدة لكل شركة).
use super::common;
use super::invoices::{self, LineIn, NewInvoice};
use crate::AppState;
use crate::db_open;
use crate::finance::scope::{FinanceError, Scope, error_response};
use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
pub struct SeedBody {
    #[serde(default)]
    pub company_id: Option<i64>,
}

fn insert_simple(
    tx: &rusqlite::Connection,
    cid: i64,
    sql: &str,
    a: &str,
    b: &str,
    c: i64,
) -> rusqlite::Result<()> {
    tx.execute(sql, params![cid, a, b, c])?;
    Ok(())
}

pub async fn run(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(body): Json<SeedBody>,
) -> Response {
    let result = (|| -> Result<Value, FinanceError> {
        let mut db = db_open(&state.db)?;
        let company_id = match body.company_id {
            Some(id) if id > 0 => id,
            _ => db
                .query_row("SELECT id FROM finance_companies ORDER BY id LIMIT 1", [], |r| {
                    r.get::<_, i64>(0)
                })
                .optional()?
                .ok_or(FinanceError::NotFound("الشركة"))?,
        };
        let scope = Scope::resolve(&state, &db, &actor, company_id)?;
        let entities_done: i64 = db.query_row(
            "SELECT COUNT(*) FROM sales_reps WHERE company_id=?1",
            [company_id],
            |r| r.get(0),
        )?;
        if entities_done > 0 {
            return Err(FinanceError::Conflict(
                "بيانات المبيعات الأساسية موجودة مسبقًا.".into(),
            ));
        }
        let invoices_done: i64 = db.query_row(
            "SELECT COUNT(*) FROM sales_invoices WHERE company_id=?1",
            [company_id],
            |r| r.get(0),
        )?;
        // العملاء: أطراف المالية بدور عميل أو كلاهما.
        let load_parties = |db: &rusqlite::Connection| -> Result<Vec<(i64, String, String)>, FinanceError> {
            let mut stmt = db.prepare(
                "SELECT p.id,p.name,COALESCE(a.currency,'YER')
                 FROM finance_parties p LEFT JOIN accounts a ON a.id=p.account_id
                 WHERE p.company_id=?1 AND p.role IN ('customer','both') ORDER BY p.id",
            )?;
            let rows = stmt
                .query_map([company_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        };
        let mut parties = load_parties(&db)?;
        if parties.is_empty() {
            // لا عملاء بعد: البذر يُنشئ ثلاثة عملاء حقيقيين في دليل المالية.
            let w = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            for name in ["مؤسسة النور التجارية", "شركة الأمل للمقاولات", "متجر الواحة"] {
                super::customers::create_customer(&w, company_id, name)?;
            }
            w.commit()?;
            parties = load_parties(&db)?;
        }
        if parties.is_empty() {
            return Err(FinanceError::Invalid(
                "تعذّر إنشاء عملاء للبذر.".into(),
            ));
        }
        {
            let w = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            for (code, name, bp) in [
                ("G-01", "تجار التجزئة", 300_i64),
                ("G-02", "الجملة", 700),
                ("G-03", "حكومي", 0),
            ] {
                insert_simple(&w, company_id,
                    "INSERT INTO sales_groups(company_id,code,name,discount_bp) VALUES(?1,?2,?3,?4)",
                    code, name, bp)?;
            }
            for (num, name, bp) in [
                ("R-01", "أحمد الشامي", 200_i64),
                ("R-02", "سارة العمري", 250),
                ("R-03", "خالد الجابري", 150),
            ] {
                insert_simple(&w, company_id,
                    "INSERT INTO sales_reps(company_id,number,name,commission_bp) VALUES(?1,?2,?3,?4)",
                    num, name, bp)?;
            }
            for (num, name, bp) in [("C-01", "محصّل الشمال", 100_i64), ("C-02", "محصّل الجنوب", 120)] {
                insert_simple(&w, company_id,
                    "INSERT INTO sales_collectors(company_id,number,name,commission_bp) VALUES(?1,?2,?3,?4)",
                    num, name, bp)?;
            }
            for (name, net, off, bp) in [
                ("فيزا", 5_000_00_i64, 2_000_00_i64, 150_i64),
                ("ماستركارد", 4_000_00, 1_500_00, 170),
                ("محلية", 1_000_00, 500_00, 100),
            ] {
                w.execute(
                    "INSERT INTO sales_cards(company_id,name,network_limit_minor,off_network_limit_minor,commission_bp)
                     VALUES(?1,?2,?3,?4,?5)",
                    params![company_id, name, net, off, bp],
                )?;
            }
            for (code, name, kind) in [
                ("T-CASH", "بيع نقدي", "cash"),
                ("T-CREDIT", "بيع آجل", "credit"),
                ("T-POS", "بيع نقاط بيع", "pos"),
            ] {
                w.execute(
                    "INSERT INTO sales_types(company_id,code,name,kind) VALUES(?1,?2,?3,?4)",
                    params![company_id, code, name, kind],
                )?;
            }
            for (code, name, cat) in [
                ("RS-01", "عيب صناعة", "جودة"),
                ("RS-02", "خطأ في الطلب", "إداري"),
                ("RS-03", "تأخر تسليم", "تشغيلي"),
            ] {
                w.execute(
                    "INSERT INTO sales_reasons(company_id,code,name,category) VALUES(?1,?2,?3,?4)",
                    params![company_id, code, name, cat],
                )?;
            }
            for (num, name) in [("POS-01", "كاشير المعرض"), ("POS-02", "كاشير المستودع")] {
                w.execute(
                    "INSERT INTO sales_pos_machines(company_id,number,name) VALUES(?1,?2,?3)",
                    params![company_id, num, name],
                )?;
            }
            for name in ["عميل نقدي عام", "عميل نقدي مناسبات"] {
                w.execute(
                    "INSERT INTO sales_cash_customers(company_id,name) VALUES(?1,?2)",
                    params![company_id, name],
                )?;
            }
            w.execute(
                "INSERT INTO sales_commission_plans(company_id,name,percent_bp,basis,active)
                 VALUES(?1,'خطة المندوبين الأساسية',200,'sale',1)",
                [company_id],
            )?;
            w.execute(
                "INSERT INTO sales_commission_rules(company_id,name,basis,applies_to,rate_bp,min_amount_minor,active)
                 VALUES(?1,'عمولة بيع المندوب','sale','rep',200,10000,1)",
                [company_id],
            )?;
            w.commit()?;
        }
        let rep_ids: Vec<i64> = {
            let mut stmt = db.prepare("SELECT id FROM sales_reps WHERE company_id=?1 ORDER BY id")?;
            stmt.query_map([company_id], |r| r.get(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        let machine_ids: Vec<i64> = {
            let mut stmt =
                db.prepare("SELECT id FROM sales_pos_machines WHERE company_id=?1 ORDER BY id")?;
            stmt.query_map([company_id], |r| r.get(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let items = [
            ("جهاز حاسب مكتبي", "2", "450000"),
            ("طابعة ليزر", "1", "180000"),
            ("شاشة 27 بوصة", "3", "95000"),
            ("كرسي مكتبي", "4", "65000"),
            ("حبر طابعة", "10", "12000"),
        ];
        let mut posted = 0_i64;
        for (index, (party_id, party_name, currency)) in parties.iter().enumerate().take(if invoices_done > 0 { 0 } else { parties.len() }) {
            let line_count = (index % 3) + 2;
            let lines: Vec<LineIn> = items
                .iter()
                .cycle()
                .skip(index)
                .take(line_count)
                .map(|(name, qty, price)| LineIn {
                    item_name: (*name).to_string(),
                    qty: (*qty).to_string(),
                    price: (*price).to_string(),
                    discount: None,
                })
                .collect();
            let input = NewInvoice {
                company_id,
                party_id: *party_id,
                invoice_date: today.clone(),
                currency: Some(currency.clone()),
                description: None,
                tax_bp: Some(500),
                discount: None,
                rep_id: rep_ids.get(index % rep_ids.len().max(1)).copied(),
                pos_machine_id: if index % 4 == 3 {
                    machine_ids.first().copied()
                } else {
                    None
                },
                note: Some(format!("فاتورة تجريبية لـ {party_name}")),
                lines,
            };
            let w = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let invoice = invoices::insert_invoice(&w, scope, actor.id, "sale", &input, None)?;
            w.commit()?;
            invoices::post_invoice(&state, &actor, company_id, invoice)?;
            posted += 1;
        }
        // مسودة غير مرحّلة تُظهر حالة «بانتظار الترحيل».
        let draft_input = NewInvoice {
            company_id,
            party_id: parties[0].0,
            invoice_date: today.clone(),
            currency: Some(parties[0].2.clone()),
            description: None,
            tax_bp: Some(500),
            discount: None,
            rep_id: rep_ids.first().copied(),
            pos_machine_id: None,
            note: Some("مسودة تجريبية".into()),
            lines: vec![LineIn {
                item_name: "خدمة تركيب".into(),
                qty: "1".into(),
                price: "120000".into(),
                discount: None,
            }],
        };
        let draft_invoice = {
            let w = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let id = invoices::insert_invoice(&w, scope, actor.id, "sale", &draft_input, None)?;
            w.commit()?;
            id
        };
        // عرض سعر وطلب عميل.
        let mut quotes = 0_i64;
        for (i, (party_id, _name, currency)) in parties.iter().take(2).enumerate() {
            let w = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let number = common::next_number(&w, company_id, "QT", &today)?;
            let valid = chrono::Utc::now()
                .checked_add_signed(chrono::Duration::days(30))
                .unwrap()
                .format("%Y-%m-%d")
                .to_string();
            w.execute(
                "INSERT INTO sales_quotes(company_id,number,quote_date,valid_until,party_id,currency,total_minor,status,note)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,'sent',?8)",
                params![company_id, number, today, valid, party_id, currency, 300000_i64, "عرض تجريبي"],
            )?;
            let quote_id = w.last_insert_rowid();
            w.execute(
                "INSERT INTO sales_quote_lines(quote_id,line_no,item_name,qty_milli,price_minor,discount_minor,net_minor)
                 VALUES(?1,0,?2,?3,?4,0,?5)",
                params![quote_id, format!("عرض تجريبي {}", i + 1), 2000_i64, 150000_i64, 300000_i64],
            )?;
            w.commit()?;
            quotes += 1;
        }
        let mut orders = 0_i64;
        for (party_id, _name, currency) in parties.iter().take(2) {
            let w = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let number = common::next_number(&w, company_id, "OR", &today)?;
            w.execute(
                "INSERT INTO sales_orders(company_id,number,order_date,party_id,rep_id,currency,total_minor,status,note)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,'confirmed',?8)",
                params![company_id, number, today, party_id, rep_ids.first(), currency, 220000_i64, "طلب تجريبي"],
            )?;
            let order_id = w.last_insert_rowid();
            w.execute(
                "INSERT INTO sales_order_lines(order_id,line_no,item_name,qty_milli,price_minor,discount_minor,net_minor)
                 VALUES(?1,0,'طلب تجريبي',2000,110000,0,220000)",
                [order_id],
            )?;
            w.commit()?;
            orders += 1;
        }
        Ok(serde_json::json!({
            "posted_invoices": posted,
            "draft_invoice": draft_invoice,
            "quotes": quotes,
            "orders": orders,
            "parties": parties.len(),
        }))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}
