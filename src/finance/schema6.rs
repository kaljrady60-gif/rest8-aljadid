//! مخطط ربط الضرائب بالقيود والسندات (schema6).
//!
//! لماذا جدول روابط مستقل بدل عمود داخل `draft_lines`: القيد قد يحمل أكثر من ضريبة
//! (قيمة مضافة + استقطاع) لكل منها وعاء ومبلغ مختلف، والربط يجب أن يبقى أثرًا
//! قابلًا للتدقيق حتى بعد ترحيل القيد (جداول القيود المرحّلة محمية بمشغّلات ضد التعديل).
use rusqlite::Connection;

pub fn init(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS finance_tax_links (
            id INTEGER PRIMARY KEY,
            company_id INTEGER NOT NULL REFERENCES finance_companies(id) ON DELETE RESTRICT,
            doc_type TEXT NOT NULL CHECK(doc_type IN ('journal','voucher')),
            doc_id INTEGER NOT NULL,
            tax_id INTEGER NOT NULL REFERENCES finance_taxes(id) ON DELETE RESTRICT,
            tax_code TEXT NOT NULL,
            direction TEXT NOT NULL CHECK(direction IN ('output','input')),
            base TEXT NOT NULL,
            rate_bp INTEGER NOT NULL,
            tax_amount TEXT NOT NULL,
            inclusive INTEGER NOT NULL DEFAULT 0,
            account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
            archived_at TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(company_id, doc_type, doc_id, tax_id, direction)
        );
        CREATE INDEX IF NOT EXISTS idx_tax_links_doc
            ON finance_tax_links(company_id, doc_type, doc_id);
        CREATE INDEX IF NOT EXISTS idx_tax_links_tax
            ON finance_tax_links(company_id, tax_id);",
    )
}
