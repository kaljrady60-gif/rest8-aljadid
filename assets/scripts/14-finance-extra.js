// تبويبات المالية المتبقية: التسويات · مراجعة الوثائق · المخططات والعمولات ·
// بيانات الدول · الحسابات الوسيطة · متغيرات الأستاذ · الطلبات · قراءة الفواتير.
(function () {
  'use strict';
  const esc = v => String(v == null ? '' : v)
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  const money = v => Number(v || 0).toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 4 });
  const cid = () => Number(window.FinanceApi.companyId());
  const q = extra => `?company_id=${cid()}${extra || ''}`;

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
  function table(headers, rows, empty) {
    if (!rows.length) return `<p class="fp-empty">${esc(empty || 'لا توجد بيانات بعد.')}</p>`;
    return `<div class="fp-table-wrap"><table class="fp-table"><thead><tr>${
      headers.map(h => `<th>${esc(h)}</th>`).join('')
    }</tr></thead><tbody>${
      rows.map(r => `<tr>${r.map(c => `<td>${c}</td>`).join('')}</tr>`).join('')
    }</tbody></table></div>`;
  }
  const badge = (status) => {
    const map = {
      pending: ['قيد المراجعة', 'fp-badge'], approved: ['معتمد', 'fp-ok'],
      rejected: ['مرفوض', 'fp-error'], draft: ['مسودة', 'fp-badge'],
      submitted: ['مقدَّم', 'fp-badge']
    };
    const [label, cls] = map[status] || [status, 'fp-badge'];
    return `<span class="${cls}">${esc(label)}</span>`;
  };
  const rate = bp => `${(Number(bp) / 100).toFixed(2)}%`;
  async function accountsOptions() {
    const list = await api(`/api/finance/accounts${q()}`).catch(() => []);
    const arr = Array.isArray(list) ? list : (list.accounts || []);
    return arr.map(a => `<option value="${esc(a.id)}">${esc(a.code)} — ${esc(a.name)}</option>`).join('');
  }

  const views = {
    // ── التسويات المحاسبية ────────────────────────────────────────────────
    async adjustments() {
      const [rows, accs] = await Promise.all([
        api(`/api/finance/adjustments${q()}`).catch(() => []),
        accountsOptions()
      ]);
      const kinds = [['accrual', 'استحقاق'], ['prepaid', 'مصروف مقدم'], ['depreciation', 'إهلاك'],
        ['provision', 'مخصص'], ['reclass', 'إعادة تصنيف']];
      const data = (Array.isArray(rows) ? rows : []).map(r => [
        esc((kinds.find(k => k[0] === r.kind) || [, r.kind])[1]),
        esc(r.description),
        `<b dir="ltr">${money(r.amount)}</b> <small>${esc(r.currency)}</small>`,
        badge(r.status),
        r.status === 'pending'
          ? `<button class="fp-link" data-act="adj-approve" data-id="${esc(r.id)}">اعتماد</button>
             <button class="fp-link" data-act="adj-reject" data-id="${esc(r.id)}">رفض</button>` : '—'
      ]);
      return `
        <form class="fp-form" id="fp-adj-form">
          <h3>تسوية محاسبية جديدة</h3>
          <div class="fp-grid">
            <label>النوع <select name="kind">${kinds.map(k => `<option value="${k[0]}">${esc(k[1])}</option>`).join('')}</select></label>
            <label>البيان <input name="description" maxlength="200" required></label>
            <label>المبلغ <input name="amount" inputmode="decimal" placeholder="0.00" required></label>
            <label>العملة <input name="currency" value="YER" maxlength="8" required></label>
            <label>حساب التسوية <select name="account_id" required>${accs}</select></label>
            <label>الحساب المقابل <select name="offset_account_id" required>${accs}</select></label>
          </div>
          <button type="submit" class="fp-primary">حفظ التسوية</button>
          <p class="fp-note" id="fp-adj-note"></p>
        </form>
        <h3>التسويات</h3>
        ${table(['النوع', 'البيان', 'المبلغ', 'الحالة', 'قرار'], data, 'لا توجد تسويات بعد.')}`;
    },

    // ── مراجعة الوثائق ────────────────────────────────────────────────────
    async docreview() {
      const rows = await api(`/api/finance/doc-reviews${q()}`).catch(() => []);
      const types = [['invoice', 'فاتورة'], ['receipt', 'إيصال'], ['contract', 'عقد'],
        ['statement', 'كشف حساب'], ['other', 'أخرى']];
      const data = (Array.isArray(rows) ? rows : []).map(r => [
        esc((types.find(t => t[0] === r.doc_type) || [, r.doc_type])[1]),
        `<code>${esc(r.doc_ref)}</code>`,
        esc(r.note || '—'),
        badge(r.status),
        r.status === 'pending'
          ? `<button class="fp-link" data-act="doc-approve" data-id="${esc(r.id)}">اعتماد</button>
             <button class="fp-link" data-act="doc-reject" data-id="${esc(r.id)}">رفض</button>` : '—'
      ]);
      const total = data.length;
      const done = data.filter(r => String(r[3]).includes('معتمد') || String(r[3]).includes('مرفوض')).length;
      const pending = total - done;
      const percent = total ? Math.round((done / total) * 100) : 0;
      const progress = `
        <div class="fp-progress" role="group" aria-label="تقدّم مراجعة الترحيل">
          <div class="fp-progress-row">
            <span>تقدّم المراجعة</span>
            <b>${percent}%</b>
            <small>${done} من ${total} وثيقة · ${pending} بانتظار قرار</small>
          </div>
          <div class="fp-progress-bar" role="progressbar" aria-valuemin="0" aria-valuemax="100"
               aria-valuenow="${percent}" aria-label="نسبة الوثائق المراجَعة">
            <i style="inline-size:${percent}%"></i>
          </div>
          ${total === 0 ? '<p class="fp-note">لا وثائق بعد — سجّل أول وثيقة أدناه ليبدأ العدّ.</p>' : ''}
        </div>`;
      return `
        ${progress}
        <form class="fp-form" id="fp-doc-form">
          <h3>تسجيل وثيقة للمراجعة</h3>
          <div class="fp-grid">
            <label>النوع <select name="doc_type">${types.map(t => `<option value="${t[0]}">${esc(t[1])}</option>`).join('')}</select></label>
            <label>المرجع <input name="doc_ref" maxlength="60" required placeholder="INV-2026-001"></label>
            <label>ملاحظة <input name="note" maxlength="200"></label>
          </div>
          <button type="submit" class="fp-primary">حفظ للمراجعة</button>
          <p class="fp-note" id="fp-doc-note"></p>
        </form>
        <h3>قائمة المراجعة</h3>
        ${table(['النوع', 'المرجع', 'ملاحظة', 'الحالة', 'قرار'], data, 'لا توجد وثائق للمراجعة.')}`;
    },

    // ── المخططات والعمولات ───────────────────────────────────────────────
    async plans() {
      const [rows, accs] = await Promise.all([
        api(`/api/finance/plans${q()}`).catch(() => []),
        accountsOptions()
      ]);
      const kinds = [['percent', 'نسبة'], ['fixed', 'مبلغ ثابت'], ['tiered', 'شرائح']];
      const data = (Array.isArray(rows) ? rows : []).map(r => [
        esc(r.code), esc(r.name),
        esc((kinds.find(k => k[0] === r.kind) || [, r.kind])[1]),
        rate(r.rate_bp),
        `<b dir="ltr">${money(r.fixed_amount)}</b>`,
        r.archived_at ? '<span class="fp-badge">مؤرشف</span>' : `<button class="fp-link" data-act="plan-archive" data-id="${esc(r.id)}">أرشفة</button>`
      ]);
      const planList = (Array.isArray(rows) ? rows : []).filter(r => !r.archived_at)
        .map(r => `<option value="${esc(r.id)}">${esc(r.code)} — ${esc(r.name)}</option>`).join('');
      return `
        <form class="fp-form" id="fp-plan-form">
          <h3>مخطط عمولة</h3>
          <div class="fp-grid">
            <label>الرمز <input name="code" maxlength="20" required></label>
            <label>الاسم <input name="name" maxlength="120" required></label>
            <label>النوع <select name="kind">${kinds.map(k => `<option value="${k[0]}">${esc(k[1])}</option>`).join('')}</select></label>
            <label>النسبة % <input name="rate" type="number" step="0.01" min="0" max="100" value="0"></label>
            <label>مبلغ ثابت <input name="fixed_amount" inputmode="decimal" value="0.00"></label>
            <label>حساب العمولة <select name="account_id"><option value="">— بلا حساب —</option>${accs}</select></label>
          </div>
          <button type="submit" class="fp-primary">حفظ المخطط</button>
          <p class="fp-note" id="fp-plan-note"></p>
        </form>
        <h3>المخططات</h3>
        ${table(['الرمز', 'الاسم', 'النوع', 'النسبة', 'ثابت', ''], data, 'لا توجد مخططات بعد.')}
        <form class="fp-form" id="fp-plan-quote">
          <h3>حساب عمولة</h3>
          <div class="fp-grid">
            <label>المخطط <select name="plan_id" required>${planList}</select></label>
            <label>المبلغ <input name="amount" inputmode="decimal" required placeholder="0.00"></label>
          </div>
          <button type="submit">احسب</button>
          <div id="fp-plan-result" class="fp-note"></div>
        </form>`;
    },

    // ── بيانات الدول ─────────────────────────────────────────────────────
    async countries() {
      const rows = await api(`/api/finance/countries${q()}`).catch(() => []);
      const data = (Array.isArray(rows) ? rows : []).map(r => [
        `<code>${esc(r.iso_code)}</code>`, esc(r.name), esc(r.currency),
        rate(r.tax_rate_bp),
        r.is_default ? '<span class="fp-ok">الدولة الافتراضية</span>' : '—',
        `<button class="fp-link" data-act="country-archive" data-id="${esc(r.id)}">أرشفة</button>`
      ]);
      return `
        <form class="fp-form" id="fp-country-form">
          <h3>إضافة دولة</h3>
          <div class="fp-grid">
            <label>الرمز (ISO) <input name="iso_code" maxlength="2" required placeholder="YE"></label>
            <label>الاسم <input name="name" maxlength="80" required></label>
            <label>العملة <input name="currency" maxlength="8" value="YER" required></label>
            <label>ضريبة % <input name="tax" type="number" step="0.01" min="0" max="100" value="0"></label>
            <label class="fp-check"><input type="checkbox" name="is_default"> اجعلها الافتراضية</label>
          </div>
          <button type="submit" class="fp-primary">حفظ الدولة</button>
          <p class="fp-note" id="fp-country-note"></p>
        </form>
        <h3>الدول</h3>
        ${table(['الرمز', 'الاسم', 'العملة', 'الضريبة', 'الحالة', ''], data, 'لا توجد دول مسجّلة.')}`;
    },

    // ── الحسابات الوسيطة ─────────────────────────────────────────────────
    async intermediate() {
      const [rows, accs] = await Promise.all([
        api(`/api/finance/intermediate${q()}`).catch(() => []),
        accountsOptions()
      ]);
      const kinds = [['clearing', 'تسوية جارية'], ['suspense', 'معلّق'], ['pending', 'قيد التسوية']];
      const data = (Array.isArray(rows) ? rows : []).map(r => [
        esc(r.code), esc(r.name),
        esc((kinds.find(k => k[0] === r.kind) || [, r.kind])[1]),
        esc(r.account_id),
        r.archived_at ? '<span class="fp-badge">مؤرشف</span>'
          : `<button class="fp-link" data-act="inter-archive" data-id="${esc(r.id)}">أرشفة</button>`
      ]);
      return `
        <form class="fp-form" id="fp-inter-form">
          <h3>حساب وسيط</h3>
          <div class="fp-grid">
            <label>الرمز <input name="code" maxlength="20" required></label>
            <label>الاسم <input name="name" maxlength="120" required></label>
            <label>النوع <select name="kind">${kinds.map(k => `<option value="${k[0]}">${esc(k[1])}</option>`).join('')}</select></label>
            <label>الحساب المرتبط <select name="account_id" required>${accs}</select></label>
          </div>
          <button type="submit" class="fp-primary">حفظ</button>
          <p class="fp-note" id="fp-inter-note"></p>
        </form>
        <h3>الحسابات الوسيطة</h3>
        ${table(['الرمز', 'الاسم', 'النوع', 'الحساب', ''], data, 'لا توجد حسابات وسيطة.')}`;
    },

    // ── متغيرات الأستاذ ──────────────────────────────────────────────────
    async ledgersettings() {
      const data = await api(`/api/finance/ledger-settings${q()}`).catch(() => ({ settings: [], allowed_keys: [] }));
      const current = new Map((data.settings || []).map(s => [s.key, s.value]));
      // القيم الفعّالة القادمة من الخادم: المضبوطة أو الافتراضية المعلنة.
      const effective = new Map((data.effective || []).map(e => [e.key, e.effective]));
      const labels = {
        'posting.require_period': 'اشترط فترة مفتوحة للترحيل',
        'posting.require_approval': 'اشترط اعتمادًا قبل الترحيل',
        'posting.allow_backdated': 'اسمح بالترحيل بتاريخ سابق',
        'reports.round_scale': 'منازل تقريب التقارير',
        'reports.base_currency': 'العملة الأساسية للتقارير'
      };
      const rows = (data.allowed_keys || []).map(key => [
        `<code>${esc(key)}</code>`,
        esc(labels[key] || key),
        `<input name="${esc(key)}" value="${esc(current.get(key) ?? '')}" class="ls-input" maxlength="120" placeholder="${esc(effective.get(key) ?? '')}">`,
        `<span dir="ltr">${esc(effective.get(key) ?? '—')}</span>`,
        current.has(key) ? '<span class="fp-ok">مضبوط</span>' : '<span class="fp-badge">افتراضي</span>'
      ]);
      return `
        <form class="fp-form" id="fp-ls-form">
          <h3>متغيرات الأستاذ</h3>
          ${table(['المفتاح', 'الوصف', 'القيمة المضبوطة', 'القيمة الفعّالة', 'الحالة'], rows, 'لا توجد مفاتيح.')}
          <p class="fp-note">تُطبَّق هذه المتغيرات لحظة الترحيل: شرط الفترة، وإجبار الاعتماد، ومنع الترحيل الرجعي.</p>
          <button type="submit" class="fp-primary">حفظ المتغيرات</button>
          <p class="fp-note" id="fp-ls-note"></p>
        </form>`;
    },

    // ── طلبات القيود والسندات ────────────────────────────────────────────
    async requests() {
      const rows = await api(`/api/finance/requests${q()}`).catch(() => []);
      const kinds = [['journal', 'طلب قيد'], ['voucher', 'طلب سند']];
      const data = (Array.isArray(rows) ? rows : []).map(r => [
        esc((kinds.find(k => k[0] === r.kind) || [, r.kind])[1]),
        esc(r.title), badge(r.status),
        r.status === 'draft'
          ? `<button class="fp-link" data-act="req-submit" data-id="${esc(r.id)}">تقديم</button>`
          : r.status === 'submitted'
            ? `<button class="fp-link" data-act="req-approve" data-id="${esc(r.id)}">اعتماد</button>
               <button class="fp-link" data-act="req-reject" data-id="${esc(r.id)}">رفض</button>` : '—'
      ]);
      return `
        <form class="fp-form" id="fp-req-form">
          <h3>طلب جديد</h3>
          <div class="fp-grid">
            <label>النوع <select name="kind">${kinds.map(k => `<option value="${k[0]}">${esc(k[1])}</option>`).join('')}</select></label>
            <label>العنوان <input name="title" maxlength="160" required></label>
            <label>ملاحظات <input name="note" maxlength="300"></label>
          </div>
          <button type="submit" class="fp-primary">حفظ الطلب</button>
          <p class="fp-note" id="fp-req-note"></p>
        </form>
        <h3>الطلبات</h3>
        ${table(['النوع', 'العنوان', 'الحالة', 'إجراء'], data, 'لا توجد طلبات.')}`;
    },

    // ── قراءة الفواتير المصورة ───────────────────────────────────────────
    async scanner() {
      let ai = { ready: false, note: 'جارٍ قراءة حالة المحرّك…', mode: 'hybrid', order: ['local', 'cloud'], local: {}, cloud: {} };
      try {
        const res = await window.apiFetch(`/api/finance/ai/settings?company_id=${cid()}`);
        if (res.ok) ai = await res.json();
      } catch (_) { /* الحالة تبقى معلنة */ }
      const mode = ai.mode || 'hybrid';
      const order = (ai.order && ai.order.length) ? ai.order : ['local', 'cloud'];
      const orderLabel = v => (v === 'local' ? 'المحلي أولًا' : 'السحابي أولًا');
      return `
        <form class="fp-form" id="fp-ai-form">
          <h3>مزوّد قراءة الفواتير — اختيارك أنت</h3>
          <p class="fp-note ${ai.ready ? '' : 'fp-error'}" id="fp-ai-state">${esc(ai.note || '')}</p>
          <div class="fp-grid">
            <label>النمط
              <select name="mode" id="fp-ai-mode">
                <option value="local" ${mode === 'local' ? 'selected' : ''}>محلي فقط (بلا إنترنت)</option>
                <option value="cloud" ${mode === 'cloud' ? 'selected' : ''}>سحابي فقط</option>
                <option value="hybrid" ${mode === 'hybrid' ? 'selected' : ''}>هجين — محاولة ثم رجوع آمن</option>
              </select>
            </label>
            <label>ترتيب الهجين
              <select name="order" id="fp-ai-order">
                <option value="local" ${order[0] === 'local' ? 'selected' : ''}>${orderLabel('local')}</option>
                <option value="cloud" ${order[0] === 'cloud' ? 'selected' : ''}>${orderLabel('cloud')}</option>
              </select>
            </label>
          </div>

          <fieldset class="fp-checks" data-ai-block="local">
            <legend>النموذج المحلي (Ollama)</legend>
            <div class="fp-grid">
              <label>عنوان الخدمة <input name="local_endpoint" dir="ltr" value="${esc((ai.local || {}).endpoint || 'http://127.0.0.1:11434')}"></label>
              <label>النموذج
                <select name="local_model" id="fp-ai-local-model">
                  <option value="${esc((ai.local || {}).model || 'llava')}">${esc((ai.local || {}).model || 'llava')}</option>
                </select>
              </label>
            </div>
            <div class="fp-actions">
              <button type="button" class="fp-ghost" data-ai-models="local">جلب النماذج المثبَّتة</button>
              <button type="button" class="fp-ghost" data-ai-probe="local">اختبار الاتصال</button>
            </div>
            <p class="fp-note" data-ai-msg="local"></p>
          </fieldset>

          <fieldset class="fp-checks" data-ai-block="cloud">
            <legend>النموذج السحابي (Gemini — الطبقة المجانية)</legend>
            <div class="fp-grid">
              <label>حالة المفتاح
                <input value="${(ai.cloud || {}).configured ? 'مضبوط ✔ — اتركه فارغًا للإبقاء عليه' : 'غير مضبوط'}" readonly></label>
              <label>مفتاح جديد (اختياري) <input name="cloud_key" type="password" dir="ltr" placeholder="AIza…"></label>
              <label>النموذج
                <select name="cloud_model" id="fp-ai-cloud-model">
                  <option value="${esc((ai.cloud || {}).model || 'gemini-2.5-flash')}">${esc((ai.cloud || {}).model || 'gemini-2.5-flash')}</option>
                </select>
              </label>
            </div>
            <div class="fp-actions">
              <button type="button" class="fp-ghost" data-ai-models="cloud">جلب النماذج المتاحة فعلًا</button>
              <button type="button" class="fp-ghost" data-ai-probe="cloud">اختبار الاتصال</button>
            </div>
            <p class="fp-note" data-ai-msg="cloud"></p>
          </fieldset>

          <div class="fp-actions"><button type="submit" class="fp-primary">حفظ الاختيار</button></div>
          <p class="fp-note" id="fp-ai-note">في النمط الهجين: يُجرَّب الأول، فإن فشل يُذكر سببه ويُجرَّب التالي، ولا يُحفظ أي ناتج جزئي.</p>
        </form>

        <form class="fp-form" id="fp-scan-image-form">
          <h3>قراءة فاتورة مصوّرة</h3>
          <div class="fp-grid">
            <label>صورة الفاتورة <input type="file" name="photo" accept="image/*"></label>
          </div>
          <button type="submit" class="fp-primary">قراءة الصورة</button>
          <div id="fp-scan-image-result"></div>
        </form>

        <form class="fp-form" id="fp-scan-form">
          <h3>قراءة فاتورة من نص</h3>
          <p class="fp-note">الصق نص الفاتورة (أو نص الاستخراج الضوئي). التحليل حتمي ومحلي: التاريخ والمبالغ والعملة تُستخرج بقواعد صريحة، وما لا يُستخرج يُعلن فشله بدل تخمينه.</p>
          <label>نص الفاتورة
            <textarea name="text" rows="8" maxlength="20000" placeholder="مثال:&#10;فاتورة رقم INV-2026-014&#10;التاريخ 12/10/2026&#10;خدمات صيانة 150000&#10;ضريبة 22500&#10;الإجمالي 172500 YER" required></textarea>
          </label>
          <button type="submit" class="fp-primary">تحليل الفاتورة</button>
          <div id="fp-scan-result"></div>
        </form>`;
    }
  };

  // ── ربط ──────────────────────────────────────────────────────────────
  function bind(tab, scope, refresh) {
    const note = (sel, text, bad) => {
      const el = scope.querySelector(sel);
      if (el) { el.textContent = text; el.classList.toggle('fp-error', !!bad); }
    };
    const val = (form, name) => {
      const el = form.querySelector(`[name="${name}"]`);
      return el ? el.value : '';
    };

    const adj = scope.querySelector('#fp-adj-form');
    if (adj) adj.addEventListener('submit', async ev => {
      ev.preventDefault();
      try {
        await post('/api/finance/adjustments', {
          company_id: cid(), kind: val(adj, 'kind'),
          description: val(adj, 'description').trim(), amount: val(adj, 'amount').trim(),
          currency: val(adj, 'currency').trim(), account_id: Number(val(adj, 'account_id')),
          offset_account_id: Number(val(adj, 'offset_account_id'))
        });
        note('#fp-adj-note', 'حُفظت التسوية وقيدت في سجل التدقيق.');
        if (refresh) await refresh();
      } catch (e) { note('#fp-adj-note', e.message, true); }
    });

    const doc = scope.querySelector('#fp-doc-form');
    if (doc) doc.addEventListener('submit', async ev => {
      ev.preventDefault();
      try {
        await post('/api/finance/doc-reviews', {
          company_id: cid(), doc_type: val(doc, 'doc_type'),
          doc_ref: val(doc, 'doc_ref').trim(), note: val(doc, 'note').trim() || null
        });
        note('#fp-doc-note', 'سُجلت الوثيقة للمراجعة.');
        if (refresh) await refresh();
      } catch (e) { note('#fp-doc-note', e.message, true); }
    });

    const plan = scope.querySelector('#fp-plan-form');
    if (plan) plan.addEventListener('submit', async ev => {
      ev.preventDefault();
      const pct = Number(val(plan, 'rate') || 0);
      const acc = val(plan, 'account_id');
      try {
        await post('/api/finance/plans', {
          company_id: cid(), code: val(plan, 'code').trim(), name: val(plan, 'name').trim(),
          kind: val(plan, 'kind'), rate_bp: Math.round(pct * 100),
          fixed_amount: val(plan, 'fixed_amount').trim() || '0',
          account_id: acc ? Number(acc) : null
        });
        note('#fp-plan-note', 'حُفظ المخطط.');
        if (refresh) await refresh();
      } catch (e) { note('#fp-plan-note', e.message, true); }
    });

    const quote = scope.querySelector('#fp-plan-quote');
    if (quote) quote.addEventListener('submit', async ev => {
      ev.preventDefault();
      try {
        const r = await api(`/api/finance/plans/quote${q(`&plan_id=${encodeURIComponent(val(quote, 'plan_id'))}&amount=${encodeURIComponent(val(quote, 'amount'))}`)}`);
        note('#fp-plan-result', `${r.plan} — المبلغ ${money(r.amount)} · العمولة ${money(r.commission)} · الصافي ${money(r.net)}`);
      } catch (e) { note('#fp-plan-result', e.message, true); }
    });

    const country = scope.querySelector('#fp-country-form');
    if (country) country.addEventListener('submit', async ev => {
      ev.preventDefault();
      const pct = Number(val(country, 'tax') || 0);
      try {
        await post('/api/finance/countries', {
          company_id: cid(), iso_code: val(country, 'iso_code').trim(),
          name: val(country, 'name').trim(), currency: val(country, 'currency').trim(),
          tax_rate_bp: Math.round(pct * 100),
          is_default: country.querySelector('[name="is_default"]').checked
        });
        note('#fp-country-note', 'حُفظت الدولة.');
        if (refresh) await refresh();
      } catch (e) { note('#fp-country-note', e.message, true); }
    });

    const inter = scope.querySelector('#fp-inter-form');
    if (inter) inter.addEventListener('submit', async ev => {
      ev.preventDefault();
      try {
        await post('/api/finance/intermediate', {
          company_id: cid(), code: val(inter, 'code').trim(), name: val(inter, 'name').trim(),
          kind: val(inter, 'kind'), account_id: Number(val(inter, 'account_id'))
        });
        note('#fp-inter-note', 'حُفظ الحساب الوسيط.');
        if (refresh) await refresh();
      } catch (e) { note('#fp-inter-note', e.message, true); }
    });

    const ls = scope.querySelector('#fp-ls-form');
    if (ls) ls.addEventListener('submit', async ev => {
      ev.preventDefault();
      const inputs = [...ls.querySelectorAll('.ls-input')];
      try {
        for (const input of inputs) {
          if (!input.value.trim()) continue;
          await post('/api/finance/ledger-settings', {
            company_id: cid(), key: input.name, value: input.value.trim()
          });
        }
        note('#fp-ls-note', 'حُفظت المتغيرات.');
      } catch (e) { note('#fp-ls-note', e.message, true); }
    });

    const req = scope.querySelector('#fp-req-form');
    if (req) req.addEventListener('submit', async ev => {
      ev.preventDefault();
      try {
        await post('/api/finance/requests', {
          company_id: cid(), kind: val(req, 'kind'), title: val(req, 'title').trim(),
          payload: { note: val(req, 'note').trim() }
        });
        note('#fp-req-note', 'حُفظ الطلب كمسودة.');
        if (refresh) await refresh();
      } catch (e) { note('#fp-req-note', e.message, true); }
    });

    // إعدادات مزوّد القراءة: النمط، النماذج الفعلية، الاختبار، الحفظ
    const aiForm = scope.querySelector('#fp-ai-form');
    if (aiForm) {
      const fillModels = (select, models) => {
        if (!select || !models || !models.length) return;
        const current = select.value;
        select.innerHTML = models.map(m => `<option value="${esc(m)}">${esc(m)}</option>`).join('');
        if (models.includes(current)) select.value = current;
      };
      scope.querySelectorAll('[data-ai-models]').forEach(btn => {
        btn.addEventListener('click', async () => {
          const which = btn.dataset.aiModels;
          const box = scope.querySelector(`[data-ai-msg="${which}"]`);
          if (box) box.textContent = 'جارٍ الجلب من الخدمة…';
          try {
            const res = await window.apiFetch(
              `/api/finance/ai/models?company_id=${cid()}&scope=${which}`);
            const data = await res.json();
            if (!res.ok || !(data.models || []).length) {
              throw new Error(data.error || 'لم تصل أي نماذج.');
            }
            fillModels(scope.querySelector(which === 'local' ? '#fp-ai-local-model' : '#fp-ai-cloud-model'), data.models);
            if (box) box.textContent = `النماذج المتاحة الآن: ${data.models.length} — ${data.models.slice(0, 6).join(' · ')}`;
          } catch (e) {
            if (box) box.textContent = e.message;
            if (box) box.classList.add('fp-error');
          }
        });
      });
      scope.querySelectorAll('[data-ai-probe]').forEach(btn => {
        btn.addEventListener('click', async () => {
          const which = btn.dataset.aiProbe;
          const box = scope.querySelector(`[data-ai-msg="${which}"]`);
          if (box) { box.textContent = 'جارٍ الاختبار…'; box.classList.remove('fp-error'); }
          try {
            const res = await window.apiFetch(
              `/api/finance/ai/probe?company_id=${cid()}&scope=${which}`);
            const data = await res.json();
            if (box) {
              box.textContent = data.message || '';
              box.classList.toggle('fp-error', !data.ok);
            }
          } catch (e) {
            if (box) { box.textContent = e.message; box.classList.add('fp-error'); }
          }
        });
      });
      aiForm.addEventListener('submit', async event => {
        event.preventDefault();
        const orderFirst = val(aiForm, 'order');
        const mode = val(aiForm, 'mode');
        try {
          const res = await window.apiFetch('/api/finance/ai/settings', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
              company_id: cid(),
              mode,
              order: orderFirst === 'cloud' ? ['cloud', 'local'] : ['local', 'cloud'],
              local_endpoint: val(aiForm, 'local_endpoint') || null,
              local_model: val(aiForm, 'local_model') || null,
              cloud_model: val(aiForm, 'cloud_model') || null,
              cloud_key: val(aiForm, 'cloud_key') || null
            })
          });
          const data = await res.json();
          if (!res.ok) throw new Error(data.error || 'تعذّر الحفظ.');
          note('#fp-ai-note', 'حُفظ الاختيار.');
          const state = scope.querySelector('#fp-ai-state');
          if (state) { state.textContent = data.note || ''; state.classList.toggle('fp-error', !data.ready); }
          if (refresh) await refresh();
        } catch (e) { note('#fp-ai-note', e.message, true); }
      });
    }

    const scanImage = scope.querySelector('#fp-scan-image-form');
    if (scanImage) scanImage.addEventListener('submit', async ev => {
      ev.preventDefault();
      const file = (scanImage.querySelector('[name="photo"]').files || [])[0];
      const host = scope.querySelector('#fp-scan-image-result');
      if (!file) { host.innerHTML = '<p class="fp-error">اختر صورة الفاتورة أولًا.</p>'; return; }
      if (file.size > 6 * 1024 * 1024) {
        host.innerHTML = '<p class="fp-error">حجم الصورة أكبر من 6 ميجابايت؛ اضغطها ثم أعد المحاولة.</p>';
        return;
      }
      host.innerHTML = '<p class="fp-note">جارٍ قراءة الصورة… قد تستغرق حتى دقيقة محليًا.</p>';
      try {
        const dataUrl = await new Promise((resolve, reject) => {
          const reader = new FileReader();
          reader.onload = () => resolve(String(reader.result || ''));
          reader.onerror = () => reject(new Error('تعذّر تحميل الصورة.'));
          reader.readAsDataURL(file);
        });
        const base64 = dataUrl.split(',')[1] || '';
        const r = await post('/api/finance/scanner/image', {
          company_id: cid(), image: base64, mime: file.type || 'image/jpeg'
        });
        const lines = (r.lines || []).map(l =>
          `<tr><td>${esc(l.description)}</td><td dir="ltr">${money(l.amount)}</td></tr>`).join('');
        host.innerHTML = `
          <div class="fp-form" style="margin-top:14px">
            <h3>نتيجة قراءة الصورة — الثقة ${esc(r.confidence)}%</h3>
            <div class="fp-grid">
              <label>التاريخ <input value="${esc(r.date || '—')}" readonly></label>
              <label>العملة <input value="${esc(r.currency || '—')}" readonly></label>
              <label>الإجمالي <input value="${esc(r.total || '—')}" readonly dir="ltr"></label>
              <label>الضريبة <input value="${esc(r.tax || '—')}" readonly dir="ltr"></label>
            </div>
            ${lines ? `<table class="fp-table"><thead><tr><th>البند</th><th>المبلغ</th></tr></thead><tbody>${lines}</tbody></table>` : ''}
            ${(r.notes || []).map(n => `<p class="fp-note">${esc(n)}</p>`).join('')}
          </div>`;
      } catch (e) {
        host.innerHTML = `<p class="fp-error">${esc(e.message)}</p>`;
      }
    });

    const scan = scope.querySelector('#fp-scan-form');
    if (scan) scan.addEventListener('submit', async ev => {
      ev.preventDefault();
      try {
        const r = await post('/api/finance/scanner/parse', {
          company_id: cid(), text: val(scan, 'text')
        });
        const lines = (r.lines || []).map(l =>
          `<tr><td>${esc(l.description)}</td><td dir="ltr">${money(l.amount)}</td></tr>`).join('');
        scope.querySelector('#fp-scan-result').innerHTML = `
          <div class="fp-form" style="margin-top:14px">
            <h3>نتيجة التحليل — الثقة ${esc(r.confidence)}%</h3>
            <div class="fp-grid">
              <label>التاريخ <input value="${esc(r.date || '—')}" readonly></label>
              <label>العملة <input value="${esc(r.currency || '—')}" readonly></label>
              <label>الإجمالي <input value="${esc(r.total || '—')}" readonly dir="ltr"></label>
              <label>الضريبة <input value="${esc(r.tax || '—')}" readonly dir="ltr"></label>
            </div>
            ${lines ? `<table class="fp-table"><thead><tr><th>البند</th><th>المبلغ</th></tr></thead><tbody>${lines}</tbody></table>` : ''}
            ${(r.notes || []).map(n => `<p class="fp-note fp-error">${esc(n)}</p>`).join('')}
          </div>`;
      } catch (e) {
        scope.querySelector('#fp-scan-result').innerHTML = `<p class="fp-error">${esc(e.message)}</p>`;
      }
    });

    // أزرار القرار والأرشفة
    const actions = {
      'adj-approve': (id) => post('/api/finance/adjustments/decide', { company_id: cid(), id, approve: true }),
      'adj-reject': (id) => post('/api/finance/adjustments/decide', { company_id: cid(), id, approve: false }),
      'doc-approve': (id) => post('/api/finance/doc-reviews/decide', { company_id: cid(), id, approve: true }),
      'doc-reject': (id) => post('/api/finance/doc-reviews/decide', { company_id: cid(), id, approve: false }),
      'req-submit': (id) => post('/api/finance/requests/submit', { company_id: cid(), id }),
      'req-approve': (id) => post('/api/finance/requests/decide', { company_id: cid(), id, approve: true }),
      'req-reject': (id) => post('/api/finance/requests/decide', { company_id: cid(), id, approve: false }),
      'plan-archive': (id) => post('/api/finance/archive/plans', { company_id: cid(), id }),
      'country-archive': (id) => post('/api/finance/archive/countries', { company_id: cid(), id }),
      'inter-archive': (id) => post('/api/finance/archive/intermediate', { company_id: cid(), id })
    };
    scope.querySelectorAll('[data-act]').forEach(btn => {
      const fn = actions[btn.dataset.act];
      if (!fn) return;
      btn.addEventListener('click', async () => {
        btn.disabled = true;
        try {
          await fn(Number(btn.dataset.id));
          if (refresh) await refresh();
        } catch (e) {
          window.alert(e.message);
          btn.disabled = false;
        }
      });
    });
  }

  window.FinanceOps4 = {
    tabs: [
      ['adjustments', 'التسويات المحاسبية'],
      ['docreview', 'مراجعة الوثائق'],
      ['plans', 'المخططات والعمولات'],
      ['countries', 'بيانات الدول'],
      ['intermediate', 'الحسابات الوسيطة'],
      ['ledgersettings', 'متغيرات الأستاذ'],
      ['requests', 'طلبات القيود والسندات'],
      ['scanner', 'قراءة الفواتير المصورة']
    ],
    has: tab => Object.prototype.hasOwnProperty.call(views, tab),
    view: tab => views[tab](),
    bind
  };
})();
