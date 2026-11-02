//! Local identity foundation. Does not implement tenant isolation or financial audit.
use crate::{AppState, db_open, fail};
use argon2::{
    Argon2,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const SESSION_SECONDS: i64 = 12 * 60 * 60;
pub fn init_schema(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS identity_users(id INTEGER PRIMARY KEY, username TEXT NOT NULL UNIQUE, password_hash TEXT NOT NULL, role TEXT NOT NULL CHECK(role IN ('admin','viewer')), active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0,1)), created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);\
    CREATE TABLE IF NOT EXISTS identity_sessions(token_hash BLOB PRIMARY KEY, user_id INTEGER NOT NULL REFERENCES identity_users(id) ON DELETE CASCADE, csrf TEXT NOT NULL, expires_at INTEGER NOT NULL);\
    CREATE INDEX IF NOT EXISTS identity_sessions_expiry ON identity_sessions(expires_at);\
    CREATE TABLE IF NOT EXISTS identity_attempts(username TEXT PRIMARY KEY, failures INTEGER NOT NULL, locked_until INTEGER NOT NULL);")
}
pub(crate) fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
pub(crate) fn random_hex() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
pub(crate) fn token_hash(value: &str) -> Vec<u8> {
    Sha256::digest(value.as_bytes()).to_vec()
}
fn valid_token(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn valid_username(s: &str) -> bool {
    !s.trim().is_empty()
        && s == s.trim()
        && s.chars().count() <= 120
        && !s.chars().any(char::is_control)
}
fn hash_password(password: &str) -> Result<String, String> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|x| x.to_string())
        .map_err(|e| e.to_string())
}
fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|p| {
        Argon2::default()
            .verify_password(password.as_bytes(), &p)
            .is_ok()
    })
}

// Bootstrap is only callable locally; transaction prevents competing first administrators.
pub fn bootstrap_admin(db: &mut Connection, username: &str, password: &str) -> Result<(), String> {
    if !valid_username(username) {
        return Err("اسم المستخدم غير صالح.".into());
    }
    let hash = hash_password(password)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let count: i64 = tx
        .query_row("SELECT COUNT(*) FROM identity_users", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if count != 0 {
        return Err("تمت تهيئة المستخدمين مسبقًا؛ لم يتغير أي حساب.".into());
    }
    tx.execute(
        "INSERT INTO identity_users(username,password_hash,role) VALUES(?1,?2,'admin')",
        params![username, hash],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}
#[derive(Clone, Debug)]
pub struct Principal {
    pub id: i64,
    pub username: String,
    pub role: String,
    csrf: String,
    token_hash: Vec<u8>,
}
// Recheck session and role inside each financial write transaction.
/// باني مُخصّص للاختبارات: الحقول الخاصة تبقى خاصة خارج الوحدة.
#[cfg(test)]
pub(crate) fn for_test(
    id: i64,
    username: &str,
    role: &str,
    csrf: String,
    token: String,
) -> Principal {
    Principal {
        id,
        username: username.into(),
        role: role.into(),
        csrf,
        token_hash: token_hash(&token),
    }
}
pub fn is_current_admin(db: &Connection, p: &Principal) -> rusqlite::Result<bool> {
    db.query_row("SELECT EXISTS(SELECT 1 FROM identity_sessions s JOIN identity_users u ON u.id=s.user_id WHERE s.token_hash=?1 AND s.user_id=?2 AND s.expires_at>?3 AND u.active=1 AND u.role='admin')",params![p.token_hash,p.id,now()],|r|r.get(0))
}
mod current;
mod session;
#[cfg(test)]
pub use crate::identity_support::test_admin;
pub use current::is_current_user;
use session::{
    SessionCookie, cookie_value, header_token, inspect_cookie, principal, secure_cookie,
};
pub use session::{preview_header_session, preview_partitioned_cookie};
fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    let Ok(uri) = origin.parse::<axum::http::Uri>() else {
        return false;
    };
    let Some(authority) = uri.authority() else {
        return false;
    };
    let Some(host) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) else {
        return false;
    };
    matches!(uri.scheme_str(), Some("http" | "https"))
        && authority.as_str().eq_ignore_ascii_case(host)
        && (uri.path().is_empty() || uri.path() == "/")
        && uri.query().is_none()
}
#[derive(Deserialize)]
pub struct Credentials {
    username: String,
    password: String,
}
// Serializing login attempts avoids concurrent guesses bypassing the persistent cooldown.
pub async fn login(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Credentials>,
) -> Response {
    if !same_origin(&headers) {
        return fail(StatusCode::FORBIDDEN, "مصدر الطلب غير مسموح.");
    }
    if !valid_username(&input.username) || input.password.is_empty() {
        return fail(StatusCode::UNAUTHORIZED, "تعذر تسجيل الدخول.");
    }
    let state = s.clone();
    let result=tokio::task::spawn_blocking(move || -> Result<Option<(String,String,String,String)>,String>{
        let mut db=db_open(&state.db).map_err(|e|e.to_string())?;
        let tx=db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
        let t=now();
        let locked:Option<i64>=tx.query_row("SELECT locked_until FROM identity_attempts WHERE username=?1",[&input.username],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        if locked.is_some_and(|until|until>t){return Ok(None)}
        let user:Option<(i64,String,String,bool)>=tx.query_row("SELECT id,password_hash,role,active FROM identity_users WHERE username=?1",[&input.username],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(|e|e.to_string())?;
        // Verify a real hash even when the supplied name is unknown, to reduce user enumeration.
        let fallback:Option<String>=tx.query_row("SELECT password_hash FROM identity_users ORDER BY id LIMIT 1",[],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        let hash=user.as_ref().map(|x|x.1.as_str()).or(fallback.as_deref());
        let verified=hash.is_some_and(|h|verify_password(&input.password,h));
        let Some((id,_,role,active))=user.filter(|u|u.3&&verified) else {
            tx.execute("INSERT INTO identity_attempts(username,failures,locked_until) VALUES(?1,1,0) ON CONFLICT(username) DO UPDATE SET failures=CASE WHEN locked_until>0 AND locked_until<=?2 THEN 1 ELSE failures+1 END, locked_until=CASE WHEN (CASE WHEN locked_until>0 AND locked_until<=?2 THEN 1 ELSE failures+1 END)>=5 THEN ?2+60 ELSE 0 END",params![input.username,t]).map_err(|e|e.to_string())?;
            tx.commit().map_err(|e|e.to_string())?;return Ok(None)
        };
        let _=active;
        tx.execute("DELETE FROM identity_attempts WHERE username=?1",[&input.username]).map_err(|e|e.to_string())?;
        let token=random_hex()?;let csrf=random_hex()?;
        tx.execute("DELETE FROM identity_sessions WHERE expires_at<=?1",[t]).map_err(|e|e.to_string())?;
        tx.execute("INSERT INTO identity_sessions(token_hash,user_id,csrf,expires_at) VALUES(?1,?2,?3,?4)",params![token_hash(&token),id,csrf,t+SESSION_SECONDS]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e|e.to_string())?;
        Ok(Some((token,csrf,input.username,role)))
    }).await;
    match result {
        Ok(Ok(Some((token, csrf, username, role)))) => {
            let cookie = cookie_value(
                &token,
                SESSION_SECONDS,
                secure_cookie(),
                preview_partitioned_cookie(),
            );
            // تنظيف أي كوكي قديم بالاسم الآخر حتى لا تتكرر الجلسات في المتصفح.
            let stale_name = if preview_partitioned_cookie() {
                "smart_session"
            } else {
                "smart_preview_session"
            };
            let stale = format!("{stale_name}=; Path=/; HttpOnly; Max-Age=0");
            let mut body = serde_json::json!({"username":username,"role":role,"csrf":csrf});
            if preview_header_session() {
                // معاينة فقط: يمكّن العميل من إكمال الجلسة حين يحجب الوسيط الكوكي.
                body["session"] = serde_json::Value::String(token.clone());
            }
            let mut response = Json(body).into_response();
            if let Ok(value) = HeaderValue::from_str(&cookie) {
                response.headers_mut().insert(header::SET_COOKIE, value);
                if let Ok(old) = HeaderValue::from_str(&stale) {
                    response.headers_mut().append(header::SET_COOKIE, old);
                }
            } else {
                return fail(StatusCode::INTERNAL_SERVER_ERROR, "تعذر إنشاء الجلسة.");
            }
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response
        }
        Ok(Ok(None)) => fail(
            StatusCode::UNAUTHORIZED,
            "تعذر تسجيل الدخول أو تم إيقاف المحاولات مؤقتًا.",
        ),
        Ok(Err(e)) => {
            tracing::error!(%e,"identity login storage failed");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "تعذر تسجيل الدخول؛ راجع سجل الخادم.",
            )
        }
        Err(e) => {
            tracing::error!(%e,"identity login task failed");
            fail(StatusCode::INTERNAL_SERVER_ERROR, "تعذر تسجيل الدخول.")
        }
    }
}

pub async fn guard(State(s): State<AppState>, mut req: Request, next: Next) -> Response {
    if !same_origin(req.headers()) {
        return fail(StatusCode::FORBIDDEN, "مصدر الطلب غير مسموح.");
    }
    if s.preview_no_login {
        if s.experimental_posting {
            return fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "لا يسمح بالترحيل في المعاينة دون دخول.",
            );
        }
        if req.uri().path() == "/api/auth/logout" {
            return fail(
                StatusCode::FORBIDDEN,
                "المعاينة مفتوحة ولا توجد جلسة لإنهائها.",
            );
        }
        req.extensions_mut().insert(Principal {
            id: 0,
            username: "معاينة مفتوحة".into(),
            role: "admin".into(),
            csrf: String::new(),
            token_hash: Vec::new(),
        });
        return next.run(req).await;
    }
    let user = match db_open(&s.db).and_then(|db| principal(&db, req.headers())) {
        Ok(Some(p)) => p,
        Ok(None) => {
            let mut response = fail(StatusCode::UNAUTHORIZED, "يلزم تسجيل الدخول.");
            // Only a classification, never the cookie, token, username, or other credentials.
            if req.uri().path() == "/api/auth/me" {
                let reason = match inspect_cookie(req.headers()) {
                    SessionCookie::Missing if header_token(req.headers()).is_some() => {
                        "not-current"
                    }
                    SessionCookie::Missing => "missing",
                    SessionCookie::Invalid => "invalid",
                    SessionCookie::Valid(_) => "not-current",
                };
                response
                    .headers_mut()
                    .insert("x-smart-session-state", HeaderValue::from_static(reason));
            }
            return response;
        }
        Err(e) => {
            tracing::error!(%e,"identity session lookup failed");
            return fail(StatusCode::INTERNAL_SERVER_ERROR, "تعذر التحقق من الجلسة.");
        }
    };
    let write = !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if write {
        if req
            .headers()
            .get("x-csrf-token")
            .and_then(|h| h.to_str().ok())
            != Some(user.csrf.as_str())
        {
            return fail(StatusCode::FORBIDDEN, "رمز حماية الطلب مفقود أو غير صالح.");
        }
        if user.role != "admin" && req.uri().path() != "/api/auth/logout" {
            return fail(StatusCode::FORBIDDEN, "لا تملك صلاحية تنفيذ هذه العملية.");
        }
    }
    req.extensions_mut().insert(user);
    next.run(req).await
}
pub async fn me(
    State(s): State<AppState>,
    axum::Extension(user): axum::Extension<Principal>,
) -> Response {
    let mut response = Json(serde_json::json!({"username":user.username,"role":user.role,"csrf":user.csrf,"preview_no_login":s.preview_no_login})).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
pub async fn logout(
    State(s): State<AppState>,
    axum::Extension(user): axum::Extension<Principal>,
) -> Response {
    match db_open(&s.db).and_then(|db| {
        db.execute(
            "DELETE FROM identity_sessions WHERE token_hash=?1",
            [user.token_hash],
        )
    }) {
        Ok(_) => {
            let cookie = cookie_value("", 0, secure_cookie(), preview_partitioned_cookie());
            let stale_name = if preview_partitioned_cookie() {
                "smart_session"
            } else {
                "smart_preview_session"
            };
            let stale = format!("{stale_name}=; Path=/; HttpOnly; Max-Age=0");
            let mut response = Json(serde_json::json!({"status":"signed_out"})).into_response();
            if let Ok(value) = HeaderValue::from_str(&cookie) {
                response.headers_mut().insert(header::SET_COOKIE, value);
                if let Ok(old) = HeaderValue::from_str(&stale) {
                    response.headers_mut().append(header::SET_COOKIE, old);
                }
            }
            response
        }
        Err(e) => {
            tracing::error!(%e,"logout failed");
            fail(StatusCode::INTERNAL_SERVER_ERROR, "تعذر إنهاء الجلسة.")
        }
    }
}

#[cfg(test)]
mod tests;
