'use strict';
(() => {
  const panel = document.getElementById('registry-panel');
  const setup = document.getElementById('registry-form');
  const voucher = document.getElementById('linked-voucher-form');
  const status = document.getElementById('registry-status');
  let data = null, accounts = [], serial = 0, pending = false, frozen = null;
  for (const control of panel.querySelectorAll('input,select')) {
    if (!control.dataset.help) control.dataset.help = `اختر أو أدخل ${control.closest('label')?.textContent.trim() || 'القيمة المطلوبة'}.`;
  }
  const fields = {
    company: ['code', 'name'],
    branch: ['company_id', 'code', 'name'],
    account_scope: ['company_id', 'account_id'],
    party: ['company_id', 'branch_id', 'account_id', 'name', 'role'],
    fund: ['company_id', 'branch_id', 'account_id', 'name', 'kind']
  };
  function note(message) { status.textContent = message; }
  function options(select, rows, label) {
    const previous = select.value;
    select.replaceChildren(new Option('اختر…', ''));
    for (const row of rows) select.add(new Option(label(row), String(row.id)));
    if ([...select.options].some(o => o.value === previous)) select.value = previous;
    else select.value = '';
  }
  function render() {
    if (!data) return;
    const type = setup.elements.type.value;
    for (const label of setup.querySelectorAll('[data-reg]')) {
      const visible = fields[type].includes(label.dataset.reg);
      label.hidden = !visible;
      label.firstElementChild.required = visible;
      label.firstElementChild.disabled = !visible;
    }
    for (const form of [setup, voucher]) {
      options(form.elements.company_id, data.companies, x => `${x.code} · ${x.name}`);
      const company = Number(form.elements.company_id.value);
      options(form.elements.branch_id, data.branches.filter(x => x.company_id === company), x => `${x.code} · ${x.name}`);
    }
    const company = Number(setup.elements.company_id.value);
    let choices = accounts.filter(a => a.kind === 'detail');
    if (type === 'account_scope') {
      const scoped = new Set(data.scopes.map(x => x.account_id));
      choices = choices.filter(a => !scoped.has(a.id));
    } else {
      const scoped = new Set(data.scopes.filter(x => x.company_id === company).map(x => x.account_id));
      const taken = new Set([...data.funds, ...data.parties].map(x => x.account_id));
      choices = choices.filter(a => scoped.has(a.id) && !taken.has(a.id) && (type !== 'fund' || a.category === 'asset'));
    }
    options(setup.elements.account_id, choices, a => `${a.code} · ${a.name} · ${a.currency}`);
    const vc = Number(voucher.elements.company_id.value), vb = Number(voucher.elements.branch_id.value);
    options(voucher.elements.fund_id, data.funds.filter(x => x.company_id === vc && x.branch_id === vb), x => `${x.name} · ${x.currency}`);
    options(voucher.elements.party_id, data.parties.filter(x => x.company_id === vc && x.branch_id === vb), x => `${x.name} · ${x.currency}`);
    const overview = document.getElementById('registry-overview');
    overview.replaceChildren();
    for (const [label, value] of [['شركات', data.companies], ['فروع', data.branches], ['حسابات مربوطة', data.scopes], ['صناديق وبنوك', data.funds], ['أطراف', data.parties]]) {
      const span = document.createElement('span'); span.textContent = `${label}: ${value.length}`; overview.append(span);
    }
  }
  async function readJson(url, init) {
    const response = await apiFetch(url, init);
    const body = await response.json();
    if (!response.ok) throw new Error(body.error || 'تعذر تنفيذ الطلب.');
    return body;
  }
  async function refresh() {
    const current = ++serial;
    note('جارٍ تحميل السجل والحسابات…');
    try {
      const [next, nextAccounts] = await Promise.all([readJson('/api/finance/registry'), readJson('/api/accounts')]);
      if (current !== serial) return;
      if (!['companies', 'branches', 'scopes', 'funds', 'parties'].every(k => Array.isArray(next[k])) || !Array.isArray(nextAccounts)) throw new Error('رد السجل غير صالح.');
      data = next; accounts = nextAccounts; render(); note('تم تحميل السجل. الترحيل والعزل الشامل غير متاحين لهذه الميزة.');
    } catch (e) {
      if (current === serial) { data = null; note(e.message || 'تعذر تحميل البيانات.'); }
    }
  }
  function post(url, body) {
    return readJson(url, {method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify(body)});
  }
  panel.addEventListener('toggle', () => { if (panel.open) refresh(); });
  document.getElementById('registry-refresh').addEventListener('click', refresh);
  setup.elements.type.addEventListener('change', render);
  for (const form of [setup, voucher]) {
    form.elements.company_id.addEventListener('change', render);
    form.elements.branch_id.addEventListener('change', render);
  }
  setup.addEventListener('submit', async event => {
    event.preventDefault();
    if (pending || !data) return;
    const type = setup.elements.type.value, body = {type};
    for (const field of fields[type]) {
      const value = setup.elements[field].value;
      body[field] = field.endsWith('_id') ? Number(value) : value;
    }
    pending = true; setup.querySelector('button[type=submit]').disabled = true;
    try {
      const result = await post('/api/finance/registry', body);
      await refresh(); note(`تم حفظ السجل برقم ${result.id}.`);
    } catch (e) { note(`${e.message || 'تعذر الحفظ.'} إذا انقطع الاتصال راجع السجل قبل إعادة المحاولة.`); }
    finally { pending = false; setup.querySelector('button[type=submit]').disabled = false; }
  });
  voucher.addEventListener('submit', async event => {
    event.preventDefault();
    if (pending || !data) return;
    const body = {};
    for (const name of ['company_id', 'branch_id', 'fund_id', 'party_id', 'kind', 'date', 'currency', 'amount', 'description', 'reference']) {
      const value = voucher.elements[name].value;
      body[name] = name.endsWith('_id') ? Number(value) : value;
    }
    const payload = JSON.stringify(body);
    if (frozen && frozen.payload !== payload) { note('بيانات طلب سابق غير محسوم تغيّرت. راجع السجل ثم اختر بدء طلب جديد.'); return; }
    if (!frozen) {
      if (!globalThis.crypto?.getRandomValues) { note('يتعذر إنشاء معرّف حفظ آمن في هذا المتصفح.'); return; }
      const bytes = new Uint8Array(16); crypto.getRandomValues(bytes);
      frozen = {payload, token: [...bytes].map(x => x.toString(16).padStart(2, '0')).join('')};
    }
    body.request_id = frozen.token;
    pending = true; voucher.querySelector('button[type=submit]').disabled = true;
    try {
      const result = await post('/api/vouchers/linked', body);
      frozen = null; voucher.reset(); render();
      note(`حُفظ السند #${result.id} ومسودة القيد #${result.draft_id}. لم يُرحّل.`);
      if (typeof loadVouchers === 'function') loadVouchers();
    } catch (e) { note(`${e.message || 'تعذر الحفظ.'} أعد إرسال البيانات نفسها أو راجع السجل قبل بدء طلب جديد.`); }
    finally { pending = false; voucher.querySelector('button[type=submit]').disabled = false; }
  });
  document.getElementById('linked-reset').addEventListener('click', () => {
    if (pending) return;
    if (frozen && !confirm('قد يكون الطلب السابق قد حُفظ. هل راجعت سجل السندات قبل بدء طلب جديد؟')) return;
    frozen = null; voucher.reset(); render(); note('أُعد النموذج. راجع السجل قبل إعادة إنشاء سند مفقود الاستجابة.');
  });
})();
