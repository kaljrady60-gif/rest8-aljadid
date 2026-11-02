//! مخطط الموجة الثانية: الأطراف، سياسة الاعتماد وسير القيود، الشيكات، الإقفال السنوي.
//! كل الجداول مرتبطة بالشركة، وسجل الإقفال السنوي محمي بمُشغّلات ضد التعديل.
//! سياسة الاعتماد تُفرض في طبقة التطبيق (لا بمُشغّل قاعدة) كي لا تمنع القيود
//! النظامية التي يولّدها المحرك نفسه مثل ترحيل الشيكات والإقفال السنوي.
use rusqlite::Connection;

pub fn init(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS finance_approval_policy (
            company_id INTEGER PRIMARY KEY REFERENCES finance_companies(id) ON DELETE RESTRICT,
            require_approval INTEGER NOT NULL DEFAULT 0 CHECK(require_approval IN (0,1)),
            updated_by INTEGER,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS finance_draft_flow (
            draft_id INTEGER PRIMARY KEY REFERENCES drafts(id) ON DELETE RESTRICT,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            state TEXT NOT NULL DEFAULT 'pending'
                CHECK(state IN ('pending','submitted','approved','rejected','posted')),
            submitted_by INTEGER,
            submitted_at TEXT,
            decided_by INTEGER,
            decided_at TEXT,
            note TEXT,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS finance_draft_flow_state ON finance_draft_flow(company_id, state);

        CREATE TABLE IF NOT EXISTS finance_checks (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            branch_id INTEGER NOT NULL REFERENCES finance_branches(id) ON DELETE RESTRICT,
            fund_id INTEGER REFERENCES finance_funds(id) ON DELETE RESTRICT,
            party_id INTEGER,
            direction TEXT NOT NULL CHECK(direction IN ('received','issued')),
            number TEXT NOT NULL,
            bank TEXT NOT NULL,
            drawer TEXT NOT NULL,
            amount TEXT NOT NULL,
            currency TEXT NOT NULL,
            due_date TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'registered'
                CHECK(status IN ('registered','deposited','collected','bounced','endorsed','cancelled')),
            entry_id INTEGER REFERENCES journal_entries(id) ON DELETE RESTRICT,
            notes TEXT,
            created_by INTEGER,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            CHECK(amount <> '0'),
            UNIQUE(company_id, direction, number, bank)
        );
        CREATE INDEX IF NOT EXISTS finance_checks_status ON finance_checks(company_id, status, due_date);

        CREATE TABLE IF NOT EXISTS finance_year_closings (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            fiscal_year INTEGER NOT NULL,
            entry_id INTEGER NOT NULL REFERENCES journal_entries(id) ON DELETE RESTRICT,
            net_income TEXT NOT NULL,
            closed_by INTEGER,
            closed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            notes TEXT,
            UNIQUE(company_id, fiscal_year)
        );
        CREATE TRIGGER IF NOT EXISTS finance_year_closings_no_update
            BEFORE UPDATE ON finance_year_closings
            BEGIN SELECT RAISE(ABORT,'year closing is immutable'); END;
        CREATE TRIGGER IF NOT EXISTS finance_year_closings_no_delete
            BEFORE DELETE ON finance_year_closings
            BEGIN SELECT RAISE(ABORT,'year closing is immutable'); END;

        "#,
    )
}
