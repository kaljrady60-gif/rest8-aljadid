//! أدوات دعم الاختبارات للهوية — خارج ملف الهوية للحفاظ على حجمه.
use crate::identity::{Principal, now, random_hex, token_hash};
use rusqlite::{Connection, params};

/// نفس `test_admin` لكنه يعيد الرمز الخام لبناء ترويسة كوكي في الاختبارات.
#[cfg(test)]
pub fn test_admin_session(db: &Connection) -> (Principal, String) {
    db.execute(
        "INSERT OR IGNORE INTO identity_users(username,password_hash,role)
         VALUES('__test_admin__','test-only','admin')",
        [],
    )
    .expect("insert test admin");
    let id: i64 = db
        .query_row(
            "SELECT id FROM identity_users WHERE username='__test_admin__'",
            [],
            |r| r.get(0),
        )
        .expect("test admin id");
    let token = random_hex().expect("token");
    let csrf = random_hex().expect("csrf");
    db.execute(
        "INSERT INTO identity_sessions(token_hash,user_id,csrf,expires_at) VALUES(?1,?2,?3,?4)",
        params![token_hash(&token), id, csrf, now() + 3600],
    )
    .expect("insert session");
    (
        crate::identity::for_test(id, "__test_admin__", "admin", csrf.clone(), token.clone()),
        token,
    )
}

#[cfg(test)]
pub fn test_admin(db: &Connection) -> Principal {
    db.execute(
        "INSERT OR IGNORE INTO identity_users(username,password_hash,role)
         VALUES('__test_admin__','test-only','admin')",
        [],
    )
    .expect("insert test admin");
    let id: i64 = db
        .query_row(
            "SELECT id FROM identity_users WHERE username='__test_admin__'",
            [],
            |r| r.get(0),
        )
        .expect("test admin id");
    let token = random_hex().expect("token");
    let _digest = token_hash(&token);
    let csrf = random_hex().expect("csrf");
    db.execute(
        "INSERT INTO identity_sessions(token_hash,user_id,csrf,expires_at) VALUES(?1,?2,?3,?4)",
        params![_digest, id, csrf, now() + 3600],
    )
    .expect("insert session");
    crate::identity::for_test(id, "__test_admin__", "admin", csrf, token)
}
