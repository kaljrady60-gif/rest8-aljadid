// 17-finance-extras.js — شاشات المالية المكتملة:
// العملات · مجموعات الفروع · الإقرارات الضريبية · الإشعارات المدينة والدائنة ·
// دورات الزكاة · الموازنات وتعديلها وموازنات الإدارات · الإقفال الموحّد · صرف العملة.
(function () {
  'use strict';
  const esc = v => String(v == null ? '' : v).replace(/^__c\d+_/, '')
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  const money = v => Number(v || 0).toLocaleString('en-US', {
    minimumFractionDigits: 2, maximumFractionDigits: 4
  });
  const cid = () => Number(window.FinanceApi.companyId());
  const q = extra => `?company_id=${cid()}${extra || ''}`;
  const today = () => new Date().toISOString().slice(0, 10);
  const year = () => new Date().getFullYear();

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
    method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body)
  });
  function table(headers, rows, empty) {
    if (!rows.length) return `<p class="fp-empty">${esc(empty || 'لا توجد بيانات بعد.')}</p>`;
    return `<div class="fp-table-wrap"><table class="fp-table"><thead><tr>${
      headers.map(h => `<th>${esc(h)}</th>`).join('')
    }</tr></thead><tbody>${
      rows.map(r => `<tr>${r.map(c => `<td>${c}</td>`).join('')}</tr>`).join('')
    }</tbody></table></div>`;
  }
  const note = (scope, id, text, bad) => {
    const node = scope.querySelector(id);
    if (!node) return;
    node.textContent = text;
    node.classList.toggle('fp-error', Boolean(bad));
  };
  async function accountOptions() {
    const list = await api(`/api/finance/accounts${q()}`).catch(() => []);
    const arr = Array.isArray(list) ? list : (list.accounts || []);
    return arr.map(a => `<option value="${esc(a.id)}">${esc(a.code)} — ${esc(a.name)}</option>`).join('');
  }
  const val = (form, name) => (form.querySelector(`[name="${name}"]`) || {}).value || '';

  const views = {
    // ── العملات ────────────────────────────────────────────────────────────
    async currencies() {
      const rows = await api(`/api/finance/currencies${q()}`);
      const body = rows.map(c => [
        `<code dir="ltr">${esc(c.code)}</code>`, esc(c.name), esc(c.symbol || '—'),
        String(c.decimals), c.archived_at ? '<span class="fp-badge">مؤرشفة</span>' : '<span class="fp-ok">نشطة</span>'
      ]);
      return `<form class="fp-form" id="fp-cur-form">
          <h3>عملة جديدة</h3>
          <div class="fp-grid">
            <label>الرمز (ISO) <input name="code" maxlength="3" required placeholder="USD"></label>
            <label>الاسم <input name="name" required maxlength="80" placeholder="دولار أمريكي"></label>
            <label>الرمز المكتوب <input name="symbol" maxlength="8" placeholder="$"></label>
            <label>منازل التقريب <input name="decimals" type="number" min="0" max="6" value="2"></label>
          </div>
          <div class="fp-actions"><button type="submit" class="fp-btn">حفظ العملة</button></div>
          <p class="fp-note" id="fp-cur-note"></p>
        </form>
        ${table(['الرمز', 'الاسم', 'الرمز المكتوب', 'المنازل', 'الحالة'], body, 'لا عملات مسجّلة بعد.')}`;
    },

    // ── مجموعات الفروع ─────────────────────────────────────────────────────
    async branchgroups() {
      const [groups, branches] = await Promise.all([
        api(`/api/finance/branch-groups${q()}`),
        api(`/api/finance/branches${q()}`).catch(() => [])
      ]);
      const body = groups.map(g => [
        `<code dir="ltr">${esc(g.code)}</code>`, esc(g.name),
        g.members.length ? g.members.map(m => esc(m.name)).join(' · ') : '<span class="fp-badge">بلا فروع</span>',
        esc(g.note || '—'),
        g.archived_at ? 'مؤرشفة' : 'نشطة'
      ]);
      return `<form class="fp-form" id="fp-group-form">
          <h3>مجموعة فروع جديدة</h3>
          <div class="fp-grid">
            <label>الرمز <input name="code" maxlength="20" required placeholder="GRP-1"></label>
            <label>الاسم <input name="name" maxlength="80" required placeholder="فروع الشمال"></label>
            <label>ملاحظة <input name="note" maxlength="120"></label>
          </div>
          <fieldset class="fp-checks"><legend>الفروع المشمولة</legend>
            ${(branches || []).map(b => `<label class="fp-check"><input type="checkbox" name="branch" value="${esc(b.id)}"> ${esc(b.code)} · ${esc(b.name)}</label>`).join('') || '<p class="fp-note">لا فروع بعد.</p>'}
          </fieldset>
          <div class="fp-actions"><button type="submit" class="fp-btn">حفظ المجموعة</button></div>
          <p class="fp-note" id="fp-group-note"></p>
        </form>
        ${table(['الرمز', 'الاسم', 'الفروع', 'ملاحظة', 'الحالة'], body, 'لا مجموعات بعد.')}`;
    },

    // ── الإقرارات الضريبية ────────────────────────────────────────────────
    async taxreturns() {
      const rows = await api(`/api/finance/tax-returns${q()}`);
      const body = rows.map(r => [
        esc(r.kind), `<span dir="ltr">${esc(r.period_from)} → ${esc(r.period_to)}</span>`,
        `<b dir="ltr">${money(r.output_tax)}</b>`, `<b dir="ltr">${money(r.input_tax)}</b>`,
        `<b dir="ltr">${money(r.net_tax)}</b>`,
        r.status === 'submitted' ? '<span class="fp-ok">مقدَّم</span>' : '<span class="fp-badge">مسودة</span>',
        r.status === 'draft'
          ? `<button type="button" class="fp-ghost" data-return-submit="${esc(r.id)}">تقديم</button>` : '—'
      ]);
      return `<form class="fp-form" id="fp-return-form">
          <h3>إقرار ضريبي لفترة</h3>
          <div class="fp-grid">
            <label>النوع <select name="kind">
              <option value="vat">قيمة مضافة</option><option value="sales">مبيعات</option>
              <option value="purchase">مشتريات</option><option value="withholding">استقطاع</option>
            </select></label>
            <label>من <input type="date" name="period_from" value="${year()}-01-01" required></label>
            <label>إلى <input type="date" name="period_to" value="${year()}-12-31" required></label>
            <label>ملاحظة <input name="note" maxlength="120"></label>
          </div>
          <div class="fp-actions">
            <button type="button" class="fp-ghost" id="fp-return-preview">معاينة من الروابط</button>
            <button type="submit" class="fp-btn">حفظ الإقرار</button>
          </div>
          <p class="fp-note" id="fp-return-note"></p>
        </form>
        ${table(['النوع', 'الفترة', 'ضريبة المخرجات', 'ضريبة المدخلات', 'الصافي', 'الحالة', 'إجراء'], body, 'لا إقرارات بعد.')}`;
    },

    // ── الإشعارات المدينة والدائنة ────────────────────────────────────────
    async notes() {
      const [rows, accounts] = await Promise.all([
        api(`/api/finance/notes${q()}`),
        accountOptions()
      ]);
      const body = rows.map(n => [
        n.kind === 'debit' ? '<span class="fp-badge">مدين</span>' : '<span class="fp-badge">دائن</span>',
        `<code>${esc(n.doc_ref)}</code>`, esc(n.party || '—'),
        `<span dir="ltr">${esc(n.date)}</span>`,
        `<b dir="ltr">${money(n.amount)}</b> <small>${esc(n.currency)}</small>`,
        esc(n.reason || '—'),
        n.status === 'posted' ? `<span class="fp-ok">مرحّل</span> <small>#${esc(n.entry_id)}</small>`
          : `<span class="fp-badge">مسودة</span>
             <button type="button" class="fp-ghost" data-note-post="${esc(n.id)}">ترحيل</button>`
      ]);
      return `<form class="fp-form" id="fp-note-form">
          <h3>إشعار جديد</h3>
          <div class="fp-grid">
            <label>النوع <select name="kind"><option value="debit">مدين (زيادة على الطرف)</option>
              <option value="credit">دائن (تخفيض للطرف)</option></select></label>
            <label>مرجع المستند <input name="doc_ref" maxlength="60" required placeholder="CN-2026-01"></label>
            <label>التاريخ <input type="date" name="note_date" value="${today()}" required></label>
            <label>المبلغ <input name="amount" inputmode="decimal" required placeholder="0.0000"></label>
            <label>العملة <input name="currency" value="YER" maxlength="8"></label>
            <label>السبب <input name="reason" maxlength="120"></label>
          </div>
          <div class="fp-actions"><button type="submit" class="fp-btn">حفظ الإشعار</button></div>
          <p class="fp-note" id="fp-note-note"></p>
        </form>
        <form class="fp-form" id="fp-note-post-form">
          <h3>ترحيل إشعار إلى قيد</h3>
          <div class="fp-grid">
            <label>الإشعار <select name="id" data-notes-options required></select></label>
            <label>الحساب المقابل <select name="counter_account_id" required>${accounts}</select></label>
          </div>
          <div class="fp-actions"><button type="submit" class="fp-btn">ترحيل الآن</button></div>
          <p class="fp-note" id="fp-note-post-note"></p>
        </form>
        ${table(['النوع', 'المرجع', 'الطرف', 'التاريخ', 'المبلغ', 'السبب', 'الحالة'], body, 'لا إشعارات بعد.')}`;
    },

    // ── الزكاة ─────────────────────────────────────────────────────────────
    async zakat() {
      const [rows, accounts] = await Promise.all([
        api(`/api/finance/zakat${q()}`),
        accountOptions()
      ]);
      const body = rows.map(z => [
        String(z.fiscal_year), `<b dir="ltr">${money(z.base)}</b>`,
        `${(Number(z.rate_bp) / 100).toFixed(2)}%`, `<b dir="ltr">${money(z.due)}</b>`,
        z.status === 'posted' ? `<span class="fp-ok">مرحّلة</span> <small>#${esc(z.entry_id)}</small>`
          : '<span class="fp-badge">محسوبة</span>'
      ]);
      return `<form class="fp-form" id="fp-zakat-form">
          <h3>دورة زكاة</h3>
          <div class="fp-grid">
            <label>السنة <input type="number" name="fiscal_year" value="${year() - 1}" min="1900" max="2999" required></label>
            <label>النسبة (٪) <input type="number" name="rate" step="0.01" value="2.5" min="0" max="100"></label>
            <label>حساب مصروف الزكاة <select name="expense_account_id"><option value="">— لا ترحيل —</option>${accounts}</select></label>
            <label>حساب الزكاة المستحقة <select name="payable_account_id"><option value="">— لا ترحيل —</option>${accounts}</select></label>
          </div>
          <div class="fp-actions">
            <button type="button" class="fp-ghost" id="fp-zakat-preview">احتساب فقط</button>
            <button type="submit" class="fp-btn">حفظ الدورة</button>
          </div>
          <p class="fp-note" id="fp-zakat-note">الوعاء تقديري: أصول جارية − التزامات جارية من القيود المرحّلة.</p>
        </form>
        ${table(['السنة', 'الوعاء', 'النسبة', 'المستحق', 'الحالة'], body, 'لا دورات زكاة بعد.')}`;
    },

    // ── الموازنات وتعديلها وموازنات الإدارات ─────────────────────────────
    async budgets() {
      const rows = await api(`/api/finance/budgets${q()}`);
      const body = rows.map(b => [
        `<code dir="ltr">${esc(b.id)}</code>`, esc(b.name), String(b.fiscal_year),
        b.scope === 'department' ? `إدارة: ${esc(b.department || '—')}` : 'الشركة كلها',
        String(b.lines), `<b dir="ltr">${money(b.total)}</b>`,
        b.status === 'approved' ? '<span class="fp-ok">معتمدة</span>'
          : b.status === 'closed' ? '<span class="fp-badge">مغلقة</span>' : '<span class="fp-badge">مفتوحة</span>',
        `<button type="button" class="fp-ghost" data-budget-open="${esc(b.id)}">مقارنة</button>`
      ]);
      return `<form class="fp-form" id="fp-budget-form">
          <h3>موازنة جديدة</h3>
          <div class="fp-grid">
            <label>الاسم <input name="name" maxlength="80" required placeholder="موازنة 2026"></label>
            <label>السنة <input type="number" name="fiscal_year" value="${year()}" min="1900" max="2999" required></label>
            <label>النطاق <select name="scope">
              <option value="company">الشركة كلها</option><option value="department">إدارة محددة</option>
            </select></label>
            <label>القسم (لموازنة إدارة) <input name="note" maxlength="120" placeholder="ملاحظة أو اسم الإدارة"></label>
          </div>
          <h4>بنود الموازنة</h4>
          <div class="fp-rows" data-budget-rows></div>
          <div class="fp-row-actions">
            <button type="button" class="fp-ghost" id="fp-budget-add">＋ بند</button>
            <span class="fp-total" data-budget-total>المجموع 0.00</span>
          </div>
          <div class="fp-actions"><button type="submit" class="fp-btn">حفظ الموازنة</button></div>
          <p class="fp-note" id="fp-budget-note"></p>
        </form>
        <div id="fp-budget-compare"></div>
        ${table(['#', 'الاسم', 'السنة', 'النطاق', 'البنود', 'الإجمالي', 'الحالة', 'إجراء'], body, 'لا موازنات بعد.')}`;
    },

    async budgetedit() {
      const rows = await api(`/api/finance/budgets${q()}`);
      const open = rows.filter(b => b.status !== 'closed');
      const options = open.map(b => `<option value="${esc(b.id)}">${esc(b.name)} · ${esc(b.fiscal_year)}</option>`).join('');
      if (!options) {
        return '<p class="fp-empty">لا موازنة مفتوحة للتعديل. أنشئ موازنة أولًا من شاشة الموازنة.</p>';
      }
      const accounts = await accountOptions();
      return `<form class="fp-form" id="fp-budget-adjust">
          <h3>تعديل بند موازنة</h3>
          <div class="fp-grid">
            <label>الموازنة <select name="budget_id" required>${options}</select></label>
            <label>الحساب <select name="account_id" required>${accounts}</select></label>
            <label>الشهر <input type="number" name="month" min="1" max="12" value="1" required></label>
            <label>المبلغ الجديد <input name="amount" inputmode="decimal" required></label>
            <label>سبب التعديل <input name="reason" maxlength="120"></label>
          </div>
          <div class="fp-actions"><button type="submit" class="fp-btn">تنفيذ التعديل</button></div>
          <p class="fp-note" id="fp-adjust-note"></p>
        </form>
        <div id="fp-budget-history"></div>`;
    },

    async deptbudgets() {
      const rows = await api(`/api/finance/budgets${q()}`);
      const dept = rows.filter(b => b.scope === 'department');
      const body = dept.map(b => [
        esc(b.department || '—'), esc(b.name), String(b.fiscal_year),
        `<b dir="ltr">${money(b.total)}</b>`, String(b.lines),
        `<button type="button" class="fp-ghost" data-budget-open="${esc(b.id)}">مقارنة</button>`
      ]);
      const company = rows.filter(b => b.scope === 'company')
        .reduce((sum, b) => sum + Number(b.total || 0), 0);
      return `<p class="fp-note">موازنات الإدارات مستقلة عن موازنة الشركة (${money(company)}) وتُقارن كل واحدة بفعلها.</p>
        <div id="fp-budget-compare"></div>
        ${table(['الإدارة', 'الاسم', 'السنة', 'الإجمالي', 'البنود', 'إجراء'], body,
                'لا موازنات إدارات بعد. أنشئ موازنة بنطاق «إدارة محددة».')}`;
    },

    async closehub() {
      const data = await api(`/api/finance/close-hub${q()}`);
      const blockers = (data.blockers || []);
      return `<div class="fp-kpis" style="grid-template-columns:repeat(auto-fit,minmax(200px,1fr))">
          <div class="fp-kpi"><span>فترات مفتوحة</span><strong>${data.open_periods}</strong><small>تُمنع في الإقفال</small></div>
          <div class="fp-kpi"><span>قيود غير مرحّلة</span><strong>${data.unposted_entries}</strong><small>مسودات معلّقة</small></div>
          <div class="fp-kpi"><span>إشعارات مسودة</span><strong>${data.draft_notes}</strong><small>لم تُرحّل</small></div>
          <div class="fp-kpi"><span>إقرارات معلّقة</span><strong>${data.pending_tax_returns}</strong><small>لم تُقدَّم</small></div>
          <div class="fp-kpi"><span>سنوات مقفلة</span><strong>${data.closed_years}</strong><small>إقفال نهائي</small></div>
        </div>
        ${blockers.length
          ? `<h3>ما يمنع الإقفال الآن</h3>${table(['العدد', 'السبب'],
              blockers.map(b => [`<b>${esc(b.count)}</b>`, esc(b.reason)]))}`
          : '<p class="fp-ok">جاهز للإقفال: لا عوائق مفتوحة.</p>'}
        <h3>الإقفال التنفيذي</h3>
        <form class="fp-form" id="fp-close-form">
          <div class="fp-grid">
            <label>السنة المالية <input type="number" name="fiscal_year" value="${year() - 1}" required></label>
            <label>العملة <input name="currency" value="YER" maxlength="8"></label>
            <label>ملاحظة <input name="notes" maxlength="120"></label>
          </div>
          <div class="fp-actions"><button type="submit" class="fp-btn">تنفيذ الإقفال الموحّد</button></div>
          <p class="fp-note" id="fp-close-note">${esc(data.ready ? 'الشروط مستوفاة.' : 'عالج العوائق أعلاه أولًا — الإقفال لا يتراجع.')}</p>
        </form>`;
    },

    async fxconvert() {
      const [rates, funds] = await Promise.all([
        api(`/api/finance/fx${q()}`),
        api(`/api/finance/funds${q()}`).catch(() => [])
      ]);
      const body = (rates || []).map(r => [
        `<code dir="ltr">${esc(r.base)}</code>`, `<code dir="ltr">${esc(r.quote)}</code>`,
        `<b dir="ltr">${money(r.rate)}</b>`, `<span dir="ltr">${esc(r.as_of)}</span>`
      ]);
      return `<form class="fp-form" id="fp-convert-form">
          <h3>صرف عملة</h3>
          <div class="fp-grid">
            <label>من عملة <input name="from" maxlength="8" value="USD" required></label>
            <label>إلى عملة <input name="to" maxlength="8" value="YER" required></label>
            <label>المبلغ <input name="amount" inputmode="decimal" required placeholder="100"></label>
            <label>الصندوق <select name="fund_id"><option value="">— بلا صندوق —</option>${
              (funds || []).map(f => `<option value="${esc(f.id)}">${esc(f.code)} · ${esc(f.name)}</option>`).join('')}</select></label>
          </div>
          <div class="fp-actions">
            <button type="button" class="fp-ghost" id="fp-convert-run">احتساب</button>
          </div>
          <p class="fp-note" id="fp-convert-note">الاحتساب يقرأ أحدث سعر مسجّل للزوج من شاشة العملات وأسعار الصرف.</p>
        </form>
        ${table(['من', 'إلى', 'السعر', 'تاريخ السعر'], body, 'لا أسعار مسجّلة بعد.')}`;
    }
  };

  // ── ربط النماذج ─────────────────────────────────────────────────────────
  function bind(tab, scope, refresh) {
    const accountsReady = accountOptions;

    if (tab === 'currencies') {
      const form = scope.querySelector('#fp-cur-form');
      if (form) form.addEventListener('submit', async event => {
        event.preventDefault();
        try {
          await post('/api/finance/currencies', {
            company_id: cid(), code: val(form, 'code'), name: val(form, 'name'),
            symbol: val(form, 'symbol') || null, decimals: Number(val(form, 'decimals') || 2)
          });
          note(scope, '#fp-cur-note', 'حُفظت العملة.');
          if (refresh) await refresh();
        } catch (e) { note(scope, '#fp-cur-note', e.message, true); }
      });
    }

    if (tab === 'branchgroups') {
      const form = scope.querySelector('#fp-group-form');
      if (form) form.addEventListener('submit', async event => {
        event.preventDefault();
        const branches = Array.from(form.querySelectorAll('[name="branch"]:checked')).map(n => Number(n.value));
        try {
          await post('/api/finance/branch-groups', {
            company_id: cid(), code: val(form, 'code'), name: val(form, 'name'),
            note: val(form, 'note') || null, branch_ids: branches
          });
          note(scope, '#fp-group-note', 'حُفظت المجموعة.');
          if (refresh) await refresh();
        } catch (e) { note(scope, '#fp-group-note', e.message, true); }
      });
    }

    if (tab === 'taxreturns') {
      const form = scope.querySelector('#fp-return-form');
      const preview = scope.querySelector('#fp-return-preview');
      if (preview) preview.addEventListener('click', async () => {
        try {
          const data = await api(`/api/finance/tax-returns/preview${q(
            `&from=${val(form, 'period_from')}&to=${val(form, 'period_to')}`)}`);
          note(scope, '#fp-return-note',
            `المخرجات ${data.output_tax} · المدخلات ${data.input_tax} · الصافي ${data.net_tax}`);
        } catch (e) { note(scope, '#fp-return-note', e.message, true); }
      });
      if (form) form.addEventListener('submit', async event => {
        event.preventDefault();
        try {
          await post('/api/finance/tax-returns', {
            company_id: cid(), kind: val(form, 'kind'),
            period_from: val(form, 'period_from'), period_to: val(form, 'period_to'),
            note: val(form, 'note') || null
          });
          note(scope, '#fp-return-note', 'حُفظ الإقرار كمسودة.');
          if (refresh) await refresh();
        } catch (e) { note(scope, '#fp-return-note', e.message, true); }
      });
      scope.querySelectorAll('[data-return-submit]').forEach(btn => {
        btn.addEventListener('click', async () => {
          btn.disabled = true;
          try {
            await post('/api/finance/tax-returns/submit', { company_id: cid(), id: Number(btn.dataset.returnSubmit) });
            if (refresh) await refresh();
          } catch (e) { window.alert(e.message); btn.disabled = false; }
        });
      });
    }

    if (tab === 'notes') {
      const form = scope.querySelector('#fp-note-form');
      if (form) form.addEventListener('submit', async event => {
        event.preventDefault();
        try {
          await post('/api/finance/notes', {
            company_id: cid(), kind: val(form, 'kind'), doc_ref: val(form, 'doc_ref'),
            note_date: val(form, 'note_date'), amount: val(form, 'amount'),
            currency: val(form, 'currency') || 'YER', reason: val(form, 'reason') || null
          });
          note(scope, '#fp-note-note', 'حُفظ الإشعار.');
          if (refresh) await refresh();
        } catch (e) { note(scope, '#fp-note-note', e.message, true); }
      });
      const poster = scope.querySelector('#fp-note-post-form');
      if (poster) poster.addEventListener('submit', async event => {
        event.preventDefault();
        try {
          const result = await post('/api/finance/notes/post', {
            company_id: cid(), id: Number(val(poster, 'id')),
            counter_account_id: Number(val(poster, 'counter_account_id'))
          });
          note(scope, '#fp-note-post-note', `رُحّل بقيد #${result.entry_id}`);
          if (refresh) await refresh();
        } catch (e) { note(scope, '#fp-note-post-note', e.message, true); }
      });
    }

    if (tab === 'zakat') {
      const form = scope.querySelector('#fp-zakat-form');
      const preview = scope.querySelector('#fp-zakat-preview');
      if (preview) preview.addEventListener('click', async () => {
        try {
          const y = Number(val(form, 'fiscal_year'));
          const rate = Math.round(Number(val(form, 'rate') || 2.5) * 100);
          const data = await api(`/api/finance/zakat/compute${q(`&fiscal_year=${y}&rate_bp=${rate}`)}`);
          note(scope, '#fp-zakat-note', `الوعاء ${data.base} · المستحق ${data.due}`);
        } catch (e) { note(scope, '#fp-zakat-note', e.message, true); }
      });
      if (form) form.addEventListener('submit', async event => {
        event.preventDefault();
        try {
          const rate = Math.round(Number(val(form, 'rate') || 2.5) * 100);
          const data = await post('/api/finance/zakat/run', {
            company_id: cid(), fiscal_year: Number(val(form, 'fiscal_year')), rate_bp: rate,
            expense_account_id: val(form, 'expense_account_id') ? Number(val(form, 'expense_account_id')) : null,
            payable_account_id: val(form, 'payable_account_id') ? Number(val(form, 'payable_account_id')) : null
          });
          note(scope, '#fp-zakat-note',
            data.entry_id ? `احتُسبت ورُحّلت بقيد #${data.entry_id}` : `احتُسبت: ${data.due}`);
          if (refresh) await refresh();
        } catch (e) { note(scope, '#fp-zakat-note', e.message, true); }
      });
    }

    if (tab === 'budgets' || tab === 'deptbudgets') {
      const host = scope.querySelector('[data-budget-rows]');
      if (host) {
        const addRow = async () => {
          const accounts = await accountsReady();
          const row = document.createElement('div');
          row.className = 'fp-budget-row';
          row.innerHTML = `<select data-account required>${accounts}</select>
            <input type="number" data-month min="1" max="12" value="1" aria-label="الشهر">
            <input data-amount inputmode="decimal" placeholder="0.0000" aria-label="المبلغ">
            <button type="button" class="fp-ghost" data-remove aria-label="حذف البند">✕</button>`;
          host.appendChild(row);
          const total = scope.querySelector('[data-budget-total]');
          const recalc = () => {
            const sum = Array.from(host.querySelectorAll('[data-amount]'))
              .reduce((s, n) => s + Number(n.value || 0), 0);
            if (total) total.textContent = `المجموع ${money(sum)}`;
          };
          row.querySelector('[data-amount]').addEventListener('input', recalc);
          row.querySelector('[data-remove]').addEventListener('click', () => { row.remove(); recalc(); });
        };
        addRow(); addRow();
        const add = scope.querySelector('#fp-budget-add');
        if (add) add.addEventListener('click', addRow);
      }
      const form = scope.querySelector('#fp-budget-form');
      if (form) form.addEventListener('submit', async event => {
        event.preventDefault();
        const lines = Array.from(form.querySelectorAll('.fp-budget-row')).map(row => ({
          account_id: Number(row.querySelector('[data-account]').value),
          month: Number(row.querySelector('[data-month]').value || 1),
          amount: row.querySelector('[data-amount]').value || '0'
        })).filter(l => l.account_id);
        try {
          await post('/api/finance/budgets', {
            company_id: cid(), fiscal_year: Number(val(form, 'fiscal_year')),
            name: val(form, 'name'), scope: val(form, 'scope'),
            department_id: null, note: val(form, 'note') || null, lines
          });
          note(scope, '#fp-budget-note', 'حُفظت الموازنة.');
          if (refresh) await refresh();
        } catch (e) { note(scope, '#fp-budget-note', e.message, true); }
      });
    }

    if (tab === 'budgetedit') {
      const form = scope.querySelector('#fp-budget-adjust');
      if (form) form.addEventListener('submit', async event => {
        event.preventDefault();
        try {
          const result = await post('/api/finance/budgets/adjust', {
            company_id: cid(), budget_id: Number(val(form, 'budget_id')),
            account_id: Number(val(form, 'account_id')), month: Number(val(form, 'month')),
            amount: val(form, 'amount'), reason: val(form, 'reason') || null
          });
          note(scope, '#fp-adjust-note', `عُدّل البند: ${result.old} → ${result.new}`);
          const history = await api(`/api/finance/budgets/history${q(`&budget_id=${result.budget_id}`)}`);
          renderHistory(scope, history);
        } catch (e) { note(scope, '#fp-adjust-note', e.message, true); }
      });
    }

    if (tab === 'closehub') {
      const form = scope.querySelector('#fp-close-form');
      if (form) form.addEventListener('submit', async event => {
        event.preventDefault();
        try {
          await post('/api/finance/closing', {
            company_id: cid(), fiscal_year: Number(val(form, 'fiscal_year')),
            currency: val(form, 'currency') || 'YER', notes: val(form, 'notes') || null
          });
          note(scope, '#fp-close-note', 'نُفّذ الإقفال.');
          if (refresh) await refresh();
        } catch (e) { note(scope, '#fp-close-note', e.message, true); }
      });
    }

    if (tab === 'fxconvert') {
      const run = scope.querySelector('#fp-convert-run');
      const form = scope.querySelector('#fp-convert-form');
      if (run) run.addEventListener('click', async () => {
        try {
          const data = await api(`/api/finance/fx/convert${q(
            `&base=${val(form, 'from')}&quote=${val(form, 'to')}&amount=${val(form, 'amount')}`)}`);
          note(scope, '#fp-convert-note',
            `النتيجة ${money(data.amount ?? data.result ?? 0)} ${val(form, 'to')} بسعر ${data.rate ?? '—'}`);
        } catch (e) { note(scope, '#fp-convert-note', e.message, true); }
      });
    }

    // مقارنة الموازنة (مشتركة بين الموازنة وموازنات الإدارات)
    scope.querySelectorAll('[data-budget-open]').forEach(btn => {
      btn.addEventListener('click', async () => {
        const host = scope.querySelector('#fp-budget-compare');
        if (!host) return;
        host.innerHTML = '<p class="fp-empty">جارٍ الحساب…</p>';
        try {
          const data = await api(`/api/finance/budgets/compare${q(`&budget_id=${btn.dataset.budgetOpen}`)}`);
          host.innerHTML = `<p class="fp-note">الموازنة ${money(data.planned_total)} · الفعلي ${money(data.actual_total)} · الفرق ${money(data.variance_total)}</p>`
            + table(['الرمز', 'الحساب', 'الشهر', 'الموازنة', 'الفعلي', 'الفرق'],
              (data.lines || []).map(l => [
                esc(l.code), esc(l.name), String(l.month),
                `<b dir="ltr">${money(l.planned)}</b>`, `<b dir="ltr">${money(l.actual)}</b>`,
                `<b dir="ltr">${money(l.variance)}</b>`
              ]), 'لا بنود.');
        } catch (e) { host.innerHTML = `<p class="fp-error">${esc(e.message)}</p>`; }
      });
    });
  }

  function renderHistory(scope, rows) {
    const host = scope.querySelector('#fp-budget-history');
    if (!host) return;
    host.innerHTML = `<h3>سجل التعديلات</h3>` + table(['الرمز', 'الحساب', 'الشهر', 'قبل', 'بعد', 'السبب', 'الوقت'],
      (rows || []).map(r => [esc(r.code), esc(r.name), String(r.month),
        `<span dir="ltr">${money(r.old)}</span>`, `<span dir="ltr">${money(r.new)}</span>`,
        esc(r.reason || '—'), `<span dir="ltr">${esc(r.at)}</span>`]), 'لا تعديلات بعد.');
  }

  window.FinanceOps6 = {
    tabs: [
      ['currencies', 'العملات'],
      ['branchgroups', 'مجموعات الفروع'],
      ['taxreturns', 'الإقرارات الضريبية'],
      ['notes', 'الإشعارات المدينة والدائنة'],
      ['zakat', 'دورة استحقاق الزكاة'],
      ['budgets', 'الموازنة'],
      ['budgetedit', 'تعديل الموازنة'],
      ['deptbudgets', 'موازنات الإدارات'],
      ['closehub', 'الإقفال المالي الموحّد'],
      ['fxconvert', 'صرف العملة']
    ],
    has: tab => Object.prototype.hasOwnProperty.call(views, tab),
    view: tab => views[tab](),
    bind
  };
})();
