use crate::generated::contracts;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// العملات تأتي من الملف الموحّد `quality/contracts.toml` عبر المولّد.
/// YER-A و YER-S وسوم داخلية للطبعة اليمنية وليست رموز ISO.
pub fn supported_currency(code: &str) -> bool {
    contracts::supported_currency(code)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineInput {
    #[serde(default)]
    pub account_id: Option<i64>,
    #[serde(default)]
    pub account: String,
    pub debit: String,
    pub credit: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftInput {
    pub description: String,
    pub date: String,
    pub currency: String,
    pub lines: Vec<LineInput>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Totals {
    pub debit: String,
    pub credit: String,
}

pub fn validate(input: &DraftInput) -> Result<Totals, String> {
    let description = input.description.trim();
    if description.is_empty() || description.chars().count() > 200 {
        return Err("أدخل وصفًا للقيد لا يتجاوز 200 حرف.".into());
    }
    if chrono::NaiveDate::parse_from_str(&input.date, "%Y-%m-%d").is_err() {
        return Err("أدخل تاريخًا صحيحًا.".into());
    }
    if !supported_currency(&input.currency) {
        return Err("العملة غير مدعومة في هذه النسخة.".into());
    }
    if !(2..=50).contains(&input.lines.len()) {
        return Err("يجب أن يتكون القيد من سطرين إلى 50 سطرًا.".into());
    }
    let mut debit = Decimal::ZERO;
    let mut credit = Decimal::ZERO;
    for (i, line) in input.lines.iter().enumerate() {
        if line.account.trim().is_empty() || line.account.chars().count() > 100 {
            return Err(format!("اختر الحساب في السطر {}.", i + 1));
        }
        let parse = |s: &str| -> Result<Decimal, String> {
            if s.len() > 24 {
                return Err("المبلغ يتجاوز النطاق المسموح في النسخة التجريبية.".into());
            }
            let value = if s.trim().is_empty() {
                Decimal::ZERO
            } else {
                Decimal::from_str(s.trim())
                    .map_err(|_| "أدخل مبلغًا عشريًا صحيحًا دون فواصل أو رموز.".to_string())?
            };
            if value < Decimal::ZERO
                || value.scale() > 4
                || value > Decimal::from(1_000_000_000_000u64)
            {
                return Err("المبلغ خارج النطاق أو يتجاوز أربع منازل عشرية.".into());
            }
            Ok(value)
        };
        let d = parse(&line.debit)?;
        let c = parse(&line.credit)?;
        if (d.is_zero() && c.is_zero()) || (!d.is_zero() && !c.is_zero()) {
            return Err(format!(
                "السطر {} يحتاج مبلغًا مدينًا أو دائنًا واحدًا فقط.",
                i + 1
            ));
        }
        debit = debit
            .checked_add(d)
            .ok_or("تجاوز مجموع المدين النطاق المسموح.")?;
        credit = credit
            .checked_add(c)
            .ok_or("تجاوز مجموع الدائن النطاق المسموح.")?;
    }
    if debit != credit {
        return Err(format!(
            "القيد غير متوازن: المدين {}، والدائن {}.",
            debit.normalize(),
            credit.normalize()
        ));
    }
    Ok(Totals {
        debit: debit.normalize().to_string(),
        credit: credit.normalize().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> DraftInput {
        DraftInput {
            description: "اختبار".into(),
            date: "2026-10-29".into(),
            currency: "YER".into(),
            lines: vec![
                LineInput {
                    account_id: None,
                    account: "النقدية".into(),
                    debit: "0.1".into(),
                    credit: "".into(),
                },
                LineInput {
                    account_id: None,
                    account: "المبيعات".into(),
                    debit: "".into(),
                    credit: "0.10".into(),
                },
            ],
        }
    }
    #[test]
    fn exact_decimal_balance() {
        // gates: allow unwrap
        assert_eq!(validate(&sample()).unwrap().debit, "0.1");
    }
    #[test]
    fn rejects_unbalanced() {
        let mut s = sample();
        s.lines[1].credit = "0.11".into();
        assert!(validate(&s).unwrap_err().contains("غير متوازن"));
    }
    #[test]
    fn rejects_two_sides_one_line() {
        let mut s = sample();
        s.lines[0].credit = "1".into();
        assert!(validate(&s).is_err());
    }
    #[test]
    fn rejects_negative_and_precision() {
        let mut s = sample();
        s.lines[0].debit = "-1".into();
        assert!(validate(&s).is_err());
        s.lines[0].debit = "0.00001".into();
        assert!(validate(&s).is_err());
    }
    #[test]
    fn rejects_bad_date() {
        let mut s = sample();
        s.date = "2026-02-30".into();
        assert!(validate(&s).is_err());
    }
    #[test]
    fn rejects_missing_account() {
        let mut s = sample();
        s.lines[0].account = " ".into();
        assert!(validate(&s).is_err());
    }
    fn balanced_amount(value: &str) -> DraftInput {
        let mut s = sample();
        s.lines[0].debit = value.into();
        s.lines[1].credit = value.into();
        s
    }
    #[test]
    fn description_boundaries_are_enforced() {
        let mut s = sample();
        s.description = " ".into();
        assert!(validate(&s).is_err());
        s.description = "س".repeat(200);
        assert!(validate(&s).is_ok());
        s.description = "س".repeat(201);
        assert!(validate(&s).is_err());
    }
    #[test]
    fn account_name_boundaries_are_enforced() {
        let mut s = sample();
        s.lines[0].account = "ا".repeat(100);
        assert!(validate(&s).is_ok());
        s.lines[0].account = "ا".repeat(101);
        assert!(validate(&s).is_err());
    }
    #[test]
    fn amount_string_length_boundaries_are_enforced() {
        assert!(validate(&balanced_amount(&format!("{}1", "0".repeat(23)))).is_ok());
        assert!(validate(&balanced_amount(&format!("{}1", "0".repeat(24)))).is_err());
    }
    #[test]
    fn amount_precision_and_maximum_boundaries_are_enforced() {
        assert!(validate(&balanced_amount("0.0001")).is_ok());
        assert!(validate(&balanced_amount("0.00001")).is_err());
        assert!(validate(&balanced_amount("1000000000000")).is_ok());
        assert!(validate(&balanced_amount("1000000000000.0001")).is_err());
        assert!(validate(&balanced_amount("-0.0001")).is_err());
    }
    #[test]
    fn zero_or_two_sides_per_line_are_rejected() {
        let mut s = sample();
        s.lines[0].debit = "".into();
        s.lines[0].credit = "".into();
        assert!(validate(&s).unwrap_err().contains("السطر 1 يحتاج"));
        s.lines[0].debit = "0.1".into();
        s.lines[0].credit = "0.1".into();
        assert!(validate(&s).unwrap_err().contains("السطر 1 يحتاج"));
    }
}

#[cfg(test)]
mod proptests;
