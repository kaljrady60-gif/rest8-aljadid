//! بذر الموجة الثالثة: تصنيف أصول وأصول قائمة وموظفون ومسير رواتب مسودة.
//! يُستدعى من `--seed-more` فقط، ويترك كل شيء قابلًا للترحيل من الواجهة،
//! ولا يُنشئ قيودًا عدا قيود شراء الأصول (لأنها عمليات تمّت فعلًا).
use super::gl;
use super::money::{parse, render};
use rusqlite::{Connection, OptionalExtension, params};

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

pub fn run(conn: &Connection, company: i64, branch: i64) -> Result<(usize, usize), String> {
    let assets = seed_assets(conn, company)?;
    let employees = seed_employees(conn, company, branch)?;
    Ok((assets, employees))
}

fn seed_assets(conn: &Connection, company: i64) -> Result<usize, String> {
    let name = "أجهزة ومعدات";
    let category: Option<i64> = conn
        .query_row(
            "SELECT id FROM finance_asset_categories WHERE company_id=?1 AND name=?2",
            params![company, name],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let category = match category {
        Some(id) => id,
        None => {
            conn.execute(
                "INSERT INTO finance_asset_categories
                 (company_id,name,method,annual_rate,asset_account_id,accum_account_id,expense_account_id)
                 VALUES(?1,?2,'straight','0',?3,?4,?5)",
                params![
                    company,
                    name,
                    account(conn, company, "1400")?,
                    account(conn, company, "1410")?,
                    account(conn, company, "5700")?
                ],
            )
            .map_err(|e| e.to_string())?;
            conn.last_insert_rowid()
        }
    };
    let asset_account = account(conn, company, "1400")?;
    let bank = account(conn, company, "1120")?;
    let mut added = 0;
    for (code, label, cost, salvage, life, acquired) in [
        (
            "FA-100",
            "حاسوب محمول Dell",
            "600000",
            "0",
            36,
            "2026-02-01",
        ),
        (
            "FA-110",
            "طابعة ليزر شبكية",
            "180000",
            "20000",
            48,
            "2026-03-01",
        ),
        (
            "FA-120",
            "سيارة توزيع",
            "4500000",
            "500000",
            60,
            "2026-01-15",
        ),
    ] {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM finance_assets WHERE company_id=?1 AND code=?2)",
                params![company, code],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if exists {
            continue;
        }
        let cost_value = parse(cost, "التكلفة").map_err(|e| format!("{e:?}"))?;
        let ctx = gl::context(conn, company, acquired).map_err(|e| format!("{e:?}"))?;
        let entry = gl::post_entry(
            conn,
            ctx,
            acquired,
            &format!("شراء أصل ثابت — {label}"),
            "YER",
            &[
                (asset_account, cost_value, rust_decimal::Decimal::ZERO),
                (bank, rust_decimal::Decimal::ZERO, cost_value),
            ],
            None,
            "seed.asset.acquire",
        )
        .map_err(|e| format!("{e:?}"))?;
        conn.execute(
            "INSERT INTO finance_assets
             (company_id,branch_id,category_id,code,name,acquired_on,cost,salvage,life_months,
              funded_from,accumulated,status,acquired_entry_id)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'bank','0','active',?10)",
            params![
                company,
                branch_of(conn, company)?,
                category,
                code,
                label,
                acquired,
                render(cost_value),
                render(parse(salvage, "القيمة التخريدية").map_err(|e| format!("{e:?}"))?),
                life,
                entry
            ],
        )
        .map_err(|e| e.to_string())?;
        added += 1;
    }
    Ok(added)
}

fn branch_of(conn: &Connection, company: i64) -> Result<i64, String> {
    conn.query_row(
        "SELECT id FROM finance_branches WHERE company_id=?1 ORDER BY id LIMIT 1",
        [company],
        |r| r.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "لا يوجد فرع للشركة".to_string())
}

fn seed_employees(conn: &Connection, company: i64, branch: i64) -> Result<usize, String> {
    let mut added = 0;
    for (code, name, job, base, allowances, deductions) in [
        (
            "EMP-001",
            "أحمد صالح",
            "مدير مالي",
            "700000",
            "120000",
            "70000",
        ),
        (
            "EMP-002",
            "مريم عبدالله",
            "محاسبة",
            "450000",
            "50000",
            "40000",
        ),
        ("EMP-003", "خالد ناصر", "مبيعات", "380000", "90000", "30000"),
    ] {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM finance_employees WHERE company_id=?1 AND code=?2)",
                params![company, code],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if exists {
            continue;
        }
        conn.execute(
            "INSERT INTO finance_employees
             (company_id,branch_id,code,name,job_title,hired_on,base_salary,allowances,deductions,currency,status)
             VALUES(?1,?2,?3,?4,?5,'2025-01-05',?6,?7,?8,'YER','active')",
            params![
                company,
                branch,
                code,
                name,
                job,
                render(parse(base, "الراتب").map_err(|e| format!("{e:?}"))?),
                render(parse(allowances, "البدلات").map_err(|e| format!("{e:?}"))?),
                render(parse(deductions, "الاستقطاعات").map_err(|e| format!("{e:?}"))?)
            ],
        )
        .map_err(|e| e.to_string())?;
        added += 1;
    }

    // مسير مسودة للشهر الحالي إن لم يوجد، ليُرحّله المستخدم من الواجهة بنفسه.
    let period = "2026-11";
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM finance_payroll_runs WHERE company_id=?1 AND period=?2)",
            params![company, period],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        conn.execute(
            "INSERT INTO finance_payroll_runs
             (company_id,branch_id,period,status,gross_total,deduction_total,net_total,currency)
             VALUES(?1,?2,?3,'draft','2550000','140000','2410000','YER')",
            params![company, branch, period],
        )
        .map_err(|e| e.to_string())?;
        let run = conn.last_insert_rowid();
        let mut stmt = conn
            .prepare(
                "SELECT id,code,name,base_salary,allowances,deductions FROM finance_employees
                 WHERE company_id=?1 AND status='active' ORDER BY code",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([company], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        drop(stmt);
        for (id, code, name, base, allowances, deductions) in rows {
            let base_v = parse(&base, "الراتب").map_err(|e| format!("{e:?}"))?;
            let allow_v = parse(&allowances, "البدلات").map_err(|e| format!("{e:?}"))?;
            let ded_v = parse(&deductions, "الاستقطاعات").map_err(|e| format!("{e:?}"))?;
            let _ = code;
            conn.execute(
                "INSERT INTO finance_payroll_lines
                 (run_id,employee_id,employee_name,base_salary,allowances,deductions,net)
                 VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    run,
                    id,
                    name,
                    render(base_v),
                    render(allow_v),
                    render(ded_v),
                    render(base_v + allow_v - ded_v)
                ],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(added)
}
