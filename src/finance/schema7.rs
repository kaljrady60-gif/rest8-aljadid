//! مخطط الإضافات المالية (schema7): العملات · مجموعات الفروع · الإقرارات الضريبية ·
//! الإشعارات المدينة والدائنة · دورات الزكاة · الموازنات وبنودها وتعديلاتها.
//!
//! لماذا جداول مستقلة: كل كيان منها له دورة حياة وقيود تفرّد خاصة به، ووضعه في جدول
//! مشترك («إعدادات عامة» أو JSON) يفقد القيود المرجعية والأثر التدقيقي.
use rusqlite::Connection;

pub fn init(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS finance_currencies (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            code TEXT NOT NULL,
            name TEXT NOT NULL,
            symbol TEXT,
            decimals INTEGER NOT NULL DEFAULT 2 CHECK(decimals BETWEEN 0 AND 6),
            archived_at TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(company_id, code)
        );

        CREATE TABLE IF NOT EXISTS finance_branch_groups (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            code TEXT NOT NULL,
            name TEXT NOT NULL,
            note TEXT,
            archived_at TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(company_id, code)
        );

        CREATE TABLE IF NOT EXISTS finance_branch_group_members (
            group_id INTEGER NOT NULL REFERENCES finance_branch_groups(id) ON DELETE CASCADE,
            branch_id INTEGER NOT NULL REFERENCES finance_branches(id) ON DELETE CASCADE,
            UNIQUE(group_id, branch_id)
        );

        CREATE TABLE IF NOT EXISTS finance_tax_returns (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            kind TEXT NOT NULL DEFAULT 'vat' CHECK(kind IN ('vat','sales','purchase','withholding')),
            period_from TEXT NOT NULL,
            period_to TEXT NOT NULL,
            output_tax TEXT NOT NULL DEFAULT '0.0000',
            input_tax TEXT NOT NULL DEFAULT '0.0000',
            net_tax TEXT NOT NULL DEFAULT '0.0000',
            status TEXT NOT NULL DEFAULT 'draft' CHECK(status IN ('draft','submitted')),
            note TEXT,
            submitted_at TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(company_id, kind, period_from, period_to)
        );

        CREATE TABLE IF NOT EXISTS finance_notes (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            kind TEXT NOT NULL CHECK(kind IN ('debit','credit')),
            doc_ref TEXT NOT NULL,
            party_id INTEGER REFERENCES finance_parties(id) ON DELETE SET NULL,
            note_date TEXT NOT NULL,
            amount TEXT NOT NULL,
            currency TEXT NOT NULL DEFAULT 'YER',
            reason TEXT,
            status TEXT NOT NULL DEFAULT 'draft' CHECK(status IN ('draft','posted')),
            entry_id INTEGER,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS finance_zakat_cycles (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            fiscal_year INTEGER NOT NULL,
            base TEXT NOT NULL DEFAULT '0.0000',
            rate_bp INTEGER NOT NULL DEFAULT 250,
            due TEXT NOT NULL DEFAULT '0.0000',
            status TEXT NOT NULL DEFAULT 'computed' CHECK(status IN ('computed','posted')),
            entry_id INTEGER,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(company_id, fiscal_year)
        );

        CREATE TABLE IF NOT EXISTS finance_budgets (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            fiscal_year INTEGER NOT NULL,
            name TEXT NOT NULL,
            scope TEXT NOT NULL DEFAULT 'company' CHECK(scope IN ('company','department')),
            department_id INTEGER REFERENCES finance_departments(id) ON DELETE SET NULL,
            status TEXT NOT NULL DEFAULT 'open' CHECK(status IN ('open','approved','closed')),
            note TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(company_id, fiscal_year, name)
        );

        CREATE TABLE IF NOT EXISTS finance_budget_lines (
            id INTEGER PRIMARY KEY,
            budget_id INTEGER NOT NULL REFERENCES finance_budgets(id) ON DELETE CASCADE,
            account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
            month INTEGER NOT NULL CHECK(month BETWEEN 1 AND 12),
            amount TEXT NOT NULL,
            UNIQUE(budget_id, account_id, month)
        );

        CREATE TABLE IF NOT EXISTS finance_budget_adjustments (
            id INTEGER PRIMARY KEY,
            budget_id INTEGER NOT NULL REFERENCES finance_budgets(id) ON DELETE CASCADE,
            account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
            month INTEGER NOT NULL,
            old_amount TEXT NOT NULL,
            new_amount TEXT NOT NULL,
            reason TEXT,
            actor_id INTEGER,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_notes_company ON finance_notes(company_id, note_date);
        CREATE INDEX IF NOT EXISTS idx_budget_lines ON finance_budget_lines(budget_id, account_id);
        CREATE INDEX IF NOT EXISTS idx_tax_returns_company ON finance_tax_returns(company_id, period_from);",
    )
}
