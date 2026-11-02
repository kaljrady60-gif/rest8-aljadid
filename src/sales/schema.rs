//! مخطط قسم المبيعات: أعداد صحيحة للوحدات الصغرى (minor) والنسب (basis points).
//! كل جدول يحمل company_id ويُقيَّد بمفاتيح خارجية صريحة — خلافًا للمرجع.
use rusqlite::Connection;

pub const MONEY_SCALE: i64 = 100; // منزلتان عشريتان
pub const QTY_SCALE: i64 = 1000; // ثلاث منازل للكميات

pub fn init_schema(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS sales_groups (
            id INTEGER PRIMARY KEY, company_id INTEGER NOT NULL,
            code TEXT NOT NULL, name TEXT NOT NULL,
            discount_bp INTEGER NOT NULL DEFAULT 0 CHECK(discount_bp BETWEEN 0 AND 10000),
            status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','archived')),
            UNIQUE(company_id, code)
        );
        CREATE TABLE IF NOT EXISTS sales_reps (
            id INTEGER PRIMARY KEY, company_id INTEGER NOT NULL,
            number TEXT NOT NULL, name TEXT NOT NULL, phone TEXT,
            commission_bp INTEGER NOT NULL DEFAULT 0 CHECK(commission_bp BETWEEN 0 AND 10000),
            status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','archived')),
            UNIQUE(company_id, number)
        );
        CREATE TABLE IF NOT EXISTS sales_collectors (
            id INTEGER PRIMARY KEY, company_id INTEGER NOT NULL,
            number TEXT NOT NULL, name TEXT NOT NULL, phone TEXT,
            commission_bp INTEGER NOT NULL DEFAULT 0 CHECK(commission_bp BETWEEN 0 AND 10000),
            status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','archived')),
            UNIQUE(company_id, number)
        );
        CREATE TABLE IF NOT EXISTS sales_cards (
            id INTEGER PRIMARY KEY, company_id INTEGER NOT NULL,
            name TEXT NOT NULL,
            network_limit_minor INTEGER NOT NULL DEFAULT 0 CHECK(network_limit_minor >= 0),
            off_network_limit_minor INTEGER NOT NULL DEFAULT 0 CHECK(off_network_limit_minor >= 0),
            commission_bp INTEGER NOT NULL DEFAULT 0 CHECK(commission_bp BETWEEN 0 AND 10000),
            status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','archived'))
        );
        CREATE TABLE IF NOT EXISTS sales_types (
            id INTEGER PRIMARY KEY, company_id INTEGER NOT NULL,
            code TEXT NOT NULL, name TEXT NOT NULL,
            kind TEXT NOT NULL DEFAULT 'cash' CHECK(kind IN ('cash','credit','installment','pos')),
            category TEXT, status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','archived')),
            UNIQUE(company_id, code)
        );
        CREATE TABLE IF NOT EXISTS sales_reasons (
            id INTEGER PRIMARY KEY, company_id INTEGER NOT NULL,
            code TEXT NOT NULL, name TEXT NOT NULL, category TEXT,
            status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','archived')),
            UNIQUE(company_id, code)
        );
        CREATE TABLE IF NOT EXISTS sales_pos_machines (
            id INTEGER PRIMARY KEY, company_id INTEGER NOT NULL,
            number TEXT NOT NULL, name TEXT NOT NULL,
            cashbox_id INTEGER, status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','archived')),
            UNIQUE(company_id, number)
        );
        CREATE TABLE IF NOT EXISTS sales_pos_custody (
            id INTEGER PRIMARY KEY, company_id INTEGER NOT NULL,
            machine_id INTEGER NOT NULL REFERENCES sales_pos_machines(id) ON DELETE RESTRICT,
            cashier TEXT NOT NULL, custody_date TEXT NOT NULL,
            amount_minor INTEGER NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('open','handover','close')),
            note TEXT, created_by INTEGER, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE TABLE IF NOT EXISTS sales_cash_customers (
            id INTEGER PRIMARY KEY, company_id INTEGER NOT NULL,
            name TEXT NOT NULL, phone TEXT, balance_minor INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','archived'))
        );
        CREATE TABLE IF NOT EXISTS sales_invoices (
            id INTEGER PRIMARY KEY, company_id INTEGER NOT NULL,
            number TEXT NOT NULL, invoice_type TEXT NOT NULL
                CHECK(invoice_type IN ('sale','sale_return')),
            invoice_date TEXT NOT NULL,
            party_id INTEGER NOT NULL REFERENCES finance_parties(id) ON DELETE RESTRICT,
            currency TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'draft' CHECK(status IN ('draft','posted','cancelled')),
            description TEXT NOT NULL DEFAULT '',
            subtotal_minor INTEGER NOT NULL DEFAULT 0 CHECK(subtotal_minor >= 0),
            discount_minor INTEGER NOT NULL DEFAULT 0 CHECK(discount_minor >= 0),
            tax_minor INTEGER NOT NULL DEFAULT 0 CHECK(tax_minor >= 0),
            total_minor INTEGER NOT NULL DEFAULT 0 CHECK(total_minor >= 0),
            paid_minor INTEGER NOT NULL DEFAULT 0 CHECK(paid_minor >= 0),
            journal_entry_id INTEGER, draft_id INTEGER,
            original_invoice_id INTEGER REFERENCES sales_invoices(id) ON DELETE RESTRICT,
            rep_id INTEGER REFERENCES sales_reps(id) ON DELETE RESTRICT,
            pos_machine_id INTEGER REFERENCES sales_pos_machines(id) ON DELETE RESTRICT,
            note TEXT, created_by INTEGER,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(company_id, number),
            CHECK(invoice_type <> 'sale_return' OR original_invoice_id IS NOT NULL)
        );
        CREATE TABLE IF NOT EXISTS sales_invoice_lines (
            id INTEGER PRIMARY KEY,
            invoice_id INTEGER NOT NULL REFERENCES sales_invoices(id) ON DELETE RESTRICT,
            line_no INTEGER NOT NULL CHECK(line_no >= 0),
            item_name TEXT NOT NULL,
            qty_milli INTEGER NOT NULL CHECK(qty_milli > 0),
            price_minor INTEGER NOT NULL CHECK(price_minor >= 0),
            discount_minor INTEGER NOT NULL DEFAULT 0 CHECK(discount_minor >= 0),
            net_minor INTEGER NOT NULL CHECK(net_minor >= 0),
            UNIQUE(invoice_id, line_no)
        );
        CREATE TABLE IF NOT EXISTS sales_quotes (
            id INTEGER PRIMARY KEY, company_id INTEGER NOT NULL,
            number TEXT NOT NULL, quote_date TEXT NOT NULL, valid_until TEXT,
            party_id INTEGER NOT NULL REFERENCES finance_parties(id) ON DELETE RESTRICT,
            currency TEXT NOT NULL, total_minor INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'draft' CHECK(status IN ('draft','sent','accepted','converted','expired','cancelled')),
            note TEXT, invoice_id INTEGER REFERENCES sales_invoices(id) ON DELETE RESTRICT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(company_id, number)
        );
        CREATE TABLE IF NOT EXISTS sales_quote_lines (
            id INTEGER PRIMARY KEY,
            quote_id INTEGER NOT NULL REFERENCES sales_quotes(id) ON DELETE RESTRICT,
            line_no INTEGER NOT NULL, item_name TEXT NOT NULL,
            qty_milli INTEGER NOT NULL CHECK(qty_milli > 0),
            price_minor INTEGER NOT NULL CHECK(price_minor >= 0),
            discount_minor INTEGER NOT NULL DEFAULT 0, net_minor INTEGER NOT NULL DEFAULT 0,
            UNIQUE(quote_id, line_no)
        );
        CREATE TABLE IF NOT EXISTS sales_orders (
            id INTEGER PRIMARY KEY, company_id INTEGER NOT NULL,
            number TEXT NOT NULL, order_date TEXT NOT NULL,
            party_id INTEGER NOT NULL REFERENCES finance_parties(id) ON DELETE RESTRICT,
            rep_id INTEGER REFERENCES sales_reps(id) ON DELETE RESTRICT,
            currency TEXT NOT NULL, total_minor INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'draft' CHECK(status IN ('draft','confirmed','delivered','invoiced','cancelled')),
            note TEXT, invoice_id INTEGER REFERENCES sales_invoices(id) ON DELETE RESTRICT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(company_id, number)
        );
        CREATE TABLE IF NOT EXISTS sales_order_lines (
            id INTEGER PRIMARY KEY,
            order_id INTEGER NOT NULL REFERENCES sales_orders(id) ON DELETE RESTRICT,
            line_no INTEGER NOT NULL, item_name TEXT NOT NULL,
            qty_milli INTEGER NOT NULL CHECK(qty_milli > 0),
            price_minor INTEGER NOT NULL CHECK(price_minor >= 0),
            discount_minor INTEGER NOT NULL DEFAULT 0, net_minor INTEGER NOT NULL DEFAULT 0,
            UNIQUE(order_id, line_no)
        );
        CREATE TABLE IF NOT EXISTS sales_commission_plans (
            id INTEGER PRIMARY KEY, company_id INTEGER NOT NULL,
            name TEXT NOT NULL, percent_bp INTEGER NOT NULL DEFAULT 0,
            basis TEXT NOT NULL DEFAULT 'sale' CHECK(basis IN ('sale','collection')),
            active INTEGER NOT NULL DEFAULT 1
        );
        CREATE TABLE IF NOT EXISTS sales_commission_rules (
            id INTEGER PRIMARY KEY, company_id INTEGER NOT NULL,
            name TEXT NOT NULL, basis TEXT NOT NULL DEFAULT 'sale' CHECK(basis IN ('sale','collection')),
            applies_to TEXT NOT NULL DEFAULT 'rep' CHECK(applies_to IN ('rep','collector','group')),
            rate_bp INTEGER NOT NULL DEFAULT 0 CHECK(rate_bp BETWEEN 0 AND 10000),
            min_amount_minor INTEGER NOT NULL DEFAULT 0, active INTEGER NOT NULL DEFAULT 1
        );
        CREATE TABLE IF NOT EXISTS sales_commission_calcs (
            id INTEGER PRIMARY KEY, company_id INTEGER NOT NULL,
            period TEXT NOT NULL, subject TEXT NOT NULL, subject_id INTEGER,
            basis TEXT NOT NULL, base_minor INTEGER NOT NULL,
            rate_bp INTEGER NOT NULL, amount_minor INTEGER NOT NULL,
            note TEXT, created_by INTEGER, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE TABLE IF NOT EXISTS sales_sequences (
            company_id INTEGER NOT NULL, kind TEXT NOT NULL, year TEXT NOT NULL,
            last INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY(company_id, kind, year)
        );
        CREATE INDEX IF NOT EXISTS sales_invoices_party ON sales_invoices(company_id, party_id);
        CREATE INDEX IF NOT EXISTS sales_invoices_status ON sales_invoices(company_id, status);
        CREATE TRIGGER IF NOT EXISTS sales_line_of_posted_invoice BEFORE INSERT ON sales_invoice_lines
        WHEN EXISTS(SELECT 1 FROM sales_invoices WHERE id=NEW.invoice_id AND status='posted')
        BEGIN SELECT RAISE(ABORT,'posted invoice is immutable'); END;
        CREATE TRIGGER IF NOT EXISTS sales_invoice_posted_no_delete BEFORE DELETE ON sales_invoices
        WHEN OLD.status='posted'
        BEGIN SELECT RAISE(ABORT,'posted invoice cannot be deleted'); END;
        "#,
    )
}
