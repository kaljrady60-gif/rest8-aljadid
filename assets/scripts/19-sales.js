// قسم المبيعات: مساحة عمل مستقلة بنفس أسلوب المالية — الفواتير والمرتجعات أولًا.
(function () {
  'use strict';
  const $ = sel => document.querySelector(sel);
  const esc = v => String(v == null ? '' : v)
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  const money = v => Number(v || 0).toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  const TABS = [
    ['invoices', 'فواتير المبيعات'],
    ['returns', 'المرتجعات'],
    ['quotes', 'عروض الأسعار'],
    ['orders', 'طلبات العملاء'],
    ['customers', 'العملاء'],
    ['entities', 'بيانات المبيعات'],
    ['commissions', 'العمولات'],
    ['aging', 'أعمار الذمم'],
    ['reports', 'تقارير المبيعات'],
    ['pos', 'نقاط البيع'],
    ['setup', 'تهيئة المبيعات']
  ];
  const STATUS = { draft: 'مسودة', posted: 'مرحّل', cancelled: 'ملغاة' };
  const state = { tab: 'invoices', setup: null, company: null, filter: 'all' };

  const companyId = async () => {
    if (state.company) return state.company;
    const id = window.FinanceApi && (await window.FinanceApi.ensureCompany());
    state.company = Number(id || 0);
    return state.company;
  };

  async function api(path, options) {
    const res = await window.apiFetch(path, options);
    let body = null;
    try { body = await res.json(); } catch (_) { body = null; }
    if (!res.ok) throw new Error((body && body.error) || 'تعذّر تنفيذ الطلب.');
    return body;
  }

  const get = path => api(path);
  const post = (path, payload) => api(path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload || {})
  });
  const q = extra => `?company_id=${state.company}${extra || ''}`;

  function table(headers, rows) {
    if (!rows.length) return '<p class="fp-note">لا توجد بيانات بعد.</p>';
    return `<div class="fp-table-wrap"><table class="fp-table"><thead><tr>${headers.map(h => `<th>${h}</th>`).join('')}</tr></thead>
      <tbody>${rows.map(cells => `<tr>${cells.map((c, i) => `<td data-label="${headers[i]}">${c}</td>`).join('')}</tr>`).join('')}</tbody></table></div>`;
  }

  function chips(items) {
    return `<div class="fn-chips">${items.map(([label, value]) =>
      `<span class="fn-chip"><em>${esc(label)}</em><strong>${esc(value)}</strong></span>`).join('')}</div>`;
  }

  function badge(status) {
    const map = { posted: 'ok', draft: 'warn', cancelled: 'danger', sent: 'warn', converted: 'ok', invoiced: 'ok', confirmed: 'warn' };
    return `<span class="sv-badge sv-${map[status] || 'neutral'}">${esc(STATUS[status] || status)}</span>`;
  }

  // ---------- الفواتير ----------
  async function renderInvoices() {
    const body = $('#sv-body');
    body.innerHTML = '<p class="fp-note">جارٍ التحميل…</p>';
    const data = await get(`/api/sales/invoices${q(`&status=${state.filter}`)}`);
    const rows = (data.items || []).map(inv => [
      esc(inv.number),
      esc(inv.date),
      esc(inv.party),
      inv.invoice_type === 'sale_return' ? '<span class="sv-badge sv-danger">مرتجع</span>' : 'بيع',
      money(inv.total),
      esc(inv.currency),
      badge(inv.status),
      esc(inv.rep || '—'),
      actionsFor(inv)
    ]);
    body.innerHTML = `
      <div class="sv-toolbar">
        <button type="button" class="fp-act act-primary" id="sv-new-invoice">+ فاتورة جديدة</button>
        <label class="sv-filter">الحالة
          <select id="sv-status-filter">
            ${['all', 'draft', 'posted', 'cancelled'].map(s => `<option value="${s}" ${state.filter === s ? 'selected' : ''}>${s === 'all' ? 'الكل' : STATUS[s]}</option>`).join('')}
          </select>
        </label>
        <span class="sv-hint">الترحيل يمرّ عبر محرّك القيود نفسه — لا قناة موازية.</span>
      </div>
      ${table(['الرقم', 'التاريخ', 'العميل', 'النوع', 'الإجمالي', 'العملة', 'الحالة', 'المندوب', 'إجراءات'], rows)}
      <div id="sv-invoice-form"></div>`;
    $('#sv-status-filter').addEventListener('change', e => { state.filter = e.target.value; renderInvoices(); });
    $('#sv-new-invoice').addEventListener('click', showInvoiceForm);
    body.querySelectorAll('[data-inv-post]').forEach(b => b.addEventListener('click', () => postInvoice(b.dataset.invPost)));
    body.querySelectorAll('[data-inv-cancel]').forEach(b => b.addEventListener('click', () => cancelInvoice(b.dataset.invCancel)));
    body.querySelectorAll('[data-inv-return]').forEach(b => b.addEventListener('click', () => createReturn(b.dataset.invReturn)));
    body.querySelectorAll('[data-inv-view]').forEach(b => b.addEventListener('click', () => showDetail(b.dataset.invView)));
  }

  function actionsFor(inv) {
    const parts = [`<button type="button" class="fp-act act-neutral" data-inv-view="${inv.id}">عرض</button>`];
    if (inv.status === 'draft') {
      parts.push(`<button type="button" class="fp-act act-success" data-inv-post="${inv.id}">ترحيل</button>`);
      parts.push(`<button type="button" class="fp-act act-danger" data-inv-cancel="${inv.id}">إلغاء</button>`);
    }
    if (inv.status === 'posted' && inv.invoice_type === 'sale') {
      parts.push(`<button type="button" class="fp-act act-warn" data-inv-return="${inv.id}">مرتجع</button>`);
    }
    return parts.join(' ');
  }

  async function postInvoice(id) {
    if (!confirm('ترحيل الفاتورة إلى القيود؟ لا يمكن التراجع بعدها.')) return;
    try {
      const res = await post(`/api/sales/invoices/${id}/post`, { company_id: state.company });
      window.toast && window.toast(`تم الترحيل — قيد رقم ${res.journal_entry_id}`);
      refresh();
    } catch (e) { alert(e.message); }
  }

  async function cancelInvoice(id) {
    if (!confirm('إلغاء المسودة؟')) return;
    try { await post(`/api/sales/invoices/${id}/cancel`, { company_id: state.company }); refresh(); }
    catch (e) { alert(e.message); }
  }

  async function createReturn(id) {
    if (!confirm('إنشاء مرتجع بكامل الكميات المتاحة وترحيله؟')) return;
    try {
      const res = await post('/api/sales/returns', { company_id: state.company, invoice_id: Number(id) });
      window.toast && window.toast(`مرتجع رقم ${res.id} — قيد ${res.journal_entry_id || '—'}`);
      refresh();
    } catch (e) { alert(e.message); }
  }

  async function showDetail(id) {
    const body = $('#sv-body');
    try {
      const inv = await get(`/api/sales/invoices/${id}${q()}`);
      const rows = (inv.lines || []).map(l => [l.line_no + 1, esc(l.item_name), esc(l.qty), money(l.price), money(l.discount), money(l.net)]);
      body.innerHTML = `
        <div class="sv-toolbar">
          <button type="button" class="fp-act act-neutral" id="sv-back">→ عودة للقائمة</button>
          <strong>${esc(inv.number)} · ${esc(inv.party)}</strong>
          ${badge(inv.status)}
        </div>
        ${chips([['التاريخ', inv.date], ['العملة', inv.currency], ['قبل الخصم', money(inv.subtotal)],
          ['الخصم', money(inv.discount)], ['الضريبة', money(inv.tax)], ['الإجمالي', money(inv.total)],
          ['قيد اليومية', inv.journal_entry_id || '—']])}
        ${table(['#', 'الصنف', 'الكمية', 'السعر', 'الخصم', 'الصافي'], rows)}`;
      $('#sv-back').addEventListener('click', refresh);
    } catch (e) { body.innerHTML = `<p class="fp-error">${esc(e.message)}</p>`; }
  }

  function showInvoiceForm() {
    const customers = (state.setup && state.setup.parties) || [];
    const reps = ((state.setup && state.setup.entities && state.setup.entities.reps) || []);
    const host = $('#sv-invoice-form');
    host.innerHTML = `
      <form class="sv-form" id="sv-inv-form">
        <h3>فاتورة بيع جديدة</h3>
        <div class="sv-grid">
          <label>العميل<select name="party_id" required>${customers.map(c => `<option value="${c.id}">${esc(c.name)}</option>`).join('')}</select></label>
          <label>التاريخ<input type="date" name="invoice_date" required value="${new Date().toISOString().slice(0, 10)}"></label>
          <label>المندوب<select name="rep_id"><option value="">بلا مندوب</option>${reps.map(r => `<option value="${r.id}">${esc(r.name)}</option>`).join('')}</select></label>
          <label>نسبة الضريبة (٪)<input type="number" name="tax_bp_percent" min="0" max="100" step="0.01" value="5"></label>
          <label>خصم الفاتورة<input type="text" name="discount" placeholder="0.00" inputmode="decimal"></label>
        </div>
        <table class="sv-lines">
          <thead><tr><th>الصنف</th><th>الكمية</th><th>السعر</th><th>خصم السطر</th><th></th></tr></thead>
          <tbody id="sv-lines"><tr>${lineRow()}</tr></tbody>
        </table>
        <div class="sv-toolbar">
          <button type="button" class="fp-act act-neutral" id="sv-add-line">+ إضافة بند</button>
          <button type="submit" class="fp-act act-primary">حفظ المسودة</button>
          <button type="button" class="fp-act act-neutral" id="sv-hide-form">إغلاق</button>
        </div>
      </form>`;
    $('#sv-add-line').addEventListener('click', () => {
      $('#sv-lines').insertAdjacentHTML('beforeend', `<tr>${lineRow()}</tr>`);
    });
    $('#sv-hide-form').addEventListener('click', () => { host.innerHTML = ''; });
    $('#sv-inv-form').addEventListener('submit', async ev => {
      ev.preventDefault();
      const form = ev.currentTarget;
      const lines = [...form.querySelectorAll('#sv-lines tr')].map(tr => ({
        item_name: tr.querySelector('[name=item]').value.trim(),
        qty: tr.querySelector('[name=qty]').value.trim(),
        price: tr.querySelector('[name=price]').value.trim(),
        discount: tr.querySelector('[name=ldiscount]').value.trim()
      })).filter(l => l.item_name && l.qty && l.price);
      if (!lines.length) { alert('أضف بندًا واحدًا على الأقل باسم وكمية وسعر.'); return; }
      const taxPercent = Number(form.elements.tax_bp_percent.value || 0);
      const payload = {
        company_id: state.company,
        party_id: Number(form.elements.party_id.value),
        invoice_date: form.elements.invoice_date.value,
        tax_bp: Math.round(taxPercent * 100),
        discount: form.elements.discount.value.trim() || null,
        rep_id: form.elements.rep_id.value ? Number(form.elements.rep_id.value) : null,
        lines
      };
      try {
        const res = await post('/api/sales/invoices', payload);
        window.toast && window.toast(`حُفظت المسودة رقم ${res.id}`);
        refresh();
      } catch (e) { alert(e.message); }
    });
    host.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }

  const lineRow = () => `<td><input name="item" placeholder="اسم الصنف" required></td>
    <td><input name="qty" inputmode="decimal" placeholder="1" required></td>
    <td><input name="price" inputmode="decimal" placeholder="0.00" required></td>
    <td><input name="ldiscount" inputmode="decimal" placeholder="0.00"></td>
    <td><button type="button" class="fp-act act-danger" onclick="this.closest('tr').remove()">حذف</button></td>`;

  function refresh() {
    render(state.tab);
  }

  // ---------- الهيكل ----------
  async function render(tab) {
    state.tab = tab;
    const body = $('#sv-body');
    if (!body) return;
    body.dataset.tab = tab;
    document.querySelectorAll('#sv-workbench [data-sales-tab]').forEach(b =>
      b.classList.toggle('active', b.dataset.salesTab === tab));
    try {
      const unit = window.SalesExtra && window.SalesExtra.views[tab];
      await (unit ? unit() : renderInvoices());
    } catch (e) {
      body.innerHTML = `<p class="fp-error">${esc(e.message)}</p>`;
    }
    if (window.ActionSemantics && window.ActionSemantics.apply) {
      window.ActionSemantics.apply(body);
    }
  }

  async function loadSetup() {
    state.company = await companyId();
    state.setup = await get(`/api/sales/setup${q()}`);
    state.setup.entities = await get(`/api/sales/entities${q()}`);
    paintIndicators();
    return state.setup;
  }

  function paintIndicators() {
    const s = (state.setup && state.setup.stats) || {};
    document.querySelectorAll('#sv-workbench [data-sv-kpi]').forEach(node => {
      const key = node.dataset.svKpi;
      const value = {
        invoices: s.invoices, posted: s.posted, drafts: s.drafts, receivable: s.receivable, sales: s.sales_total
      }[key];
      node.querySelector('strong').textContent = value != null ? value : '—';
    });
  }

  function buildWorkbench() {
    if ($('#sv-workbench')) return;
    const section = document.createElement('section');
    section.id = 'sv-workbench';
    section.className = 'sv-workbench hidden';
    section.setAttribute('aria-label', 'مساحة المبيعات');
    section.innerHTML = `
      <div class="finance-crumb"><button type="button" data-open="home">الرئيسية</button><span>‹</span><b>المبيعات والعملاء</b></div>
      <header class="finance-room-header">
        <div class="finance-room-main">
          <div class="finance-room-art"><img src="/assets/clients.webp" alt=""></div>
          <div><h2>المبيعات والعملاء</h2><p>الفواتير والعملاء والتحصيل والعمولات</p></div>
        </div>
        <div class="finance-room-controls">
          <button type="button" data-open="home" class="ghost-btn">العودة ←</button>
        </div>
      </header>
      <div class="finance-indicators">
        <div data-sv-kpi="sales"><span>إجمالي المبيعات</span><strong>…</strong><small>مرحّلة</small></div>
        <div data-sv-kpi="receivable"><span>ذمم غير محصّلة</span><strong>…</strong><small>متبقٍ</small></div>
        <div data-sv-kpi="posted"><span>فواتير مرحّلة</span><strong>…</strong><small>لها قيود</small></div>
        <div data-sv-kpi="drafts"><span>بانتظار الترحيل</span><strong>…</strong><small>مسودات</small></div>
      </div>
      <nav class="finance-tab-bar" aria-label="أقسام المبيعات">
        ${TABS.map(([id, label], i) => `<button type="button" data-sales-tab="${id}" class="${i === 0 ? 'active' : ''}">${label}</button>`).join('')}
      </nav>
      <div class="fp-body" id="sv-body"></div>`;
    const anchor = $('#view-home');
    if (anchor && anchor.parentNode) anchor.parentNode.insertBefore(section, anchor);
    else document.body.appendChild(section);
    section.querySelectorAll('[data-sales-tab]').forEach(btn =>
      btn.addEventListener('click', () => render(btn.dataset.salesTab)));
  }

  let ready = false;
  async function openSales(tab) {
    buildWorkbench();
    document.body.classList.add('sales-mode');
    document.querySelectorAll('#view-home, #view-detail, #finance-workbench').forEach(n => n.classList.add('hidden'));
    $('#view-detail') && $('#view-detail').classList.add('hidden');
    const shell = $('#sv-workbench');
    shell.classList.remove('hidden');
    if (!ready) {
      try { await loadSetup(); } catch (e) { $('#sv-body').innerHTML = `<p class="fp-error">${esc(e.message)}</p>`; return; }
      ready = true;
    }
    await render(tab || 'invoices');
  }

  window.SalesHub = {
    open: openSales,
    refresh: async () => { await loadSetup(); refresh(); },
    companyId,
    get, post, table, chips, esc, money, badge,
    state
  };

  const previous = window.navigate;
  window.navigate = function (id) {
    if (id === 'sales' || String(id).startsWith('sales:')) {
      return openSales(String(id).split(':')[1] || 'invoices');
    }
    document.body.classList.remove('sales-mode');
    const shell = $('#sv-workbench');
    if (shell) shell.classList.add('hidden');
    return previous ? previous(id) : undefined;
  };
})();
