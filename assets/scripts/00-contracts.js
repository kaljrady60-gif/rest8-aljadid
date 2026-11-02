// مُولَّد آليًا من quality/contracts.toml — لا تُحرّره يدويًا.
window.CONTRACTS = Object.freeze({
  "account": {
    "balance_sheet_categories": [
      "asset",
      "liability",
      "equity"
    ],
    "categories": [
      "asset",
      "liability",
      "equity",
      "revenue",
      "expense"
    ],
    "income_categories": [
      "revenue",
      "expense"
    ],
    "kinds": [
      "group",
      "detail"
    ],
    "natures": [
      "debit",
      "credit"
    ]
  },
  "currencies": [
    {
      "code": "YER",
      "decimals": 2,
      "iso": true,
      "label": "ريال يمني · غير محدد الطبعة"
    },
    {
      "code": "YER-A",
      "decimals": 2,
      "iso": false,
      "label": "ريال يمني · طبعة عدن الجديدة"
    },
    {
      "code": "YER-S",
      "decimals": 2,
      "iso": false,
      "label": "ريال يمني · طبعة صنعاء القديمة"
    },
    {
      "code": "USD",
      "decimals": 2,
      "iso": true,
      "label": "دولار أمريكي"
    },
    {
      "code": "SAR",
      "decimals": 2,
      "iso": true,
      "label": "ريال سعودي"
    }
  ],
  "funds": {
    "kinds": [
      "cash",
      "bank"
    ]
  },
  "limits": {
    "max_description_chars": 200,
    "max_file_lines": 700,
    "max_lines_per_entry": 50,
    "max_page_size": 100,
    "max_script_lines": 700,
    "min_lines_per_entry": 2,
    "money_scale": 4
  },
  "periods": {
    "statuses": [
      "open",
      "soft_closed",
      "closed"
    ]
  },
  "reports": [
    {
      "id": "income",
      "kind": "statement",
      "title": "قائمة الدخل"
    },
    {
      "id": "balance",
      "kind": "statement",
      "title": "الميزانية العمومية"
    },
    {
      "id": "cashflow",
      "kind": "statement",
      "title": "التدفقات النقدية"
    },
    {
      "id": "general",
      "kind": "journal",
      "title": "اليومية العامة"
    },
    {
      "id": "trial",
      "kind": "statement",
      "title": "ميزان المراجعة"
    }
  ],
  "roles": {
    "labels": {
      "admin": "مدير مالي",
      "operator": "مُنفّذ",
      "reader": "قارئ"
    },
    "order": [
      "reader",
      "operator",
      "admin"
    ]
  },
  "schemaVersion": 1,
  "screens": [
    {
      "api": "/api/finance/accounts",
      "id": "fin.coa",
      "title": "دليل الحسابات"
    },
    {
      "api": "/api/finance/journals",
      "id": "fin.journal",
      "title": "القيود اليومية"
    },
    {
      "api": "/api/finance/ledger",
      "id": "fin.ledger",
      "title": "دفتر الأستاذ"
    },
    {
      "api": "/api/finance/trial",
      "id": "fin.trial",
      "title": "ميزان المراجعة"
    },
    {
      "api": "/api/finance/vouchers",
      "id": "fin.vouchers",
      "title": "السندات"
    },
    {
      "api": "/api/finance/opening",
      "id": "fin.opening",
      "title": "الأرصدة الافتتاحية"
    },
    {
      "api": "/api/finance/funds?kind=cash",
      "id": "fin.cashboxes",
      "title": "الصناديق"
    },
    {
      "api": "/api/finance/funds?kind=bank",
      "id": "fin.banks",
      "title": "البنوك"
    },
    {
      "api": "/api/finance/fx",
      "id": "fin.currencies",
      "title": "العملات وأسعار الصرف"
    },
    {
      "api": "/api/finance/dimensions",
      "id": "fin.costcenters",
      "title": "مراكز التكلفة"
    },
    {
      "api": "/api/finance/periods",
      "id": "fin.periods",
      "title": "الإقفال والفترات"
    },
    {
      "api": "/api/finance/statements",
      "id": "fin.reports",
      "title": "التقارير المالية"
    },
    {
      "api": "/api/finance/bankrec",
      "id": "fin.bankrec",
      "title": "التسوية البنكية"
    },
    {
      "api": "/api/finance/parties",
      "id": "fin.parties",
      "title": "الأطراف"
    },
    {
      "api": "/api/finance/checks",
      "id": "fin.checks",
      "title": "الشيكات"
    },
    {
      "api": "/api/finance/approval",
      "id": "fin.approval",
      "title": "سير الاعتماد"
    },
    {
      "api": "/api/finance/closing",
      "id": "fin.closing",
      "title": "الإقفال السنوي"
    },
    {
      "api": "/api/finance/export",
      "id": "fin.export",
      "title": "التصدير والطباعة"
    },
    {
      "api": "/api/finance/assets",
      "id": "fin.assets",
      "title": "الأصول الثابتة"
    },
    {
      "api": "/api/finance/payroll/runs",
      "id": "fin.payroll",
      "title": "الرواتب"
    },
    {
      "api": "/api/finance/branches",
      "id": "fin.branches",
      "title": "الفروع"
    },
    {
      "api": "/api/finance/print",
      "id": "fin.print",
      "title": "الطباعة وPDF"
    }
  ],
  "security": {
    "lockout_minutes": 15,
    "max_login_failures": 5,
    "password_min_chars": 10,
    "session_hours": 12
  }
});
window.CONTRACT_SCREENS = window.CONTRACTS.screens;
window.CONTRACT_CURRENCIES = window.CONTRACTS.currencies;
