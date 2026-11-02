// تبويبا «الضرائب والرسوم» و«ترقيم المستندات» داخل نافذة المالية المتقدمة.
// كل البيانات من /api/finance/* المحمية؛ لا نسب مثبّتة في الواجهة ولا حساب في المتصفح.
(function () {
  'use strict';
  const esc = v => String(v == null ? '' : v)
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  const money = v => Number(v || 0).toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 4 });
  const cid = () => Number(window.FinanceApi.companyId());
  const q = extra => `?company_id=${cid()}${extra || ''}`;

  const KINDS = [
    ['vat', 'ضريبة قيمة مضافة'],
    ['sales', 'ضريبة مبيعات'],
    ['purchase', 'ضريبة مشتريات'],
    ['withholding', 'استقطاع من المنبع']
  ];
  const DOC_TYPES = [
    ['journal', 'قيد يومية'],
    ['receipt_voucher', 'سند قبض'],
    ['payment_voucher', 'سند صرف'],
    ['invoice_sales', 'فاتورة مبيعات'],
    ['invoice_purchase', 'فاتورة مشتريات'],
    ['asset', 'أصل ثابت'],
    ['payroll_run', 'مسير رواتب']
  ];

  async function api(path, options) {
    const res = await window.apiFetch(path, options);
    if (!res.ok) {
      let msg = 'تعذّر إتمام الطلب.';
      try { msg = (await res.json()).error || msg; } catch (_) { /* جسم غير JSON */ }
      throw new Error(msg);
    }
    return res.status === 204 ? {} : res.json();
  }
  const post = (path, body) => api(path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body)
  });

  function table(headers, rows, empty) {
    if (!rows.length) return `<p class="fp-empty">${esc(empty || 'لا توجد بيانات بعد.')}</p>`;
    return `<div class="fp-table-wrap"><table class="fp-table"><thead><tr>${
      headers.map(h => `<th>${esc(h)}</th>`).join('')
    }</tr></thead><tbody>${
      rows.map(r => `<tr>${r.map(c => `<td>${c}</td>`).join('')}</tr>`).join('')
    }</tbody></table></div>`;
  }

  const rateLabel = bp => `${(Number(bp) / 100).toFixed(2)}%`;

  const views = {
    async taxes() {
      const [taxes, accounts] = await Promise.all([
        api(`/api/finance/taxes${q()}`).catch(() => []),
        api(`/api/finance/accounts${q()}`).catch(() => [])
      ]);
      const list = Array.isArray(taxes) ? taxes : (taxes.taxes || []);
      const accs = Array.isArray(accounts) ? accounts : (accounts.accounts || []);
      const accOptions = accs
        .map(a => `<option value="${esc(a.id)}">${esc(a.code)} — ${esc(a.name)}</option>`).join('');
      const rows = list.map(t => [
        esc(t.code),
        esc(t.name),
        esc((KINDS.find(k => k[0] === t.kind) || [, t.kind])[1]),
        rateLabel(t.rate_bp),
        t.is_inclusive ? 'شاملة' : 'غير شاملة',
        accs.find(a => Number(a.id) === Number(t.account_id))
          ? esc(accs.find(a => Number(a.id) === Number(t.account_id)).code) : '—',
        t.archived_at ? '<span class="fp-badge">مؤرشفة</span>' : '<span class="fp-ok">نشطة</span>',
        t.archived_at ? '' : `<button class="fp-link" data-act="tax-archive" data-id="${esc(t.id)}">أرشفة</button>`
      ]);
      return `
        <form class="fp-form" id="fp-tax-form">
          <h3>تعريف ضريبة</h3>
          <div class="fp-grid">
            <label>الرمز <input name="code" maxlength="20" required></label>
            <label>الاسم <input name="name" maxlength="120" required></label>
            <label>النوع <select name="kind">${KINDS.map(k => `<option value="${k[0]}">${esc(k[1])}</option>`).join('')}</select></label>
            <label>النسبة % <input name="rate" type="number" step="0.0001" min="0" max="100" value="15" required></label>
            <label>حساب الضريبة <select name="account_id" required>${accOptions}</select></label>
            <label class="fp-check"><input type="checkbox" name="is_inclusive"> المبالغ شاملة الضريبة</label>
          </div>
          <button type="submit" class="fp-primary">حفظ الضريبة</button>
          <p class="fp-note" id="fp-tax-note"></p>
        </form>
        <h3>الضرائب المعرّفة</h3>
        ${table(['الرمز', 'الاسم', 'النوع', 'النسبة', 'الشمول', 'الحساب', 'الحالة', ''], rows)}
        <form class="fp-form" id="fp-tax-calc">
          <h3>معاينة احتساب</h3>
          <div class="fp-grid">
            <label>الضريبة <select name="tax_id" required>${
              list.filter(t => !t.archived_at).map(t => `<option value="${esc(t.id)}">${esc(t.code)} — ${esc(t.name)}</option>`).join('')
            }</select></label>
            <label>المبلغ <input name="amount" inputmode="decimal" placeholder="0.00" required></label>
          </div>
          <button type="submit">احسب</button>
          <div id="fp-tax-result" class="fp-note"></div>
        </form>`;
    },

    async numbering() {
      const data = await api(`/api/finance/numbering${q()}`).catch(() => ({ sequences: [] }));
      const seqs = data.sequences || [];
      const rows = seqs.map(s => [
        esc((DOC_TYPES.find(d => d[0] === s.doc_type) || [, s.doc_type])[1]),
        esc(s.prefix || '—'),
        esc(s.next_no),
        esc(s.width),
        `<code>${esc(s.preview)}</code>`
      ]);
      const missing = DOC_TYPES.filter(d => !seqs.some(s => s.doc_type === d[0]));
      return `
        <form class="fp-form" id="fp-num-form">
          <h3>ضبط توالٍ</h3>
          <div class="fp-grid">
            <label>نوع المستند <select name="doc_type">${
              DOC_TYPES.map(d => `<option value="${d[0]}">${esc(d[1])}</option>`).join('')
            }</select></label>
            <label>البادئة <input name="prefix" maxlength="12" placeholder="JV"></label>
            <label>عدد الخانات <input name="width" type="number" min="1" max="12" value="5"></label>
            <label>الرقم التالي <input name="next_no" type="number" min="1" value="1"></label>
          </div>
          <button type="submit" class="fp-primary">حفظ التوالٍ</button>
          <p class="fp-note" id="fp-num-note"></p>
        </form>
        <h3>التواليات المسجّلة</h3>
        ${table(['النوع', 'البادئة', 'التالي', 'الخانات', 'المعاينة'], rows, 'لا توجد تواليات بعد.')}
        ${missing.length ? `<p class="fp-note">بلا توالٍ بعد: ${missing.map(m => esc(m[1])).join(' · ')} — تُنشأ تلقائيًا عند أول إصدار.</p>` : ''}
        <form class="fp-form" id="fp-num-issue">
          <h3>إصدار رقم</h3>
          <div class="fp-grid">
            <label>نوع المستند <select name="doc_type">${
              DOC_TYPES.map(d => `<option value="${d[0]}">${esc(d[1])}</option>`).join('')
            }</select></label>
          </div>
          <button type="submit">أصدر الرقم التالي</button>
          <div id="fp-num-result" class="fp-note"></div>
        </form>`;
    },

    async departments() {
      const data = await api(`/api/finance/departments${q()}`).catch(() => []);
      const list = Array.isArray(data) ? data : [];
      const rows = list.map(d => [
        esc(d.code),
        esc(d.name),
        esc(d.manager_name || '—'),
        esc(d.depth),
        d.parent_id ? esc((list.find(x => Number(x.id) === Number(d.parent_id)) || {}).name || d.parent_id) : 'جذر',
        d.archived_at ? '<span class="fp-badge">مؤرشف</span>' : '<span class="fp-ok">نشط</span>',
        d.archived_at ? '' : `<button class="fp-link" data-act="dept-archive" data-id="${esc(d.id)}">أرشفة</button>`
      ]);
      const parents = list.filter(d => !d.archived_at)
        .map(d => `<option value="${esc(d.id)}">${esc(d.code)} — ${esc(d.name)}</option>`).join('');
      return `
        <form class="fp-form" id="fp-dept-form">
          <h3>إضافة قسم</h3>
          <div class="fp-grid">
            <label>الرمز <input name="code" maxlength="20" required placeholder="SALES-01"></label>
            <label>الاسم <input name="name" maxlength="120" required></label>
            <label>القسم الأب <select name="parent_id"><option value="">— قسم جذري —</option>${parents}</select></label>
            <label>المسؤول <input name="manager_name" maxlength="120"></label>
          </div>
          <button type="submit" class="fp-primary">حفظ القسم</button>
          <p class="fp-note" id="fp-dept-note"></p>
        </form>
        <h3>شجرة الأقسام</h3>
        ${table(['الرمز', 'الاسم', 'المسؤول', 'العمق', 'الأب', 'الحالة', ''], rows, 'لا توجد أقسام بعد.')}`;
    },
  };

  // العقد مع لوحة المالية: bind(اسم التبويب، حاوية العرض، دالة إعادة الرسم).
  function bind(tab, scope, refresh) {
    const note = (id, text, bad) => {
      const el = scope.querySelector(id);
      if (el) { el.textContent = text; el.classList.toggle('fp-error', !!bad); }
    };


    const deptForm = scope.querySelector('#fp-dept-form');
    if (deptForm) {
      deptForm.addEventListener('submit', async ev => {
        ev.preventDefault();
        const f = new FormData(deptForm);
        const parent = String(f.get('parent_id') || '').trim();
        try {
          await post('/api/finance/departments', {
            company_id: cid(),
            code: String(f.get('code') || '').trim(),
            name: String(f.get('name') || '').trim(),
            parent_id: parent ? Number(parent) : null,
            manager_name: String(f.get('manager_name') || '').trim() || null
          });
          note('#fp-dept-note', 'حُفظ القسم.');
          if (typeof refresh === 'function') await refresh();
        } catch (e) {
          note('#fp-dept-note', e.message, true);
        }
      });
    }

    scope.querySelectorAll('[data-act="dept-archive"]').forEach(btn => {
      btn.addEventListener('click', async () => {
        if (!window.confirm('أرشفة هذا القسم؟')) return;
        try {
          await post('/api/finance/departments/archive', { company_id: cid(), id: Number(btn.dataset.id) });
          if (typeof refresh === 'function') await refresh();
        } catch (e) { window.alert(e.message); }
      });
    });

    const taxForm = scope.querySelector('#fp-tax-form');
    if (taxForm) {
      taxForm.addEventListener('submit', async ev => {
        ev.preventDefault();
        const f = new FormData(taxForm);
        const pct = Number(f.get('rate'));
        if (!Number.isFinite(pct) || pct < 0 || pct > 100) {
          note('#fp-tax-note', 'النسبة يجب أن تكون بين 0 و100.', true);
          return;
        }
        try {
          await post('/api/finance/taxes', {
            company_id: cid(),
            code: String(f.get('code') || '').trim(),
            name: String(f.get('name') || '').trim(),
            kind: String(f.get('kind')),
            rate_bp: Math.round(pct * 100),
            account_id: Number(f.get('account_id')),
            is_inclusive: f.get('is_inclusive') === 'on'
          });
          note('#fp-tax-note', 'حُفظت الضريبة.');
          if (typeof refresh === 'function') await refresh();
        } catch (e) {
          note('#fp-tax-note', e.message, true);
        }
      });
    }

    const calc = scope.querySelector('#fp-tax-calc');
    if (calc) {
      calc.addEventListener('submit', async ev => {
        ev.preventDefault();
        const f = new FormData(calc);
        try {
          const r = await api(`/api/finance/taxes/compute${q(
            `&tax_id=${encodeURIComponent(f.get('tax_id'))}&amount=${encodeURIComponent(f.get('amount'))}`)}`);
          note('#fp-tax-result',
            `${esc(r.tax)} — الوعاء ${money(r.base)} · الضريبة ${money(r.tax_value)} · الإجمالي ${money(r.total)}`);
        } catch (e) {
          note('#fp-tax-result', e.message, true);
        }
      });
    }

    const numForm = scope.querySelector('#fp-num-form');
    if (numForm) {
      numForm.addEventListener('submit', async ev => {
        ev.preventDefault();
        const f = new FormData(numForm);
        try {
          await post('/api/finance/numbering', {
            company_id: cid(),
            doc_type: String(f.get('doc_type')),
            prefix: String(f.get('prefix') || '').trim(),
            width: Number(f.get('width') || 5),
            next_no: Number(f.get('next_no') || 1)
          });
          note('#fp-num-note', 'حُفظ التوالٍ.');
        } catch (e) {
          note('#fp-num-note', e.message, true);
        }
      });
    }

    const issue = scope.querySelector('#fp-num-issue');
    if (issue) {
      issue.addEventListener('submit', async ev => {
        ev.preventDefault();
        const f = new FormData(issue);
        try {
          const r = await post('/api/finance/numbering/next', {
            company_id: cid(),
            doc_type: String(f.get('doc_type'))
          });
          note('#fp-num-result', `الرقم الصادر: ${esc(r.number)}`);
        } catch (e) {
          note('#fp-num-result', e.message, true);
        }
      });
    }

    scope.querySelectorAll('[data-act="tax-archive"]').forEach(btn => {
      btn.addEventListener('click', async () => {
        if (!window.confirm('أرشفة هذه الضريبة؟')) return;
        try {
          await post('/api/finance/taxes/archive', { company_id: cid(), id: Number(btn.dataset.id) });
          btn.closest('tr').style.opacity = '0.4';
        } catch (e) {
          window.alert(e.message);
        }
      });
    });
  }

  window.FinanceOps3 = {
    tabs: [
      ['taxes', 'الضرائب والرسوم'],
      ['numbering', 'ترقيم المستندات'],
      ['departments', 'الهيكل الإداري']
    ],
    has: tab => Object.prototype.hasOwnProperty.call(views, tab),
    view: tab => views[tab](),
    bind
  };
})();
