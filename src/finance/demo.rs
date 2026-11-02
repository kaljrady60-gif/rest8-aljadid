//! بيانات العرض التجريبي: تُنشأ فقط في وضع المعاينة المفتوحة (قاعدة مؤقتة تُمسح).
//! لا تُستدعى أبدًا مع قاعدة حقيقية — الحماية في `main::main`.
use rusqlite::{Connection, params};

pub const DEMO_COMPANY_ID: i64 = 1;

/// (البيان، التاريخ، المبلغ، سطور (رمز الحساب، مدين، دائن)).
type DemoLine<'a> = (&'a str, &'a str, &'a str);
type DemoEntry<'a> = (&'a str, &'a str, &'a str, &'a [DemoLine<'a>]);

/// دليل حسابات مختصر + صندوق + مركز تكلفة + فترة مفتوحة، كافية لتجربة المالية.
pub fn seed(db: &mut Connection) -> rusqlite::Result<()> {
    let tx = db.transaction()?;
    tx.execute(
        "INSERT OR IGNORE INTO finance_companies(id,code,name) VALUES(?1,'DEMO','الشركة التجريبية')",
        params![DEMO_COMPANY_ID],
    )?;
    tx.execute(
        "INSERT OR IGNORE INTO finance_branches(company_id,code,name) VALUES(?1,'HQ','الفرع الرئيسي')",
        params![DEMO_COMPANY_ID],
    )?;
    let chart: &[(&str, &str, &str)] = &[
        ("1000", "الأصول", "asset"),
        ("1100", "النقدية والبنوك", "asset"),
        ("1110", "الصندوق الرئيسي", "asset"),
        ("1120", "البنك التجاري", "asset"),
        ("1200", "العملاء", "asset"),
        ("2000", "الالتزامات", "liability"),
        ("2100", "الموردون", "liability"),
        ("3000", "حقوق الملكية", "equity"),
        ("3100", "رأس المال", "equity"),
        ("4000", "الإيرادات", "revenue"),
        ("4100", "إيرادات المبيعات", "revenue"),
        ("5000", "المصروفات", "expense"),
        ("5100", "مصروف الإيجار", "expense"),
        ("5200", "مصروف الرواتب", "expense"),
    ];
    let mut cash_id = 0i64;
    for (code, name, category) in chart {
        let exists: Option<i64> = tx
            .query_row(
                "SELECT account_id FROM finance_company_account_codes WHERE company_id=?1 AND code=?2",
                params![DEMO_COMPANY_ID, code],
                |r| r.get(0),
            )
            .ok();
        let account_id = match exists {
            Some(id) => id,
            None => {
                let kind = if code.ends_with("00") {
                    "group"
                } else {
                    "detail"
                };
                let nature = if matches!(*category, "asset" | "expense") {
                    "debit"
                } else {
                    "credit"
                };
                tx.execute(
                    "INSERT INTO accounts(code,name,category,kind,nature,currency)
                     VALUES(?1,?2,?3,?4,?5,'YER')",
                    params![format!("__demo_{code}"), name, category, kind, nature],
                )?;
                let id = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO finance_account_scopes(company_id,account_id) VALUES(?1,?2)",
                    params![DEMO_COMPANY_ID, id],
                )?;
                tx.execute(
                    "INSERT INTO finance_company_account_codes(company_id,account_id,code)
                     VALUES(?1,?2,?3)",
                    params![DEMO_COMPANY_ID, id, code],
                )?;
                id
            }
        };
        if *code == "1110" {
            cash_id = account_id;
        }
    }
    if cash_id != 0 {
        tx.execute(
            "INSERT OR IGNORE INTO finance_funds
             (company_id,branch_id,kind,code,name,account_id,currency,opened_on)
             VALUES(?1,1,'cash','CASH-1','الصندوق الرئيسي',?2,'YER','2026-01-01')",
            params![DEMO_COMPANY_ID, cash_id],
        )?;
    }
    tx.execute(
        "INSERT OR IGNORE INTO finance_dimensions(company_id,code,name,kind)
         VALUES(?1,'CC-100','الإدارة العامة','cost_center')",
        params![DEMO_COMPANY_ID],
    )?;
    tx.execute(
        "INSERT OR IGNORE INTO financial_periods(name,start_date,end_date,status)
         VALUES('فترة العرض','2026-01-01','2026-12-31','open')",
        [],
    )?;
    let period_id = tx.query_row(
        "SELECT id FROM financial_periods WHERE start_date='2026-01-01' AND end_date='2026-12-31'",
        [],
        |r| r.get::<_, i64>(0),
    )?;
    tx.execute(
        "INSERT OR IGNORE INTO finance_period_context(period_id,company_id) VALUES(?1,?2)",
        params![period_id, DEMO_COMPANY_ID],
    )?;
    seed_entries(&tx)?;
    tx.commit()
}

/// قيود مرحّلة تجريبية متوازنة، حتى تظهر التقارير بأرقام واقعية في المعاينة.
fn seed_entries(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let already: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM journal_entries WHERE company_id=?1)",
        [DEMO_COMPANY_ID],
        |r| r.get(0),
    )?;
    if already {
        return Ok(());
    }
    let entries: &[DemoEntry] = &[
        (
            "رأس المال الافتتاحي",
            "2026-01-02",
            "10000.0000",
            &[("1110", "10000.0000", "0"), ("3100", "0", "10000.0000")],
        ),
        (
            "إيراد مبيعات نقدية",
            "2026-02-10",
            "2500.0000",
            &[("1110", "2500.0000", "0"), ("4100", "0", "2500.0000")],
        ),
        (
            "مصروف إيجار",
            "2026-02-28",
            "800.0000",
            &[("5100", "800.0000", "0"), ("1110", "0", "800.0000")],
        ),
    ];
    for (index, (description, date, total, lines)) in entries.iter().enumerate() {
        // كل قيد مرحّل يحتاج مسودة أصلية (قيد FK) — ننشئها هنا للمعاينة فقط.
        tx.execute(
            "INSERT INTO drafts(description,date,currency,total,lines_json,lines_count,company_id)
             VALUES(?1,?2,'YER',?3,'[]',?4,?5)",
            rusqlite::params![
                description,
                date,
                total,
                lines.len() as i64,
                DEMO_COMPANY_ID
            ],
        )?;
        let draft_id = tx.last_insert_rowid();
        let _ = index;
        tx.execute(
            "INSERT INTO journal_entries(draft_id,description,entry_date,currency,total,finalized,company_id)
             VALUES(?1,?2,?3,'YER',?4,0,?5)",
            rusqlite::params![draft_id, description, date, total, DEMO_COMPANY_ID],
        )?;
        let entry_id = tx.last_insert_rowid();
        for (line_no, (code, debit, credit)) in lines.iter().enumerate() {
            let account: i64 = tx.query_row(
                "SELECT account_id FROM finance_company_account_codes WHERE company_id=?1 AND code=?2",
                rusqlite::params![DEMO_COMPANY_ID, code],
                |r| r.get(0),
            )?;
            tx.execute(
                "INSERT INTO journal_lines(entry_id,line_no,account_id,account_snapshot,debit,credit)
                 VALUES(?1,?2,?3,?4,?5,?6)",
                rusqlite::params![entry_id, line_no as i64, account, code, debit, credit],
            )?;
        }
        // التثبيت بعد إدخال السطور: المُشغِّل يمنع أي سطر بعد الترحيل.
        tx.execute(
            "UPDATE journal_entries SET finalized=1 WHERE id=?1",
            rusqlite::params![entry_id],
        )?;
    }
    Ok(())
}
