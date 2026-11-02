//! تحويل وتمثيل المبالغ: Decimal فقط، ودائمًا 4 منازل داخلية.
use super::scope::FinanceError;
use rust_decimal::Decimal;
use std::str::FromStr;

pub const SCALE: u32 = 4;

/// يقرأ نصًّا عشريًا بحد أقصى 30 حرفًا وبلا أس سالب، ثم يوحّد المنازل.
pub fn parse(raw: &str, field: &str) -> Result<Decimal, FinanceError> {
    let text = raw.trim();
    if text.is_empty() || text.len() > 30 {
        return Err(FinanceError::Invalid(format!("قيمة {field} غير صالحة.")));
    }
    if text.contains('e') || text.contains('E') {
        return Err(FinanceError::Invalid(format!(
            "قيمة {field} يجب أن تكون عددًا عشريًا صريحًا."
        )));
    }
    let value = Decimal::from_str(text)
        .map_err(|_| FinanceError::Invalid(format!("قيمة {field} ليست رقمًا صالحًا.")))?;
    if value.is_sign_negative() {
        return Err(FinanceError::Invalid(format!(
            "قيمة {field} يجب أن تكون موجبة."
        )));
    }
    Ok(normalize(value))
}

/// يحوّل نصًّا من قاعدة البيانات؛ أي تلف يُعد خطأً صريحًا لا صفرًا صامتًا.
pub fn from_db(raw: &str, field: &str) -> Result<Decimal, FinanceError> {
    Decimal::from_str(raw.trim())
        .map(normalize)
        .map_err(|_| FinanceError::Invalid(format!("قيمة {field} المخزّنة تالفة.")))
}

pub fn normalize(value: Decimal) -> Decimal {
    value.round_dp(SCALE).normalize()
}

/// صيغة موحّدة للعرض والتخزين: 4 منازل بلا أس، وبنقطة عشرية.
pub fn render(value: Decimal) -> String {
    format!("{:.*}", SCALE as usize, value.round_dp(SCALE))
}
