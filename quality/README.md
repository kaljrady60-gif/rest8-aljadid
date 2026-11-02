# نظام الجودة في «المحاسب الذكي»

> كل ما في هذا المجلد مجاني ومفتوح المصدر ويعمل محليًا بلا خدمات مدفوعة أو مفاتيح.

## التشغيل السريع

```bash
python3 quality/gates.py --list      # قائمة الفحوص
python3 quality/gates.py             # الفحوص السريعة (عقود، حجم، نظافة، Python)
python3 quality/gates.py fmt clippy test deps   # بوابات Rust الكاملة
python3 quality/gates.py gen         # توليد العقود بعد تعديل contracts.toml
```

## الملف الموحّد (Single Source of Truth)

`quality/contracts.toml` هو المصدر الوحيد لـ: العملات، تصنيفات الحسابات،
أنواع الصناديق، حالات الفترات، الأدوار وترتيبها، سقوف الحجم، إعدادات الأمان،
شاشات المالية المنفَّذة، والتقارير المالية.

عند أي إضافة:

1. عدّل `contracts.toml` فقط.
2. نفّذ `python3 quality/gates.py gen`.
3. تُولَّد آليًا: `src/generated/contracts.rs`، `assets/scripts/00-contracts.js`،
   `templates/components/contracts.html`.
4. بوابة `contracts` تفشل إن نُسي التوليد، فلا يحدث انحراف صامت بين الطبقات.

## البوابات

| البوابة | الأداة | ماذا تمنع |
|---|---|---|
| `contracts` | مولّد داخلي | انحراف الملفات المولّدة عن الملف الموحّد |
| `size` | حارس داخلي | أي ملف يتجاوز 400 سطر (صيانة سهلة) |
| `hygiene` | حارس داخلي | `unsafe`، أنواع عائمة، `unwrap` في الإنتاج، مسارات بلا نطاق |
| `py` | Ruff | أخطاء وأسلوب سكربتات Python |
| `fmt` | rustfmt | اختلاف التنسيق |
| `clippy` | clippy `-D warnings` | تحذيرات، وأنواع ممنوعة (f32/f64) |
| `test` | cargo-nextest | فشل أي اختبار |
| `deps` | cargo-machete + cargo-deny | تبعيات غير مستخدمة، ثغرات، تراخيص، تكرار |

## فحوص ثقيلة (يدوية/ليلية)

```bash
cargo llvm-cov --summary-only          # التغطية
cargo mutants --file src/finance/money.rs --timeout 60   # قوة الاختبارات
```

## أين تُستثنى القواعد

الاختبارات (`tests/`، `api_tests/`، `*tests.rs`) مستثناة من قواعد `hygiene`
لأن الاختبار يفشل صراحةً. الاستثناء الموضعي في الإنتاج يكون بتعليق صريح:

```rust
// gates: allow unwrap
```

كل استثناء مكتوب ومقصود، لا صامت.
