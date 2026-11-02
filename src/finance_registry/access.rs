//! Explicit grants. This is not yet system-wide tenant isolation.
use rusqlite::{Connection, params};

pub(crate) fn company_admin(db: &Connection, user: i64, company: i64) -> rusqlite::Result<bool> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM finance_company_admins g
         JOIN identity_users u ON u.id=g.user_id AND u.active=1
         WHERE g.user_id=?1 AND g.company_id=?2)",
        params![user, company],
        |r| r.get(0),
    )
}

pub(crate) fn branch_write(
    db: &Connection,
    user: i64,
    company: i64,
    branch: i64,
) -> rusqlite::Result<bool> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM finance_branch_grants g
         JOIN identity_users u ON u.id=g.user_id AND u.active=1
         WHERE g.user_id=?1 AND g.company_id=?2 AND g.branch_id=?3
               AND g.role IN ('operator','admin'))",
        params![user, company, branch],
        |r| r.get(0),
    )
}

pub(super) fn branch_admin(
    db: &Connection,
    user: i64,
    company: i64,
    branch: i64,
) -> rusqlite::Result<bool> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM finance_branch_grants g
         JOIN identity_users u ON u.id=g.user_id AND u.active=1
         WHERE g.user_id=?1 AND g.company_id=?2 AND g.branch_id=?3 AND g.role='admin')",
        params![user, company, branch],
        |r| r.get(0),
    )
}

pub(crate) fn company_read(db: &Connection, user: i64, company: i64) -> rusqlite::Result<bool> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM identity_users u WHERE u.id=?1 AND u.active=1 AND
          (EXISTS(SELECT 1 FROM finance_company_admins a WHERE a.user_id=u.id AND a.company_id=?2)
           OR EXISTS(SELECT 1 FROM finance_branch_grants g WHERE g.user_id=u.id AND g.company_id=?2)))",
        params![user, company], |r| r.get(0),
    )
}

pub(crate) fn branch_read(
    db: &Connection,
    user: i64,
    company: i64,
    branch: i64,
) -> rusqlite::Result<bool> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM finance_branch_grants g
         JOIN identity_users u ON u.id=g.user_id AND u.active=1
         WHERE g.user_id=?1 AND g.company_id=?2 AND g.branch_id=?3)",
        params![user, company, branch],
        |r| r.get(0),
    )
}
