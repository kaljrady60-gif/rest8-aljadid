//! صياغة «البيان» في القيود بالطريقة المرجعية نفسها.
//!
//! قاعدة المرجع المستخرجة من محرّك الترحيل:
//! * مستند مرقّم (فاتورة/مرتجع): `«<نوع المستند> <رقمه> — <اسم الطرف>»` مثل: `بيع 2026-014 — شركة النور`
//! * سند قبض/صرف: `«<اسم الطرف> — <بيان السند>»` مثل: `مؤسسة الأمل — دفعة من حساب أكتوبر`
//!
//! والمرجع نفسه يُثبت رقم المستند في حقل `reference` للسطر، ويترك البيان موحّدًا في الرأس والسطور.
//! هذا يجعل القيد مقروءًا في دفتر الأستاذ وميزان المراجعة دون فتح المستند الأصلي.

/// أنواع المستندات المعروفة عندنا، وتسميتها العربية المهنية.
pub fn document_label(kind: &str) -> &'static str {
    match kind {
        "sale" => "بيع",
        "purchase" => "شراء",
        "sale_return" => "مرتجع بيع",
        "purchase_return" => "مرتجع شراء",
        "quotation" => "عرض سعر",
        "order" => "طلب عميل",
        "receipt" => "سند قبض",
        "payment" => "سند صرف",
        "debit_note" => "إشعار مدين",
        "credit_note" => "إشعار دائن",
        "zakat" => "استحقاق الزكاة",
        "tax_return" => "إقرار ضريبي",
        "adjustment" => "تسوية",
        "opening" => "رصيد افتتاحي",
        "closing" => "إقفال",
        "payroll" => "رواتب",
        "fx" => "صرف عملة",
        _ => "قيد",
    }
}

/// تنظيف النص: بلا مسافات زائدة ولا شرطات مكررة، وحتى الشرطة تُعرض بالشكل المرجعي.
fn tidy(raw: &str) -> String {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
        .replace(" -- ", " — ")
        .replace(" - ", " — ")
        .replace("— —", "—")
        .trim_matches(['—', ' ', '-'])
        .to_string()
}

/// `بيع 2026-014 — شركة النور`
pub fn compose(label: &str, number: &str, party: &str) -> String {
    let label = tidy(label);
    let number = tidy(number);
    let party = tidy(party);
    match (number.is_empty(), party.is_empty()) {
        (false, false) => format!("{label} {number} — {party}"),
        (false, true) => format!("{label} {number}"),
        (true, false) => format!("{label} — {party}"),
        (true, true) => label,
    }
}

/// `مؤسسة الأمل — دفعة من حساب أكتوبر` (السندات في المرجع بهذا الترتيب)
pub fn compose_voucher(party: &str, body: &str) -> String {
    let party = tidy(party);
    let body = tidy(body);
    match (party.is_empty(), body.is_empty()) {
        (false, false) => format!("{party} — {body}"),
        (false, true) => party,
        (true, false) => body,
        (true, true) => "سند".to_string(),
    }
}

/// توحيد بيان قديم مكتوب يدويًا إلى الصيغة المهنية بلا فقدان معناه.
/// يُستخدم في الترحيل والإصلاح، ولا يغيّر بيانًا فارغًا.
pub fn normalize(raw: &str) -> String {
    let tidied = tidy(raw);
    if tidied.is_empty() {
        return tidied;
    }
    if tidied.contains(" — ") {
        return tidied;
    }
    tidied
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invoice_style_matches_reference() {
        assert_eq!(
            compose("بيع", "2026-014", "شركة النور"),
            "بيع 2026-014 — شركة النور"
        );
    }

    #[test]
    fn voucher_style_puts_party_first() {
        assert_eq!(
            compose_voucher("مؤسسة الأمل", "دفعة من حساب أكتوبر"),
            "مؤسسة الأمل — دفعة من حساب أكتوبر"
        );
    }

    #[test]
    fn missing_parts_do_not_leave_dangling_dashes() {
        assert_eq!(compose("بيع", "", "شركة النور"), "بيع — شركة النور");
        assert_eq!(compose("بيع", "2026-014", ""), "بيع 2026-014");
        assert_eq!(compose_voucher("", "دفعة"), "دفعة");
    }

    #[test]
    fn normalize_collapses_spaces_and_dashes() {
        assert_eq!(normalize("  بيع   2026-014   "), "بيع 2026-014");
        assert_eq!(normalize("بيع - شركة"), "بيع — شركة");
        assert_eq!(normalize(""), "");
    }
}
