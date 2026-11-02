//! مُولَّد آليًا من quality/contracts.toml — لا تُحرّره يدويًا.
#![allow(dead_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurrencySpec {
    pub code: &'static str,
    pub label: &'static str,
    pub iso: bool,
    pub decimals: u32,
}

pub const CURRENCIES: &[CurrencySpec] = &[
    CurrencySpec {
        code: "YER",
        label: "ريال يمني · غير محدد الطبعة",
        iso: true,
        decimals: 2,
    },
    CurrencySpec {
        code: "YER-A",
        label: "ريال يمني · طبعة عدن الجديدة",
        iso: false,
        decimals: 2,
    },
    CurrencySpec {
        code: "YER-S",
        label: "ريال يمني · طبعة صنعاء القديمة",
        iso: false,
        decimals: 2,
    },
    CurrencySpec {
        code: "USD",
        label: "دولار أمريكي",
        iso: true,
        decimals: 2,
    },
    CurrencySpec {
        code: "SAR",
        label: "ريال سعودي",
        iso: true,
        decimals: 2,
    },
];

pub fn supported_currency(code: &str) -> bool {
    CURRENCIES.iter().any(|c| c.code == code)
}

pub mod limits {
    pub const MAX_FILE_LINES: usize = 700;
    pub const MAX_SCRIPT_LINES: usize = 700;
    pub const MAX_DESCRIPTION_CHARS: usize = 200;
    pub const MAX_LINES_PER_ENTRY: usize = 50;
    pub const MIN_LINES_PER_ENTRY: usize = 2;
    pub const MAX_PAGE_SIZE: usize = 100;
    pub const MONEY_SCALE: usize = 4;
}

pub mod security {
    pub const SESSION_HOURS: i64 = 12;
    pub const MAX_LOGIN_FAILURES: i64 = 5;
    pub const LOCKOUT_MINUTES: i64 = 15;
    pub const PASSWORD_MIN_CHARS: i64 = 10;
}

pub mod account {
    pub const CATEGORIES: &[&str] = &["asset", "liability", "equity", "revenue", "expense"];
    pub const KINDS: &[&str] = &["group", "detail"];
    pub const NATURES: &[&str] = &["debit", "credit"];
    pub const INCOME_CATEGORIES: &[&str] = &["revenue", "expense"];
    pub const BALANCE_SHEET_CATEGORIES: &[&str] = &["asset", "liability", "equity"];
}

pub mod funds {
    pub const KINDS: &[&str] = &["cash", "bank"];
}

pub mod periods {
    pub const STATUSES: &[&str] = &["open", "soft_closed", "closed"];
}

pub mod roles {
    pub const ORDER: &[&str] = &["reader", "operator", "admin"];
    pub fn rank(role: &str) -> Option<usize> {
        ORDER.iter().position(|r| *r == role)
    }
    pub fn at_least(role: &str, required: &str) -> bool {
        matches!((rank(role), rank(required)), (Some(a), Some(b)) if a >= b)
    }
    pub const LABELS: &[(&str, &str)] = &[
        ("reader", "قارئ"),
        ("operator", "مُنفّذ"),
        ("admin", "مدير مالي"),
    ];
    pub fn label(role: &str) -> &'static str {
        LABELS
            .iter()
            .find(|(r, _)| *r == role)
            .map(|(_, l)| *l)
            .unwrap_or("غير معروف")
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ScreenSpec {
    pub id: &'static str,
    pub title: &'static str,
    pub api: &'static str,
}
pub const SCREENS: &[ScreenSpec] = &[
    ScreenSpec {
        id: "fin.coa",
        title: "دليل الحسابات",
        api: "/api/finance/accounts",
    },
    ScreenSpec {
        id: "fin.journal",
        title: "القيود اليومية",
        api: "/api/finance/journals",
    },
    ScreenSpec {
        id: "fin.ledger",
        title: "دفتر الأستاذ",
        api: "/api/finance/ledger",
    },
    ScreenSpec {
        id: "fin.trial",
        title: "ميزان المراجعة",
        api: "/api/finance/trial",
    },
    ScreenSpec {
        id: "fin.vouchers",
        title: "السندات",
        api: "/api/finance/vouchers",
    },
    ScreenSpec {
        id: "fin.opening",
        title: "الأرصدة الافتتاحية",
        api: "/api/finance/opening",
    },
    ScreenSpec {
        id: "fin.cashboxes",
        title: "الصناديق",
        api: "/api/finance/funds?kind=cash",
    },
    ScreenSpec {
        id: "fin.banks",
        title: "البنوك",
        api: "/api/finance/funds?kind=bank",
    },
    ScreenSpec {
        id: "fin.currencies",
        title: "العملات وأسعار الصرف",
        api: "/api/finance/fx",
    },
    ScreenSpec {
        id: "fin.costcenters",
        title: "مراكز التكلفة",
        api: "/api/finance/dimensions",
    },
    ScreenSpec {
        id: "fin.periods",
        title: "الإقفال والفترات",
        api: "/api/finance/periods",
    },
    ScreenSpec {
        id: "fin.reports",
        title: "التقارير المالية",
        api: "/api/finance/statements",
    },
    ScreenSpec {
        id: "fin.bankrec",
        title: "التسوية البنكية",
        api: "/api/finance/bankrec",
    },
    ScreenSpec {
        id: "fin.parties",
        title: "الأطراف",
        api: "/api/finance/parties",
    },
    ScreenSpec {
        id: "fin.checks",
        title: "الشيكات",
        api: "/api/finance/checks",
    },
    ScreenSpec {
        id: "fin.approval",
        title: "سير الاعتماد",
        api: "/api/finance/approval",
    },
    ScreenSpec {
        id: "fin.closing",
        title: "الإقفال السنوي",
        api: "/api/finance/closing",
    },
    ScreenSpec {
        id: "fin.export",
        title: "التصدير والطباعة",
        api: "/api/finance/export",
    },
    ScreenSpec {
        id: "fin.assets",
        title: "الأصول الثابتة",
        api: "/api/finance/assets",
    },
    ScreenSpec {
        id: "fin.payroll",
        title: "الرواتب",
        api: "/api/finance/payroll/runs",
    },
    ScreenSpec {
        id: "fin.branches",
        title: "الفروع",
        api: "/api/finance/branches",
    },
    ScreenSpec {
        id: "fin.print",
        title: "الطباعة وPDF",
        api: "/api/finance/print",
    },
];

#[derive(Debug, Clone, Copy)]
pub struct ReportSpec {
    pub id: &'static str,
    pub title: &'static str,
    pub kind: &'static str,
}
pub const REPORTS: &[ReportSpec] = &[
    ReportSpec {
        id: "income",
        title: "قائمة الدخل",
        kind: "statement",
    },
    ReportSpec {
        id: "balance",
        title: "الميزانية العمومية",
        kind: "statement",
    },
    ReportSpec {
        id: "cashflow",
        title: "التدفقات النقدية",
        kind: "statement",
    },
    ReportSpec {
        id: "general",
        title: "اليومية العامة",
        kind: "journal",
    },
    ReportSpec {
        id: "trial",
        title: "ميزان المراجعة",
        kind: "statement",
    },
];

pub const SCHEMA_VERSION: u32 = 1;
