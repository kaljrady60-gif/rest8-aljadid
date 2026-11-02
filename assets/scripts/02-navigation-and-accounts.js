'use strict';

function navigate(id) {
  setFinanceContext(id);
  if (id === 'finance') {
    navigate('journals');
    return;
  }
  if (id === 'finance-catalog') {
    const d = departments.find(x => x.id === 'finance');
    baseView('detail', d.title, 'فهرس الشاشات وحالة بنائها', 'المالية / فهرس الشاشات');
    setNav('departments');
    $('#view-detail').innerHTML = referenceCatalog ? showCatalogDepartment(d) : '<p class="account-notice">تعذر تحميل الفهرس.</p>';
    bindCatalogSearch();
    return;
  }
  if (id.startsWith('screen:')) {
    if (id === 'screen:fin.vouchers') {
      navigate('vouchers');
      return;
    }
    if (id === 'screen:fin.journal') {
      navigate('journals');
      return;
    }
    if (id === 'screen:fin.coa') {
      navigate('accounts');
      return;
    }
    if (id === 'screen:fin.ledger') {
      navigate('ledger');
      return;
    }
    if (id === 'screen:fin.trial') {
      navigate('trial');
      return;
    }
    if (id === 'screen:fin.periods') {
      navigate('periods');
      return;
    }
    showScreenDetails(id.slice(7));
    return;
  }
  if (id === 'home' || id === 'departments') {
    baseView('home', 'الإدارات الرئيسية', 'اختر الإدارة للانتقال إلى أقسامها وعملياتها', 'الإدارات الرئيسية');
    setNav(id === 'home' ? 'home' : 'departments');
    return;
  }
  if (id === 'vouchers') {
    if (!$('#view-vouchers')) {
      toast('افتح المعاينة المالية المحدثة لعرض السندات.');
      return;
    }
    baseView('vouchers', 'السندات', 'سندات القبض والصرف كمسودات مرتبطة بقيود', 'المالية / السندات');
    setNav('departments');
    loadVouchers();
    return;
  }
  if (id === 'accounts') {
    baseView('accounts', 'دليل الحسابات', 'استعراض الحسابات وإنشاء حساب تجريبي', 'المالية / دليل الحسابات');
    setNav('departments');
    loadAccounts();
    return;
  }
  if (id === 'journals') {
    baseView('journals', 'القيود اليومية', 'عرض المسودات المحفوظة وإضافة قيد جديد', 'المالية / القيود اليومية');
    setNav('journals');
    loadDrafts();
    return;
  }
  if (id === 'ledger' || id === 'trial') {
    baseView('ledger', id === 'trial' ? 'ميزان المراجعة' : 'حركات الأستاذ', 'بيانات تجريبية من القيود المرحّلة', 'المالية / ' + (id === 'trial' ? 'الميزان' : 'الأستاذ'));
    setNav('departments');
    $('#ledger-mode').value = id === 'trial' ? 'trial' : 'entries';
    configureLedger();
    loadLedger();
    return;
  }
  if (id === 'periods') {
    baseView('periods', 'الفترات المالية', 'إدارة تواريخ السماح بالترحيل', 'المالية / الفترات');
    setNav('departments');
    loadPeriods();
    return;
  }
  if (id === 'split') {
    baseView('split', 'عرض متجاور', 'افتح إدارتين في مساحة العمل نفسها', 'العرض المتجاور');
    setNav('departments');
    fillSplit();
    return;
  }
  const d = departments.find(x => x.id === id);
  if (!d) return;
  setNav(id === 'reports' ? 'reports' : id === 'settings' ? 'settings' : 'departments');
  baseView('detail', d.title, 'اختر قسمًا للاطلاع على حالته', 'الإدارات / ' + d.title);
  const catalog = referenceCatalog && showCatalogDepartment(d);
  $('#view-detail').innerHTML = catalog || `<div class="detail-banner"><div><span class="detail-kicker">الإدارات / ${d.title}</span><h2>${d.title}</h2><p>${d.help} الأقسام الموضحة أدناه تمثل نطاق العمل المخطط.</p></div><button class="ghost-btn" data-open="split">▥ فتح عرض متجاور</button></div><div class="detail-cards">${d.sections.map(sectionCard).join('')}</div>`;
  bindCatalogSearch();
}

const accountLabels = {
  asset: 'الأصول',
  liability: 'الالتزامات',
  equity: 'حقوق الملكية',
  revenue: 'الإيرادات',
  expense: 'المصروفات',
  group: 'رئيسي',
  detail: 'تفصيلي',
  debit: 'مدين',
  credit: 'دائن'
};

let loadedAccounts = [],
  accountCompanies = [],
  selectedAccount = null;

function accountCompanyOptions(select) {
  const current = select.value;
  select.replaceChildren(new Option('غير مسند — تجريبي', ''));
  for (const c of accountCompanies) select.add(new Option(`${c.code} · ${c.name}`, String(c.id)));
  if ([...select.options].some(o => o.value === current)) select.value = current;
}
function accountsInCompany(company) {
  return loadedAccounts.filter(a => String(a.company_id || '') === company);
}
function populateAccountParents(parentId = null) {
  const form = $('#account-form'), select = form.elements.parent_id;
  const company = form.elements.company_id.value;
  select.innerHTML = '<option value="">حساب جذري بلا أب</option>' + accountsInCompany(company).filter(a => a.kind === 'group').map(a => `<option value="${escapeHTML(a.id)}">${escapeHTML(a.code)} — ${escapeHTML(a.name)}</option>`).join('');
  if (parentId) select.value = String(parentId);
}


function renderAccountInspector() {
  const panel = $('#account-inspector'),
    a = loadedAccounts.find(x => x.id === selectedAccount);
  if (!a) {
    panel.innerHTML = '<h3>تفاصيل الحساب</h3><p>اختر حسابًا من الشجرة لعرض تفاصيله.</p>';
    return;
  }
  const parent = loadedAccounts.find(x => x.id === a.parent_id);
  panel.innerHTML = `<h3>تفاصيل الحساب</h3><div class="account-selected"><strong>${escapeHTML(a.name)}</strong><span dir="ltr">${escapeHTML(a.code)}</span></div><dl class="account-properties"><div><dt>التصنيف</dt><dd>${escapeHTML(accountLabels[a.category] || a.category)}</dd></div><div><dt>النوع</dt><dd>${escapeHTML(accountLabels[a.kind] || a.kind)}</dd></div><div><dt>الطبيعة</dt><dd>${escapeHTML(accountLabels[a.nature] || a.nature)}</dd></div><div><dt>العملة</dt><dd>${escapeHTML(a.currency)}</dd></div><div><dt>الحساب الأب</dt><dd>${escapeHTML(parent ? parent.code + ' — ' + parent.name : 'لا يوجد')}</dd></div></dl><p class="account-muted">المسودات الجديدة تستخدم حسابات الدليل؛ الترحيل والميزان تجريبيان فقط إن فُعّلا.</p>${a.kind === 'group' ? '<button class="ghost-btn" type="button" id="account-child">＋ إضافة حساب فرعي</button>' : ''} <button class="ghost-btn" type="button" id="account-rename" data-help="تعديل اسم الحساب فقط مع التحقق من عدم وجود تعديل متزامن.">تعديل الاسم</button>`;
  $('#account-child')?.addEventListener('click', () => openAccountForm(a.id));
  $('#account-rename')?.addEventListener('click', () => openRename(a));
}

function renderAccountTree() {
  const q = $('#account-search').value.trim().toLocaleLowerCase('ar');
  const children = new Map();
  const visibleAccounts = accountsInCompany($('#account-company-filter').value);
  for (const a of visibleAccounts) {
    const key = a.parent_id || 0;
    if (!children.has(key)) children.set(key, []);
    children.get(key).push(a);
  }
  const visited = new Set();
  function node(a, depth) {
    if (depth > 100 || visited.has(a.id)) return '';
    visited.add(a.id);
    const nested = (children.get(a.id) || []).map(x => node(x, depth + 1)).join('');
    const matches = !q || `${a.code} ${a.name}`.toLocaleLowerCase('ar').includes(q);
    if (!matches && !nested) return '';
    return `<div class="account-node"><button type="button" data-account-id="${escapeHTML(a.id)}" class="account-tree-row ${selectedAccount === a.id ? 'selected' : ''}" data-help="${escapeHTML(a.name)}، ${escapeHTML(accountLabels[a.kind] || a.kind)}. اختره لعرض تفاصيله."><span class="account-tree-code" dir="ltr">${escapeHTML(a.code)}</span><strong>${escapeHTML(a.name)}</strong><span class="account-kind">${escapeHTML(accountLabels[a.kind] || a.kind)}</span></button>${nested ? `<details open class="account-branches"><summary>الفروع</summary><div class="account-children">${nested}</div></details>` : ''}</div>`;
  }
  const html = (children.get(0) || []).map(a => node(a, 0)).join('');
  $('#account-tree').innerHTML = html || `<div class="account-empty">${q ? 'لا توجد حسابات مطابقة للبحث.' : 'لا توجد حسابات بعد. ابدأ بإضافة حساب جديد.'}</div>`;
  if (visited.size !== visibleAccounts.length) {
    $('#account-tree').insertAdjacentHTML('beforeend', '<p class="account-tree-error">تعذر عرض بعض الحسابات بسبب خلل في البنية؛ راجع سجل البيانات.</p>');
  }
  $('#account-count').textContent = `${visibleAccounts.length} حساب في النطاق المحدد`;
  renderAccountInspector();
}

async function loadAccounts() {
  try {
    const res = await apiFetch('/api/accounts');
    if (!res.ok) throw new Error('HTTP ' + res.status);
    const data = await res.json();
    if (!Array.isArray(data)) throw new Error('رد غير صالح');
    loadedAccounts = data;
    const registry = await apiFetch('/api/finance/registry');
    if (!registry.ok) throw new Error('تعذر تحميل الشركات');
    const organization = await registry.json();
    if (!Array.isArray(organization.companies)) throw new Error('رد الشركات غير صالح');
    accountCompanies = organization.companies;
    accountCompanyOptions($('#account-company-filter'));
    renderAccountTree();
    return true;
  } catch (error) {
    console.error('account list error:', error);
    loadedAccounts = [];
    selectedAccount = null;
    $('#account-count').textContent = 'تعذر تحميل الحسابات';
    $('#account-tree').innerHTML = '<p class="account-tree-error">تعذر تحميل دليل الحسابات. تحقق من اتصال الخادم.</p>';
    renderAccountInspector();
    return false;
  }
}

let renameTarget = null,
  renameSaving = false;

function openRename(account) {
  renameTarget = {
    id: account.id,
    name: account.name
  };
  $('#rename-form').elements.name.value = account.name;
  $('#rename-error').classList.add('hidden');
  $('#rename-dialog').showModal();
}

['#close-rename', '#cancel-rename'].forEach(x => $(x).addEventListener('click', () => requestDialogDismiss($('#rename-dialog'))));

$('#rename-form').addEventListener('submit', async e => {
  e.preventDefault();
  if (renameSaving || !renameTarget) return;
  const f = e.currentTarget,
    err = $('#rename-error'),
    btn = $('button[type=submit]', f);
  err.classList.add('hidden');
  renameSaving = true;
  btn.disabled = true;
  try {
    const r = await apiFetch('/api/accounts/' + encodeURIComponent(renameTarget.id), {
      method: 'PUT',
      headers: {
        'Content-Type': 'application/json'
      },
      body: JSON.stringify({
        name: f.elements.name.value,
        expected_name: renameTarget.name
      })
    });
    const data = await r.json();
    if (!r.ok) throw new Error(data.error || 'تعذر تعديل الاسم.');
    $('#rename-dialog').close();
    selectedAccount = data.id;
    if (await loadAccounts()) toast('حُفظ اسم الحساب.');else toast('تم الحفظ لكن تعذر تحديث الدليل.');
  } catch (error) {
    err.textContent = error instanceof TypeError ? 'تعذر الاتصال؛ قد يكون التعديل قد تم. تحقق من الدليل قبل المحاولة.' : error.message;
    err.classList.remove('hidden');
  } finally {
    renameSaving = false;
    btn.disabled = false;
  }
});

function openAccountForm(parentId = null) {
  const form = $('#account-form');
  form.reset();
  $('#account-form-error').classList.add('hidden');
  accountCompanyOptions(form.elements.company_id);
  const parent = loadedAccounts.find(x => x.id === parentId);
  form.elements.company_id.value = parent ? String(parent.company_id || '') : $('#account-company-filter').value;
  populateAccountParents(parentId);
  if (parentId) {
    if (parent) {
      form.elements.category.value = parent.category;
      form.elements.nature.value = parent.nature;
      form.elements.currency.value = parent.currency;
    }
  }
  $('#account-dialog').showModal();
}

$('#account-new').addEventListener('click', () => openAccountForm());

['#close-account', '#cancel-account'].forEach(x => $(x).addEventListener('click', () => requestDialogDismiss($('#account-dialog'))));

$('#account-search').addEventListener('input', renderAccountTree);
$('#account-company-filter').addEventListener('change', () => { selectedAccount = null; renderAccountTree(); });
$('#account-form').elements.company_id.addEventListener('change', () => populateAccountParents());

$('#account-tree').addEventListener('click', e => {
  const btn = e.target.closest('[data-account-id]');
  if (btn) {
    selectedAccount = Number(btn.dataset.accountId);
    renderAccountTree();
  }
});

let accountSaving = false;

$('#account-form').addEventListener('submit', async e => {
  e.preventDefault();
  if (accountSaving) return;
  const f = e.currentTarget,
    err = $('#account-form-error'),
    button = $('button[type=submit]', f);
  err.classList.add('hidden');
  const payload = {
    code: f.elements.code.value,
    name: f.elements.name.value,
    parent_id: f.elements.parent_id.value ? Number(f.elements.parent_id.value) : null,
    category: f.elements.category.value,
    kind: f.elements.kind.value,
    nature: f.elements.nature.value,
    currency: f.elements.currency.value
  };
  accountSaving = true;
  button.disabled = true;
  try {
    const company = f.elements.company_id.value;
    if (company) payload.company_id = Number(company);
    const res = await apiFetch(company ? '/api/finance/accounts' : '/api/accounts', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json'
      },
      body: JSON.stringify(payload)
    });
    const data = await res.json();
    if (!res.ok) throw new Error(data.error || 'تعذر حفظ الحساب.');
    $('#account-dialog').close();
    selectedAccount = data.id;
    $('#account-company-filter').value = company;
    if (await loadAccounts()) toast('حُفظ الحساب التجريبي دون إنشاء رصيد.');else toast('حُفظ الحساب، لكن تعذر تحديث الدليل. أعد تحميله.');
  } catch (error) {
    err.textContent = error instanceof TypeError ? 'تعذر الاتصال؛ قد يكون الحفظ قد تم. تحقق من الدليل قبل إعادة المحاولة.' : error.message || 'تعذر الاتصال؛ تحقق من الدليل قبل إعادة المحاولة.';
    err.classList.remove('hidden');
  } finally {
    accountSaving = false;
    button.disabled = false;
  }
});

let loadedDrafts = [];

function renderDrafts() {
  const q = $('#draft-search').value.trim().toLocaleLowerCase('ar');
  const filtered = loadedDrafts.filter(d => [d.id, d.description, d.currency].some(x => String(x).toLocaleLowerCase('ar').includes(q)));
  $('#draft-search-count').textContent = `${filtered.length} من ${loadedDrafts.length} محمّلة`;
  $('#empty-drafts').classList.toggle('hidden', filtered.length > 0);
  $('#empty-drafts-title').textContent = q ? 'لا توجد نتائج مطابقة' : 'لا توجد مسودات بعد';
  $('#empty-drafts-text').textContent = q ? 'ابحث برقم أو بيان أو عملة ضمن المسودات المحمّلة.' : 'ابدأ بإضافة مسودة قيد جديدة، وستظهر هنا.';
  renderDraftKpis(filtered);
  $('#draft-list').innerHTML = filtered.map(d => `<tr data-help="راجع حالة هذا المستند وتفاصيله قبل الاعتماد."><td>#${escapeHTML(d.id)}</td><td>${escapeHTML(d.description)}</td><td dir="ltr">${escapeHTML(d.date)}</td><td>${escapeHTML(d.currency)}</td><td dir="ltr">${escapeHTML(d.total)}</td><td><span class="status-tag">${d.posted ? 'مرحّل تجريبيًا' : 'مسودة'}</span></td><td><button type="button" class="draft-view-btn" data-draft-id="${escapeHTML(d.id)}" data-help="افتح بنود هذه المسودة وإجمالياتها.">عرض</button></td></tr>`).join('');
}

async function loadDrafts() {
  try {
    const res = await apiFetch('/api/drafts');
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const items = await res.json();
    if (!Array.isArray(items)) throw new Error('رد غير متوقع');
    loadedDrafts = items;
    $('#draft-count').textContent = String(items.length);
    renderDrafts();
  } catch (error) {
    console.error('draft list error:', error);
    loadedDrafts = [];
    $('#draft-list').innerHTML = '';
    $('#empty-drafts').classList.add('hidden');
    $('#draft-search-count').textContent = 'تعذر تحميل القائمة';
    toast('تعذر تحميل المسودات. تحقق من اتصال الخادم.');
    $('#draft-count').textContent = '—';
  }
}

// بطاقة ملخص تفاعلية لحظية لمسودات القيود: تُحسب من البيانات المحمّلة نفسها.
function renderDraftKpis(list) {
  const host = document.getElementById('draft-kpis');
  if (!host) return;
  const rows = Array.isArray(list) ? list : [];
  const total = rows.reduce((sum, d) => sum + Number(d.total || 0), 0);
  const posted = rows.filter(d => d.posted).length;
  const drafts = rows.length - posted;
  const today = new Date().toISOString().slice(0, 10);
  const todays = rows.filter(d => String(d.date || '').slice(0, 10) === today).length;
  const money = v => Number(v || 0).toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  host.innerHTML = `
    <div class="kpi-card"><span>عدد المسودات</span><strong>${drafts}</strong><small>جاهزة للمراجعة</small></div>
    <div class="kpi-card"><span>مرحّلة تجريبيًا</span><strong>${posted}</strong><small>دخلت الأرصدة التجريبية</small></div>
    <div class="kpi-card"><span>إجمالي المبالغ</span><strong dir="ltr">${money(total)}</strong><small>بعملات المسودات</small></div>
    <div class="kpi-card"><span>مسودات اليوم</span><strong>${todays}</strong><small>${today}</small></div>`;
}
window.renderDraftKpis = renderDraftKpis;
