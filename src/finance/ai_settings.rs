//! إعدادات مزوّد قراءة الصور: تُحفظ لكل شركة، ويختارها المستخدم بنفسه.
//! المفتاح السحابي لا يُعاد للواجهة أبدًا — تُعاد فقط حالة «مضبوط/غير مضبوط».
use super::invoice_ai::{Config, Mode};
use super::scope::{FinanceError, Scope, error_response};
use crate::{AppState, db_open};
use axum::Json;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;

pub fn init(db: &rusqlite::Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS finance_ai_settings (\
        company_id INTEGER PRIMARY KEY,\
        mode TEXT NOT NULL DEFAULT 'hybrid' CHECK(mode IN ('local','cloud','hybrid')),\
        order_json TEXT NOT NULL DEFAULT '[\"local\",\"cloud\"]',\
        local_endpoint TEXT NOT NULL DEFAULT 'http://127.0.0.1:11434',\
        local_model TEXT NOT NULL DEFAULT 'llava',\
        cloud_provider TEXT NOT NULL DEFAULT 'gemini',\
        cloud_model TEXT NOT NULL DEFAULT 'gemini-2.5-flash',\
        cloud_key TEXT NOT NULL DEFAULT '',\
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);",
    )
}

/// يقرأ إعدادات الشركة إن وُجدت، وإلا القيم الافتراضية من البيئة.
pub fn load(db: &rusqlite::Connection, company_id: i64) -> (Config, bool) {
    let row = db
        .query_row(
            "SELECT mode,order_json,local_endpoint,local_model,cloud_provider,cloud_model,cloud_key
             FROM finance_ai_settings WHERE company_id=?1",
            [company_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                ))
            },
        )
        .optional();
    let Ok(Some((mode, order_json, endpoint, local_model, provider, cloud_model, key))) = row
    else {
        return (Config::default(), false);
    };
    let order: Vec<String> = serde_json::from_str(&order_json).unwrap_or_else(|_| {
        vec!["local".to_string(), "cloud".to_string()]
    });
    let order: Vec<String> = order
        .into_iter()
        .filter(|slot| slot == "local" || slot == "cloud")
        .collect();
    let mut config = Config::default();
    config.mode = Mode::parse(&mode);
    config.order = if order.is_empty() {
        vec!["local".into(), "cloud".into()]
    } else {
        order
    };
    config.local_endpoint = endpoint.trim_end_matches('/').to_string();
    config.local_model = local_model;
    config.cloud_provider = provider;
    config.cloud_model = cloud_model;
    if !key.trim().is_empty() {
        config.cloud_key = key;
    }
    (config, true)
}

#[derive(Deserialize)]
pub struct CompanyQuery {
    company_id: i64,
}

#[derive(Deserialize)]
pub struct ModelsQuery {
    company_id: i64,
    scope: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveSettings {
    company_id: i64,
    mode: String,
    #[serde(default)]
    order: Vec<String>,
    #[serde(default)]
    local_endpoint: Option<String>,
    #[serde(default)]
    local_model: Option<String>,
    #[serde(default)]
    cloud_provider: Option<String>,
    #[serde(default)]
    cloud_model: Option<String>,
    /// يُمرَّر مرة واحدة عند الإضافة؛ الفراغ يعني «أبقِ المفتاح الحالي».
    #[serde(default)]
    cloud_key: Option<String>,
}

pub async fn get(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<CompanyQuery>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let (config, saved) = load(&db, scope.company_id);
        Ok(super::invoice_ai::status(&config, saved))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

pub async fn save(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Json(input): Json<SaveSettings>,
) -> Response {
    let result = (|| -> Result<serde_json::Value, FinanceError> {
        if !["local", "cloud", "hybrid"].contains(&input.mode.as_str()) {
            return Err(FinanceError::Invalid(
                "النمط: local أو cloud أو hybrid.".into(),
            ));
        }
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, input.company_id)?;
        scope.require_admin()?;
        let (mut config, _) = load(&db, scope.company_id);

        let order: Vec<String> = input
            .order
            .iter()
            .filter(|slot| *slot == "local" || *slot == "cloud")
            .cloned()
            .collect();
        // الهجين يحتاج ترتيبًا صالحًا؛ وإلا نُكمل الناقص تلقائيًا بدل رفض الحفظ.
        let order = if order.is_empty() {
            vec!["local".to_string(), "cloud".to_string()]
        } else {
            let mut full = order.clone();
            for slot in ["local", "cloud"] {
                if !full.iter().any(|s| s == slot) {
                    full.push(slot.to_string());
                }
            }
            full
        };

        config.mode = Mode::parse(&input.mode);
        config.order = order.clone();
        if let Some(endpoint) = input.local_endpoint.as_deref().map(str::trim) {
            if !endpoint.is_empty() {
                config.local_endpoint = endpoint.trim_end_matches('/').to_string();
            }
        }
        if let Some(model) = input.local_model.as_deref().map(str::trim) {
            if !model.is_empty() {
                config.local_model = model.to_string();
            }
        }
        if let Some(provider) = input.cloud_provider.as_deref().map(str::trim) {
            if !provider.is_empty() {
                config.cloud_provider = provider.to_string();
            }
        }
        if let Some(model) = input.cloud_model.as_deref().map(str::trim) {
            if !model.is_empty() {
                config.cloud_model = model.to_string();
            }
        }
        if let Some(key) = input.cloud_key.as_deref().map(str::trim) {
            if !key.is_empty() {
                config.cloud_key = key.to_string();
            }
        }

        db.execute(
            "INSERT INTO finance_ai_settings
               (company_id,mode,order_json,local_endpoint,local_model,cloud_provider,cloud_model,cloud_key,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,CURRENT_TIMESTAMP)
             ON CONFLICT(company_id) DO UPDATE SET
               mode=excluded.mode, order_json=excluded.order_json,
               local_endpoint=excluded.local_endpoint, local_model=excluded.local_model,
               cloud_provider=excluded.cloud_provider, cloud_model=excluded.cloud_model,
               cloud_key=excluded.cloud_key, updated_at=CURRENT_TIMESTAMP",
            params![
                scope.company_id,
                config.mode.code(),
                serde_json::to_string(&order).unwrap_or_else(|_| "[]".into()),
                config.local_endpoint,
                config.local_model,
                config.cloud_provider,
                config.cloud_model,
                config.cloud_key
            ],
        )?;
        Ok(super::invoice_ai::status(&config, true))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(e),
    }
}

/// النماذج الفعلية المتاحة الآن — من الخدمة نفسها لا من قائمة ثابتة.
pub async fn models(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ModelsQuery>,
) -> Response {
    let config = (|| -> Result<Config, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let (config, _) = load(&db, scope.company_id);
        Ok(config)
    })();
    let config = match config {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };
    match super::invoice_ai::models(&q.scope, config, None).await {
        Ok(models) => Json(serde_json::json!({"scope": q.scope, "models": models})).into_response(),
        Err(reason) => (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"scope": q.scope, "models": [], "error": reason})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct ProbeQuery {
    company_id: i64,
    scope: String,
}

/// «اختبر الاتصال» — لا يقرأ صورة، فقط يتحقق أن المزوّد يستجيب فعلًا.
pub async fn probe(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<crate::identity::Principal>,
    Query(q): Query<ProbeQuery>,
) -> Response {
    let config = (|| -> Result<Config, FinanceError> {
        let db = db_open(&state.db)?;
        let scope = Scope::resolve(&state, &db, &actor, q.company_id)?;
        let (config, _) = load(&db, scope.company_id);
        Ok(config)
    })();
    let config = match config {
        Ok(c) => c,
        Err(e) => return error_response(e),
    };
    match super::invoice_ai::probe(&q.scope, config).await {
        Ok(message) => Json(serde_json::json!({"ok": true, "message": message})).into_response(),
        Err(reason) => Json(serde_json::json!({"ok": false, "message": reason})).into_response(),
    }
}
