//! بذر بيانات تشغيلية واقعية لشركة قائمة (أمر `--seed-demo`).
//! القيود هنا **مرحّلة فعليًا** ومتوازنة، وتُبنى بنفس ثوابت التخزين التي تستخدمها
//! مسارات المالية، حتى تُقرأ التقارير من بيانات حقيقية لا من أرقام مزخرفة.
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

/// حسابات تفصيلية تُضاف عند غيابها لتغطية دورة تشغيل كاملة.
const CHART: &[(i64, &str, &str, &str)] = &[
    (20, "1130", "شيكات برسم التحصيل", "asset"),
    (21, "1140", "سلف ومقدمات", "asset"),
    (22, "1150", "عهد ومصروفات مقدمة", "asset"),
    (23, "1400", "أصول ثابتة", "asset"),
    (24, "1410", "مجمع إهلاك الأصول", "asset"),
    (25, "2300", "ضرائب مستحقة", "liability"),
    (26, "2400", "مصروفات مستحقة", "liability"),
    (27, "2500", "قروض قصيرة الأجل", "liability"),
    (28, "4200", "إيرادات خدمات", "revenue"),
    (29, "4300", "خصومات مبيعات", "revenue"),
    (30, "5400", "كهرباء ومياه", "expense"),
    (31, "5500", "اتصالات وإنترنت", "expense"),
    (32, "5600", "صيانة وإصلاح", "expense"),
    (33, "5700", "إهلاك", "expense"),
    (34, "5800", "مصاريف بنكية", "expense"),
];

/// (الحساب، مدين، دائن)
type Line = (i64, i64, i64);

fn money(value: i64) -> String {
    format!("{}.0000", value)
}

fn account(conn: &Connection, company: i64, code: &str) -> Result<i64, String> {
    conn.query_row(
        "SELECT account_id FROM finance_company_account_codes WHERE company_id=?1 AND code=?2",
        params![company, code],
        |r| r.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())?
    .ok_or_else(|| format!("الحساب {code} غير موجود في دليل الشركة"))
}

fn ensure_chart(conn: &Connection, company: i64) -> Result<u32, String> {
    let mut added = 0;
    for (id, code, name, category) in CHART {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM finance_company_account_codes WHERE company_id=?1 AND code=?2)",
                params![company, code],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if exists {
            continue;
        }
        conn.execute(
            "INSERT OR IGNORE INTO accounts (id,code,name,parent_id,category,kind,nature,currency)
             VALUES (?1,?2,?3,NULL,?4,'detail','debit','YER')",
            params![id, code, name, category],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT OR IGNORE INTO finance_account_scopes (account_id,company_id)
             VALUES ((SELECT id FROM accounts WHERE code=?2),?1)",
            params![company, code],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT OR IGNORE INTO finance_company_account_codes (company_id,account_id,code)
             VALUES (?1,(SELECT id FROM accounts WHERE code=?2),?2)",
            params![company, code],
        )
        .map_err(|e| e.to_string())?;
        added += 1;
    }
    Ok(added)
}

/// قيد مسودة + سطر + قيد مرحّل — بنفس بنية `drafts/draft_lines/journal_entries/journal_lines`.
fn post(
    conn: &Connection,
    ctx: (i64, i64, i64),
    date: &str,
    description: &str,
    lines: &[Line],
) -> Result<i64, String> {
    let (company, branch, period) = ctx;
    let total: i64 = lines.iter().map(|l| l.1).sum();
    let credit: i64 = lines.iter().map(|l| l.2).sum();
    if total != credit {
        return Err(format!("قيد غير متوازن: {description}"));
    }
    conn.execute(
        "INSERT INTO drafts (description,date,currency,total,lines_json,lines_count,company_id)
         VALUES (?1,?2,'YER',?3,'[]',?4,?5)",
        params![description, date, money(total), lines.len() as i64, company],
    )
    .map_err(|e| e.to_string())?;
    let draft_id = conn.last_insert_rowid();
    for (index, (account, debit, credit)) in lines.iter().enumerate() {
        conn.execute(
            "INSERT INTO draft_lines (draft_id,line_no,account_id,account_snapshot,debit,credit)
             VALUES (?1,?2,?3,(SELECT code || ' · ' || name FROM accounts WHERE id=?3),?4,?5)",
            params![
                draft_id,
                index as i64,
                account,
                money(*debit),
                money(*credit)
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    conn.execute(
        "INSERT INTO journal_entries (draft_id,description,entry_date,currency,total,finalized,posted_by,period_id,company_id,branch_id)
         VALUES (?1,?2,?3,'YER',?4,0,1,?5,?6,?7)",
        params![draft_id, description, date, money(total), period, company, branch],
    )
    .map_err(|e| e.to_string())?;
    let entry_id = conn.last_insert_rowid();
    for (index, (account, debit, credit)) in lines.iter().enumerate() {
        conn.execute(
            "INSERT INTO journal_lines (entry_id,line_no,account_id,account_snapshot,debit,credit)
             VALUES (?1,?2,?3,(SELECT code || ' · ' || name FROM accounts WHERE id=?3),?4,?5)",
            params![
                entry_id,
                index as i64,
                account,
                money(*debit),
                money(*credit)
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    // الترحيل الفعلي: القفل بعد إدخال السطور (مشغّل القاعدة يمنع الإدخال المتأخر).
    conn.execute(
        "UPDATE journal_entries SET finalized=1 WHERE id=?1",
        params![entry_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(entry_id)
}

fn audit(
    conn: &Connection,
    company: i64,
    action: &str,
    entity: &str,
    id: i64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO finance_audit_log (company_id,actor_id,action,entity,entity_id,payload)
         VALUES (?1,1,?2,?3,?4,'{\"source\":\"seed-demo\"}')",
        params![company, action, entity, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug)]
pub struct SeedReport {
    pub company: i64,
    pub accounts_added: u32,
    pub funds_added: u32,
    pub rates_added: u32,
    pub dimensions_added: u32,
    pub entries: u32,
    pub opening_lines: u32,
}

/// بذر كامل: يرفض إن كانت الشركة تحتوي قيودًا مرحّلة.
pub fn seed_demo(db: &mut Connection) -> Result<SeedReport, String> {
    run(db, false)
}

/// إضافة الأطراف والشيكات فقط لشركة قائمة (لا يمسّ القيود).
pub fn seed_augment(db: &mut Connection) -> Result<SeedReport, String> {
    run(db, true)
}

fn run(db: &mut Connection, allow_existing: bool) -> Result<SeedReport, String> {
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let company: i64 = tx
        .query_row(
            "SELECT id FROM finance_companies ORDER BY id LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("لا توجد شركة؛ شغّل --init-company أولًا")?;
    let existing: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM journal_entries WHERE company_id=?1 AND finalized=1",
            [company],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if existing > 0 && !allow_existing {
        return Err("الشركة تحتوي قيودًا مرحّلة؛ لم يتغير شيء.".into());
    }
    let branch: i64 = tx
        .query_row(
            "SELECT id FROM finance_branches WHERE company_id=?1 ORDER BY id LIMIT 1",
            [company],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("لا يوجد فرع للشركة")?;
    let (period, start): (i64, String) = tx
        .query_row(
            "SELECT p.id,p.start_date FROM financial_periods p
             JOIN finance_period_context c ON c.period_id=p.id
             WHERE c.company_id=?1 AND p.status='open' ORDER BY p.start_date DESC LIMIT 1",
            [company],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("لا توجد فترة مفتوحة")?;
    let ctx = (company, branch, period);

    let accounts_added = if allow_existing {
        0
    } else {
        ensure_chart(&tx, company)?
    };

    // صناديق: بنك تجاري مرتبط بالحساب 1120.
    let mut funds_added = 0;
    for (kind, code, name, account_code, opened) in [(
        "bank",
        "BANK-1",
        "البنك التجاري — الحساب الجاري",
        "1120",
        "2026-01-01",
    )] {
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM finance_funds WHERE company_id=?1 AND code=?2)",
                params![company, code],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if exists {
            continue;
        }
        let account_id = account(&tx, company, account_code)?;
        tx.execute(
            "INSERT INTO finance_funds (company_id,branch_id,account_id,name,kind,code,currency,iban,opened_on)
             VALUES (?1,?2,?3,?4,?5,?6,'YER',NULL,?7)",
            params![company, branch, account_id, name, kind, code, opened],
        )
        .map_err(|e| e.to_string())?;
        funds_added += 1;
    }

    // أسعار صرف مرجعية.
    let mut rates_added = 0;
    for (base, quote, rate) in [("USD", "YER", "530.0000"), ("SAR", "YER", "141.0000")] {
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM finance_fx_rates WHERE company_id=?1 AND base=?2 AND quote=?3)",
                params![company, base, quote],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if exists {
            continue;
        }
        tx.execute(
            "INSERT INTO finance_fx_rates (company_id,base,quote,rate,as_of) VALUES (?1,?2,?3,?4,?5)",
            params![company, base, quote, rate, start],
        )
        .map_err(|e| e.to_string())?;
        rates_added += 1;
    }

    // مراكز تكلفة ومشاريع.
    let mut dimensions_added = 0;
    for (code, name, kind) in [
        ("PRJ-1", "مشروع التوسع", "project"),
        ("PRJ-2", "صيانة الأسطول", "project"),
    ] {
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM finance_dimensions WHERE company_id=?1 AND code=?2)",
                params![company, code],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if exists {
            continue;
        }
        tx.execute(
            "INSERT INTO finance_dimensions (company_id,code,name,kind) VALUES (?1,?2,?3,?4)",
            params![company, code, name, kind],
        )
        .map_err(|e| e.to_string())?;
        dimensions_added += 1;
    }

    if allow_existing {
        // وضع التوسيع: نضيف الأطراف والشيكات فقط ثم نخرج.
        seed_parties_and_checks(&tx, company, branch)?;
        super::seed2::run(&tx, company, branch)?;
        tx.commit().map_err(|e| e.to_string())?;
        return Ok(SeedReport {
            company,
            accounts_added: 0,
            funds_added,
            rates_added,
            dimensions_added,
            entries: 0,
            opening_lines: 0,
        });
    }

    // أرصدة افتتاحية متوازنة: رأس مال مقابل نقدية وبنك.
    let cash = account(&tx, company, "1110")?;
    let bank = account(&tx, company, "1120")?;
    let capital = account(&tx, company, "3100")?;
    let opening: [Line; 3] = [
        (cash, 1_500_000, 0),
        (bank, 6_500_000, 0),
        (capital, 0, 8_000_000),
    ];
    let mut opening_lines = 0;
    for (account_id, debit, credit) in opening {
        tx.execute(
            "INSERT INTO finance_opening_balances (company_id,account_id,currency,debit,credit,created_by)
             VALUES (?1,?2,'YER',?3,?4,1)",
            params![company, account_id, money(debit), money(credit)],
        )
        .map_err(|e| e.to_string())?;
        opening_lines += 1;
    }

    // دورة تشغيل شهرية.
    let clients = account(&tx, company, "1200")?;
    let inventory = account(&tx, company, "1300")?;
    let suppliers = account(&tx, company, "2100")?;
    let salaries_payable = account(&tx, company, "2200")?;
    let sales = account(&tx, company, "4100")?;
    let services = account(&tx, company, "4200")?;
    let rent = account(&tx, company, "5100")?;
    let salaries = account(&tx, company, "5200")?;
    let operations = account(&tx, company, "5300")?;
    let utilities = account(&tx, company, "5400")?;
    let telecom = account(&tx, company, "5500")?;
    let bank_fees = account(&tx, company, "5800")?;


    // البيان المهني بنفس صيغة المرجع: «بيع INV-2026-0001 — عميل نقدي».
    // الرقم تسلسلي لكل نوع داخل سنة البذر، ويُعاد ضبطه في كل استدعاء بذر.
    let mut counters: std::collections::HashMap<&'static str, i64> = std::collections::HashMap::new();
    let mut sale_line = |kind: &'static str, month: i64, party: &str| -> String {
        let n = counters.entry(kind).or_insert(0);
        *n += 1;
        let prefix = match kind {
            "sale" => "INV",
            "purchase" => "PUR",
            "receipt" => "RV",
            "payment" => "PV",
            "payroll" => "PR",
            _ => "EXP",
        };
        let year = &start[..4];
        let label = match kind {
            "sale" => "بيع",
            "purchase" => "شراء",
            "receipt" => "تحصيل",
            "payment" => "سداد",
            "payroll" => "رواتب",
            _ => "مصروف",
        };
        format!(
            "{}",
            crate::finance::narrative::compose(
                label,
                &format!("{prefix}-{year}-{n:04}"),
                &format!("{party} · الشهر {month}")
            )
        )
    };

    let mut entries = 0;
    for month in 1..=10i64 {
        let day = |d: i64| format!("{}-{:02}-{:02}", &start[..4], month, d);
        let growth = (month - 1) * 1_000;
        // مبالغ شهرية متزايدة بمنطق تشغيل واقعي (كلها بالريال اليمني).
        let cash_sales = 1_200_000 + 6 * growth;
        let credit_sales = 900_000 + 4 * growth;
        let collection = 800_000 + 3 * growth;
        let purchases = 600_000 + 2 * growth;
        let supplier_pay = 500_000 + growth / 2;
        let salaries_due = 420_000 + 2 * month;
        let operations_exp = 60_000 + growth / 6;
        let rows: Vec<(String, Vec<Line>)> = vec![
            (
                sale_line("sale", month, "عميل نقدي"),
                vec![(cash, cash_sales, 0), (sales, 0, cash_sales)],
            ),
            (
                sale_line("sale", month, "عملاء آجل"),
                vec![(clients, credit_sales, 0), (sales, 0, credit_sales)],
            ),
            (
                sale_line("receipt", month, "عملاء آجل"),
                vec![(bank, collection, 0), (clients, 0, collection)],
            ),
            (
                sale_line("purchase", month, "موردون"),
                vec![(inventory, purchases, 0), (suppliers, 0, purchases)],
            ),
            (
                sale_line("payment", month, "موردون"),
                vec![(suppliers, supplier_pay, 0), (bank, 0, supplier_pay)],
            ),
            (
                sale_line("expense", month, "إيجار المكتب"),
                vec![(rent, 150_000, 0), (bank, 0, 150_000)],
            ),
            (
                sale_line("payroll", month, "الموظفون"),
                vec![
                    (salaries, salaries_due, 0),
                    (salaries_payable, 0, salaries_due),
                ],
            ),
            (
                sale_line("payment", month, "الموظفون"),
                vec![
                    (salaries_payable, salaries_due - 20_000, 0),
                    (bank, 0, salaries_due - 20_000),
                ],
            ),
            (
                sale_line("expense", month, "الكهرباء والمياه"),
                vec![(utilities, 45_000, 0), (cash, 0, 45_000)],
            ),
            (
                sale_line("expense", month, "الاتصالات"),
                vec![(telecom, 30_000, 0), (bank, 0, 30_000)],
            ),
            (
                sale_line("sale", month, "خدمات"),
                vec![(bank, 90_000, 0), (services, 0, 90_000)],
            ),
            (
                sale_line("expense", month, "مصاريف تشغيلية"),
                vec![(operations, operations_exp, 0), (cash, 0, operations_exp)],
            ),
            (
                format!("عمولات بنكية — الشهر {month}"),
                vec![(bank_fees, 5_000, 0), (bank, 0, 5_000)],
            ),
        ];
        for (index, (description, lines)) in rows.iter().enumerate() {
            let when = day((index as i64 % 26) + 2);
            let id = post(&tx, ctx, &when, description, lines)?;
            audit(&tx, company, "journal.posted", "journal_entry", id)?;
            entries += 1;
        }
    }

    // كشف بنكي مع بنود معلّقة ومُسوّاة.
    let fund: i64 = tx
        .query_row(
            "SELECT id FROM finance_funds WHERE company_id=?1 AND code='BANK-1'",
            [company],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO finance_bank_statements (company_id,fund_id,period_start,period_end,closing_balance)
         VALUES (?1,?2,?3,?4,'2900000.0000')",
        params![company, fund, start, format!("{}-12-31", &start[..4])],
    )
    .map_err(|e| e.to_string())?;
    let statement = tx.last_insert_rowid();
    for (kind, amount, memo, cleared) in [
        ("deposit", "125000.0000", "إيداع عميل لم يظهر بالدفاتر", 0),
        ("withdrawal", "18500.0000", "شيك صادر لم يُصرف بعد", 0),
        ("deposit", "90000.0000", "تحويل وارد", 1),
        ("withdrawal", "5000.0000", "عمولة بنك", 1),
    ] {
        tx.execute(
            "INSERT INTO finance_bankrec_lines (statement_id,entry_id,kind,amount,memo,cleared)
             VALUES (?1,NULL,?2,?3,?4,?5)",
            params![statement, kind, amount, memo, cleared],
        )
        .map_err(|e| e.to_string())?;
    }
    audit(&tx, company, "bankrec.created", "bank_statement", statement)?;

    tx.commit().map_err(|e| e.to_string())?;

    Ok(SeedReport {
        company,
        accounts_added,
        funds_added,
        rates_added,
        dimensions_added,
        entries,
        opening_lines,
    })
}

fn seed_parties_and_checks(
    tx: &rusqlite::Transaction<'_>,
    company: i64,
    branch: i64,
) -> Result<(), String> {
    // أطراف: عميل ومورد على حسابيهما التفصيليين.
    for (name, role, code) in [
        ("مؤسسة النور التجارية", "customer", "1200"),
        ("شركة الإمداد الحديثة", "supplier", "2100"),
    ] {
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM finance_parties WHERE company_id=?1 AND account_id=(SELECT account_id FROM finance_company_account_codes WHERE company_id=?1 AND code=?2))",
                params![company, code],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if exists {
            continue;
        }
        let account_id = account(tx, company, code)?;
        tx.execute(
            "INSERT INTO finance_parties (company_id,branch_id,account_id,name,role) VALUES (?1,?2,?3,?4,?5)",
            params![company, branch, account_id, name, role],
        )
        .map_err(|e| e.to_string())?;
    }

    // شيكات: وارد ومحتمل الارتداع وآخر صادر.
    let fund: Option<i64> = tx
        .query_row(
            "SELECT id FROM finance_funds WHERE company_id=?1 AND kind='bank' ORDER BY id LIMIT 1",
            [company],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let party: Option<i64> = tx
        .query_row(
            "SELECT id FROM finance_parties WHERE company_id=?1 ORDER BY id LIMIT 1",
            [company],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    for (direction, number, bank, drawer, amount, due, status) in [
        (
            "received",
            "RC-1001",
            "بنك التضامن",
            "مؤسسة النور التجارية",
            "450000.0000",
            "2026-11-15",
            "registered",
        ),
        (
            "received",
            "RC-1002",
            "بنك اليمن الدولي",
            "عميل نقدي",
            "120000.0000",
            "2026-10-20",
            "deposited",
        ),
        (
            "issued",
            "IS-2001",
            "بنك التضامن",
            "شركة الإمداد الحديثة",
            "300000.0000",
            "2026-12-01",
            "registered",
        ),
    ] {
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM finance_checks WHERE company_id=?1 AND number=?2)",
                params![company, number],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if exists {
            continue;
        }
        tx.execute(
            "INSERT INTO finance_checks
             (company_id,branch_id,fund_id,party_id,direction,number,bank,drawer,amount,currency,due_date,status)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'YER',?10,?11)",
            params![company, branch, fund, party, direction, number, bank, drawer, amount, due, status],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
