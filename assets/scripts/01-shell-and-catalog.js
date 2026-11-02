'use strict';

const $ = (s, root = document) => root.querySelector(s);

const $$ = (s, root = document) => [...root.querySelectorAll(s)];

const cleanScreenName = s => String(s).replace(/^[\p{Extended_Pictographic}\p{Emoji_Presentation}\uFE0F\u200D\s]+/u, '');

const escapeHTML = s => String(s).replace(/[&<>"']/g, x => ({
  '&': '&amp;',
  '<': '&lt;',
  '>': '&gt;',
  '"': '&quot;',
  "'": '&#39;'
})[x]);

const departments = [{
  id: 'finance',
  title: 'الإدارة المالية',
  desc: 'الحسابات والقيود والأرصدة',
  img: 'ledger',
  help: 'إدارة الحسابات والقيود والتقارير المالية.',
  sections: [['دليل الحسابات', 'هيكل الحسابات وتصنيفاتها.'], ['القيود اليومية', 'إنشاء المسودات والتحقق من توازنها.', 'journals'], ['الصناديق والبنوك', 'متابعة الحركات والتسويات.'], ['العملات والصرف', 'إدارة العملات وأسعار الصرف.'], ['الفترات المالية', 'إعداد الفترات والإقفال.'], ['ميزان المراجعة', 'عرض الأرصدة بعد الترحيل.']]
}, {
  id: 'sales',
  title: 'المبيعات والعملاء',
  desc: 'الفواتير والعملاء والتحصيل',
  img: 'clients',
  help: 'إدارة العملاء والمبيعات والتحصيل.',
  sections: [['العملاء', 'بيانات العملاء وأرصدتهم.'], ['عروض الأسعار', 'إنشاء عروض العملاء.'], ['فواتير المبيعات', 'إدارة الفواتير والمرتجعات.'], ['التحصيل', 'متابعة التحصيل والمستحقات.']]
}, {
  id: 'purchases',
  title: 'المشتريات والموردون',
  desc: 'الشراء والموردون والمدفوعات',
  img: 'invoices',
  help: 'إدارة عمليات الشراء والموردين.',
  sections: [['الموردون', 'ملفات الموردين وحساباتهم.'], ['طلبات الشراء', 'متابعة طلبات الشراء.'], ['فواتير المشتريات', 'إدارة فواتير الموردين.'], ['المدفوعات', 'متابعة مدفوعات الموردين.']]
}, {
  id: 'stock',
  title: 'المخزون والمستودعات',
  desc: 'الأصناف والمخازن والجرد',
  img: 'stock',
  help: 'إدارة الأصناف والمخزون والمستودعات.',
  sections: [['الأصناف', 'تعريف الأصناف ووحدات القياس.'], ['المستودعات', 'توزيع الأرصدة على المستودعات.'], ['الجرد والتسويات', 'مراجعة الكميات وتسوية الفروقات.'], ['التحويلات', 'التحويل بين المستودعات.']]
}, {
  id: 'assets',
  title: 'الأصول الثابتة',
  desc: 'الأصول والإهلاك والاستبعاد',
  img: 'assets',
  help: 'إدارة الأصول الثابتة وحساب الإهلاك.',
  sections: [['سجل الأصول', 'إضافة الأصول وتصنيفها.'], ['الإهلاك', 'إعداد الإهلاك وتتبعه.'], ['الاستبعاد', 'معالجة بيع الأصول أو استبعادها.']]
}, {
  id: 'reports',
  title: 'التقارير المالية',
  desc: 'التقارير والتحليلات المالية',
  img: 'reports',
  help: 'القوائم والتقارير المالية والتحليلات.',
  sections: [['ميزان المراجعة', 'فحص أرصدة الحسابات.'], ['المركز المالي', 'عرض الأصول والالتزامات وحقوق الملكية.'], ['الأرباح والخسائر', 'عرض الإيرادات والمصروفات.'], ['التدفقات النقدية', 'تحليل التدفقات النقدية.']]
}, {
  id: 'hr',
  title: 'الموارد البشرية',
  desc: 'الموظفون والرواتب والإجازات',
  img: 'hr',
  help: 'إدارة الموارد البشرية وشؤون الموظفين.',
  sections: []
}, {
  id: 'social',
  title: 'التجارة الاجتماعية',
  desc: 'القنوات والمحادثات والطلبات',
  emoji: '💬',
  help: 'إدارة قنوات التواصل الاجتماعي والطلبات.',
  sections: [['القنوات', 'إعداد قنوات الاتصال الرسمية.'], ['صندوق المحادثات', 'إدارة رسائل العملاء.'], ['الطلبات', 'متابعة الطلبات من القنوات.'], ['القوالب', 'إدارة قوالب الرسائل.']]
}, {
  id: 'settings',
  title: 'إعدادات النظام',
  desc: 'الشركات والفروع والصلاحيات',
  emoji: '⚙️',
  help: 'إعداد الشركات والمستخدمين وتفضيلات النظام.',
  sections: [['الشركات والفروع', 'إعداد الشركة وفروعها.'], ['المستخدمون والصلاحيات', 'إدارة الحسابات والأدوار.'], ['إعدادات المهندس', 'صيانة النظام والترخيص المحلي.'], ['النسخ والاستعادة', 'سياسات النسخ الاحتياطي.']]
}];

let referenceCatalog = null;

const referenceIdMap = {
  stock: 'inventory',
  settings: 'system'
};

const shortcutChoices = [{
  id: 'journals',
  title: 'القيود اليومية'
}, {
  id: 'finance',
  title: 'الإدارة المالية'
}, {
  id: 'sales',
  title: 'المبيعات'
}, {
  id: 'stock',
  title: 'المخزون'
}, {
  id: 'reports',
  title: 'التقارير'
}, {
  id: 'split',
  title: 'نافذتان متجاورتان'
}];

let shortcuts;

try {
  shortcuts = JSON.parse(localStorage.getItem('smart-shortcuts') || 'null');
} catch {
  shortcuts = null;
}

if (!Array.isArray(shortcuts)) shortcuts = ['journals', 'sales', 'split'];

let timer;

function toast(text) {
  const el = $('#toast');
  el.textContent = text;
  el.classList.remove('hidden');
  clearTimeout(timer);
  timer = setTimeout(() => el.classList.add('hidden'), 3800);
}

function renderCards() {
  const root = $('#department-cards');
  root.innerHTML = departments.map(d => `<button class="department-card" data-open="${d.id}" data-help="${escapeHTML(d.help)}"><div class="card-art ${d.emoji ? 'custom' : ''}">${d.img ? `<img src="/assets/${d.img}.webp" alt="" loading="lazy">` : `<span class="art-emoji" aria-hidden="true"><svg><use href="#${d.id === 'social' ? 'i-chat' : 'i-settings'}"/></svg></span>`}</div><div class="card-body"><div><h3>${d.title}</h3><p>${d.desc}</p></div><span class="card-arrow"><svg><use href="#i-arrow"/></svg></span></div></button>`).join('');
}

function renderShortcuts() {
  const root = $('#shortcuts');
  const found = shortcuts.map(id => shortcutChoices.find(x => x.id === id)).filter(Boolean);
  root.innerHTML = found.length ? found.map(x => `<button class="shortcut-link" data-open="${x.id}" data-help="فتح ${x.title}.">${x.title}</button>`).join('') : '<span class="short-empty">أضف الشاشات الأكثر استخدامًا</span>';
}

function setNav(id) {
  $$('.nav-item').forEach(b => b.classList.toggle('active', b.dataset.nav === id));
}
// Built in this project from visual observations, not from reference source files.

// Built in this project from visual observations, not from reference source files.
const financeTabs = [['journals', 'القيود اليومية'], ['vouchers', 'السندات'], ['accounts', 'دليل الحسابات'], ['screen:fin.intermediate', 'الحسابات الوسيطة'], ['trial', 'ميزان المراجعة'], ['ledger', 'دفتر الأستاذ'], ['screen:fin.bankrec', 'التسوية البنكية'], ['screen:fin.reports', 'التقارير المالية'], ['screen:fin.opening', 'الأرصدة الافتتاحية'], ['screen:fin.cashboxes', 'الصناديق'], ['screen:fin.banks', 'البنوك'], ['screen:fin.adjustments', 'التسويات'], ['screen:fin.taxes', 'الضرائب'], ['screen:fin.taxlinks', 'ربط الضريبة بالقيود'], ['screen:fin.reviewhub', 'المراجعة والاعتماد'], ['periods', 'الإقفال والفترات'], ['screen:fin.dimensions', 'الأبعاد التحليلية'], ['screen:fin.ledgersettings', 'الإعدادات المحاسبية'], ['screen:fin.budgethub', 'الموازنة'], ['screen:fin.plans', 'المخططات والعمولات'], ['screen:fin.scanner', 'قراءة الفواتير المصورة'], ['screen:fin.requests', 'طلبات القيود والسندات'], ['screen:fin.rules', 'قواعد الترحيل'], ['screen:fin.parties', 'الأطراف'], ['screen:fin.checks', 'الشيكات'], ['screen:fin.assets', 'الأصول الثابتة'], ['screen:fin.payroll', 'الرواتب'], ['screen:fin.branches', 'الفروع'], ['screen:fin.audit', 'سجل التدقيق'], ['screen:fin.numbering', 'ترقيم المستندات'], ['screen:fin.departments', 'الهيكل الإداري'], ['screen:fin.fx', 'العملات وأسعار الصرف'], ['screen:fin.setup', 'بيانات الدول'], ['screen:fin.currencies', 'العملات'], ['screen:fin.branchgroups', 'مجموعات الفروع'], ['screen:fin.taxreturns', 'الإقرارات الضريبية'], ['screen:fin.notes', 'الإشعارات المدينة والدائنة'], ['screen:fin.zakat', 'دورة استحقاق الزكاة'], ['screen:fin.budgets', 'الموازنة السنوية'], ['screen:fin.budgetedit', 'تعديل الموازنة'], ['screen:fin.deptbudgets', 'موازنات الإدارات'], ['screen:fin.closehub', 'الإقفال الموحّد'], ['screen:fin.fxconvert', 'صرف العملة']];

// مؤشرات المالية أرقام حقيقية من قاعدة البيانات، لا شرطات ولا نصوص «غير محسوب بعد».
async function updateFinanceIndicators() {
  const set = (id, value, note) => {
    const el = document.getElementById(id);
    if (el) el.textContent = value;
    const nt = document.getElementById(id + '-note');
    if (nt && note) nt.textContent = note;
  };
  try {
    // المسار المالي يحتاج شركة مسندة؛ وإن تعذّر نرجع إلى قائمة المسودات المتاحة دائمًا.
    let rows = [];
    try {
      const company = window.FinanceApi && (await window.FinanceApi.ensureCompany());
      const res = await window.apiFetch(`/api/finance/journals?company_id=${company}`);
      if (!res.ok) throw new Error('HTTP ' + res.status);
      const items = await res.json();
      rows = Array.isArray(items) ? items : [];
    } catch (_) {
      const res = await window.apiFetch('/api/drafts');
      if (!res.ok) throw new Error('HTTP ' + res.status);
      const items = await res.json();
      rows = Array.isArray(items) ? items : [];
    }
    const posted = rows.filter(j => j.posted).length;
    const drafts = rows.length - posted;
    const month = new Date().toISOString().slice(0, 7);
    const day = new Date().toISOString().slice(0, 10);
    const inMonth = rows.filter(j => String(j.date || '').slice(0, 7) === month).length;
    const today = rows.filter(j => String(j.date || '').slice(0, 10) === day).length;
    const ratio = rows.length ? Math.round((posted / rows.length) * 100) : 0;
    set('fi-month', String(inMonth), `من أصل ${rows.length} قيدًا محفوظًا`);
    set('fi-pending', String(drafts), drafts ? 'مسودة تحتاج ترحيلًا' : 'لا مسودات معلّقة');
    set('fi-ratio', ratio + '٪', `${posted} مرحّل · ${drafts} مسودة`);
    set('fi-today', String(today), day);
  } catch (error) {
    ['fi-month', 'fi-pending', 'fi-ratio', 'fi-today'].forEach(id => set(id, '—', 'تعذّر القراءة'));
  }
}
window.updateFinanceIndicators = updateFinanceIndicators;

// ينقل كتل القيود (المؤشرات + مساحة القيود) إلى ما قبل لوحة الشاشة، فتصبح أول ما يُرى.
function orderFinanceBlocks() {
  const hub = document.getElementById('finance-hub');
  if (!hub || !hub.parentNode) return;
  const parent = hub.parentNode;
  ['draft-kpis', 'view-journals'].forEach((id, index) => {
    const node = document.getElementById(id);
    if (node && node.parentNode !== parent) parent.insertBefore(node, hub);
    else if (node) parent.insertBefore(node, hub);
  });
}
window.orderFinanceBlocks = orderFinanceBlocks;

function initFinanceWorkbench() {
  const panel = document.createElement('section');
  panel.id = 'finance-workbench';
  panel.className = 'finance-workbench hidden';
  panel.setAttribute('aria-label', 'مساحة الإدارة المالية');
  panel.innerHTML = `<div class="finance-crumb"><button type="button" data-open="home">الرئيسية</button><span>‹</span><b>المالية</b></div><header class="finance-room-header"><div class="finance-room-main"><div class="finance-room-art"><img src="/assets/ledger.webp" alt=""></div><div><h2>المالية</h2><p>الحسابات والقيود والسندات والتقارير المالية</p><div class="finance-room-links"><button type="button" data-open="journals">قيود ومسودات</button><button type="button" data-open="trial">ميزان المراجعة</button></div></div></div><div class="finance-room-controls"><button type="button" data-open="finance-catalog" data-help="استعراض فهرس الشاشات المالية وحالة كل شاشة.">كل الشاشات</button><button type="button" data-open="home">العودة ←</button><button type="button" id="finance-chat-toggle" class="finance-chat-toggle" aria-expanded="false" data-help="فتح أو إغلاق لوحة المساعد الإرشادي.">✦ المساعد</button></div></header><div class="finance-indicators" aria-label="مؤشرات المالية"><div><span>قيود هذا الشهر</span><strong id="fi-month">…</strong><small id="fi-month-note">جارٍ الحساب</small></div><div><span>بانتظار الترحيل</span><strong id="fi-pending">…</strong><small id="fi-pending-note">جارٍ الحساب</small></div><div><span>نسبة الترحيل</span><strong id="fi-ratio">…</strong><small id="fi-ratio-note">جارٍ الحساب</small></div><div><span>نشاط اليوم</span><strong id="fi-today">…</strong><small id="fi-today-note">جارٍ الحساب</small></div></div><nav class="finance-tab-bar" aria-label="أقسام الإدارة المالية">${financeTabs.map(([id, label]) => `<button type="button" data-open="${id}" data-finance-tab="${id}" data-help="${['journals', 'vouchers', 'accounts', 'trial', 'ledger', 'periods'].includes(id) ? 'فتح الوظيفة المتاحة جزئيًا' : 'فتح الشاشة المالية'}.">${label}</button>`).join('')}</nav><div id="finance-hub" class="finance-hub"></div><div class="finance-screen-heading"><strong id="finance-screen-label">القيود اليومية</strong></div>`;
  $('#view-home').before(panel);
  // ترتيب الشاشة المالية: قسم القيود اليومية أولًا، ثم لوحة الشاشة (تقارير/جداول) أسفله.
  // قاعدة ثابتة في كل الشاشات: العمل (القيود) أعلى، والعرض (التقرير/الجدول) أدناه.
  orderFinanceBlocks();
  updateFinanceIndicators();
  $('#finance-chat-toggle').addEventListener('click', e => {
    const open = document.body.classList.toggle('finance-chat-open');
    e.currentTarget.setAttribute('aria-expanded', String(open));
  });
}

const hubMap = {
    'journals': 'journals',
    'screen:fin.intermediate': 'intermediate', 'screen:fin.adjustments': 'adjustments',
    'trial': 'reports', 'screen:fin.reports': 'reports',
    'screen:fin.budgethub': 'budgets', 'screen:fin.budgets': 'budgets',
    'screen:fin.currencies': 'currencies', 'screen:fin.branchgroups': 'branchgroups',
    'screen:fin.taxreturns': 'taxreturns', 'screen:fin.notes': 'notes', 'screen:fin.zakat': 'zakat',
    'screen:fin.budgetedit': 'budgetedit', 'screen:fin.deptbudgets': 'deptbudgets',
    'screen:fin.closehub': 'closehub', 'screen:fin.fxconvert': 'fxconvert', 'screen:fin.plans': 'plans',
    'screen:fin.opening': 'opening', 'screen:fin.cashboxes': 'funds',
    'screen:fin.banks': 'funds', 'screen:fin.bankrec': 'bankrec',
    'screen:fin.dimensions': 'dimensions', 'screen:fin.taxes': 'taxes', 'screen:fin.taxlinks': 'taxlinks',
    'screen:fin.requests': 'requests', 'screen:fin.taxlinks': 'taxlinks',
    'screen:fin.requests': 'requests',
    'screen:fin.setup': 'countries', 'screen:fin.numbering': 'numbering', 'screen:fin.departments': 'departments',
    'screen:fin.reviewhub': 'docreview', 'screen:fin.rules': 'approval',
    'screen:fin.parties': 'parties', 'screen:fin.checks': 'checks',
    'screen:fin.assets': 'assets', 'screen:fin.payroll': 'payroll',
    'screen:fin.branches': 'branches', 'screen:fin.audit': 'audit',
    'screen:fin.fx': 'fx', 'screen:fin.ledgersettings': 'ledgersettings',
    'screen:fin.scanner': 'scanner', 'periods': 'closing',
    'finance': 'reports', 'finance-catalog': 'reports'
  };

function setFinanceContext(id) {
  const enabled = ['finance', 'finance-catalog', 'journals', 'vouchers', 'accounts', 'trial', 'ledger', 'periods'].includes(id) || id.startsWith('screen:fin.');
  document.body.classList.toggle('finance-mode', enabled);
  if (!enabled) {
    document.body.classList.remove('finance-chat-open');
    $('#finance-chat-toggle')?.setAttribute('aria-expanded', 'false');
  }
  const shell = $('#finance-workbench');
  if (!shell) return;
  shell.classList.toggle('hidden', !enabled);
  if (!enabled) return;
  const aliases = {
    'screen:fin.journal': 'journals',
    'screen:fin.vouchers': 'vouchers',
    'screen:fin.coa': 'accounts',
    'screen:fin.trial': 'trial',
    'screen:fin.ledger': 'ledger',
    'screen:fin.periods': 'periods'
  };
  const active = aliases[id] || id;
  const entry = financeTabs.find(t => t[0] === active);
  const available = ['journals', 'vouchers', 'accounts', 'trial', 'ledger', 'periods'].includes(active);
  $('#finance-screen-label').textContent = entry ? entry[1] : id === 'finance-catalog' ? 'فهرس الشاشات المالية' : 'المالية';
  const statusEl = $('#finance-screen-status');
  if (statusEl) statusEl.classList.add('hidden');

  // ربط تبويبات شاشة المالية الأصلية بلوحة المالية المدمجة.

  // كل شاشة مالية تُعرض وحدها: نُخفي لوحات العرض القديمة (مسودات/سندات/دليل/أستاذ).
  if (hubMap[active]) {
    ['view-journals', 'view-vouchers', 'view-accounts', 'view-ledger', 'view-periods', 'view-split', 'view-detail']
      .forEach(viewId => {
        const node = document.getElementById(viewId);
        if (node) node.classList.add('hidden');
      });
  }
  // مؤشرات المسودات وقسم القيود يخصّان شاشة القيود وحدها؛ بقية الشاشات لا تُظهرهما.
  const journalsScreen = active === 'journals';
  ['draft-kpis', 'view-journals'].forEach(nodeId => {
    const node = document.getElementById(nodeId);
    if (node) node.classList.toggle('hidden', !journalsScreen);
  });
  const hub = $('#finance-hub');
  if (window.FinanceHub && hubMap[active]) {
    if (hub) hub.classList.remove('hidden');
    try { window.FinanceHub.show(hubMap[active]); } catch (err) { console.warn('finance hub', err); }
  } else if (hub) {
    // شاشة لها عرضها الخاص (دليل الحسابات، دفتر الأستاذ، السندات…): لا نُبقي لوحة قديمة معلّقة.
    hub.classList.add('hidden');
  }
  updateFinanceIndicators();
  $$('#finance-workbench [data-finance-tab]').forEach(button => {
    const on = button.dataset.financeTab === active;
    button.classList.toggle('active', on);
    if (on) button.setAttribute('aria-current', 'page');else button.removeAttribute('aria-current');
  });
}

// خريطة ربط تبويبات شاشة المالية بوحدات اللوحة المنفّذة.


// تصغير لوحة المحادثة إلى أيقونة ثابتة تظهر في كل الشاشات.
(function initAssistantDock() {
  const panel = document.querySelector('.assistant-panel');
  if (!panel) return;
  const dock = document.createElement('button');
  dock.id = 'assistant-dock-toggle';
  dock.type = 'button';
  dock.setAttribute('aria-label', 'فتح المساعد الذكي');
  dock.textContent = '✦';
  document.body.appendChild(dock);
  const collapse = () => {
    document.body.classList.add('assistant-collapsed');
    document.querySelector('#assistant-minimize')?.setAttribute('aria-expanded', 'false');
  };
  const expand = () => {
    document.body.classList.remove('assistant-collapsed');
    document.querySelector('#assistant-minimize')?.setAttribute('aria-expanded', 'true');
  };
  dock.addEventListener('click', expand);
  panel.addEventListener('click', ev => {
    if (document.body.classList.contains('assistant-collapsed')) { ev.preventDefault(); expand(); }
  });
  document.querySelector('#assistant-minimize')?.addEventListener('click', collapse);
})();

initFinanceWorkbench();

function baseView(id, title, desc, breadcrumb) {
  ['home', 'detail', 'journals', 'accounts', 'ledger', 'periods', 'vouchers', 'split'].forEach(x => $('#view-' + x)?.classList.toggle('hidden', x !== id));
  $('#section-title').textContent = title;
  $('#section-subtitle').textContent = desc;
  $('#breadcrumb').textContent = breadcrumb;
  if (id === 'home') {
    const count = document.createElement('span');
    count.className = 'count';
    count.id = 'department-count';
    count.textContent = String(departments.length);
    $('#section-title').append(' ', count);
  }
  $('#all-departments').classList.toggle('hidden', id === 'home');
}

function sectionCard(s) {
  return `<article class="detail-card" data-help="${escapeHTML(s[1])}"><h3>${escapeHTML(s[0])}</h3><p>${escapeHTML(s[1])}</p>${s[2] ? `<span class="state-label ready">متاح كمسودة</span><br><button class="ghost-btn" data-open="${s[2]}">فتح الشاشة ←</button>` : '<span class="state-label">قيد التطوير</span>'}</article>`;
}

function referenceScreenCard(screen) {
  const implemented = ['fin.journal', 'fin.coa', 'fin.ledger', 'fin.trial', 'fin.periods', 'fin.vouchers'].includes(screen.perm_id);
  const open = screen.perm_id === 'fin.journal' ? 'journals' : screen.perm_id === 'fin.coa' ? 'accounts' : screen.perm_id === 'fin.ledger' ? 'ledger' : screen.perm_id === 'fin.trial' ? 'trial' : screen.perm_id === 'fin.periods' ? 'periods' : 'screen:' + screen.perm_id;
  const pinId = 'screen:' + screen.perm_id;
  const pinned = shortcuts.includes(pinId);
  return `<article class="detail-card catalog-card" data-screen-name="${escapeHTML(cleanScreenName(screen.name))}" data-help="${escapeHTML(cleanScreenName(screen.name))}: ${implemented ? 'وظيفة تجريبية جزئية متاحة.' : 'مدرج في فهرس رست 8؛ لم تُنفّذ هذه الشاشة في النسخة الحالية.'}">
   <div class="catalog-card-top"><span class="screen-icon" aria-hidden="true"><svg><use href="#i-file"/></svg></span><button class="pin-screen ${pinned ? 'pinned' : ''}" data-pin="${escapeHTML(pinId)}" aria-label="${pinned ? 'إزالة من الاختصارات' : 'إضافة للاختصارات'}" data-help="${pinned ? 'إزالة الشاشة من شريط الاختصارات.' : 'إضافة الشاشة إلى شريط الاختصارات.'}"><svg><use href="#i-star"/></svg></button></div>
   <h3>${escapeHTML(cleanScreenName(screen.name))}</h3><p class="catalog-id" dir="ltr">${escapeHTML(screen.perm_id)}</p>
   <div class="catalog-card-bottom"><span class="state-label ${implemented ? 'ready' : ''}">${implemented ? 'متاح جزئيًا' : 'غير منفذة بعد'}</span><button class="screen-open" data-open="${escapeHTML(open)}">${implemented ? 'فتح' : 'التفاصيل'} ←</button></div>
 </article>`;
}

function showCatalogDepartment(d) {
  const refId = referenceIdMap[d.id] || d.id;
  const screens = referenceCatalog.screens.filter(s => s.dept_id === refId);
  if (!screens.length) return null;
  const byId = new Map(screens.map(s => [s.perm_id, s]));
  const used = new Set();
  let groups = [];
  const refs = referenceCatalog.sections.filter(g => g.dept_id === refId).sort((a, b) => a.order - b.order);
  for (const g of refs) {
    const list = g.screens.map(id => byId.get(id)).filter(x => x && !used.has(x.perm_id));
    list.forEach(x => used.add(x.perm_id));
    const links = g.screens.map(id => referenceCatalog.screens.find(x => x.perm_id === id)).filter(x => x && x.dept_id !== refId);
    if (list.length || links.length) groups.push({
      name: g.name,
      items: list,
      links
    });
  }
  const other = screens.filter(s => !used.has(s.perm_id));
  if (other.length) groups.push({
    name: groups.length ? 'شاشات أخرى' : 'الشاشات والتبويبات',
    items: other
  });
  const chunks = groups.map((g, i) => `<details class="catalog-group" ${i === 0 || groups.length === 1 ? 'open' : ''}><summary><span>${escapeHTML(g.name)}</span><span class="catalog-count">${g.items.length} شاشة</span></summary><div class="detail-cards">${g.items.map(referenceScreenCard).join('')}</div>${g.links?.length ? `<div class="catalog-related"><span>ارتباطات بإدارات أخرى:</span>${g.links.map(x => `<button data-open="screen:${escapeHTML(x.perm_id)}" data-help="${escapeHTML(cleanScreenName(x.name))}: شاشة مرتبطة من إدارة أخرى.">${escapeHTML(cleanScreenName(x.name))} ↗</button>`).join('')}</div>` : ''}</details>`).join('');
  return `<div class="detail-banner catalog-banner"><div><span class="detail-kicker">الإدارات / ${escapeHTML(d.title)}</span><h2>${escapeHTML(d.title)}</h2><p>فهرس مستلهم من مشروع رست 8 • ${screens.length} شاشة مسجلة. لا تعني إضافتها تشغيل وظائفها.</p></div><button class="ghost-btn" data-open="split">▥ فتح عرض متجاور</button></div><div class="catalog-tools"><label for="catalog-search">البحث في الشاشات</label><input id="catalog-search" type="search" placeholder="اكتب اسم الشاشة..." data-help="تصفية شاشات هذه الإدارة حسب اسمها."><span id="catalog-results">${screens.length} شاشة</span></div><div id="catalog-groups">${chunks}</div><div id="catalog-empty" class="empty hidden">لا توجد شاشة مطابقة لبحثك.</div>`;
}

function bindCatalogSearch() {
  const inp = $('#catalog-search');
  if (!inp) return;
  inp.addEventListener('input', () => {
    const q = inp.value.trim().toLocaleLowerCase('ar');
    let count = 0;
    $$('#catalog-groups .catalog-group').forEach(g => {
      let visible = 0;
      $$('.catalog-card', g).forEach(card => {
        const ok = card.dataset.screenName.toLocaleLowerCase('ar').includes(q);
        card.classList.toggle('hidden', !ok);
        if (ok) {
          visible++;
          count++;
        }
      });
      g.classList.toggle('hidden', visible === 0);
      if (q && visible) g.open = true;
    });
    $('#catalog-results').textContent = `${count} شاشة`;
    $('#catalog-empty').classList.toggle('hidden', count !== 0);
  });
}

function showScreenDetails(id) {
  const screen = referenceCatalog?.screens.find(s => s.perm_id === id);
  if (!screen) {
    // شاشات المالية تُفتح داخل شاشة المالية نفسها؛ لا داعي لتنبيه «غير موجودة في الفهرس».
    if (!String(id).startsWith('screen:fin.')) toast('لم تُعثر الشاشة في الفهرس.');
    return;
  }
  const d = departments.find(x => (referenceIdMap[x.id] || x.id) === screen.dept_id);
  if (!d) return;
  baseView('detail', screen.name, 'تفاصيل الشاشة من فهرس رست 8', `${d.title} / ${screen.name}`);
  setNav('departments');
  const groups = referenceCatalog.sections.filter(g => g.screens.includes(id));
  $('#view-detail').innerHTML = `<div class="detail-banner"><div><span class="detail-kicker">${escapeHTML(d.title)} / ${escapeHTML(screen.perm_id)}</span><h2>${escapeHTML(cleanScreenName(screen.name))}</h2><p>هذه الشاشة مدرجة في فهرس المشروع المرجعي، ووظيفتها لم تُنفذ في النسخة الحالية.</p></div><button class="ghost-btn" data-open="${d.id === 'finance' ? 'finance-catalog' : d.id}">العودة للإدارة</button></div><div class="screen-info"><div class="screen-info-icon"><svg><use href="#i-file"/></svg></div><div><h3>حالة التنفيذ</h3><p>فهرس وتنقل فقط، دون ربط ببيانات أو تنفيذ عمليات مالية. لا تُعرض حقول أو أرصدة غير حقيقية.</p><p>${groups.length ? 'القسم المرجعي: ' + escapeHTML(groups.map(g => g.name).join('، ')) : 'لم يحدد الفهرس المرجعي قسمًا إضافيًا لهذه الشاشة.'}</p>${screen.initial_tab ? `<p>التبويب الأولي في المرجع: <code dir="ltr">${escapeHTML(screen.initial_tab)}</code> (معرّف مرجعي فقط).</p>` : ''}<button class="ghost-btn" data-pin="screen:${escapeHTML(screen.perm_id)}">☆ إضافة أو إزالة الاختصار</button></div></div>`;
}

// Modal dismissals share cancellation, pending-operation and unsaved-change guards.
function requestDialogDismiss(dialog) {
  if (typeof dialog.requestClose === 'function') {
    dialog.requestClose();
  } else if (dialog.dispatchEvent(new Event('cancel', { cancelable: true }))) {
    dialog.close();
  }
}

function dialogBusy(dialog) {
  return (dialog.id === 'voucher-dialog' && voucherSaving) ||
    (dialog.id === 'draft-dialog' && saving) ||
    (dialog.id === 'rename-dialog' && renameSaving) ||
    (dialog.id === 'account-dialog' && accountSaving) ||
    (dialog.id === 'draft-details-dialog' && $('#post-draft').disabled);
}

function outsideDialog(event, dialog) {
  const rect = dialog.getBoundingClientRect();
  return event.target === dialog && (event.clientX < rect.left || event.clientX > rect.right ||
    event.clientY < rect.top || event.clientY > rect.bottom);
}

let outsidePointer = null;
document.addEventListener('pointerdown', event => {
  const dialog = event.target.closest('dialog[open]');
  outsidePointer = dialog && event.button === 0 && outsideDialog(event, dialog) ? dialog : null;
}, true);
document.addEventListener('click', event => {
  const dialog = event.target.closest('dialog[open]');
  if (dialog && outsidePointer === dialog && outsideDialog(event, dialog)) requestDialogDismiss(dialog);
  outsidePointer = null;
}, true);
for (const type of ['input', 'change']) document.addEventListener(type, event => {
  const dialog = event.target.closest('dialog[open]');
  if (dialog) dialog.dataset.dirty = 'true';
}, true);
document.addEventListener('click', event => {
  if (event.target.closest('#add-line, .remove-line')) {
    const dialog = event.target.closest('dialog[open]');
    if (dialog) dialog.dataset.dirty = 'true';
  }
}, true);
document.addEventListener('cancel', event => {
  const dialog = event.target;
  if (!(dialog instanceof HTMLDialogElement)) return;
  if (dialogBusy(dialog)) {
    event.preventDefault();
    toast('انتظر انتهاء العملية قبل إغلاق النافذة.');
  } else if (dialog.dataset.dirty === 'true') {
    const message = dialog.id === 'voucher-dialog' && voucherFrozen
      ? 'نتيجة محاولة حفظ السند غير مؤكدة. عند الإغلاق لن يُحتفظ بمعرّف المحاولة؛ افحص سجل السندات قبل إدخالها مجددًا. هل تريد الإغلاق؟'
      : 'توجد تغييرات غير محفوظة. هل تريد إغلاق النافذة وفقد هذه التغييرات؟';
    if (!confirm(message)) event.preventDefault();
  }
}, true);
document.addEventListener('close', event => {
  if (event.target instanceof HTMLDialogElement) delete event.target.dataset.dirty;
}, true);
