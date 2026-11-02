#![deny(clippy::disallowed_types)]
mod accounts;
mod domain;
mod drafts;
mod finance;
mod finance_registry;
mod generated;
mod identity;
#[cfg(test)]
mod identity_support;
mod ledger;
mod periods;
mod vouchers;
use askama::Template;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use drafts::{SaveError, create, draft_detail, list, read_draft};
use rusqlite::Connection;
#[cfg(test)]
use rusqlite::params;
use serde::Serialize;
use std::{path::PathBuf, sync::Arc};
use tower_http::services::ServeDir;

#[derive(Clone)]
struct AppState {
    db: Arc<PathBuf>,
    experimental_posting: bool,
    preview_no_login: bool,
}
#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    currencies: &'static [generated::contracts::CurrencySpec],
}
#[derive(Serialize)]
struct ApiError {
    error: String,
}
fn fail(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(ApiError { error: msg.into() })).into_response()
}
fn db_open(path: &PathBuf) -> Result<Connection, rusqlite::Error> {
    let db = Connection::open(path)?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    db.pragma_update(None, "foreign_keys", "ON")?;
    Ok(db)
}
fn init_db(path: &PathBuf) -> Result<(), rusqlite::Error> {
    let mut db = db_open(path)?;
    db.execute_batch("PRAGMA journal_mode=WAL;")?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS drafts (id INTEGER PRIMARY KEY, description TEXT NOT NULL, date TEXT NOT NULL, currency TEXT NOT NULL, total TEXT NOT NULL, lines_json TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP); CREATE TABLE IF NOT EXISTS accounts (id INTEGER PRIMARY KEY, code TEXT NOT NULL UNIQUE, name TEXT NOT NULL, parent_id INTEGER REFERENCES accounts(id), category TEXT NOT NULL, kind TEXT NOT NULL, nature TEXT NOT NULL, currency TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);")?;
    let has_count: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('drafts') WHERE name='lines_count')",
        [],
        |r| r.get(0),
    )?;
    if !has_count {
        tx.execute_batch("ALTER TABLE drafts ADD COLUMN lines_count INTEGER;")?;
    }
    tx.execute_batch("CREATE TABLE IF NOT EXISTS draft_lines (draft_id INTEGER NOT NULL REFERENCES drafts(id) ON DELETE RESTRICT, line_no INTEGER NOT NULL CHECK(line_no>=0), account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT, account_snapshot TEXT NOT NULL, debit TEXT NOT NULL, credit TEXT NOT NULL, PRIMARY KEY(draft_id,line_no));")?;
    identity::init_schema(&tx)?;
    ledger::init_schema(&tx)?;
    periods::init_schema(&tx)?;
    vouchers::init_schema(&tx)?;
    // الترتيب مهم: سجل المالية يُنشئ جداول الشركات/الفروع/الصناديق
    // ثم يوسّعها نطاق المالية بأعمدة إضافية.
    finance_registry::init_schema(&tx)?;
    finance::init_schema(&tx)?;
    tx.commit()
}

async fn index() -> Response {
    match (IndexTemplate {
        currencies: generated::contracts::CURRENCIES,
    })
    .render()
    {
        Ok(s) => Html(s).into_response(),
        Err(e) => {
            tracing::error!(%e,"template failure");
            fail(StatusCode::INTERNAL_SERVER_ERROR, "تعذر عرض الصفحة.")
        }
    }
}
async fn health(State(s): State<AppState>) -> Response {
    let result = db_open(&s.db).and_then(|db| {
        db.query_row("SELECT COUNT(*) FROM drafts", [], |row| {
            row.get::<_, i64>(0)
        })
    });
    match result {
        Ok(_) => Json(serde_json::json!({"status":"ok", "database":"reachable", "mode":"development", "posting":if s.experimental_posting {"experimental_only"} else {"disabled"}, "authentication":"local_sessions"})).into_response(),
        Err(e) => {
            tracing::error!(%e, "health check failed");
            (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"status":"error", "database":"unavailable", "mode":"development"}))).into_response()
        }
    }
}
async fn headers(req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let path = req.uri().path();
    let no_store = path.starts_with("/api/") || path == "/" || path == "/assets/auth.js";
    let mut r = next.run(req).await;
    let h = r.headers_mut();
    if no_store {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    h.insert(header::CONTENT_SECURITY_POLICY,HeaderValue::from_static("default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; connect-src 'self'"));
    r
}
// المسارات القديمة مخدومة فعليًا (لا حجب 503)؛ البديل المعزول هو /api/finance/*.
// الحماية تأتي من صلاحية الجلسة ومن الإسقاط بالشركة داخل كل معالج.

fn app(state: AppState) -> Router {
    // كل مسار مالي محمي بالجلسة؛ المالية المعزولة في /api/finance/*،
    // والمسارات القديمة تبقى للتوافق مع الواجهات القائمة.
    let legacy = Router::new()
        // gates: allow legacy-api
        .route("/api/drafts", get(list).post(create))
        .route("/api/drafts/{id}", get(draft_detail))
        // gates: allow legacy-api
        .route("/api/vouchers", get(vouchers::list).post(vouchers::create))
        // gates: allow legacy-api
        .route("/api/periods", get(periods::list).post(periods::create))
        .route("/api/periods/{id}/events", get(periods::events))
        .route(
            "/api/periods/{id}/transition",
            axum::routing::post(periods::transition),
        )
        // gates: allow legacy-api
        .route("/api/ledger/post/{id}", axum::routing::post(ledger::post))
        // gates: allow legacy-api
        .route("/api/ledger/trial", get(ledger::trial))
        // gates: allow legacy-api
        .route("/api/ledger/entries", get(ledger::entries))
        // gates: allow legacy-api
        .route("/api/ledger/accounts/{id}", get(ledger::account_ledger))
        // gates: allow legacy-api
        .route("/api/accounts", get(accounts::list).post(accounts::create))
        .route("/api/accounts/{id}", axum::routing::put(accounts::rename));
    let protected = Router::new()
        .merge(finance::router())
        .merge(legacy)
        .route(
            "/api/vouchers/linked",
            axum::routing::post(vouchers::create_linked),
        )
        .route(
            "/api/finance/registry",
            get(finance_registry::list).post(finance_registry::create),
        )
        .route(
            "/api/finance/grants",
            axum::routing::post(finance_registry::change_grant),
        )
        .route(
            "/api/finance/accounts",
            get(accounts::list_scoped).post(accounts::create_scoped),
        )
        .route("/api/finance/vouchers", get(vouchers::scoped_list))
        .route("/api/finance/vouchers/{id}", get(vouchers::scoped_detail))
        .route(
            "/api/finance/periods",
            get(periods::list_scoped).post(periods::create_scoped),
        )
        .route(
            "/api/finance/periods/{id}/transition",
            axum::routing::post(periods::transition_scoped),
        )
        // gates: allow legacy-api
        .route("/api/ledger/config", get(ledger::config))
        .route("/api/auth/me", get(identity::me))
        .route("/api/auth/logout", axum::routing::post(identity::logout))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            identity::guard,
        ));
    Router::new()
        .route("/", get(index))
        .route("/api/health", get(health))
        .route("/api/auth/login", axum::routing::post(identity::login))
        .merge(protected)
        .nest_service("/assets", ServeDir::new("assets"))
        .layer(axum::middleware::from_fn(headers))
        .with_state(state)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let preview_no_login = std::env::var("SMART_PREVIEW_NO_LOGIN").as_deref() == Ok("1");
    if preview_no_login
        && (std::env::var_os("SMART_DB").is_some()
            || std::env::var("SMART_EXPERIMENTAL_POSTING").as_deref() == Ok("1"))
    {
        panic!("unauthenticated preview cannot use SMART_DB or enable posting");
    }
    let path = if preview_no_login {
        // Unique fresh temporary DB. Never reuse the user's real database in open preview mode.
        let mut random = [0u8; 16];
        // gates: allow unwrap
        getrandom::fill(&mut random).expect("preview database randomness unavailable");
        let suffix: String = random.iter().map(|b| format!("{b:02x}")).collect();
        let path = std::env::temp_dir().join(format!("smart-accountant-preview-{suffix}.sqlite"));
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            // gates: allow unwrap
            .expect("cannot create fresh preview database");
        path
    } else {
        PathBuf::from(std::env::var("SMART_DB").unwrap_or_else(|_| "data/smart.sqlite".into()))
    };
    if let Some(p) = path.parent() {
        // gates: allow unwrap
        std::fs::create_dir_all(p).expect("cannot create database directory");
    }
    // gates: allow unwrap
    init_db(&path).expect("cannot initialize sqlite database");
    if preview_no_login {
        // قاعدة العرض مؤقتة وتُمسح: بذر شركة تجريبية لا يمسّ بيانات المستخدم.
        // gates: allow unwrap
        let mut demo = db_open(&path).expect("preview database failed");
        // gates: allow unwrap
        finance::demo::seed(&mut demo).expect("preview demo seed failed");
    }
    if std::env::args().nth(1).as_deref() == Some("--init-admin") {
        use std::io::{self, IsTerminal, Write};
        if !io::stdin().is_terminal() {
            eprintln!("يجب تشغيل التهيئة محليًا من طرفية تفاعلية.");
            std::process::exit(1)
        }
        print!("اسم المدير: ");
        // gates: allow unwrap
        io::stdout().flush().expect("flush failed");
        let mut username = String::new();
        // gates: allow unwrap
        io::stdin().read_line(&mut username).expect("read failed");
        // gates: allow unwrap
        let password = rpassword::prompt_password("كلمة المرور: ").expect("password read failed");
        let confirm =
            // gates: allow unwrap
            rpassword::prompt_password("أعد كلمة المرور: ").expect("password read failed");
        if password != confirm {
            eprintln!("كلمتا المرور غير متطابقتين.");
            std::process::exit(1)
        }
        // gates: allow unwrap
        let mut db = db_open(&path).expect("database failed");
        // سياسة الطول من الملف الموحّد، مع تجاوز صريح للتطوير المحلي فقط.
        let min = generated::contracts::security::PASSWORD_MIN_CHARS as usize;
        let weak_ok = std::env::var("SMART_DEV_WEAK_PASSWORD").as_deref() == Ok("1");
        if !weak_ok && password.chars().count() < min {
            eprintln!(
                "كلمة المرور قصيرة: الحد الأدنى {min} حرفًا. للتطوير المحلي فقط: SMART_DEV_WEAK_PASSWORD=1"
            );
            std::process::exit(1)
        }
        match identity::bootstrap_admin(&mut db, username.trim_end_matches(['\r', '\n']), &password)
        {
            Ok(()) => println!("تم إنشاء المدير المحلي."),
            Err(e) => {
                eprintln!("تعذرت التهيئة: {e}");
                std::process::exit(1)
            }
        }
        return;
    }
    // تهيئة شركة أولى للمدير المحلي: شركة + فرع + دليل حسابات + فترة مفتوحة + إسناد.
    if std::env::args().nth(1).as_deref() == Some("--init-company") {
        let name = std::env::args()
            .nth(2)
            .unwrap_or_else(|| "الشركة الأولى".into());
        // gates: allow unwrap
        let mut db = db_open(&path).expect("database failed");
        match finance::bootstrap::first_company(&mut db, &name) {
            Ok(id) => println!("تم إنشاء الشركة رقم {id} وإسناد المدير إليها."),
            Err(e) => {
                eprintln!("تعذر إنشاء الشركة: {e}");
                std::process::exit(1)
            }
        }
        return;
    }
    // بذر بيانات تشغيلية واقعية للشركة الأولى (قيود مرحّلة + صناديق + صرف + تسوية).
    let seed_arg = std::env::args().nth(1);
    if matches!(
        seed_arg.as_deref(),
        Some("--seed-demo") | Some("--seed-more")
    ) {
        // gates: allow unwrap
        let mut db = db_open(&path).expect("database failed");
        let outcome = if seed_arg.as_deref() == Some("--seed-more") {
            finance::seed::seed_augment(&mut db)
        } else {
            finance::seed::seed_demo(&mut db)
        };
        match outcome {
            Ok(r) => println!(
                "تم بذر الشركة {}: {} قيدًا مرحّلًا · {} حسابًا جديدًا · {} صندوقًا · {} سعر صرف · {} مركز تكلفة · {} سطر افتتاحي",
                r.company,
                r.entries,
                r.accounts_added,
                r.funds_added,
                r.rates_added,
                r.dimensions_added,
                r.opening_lines
            ),
            Err(e) => {
                eprintln!("تعذر البذر: {e}");
                std::process::exit(1)
            }
        }
        return;
    }
    if identity::preview_partitioned_cookie()
        && std::env::var("SMART_SECURE_COOKIE").as_deref() != Ok("1")
    {
        panic!("SMART_PREVIEW_PARTITIONED_COOKIE requires SMART_SECURE_COOKIE=1 and HTTPS");
    }
    let state = AppState {
        db: Arc::new(path),
        experimental_posting: std::env::var("SMART_EXPERIMENTAL_POSTING").as_deref() != Ok("0"),
        preview_no_login,
    };
    let posting_enabled = state.experimental_posting;
    let app = app(state);
    let addr = std::env::var("SMART_BIND").unwrap_or_else(|_| "127.0.0.1:3000".into());
    // gates: allow unwrap
    let address: std::net::SocketAddr = addr.parse().expect("SMART_BIND must be a socket address");
    if !address.ip().is_loopback() && std::env::var("SMART_SECURE_COOKIE").as_deref() != Ok("1") {
        panic!("non-loopback requires SMART_SECURE_COOKIE=1 and an HTTPS reverse proxy")
    }
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        // gates: allow unwrap
        .expect("bind failed");
    tracing::warn!(%addr, experimental_posting=posting_enabled,"development only; local identity is incomplete; do not use real data");
    // gates: allow unwrap
    axum::serve(listener, app).await.expect("server failed");
}

#[cfg(test)]
mod api_tests;
