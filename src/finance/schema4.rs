//! مخطط الجيل الرابع: الضرائب والرسوم وترقيم المستندات.
use rusqlite::Connection;

pub fn init(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS finance_taxes (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            code TEXT NOT NULL,
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            rate_bp INTEGER NOT NULL,
            account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
            is_inclusive INTEGER NOT NULL DEFAULT 0,
            archived_at TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(company_id, code)
        );
        CREATE TABLE IF NOT EXISTS finance_doc_sequences (
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            doc_type TEXT NOT NULL,
            prefix TEXT NOT NULL DEFAULT '',
            next_no INTEGER NOT NULL DEFAULT 1,
            width INTEGER NOT NULL DEFAULT 5,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY(company_id, doc_type)
        );
        CREATE TABLE IF NOT EXISTS finance_departments (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            code TEXT NOT NULL,
            name TEXT NOT NULL,
            parent_id INTEGER REFERENCES finance_departments(id) ON DELETE RESTRICT,
            manager_name TEXT,
            depth INTEGER NOT NULL DEFAULT 0,
            archived_at TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(company_id, code)
        );
        "#,
    )
}
