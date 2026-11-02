//! مخطط الموجة الثالثة: الأصول الثابتة وإهلاكها، والموظفون والرواتب.
//!
//! كل الجداول مرتبطة بالشركة، والإهلاك الشهري مُقيّد بفترة واحدة لكل أصل
//! (`UNIQUE(asset_id, period)`) فلا يُحتسب الإهلاك مرتين لنفس الشهر حتى لو
//! نُقر الزر مرارًا أو تكرر الطلب من الواجهة.
use rusqlite::Connection;

pub fn init(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS finance_asset_categories (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            name TEXT NOT NULL,
            method TEXT NOT NULL DEFAULT 'straight'
                CHECK(method IN ('straight','declining')),
            annual_rate TEXT NOT NULL DEFAULT '0',
            asset_account_id INTEGER NOT NULL,
            accum_account_id INTEGER NOT NULL,
            expense_account_id INTEGER NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(company_id, name)
        );

        CREATE TABLE IF NOT EXISTS finance_assets (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            branch_id INTEGER NOT NULL REFERENCES finance_branches(id) ON DELETE RESTRICT,
            category_id INTEGER NOT NULL REFERENCES finance_asset_categories(id) ON DELETE RESTRICT,
            code TEXT NOT NULL,
            name TEXT NOT NULL,
            acquired_on TEXT NOT NULL,
            cost TEXT NOT NULL,
            salvage TEXT NOT NULL DEFAULT '0',
            life_months INTEGER NOT NULL CHECK(life_months > 0),
            funded_from TEXT NOT NULL DEFAULT 'bank'
                CHECK(funded_from IN ('bank','cash','payable','capital')),
            accumulated TEXT NOT NULL DEFAULT '0',
            status TEXT NOT NULL DEFAULT 'active'
                CHECK(status IN ('active','disposed')),
            acquired_entry_id INTEGER REFERENCES journal_entries(id) ON DELETE RESTRICT,
            disposal_entry_id INTEGER REFERENCES journal_entries(id) ON DELETE RESTRICT,
            disposed_on TEXT,
            created_by INTEGER,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(company_id, code)
        );
        CREATE INDEX IF NOT EXISTS finance_assets_status ON finance_assets(company_id, status);

        CREATE TABLE IF NOT EXISTS finance_asset_depreciations (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            asset_id INTEGER NOT NULL REFERENCES finance_assets(id) ON DELETE RESTRICT,
            period TEXT NOT NULL,
            amount TEXT NOT NULL,
            accumulated_after TEXT NOT NULL,
            entry_id INTEGER NOT NULL REFERENCES journal_entries(id) ON DELETE RESTRICT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(asset_id, period)
        );
        CREATE INDEX IF NOT EXISTS finance_asset_dep_period
            ON finance_asset_depreciations(company_id, period);

        CREATE TABLE IF NOT EXISTS finance_employees (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            branch_id INTEGER NOT NULL REFERENCES finance_branches(id) ON DELETE RESTRICT,
            code TEXT NOT NULL,
            name TEXT NOT NULL,
            job_title TEXT,
            hired_on TEXT NOT NULL,
            base_salary TEXT NOT NULL,
            allowances TEXT NOT NULL DEFAULT '0',
            deductions TEXT NOT NULL DEFAULT '0',
            currency TEXT NOT NULL DEFAULT 'YER',
            status TEXT NOT NULL DEFAULT 'active'
                CHECK(status IN ('active','suspended','terminated')),
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(company_id, code)
        );
        CREATE INDEX IF NOT EXISTS finance_employees_status
            ON finance_employees(company_id, status);

        CREATE TABLE IF NOT EXISTS finance_payroll_runs (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            branch_id INTEGER NOT NULL REFERENCES finance_branches(id) ON DELETE RESTRICT,
            period TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'draft'
                CHECK(status IN ('draft','posted','paid','cancelled')),
            gross_total TEXT NOT NULL DEFAULT '0',
            deduction_total TEXT NOT NULL DEFAULT '0',
            net_total TEXT NOT NULL DEFAULT '0',
            currency TEXT NOT NULL DEFAULT 'YER',
            entry_id INTEGER REFERENCES journal_entries(id) ON DELETE RESTRICT,
            payment_entry_id INTEGER REFERENCES journal_entries(id) ON DELETE RESTRICT,
            created_by INTEGER,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(company_id, period)
        );

        CREATE TABLE IF NOT EXISTS finance_payroll_lines (
            id INTEGER PRIMARY KEY,
            run_id INTEGER NOT NULL REFERENCES finance_payroll_runs(id) ON DELETE RESTRICT,
            employee_id INTEGER NOT NULL REFERENCES finance_employees(id) ON DELETE RESTRICT,
            employee_name TEXT NOT NULL,
            base_salary TEXT NOT NULL,
            allowances TEXT NOT NULL,
            deductions TEXT NOT NULL,
            net TEXT NOT NULL,
            UNIQUE(run_id, employee_id)
        );
        "#,
    )
}
