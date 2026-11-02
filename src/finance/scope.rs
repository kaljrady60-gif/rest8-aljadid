//! نطاق الشركة/الفرع: المصدر الوحيد لتصريح الوصول في كل مسارات المالية.
//!
//! كل معالج مالي يبدأ بـ `Scope::resolve` فلا تتكرر شروط الصلاحية في الملفات.
use crate::AppState;
use crate::finance_registry::access;
use crate::identity::Principal;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

#[derive(Debug)]
pub enum FinanceError {
    Forbidden,
    NotFound(&'static str),
    Invalid(String),
    Conflict(&'static str),
    Storage(rusqlite::Error),
}

impl From<rusqlite::Error> for FinanceError {
    fn from(value: rusqlite::Error) -> Self {
        FinanceError::Storage(value)
    }
}

#[derive(Serialize)]
struct Body {
    error: String,
}

pub fn error_response(e: FinanceError) -> Response {
    match e {
        FinanceError::Forbidden => (
            StatusCode::FORBIDDEN,
            axum::Json(Body {
                error: "لا يوجد إسناد يسمح بهذه العملية.".into(),
            }),
        )
            .into_response(),
        FinanceError::NotFound(what) => (
            StatusCode::NOT_FOUND,
            axum::Json(Body {
                error: format!("{what} غير موجود."),
            }),
        )
            .into_response(),
        FinanceError::Invalid(message) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            axum::Json(Body { error: message }),
        )
            .into_response(),
        FinanceError::Conflict(message) => (
            StatusCode::CONFLICT,
            axum::Json(Body {
                error: message.into(),
            }),
        )
            .into_response(),
        FinanceError::Storage(e) => {
            tracing::error!(%e, "finance storage failure");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(Body {
                    error: "تعذّر تنفيذ العملية المالية.".into(),
                }),
            )
                .into_response()
        }
    }
}

/// صلاحية محسوبة مسبقًا: الشركة المعتمدة + هل المستخدم مدير مالي فيها.
#[derive(Debug, Clone, Copy)]
pub struct Scope {
    pub company_id: i64,
    pub is_admin: bool,
}

impl Scope {
    /// يتحقق من أن: الجلسة سارية، الشركة موجودة، وللمستخدم إسناد قراءة عليها.
    pub fn resolve(
        state: &AppState,
        db: &Connection,
        actor: &Principal,
        company_id: i64,
    ) -> Result<Scope, FinanceError> {
        if company_id <= 0 {
            return Err(FinanceError::Invalid("شركة غير صالحة.".into()));
        }
        if !state.preview_no_login && !crate::identity::is_current_user(db, actor)? {
            return Err(FinanceError::Forbidden);
        }
        let exists: Option<i64> = db
            .query_row(
                "SELECT id FROM finance_companies WHERE id=?1",
                [company_id],
                |r| r.get(0),
            )
            .optional()?;
        if exists.is_none() {
            return Err(FinanceError::NotFound("الشركة"));
        }
        if state.preview_no_login {
            return Ok(Scope {
                company_id,
                is_admin: true,
            });
        }
        let can_read = access::company_read(db, actor.id, company_id)?;
        if !can_read {
            return Err(FinanceError::Forbidden);
        }
        let is_admin = access::company_admin(db, actor.id, company_id)?;
        Ok(Scope {
            company_id,
            is_admin,
        })
    }

    pub fn require_admin(&self) -> Result<(), FinanceError> {
        if self.is_admin {
            Ok(())
        } else {
            Err(FinanceError::Forbidden)
        }
    }

    /// هل الحساب تابع لهذه الشركة؟
    pub fn owns_account(&self, db: &Connection, account_id: i64) -> Result<bool, FinanceError> {
        let owned: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM finance_account_scopes WHERE company_id=?1 AND account_id=?2)",
            params![self.company_id, account_id],
            |r| r.get(0),
        )?;
        Ok(owned)
    }
}
