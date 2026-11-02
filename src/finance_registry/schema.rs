use rusqlite::Connection;

pub(super) fn init(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS finance_companies (
            id INTEGER PRIMARY KEY, code TEXT NOT NULL UNIQUE, name TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE TABLE IF NOT EXISTS finance_branches (
            id INTEGER PRIMARY KEY, company_id INTEGER NOT NULL REFERENCES finance_companies(id),
            code TEXT NOT NULL, name TEXT NOT NULL, UNIQUE(company_id,code), UNIQUE(company_id,id)
        );
        CREATE TABLE IF NOT EXISTS finance_company_admins (
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            user_id INTEGER NOT NULL REFERENCES identity_users(id) ON DELETE RESTRICT,
            granted_by INTEGER NOT NULL REFERENCES identity_users(id) ON DELETE RESTRICT,
            granted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY(company_id,user_id)
        );
        CREATE TABLE IF NOT EXISTS finance_branch_grants (
            company_id INTEGER NOT NULL,
            branch_id INTEGER NOT NULL,
            user_id INTEGER NOT NULL REFERENCES identity_users(id) ON DELETE RESTRICT,
            role TEXT NOT NULL CHECK(role IN ('reader','operator','admin')),
            granted_by INTEGER NOT NULL REFERENCES identity_users(id) ON DELETE RESTRICT,
            granted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY(company_id,branch_id) REFERENCES finance_branches(company_id,id),
            PRIMARY KEY(company_id,branch_id,user_id)
        );
        CREATE INDEX IF NOT EXISTS finance_branch_grants_user
            ON finance_branch_grants(user_id,company_id,branch_id);
        CREATE TABLE IF NOT EXISTS finance_grant_events (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL,
            branch_id INTEGER NOT NULL,
            user_id INTEGER NOT NULL REFERENCES identity_users(id) ON DELETE RESTRICT,
            actor_id INTEGER NOT NULL REFERENCES identity_users(id) ON DELETE RESTRICT,
            old_role TEXT, new_role TEXT,
            reason TEXT NOT NULL,
            occurred_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY(company_id,branch_id) REFERENCES finance_branches(company_id,id)
        );
        CREATE TRIGGER IF NOT EXISTS finance_grant_events_no_update
            BEFORE UPDATE ON finance_grant_events
            BEGIN SELECT RAISE(ABORT,'grant audit is immutable'); END;
        CREATE TRIGGER IF NOT EXISTS finance_grant_events_no_delete
            BEFORE DELETE ON finance_grant_events
            BEGIN SELECT RAISE(ABORT,'grant audit is immutable'); END;
        CREATE TABLE IF NOT EXISTS finance_period_context (
            period_id INTEGER PRIMARY KEY REFERENCES financial_periods(id) ON DELETE RESTRICT,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT
        );
        CREATE INDEX IF NOT EXISTS finance_period_company ON finance_period_context(company_id,period_id);
        CREATE TRIGGER IF NOT EXISTS finance_period_context_no_update BEFORE UPDATE ON finance_period_context
            BEGIN SELECT RAISE(ABORT,'company period context is immutable'); END;
        CREATE TRIGGER IF NOT EXISTS finance_period_context_no_delete BEFORE DELETE ON finance_period_context
            BEGIN SELECT RAISE(ABORT,'company period context is immutable'); END;
        CREATE TABLE IF NOT EXISTS finance_period_reasons (
            event_id INTEGER PRIMARY KEY REFERENCES period_events(id) ON DELETE RESTRICT,
            reason TEXT NOT NULL
        );
        CREATE TRIGGER IF NOT EXISTS finance_period_reasons_no_update BEFORE UPDATE ON finance_period_reasons
            BEGIN SELECT RAISE(ABORT,'period reasons are immutable'); END;
        CREATE TRIGGER IF NOT EXISTS finance_period_reasons_no_delete BEFORE DELETE ON finance_period_reasons
            BEGIN SELECT RAISE(ABORT,'period reasons are immutable'); END;
        CREATE TABLE IF NOT EXISTS finance_account_scopes (
            account_id INTEGER PRIMARY KEY REFERENCES accounts(id),
            company_id INTEGER NOT NULL REFERENCES finance_companies(id),
            UNIQUE(company_id,account_id)
        );
        CREATE TABLE IF NOT EXISTS finance_company_account_codes (
            company_id INTEGER NOT NULL,
            account_id INTEGER NOT NULL UNIQUE,
            code TEXT NOT NULL,
            PRIMARY KEY(company_id,code),
            FOREIGN KEY(company_id,account_id) REFERENCES finance_account_scopes(company_id,account_id)
        );
        CREATE TABLE IF NOT EXISTS finance_parties (
            id INTEGER PRIMARY KEY, company_id INTEGER NOT NULL, branch_id INTEGER NOT NULL,
            account_id INTEGER NOT NULL UNIQUE, name TEXT NOT NULL,
            role TEXT NOT NULL CHECK(role IN ('customer','supplier','both')),
            FOREIGN KEY(company_id,branch_id) REFERENCES finance_branches(company_id,id),
            FOREIGN KEY(company_id,account_id) REFERENCES finance_account_scopes(company_id,account_id),
            UNIQUE(company_id,branch_id,id)
        );
        CREATE TABLE IF NOT EXISTS finance_funds (
            id INTEGER PRIMARY KEY, company_id INTEGER NOT NULL, branch_id INTEGER NOT NULL,
            account_id INTEGER NOT NULL UNIQUE, name TEXT NOT NULL,
            kind TEXT NOT NULL CHECK(kind IN ('cash','bank')),
            FOREIGN KEY(company_id,branch_id) REFERENCES finance_branches(company_id,id),
            FOREIGN KEY(company_id,account_id) REFERENCES finance_account_scopes(company_id,account_id),
            UNIQUE(company_id,branch_id,id)
        );
        CREATE TRIGGER IF NOT EXISTS finance_party_not_fund_insert BEFORE INSERT ON finance_parties
        WHEN EXISTS(SELECT 1 FROM finance_funds WHERE account_id=NEW.account_id)
        BEGIN SELECT RAISE(ABORT,'account already used by fund'); END;
        CREATE TRIGGER IF NOT EXISTS finance_fund_not_party_insert BEFORE INSERT ON finance_funds
        WHEN EXISTS(SELECT 1 FROM finance_parties WHERE account_id=NEW.account_id)
        BEGIN SELECT RAISE(ABORT,'account already used by party'); END;
        CREATE TABLE IF NOT EXISTS finance_voucher_context (
            voucher_id INTEGER PRIMARY KEY REFERENCES financial_vouchers(id),
            company_id INTEGER NOT NULL, branch_id INTEGER NOT NULL,
            party_id INTEGER NOT NULL, fund_id INTEGER NOT NULL,
            FOREIGN KEY(company_id,branch_id) REFERENCES finance_branches(company_id,id),
            FOREIGN KEY(company_id,branch_id,party_id) REFERENCES finance_parties(company_id,branch_id,id),
            FOREIGN KEY(company_id,branch_id,fund_id) REFERENCES finance_funds(company_id,branch_id,id)
        );
    "#)?;
    // Existing scoped accounts retain their original business code; no owner is guessed.
    db.execute("INSERT INTO finance_company_account_codes(company_id,account_id,code)
        SELECT s.company_id,s.account_id,a.code FROM finance_account_scopes s
        JOIN accounts a ON a.id=s.account_id
        WHERE NOT EXISTS(SELECT 1 FROM finance_company_account_codes c WHERE c.account_id=s.account_id)", [])?;
    Ok(())
}
