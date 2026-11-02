// تبويبات الأصول الثابتة والرواتب داخل نافذة «المالية المتقدمة».
// تعتمد نفس نمط التمرير المفوَّض المستخدم في 09-finance-ops.js، وتقرأ الحسابات من
// دليل الشركة فلا تُثبَّت أرقام حسابات في الواجهة.
(function () {
  'use strict';
  const esc = v => String(v == null ? '' : v)
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  const money = v => Number(v || 0).toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 4 });
  const cid = () => Number(window.FinanceApi.companyId());
  const month = () => new Date().toISOString().slice(0, 7);

  const TABS = [
    ['assets', 'الأصول الثابتة'],
    ['payroll', 'الرواتب'],
    ['branches', 'الفروع']
  ];

  async function get(path) {
    const res = await window.apiFetch(path);
    if (!res.ok) {
      let msg = 'تعذّر تحميل البيانات.';
      try { msg = (await res.json()).error || msg; } catch (_) { /* جسم غير JSON */ }
      throw new Error(msg);
    }
    return res.json();
  }

  async function post(path, body) {
    const res = await window.apiFetch(path, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body)
    });
    if (!res.ok) {
      let msg = 'تعذّر تنفيذ العملية.';
      try { msg = (await res.json()).error || msg; } catch (_) { /* جسم غير JSON */ }
      throw new Error(msg);
    }
    return res.json().catch(() => ({}));
  }

  function table(headers, rows, empty = 'لا توجد بيانات بعد.') {
    if (!rows.length) return `<p class="fp-empty">${esc(empty)}</p>`;
    return `<div class="fp-table-wrap"><table class="fp-table"><thead><tr>${
      headers.map(h => `<th>${esc(h)}</th>`).join('')}</tr></thead><tbody>${
      rows.map(r => `<tr>${r.map(c => `<td>${c}</td>`).join('')}</tr>`).join('')
    }</tbody></table></div>`;
  }

  async function accounts() {
    const body = await get(`/api/finance/ledger?company_id=${cid()}`);
    return (body.accounts || []).filter(a => a.kind === 'detail');
  }

  const views = {
    async branches() {
      const items = await get(`/api/finance/branches?company_id=${cid()}`);
      const rows = items.map(b => [
        esc(b.code), esc(b.name), esc(b.entries), money(b.debit),
        String(window.FinanceApi.branchId()) === String(b.id)
          ? '<span class="fp-state ok">محدد في التقارير</span>'
          : `<button type="button" class="fp-ghost" data-branch="${esc(b.id)}">تقييد التقارير به</button>`
      ]);
      return `<details class="fp-form"><summary>＋ فرع جديد</summary>
        <form data-ops2="branch">
          <label>الرمز<input name="code" required maxlength="20"></label>
          <label>الاسم<input name="name" required maxlength="80"></label>
          <label>المدينة<input name="city" maxlength="60"></label>
          <button type="submit" class="fp-btn">حفظ الفرع</button>
          <p class="fp-form-error" role="alert"></p>
        </form></details>
        <p class="fp-note">${items.length} فرعًا · كل قيد يُسجَّل على فرعه، وتقارير الشاشة والطباعة والتصدير تتبع الفرع المحدد هنا أو تعود للموحّد.</p>` +
        table(['الرمز', 'الفرع', 'عدد القيود', 'إجمالي المدين', 'التقييد'], rows);
    },

    async assets() {
      const [list, categories, details] = await Promise.all([
        get(`/api/finance/assets?company_id=${cid()}`),
        get(`/api/finance/assets/categories?company_id=${cid()}`),
        accounts()
      ]);
      const accountOptions = details.map(a => `<option value="${esc(a.account_id)}">${esc(a.code)} · ${esc(a.name)}</option>`).join('');
      const rows = list.items.map(a => [
        esc(a.code), esc(a.name), esc(a.category), esc(a.acquired_on),
        money(a.cost), money(a.accumulated), money(a.book_value), money(a.monthly),
        `<span class="fp-state${a.status === 'active' ? ' ok' : ''}">${a.status === 'active' ? 'قائم' : 'مستبعد'}</span>`,
        a.status === 'active'
          ? `<button type="button" class="fp-ghost" data-dispose="${esc(a.id)}">استبعاد</button>`
          : '—'
      ]);
      const categoryRows = categories.map(c => [
        esc(c.name), c.method === 'declining' ? 'متناقص' : 'قسط ثابت',
        c.method === 'declining' ? esc(c.annual_rate) + '%' : '—',
        esc(c.asset_account), esc(c.accum_account), esc(c.expense_account)
      ]);
      return `<details class="fp-form"${categories.length ? '' : ' open'}><summary>＋ تصنيف أصول</summary>
        <form data-ops2="asset-category">
          <label>اسم التصنيف<input name="name" required maxlength="60"></label>
          <label>طريقة الإهلاك<select name="method">
            <option value="straight">قسط ثابت</option><option value="declining">متناقص</option>
          </select></label>
          <label>النسبة السنوية %<input name="annual_rate" inputmode="decimal" value="0"></label>
          <label>حساب الأصل<select name="asset_account_id" required>${accountOptions}</select></label>
          <label>حساب مجمع الإهلاك<select name="accum_account_id" required>${accountOptions}</select></label>
          <label>حساب مصروف الإهلاك<select name="expense_account_id" required>${accountOptions}</select></label>
          <button type="submit" class="fp-btn">حفظ التصنيف</button>
          <p class="fp-form-error" role="alert"></p>
        </form></details>
        <details class="fp-form"${categories.length ? ' open' : ''}><summary>＋ أصل جديد</summary>
        <form data-ops2="asset">
          <label>التصنيف<select name="category_id" required>${categories.map(c =>
            `<option value="${esc(c.id)}">${esc(c.name)}</option>`).join('')}</select></label>
          <label>الرمز<input name="code" required maxlength="30"></label>
          <label>اسم الأصل<input name="name" required maxlength="120"></label>
          <label>تاريخ الشراء<input type="date" name="acquired_on" required></label>
          <label>التكلفة<input name="cost" required inputmode="decimal"></label>
          <label>القيمة التخريدية<input name="salvage" inputmode="decimal" value="0"></label>
          <label>العمر بالأشهر<input name="life_months" type="number" min="1" max="600" value="60" required></label>
          <label>التمويل<select name="funded_from">
            <option value="bank">بنك</option><option value="cash">نقدًا</option>
            <option value="payable">على الحساب</option><option value="capital">رأس المال</option>
          </select></label>
          <button type="submit" class="fp-btn">تسجيل الأصل وترحيل الشراء</button>
          <p class="fp-form-error" role="alert"></p>
        </form></details>
        <details class="fp-form"><summary>⚙ تشغيل الإهلاك الشهري</summary>
        <form data-ops2="depreciate">
          <label>الشهر<input name="period" value="${month()}" pattern="\\d{4}-\\d{2}" required></label>
          <button type="submit" class="fp-btn">ترحيل إهلاك الشهر</button>
          <p class="fp-form-error" role="alert"></p>
        </form></details>
        <p class="fp-note">${list.items.length} أصلًا · التكلفة ${money(list.totals.cost)} · مجمع الإهلاك ${money(list.totals.accumulated)} · القيمة الدفترية ${money(list.totals.book_value)}. الإهلاك يعمل مرة واحدة لكل أصل في الشهر.</p>` +
        table(['الرمز', 'الأصل', 'التصنيف', 'الشراء', 'التكلفة', 'مجمع الإهلاك', 'القيمة الدفترية', 'قسط شهري', 'الحالة', 'إجراء'], rows) +
        `<h4 class="fp-sub">تصنيفات الإهلاك</h4>` +
        table(['التصنيف', 'الطريقة', 'النسبة', 'حساب الأصل', 'مجمع الإهلاك', 'مصروف الإهلاك'], categoryRows, 'لا توجد تصنيفات بعد.');
    },

    async payroll() {
      const [employees, runs, funds] = await Promise.all([
        get(`/api/finance/payroll/employees?company_id=${cid()}`),
        get(`/api/finance/payroll/runs?company_id=${cid()}`),
        get(`/api/finance/funds?company_id=${cid()}`)
      ]);
      const empRows = employees.map(e => [
        esc(e.code), esc(e.name), esc(e.job_title || '—'), esc(e.hired_on),
        money(e.base_salary), money(e.allowances), money(e.deductions), money(e.net),
        `<span class="fp-state${e.status === 'active' ? ' ok' : ''}">${e.status === 'active' ? 'نشط' : 'موقوف'}</span>`
      ]);
      const runRows = runs.map(r => {
        const actions = [];
        if (r.status === 'draft') actions.push(`<button type="button" class="fp-ghost" data-run="${esc(r.id)}" data-do="post">ترحيل المسير</button>`);
        if (r.status === 'posted') actions.push(`<button type="button" class="fp-ghost" data-run="${esc(r.id)}" data-do="pay">صرف</button>`);
        return [
          `<span data-run-id="${esc(r.id)}" title="اعرض كشف المسير">${esc(r.period)}</span>`,
          esc(r.employees), money(r.gross), money(r.deductions), money(r.net),
          `<span class="fp-state${r.status === 'paid' ? ' ok' : ''}">${
            ({ draft: 'مسودة', posted: 'مرحّل', paid: 'مدفوع', cancelled: 'ملغى' })[r.status] || esc(r.status)}</span>`,
          r.entry_id ? '#' + esc(r.entry_id) : '—',
          r.payment_entry_id ? '#' + esc(r.payment_entry_id) : '—',
          actions.join(' ') || '—'
        ];
      });
      return `<details class="fp-form"><summary>＋ موظف جديد</summary>
        <form data-ops2="employee">
          <label>الرمز<input name="code" required maxlength="30"></label>
          <label>الاسم<input name="name" required maxlength="120"></label>
          <label>الوظيفة<input name="job_title" maxlength="80"></label>
          <label>تاريخ التعيين<input type="date" name="hired_on" required></label>
          <label>الراتب الأساسي<input name="base_salary" required inputmode="decimal"></label>
          <label>البدلات<input name="allowances" inputmode="decimal" value="0"></label>
          <label>الاستقطاعات<input name="deductions" inputmode="decimal" value="0"></label>
          <button type="submit" class="fp-btn">حفظ الموظف</button>
          <p class="fp-form-error" role="alert"></p>
        </form></details>
        <details class="fp-form"${employees.length ? '' : ' open'}><summary>⚙ مسير رواتب شهري</summary>
        <form data-ops2="run">
          <label>الشهر<input name="period" value="${month()}" pattern="\\d{4}-\\d{2}" required></label>
          <button type="submit" class="fp-btn">تكوين المسير</button>
          <p class="fp-form-error" role="alert"></p>
        </form></details>
        <p class="fp-note">${employees.length} موظفًا نشطًا · ${runs.length} مسيرًا · الترحيل ينشئ قيدًا واحدًا: مصروف الرواتب مقابل صافي مستحق واستقطاعات${
          funds.length ? '، والصرف يخصم من الصندوق المحدد' : ''}.</p>` +
        table(['الرمز', 'الموظف', 'الوظيفة', 'التعيين', 'الأساسي', 'البدلات', 'الاستقطاعات', 'الصافي', 'الحالة'], empRows) +
        `<h4 class="fp-sub">مسيرات الرواتب</h4>` +
        table(['الشهر', 'الموظفون', 'الإجمالي', 'الاستقطاعات', 'الصافي', 'الحالة', 'قيد الترحيل', 'قيد الصرف', 'إجراء'], runRows, 'لا توجد مسيرات بعد.');
    }
  };

  function formValue(form, name) {
    const d = new FormData(form);
    const v = d.get(name);
    return v === null ? '' : String(v).trim();
  }

  const SUBMIT = {
    async 'asset-category'(form) {
      return post('/api/finance/assets/categories', {
        company_id: cid(),
        name: formValue(form, 'name'),
        method: formValue(form, 'method'),
        annual_rate: formValue(form, 'annual_rate') || '0',
        asset_account_id: Number(formValue(form, 'asset_account_id')),
        accum_account_id: Number(formValue(form, 'accum_account_id')),
        expense_account_id: Number(formValue(form, 'expense_account_id'))
      });
    },
    async asset(form) {
      return post('/api/finance/assets', {
        company_id: cid(),
        category_id: Number(formValue(form, 'category_id')),
        code: formValue(form, 'code'),
        name: formValue(form, 'name'),
        acquired_on: formValue(form, 'acquired_on'),
        cost: formValue(form, 'cost'),
        salvage: formValue(form, 'salvage') || '0',
        life_months: Number(formValue(form, 'life_months')),
        funded_from: formValue(form, 'funded_from')
      });
    },
    async depreciate(form) {
      return post('/api/finance/assets/depreciate', {
        company_id: cid(), period: formValue(form, 'period')
      });
    },
    async employee(form) {
      return post('/api/finance/payroll/employees', {
        company_id: cid(),
        code: formValue(form, 'code'),
        name: formValue(form, 'name'),
        job_title: formValue(form, 'job_title') || null,
        hired_on: formValue(form, 'hired_on'),
        base_salary: formValue(form, 'base_salary'),
        allowances: formValue(form, 'allowances') || '0',
        deductions: formValue(form, 'deductions') || '0'
      });
    },
    async branch(form) {
      return post('/api/finance/branches', {
        company_id: cid(), code: formValue(form, 'code'), name: formValue(form, 'name'),
        city: formValue(form, 'city') || null
      });
    },
    async run(form) {
      return post('/api/finance/payroll/runs', {
        company_id: cid(), period: formValue(form, 'period')
      });
    }
  };

  function bind(tab, scope, reload) {
    scope.querySelectorAll('form[data-ops2]').forEach(form => {
      const kind = form.dataset.ops2;
      form.addEventListener('submit', async event => {
        event.preventDefault();
        const error = form.querySelector('.fp-form-error');
        const button = form.querySelector('button[type=submit]');
        error.textContent = '';
        button.disabled = true;
        try {
          await SUBMIT[kind](form);
          reload();
        } catch (e) {
          error.textContent = e.message || 'تعذّر الحفظ.';
        } finally {
          button.disabled = false;
        }
      });
    });
    scope.querySelectorAll('[data-dispose]').forEach(btn => {
      btn.addEventListener('click', async () => {
        const proceeds = window.prompt('قيمة البيع (اتركها صفرًا إن كان الأصل تالفًا):', '0');
        if (proceeds === null) return;
        btn.disabled = true;
        try {
          await post(`/api/finance/assets/${btn.dataset.dispose}/dispose`, {
            company_id: cid(),
            date: new Date().toISOString().slice(0, 10),
            proceeds: String(proceeds || '0')
          });
          reload();
        } catch (e) {
          btn.disabled = false;
          window.alert(e.message);
        }
      });
    });
    scope.querySelectorAll('[data-branch]').forEach(btn => {
      btn.addEventListener('click', () => {
        window.FinanceApi.setBranch(btn.dataset.branch);
        reload();
      });
    });
    scope.querySelectorAll('[data-run]').forEach(btn => {
      btn.addEventListener('click', async () => {
        btn.disabled = true;
        try {
          await post(`/api/finance/payroll/runs/${btn.dataset.run}/${btn.dataset.do}`, {
            company_id: cid()
          });
          reload();
        } catch (e) {
          btn.disabled = false;
          window.alert(e.message);
        }
      });
    });
  }

  window.FinanceOps2 = {
    tabs: TABS,
    has: tab => Object.prototype.hasOwnProperty.call(views, tab),
    view: tab => views[tab](),
    bind
  };
})();
