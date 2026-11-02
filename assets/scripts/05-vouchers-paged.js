'use strict';

$('#today').textContent = new Intl.DateTimeFormat('ar-YE', {
  year: 'numeric',
  month: 'long',
  day: 'numeric'
}).format(new Date());

renderCards();

renderShortcuts();

loadReferenceCatalog();

window.refreshAfterAuth = () => {
  ledgerConfigPromise = readLedgerConfig();
  loadDrafts();
};

// Voucher amounts and account snapshots come from the same validated journal draft.
let voucherRows = [],
  voucherRequest = 0,
  voucherKind = 'receipt',
  voucherToken = '',
  voucherSaving = false,
  voucherOpening = false,
  voucherAccounts = [],
  voucherFrozen = null,
  voucherCeiling = null,
  voucherBefore = null,
  voucherNext = null,
  voucherHistory = [],
  voucherSearchTimer = null;

function renderVouchers() {
  const rows = voucherRows;
  $('#voucher-count').textContent = `${rows.length} سند في الصفحة`;
  $('#voucher-list').innerHTML = rows.map(v => `<tr><td><span class="voucher-type ${v.kind === 'receipt' ? 'receipt' : 'payment'}">${v.kind === 'receipt' ? '↙ قبض' : '↗ صرف'}</span><small class="voucher-sub">#${escapeHTML(v.id)}</small></td><td dir="ltr">${escapeHTML(v.date)}</td><td><strong>${escapeHTML(v.party_name)}</strong><small class="voucher-sub">${escapeHTML(v.description)}${v.reference ? ' · ' + escapeHTML(v.reference) : ''}</small></td><td>${escapeHTML(v.cash_account)}<small class="voucher-sub">المقابل: ${escapeHTML(v.other_account)}</small></td><td><b dir="ltr">${escapeHTML(v.amount)}</b> <small>${escapeHTML(v.currency)}</small></td><td><span class="status-tag">${v.status === 'posted_experimental' ? 'مرحّل تجريبيًا' : 'مسودة'}</span></td><td><button type="button" class="draft-view-btn" data-voucher-draft="${escapeHTML(v.draft_id)}" data-help="عرض القيد المتوازن المرتبط بهذا السند.">القيد #${escapeHTML(v.draft_id)}</button></td></tr>`).join('');
  $('#voucher-empty').classList.toggle('hidden', rows.length > 0);
  $('#voucher-empty').textContent = 'لا توجد سندات مطابقة للمرشحات في هذه الصفحة.';
}

function voucherLoading() {
  $('#voucher-count').textContent = 'جارٍ التحميل…';
  $('#voucher-list').innerHTML = '';
  $('#voucher-empty').classList.add('hidden');
  $('#voucher-more').classList.add('hidden');
  $('#voucher-newer').disabled = true;
  $('#voucher-older').disabled = true;
}

async function loadVouchers(page = {}) {
  clearTimeout(voucherSearchTimer);
  const request = ++voucherRequest;
  const error = $('#voucher-list-error');
  error.classList.add('hidden');
  voucherLoading();
  const params = new URLSearchParams();
  for (const [field, selector] of [['q', '#voucher-search'], ['kind', '#voucher-filter'],
    ['currency', '#voucher-currency'], ['from', '#voucher-from'], ['to', '#voucher-to']]) {
    const value = $(selector).value.trim();
    if (value) params.set(field, value);
  }
  if (page.ceiling != null) params.set('ceiling', String(page.ceiling));
  if (page.before != null) params.set('before', String(page.before));
  try {
    const r = await apiFetch('/api/vouchers?' + params.toString());
    const data = await r.json();
    if (request !== voucherRequest) return;
    if (!r.ok) throw new Error(data.error || 'تعذر تحميل السندات.');
    if (!Array.isArray(data.rows) || typeof data.has_more !== 'boolean' ||
      !Number.isSafeInteger(data.ceiling) || data.ceiling < 0 ||
      (data.has_more && (!Number.isSafeInteger(data.next_before) || data.next_before <= 0)) ||
      (!data.has_more && data.next_before !== null)) throw new Error('رد السندات غير صالح.');
    voucherRows = data.rows;
    voucherCeiling = data.ceiling;
    voucherBefore = page.before ?? null;
    voucherNext = data.next_before;
    voucherHistory = page.history ?? [];
    $('#voucher-page').textContent = `صفحة ${voucherHistory.length + 1}`;
    $('#voucher-more').classList.toggle('hidden', !data.has_more);
    $('#voucher-newer').disabled = voucherHistory.length === 0;
    $('#voucher-older').disabled = !data.has_more;
    renderVouchers();
  } catch (e) {
    if (request !== voucherRequest) return;
    voucherRows = [];
    $('#voucher-list').innerHTML = '';
    $('#voucher-empty').classList.add('hidden');
    $('#voucher-more').classList.add('hidden');
    $('#voucher-count').textContent = 'تعذر التحميل';
    error.textContent = e.message || 'تعذر الاتصال بالخادم.';
    error.classList.remove('hidden');
  }
}

function scheduleVoucherSearch(delay = 0) {
  clearTimeout(voucherSearchTimer);
  ++voucherRequest; // Discard any in-flight response as soon as the filters change.
  voucherLoading();
  $('#voucher-list-error').classList.add('hidden');
  voucherSearchTimer = setTimeout(() => loadVouchers(), delay);
}

function voucherAccountOptions() {
  const form = $('#voucher-form'),
    choices = voucherAccounts.filter(a => a.kind === 'detail' && a.currency === form.elements.currency.value);
  const html = '<option value="">اختر حسابًا تفصيليًا</option>' + choices.map(a => `<option value="${escapeHTML(a.id)}">${escapeHTML(a.code)} — ${escapeHTML(a.name)}</option>`).join('');
  for (const name of ['cash_account_id', 'other_account_id']) {
    const el = form.elements[name],
      old = el.value;
    el.innerHTML = html;
    if (choices.some(a => String(a.id) === old)) el.value = old;
  }
  const notice = $('#voucher-account-notice');
  notice.classList.toggle('hidden', choices.length >= 2);
  notice.textContent = `يلزم حسابان تفصيليان مختلفان بعملة ${form.elements.currency.value}؛ أضفهما من دليل الحسابات أولًا.`;
  $('button[type=submit]', form).disabled = choices.length < 2 || voucherSaving;
}

async function openVoucher(kind) {
  if (voucherOpening || voucherSaving) return;
  voucherOpening = true;
  try {
    if (!globalThis.crypto?.getRandomValues) throw new Error('يتعذر إنشاء معرّف حفظ آمن في هذا المتصفح.');
    const bytes = new Uint8Array(16);
    crypto.getRandomValues(bytes);
    voucherToken = Array.from(bytes, b => b.toString(16).padStart(2, '0')).join('');
    const r = await apiFetch('/api/accounts');
    if (!r.ok) throw new Error('تعذر تحميل دليل الحسابات.');
    const data = await r.json();
    if (!Array.isArray(data)) throw new Error('رد الحسابات غير صالح.');
    voucherAccounts = data;
    voucherKind = kind;
    voucherFrozen = null;
    const form = $('#voucher-form');
    form.reset();
    form.elements.date.value = new Date(Date.now() - new Date().getTimezoneOffset() * 60000).toISOString().slice(0, 10);
    $('#voucher-form-error').classList.add('hidden');
    $('#voucher-dialog-title').textContent = kind === 'receipt' ? 'مسودة سند قبض' : 'مسودة سند صرف';
    $('#voucher-direction').textContent = kind === 'receipt' ? 'القيد: حساب النقدية مدين، والحساب المقابل دائن. لا يُرحّل السند عند الحفظ.' : 'القيد: حساب النقدية دائن، والحساب المقابل مدين. لا يُرحّل السند عند الحفظ.';
    voucherAccountOptions();
    $('#voucher-dialog').showModal();
  } catch (e) {
    toast(e.message || 'تعذر فتح السند.');
  } finally {
    voucherOpening = false;
  }
}

if ($('#voucher-form')) {
  $('#voucher-receipt').onclick = () => openVoucher('receipt');
  $('#voucher-payment').onclick = () => openVoucher('payment');
  $('#voucher-refresh').onclick = () => loadVouchers();
  $('#voucher-search').addEventListener('input', () => scheduleVoucherSearch(250));
  for (const id of ['#voucher-filter', '#voucher-currency', '#voucher-from', '#voucher-to']) {
    $(id).addEventListener('change', () => scheduleVoucherSearch());
  }
  $('#voucher-older').onclick = () => {
    if (voucherNext != null) loadVouchers({
      before: voucherNext, ceiling: voucherCeiling, history: [...voucherHistory, voucherBefore]
    });
  };
  $('#voucher-newer').onclick = () => {
    if (voucherHistory.length) loadVouchers({
      before: voucherHistory[voucherHistory.length - 1],
      ceiling: voucherCeiling, history: voucherHistory.slice(0, -1)
    });
  };
  $('#voucher-list').addEventListener('click', e => {
    const btn = e.target.closest('[data-voucher-draft]');
    if (btn) openDraftDetail(btn.dataset.voucherDraft);
  });
  $('#voucher-form [name=currency]').addEventListener('change', voucherAccountOptions);
  for (const id of ['#close-voucher', '#cancel-voucher']) $(id).onclick = () => {
    requestDialogDismiss($('#voucher-dialog'));
  };
  $('#voucher-dialog').addEventListener('cancel', e => {
    if (voucherSaving) e.preventDefault();
  });
  $('#voucher-form').addEventListener('submit', async e => {
    e.preventDefault();
    if (voucherSaving) return;
    const form = e.currentTarget,
      err = $('#voucher-form-error');
    err.classList.add('hidden');
    const payload = {
      request_id: voucherToken,
      kind: voucherKind,
      date: form.elements.date.value,
      currency: form.elements.currency.value,
      amount: form.elements.amount.value,
      cash_account_id: Number(form.elements.cash_account_id.value),
      other_account_id: Number(form.elements.other_account_id.value),
      party_name: form.elements.party_name.value,
      description: form.elements.description.value,
      reference: form.elements.reference.value
    };
    const encoded = JSON.stringify(payload);
    if (voucherFrozen && voucherFrozen !== encoded) {
      err.textContent = 'نتيجة المحاولة السابقة غير مؤكدة؛ أعد المحاولة دون تغيير البيانات أو أغلق النافذة وافحص سجل السندات أولًا.';
      err.classList.remove('hidden');
      return;
    }
    voucherSaving = true;
    voucherFrozen = encoded;
    const submit = $('button[type=submit]', form);
    submit.disabled = true;
    $('#close-voucher').disabled = true;
    $('#cancel-voucher').disabled = true;
    try {
      const r = await apiFetch('/api/vouchers', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json'
        },
        body: encoded
      });
      const data = await r.json();
      if (!r.ok) {
        if (r.status === 422) voucherFrozen = null;
        throw new Error(data.error || 'تعذر حفظ السند.');
      }
      $('#voucher-dialog').close();
      toast(data.status === 'posted_experimental' ? `السند #${data.id} مرتبط بالقيد #${data.draft_id} المرحّل تجريبيًا.` : `حُفظ سند #${data.id} مع مسودة القيد #${data.draft_id} دون ترحيل.`);
      await loadVouchers();
    } catch (ex) {
      err.textContent = ex.message || 'تعذر التحقق من النتيجة. أعد المحاولة دون تغيير البيانات أو افحص السجل.';
      err.classList.remove('hidden');
    } finally {
      voucherSaving = false;
      $('#close-voucher').disabled = false;
      $('#cancel-voucher').disabled = false;
      voucherAccountOptions();
    }
  });
}
