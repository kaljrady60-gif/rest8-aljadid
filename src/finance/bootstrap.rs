//! تهيئة أول شركة بعد إنشاء المدير: دليل حسابات أساسي + فرع + فترة مفتوحة + إسناد.
//! تُستدعى مرة واحدة عبر `--init-company`، ولا تعمل إن كانت الشركات موجودة.
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

/// دليل حسابات أساسي: (الرمز، الاسم، التصنيف، رئيسي/تفصيلي).
const CHART: &[(&str, &str, &str, bool)] = &[
    ("1000", "الأصول", "asset", true),
    ("1100", "النقدية والبنوك", "asset", true),
    ("1110", "الصندوق الرئيسي", "asset", false),
    ("1120", "البنك التجاري", "asset", false),
    ("1200", "العملاء", "asset", false),
    ("1300", "المخزون", "asset", false),
    ("2000", "الالتزامات", "liability", true),
    ("2100", "الموردون", "liability", false),
    ("2200", "رواتب مستحقة", "liability", false),
    ("3000", "حقوق الملكية", "equity", true),
    ("3100", "رأس المال", "equity", false),
    ("3200", "الأرباح المحتجزة", "equity", false),
    ("4000", "الإيرادات", "revenue", true),
    ("4100", "إيرادات المبيعات", "revenue", false),
    ("4900", "إيرادات أخرى", "revenue", false),
    ("5000", "المصروفات", "expense", true),
    ("5100", "الإيجار", "expense", false),
    ("5200", "الرواتب", "expense", false),
    ("5300", "مصروفات تشغيلية", "expense", false),
];

pub fn first_company(db: &mut Connection, name: &str) -> Result<i64, String> {
    let clean = name.trim();
    if clean.is_empty() || clean.chars().count() > 120 {
        return Err("اسم الشركة مطلوب حتى 120 حرفًا.".into());
    }
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let existing: Option<i64> = tx
        .query_row(
            "SELECT id FROM finance_companies ORDER BY id LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(id) = existing {
        return Err(format!("توجد شركة مسبقًا (رقم {id})؛ لم يتغير شيء."));
    }
    let admin: i64 = tx
        .query_row(
            "SELECT id FROM identity_users WHERE role='admin' ORDER BY id LIMIT 1",
            [],
            |r| r.get(0),
        )
        .map_err(|_| "أنشئ المدير أولًا عبر --init-admin.".to_string())?;
    tx.execute(
        "INSERT INTO finance_companies(code,name) VALUES('C1',?1)",
        params![clean],
    )
    .map_err(|e| e.to_string())?;
    let company_id = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO finance_branches(company_id,code,name) VALUES(?1,'HQ','الفرع الرئيسي')",
        params![company_id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO finance_company_admins(company_id,user_id,granted_by) VALUES(?1,?2,?2)",
        params![company_id, admin],
    )
    .map_err(|e| e.to_string())?;
    for (index, (code, label, category, group)) in CHART.iter().enumerate() {
        let kind = if *group { "group" } else { "detail" };
        let nature = if matches!(*category, "asset" | "expense") {
            "debit"
        } else {
            "credit"
        };
        tx.execute(
            "INSERT INTO accounts(code,name,category,kind,nature,currency)
             VALUES(?1,?2,?3,?4,?5,'YER')",
            params![format!("__c1_{code}"), label, category, kind, nature],
        )
        .map_err(|e| e.to_string())?;
        let account_id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO finance_account_scopes(company_id,account_id) VALUES(?1,?2)",
            params![company_id, account_id],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO finance_company_account_codes(company_id,account_id,code) VALUES(?1,?2,?3)",
            params![company_id, account_id, code],
        )
        .map_err(|e| e.to_string())?;
        let _ = index;
    }
    let cash: i64 = tx
        .query_row(
            "SELECT account_id FROM finance_company_account_codes WHERE company_id=?1 AND code='1110'",
            params![company_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO finance_funds(company_id,branch_id,kind,code,name,account_id,currency,opened_on)
         VALUES(?1,1,'cash','CASH-1','الصندوق الرئيسي',?2,'YER',date('now'))",
        params![company_id, cash],
    )
    .map_err(|e| e.to_string())?;
    let year = chrono::Utc::now().format("%Y").to_string();
    tx.execute(
        "INSERT INTO financial_periods(name,start_date,end_date,status) VALUES(?1,?2,?3,'open')",
        params![
            format!("سنة {year}"),
            format!("{year}-01-01"),
            format!("{year}-12-31")
        ],
    )
    .map_err(|e| e.to_string())?;
    let period_id = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO finance_period_context(period_id,company_id) VALUES(?1,?2)",
        params![period_id, company_id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO finance_dimensions(company_id,code,name,kind) VALUES(?1,'CC-1','الإدارة العامة','cost_center')",
        params![company_id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO finance_audit_log(company_id,actor_id,action,entity,entity_id,payload)
         VALUES(?1,?2,'company.bootstrap','finance_companies',?3,'{\"chart\":19}')",
        params![company_id, admin, company_id.to_string()],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(company_id)
}
