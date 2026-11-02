// نماذج الإدخال داخل لوحة المالية: إنشاء صندوق، سعر صرف، مركز تكلفة،
// سطور افتتاحية متوازنة، كشف بنكي وبنوده، وقيد يومية متعدد السطور ثم ترحيله.
(function () {
  'use strict';
  const esc = v => String(v == null ? '' : v)
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  const money = v => Number(v || 0).toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 4 });
  const cid = () => Number(window.FinanceApi.companyId());
  const today = () => new Date().toISOString().slice(0, 10);

  let accountsCache = null;
  async function accounts() {
    if (accountsCache) return accountsCache;
    const res = await window.apiFetch(`/api/finance/ledger?company_id=${cid()}`);
    if (!res.ok) throw new Error('تعذّر تحميل دليل الحسابات.');
    const data = await res.json();
    accountsCache = (data.accounts || []).filter(a => a.kind === 'detail');
    return accountsCache;
  }

  async function postJSON(path, body) {
    const res = await window.apiFetch(path, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body)
    });
    if (!res.ok) {
      let msg = 'تعذّر الحفظ.';
      try { msg = (await res.json()).error || msg; } catch (_) { /* جسم غير JSON */ }
      throw new Error(msg);
    }
    return res.json().catch(() => ({}));
  }

  function options(list, key = 'account_id', label = a => `${a.code} · ${a.name}`) {
    return list.map(a => `<option value="${esc(a[key])}">${esc(label(a))}</option>`).join('');
  }

  const TOOLBARS = {
    funds: () => `<details class="fp-form"><summary>＋ إضافة صندوق أو بنك</summary>
      <form data-form="fund">
        <label>النوع<select name="kind"><option value="cash">صندوق نقدي</option><option value="bank">حساب بنكي</option></select></label>
        <label>الرمز<input name="code" required maxlength="20" placeholder="BANK-1"></label>
        <label>الاسم<input name="name" required maxlength="80" placeholder="البنك التجاري"></label>
        <label>الحساب<select name="account_id" data-accounts required></select></label>
        <label>IBAN (اختياري)<input name="iban" maxlength="34"></label>
        <label>تاريخ الفتح<input type="date" name="opened_on" value="${today()}" required></label>
        <div class="fp-actions"><button type="submit" class="fp-btn">حفظ الصندوق</button></div>
        <p class="fp-form-error" role="alert"></p>
      </form></details>`,
    fx: () => `<details class="fp-form"><summary>＋ إضافة سعر صرف</summary>
      <form data-form="fx">
        <label>من عملة<input name="base" required maxlength="8" placeholder="USD"></label>
        <label>إلى عملة<input name="quote" required maxlength="8" value="YER"></label>
        <label>السعر<input name="rate" required inputmode="decimal" placeholder="530.0000"></label>
        <label>تاريخ السعر<input type="date" name="as_of" value="${today()}" required></label>
        <div class="fp-actions"><button type="submit" class="fp-btn">حفظ السعر</button></div>
        <p class="fp-form-error" role="alert"></p>
      </form></details>`,
    dimensions: () => `<details class="fp-form"><summary>＋ إضافة مركز تكلفة أو مشروع</summary>
      <form data-form="dimension">
        <label>الرمز<input name="code" required maxlength="20" placeholder="PRJ-3"></label>
        <label>الاسم<input name="name" required maxlength="80"></label>
        <label>النوع<select name="kind"><option value="cost_center">مركز تكلفة</option><option value="project">مشروع</option></select></label>
        <div class="fp-actions"><button type="submit" class="fp-btn">حفظ البُعد</button></div>
        <p class="fp-form-error" role="alert"></p>
      </form></details>`,
    opening: () => `<details class="fp-form"><summary>＋ إدخال أرصدة افتتاحية</summary>
      <form data-form="opening">
        <label>العملة<input name="currency" value="YER" maxlength="8" required></label>
        <div class="fp-rows" data-rows="opening"></div>
        <div class="fp-row-actions">
          <button type="button" class="fp-ghost" data-add-row="opening" aria-label="إضافة سطر جديد">＋ سطر</button>
          <span class="fp-total" data-total="opening">مدين 0.00 · دائن 0.00</span>
        </div>
        <div class="fp-actions"><button type="submit" class="fp-btn">حفظ الأرصدة</button></div>
        <p class="fp-form-error" role="alert"></p>
      </form></details>`,
    bankrec: () => `<details class="fp-form"><summary>＋ كشف بنكي جديد</summary>
      <form data-form="bankrec">
        <label>البنك<select name="fund_id" data-funds required></select></label>
        <label>من تاريخ<input type="date" name="period_start" value="${today().slice(0, 7)}-01" required></label>
        <label>إلى تاريخ<input type="date" name="period_end" value="${today()}" required></label>
        <label>رصيد الكشف<input name="closing_balance" required inputmode="decimal" placeholder="0.0000"></label>
        <div class="fp-actions"><button type="submit" class="fp-btn">إنشاء الكشف</button></div>
        <p class="fp-form-error" role="alert"></p>
      </form></details>
      <details class="fp-form"><summary>＋ بند تسوية على كشف</summary>
      <form data-form="bankline">
        <label>الكشف<select name="statement_id" data-statements required></select></label>
        <label>النوع<select name="kind"><option value="deposit">إيداع</option><option value="withdrawal">سحب</option></select></label>
        <label>المبلغ<input name="amount" required inputmode="decimal"></label>
        <label>البيان<input name="memo" required maxlength="120"></label>
        <div class="fp-actions"><button type="submit" class="fp-btn">إضافة البند</button></div>
        <p class="fp-form-error" role="alert"></p>
      </form></details>`,
    journals: () => `<details class="fp-form"><summary>＋ قيد يومية جديد</summary>
      <form data-form="journal">
        <label>البيان<input name="description" required maxlength="120"></label>
        <label>التاريخ<input type="date" name="date" value="${today()}" required></label>
        <label>العملة<input name="currency" value="YER" maxlength="8" required></label>
        <div class="fp-rows" data-rows="journal"></div>
        <div class="fp-row-actions">
          <button type="button" class="fp-ghost" data-add-row="journal" aria-label="إضافة سطر جديد">＋ سطر</button>
          <span class="fp-total" data-total="journal">مدين 0.00 · دائن 0.00</span>
        </div>
        <div class="fp-actions"><button type="submit" class="fp-btn">حفظ كمسودة</button></div>
        <p class="fp-form-error" role="alert"></p>
      </form></details>`
  };

  const toolbar = tab => (TOOLBARS[tab] ? TOOLBARS[tab]() : '');

  function rowHTML(kind, accountList) {
    const select = `<td><select data-account required>${options(accountList)}</select></td>`;
    if (kind === 'opening') {
      return `<tr data-row>${select}<td><input data-debit inputmode="decimal" placeholder="0.0000"></td>
        <td><input data-credit inputmode="decimal" placeholder="0.0000"></td>
        <td><button type="button" class="fp-ghost" data-del-row aria-label="حذف السطر" title="حذف السطر">✕</button></td></tr>`;
    }
    return `<tr data-row>${select}<td><input data-debit inputmode="decimal" placeholder="0.0000"></td>
      <td><input data-credit inputmode="decimal" placeholder="0.0000"></td>
      <td><button type="button" class="fp-ghost" data-del-row aria-label="حذف السطر" title="حذف السطر">✕</button></td></tr>`;
  }

  let banks = [];
  let statements = [];

  async function hydrate(form, tab) {
    const accountList = await accounts();
    form.querySelectorAll('[data-accounts]').forEach(node => {
      node.innerHTML = options(accountList);
    });
    form.querySelectorAll('[data-rows]').forEach(node => {
      node.innerHTML = `<table class="fp-rows-table"><thead><tr><th>الحساب</th><th>مدين</th><th>دائن</th><th></th></tr></thead>
        <tbody>${rowHTML(node.dataset.rows, accountList)}${rowHTML(node.dataset.rows, accountList)}</tbody></table>`;
    });
    if (tab === 'bankrec') {
      const res = await window.apiFetch(`/api/finance/funds?company_id=${cid()}`);
      if (res.ok) {
        banks = (await res.json()).filter(f => f.kind === 'bank');
        form.querySelectorAll('[data-funds]').forEach(node => {
          node.innerHTML = banks.length
            ? banks.map(f => `<option value="${esc(f.id)}">${esc(f.code)} · ${esc(f.name)}</option>`).join('')
            : '<option value="">لا يوجد حساب بنكي بعد</option>';
        });
      }
      const recs = await window.apiFetch(`/api/finance/bankrec?company_id=${cid()}`);
      if (recs.ok) {
        statements = await recs.json();
        form.querySelectorAll('[data-statements]').forEach(node => {
          node.innerHTML = statements.length
            ? statements.map(s => `<option value="${esc(s.id)}">#${esc(s.id)} · ${esc(s.fund_code || s.fund || '')}</option>`).join('')
            : '<option value="">لا يوجد كشف بعد</option>';
        });
      }
    }
  }

  function recalc(form, kind) {
    const total = form.querySelector(`[data-total="${kind}"]`);
    if (!total) return;
    let debit = 0;
    let credit = 0;
    form.querySelectorAll('[data-row]').forEach(row => {
      debit += Number(row.querySelector('[data-debit]').value || 0);
      credit += Number(row.querySelector('[data-credit]').value || 0);
    });
    const balanced = Math.abs(debit - credit) < 0.00005;
    total.textContent = `مدين ${money(debit)} · دائن ${money(credit)} · ${balanced ? 'متوازن ✔' : 'غير متوازن ✘'}`;
    // تلوين الحاوية نفسها: أخضر عند التوازن، أحمر عند الاختلال.
    total.classList.toggle('is-balanced', balanced);
    total.classList.toggle('is-unbalanced', !balanced);
  }

  function collectRows(form, kind) {
    const rows = [];
    form.querySelectorAll('[data-row]').forEach(row => {
      const accountId = Number(row.querySelector('[data-account]').value);
      const debit = (row.querySelector('[data-debit]').value || '0').trim();
      const credit = (row.querySelector('[data-credit]').value || '0').trim();
      if (!accountId) return;
      if (Number(debit) === 0 && Number(credit) === 0) return;
      rows.push({ account_id: accountId, account: '', debit, credit });
    });
    if (kind === 'opening') {
      return rows.map(r => ({ account_id: r.account_id, debit: r.debit, credit: r.credit }));
    }
    return rows;
  }

  const SUBMIT = {
    async fund(form) {
      const data = new FormData(form);
      return postJSON('/api/finance/funds', {
        company_id: cid(), kind: data.get('kind'), code: data.get('code'), name: data.get('name'),
        account_id: Number(data.get('account_id')), iban: data.get('iban') || null,
        opened_on: data.get('opened_on')
      });
    },
    async fx(form) {
      const data = new FormData(form);
      return postJSON('/api/finance/fx', {
        company_id: cid(), base: String(data.get('base')).toUpperCase(),
        quote: String(data.get('quote')).toUpperCase(), rate: data.get('rate'), as_of: data.get('as_of')
      });
    },
    async dimension(form) {
      const data = new FormData(form);
      return postJSON('/api/finance/dimensions', {
        company_id: cid(), code: data.get('code'), name: data.get('name'), kind: data.get('kind')
      });
    },
    async opening(form) {
      const data = new FormData(form);
      const lines = collectRows(form, 'opening');
      if (lines.length < 2) throw new Error('أدخل سطرين على الأقل.');
      return postJSON('/api/finance/opening', {
        company_id: cid(), currency: data.get('currency'), lines
      });
    },
    async bankrec(form) {
      const data = new FormData(form);
      if (!data.get('fund_id')) throw new Error('لا يوجد حساب بنكي؛ أضف صندوقًا نوعه «حساب بنكي» أولًا.');
      return postJSON('/api/finance/bankrec', {
        company_id: cid(), fund_id: Number(data.get('fund_id')),
        period_start: data.get('period_start'), period_end: data.get('period_end'),
        closing_balance: data.get('closing_balance')
      });
    },
    async bankline(form) {
      const data = new FormData(form);
      if (!data.get('statement_id')) throw new Error('لا يوجد كشف بنكي بعد.');
      return postJSON(`/api/finance/bankrec/${data.get('statement_id')}/lines`, {
        company_id: cid(), kind: data.get('kind'), amount: data.get('amount'), memo: data.get('memo')
      });
    },
    async journal(form) {
      const data = new FormData(form);
      const lines = collectRows(form, 'journal');
      if (lines.length < 2) throw new Error('القيد يحتاج سطرين على الأقل.');
      return postJSON('/api/finance/journals', {
        company_id: cid(), description: data.get('description'), date: data.get('date'),
        currency: data.get('currency'), lines
      });
    }
  };

  function bind(tab, scope, reload) {
    if (!TOOLBARS[tab]) return;
    scope.querySelectorAll('form[data-form]').forEach(form => {
      const kind = form.dataset.form;
      hydrate(form, tab).catch(() => { /* تُظهر الأخطاء عند الإرسال */ });
      form.addEventListener('input', event => {
        if (event.target.closest('[data-debit],[data-credit]')) recalc(form, kind);
      });
      form.addEventListener('click', event => {
        if (event.target.matches('[data-add-row]')) {
          const wrap = form.querySelector(`[data-rows="${kind}"] tbody`);
          if (wrap && accountsCache) wrap.insertAdjacentHTML('beforeend', rowHTML(kind, accountsCache));
        }
        if (event.target.matches('[data-del-row]')) {
          const row = event.target.closest('[data-row]');
          if (row && row.parentElement.children.length > 2) row.remove();
          recalc(form, kind);
        }
      });
      form.addEventListener('submit', async event => {
        event.preventDefault();
        const error = form.querySelector('.fp-form-error');
        error.textContent = '';
        const button = form.querySelector('button[type=submit]');
        button.disabled = true;
        try {
          await SUBMIT[kind](form);
          accountsCache = null;
          reload();
        } catch (e) {
          error.textContent = e.message || 'تعذّر الحفظ.';
        } finally {
          button.disabled = false;
        }
      });
    });
  }

  window.FinanceForms = { toolbar, bind };
})();
