'use strict';

let draftDetailRequest = 0;

async function openDraftDetail(id) {
  const request = ++draftDetailRequest;
  const dialog = $('#draft-details-dialog'),
    body = $('#draft-detail-body');
  $('#post-draft').classList.add('hidden');
  body.className = '';
  body.textContent = 'جارٍ تحميل تفاصيل المسودة والتحقق منها…';
  dialog.showModal();
  try {
    const res = await apiFetch('/api/drafts/' + encodeURIComponent(id));
    let data = await res.json();
    if (request !== draftDetailRequest || !dialog.open) return;
    if (!res.ok) throw new Error(data.error || 'تعذر تحميل التفاصيل.');
    if (!Array.isArray(data.lines) || !data.totals) throw new Error('رد الخادم غير مكتمل.');
    const postButton = $('#post-draft');
    postButton.dataset.id = String(data.id);
    postButton.classList.toggle('hidden', !ledgerEnabled || data.status !== 'draft' || data.lines.some(x => !x.account_id));
    body.innerHTML = `<div class="detail-meta"><div><small>رقم المسودة</small><b>#${escapeHTML(data.id)}</b></div><div><small>التاريخ</small><b dir="ltr">${escapeHTML(data.date)}</b></div><div><small>العملة</small><b>${escapeHTML(data.currency)}</b></div><div><small>الحالة</small><b>${data.status === 'posted_experimental' ? 'مرحّل تجريبيًا' : 'مسودة'}</b></div></div><p class="detail-description"><b>البيان:</b> ${escapeHTML(data.description)}</p><div class="table-wrap"><table><thead><tr><th data-help="اسم الحساب كما كان عند حفظ المسودة. المسودات القديمة قد لا تحمل رابطًا للدليل.">الحساب عند الحفظ</th><th data-help="مبلغ المدين كما حُفظ.">مدين</th><th data-help="مبلغ الدائن كما حُفظ.">دائن</th></tr></thead><tbody>${data.lines.map(l => `<tr><td>${escapeHTML(l.account)}${l.account_id ? ` <small class="draft-account-ref">#${escapeHTML(l.account_id)}</small>` : '<small class="draft-account-ref">مسودة قديمة بلا ارتباط بالدليل</small>'}</td><td dir="ltr">${escapeHTML(l.debit || '—')}</td><td dir="ltr">${escapeHTML(l.credit || '—')}</td></tr>`).join('')}</tbody><tfoot><tr><th>الإجمالي</th><th dir="ltr">${escapeHTML(data.totals.debit)}</th><th dir="ltr">${escapeHTML(data.totals.credit)}</th></tr></tfoot></table></div><p class="draft-limit-note">الاسم المعروض لقطة وقت الحفظ. المسودات القديمة بلا معرّف حساب تبقى للقراءة فقط ولا تُربط تلقائيًا.</p>`;
  } catch (error) {
    if (request !== draftDetailRequest || !dialog.open) return;
    console.error('draft detail error:', error);
    body.textContent = error.message || 'تعذر الاتصال بالخادم.';
    body.className = 'draft-detail-error';
    return;
  }
  body.className = '';
}

$('#draft-search').addEventListener('input', renderDrafts);

$('#draft-list').addEventListener('click', e => {
  const btn = e.target.closest('[data-draft-id]');
  if (btn) openDraftDetail(btn.dataset.draftId);
});

['#close-draft-details', '#done-draft-details'].forEach(x => $(x).onclick = () => requestDialogDismiss($('#draft-details-dialog')));

let draftAccountsLoaded = false;

function updateDraftChoices() {
  const currency = $('#draft-form [name=currency]').value,
    choices = loadedAccounts.filter(a => a.kind === 'detail' && a.currency === currency);
  const options = '<option value="">اختر حسابًا تفصيليًا</option>' + choices.map(a => `<option value="${escapeHTML(a.id)}">${escapeHTML(a.code)} — ${escapeHTML(a.name)}</option>`).join('');
  let cleared = false;
  $$('#line-items [name=account]').forEach(sel => {
    const old = sel.value;
    sel.innerHTML = options;
    if (old && choices.some(a => String(a.id) === old)) sel.value = old;else if (old) cleared = true;
  });
  const notice = $('#draft-account-notice');
  notice.classList.toggle('hidden', draftAccountsLoaded && choices.length > 0);
  notice.textContent = draftAccountsLoaded ? `لا توجد حسابات تفصيلية بعملة ${currency}. أضفها من دليل الحسابات قبل حفظ مسودة جديدة.` : 'تعذر تحميل دليل الحسابات؛ تحقق من الخادم وأعد فتح المسودة.';
  $('#draft-form button[type=submit]').disabled = !draftAccountsLoaded || choices.length === 0;
  if (cleared) toast('أُلغي اختيار حساب لا يطابق العملة الجديدة؛ أعد الاختيار.');
}

function addLine() {
  if ($$('.line-row').length >= 50) {
    toast('الحد الأقصى ٥٠ سطرًا.');
    return;
  }
  let el = document.createElement('div');
  el.className = 'line-row';
  el.innerHTML = '<select name="account" required aria-label="الحساب" data-help="اختر حسابًا تفصيليًا من دليل الحسابات بنفس عملة المسودة."></select><input name="debit" placeholder="0.00" inputmode="decimal" aria-label="مدين" data-help="مبلغ مدين؛ اتركه فارغًا إن كان السطر دائنًا."><input name="credit" placeholder="0.00" inputmode="decimal" aria-label="دائن" data-help="مبلغ دائن؛ اتركه فارغًا إن كان السطر مدينًا."><button type="button" class="remove-line" aria-label="حذف السطر" data-help="حذف هذا السطر من المسودة.">×</button>';
  $('#line-items').append(el);
  updateDraftChoices();
}

let openingDraft = false;

async function openDraft() {
  if (openingDraft) return;
  openingDraft = true;
  $('#open-draft').disabled = true;
  try {
    draftAccountsLoaded = await loadAccounts();
    $('#draft-form').reset();
    $('#form-message').classList.add('hidden');
    $('#line-items').innerHTML = '';
    addLine();
    addLine();
    $('#draft-form [name=date]').value = new Date(Date.now() - new Date().getTimezoneOffset() * 60000).toISOString().slice(0, 10);
    $('#draft-dialog').showModal();
  } finally {
    openingDraft = false;
    $('#open-draft').disabled = false;
  }
}

$('#draft-form [name=currency]').addEventListener('change', updateDraftChoices);

let saving = false;

$('#draft-form').addEventListener('submit', async e => {
  e.preventDefault();
  if (saving) return;
  const form = e.currentTarget,
    err = $('#form-message');
  err.classList.add('hidden');
  const payload = {
    description: form.elements.description.value,
    date: form.elements.date.value,
    currency: form.elements.currency.value,
    lines: $$('.line-row').map(row => ({
      account_id: Number($('[name=account]', row).value),
      debit: $('[name=debit]', row).value,
      credit: $('[name=credit]', row).value
    }))
  };
  saving = true;
  const button = $('button[type=submit]', form);
  button.disabled = true;
  try {
    const r = await apiFetch('/api/drafts', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json'
      },
      body: JSON.stringify(payload)
    });
    let data = await r.json();
    if (!r.ok) throw new Error(data.error || 'تعذر حفظ المسودة.');
    $('#draft-dialog').close();
    toast(`تم حفظ المسودة #${data.id} دون ترحيل.`);
    await loadDrafts();
  } catch (ex) {
    err.textContent = ex.message || 'تعذر الاتصال بالخادم. تحقق من قائمة المسودات قبل إعادة المحاولة.';
    err.classList.remove('hidden');
  } finally {
    saving = false;
    button.disabled = false;
  }
});

function fillSplit() {
  for (const [sel, content, defaultId] of [['#split-one', '#split-one-content', 'finance'], ['#split-two', '#split-two-content', 'sales']]) {
    const el = $(sel);
    if (!el.options.length) {
      el.innerHTML = departments.map(x => `<option value="${x.id}">${x.title}</option>`).join('');
      el.value = defaultId;
    }
    const render = () => {
      let d = departments.find(x => x.id === el.value);
      const refId = referenceIdMap[d.id] || d.id;
      const screens = referenceCatalog?.screens.filter(s => s.dept_id === refId) || [];
      $(content).innerHTML = `<h3>${escapeHTML(d.title)}</h3><p class="split-description">${escapeHTML(d.desc)}</p>${screens.length ? screens.map(referenceScreenCard).join('') : d.sections.map(sectionCard).join('')}`;
    };
    el.onchange = render;
    render();
  }
}

$('#open-draft').onclick = openDraft;

$('#add-line').onclick = addLine;

$('#line-items').addEventListener('click', e => {
  if (e.target.closest('.remove-line')) {
    if ($$('.line-row').length <= 2) {
      toast('يجب أن يحتوي القيد على سطرين على الأقل.');
      return;
    }
    e.target.closest('.line-row').remove();
  }
});

['#close-draft', '#cancel-draft'].forEach(x => $(x).onclick = () => requestDialogDismiss($('#draft-dialog')));

$('#reload-drafts').onclick = loadDrafts;

$('#close-split').onclick = () => navigate('home');

$('#notice-close').onclick = () => $('.notice').classList.add('hidden');

$('#all-departments').onclick = () => navigate('home');

document.addEventListener('click', e => {
  const pin = e.target.closest('[data-pin]');
  if (pin) {
    const id = pin.dataset.pin;
    shortcuts = shortcuts.includes(id) ? shortcuts.filter(x => x !== id) : [...shortcuts, id];
    try {
      localStorage.setItem('smart-shortcuts', JSON.stringify(shortcuts));
    } catch {
      toast('تعذر حفظ الاختصارات على هذا الجهاز.');
    }
    renderShortcuts();
    pin.classList.toggle('pinned', shortcuts.includes(id));
    pin.setAttribute('aria-label', shortcuts.includes(id) ? 'إزالة من الاختصارات' : 'إضافة للاختصارات');
    pin.dataset.help = shortcuts.includes(id) ? 'إزالة الشاشة من شريط الاختصارات.' : 'إضافة الشاشة إلى شريط الاختصارات.';
    toast(shortcuts.includes(id) ? 'أضيفت الشاشة إلى اختصاراتك.' : 'أزيلت الشاشة من الاختصارات.');
    return;
  }
  const el = e.target.closest('[data-open],[data-nav],[data-action]');
  if (el) {
    navigate(el.dataset.open || el.dataset.nav || el.dataset.action);
  }
});

$('#add-shortcut').onclick = () => {
  $('#shortcut-options').innerHTML = shortcutChoices.filter(x => !x.id.startsWith('screen:') || shortcuts.includes(x.id)).map(x => `<label><input type="checkbox" value="${x.id}" ${shortcuts.includes(x.id) ? 'checked' : ''}>${x.title}</label>`).join('');
  $('#shortcut-dialog').showModal();
};

$('#close-shortcuts').onclick = () => requestDialogDismiss($('#shortcut-dialog'));

$('#save-shortcuts').onclick = () => {
  shortcuts = $$('#shortcut-options input:checked').map(x => x.value);
  try {
    localStorage.setItem('smart-shortcuts', JSON.stringify(shortcuts));
  } catch {
    toast('تعذر حفظ الاختصارات على هذا الجهاز.');
  }
  renderShortcuts();
  $('#shortcut-dialog').close();
};

$('#notifications').onclick = () => toast('لا توجد إشعارات في النسخة التأسيسية.');

$('#assistant-info').onclick = () => toast('مساعد إرشادي للتنقل؛ لم يُربط بنموذج ذكاء اصطناعي بعد.');

$('#global-search').onclick = () => {
  const q = prompt('ابحث عن إدارة أو شاشة:');
  if (!q) return;
  const v = q.trim();
  const d = departments.find(x => x.title.includes(v));
  const screen = referenceCatalog?.screens.find(x => x.name.includes(v));
  if (v.includes('قيد') || v.includes('قيود')) navigate('journals');else if (d) navigate(d.id);else if (screen) navigate('screen:' + screen.perm_id);else toast('لم يُعثر على نتيجة مطابقة.');
};

$('#chat-form').onsubmit = e => {
  e.preventDefault();
  const inp = $('#chat-text');
  const value = inp.value.trim();
  if (!value) return;
  const chat = $('#chat-area');
  const user = document.createElement('div');
  user.className = 'user-bubble';
  user.textContent = value;
  chat.append(user);
  const row = document.createElement('div');
  row.className = 'chat-row';
  const answer = document.createElement('div');
  answer.className = 'bubble';
  let message = 'أنا مساعد إرشادي بسيط للتنقل، ولم أتصل بنموذج ذكاء اصطناعي بعد. استخدم الأزرار أو ابحث عن الإدارة المطلوبة.';
  if (value.includes('قيد') || value.includes('قيود')) {
    navigate('journals');
    message = 'فتحت لك مسودات القيود. يمكنك إنشاء مسودة متوازنة؛ الترحيل إن فُعّل تجريبي فقط وبعد اعتمادك.';
  } else if (value.includes('مالي')) {
    navigate('finance');
    message = 'فتحت لك الإدارة المالية وأقسامها.';
  } else if (value.includes('نافذت')) {
    navigate('split');
    message = 'فتحت العرض المتجاور، ويمكنك اختيار إدارة لكل جانب.';
  } else if (value.includes('مخزون')) {
    navigate('stock');
    message = 'فتحت إدارة المخزون. أقسامها قيد التطوير.';
  }
  answer.textContent = message;
  row.innerHTML = '<div class="bot-avatar">✦</div>';
  row.append(answer);
  chat.append(row);
  inp.value = '';
  chat.scrollTop = chat.scrollHeight;
};
// Experimental reports and explicit human approval. Never treat as production accounting.

// Experimental reports and explicit human approval. Never treat as production accounting.
let ledgerEnabled = false;

async function readLedgerConfig() {
  try {
    const r = await apiFetch('/api/ledger/config');
    if (!r.ok) throw new Error('config');
    ledgerEnabled = Boolean((await r.json()).experimental_posting);
    const notice = $('.notice');
  if (notice) notice.classList.add('hidden'); // أُزيل شريط التنبيه من الواجهة بطلب المالك
  } catch {
    ledgerEnabled = false;
  }
}

let ledgerConfigPromise = Promise.resolve();

$('#post-draft').addEventListener('click', async e => {
  const button = e.currentTarget,
    id = button.dataset.id;
  if (!ledgerEnabled || !id || button.disabled) return;
  if (!confirm('ترحيل تجريبي فقط: الصلاحيات المحلية محدودة ولا يوجد إقفال مالي نهائي، ولا يمكن تعديل القيد المرحّل. هل راجعت السطور وتؤكد الترحيل؟')) return;
  button.disabled = true;
  try {
    const r = await apiFetch('/api/ledger/post/' + encodeURIComponent(id), {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json'
      },
      body: JSON.stringify({
        approved: true
      })
    });
    const data = await r.json();
    if (!r.ok) throw new Error(data.error || 'تعذر الترحيل');
    $('#draft-details-dialog').close();
    toast('رُحّل القيد تجريبيًا #' + data.entry_id);
    await loadDrafts();
  } catch (err) {
    toast((err.message || 'انقطع الاتصال.') + ' تحقق من القائمة قبل المحاولة مرة أخرى.');
    await loadDrafts();
  } finally {
    button.disabled = false;
  }
});
