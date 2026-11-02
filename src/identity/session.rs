//! قراءة الجلسة: الكوكي أو ترويسة المعاينة، وبناء ترويسة الكوكي.
use super::{Principal, now, token_hash, valid_token};
use axum::http::{HeaderMap, header};
use rusqlite::{Connection, OptionalExtension, params};

pub(super) fn secure_cookie() -> bool {
    std::env::var("SMART_SECURE_COOKIE").as_deref() == Ok("1")
}
pub fn preview_partitioned_cookie() -> bool {
    std::env::var("SMART_PREVIEW_PARTITIONED_COOKIE").as_deref() == Ok("1")
}
/// سياق iframe عبر المواقع قد يحجب الكوكي كليًا (أصل معزول). في المعاينة فقط
/// نسمح بحمل رمز الجلسة في ترويسة، وهو نفس الرمز العشوائي المخزّن مُجزّأً في القاعدة.
pub fn preview_header_session() -> bool {
    std::env::var("SMART_PREVIEW_HEADER_SESSION").as_deref() == Ok("1")
}
pub(super) fn cookie_name() -> &'static str {
    if preview_partitioned_cookie() {
        "smart_preview_session"
    } else {
        "smart_session"
    }
}
pub(super) fn cookie_value(token: &str, max_age: i64, secure: bool, partitioned: bool) -> String {
    let name = if partitioned {
        "smart_preview_session"
    } else {
        "smart_session"
    };
    let attributes = if partitioned {
        "; SameSite=None; Secure; Partitioned"
    } else if secure {
        "; SameSite=Strict; Secure"
    } else {
        "; SameSite=Strict"
    };
    format!("{name}={token}; Path=/; HttpOnly; Max-Age={max_age}{attributes}")
}

pub(super) enum SessionCookie<'a> {
    Missing,
    Invalid,
    Valid(&'a str),
}
pub(super) fn inspect_cookie(headers: &HeaderMap) -> SessionCookie<'_> {
    // قد يحمل المتصفح نسختين من الكوكي نفسه (بعد تسجيل دخول متكرر أو تغيّر مسار).
    // الأصلح للمستخدم: نأخذ آخر قيمة صالحة بدل رفض الجلسة كلها.
    let mut token = None;
    let mut malformed = false;
    for raw in headers.get_all(header::COOKIE) {
        let Ok(raw) = raw.to_str() else { continue };
        for item in raw.split(';') {
            let item = item.trim();
            let Some((key, value)) = item.split_once('=') else {
                continue;
            };
            if key.trim() != cookie_name() {
                continue;
            }
            if valid_token(value) {
                token = Some(value);
            } else {
                malformed = true;
            }
        }
    }
    match (token, malformed) {
        (Some(token), _) => SessionCookie::Valid(token),
        (None, true) => SessionCookie::Invalid,
        (None, false) => SessionCookie::Missing,
    }
}
pub(super) fn cookie_token(headers: &HeaderMap) -> Option<&str> {
    match inspect_cookie(headers) {
        SessionCookie::Valid(token) => Some(token),
        SessionCookie::Missing | SessionCookie::Invalid => None,
    }
}
/// لا يُفعَّل إلا بـ SMART_PREVIEW_HEADER_SESSION=1 (معاينة فقط، مطفأ افتراضيًا).
pub(super) fn header_token(headers: &HeaderMap) -> Option<&str> {
    if !preview_header_session() {
        return None;
    }
    headers
        .get("x-smart-session")
        .and_then(|h| h.to_str().ok())
        .filter(|t| valid_token(t))
}
pub(super) fn principal(
    db: &Connection,
    headers: &HeaderMap,
) -> rusqlite::Result<Option<Principal>> {
    let Some(token) = cookie_token(headers).or_else(|| header_token(headers)) else {
        return Ok(None);
    };
    let digest = token_hash(token);
    db.query_row("SELECT u.id,u.username,u.role,s.csrf FROM identity_sessions s JOIN identity_users u ON u.id=s.user_id WHERE s.token_hash=?1 AND s.expires_at>?2 AND u.active=1",params![digest,now()],|r|Ok(Principal{id:r.get(0)?,username:r.get(1)?,role:r.get(2)?,csrf:r.get(3)?,token_hash:digest.clone()})).optional()
}
