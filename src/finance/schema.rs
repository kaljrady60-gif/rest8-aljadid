//! مخطط المالية الموسّع: صناديق، أسعار صرف، مراكز تكلفة، أرصدة افتتاحية،
//! تسوية بنكية، وسجل تدقيق غير قابل للتعديل. كل الجداول مرتبطة بالشركة.
use rusqlite::Connection;

pub fn init(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS finance_fx_rates (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            base TEXT NOT NULL,
            quote TEXT NOT NULL,
            rate TEXT NOT NULL,
            as_of TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            CHECK(base <> quote),
            UNIQUE(company_id, base, quote, as_of)
        );
        CREATE INDEX IF NOT EXISTS finance_fx_lookup ON finance_fx_rates(company_id, base, quote, as_of DESC);

        CREATE TABLE IF NOT EXISTS finance_dimensions (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            code TEXT NOT NULL,
            name TEXT NOT NULL,
            kind TEXT NOT NULL CHECK(kind IN ('cost_center','project')),
            archived_at TEXT,
            UNIQUE(company_id, code),
            UNIQUE(company_id, id)
        );

        CREATE TABLE IF NOT EXISTS finance_entry_dimensions (
            entry_id INTEGER NOT NULL REFERENCES journal_entries(id) ON DELETE RESTRICT,
            line_no INTEGER NOT NULL,
            dimension_id INTEGER NOT NULL,
            company_id INTEGER NOT NULL,
            PRIMARY KEY(entry_id, line_no, dimension_id),
            FOREIGN KEY(entry_id, line_no) REFERENCES journal_lines(entry_id, line_no) ON DELETE RESTRICT,
            FOREIGN KEY(company_id, dimension_id) REFERENCES finance_dimensions(company_id, id) ON DELETE RESTRICT
        );

        CREATE TABLE IF NOT EXISTS finance_opening_balances (
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
            currency TEXT NOT NULL,
            debit TEXT NOT NULL DEFAULT '0',
            credit TEXT NOT NULL DEFAULT '0',
            created_by INTEGER REFERENCES identity_users(id) ON DELETE RESTRICT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY(company_id, account_id, currency)
        );

        CREATE TABLE IF NOT EXISTS finance_bank_statements (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            fund_id INTEGER NOT NULL REFERENCES finance_funds(id) ON DELETE RESTRICT,
            period_start TEXT NOT NULL,
            period_end TEXT NOT NULL,
            closing_balance TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS finance_bankrec_lines (
            id INTEGER PRIMARY KEY,
            statement_id INTEGER NOT NULL REFERENCES finance_bank_statements(id) ON DELETE RESTRICT,
            entry_id INTEGER REFERENCES journal_entries(id) ON DELETE RESTRICT,
            kind TEXT NOT NULL CHECK(kind IN ('deposit','withdrawal','adjustment','charge','interest')),
            amount TEXT NOT NULL,
            memo TEXT NOT NULL,
            cleared INTEGER NOT NULL DEFAULT 0 CHECK(cleared IN (0,1))
        );

        CREATE TABLE IF NOT EXISTS finance_audit_log (
            id INTEGER PRIMARY KEY,
            company_id INTEGER,
            actor_id INTEGER,
            action TEXT NOT NULL,
            entity TEXT NOT NULL,
            entity_id TEXT,
            payload TEXT,
            occurred_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS finance_audit_company ON finance_audit_log(company_id, id DESC);
        CREATE TRIGGER IF NOT EXISTS finance_audit_no_update BEFORE UPDATE ON finance_audit_log
            BEGIN SELECT RAISE(ABORT,'audit log is immutable'); END;
        CREATE TRIGGER IF NOT EXISTS finance_audit_no_delete BEFORE DELETE ON finance_audit_log
            BEGIN SELECT RAISE(ABORT,'audit log is immutable'); END;
        "#,
    )?;
    // جدول الصناديق موجود في سجل المالية؛ نوسّعه بدل تكراره (مصدر واحد).
    for (column, kind) in [
        ("code", "TEXT"),
        ("currency", "TEXT"),
        ("iban", "TEXT"),
        ("opened_on", "TEXT"),
        ("archived_at", "TEXT"),
    ] {
        add_column_if_missing(db, "finance_funds", column, kind)?;
    }
    db.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS finance_funds_code
             ON finance_funds(company_id, kind, code) WHERE code IS NOT NULL;",
    )?;
    add_column_if_missing(db, "drafts", "company_id", "INTEGER")?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS drafts_company ON drafts(company_id, id DESC);")?;
    add_column_if_missing(db, "journal_entries", "company_id", "INTEGER")?;
    add_column_if_missing(db, "journal_entries", "branch_id", "INTEGER")?;
    db.execute_batch(
        "CREATE INDEX IF NOT EXISTS journal_entries_company ON journal_entries(company_id, entry_date);",
    )?;
    Ok(())
}

/// إضافة عمود إن لم يكن موجودًا (ترقية آمنة لقواعد قديمة).
pub fn add_column_if_missing(
    db: &Connection,
    table: &str,
    column: &str,
    kind: &str,
) -> rusqlite::Result<()> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name=?2)",
        rusqlite::params![table, column],
        |r| r.get(0),
    )?;
    if !exists {
        db.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {kind};"))?;
    }
    Ok(())
}
