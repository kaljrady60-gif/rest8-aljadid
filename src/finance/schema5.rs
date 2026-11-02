//! مخطط الجيل الخامس: التسويات · مراجعة الوثائق · المخططات والعمولات ·
//! بيانات الدول · الحسابات الوسيطة · متغيرات الأستاذ · الطلبات.
use rusqlite::Connection;

pub fn init(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS finance_adjustments (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            kind TEXT NOT NULL,
            description TEXT NOT NULL,
            amount TEXT NOT NULL,
            currency TEXT NOT NULL,
            account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
            offset_account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
            status TEXT NOT NULL DEFAULT 'pending',
            decided_by INTEGER,
            decided_at TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            archived_at TEXT
        );

        CREATE TABLE IF NOT EXISTS finance_doc_reviews (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            doc_type TEXT NOT NULL,
            doc_ref TEXT NOT NULL,
            note TEXT,
            status TEXT NOT NULL DEFAULT 'pending',
            decided_by INTEGER,
            decided_at TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS finance_plans (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            code TEXT NOT NULL,
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            rate_bp INTEGER NOT NULL DEFAULT 0,
            fixed_amount TEXT NOT NULL DEFAULT '0.0000',
            account_id INTEGER REFERENCES accounts(id) ON DELETE RESTRICT,
            archived_at TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(company_id, code)
        );

        CREATE TABLE IF NOT EXISTS finance_countries (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            iso_code TEXT NOT NULL,
            name TEXT NOT NULL,
            currency TEXT NOT NULL,
            tax_rate_bp INTEGER NOT NULL DEFAULT 0,
            is_default INTEGER NOT NULL DEFAULT 0,
            archived_at TEXT,
            UNIQUE(company_id, iso_code)
        );

        CREATE TABLE IF NOT EXISTS finance_intermediate (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            code TEXT NOT NULL,
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
            archived_at TEXT,
            UNIQUE(company_id, code)
        );

        CREATE TABLE IF NOT EXISTS finance_ledger_settings (
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            key TEXT NOT NULL,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY(company_id, key)
        );

        CREATE TABLE IF NOT EXISTS finance_requests (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            kind TEXT NOT NULL,
            title TEXT NOT NULL,
            payload TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'draft',
            created_by INTEGER,
            decided_by INTEGER,
            decided_at TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )
}
