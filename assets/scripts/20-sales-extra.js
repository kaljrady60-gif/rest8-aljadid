// شاشات المبيعات التكميلية: المرتجعات، العروض، الطلبات، البيانات، العمولات، التقارير، POS.
(function () {
  'use strict';
  const H = () => window.SalesHub;
  const esc = v => H().esc(v);
  const money = v => H().money(v);
  const table = (h, r) => H().table(h, r);
  const chips = i => H().chips(i);
  const badge = s => H().badge(s);
  const body = () => document.getElementById('sv-body');
  const cid = () => H().state.company;

  const STATUS_LABEL = {
    draft: 'مسودة', sent: 'مُرسل', accepted: 'مقبول', converted: 'محوّل',
    expired: 'منتهي', cancelled: 'ملغى', confirmed: 'مؤكد', delivered: 'مُسلّم',
    invoiced: 'مفوتر', posted: 'مرحّل', active: 'نشط', archived: 'مؤرشف',
    open: 'فتح', handover: 'تسليم', close: 'إقفال'
  };

  function fail(e) {
    body().innerHTML = `<p class="fp-error">${esc(e.message)}</p>`;
  }

  // ---------- المرتجعات ----------
  async function returns() {
    const data = await H().get(`/api/sales/invoices?company_id=${cid()}&invoice_type=sale_return`);
    const rows = (data.items || []).map(i => [
      esc(i.number), esc(i.date), esc(i.party), money(i.total), esc(i.currency), badge(i.status),
      i.journal_entry_id || '—'
    ]);
    body().innerHTML = `
      ${chips([['عدد المرتجعات', String((data.items || []).length)],
        ['قيمة المرتجعات', money((data.items || []).reduce((s, x) => s + Number(x.total || 0), 0))]])}
      <p class="fp-note">المرتجع لا يُنشأ من الصفر: يُفتح من فاتورة مرحّلة عبر زر «مرتجع»، والقيود تُعكس تلقائيًا.</p>
      ${table(['الرقم', 'التاريخ', 'العميل', 'الإجمالي', 'العملة', 'الحالة', 'قيد اليومية'], rows)}`;
  }

  // ---------- العروض والطلبات ----------
  async function docView(kind) {
    const data = await H().get(`/api/sales/docs?company_id=${cid()}&kind=${kind}`);
    const label = kind === 'quote' ? 'عرض سعر' : 'طلب عميل';
    const rows = (data.items || []).map(d => [
      esc(d.number), esc(d.date), esc(d.party), money(d.total), esc(d.currency),
      `<span class="sv-badge sv-neutral">${esc(STATUS_LABEL[d.status] || d.status)}</span>`,
      d.invoice_id ? `فاتورة #${d.invoice_id}` : '',
      d.invoice_id ? '—' : `<button type="button" class="fp-act act-success" data-convert="${kind}:${d.id}">تحويل لفاتورة</button>`
    ]);
    body().innerHTML = `
      <div class="sv-toolbar">
        <button type="button" class="fp-act act-primary" id="sv-new-doc">+ ${label}</button>
        <span class="sv-hint">${kind === 'quote' ? 'العرض المنتهي لا يُحوَّل.' : 'الطلب المؤكد يُحوَّل لفاتورة بنفس السطور.'}</span>
      </div>
      ${table(['الرقم', 'التاريخ', 'العميل', 'الإجمالي', 'العملة', 'الحالة', 'الفاتورة', 'إجراء'], rows)}
      <div id="sv-doc-form"></div>`;
    document.getElementById('sv-new-doc').addEventListener('click', () => docForm(kind));
    body().querySelectorAll('[data-convert]').forEach(b => b.addEventListener('click', async () => {
      const [k, id] = b.dataset.convert.split(':');
      try {
        const res = await H().post(`/api/sales/docs/${k}/${id}/convert`, { company_id: cid() });
        window.toast && window.toast(`أُنشئت الفاتورة #${res.invoice_id}`);
        await H().refresh();
      } catch (e) { alert(e.message); }
    }));
  }

  function docForm(kind) {
    const parties = (H().state.setup.parties || []);
    const host = document.getElementById('sv-doc-form');
    host.innerHTML = `
      <form class="sv-form" id="sv-docf">
        <h3>${kind === 'quote' ? 'عرض سعر جديد' : 'طلب عميل جديد'}</h3>
        <div class="sv-grid">
          <label>العميل<select name="party_id" required>${parties.map(p => `<option value="${p.id}">${esc(p.name)}</option>`).join('')}</select></label>
          <label>التاريخ<input type="date" name="date" required value="${new Date().toISOString().slice(0, 10)}"></label>
          ${kind === 'quote' ? `<label>صالح حتى<input type="date" name="valid_until"></label>` : ''}
          <label>ملاحظة<input type="text" name="note" placeholder="اختياري"></label>
        </div>
        <table class="sv-lines"><thead><tr><th>الصنف</th><th>الكمية</th><th>السعر</th><th>خصم</th></tr></thead>
          <tbody id="sv-doc-lines"><tr>${docLine()}</tr></tbody></table>
        <div class="sv-toolbar">
          <button type="button" class="fp-act act-neutral" id="sv-doc-add">+ بند</button>
          <button type="submit" class="fp-act act-primary">حفظ</button>
          <button type="button" class="fp-act act-neutral" id="sv-doc-close">إغلاق</button>
        </div>
      </form>`;
    document.getElementById('sv-doc-add').addEventListener('click', () =>
      document.getElementById('sv-doc-lines').insertAdjacentHTML('beforeend', `<tr>${docLine()}</tr>`));
    document.getElementById('sv-doc-close').addEventListener('click', () => { host.innerHTML = ''; });
    document.getElementById('sv-docf').addEventListener('submit', async ev => {
      ev.preventDefault();
      const form = ev.currentTarget;
      const lines = [...form.querySelectorAll('#sv-doc-lines tr')].map(tr => ({
        item_name: tr.querySelector('[name=item]').value.trim(),
        qty: tr.querySelector('[name=qty]').value.trim(),
        price: tr.querySelector('[name=price]').value.trim(),
        discount: tr.querySelector('[name=ldiscount]').value.trim()
      })).filter(l => l.item_name && l.qty && l.price);
      if (!lines.length) { alert('أضف بندًا واحدًا على الأقل.'); return; }
      try {
        await H().post('/api/sales/docs', {
          company_id: cid(), kind,
          party_id: Number(form.elements.party_id.value),
          date: form.elements.date.value,
          valid_until: form.elements.valid_until ? form.elements.valid_until.value || null : null,
          note: form.elements.note.value.trim() || null,
          lines
        });
        await H().refresh();
        await docView(kind);
      } catch (e) { alert(e.message); }
    });
  }

  const docLine = () => `<td><input name="item" placeholder="الصنف" required></td>
    <td><input name="qty" inputmode="decimal" placeholder="1" required></td>
    <td><input name="price" inputmode="decimal" placeholder="0.00" required></td>
    <td><input name="ldiscount" inputmode="decimal" placeholder="0.00"></td>`;

  // ---------- العملاء ----------
  async function customers() {
    const parties = H().state.setup.parties || [];
    const rows = parties.map(p => [
      esc(p.name), esc(p.currency),
      `<button type="button" class="fp-act act-neutral" data-open="screen:fin.parties">إدارة في المالية</button>`
    ]);
    body().innerHTML = `
      ${chips([['عدد العملاء', String(parties.length)]])}
      <p class="fp-note">العملاء مصدرهم الواحد هو أطراف المالية — لا جدول عملاء موازٍ.</p>
      ${table(['العميل', 'العملة', 'إجراء'], rows)}`;
  }

  // ---------- الكيانات ----------
  const ENTITY_TABS = [
    ['groups', 'مجموعات العملاء', [['code', 'الرمز'], ['name', 'الاسم'], ['discount_bp', 'خصم (نقاط أساس)']]],
    ['reps', 'المندوبون', [['number', 'الرقم'], ['name', 'الاسم'], ['phone', 'الجوال'], ['commission_bp', 'عمولة (نقاط أساس)']]],
    ['collectors', 'المحصّلون', [['number', 'الرقم'], ['name', 'الاسم'], ['phone', 'الجوال'], ['commission_bp', 'عمولة (نقاط أساس)']]],
    ['cards', 'بطاقات الدفع', [['name', 'الاسم'], ['network_limit', 'حد الشبكة'], ['off_network_limit', 'حد خارج الشبكة'], ['commission_bp', 'عمولة']]],
    ['types', 'أنواع المبيعات', [['code', 'الرمز'], ['name', 'الاسم'], ['kind', 'النوع (cash/credit/pos)']]],
    ['reasons', 'أسباب الإرجاع', [['code', 'الرمز'], ['name', 'الاسم'], ['category', 'التصنيف']]],
    ['machines', 'أجهزة POS', [['number', 'الرقم'], ['name', 'الاسم']]],
    ['cash', 'عملاء نقديون', [['name', 'الاسم'], ['phone', 'الجوال']]]
  ];
  let entityKind = 'groups';

  async function entities() {
    const data = await H().get(`/api/sales/entities?company_id=${cid()}`);
    const [kind, label, fields] = ENTITY_TABS.find(t => t[0] === entityKind);
    const rows = (data[kind] || []).map(row => [
      ...fields.map(([key]) => esc(row[key] == null ? '' : row[key])),
      `<span class="sv-badge sv-neutral">${esc(STATUS_LABEL[row.status] || row.status || '')}</span>`,
      `<button type="button" class="fp-act act-danger" data-arch="${kind}:${row.id}">${row.status === 'active' ? 'أرشفة' : 'تنشيط'}</button>`
    ]);
    body().innerHTML = `
      <div class="sv-subtabs">${ENTITY_TABS.map(([id, name]) =>
        `<button type="button" data-ent="${id}" class="${id === kind ? 'active' : ''}">${name}</button>`).join('')}</div>
      <form class="sv-form sv-inline" id="sv-ent-form">
        <h3>${label}</h3>
        <div class="sv-grid">${fields.map(([key, name]) =>
          `<label>${name}<input name="${key}" ${key === 'code' || key === 'number' || key === 'name' ? 'required' : ''}></label>`).join('')}</div>
        <button type="submit" class="fp-act act-primary">إضافة</button>
      </form>
      ${table([...fields.map(f => f[1]), 'الحالة', 'إجراء'], rows)}`;
    body().querySelectorAll('[data-ent]').forEach(b => b.addEventListener('click', () => {
      entityKind = b.dataset.ent; entities();
    }));
    body().querySelectorAll('[data-arch]').forEach(b => b.addEventListener('click', async () => {
      const [k, id] = b.dataset.arch.split(':');
      try {
        await H().post(`/api/sales/entities/${k}/${id}/archive`, { company_id: cid() });
        entities();
      } catch (e) { alert(e.message); }
    }));
    document.getElementById('sv-ent-form').addEventListener('submit', async ev => {
      ev.preventDefault();
      const form = ev.currentTarget;
      const payload = { company_id: cid() };
      fields.forEach(([key]) => { payload[key] = form.elements[key].value.trim(); });
      if (payload.discount_bp) payload.discount_bp = Number(payload.discount_bp);
      if (payload.commission_bp) payload.commission_bp = Number(payload.commission_bp);
      try { await H().post(`/api/sales/entities/${kind}`, payload); entities(); }
      catch (e) { alert(e.message); }
    });
  }

  // ---------- العمولات ----------
  async function commissions() {
    const data = await H().get(`/api/sales/commissions?company_id=${cid()}`);
    const month = new Date().toISOString().slice(0, 7);
    body().innerHTML = `
      <div class="sv-toolbar">
        <input type="month" id="sv-period" value="${month}">
        <button type="button" class="fp-act act-success" id="sv-calc">احتساب الدورة</button>
        <button type="button" class="fp-act act-warn" id="sv-accrue">ترحيل الاستحقاق</button>
      </div>
      <h4>الخطط</h4>
      ${table(['الخطة', 'النسبة', 'الأساس', 'الحالة'],
        (data.plans || []).map(p => [esc(p.name), (p.percent_bp / 100).toFixed(2) + '٪', esc(p.basis), p.active ? 'نشطة' : 'موقوفة']))}
      <h4>القواعد</h4>
      ${table(['القاعدة', 'الأساس', 'الجهة', 'النسبة', 'الحد الأدنى'],
        (data.rules || []).map(r => [esc(r.name), esc(r.basis), esc(r.applies_to), (r.rate_bp / 100).toFixed(2) + '٪', money(r.min_amount)]))}
      <h4>الاحتسابات</h4>
      ${table(['الدورة', 'الجهة', 'الأساس', 'المبلغ', 'النسبة', 'العمولة'],
        (data.calcs || []).map(c => [esc(c.period), esc(c.subject), money(c.base), money(c.base), (c.rate_bp / 100).toFixed(2) + '٪', money(c.amount)]))}`;
    document.getElementById('sv-calc').addEventListener('click', async () => {
      try {
        const res = await H().post('/api/sales/commissions/calculate', { company_id: cid(), period: document.getElementById('sv-period').value });
        window.toast && window.toast(`حُسبت ${res.calculated} عمولة`);
        commissions();
      } catch (e) { alert(e.message); }
    });
    document.getElementById('sv-accrue').addEventListener('click', async () => {
      try {
        const res = await H().post('/api/sales/commissions/accrue', { company_id: cid(), period: document.getElementById('sv-period').value });
        window.toast && window.toast(`قيد الاستحقاق ${res.journal_entry_id}`);
        commissions();
      } catch (e) { alert(e.message); }
    });
  }

  // ---------- التقارير ----------
  async function aging() {
    const data = await H().get(`/api/sales/reports/aging?company_id=${cid()}`);
    body().innerHTML = `
      ${chips([['إجمالي الذمم', money(data.total)]])}
      ${table(['العميل', '0–30', '31–60', '61–90', '90+', 'الإجمالي'],
        (data.items || []).map(r => [esc(r.party), money(r.b0), money(r.b30), money(r.b60), money(r.b90), money(r.total)]))}`;
  }

  async function reports() {
    const data = await H().get(`/api/sales/reports/summary?company_id=${cid()}`);
    body().innerHTML = `
      ${chips([['المبيعات', money(data.sales)], ['المرتجعات', money(data.returns)],
        ['الصافي', money(data.net)], ['مسودات', String(data.drafts)]])}
      <h4>التوزيع الشهري</h4>
      ${table(['الشهر', 'مبيعات', 'مرتجعات'],
        (data.months || []).map(m => [esc(m.month), money(m.sales), money(m.returns)]))}
      <h4>أفضل العملاء</h4>
      ${table(['العميل', 'الإجمالي'], (data.top_parties || []).map(p => [esc(p.party), money(p.total)]))}
      <h4>أداء المندوبين</h4>
      ${table(['المندوب', 'المبيعات'], (data.reps || []).map(r => [esc(r.rep), money(r.total)]))}`;
  }

  // ---------- POS ----------
  async function pos() {
    const machines = (H().state.setup.entities.machines || []);
    const today = new Date().toISOString().slice(0, 10);
    body().innerHTML = `
      <div class="sv-toolbar">
        <label>الجهاز<select id="sv-machine">${machines.map(m => `<option value="${m.id}">${esc(m.name)}</option>`).join('')}</select></label>
        <label>التاريخ<input type="date" id="sv-shift-date" value="${today}"></label>
        <button type="button" class="fp-act act-neutral" id="sv-shift">إقفال الوردية</button>
      </div>
      <div id="sv-shift-out"></div>
      <h4>سجل العهود</h4>
      <div id="sv-custody"></div>`;
    document.getElementById('sv-shift').addEventListener('click', async () => {
      const machine = document.getElementById('sv-machine').value;
      const date = document.getElementById('sv-shift-date').value;
      try {
        const s = await H().get(`/api/sales/pos/shift?company_id=${cid()}&machine_id=${machine}&date=${date}`);
        document.getElementById('sv-shift-out').innerHTML = chips([
          ['العهدة الافتتاحية', money(s.opened)], ['مبيعات الوردية', money(s.sales)],
          ['مرتجعات', money(s.returns)], ['المتوقع', money(s.expected)],
          ['المسلَّم', money(s.delivered)], ['الفرق', money(s.difference)]
        ]) + `<p class="fp-note">عدد فواتير الوردية: ${s.invoices}</p>`;
      } catch (e) { alert(e.message); }
    });
    const rows = await H().get(`/api/sales/pos/custody?company_id=${cid()}`);
    document.getElementById('sv-custody').innerHTML = table(
      ['الجهاز', 'الكاشير', 'التاريخ', 'المبلغ', 'النوع', 'ملاحظة'],
      (rows.items || []).map(c => [esc(c.machine), esc(c.cashier), esc(c.date), money(c.amount),
        esc(STATUS_LABEL[c.kind] || c.kind), esc(c.note)]));
  }

  // ---------- التهيئة ----------
  async function setup() {
    const s = H().state.setup;
    const stats = s.stats || {};
    const accounts = s.accounts || {};
    body().innerHTML = `
      ${chips([['عملاء', String((s.parties || []).length)], ['فواتير', String(stats.invoices || 0)],
        ['عروض', String(stats.quotes || 0)], ['طلبات', String(stats.orders || 0)]])}
      <h4>حسابات الترحيل الفعلية</h4>
      ${table(['الوظيفة', 'الحساب'], [
        ['ذمم العملاء', 'حساب كل عميل في دليل الحسابات'],
        ['الإيرادات', esc(accounts.revenue || 'غير موجود — الترحيل سيرفض')],
        ['ضريبة المخرجات', esc(accounts.tax || 'لا توجد — تُرحّل الفاتورة بلا ضريبة')]
      ])}
      <div class="sv-toolbar">
        <button type="button" class="fp-act act-success" id="sv-seed">توليد بيانات مبيعات تجريبية</button>
      </div>
      <p class="fp-note">البيانات التجريبية تُنشأ مرة واحدة فقط؛ تُرقّم INV/QT/OR تلقائيًا وتُرحّل إلى القيود.</p>`;
    document.getElementById('sv-seed').addEventListener('click', async () => {
      if (!confirm('توليد بيانات مبيعات تجريبية لهذه الشركة؟')) return;
      try {
        const res = await H().post('/api/sales/seed', { company_id: cid() });
        window.toast && window.toast(`أُنشئت ${res.posted_invoices} فاتورة مرحّلة و${res.quotes} عرضًا`);
        await H().refresh();
      } catch (e) { alert(e.message); }
    });
  }

  window.SalesExtra = {
    views: {
      returns,
      quotes: () => docView('quote'),
      orders: () => docView('order'),
      customers,
      entities,
      commissions,
      aging,
      reports,
      pos,
      setup
    }
  };
})();
