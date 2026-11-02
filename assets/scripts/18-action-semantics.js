// 18-action-semantics.js — طبقة واحدة تُطبِّق قواعد المرجع على كل الشاشات:
// 1) لكل زر دلالة لونية واحدة: أساسي · حيادي · خطر · تحذير · نجاح.
// 2) لكل مبلغ مدين/دائن لونه.
// 3) بيان المستند يُعرض بهوية واضحة: «نوع + رقم — طرف» بفواصل بصرية.
(function () {
  'use strict';

  // كلمات تدلّ على دلالة الفعل — مستخرجة من أزرار المرجع الثمانية.
  const DANGER = ['حذف', 'أرشفة', 'رفض', 'فك ربط', 'إبطال', 'إلغاء المستند', 'مسح', 'إزالة', 'سحب'];
  const WARN = ['عكسي', 'قيد عكسي', 'تراجع', 'إعادة فتح', 'إلغاء ترحيل', 'تعليق', 'إيقاف', 'تصحيح'];
  const SUCCESS = ['ترحيل', 'اعتماد', 'تقديم', 'إقفال', 'تأكيد', 'تنفيذ', 'احتساب', 'تطبيق', 'إقرار'];
  const NEUTRAL = ['إلغاء', 'عرض', 'تحديث', 'إغلاق', 'رجوع', 'بحث', 'تصدير', 'طباعة', 'إضافة', 'بند', 'تعديل', 'اختيار', 'اتصال', 'جلب'];
  const PRIMARY = ['حفظ', 'إنشاء', 'جديد', 'اضافة', 'إضافة بند', 'احتساب', 'تنفيذ', 'معاينة'];

  function classify(text, hint) {
    const label = (text || '').trim();
    // زر صامت (أيقونة فقط) يأخذ وصفه من حارس الإتاحة، ولا يُلوَّن بلا دلالة نصية.
    if (!label) return '';
    // التلميح الصريح من الكود يتقدّم، ولا يُستنتج من أسماء الأصناف (ghost ليست دلالة فعل).
    const marked = (hint || '').toLowerCase();
    if (marked.includes('danger')) return 'danger';
    if (marked.includes('warn')) return 'warn';
    if (marked.includes('success')) return 'success';
    if (marked.includes('neutral') || marked.includes('ghost')) return 'neutral';
    if (marked.includes('primary')) return 'primary';
    if (DANGER.some(word => label.includes(word))) return 'danger';
    if (WARN.some(word => label.includes(word))) return 'warn';
    if (SUCCESS.some(word => label.includes(word))) return 'success';
    if (NEUTRAL.some(word => label === word || label.startsWith(word))) return 'neutral';
    if (PRIMARY.some(word => label.includes(word))) return 'primary';
    return '';
  }

  const CLASS = {
    primary: 'fp-act act-primary',
    neutral: 'fp-act act-neutral',
    danger: 'fp-act act-danger',
    warn: 'fp-act act-warn',
    success: 'fp-act act-success'
  };
  const TITLE = {
    primary: 'فعل رئيسي',
    neutral: 'فعل مساعد',
    danger: 'فعل لا رجعة فيه',
    warn: 'فعل يغيّر أثرًا محاسبيًا',
    success: 'فعل إتمام'
  };

  function paintButtons(scope) {
    scope.querySelectorAll('button').forEach(button => {
      if (button.dataset.actPainted === '1') return;
      const hint = [button.dataset.act, button.dataset.action, button.dataset.kind].filter(Boolean).join(' ');
      const kind = classify(button.innerText, hint);
      if (!kind) return;
      // أزرار التنقل بين التبويبات والقوائم لا تُلوَّن: ليست أفعالًا على البيانات.
      if (button.closest('.fp-tabs, .finance-tab-bar, nav')) return;
      // الأزرار التي تلبس صنفًا صريحًا في المرسوم لا تُعاد صباغتها.
      if (!/fp-(primary|ghost|danger|warn|success|act)\b/.test(button.className)) {
        button.className = `${button.className} ${CLASS[kind]}`.trim();
      }
      button.dataset.actPainted = '1';
      button.dataset.actKind = kind;
      if (!button.title) button.title = TITLE[kind];
    });
  }

  // أعمدة المدين/الدائن: تُلوَّن بحسب رأس العمود لا بحسب الرقم.
  function paintAmounts(scope) {
    scope.querySelectorAll('table').forEach(table => {
      const heads = Array.from(table.querySelectorAll('thead th')).map(th => (th.innerText || '').trim());
      if (!heads.length) return;
      const debitAt = heads.findIndex(h => h.includes('مدين'));
      const creditAt = heads.findIndex(h => h.includes('دائن'));
      if (debitAt < 0 && creditAt < 0) return;
      table.querySelectorAll('tbody tr').forEach(row => {
        const cells = row.children;
        if (debitAt >= 0 && cells[debitAt]) cells[debitAt].classList.add('amt-debit');
        if (creditAt >= 0 && cells[creditAt]) cells[creditAt].classList.add('amt-credit');
      });
    });
  }

  // «بيع INV-2026-0001 — عميل نقدي» → نوع + رقم + طرف بفواصل بصرية.
  const DOC_KINDS = ['بيع', 'شراء', 'مرتجع بيع', 'مرتجع شراء', 'سند قبض', 'سند صرف',
    'إشعار مدين', 'إشعار دائن', 'تحصيل', 'سداد', 'رواتب', 'مصروف', 'استحقاق الزكاة',
    'إقرار ضريبي', 'قيد', 'تسوية', 'رصيد افتتاحي', 'إقفال', 'عرض سعر', 'طلب عميل'];

  // اشتقاق نوع المستند من نص البيان — نفس منطق الخادم، فلا يظهر لون بلا معنى.
  function kindFromText(text) {
    if (text.includes('مرتجع')) return 'مرتجع بيع';
    if (/مبيعات|بيع|إيرادات|خدمات/.test(text)) return 'بيع';
    if (text.includes('تحصيل')) return 'تحصيل';
    if (/مشتريات|شراء/.test(text)) return 'شراء';
    if (text.includes('سداد')) return 'سداد';
    if (text.includes('رواتب')) return 'رواتب';
    if (/إيجار|كهرباء|اتصالات|مصاريف|مصروف/.test(text)) return 'مصروف';
    if (text.includes('زكاة')) return 'استحقاق الزكاة';
    if (text.includes('إشعار')) return 'إشعار';
    return 'قيد';
  }

  const cellText = (row, label) => {
    const cell = row.querySelector(`td[data-label="${label}"]`);
    return cell ? (cell.textContent || '').trim() : '';
  };

  function decorateDescriptions(scope) {
    scope.querySelectorAll('.fp-table tbody tr').forEach(row => {
      const cell = row.querySelector('td[data-label="البيان"], td.fp-desc');
      if (!cell || cell.dataset.decorated === '1') return;
      const raw = (cell.textContent || '').trim();
      if (!raw) return;
      // 1) بيان مكتوب أصلاً بصيغة المستند: نُبرزه كما هو.
      const kind = DOC_KINDS.find(k => raw.startsWith(k + ' ') || raw === k);
      if (kind) {
        const rest = raw.slice(kind.length).trim();
        const [numberPart, partyPart] = rest.split(' — ');
        cell.innerHTML = `<span class="fp-doc-line">
            <span class="fp-doc-kind">${kind}</span>
            ${numberPart ? `<span class="fp-doc-no">${numberPart.trim()}</span>` : ''}
            ${partyPart ? `<span class="fp-doc-sep">—</span><span class="fp-doc-party">${partyPart.trim()}</span>` : ''}
          </span>`;
        cell.dataset.decorated = '1';
        return;
      }
      // 2) بيان قديم مقفل (قيد مرحّل لا يُعدَّل): تُضاف هوية مستند مشتقّة للعرض فقط.
      const idText = cellText(row, '#') || cellText(row, 'الرقم');
      const dateText = cellText(row, 'التاريخ');
      if (!idText || !/^\d+$/.test(idText) || !dateText) return;
      const year = (dateText.match(/^(\d{4})/) || [])[1] || '';
      const number = `JE-${year}-${String(idText).padStart(4, '0')}`;
      cell.innerHTML = `<span class="fp-doc-line">
          <span class="fp-doc-kind">${kindFromText(raw)}</span>
          <span class="fp-doc-no" title="هوية مشتقّة من رقم القيد وسنته — البيان الأصلي محفوظ كما هو">${number}</span>
          <span class="fp-doc-sep">—</span><span class="fp-doc-party">${raw.replace(/ — /g, ' · ')}</span>
        </span>`;
      cell.dataset.decorated = '1';
    });
  }

  function apply(scope) {
    const root = scope || document;
    paintButtons(root);
    paintAmounts(root);
    decorateDescriptions(root);
  }

  // يُطبَّق بعد كل رسم للوحة (لا تعديل على المنطق القائم).
  let observer = null;
  function watch() {
    const body = document.getElementById('fp-body');
    if (!body) return false;
    if (observer) observer.disconnect();
    observer = new MutationObserver(() => apply(body));
    observer.observe(body, { childList: true, subtree: true });
    apply(body);
    return true;
  }

  function boot() {
    if (window.__actSemantics) return;
    window.__actSemantics = true;
    if (!watch()) setTimeout(boot, 400);
    // كل فتح لشاشة مالية يعيد التطبيق (اللوحة تُعاد بناؤها).
    const hub = window.FinanceHub;
    if (hub && !hub.__actHooked) {
      const original = hub.show.bind(hub);
      hub.show = tab => { original(tab); setTimeout(() => apply(document), 260); };
      hub.__actHooked = true;
    }
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', boot);
  } else {
    boot();
  }
  window.FinanceActions = { apply, classify };
})();
