'use strict';

function configureLedger() {
  const trial = $('#ledger-mode').value === 'trial';
  $('#ledger-title').textContent = trial ? 'ميزان المراجعة التجريبي' : 'حركات الأستاذ التجريبية';
  if (!$('#view-ledger').classList.contains('hidden')) {
    $('#section-title').textContent = trial ? 'ميزان المراجعة' : 'حركات الأستاذ';
    $('#breadcrumb').textContent = trial ? 'المالية / ميزان المراجعة' : 'المالية / الأستاذ';
  }
  $('#ledger-account-label').classList.toggle('hidden', trial);
  $('#ledger-from-label').classList.toggle('hidden', trial);
  $('#ledger-note').textContent = trial ? 'الأرصدة من أول قيد حتى التاريخ المحدد، بعملة واحدة؛ لا يتضمن ميزانًا افتتاحيًا أو تحويل عملات.' : $('#ledger-account').value ? 'الرصيد السابق والجاري محسوبان من القيود المرحّلة فقط؛ لا يتضمنان رصيدًا افتتاحيًا مستقلًا أو تحويل عملات.' : 'حركات فقط للفترة المختارة؛ اختر حسابًا لعرض الرصيد السابق والجاري من القيود المرحّلة.';
}

$('#ledger-mode').addEventListener('change', configureLedger);

$('#ledger-load').addEventListener('click', loadLedger);

let ledgerRequest = 0,
  ledgerRenderedMode = null;

async function loadLedger() {
  const request = ++ledgerRequest,
    mode = $('#ledger-mode').value,
    currency = $('#ledger-currency').value;
  // توحيد شكل الاستجابة: بعض المسارات تُعيد accounts وأخرى rows — نتعامل مع الاثنين.
  const normalizeTrial = data => {
    const src = Array.isArray(data.rows) ? data.rows
      : Array.isArray(data.accounts) ? data.accounts : [];
    return src.map(r => ({
      code: r.code || '',
      name: r.name || '',
      debit_turnover: r.debit_turnover ?? r.debit ?? '0',
      credit_turnover: r.credit_turnover ?? r.credit ?? '0',
      debit_balance: r.debit_balance ?? r.balance ?? '0',
      credit_balance: r.credit_balance ?? '0'
    }));
  };
  configureLedger();
  const error = $('#ledger-error'),
    body = $('#ledger-body');
  error.classList.add('hidden');
  body.innerHTML = '';
  $('#ledger-summary').textContent = 'جارٍ تحميل التقرير والتحقق منه…';
  try {
    const ar = await apiFetch('/api/accounts');
    if (!ar.ok) throw new Error('تعذر تحميل قائمة الحسابات؛ لم يُعرض التقرير.');
    const accounts = await ar.json();
    if (request !== ledgerRequest) return;
    const select = $('#ledger-account'),
      previous = select.value;
    select.innerHTML = '<option value="">كل الحسابات</option>' + accounts.filter(a => a.kind === 'detail' && a.currency === currency).map(a => `<option value="${escapeHTML(a.id)}">${escapeHTML(a.code)} — ${escapeHTML(a.name)}</option>`).join('');
    if ([...select.options].some(o => o.value === previous)) select.value = previous;
    const params = new URLSearchParams({
      currency
    });
    const to = $('#ledger-to').value;
    if (to) params.set('to', to);
    const account = mode === 'entries' ? select.value : '';
    if (mode === 'entries') {
      const from = $('#ledger-from').value;
      if (from) params.set('from', from);
    }
    const endpoint = account ? 'accounts/' + encodeURIComponent(account) : mode;
    const res = await apiFetch('/api/ledger/' + endpoint + '?' + params);
    const data = await res.json();
    if (request !== ledgerRequest) return;
    if (!res.ok) throw new Error(data.error || 'تعذر تحميل التقرير.');
    const layout = mode + (account ? '-account' : ''),
      layoutChanged = ledgerRenderedMode !== layout;
    if (layoutChanged) {
      const table = $('#ledger-head').closest('table');
      table.querySelector('colgroup')?.remove();
      table.classList.remove('resizable-table');
      delete table.dataset.resizable;
      table.style.width = '';
      ledgerRenderedMode = layout;
    }
    configureLedger();
    if (mode === 'trial') {
      const rows = normalizeTrial(data);
      if (layoutChanged) $('#ledger-head').innerHTML = '<tr><th>رقم الحساب</th><th>الحساب</th><th>حركة مدين</th><th>حركة دائن</th><th>رصيد مدين</th><th>رصيد دائن</th></tr>';
      body.innerHTML = rows.map(r => `<tr><td dir="ltr">${escapeHTML(r.code)}</td><td>${escapeHTML(r.name)}</td><td dir="ltr">${escapeHTML(r.debit_turnover)}</td><td dir="ltr">${escapeHTML(r.credit_turnover)}</td><td dir="ltr">${escapeHTML(r.debit_balance)}</td><td dir="ltr">${escapeHTML(r.credit_balance)}</td></tr>`).join('');
      $('#ledger-summary').textContent = `${rows.length} حساب • مجموع المدين ${data.debit_total ?? '—'} • مجموع الدائن ${data.credit_total ?? '—'} ${currency}`;
    } else if (account) {
      if (layoutChanged) $('#ledger-head').innerHTML = '<tr><th>القيد</th><th>التاريخ</th><th>البيان</th><th>المرحل</th><th>الاسم عند الحفظ</th><th>مدين</th><th>دائن</th><th>الرصيد مدين</th><th>الرصيد دائن</th></tr>';
      body.innerHTML = data.rows.map(({
        movement: r,
        running_debit: d,
        running_credit: c
      }) => `<tr><td>#${escapeHTML(r.entry_id)}</td><td dir="ltr">${escapeHTML(r.date)}</td><td>${escapeHTML(r.description)}</td><td>${escapeHTML(r.posted_by || 'غير معروف')}</td><td>${escapeHTML(r.account_snapshot)}</td><td dir="ltr">${escapeHTML(r.debit || '—')}</td><td dir="ltr">${escapeHTML(r.credit || '—')}</td><td dir="ltr">${escapeHTML(d)}</td><td dir="ltr">${escapeHTML(c)}</td></tr>`).join('');
      $('#ledger-summary').textContent = `${data.rows.length} حركة • السابق مدين ${data.opening_debit} دائن ${data.opening_credit} • النهائي مدين ${data.closing_debit} دائن ${data.closing_credit} ${currency}`;
    } else {
      if (layoutChanged) $('#ledger-head').innerHTML = '<tr><th>القيد</th><th>التاريخ</th><th>البيان</th><th>المرحل</th><th>الحساب الحالي</th><th>الاسم عند الحفظ</th><th>مدين</th><th>دائن</th></tr>';
      body.innerHTML = data.movements.map(r => `<tr><td>#${escapeHTML(r.entry_id)}</td><td dir="ltr">${escapeHTML(r.date)}</td><td>${escapeHTML(r.description)}</td><td>${escapeHTML(r.posted_by || 'غير معروف')}</td><td>${escapeHTML(r.account_code)} — ${escapeHTML(r.account_name)}</td><td>${escapeHTML(r.account_snapshot)}</td><td dir="ltr">${escapeHTML(r.debit || '—')}</td><td dir="ltr">${escapeHTML(r.credit || '—')}</td></tr>`).join('');
      $('#ledger-summary').textContent = data.movements.length ? `${data.movements.length} حركة • ${currency}` : 'لا توجد حركات مطابقة.';
    }
  } catch (e) {
    if (request !== ledgerRequest) return;
    body.innerHTML = '';
    $('#ledger-summary').textContent = '';
    error.textContent = e.message || 'تعذر الاتصال بالخادم.';
    error.classList.remove('hidden');
  }
}
// Financial periods and posting boundary. These controls remain disabled without the test switch.

// Financial periods and posting boundary. These controls remain disabled without the test switch.
let periodRows = [],
  periodRequest = 0,
  periodBusy = false;

function periodError(message) {
  const el = $('#period-error');
  el.textContent = message;
  el.classList.toggle('hidden', !message);
}

async function loadPeriods() {
  const request = ++periodRequest;
  await ledgerConfigPromise;
  if (request !== periodRequest) return;
  $('#period-history').classList.add('hidden');
  periodError('');
  $('#period-empty').textContent = 'جارٍ تحميل الفترات…';
  $('#period-empty').classList.remove('hidden');
  $('#period-list').innerHTML = '';
  try {
    const r = await apiFetch('/api/periods');
    const data = await r.json();
    if (request !== periodRequest) return;
    if (!r.ok || !Array.isArray(data)) throw new Error(data.error || 'تعذر تحميل الفترات.');
    periodRows = data;
    $('#period-empty').classList.toggle('hidden', data.length > 0);
    $('#period-empty').textContent = 'لا توجد فترات مسجلة.';
    $('#period-list').innerHTML = data.map(p => `<tr><td>${escapeHTML(p.name)}</td><td dir="ltr">${escapeHTML(p.start_date)}</td><td dir="ltr">${escapeHTML(p.end_date)}</td><td>${p.status === 'open' ? 'مفتوحة' : 'مغلقة'}</td><td>${escapeHTML(p.posted)}</td><td>${escapeHTML(p.unposted)}</td><td>${escapeHTML(p.unassigned)}</td><td><button type="button" class="ghost-btn period-transition" data-id="${escapeHTML(p.id)}" ${ledgerEnabled ? '' : 'disabled'} data-help="${p.status === 'open' ? 'منع الترحيل في هذه الفترة بعد تأكيدك.' : 'إعادة فتح الفترة بعد تأكيدك.'}">${p.status === 'open' ? 'إغلاق' : 'إعادة فتح'}</button> <button type="button" class="ghost-btn period-events" data-id="${escapeHTML(p.id)}" data-help="عرض أحداث إنشاء وإغلاق وإعادة فتح الفترة؛ لا يحدد السجل هوية المنفذ.">السجل</button></td></tr>`).join('');
  } catch (e) {
    if (request !== periodRequest) return;
    $('#period-empty').classList.add('hidden');
    periodError(e.message || 'تعذر الاتصال بالخادم.');
  }
}

$('#period-refresh').addEventListener('click', loadPeriods);

$('#goto-periods').addEventListener('click', () => {
  if ($('#post-draft').disabled) {
    toast('انتظر انتهاء العملية قبل إغلاق النافذة.');
    return;
  }
  $('#draft-details-dialog').close();
  navigate('periods');
});

$('#period-form').addEventListener('submit', async e => {
  e.preventDefault();
  if (periodBusy) return;
  const form = e.currentTarget;
  if (!ledgerEnabled) {
    periodError('إدارة الفترات الاختبارية معطلة على الخادم.');
    return;
  }
  periodBusy = true;
  const button = form.querySelector('button[type=submit]');
  button.disabled = true;
  periodError('');
  try {
    const payload = Object.fromEntries(new FormData(form));
    const r = await apiFetch('/api/periods', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json'
      },
      body: JSON.stringify(payload)
    });
    const data = await r.json();
    if (!r.ok) throw new Error(data.error || 'تعذر إنشاء الفترة.');
    form.reset();
    await loadPeriods();
    toast('أُضيفت الفترة وهي مفتوحة.');
  } catch (e) {
    periodError(e.message || 'تعذر الاتصال؛ راجع القائمة قبل الإعادة.');
  } finally {
    periodBusy = false;
    button.disabled = false;
  }
});

$('#period-list').addEventListener('click', async e => {
  const transition = e.target.closest('.period-transition'),
    history = e.target.closest('.period-events');
  if (periodBusy) return;
  if (history) {
    const id = history.dataset.id;
    try {
      const r = await apiFetch('/api/periods/' + encodeURIComponent(id) + '/events');
      const data = await r.json();
      if (!r.ok) throw new Error(data.error || 'تعذر تحميل السجل.');
      const names = {
        created: 'إنشاء',
        closed: 'إغلاق',
        reopened: 'إعادة فتح'
      };
      const panel = $('#period-history');
      panel.classList.remove('hidden');
      panel.innerHTML = '<h3>سجل حالة الفترة</h3><p>سجل تقني: الأحداث الجديدة مرتبطة بهوية محلية، والقديمة قد تكون مجهولة؛ ليس سجل تدقيق معتمدًا.</p><ul>' + data.map(x => `<li>${escapeHTML(names[x.event] || x.event)} — المنفذ: ${escapeHTML(x.actor || 'غير معروف')} — <span dir="ltr">${escapeHTML(x.occurred_at)} UTC</span>${x.unposted_count !== null ? ' — مسودات وقت الإغلاق: ' + escapeHTML(x.unposted_count) : ''}</li>`).join('') + '</ul>';
    } catch (err) {
      periodError(err.message || 'تعذر الاتصال.');
    }
    ;
    return;
  }
  if (!transition) return;
  const p = periodRows.find(x => String(x.id) === transition.dataset.id);
  if (!p || !ledgerEnabled) return;
  const closing = p.status === 'open';
  if (!confirm(closing ? 'إغلاق الفترة يمنع الترحيل فيها؛ لا ينفّذ إقفال النتائج. هل تؤكد؟' : 'إعادة فتح الفترة تسمح بالترحيل مجددًا. هل تؤكد؟')) return;
  let allow = false;
  if (closing && p.unposted > 0) {
    allow = confirm(`توجد ${p.unposted} مسودة غير مرحلة. هل تريد الإغلاق رغم ذلك؟`);
    if (!allow) return;
  }
  periodBusy = true;
  transition.disabled = true;
  periodError('');
  try {
    const r = await apiFetch('/api/periods/' + encodeURIComponent(p.id) + '/transition', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json'
      },
      body: JSON.stringify({
        expected_status: p.status,
        approved: true,
        allow_unposted: allow
      })
    });
    const data = await r.json();
    if (!r.ok) throw new Error(data.error || 'تعذر تغيير الحالة.');
    await loadPeriods();
    toast(closing ? 'أُغلقت الفترة للترحيل.' : 'أُعيد فتح الفترة للترحيل.');
  } catch (err) {
    periodError(err.message || 'تعذر الاتصال؛ حدّث القائمة قبل الإعادة.');
  } finally {
    periodBusy = false;
    transition.disabled = false;
  }
});
// A shared tooltip is used for all contracted elements, on pointer and keyboard focus.

// A shared tooltip is used for all contracted elements, on pointer and keyboard focus.
const tip = $('#tooltip');

let tipTimer,
  activeTrigger = null;

function hideTip() {
  clearTimeout(tipTimer);
  tip.classList.add('hidden');
  if (activeTrigger) activeTrigger.removeAttribute('aria-describedby');
  activeTrigger = null;
}

function showTip(el) {
  clearTimeout(tipTimer);
  tipTimer = setTimeout(() => {
    if (!el.isConnected) return;
    activeTrigger = el;
    tip.textContent = el.dataset.help;
    tip.classList.remove('hidden');
    el.setAttribute('aria-describedby', 'tooltip');
    const r = el.getBoundingClientRect(),
      t = tip.getBoundingClientRect();
    let left = Math.max(8, Math.min(window.innerWidth - t.width - 8, r.right - t.width));
    let top = r.bottom + 8;
    if (top + t.height > window.innerHeight - 8) top = Math.max(8, r.top - t.height - 8);
    tip.style.left = left + 'px';
    tip.style.top = top + 'px';
  }, 350);
}

document.addEventListener('mouseover', e => {
  let el = e.target.closest('[data-help]');
  if (el && el !== activeTrigger) {
    hideTip();
    showTip(el);
  }
});

document.addEventListener('mouseout', e => {
  if (activeTrigger && (activeTrigger.contains(e.relatedTarget) || tip.contains(e.relatedTarget))) return;
  hideTip();
});

tip.addEventListener('mouseleave', hideTip);

document.addEventListener('focusin', e => {
  const el = e.target.closest('[data-help]');
  if (el) {
    hideTip();
    showTip(el);
  }
});

document.addEventListener('focusout', () => hideTip());

document.addEventListener('keydown', e => {
  if (e.key === 'Escape') hideTip();
});

async function loadReferenceCatalog() {
  try {
    const r = await apiFetch('/assets/rest8-catalog.json');
    if (!r.ok) throw new Error('catalog HTTP ' + r.status);
    const data = await r.json();
    if (!Array.isArray(data.screens) || !Array.isArray(data.sections)) throw new Error('invalid catalog');
    referenceCatalog = data;
    for (const screen of data.screens) {
      shortcutChoices.push({
        id: 'screen:' + screen.perm_id,
        title: screen.name
      });
    }
    renderShortcuts();
  } catch (e) {
    console.error('تعذر تحميل فهرس رست 8:', e);
    toast('تعذر تحميل فهرس رست 8؛ تُعرض الإدارات الأساسية فقط.');
  }
}

$('#department-count').textContent = String(departments.length);

const welcomeIcon = $('#page-title span');

if (welcomeIcon) welcomeIcon.textContent = '✦';
