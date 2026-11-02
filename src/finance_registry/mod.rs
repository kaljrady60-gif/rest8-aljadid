//! Explicit organization and financial registry; legacy accounts are never assigned automatically.
use crate::{AppState, db_open, fail, identity::Principal};
use axum::{
    Extension, Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use rusqlite::{Connection, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

pub(crate) mod access;
mod grants;
mod read;
mod schema;
#[cfg(test)]
mod tests;
mod write;

pub fn init_schema(db: &Connection) -> rusqlite::Result<()> {
    schema::init(db)
}
pub use grants::change_grant;
pub use read::list;
pub use write::create;

#[derive(Debug)]
enum Error {
    Invalid(&'static str),
    Unauthorized,
    Conflict,
    Storage(rusqlite::Error),
}
impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error)
    }
}
fn error_response(error: Error) -> Response {
    match error {
        Error::Invalid(message) => fail(StatusCode::UNPROCESSABLE_ENTITY, message),
        Error::Unauthorized => fail(StatusCode::FORBIDDEN, "انتهت جلسة المدير أو أُلغيت صلاحيتها."),
        Error::Conflict => fail(
            StatusCode::CONFLICT,
            "الرمز أو الحساب مستخدم بالفعل أو يتعارض مع نطاق آخر.",
        ),
        Error::Storage(e) => {
            tracing::error!(%e, "finance registry storage failed");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "تعذر حفظ أو قراءة البيانات؛ راجع سجل الخادم.",
            )
        }
    }
}
fn text(value: &str, limit: usize) -> Result<&str, Error> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > limit || value.chars().any(char::is_control) {
        Err(Error::Invalid(
            "الاسم أو الرمز مطلوب ولا يقبل محارف التحكم أو الطول الزائد.",
        ))
    } else {
        Ok(value)
    }
}
