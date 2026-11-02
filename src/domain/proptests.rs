//! اختبارات خصائص (Property-based) على منطق المال والقيد.
//!
//! الفكرة: لا نختبر أمثلة ثابتة فقط، بل نولّد آلاف المدخلات ونثبت خصائص
//! يجب أن تصحّ دائمًا — أهمها أن `validate` لا تُسبّب panic أبدًا مهما كان
//! المدخل، وأن التوازن يُحسب بدقة عشرية لا بفاصلة عائمة.
use super::*;
use crate::generated::contracts;
use proptest::prelude::*;

fn any_text() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop::sample::select(vec!['أ', 'ب', '1', '.', '-', ' ', '\n', '0', '٩']),
        0..12,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

fn any_amount() -> impl Strategy<Value = String> {
    (-1_000_000i64..1_000_000).prop_map(|n| format!("{}.{:04}", n / 100, n.unsigned_abs() % 10000))
}

fn any_line() -> impl Strategy<Value = LineInput> {
    (any_text(), any_amount(), any_amount()).prop_map(|(account, debit, credit)| LineInput {
        account_id: None,
        account,
        debit,
        credit,
    })
}

fn any_draft() -> impl Strategy<Value = DraftInput> {
    (
        any_text(),
        any_text(),
        prop::sample::select(vec!["YER", "USD", "SAR", "YER-A", "YER-S", "XXX", ""]),
        prop::collection::vec(any_line(), 0..6),
    )
        .prop_map(|(description, date, currency, lines)| DraftInput {
            description,
            date,
            currency: currency.to_string(),
            lines,
        })
}

proptest! {
    /// أي مدخل، حتى المشوّه، يجب أن يعطي Err أو Ok — لا panic.
    #[test]
    fn validate_never_panics(input in any_draft()) {
        let _ = validate(&input);
    }

    /// القيد المتوازن يُقبل دائمًا، وغير المتوازن يُرفض دائمًا.
    #[test]
    fn balance_is_strict(amount in 1i64..500_000) {
        let value = format!("{}.{:02}", amount / 100, amount % 100);
        let balanced = DraftInput {
            description: "قيد اختبار".into(),
            date: "2026-01-15".into(),
            currency: "YER".into(),
            lines: vec![
                LineInput { account_id: None, account: "1110".into(), debit: value.clone(), credit: "0".into() },
                LineInput { account_id: None, account: "2110".into(), debit: "0".into(), credit: value.clone() },
            ],
        };
        prop_assert!(validate(&balanced).is_ok());

        let mut unbalanced = balanced.clone();
        unbalanced.lines[1].credit = "1".into();
        if value != "1" {
            prop_assert!(validate(&unbalanced).is_err());
        }
    }

    /// العملات المدعومة تأتي من الملف الموحّد فقط، ولا تُقبل غيرها.
    #[test]
    fn currency_gate_matches_contract(code in any_text()) {
        let known = contracts::CURRENCIES.iter().any(|c| c.code == code);
        prop_assert_eq!(supported_currency(&code), known);
    }

    /// تحويل نصّي إلى Decimal ثم إلى نص مرة أخرى لا يفقد أي منزلة.
    #[test]
    fn decimal_roundtrip(units in -10_000_000i64..10_000_000) {
        let original = Decimal::new(units, 4);
        let text = original.to_string();
        let parsed = Decimal::from_str(&text).expect("decimal parse");
        prop_assert_eq!(original, parsed);
    }
}
