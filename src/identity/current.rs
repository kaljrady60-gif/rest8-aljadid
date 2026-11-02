use super::*;

pub fn is_current_user(db: &Connection, p: &Principal) -> rusqlite::Result<bool> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM identity_sessions s JOIN identity_users u ON u.id=s.user_id
         WHERE s.token_hash=?1 AND s.user_id=?2 AND s.expires_at>?3 AND u.active=1)",
        params![p.token_hash, p.id, now()],
        |r| r.get(0),
    )
}
