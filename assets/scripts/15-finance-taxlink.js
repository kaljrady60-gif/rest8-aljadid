// ربط الضرائب بالقيود والسندات: ربط ضريبة بمستند قائم، ملخّص ضريبي، وإلغاء الربط.
// كل الحساب في الخادم (Decimal HALF_UP) وهذه الواجهة تعرض النتيجة فقط.
(function () {
  'use strict';
  const esc = v => String(v == null ? '' : v)
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  const money = v => Number(v || 0).toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 4 });
  const cid = () => Number(window.FinanceApi.companyId());
  const q = extra => `?company_id=${cid()}${extra || ''}`;
  const rate = bp => `${(Number(bp) / 100).toFixed(2)}%`;

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
  const val = (scope, name) => {
    const el = scope.querySelector(`[name="${name}"]`);
    return el ? el.value : '';
  };
  function note(scope, sel, text, bad) {
    const el = scope.querySelector(sel);
    if (!el) return;
    el.textContent = text;
    el.className = bad ? 'fp-note fp-error' : 'fp-note fp-ok';
  }
  const DOC_LABEL = { journal: 'قيد', voucher: 'سند' };
  const DIR_LABEL = { output: 'مخرجة (بيع)', input: 'مدخلة (شراء)' };

  const views = {
    async taxlinks() {
      const [links, summary, taxes] = await Promise.all([
        api(`/api/finance/tax-links${q()}`).catch(() => []),
        api(`/api/finance/tax-links/report${q()}`).catch(() => []),
        api(`/api/finance/taxes${q()}`).catch(() => [])
      ]);
      const taxRows = Array.isArray(taxes) ? taxes.filter(t => !t.archived_at) : [];
      const taxOptions = taxRows
        .map(t => `<option value="${esc(t.id)}">${esc(t.code)} — ${esc(t.name)} (${rate(t.rate_bp)})</option>`)
        .join('') || '<option value="">لا توجد ضرائب معرّفة</option>';
      const totals = (Array.isArray(summary) ? summary : []);
      const cards = totals.length
        ? totals.map(s => `<div class="fp-dims" data-help="ملخّص ضريبة ${esc(s.tax_code)} باتجاه ${esc(DIR_LABEL[s.direction] || s.direction)}">
            <span>${esc(s.tax_code)} · ${esc(DIR_LABEL[s.direction] || s.direction)}</span>
            <strong dir="ltr">${money(s.tax_amount)}</strong>
            <small>الوعاء <b dir="ltr">${money(s.base)}</b> · النسبة ${rate(s.rate_bp)} · ${esc(s.links)} ربط</small>
          </div>`).join('')
        : '<p class="fp-empty">لم يُربط أي ضريبة بمستند بعد.</p>';
      const list = Array.isArray(links) ? links : [];
      const rows = list.map(l => `<tr>
          <td>${esc(DOC_LABEL[l.doc_type] || l.doc_type)} #${esc(l.doc_id)}</td>
          <td dir="ltr">${esc(l.doc_date || '—')}</td>
          <td>${esc(l.tax_code)} <small>${rate(l.rate_bp)}</small></td>
          <td>${esc(DIR_LABEL[l.direction] || l.direction)}</td>
          <td dir="ltr">${money(l.base)}</td>
          <td dir="ltr">${money(l.tax_amount)}</td>
          <td>${l.archived_at
            ? '<span class="fp-badge">ملغى</span>'
            : `<button type="button" class="fp-mini" data-unlink="${esc(l.id)}" data-help="إلغاء ربط هذه الضريبة بالمستند مع إبقاء أثر تدقيقي.">إلغاء الربط</button>`}</td>
        </tr>`).join('');

      return `<div class="fp-form" data-help="ربط ضريبة معرّفة بقيد مرحّل أو سند قائم وحساب وعائها وضريبتها.">
          <h3>ربط ضريبة بمستند</h3>
          <form id="fp-tl-form" class="fp-grid">
            <label>نوع المستند
              <select name="doc_type" data-help="قيد مرحّل أو سند قبض/صرف.">
                <option value="journal">قيد</option>
                <option value="voucher">سند</option>
              </select>
            </label>
            <label>رقم المستند <input name="doc_id" type="number" min="1" step="1" dir="ltr" required data-help="معرّف القيد أو السند كما يظهر في شاشته." placeholder="1"></label>
            <label>الضريبة <select name="tax_id" data-help="من الضرائب المعرّفة غير المؤرشفة.">${taxOptions}</select></label>
            <label>الاتجاه
              <select name="direction" data-help="مخرجة على المبيعات أو مدخلة على المشتريات.">
                <option value="output">مخرجة (بيع)</option>
                <option value="input">مدخلة (شراء)</option>
              </select>
            </label>
            <label>المبلغ <input name="amount" dir="ltr" required placeholder="1000.0000" data-help="المبلغ الإجمالي أو الوعاء حسب إعداد الضريبة."></label>
            <label>&nbsp;<button type="submit" class="fp-primary" data-help="حساب الوعاء والضريبة بربع منازل وربطهما بالمستند.">ربط الضريبة</button></label>
          </form>
          <p class="fp-note" id="fp-tl-note"></p>
        </div>
        <div class="fp-form" data-help="فلترة الملخّص الضريبي بتاريخ المستند.">
          <h3>فلترة الملخّص</h3>
          <div class="fp-grid">
            <label>من تاريخ <input id="fp-tl-from" type="date" dir="ltr" data-help="أول تاريخ مستند يُحتسب."></label>
            <label>إلى تاريخ <input id="fp-tl-to" type="date" dir="ltr" data-help="آخر تاريخ مستند يُحتسب."></label>
            <label>&nbsp;<button type="button" class="fp-primary" id="fp-tl-filter" data-help="تحديث الملخّص حسب التاريخين.">تحديث الملخّص</button></label>
          </div>
          <p class="fp-note" id="fp-tl-filter-note"></p>
        </div>
        <div class="fp-form">
          <h3>الملخّص الضريبي</h3>
          <div class="kpi-row" id="fp-tl-cards">${cards}</div>
        </div>
        <div class="fp-form">
          <h3>الروابط القائمة</h3>
          ${list.length ? `<div class="fp-table-wrap"><table class="fp-table">
            <thead><tr><th>المستند</th><th>التاريخ</th><th>الضريبة</th><th>الاتجاه</th><th>الوعاء</th><th>الضريبة</th><th>إجراء</th></tr></thead>
            <tbody>${rows}</tbody></table></div>` : '<p class="fp-empty">لا توجد روابط بعد.</p>'}
        </div>`;
    }
  };

  function bind(tab, scope, refresh) {
    if (tab !== 'taxlinks') return;
    const form = scope.querySelector('#fp-tl-form');
    if (form) form.addEventListener('submit', async ev => {
      ev.preventDefault();
      const btn = form.querySelector('button[type="submit"]');
      btn.disabled = true;
      try {
        const out = await post('/api/finance/tax-links', {
          company_id: cid(),
          doc_type: val(form, 'doc_type'),
          doc_id: Number(val(form, 'doc_id')),
          tax_id: Number(val(form, 'tax_id')),
          direction: val(form, 'direction'),
          amount: val(form, 'amount').trim()
        });
        note(scope, '#fp-tl-note', `تم الربط: الوعاء ${money(out.base)} · الضريبة ${money(out.tax)} · الإجمالي ${money(out.total)}`);
        if (refresh) await refresh();
      } catch (e) {
        note(scope, '#fp-tl-note', e.message, true);
        btn.disabled = false;
      }
    });

    const filter = scope.querySelector('#fp-tl-filter');
    if (filter) filter.addEventListener('click', async () => {
      const from = scope.querySelector('#fp-tl-from').value;
      const to = scope.querySelector('#fp-tl-to').value;
      const extra = `${from ? `&from=${from}` : ''}${to ? `&to=${to}` : ''}`;
      try {
        const rows = await api(`/api/finance/tax-links/report${q(extra)}`);
        const host = scope.querySelector('#fp-tl-cards');
        host.innerHTML = (Array.isArray(rows) && rows.length)
          ? rows.map(s => `<div class="fp-dims"><span>${esc(s.tax_code)} · ${esc(DIR_LABEL[s.direction] || s.direction)}</span>
              <strong dir="ltr">${money(s.tax_amount)}</strong>
              <small>الوعاء <b dir="ltr">${money(s.base)}</b> · النسبة ${rate(s.rate_bp)} · ${esc(s.links)} ربط</small></div>`).join('')
          : '<p class="fp-empty">لا توجد روابط في هذه الفترة.</p>';
        note(scope, '#fp-tl-filter-note', 'حُدّث الملخّص.');
      } catch (e) { note(scope, '#fp-tl-filter-note', e.message, true); }
    });

    scope.querySelectorAll('[data-unlink]').forEach(btn => {
      btn.addEventListener('click', async () => {
        btn.disabled = true;
        try {
          await post('/api/finance/tax-links/unlink', { company_id: cid(), id: Number(btn.dataset.unlink) });
          if (refresh) await refresh();
        } catch (e) { window.alert(e.message); btn.disabled = false; }
      });
    });
  }

  window.FinanceOps5 = {
    tabs: [['taxlinks', 'ربط الضريبة بالقيود']],
    has: tab => Object.prototype.hasOwnProperty.call(views, tab),
    view: tab => views[tab](),
    bind
  };
})();
