// تبويبات التشغيل المالي: الأطراف، الشيكات، سير الاعتماد، الإقفال السنوي.
// تعمل داخل نافذة «المالية المتقدمة» وتستخدم نفس جلسة /api/finance/*.
(function () {
  'use strict';
  const esc = v => String(v == null ? '' : v)
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  const money = v => Number(v || 0).toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 4 });
  const cid = () => Number(window.FinanceApi.companyId());
  const today = () => new Date().toISOString().slice(0, 10);

  const TABS = [
    ['parties', 'الأطراف'],
    ['checks', 'الشيكات'],
    ['approval', 'سير الاعتماد'],
    ['closing', 'الإقفال السنوي']
  ];

  async function get(path) {
    const res = await window.apiFetch(path);
    if (!res.ok) {
      let msg = 'تعذّر تحميل البيانات.';
      try { msg = (await res.json()).error || msg; } catch (_) { /* جسم غير JSON */ }
      throw new Error(msg);
    }
    return res.json();
  }

  async function post(path, body) {
    const res = await window.apiFetch(path, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body)
    });
    if (!res.ok) {
      let msg = 'تعذّر تنفيذ العملية.';
      try { msg = (await res.json()).error || msg; } catch (_) { /* جسم غير JSON */ }
      throw new Error(msg);
    }
    return res.json().catch(() => ({}));
  }

  function table(headers, rows, empty = 'لا توجد بيانات بعد.') {
    if (!rows.length) return `<p class="fp-empty">${esc(empty)}</p>`;
    return `<div class="fp-table-wrap"><table class="fp-table"><thead><tr>${
      headers.map(h => `<th>${esc(h)}</th>`).join('')}</tr></thead><tbody>${
      rows.map(r => `<tr>${r.map(c => `<td>${c}</td>`).join('')}</tr>`).join('')
    }</tbody></table></div>`;
  }

  const STATE_LABEL = {
    pending: 'مسودة',
    submitted: 'بانتظار اعتماد',
    approved: 'معتمد',
    rejected: 'مرفوض',
    posted: 'مرحّل',
    registered: 'مسجّل',
    deposited: 'مودع',
    collected: 'محصّل',
    bounced: 'مرتجع',
    endorsed: 'مُظهر',
    cancelled: 'ملغى'
  };

  const CHECK_ACTIONS = {
    received: [['deposited', 'إيداع'], ['collected', 'تحصيل'], ['bounced', 'ارتجاع'], ['endorsed', 'تظهير'], ['cancelled', 'إلغاء']],
    issued: [['deposited', 'تسليم'], ['collected', 'صرف'], ['bounced', 'ارتجاع'], ['cancelled', 'إلغاء']]
  };

  const views = {
    async parties() {
      const [items, accounts] = await Promise.all([
        get(`/api/finance/parties?company_id=${cid()}`),
        get(`/api/finance/ledger?company_id=${cid()}`)
      ]);
      const details = (accounts.accounts || []).filter(a => a.kind === 'detail');
      // الحساب المرتبط بطرف قائم لا يُعرض مرة أخرى: القيد يمنع التكرار فيرفضه الخادم.
      const used = new Set(items.map(p => Number(p.account_id)));
      const free = details.filter(a => !used.has(Number(a.account_id)));
      const rows = items.map(p => [
        esc(p.name), esc(p.role === 'customer' ? 'عميل' : p.role === 'supplier' ? 'مورد' : 'أخرى'),
        esc(p.account || '—')
      ]);
      return `<details class="fp-form"><summary>＋ إضافة طرف</summary>
        <form data-ops="party">
          <label>الاسم<input name="name" required maxlength="120"></label>
          <label>التصنيف<select name="role">
            <option value="customer">عميل</option><option value="supplier">مورد</option><option value="other">أخرى</option>
          </select></label>
          <label>الحساب<select name="account_id" required>${free.map(a =>
            `<option value="${esc(a.account_id)}">${esc(a.code)} · ${esc(a.name)}</option>`).join('')}</select></label>
          <button type="submit" class="fp-btn">حفظ الطرف</button>
          <p class="fp-form-error" role="alert"></p>
        </form></details>` +
        `<p class="fp-note">${items.length} طرفًا مرتبطًا بهذه الشركة، كلٌّ بحساب تفصيلي مستقل. الحسابات المستعملة لأطراف أخرى لا تظهر في القائمة (${free.length} حسابًا متاحًا).</p>` +
        table(['الاسم', 'التصنيف', 'الحساب'], rows);
    },

    async checks() {
      const [items, funds, parties] = await Promise.all([
        get(`/api/finance/checks?company_id=${cid()}`),
        get(`/api/finance/funds?company_id=${cid()}`),
        get(`/api/finance/parties?company_id=${cid()}`)
      ]);
      const rows = items.map(c => {
        const actions = (CHECK_ACTIONS[c.direction] || [])
          .filter(([status]) => status !== c.status && !(c.status === 'collected' && status !== c.status))
          .map(([status, label]) => `<button type="button" class="fp-ghost" data-check="${esc(c.id)}" data-to="${status}">${label}</button>`)
          .join(' ');
        return [
          esc(c.direction === 'received' ? 'وارد' : 'صادر'),
          esc(c.number), esc(c.bank), esc(c.drawer),
          money(c.amount) + ' ' + esc(c.currency),
          esc(c.due_date),
          `<span class="fp-state${c.status === 'collected' ? ' ok' : ''}">${esc(STATE_LABEL[c.status] || c.status)}</span>`,
          c.entry_id ? `#${esc(c.entry_id)}` : '—',
          actions
        ];
      });
      const open = items.filter(c => c.status !== 'collected' && c.status !== 'cancelled').length;
      return `<details class="fp-form"><summary>＋ تسجيل شيك</summary>
        <form data-ops="check">
          <label>الاتجاه<select name="direction">
            <option value="received">وارد (من عميل)</option><option value="issued">صادر (لمورد)</option>
          </select></label>
          <label>رقم الشيك<input name="number" required maxlength="40"></label>
          <label>البنك<input name="bank" required maxlength="80"></label>
          <label>الساحب<input name="drawer" required maxlength="120"></label>
          <label>المبلغ<input name="amount" required inputmode="decimal"></label>
          <label>العملة<input name="currency" value="YER" maxlength="8" required></label>
          <label>الاستحقاق<input type="date" name="due_date" value="${today()}" required></label>
          <label>الصندوق<select name="fund_id"><option value="">— بلا صندوق —</option>${funds.map(f =>
            `<option value="${esc(f.id)}">${esc(f.code)} · ${esc(f.name)}</option>`).join('')}</select></label>
          <label>الطرف<select name="party_id"><option value="">— بلا طرف —</option>${parties.map(p =>
            `<option value="${esc(p.id)}">${esc(p.name)}</option>`).join('')}</select></label>
          <button type="submit" class="fp-btn">حفظ الشيك</button>
          <p class="fp-form-error" role="alert"></p>
        </form></details>` +
        `<p class="fp-note">${items.length} شيكًا · ${open} قيد التحصيل · التحصيل يُنشئ قيدًا محاسبيًا متوازنًا تلقائيًا.</p>` +
        table(['الاتجاه', 'الرقم', 'البنك', 'الساحب', 'المبلغ', 'الاستحقاق', 'الحالة', 'القيد', 'إجراء'], rows);
    },

    async approval() {
      const [policy, flow] = await Promise.all([
        get(`/api/finance/approval/policy?company_id=${cid()}`),
        get(`/api/finance/approval?company_id=${cid()}`)
      ]);
      const rows = flow.slice(0, 80).map(f => {
        const actions = [];
        if (f.state === 'pending' || f.state === 'rejected') {
          actions.push(`<button type="button" class="fp-ghost" data-flow="${esc(f.draft_id)}" data-to="submit">إرسال</button>`);
        }
        if (f.state === 'submitted') {
          actions.push(`<button type="button" class="fp-ghost" data-flow="${esc(f.draft_id)}" data-to="approve">اعتماد</button>`);
          actions.push(`<button type="button" class="fp-ghost" data-flow="${esc(f.draft_id)}" data-to="reject">رفض</button>`);
        }
        return [
          '#' + esc(f.draft_id), esc(f.date), esc(f.description), money(f.total),
          `<span class="fp-state${f.state === 'approved' || f.state === 'posted' ? ' ok' : ''}">${esc(STATE_LABEL[f.state] || f.state)}</span>`,
          esc(f.note || '—'),
          actions.join(' ') || '—'
        ];
      });
      return `<details class="fp-form"><summary>⚙ سياسة الاعتماد</summary>
        <form data-ops="policy">
          <label>الاعتماد قبل الترحيل<select name="require_approval">
            <option value="1"${policy.require_approval ? ' selected' : ''}>مطلوب</option>
            <option value="0"${policy.require_approval ? '' : ' selected'}>غير مطلوب</option>
          </select></label>
          <button type="submit" class="fp-btn">تحديث السياسة</button>
          <p class="fp-form-error" role="alert"></p>
        </form></details>
        <p class="fp-note">${policy.require_approval
          ? 'السياسة مفعّلة: لا يُرحّل قيد قبل اعتماده، ولا يعتمد المُنشئ قيده.'
          : 'السياسة متوقفة: الترحيل المباشر متاح للمدير المالي.'} · ${flow.length} قيدًا في السجل.</p>` +
        table(['#', 'التاريخ', 'البيان', 'المبلغ', 'الحالة', 'ملاحظة', 'إجراء'], rows);
    },

    async closing() {
      const items = await get(`/api/finance/closing?company_id=${cid()}`);
      const rows = items.map(c => [
        esc(c.fiscal_year), money(c.net_income), '#' + esc(c.entry_id), esc(c.closed_at), esc(c.notes || '—')
      ]);
      const year = new Date().getFullYear();
      return `<details class="fp-form"><summary>＋ إقفال سنة مالية</summary>
        <form data-ops="closing">
          <label>السنة المالية<input name="fiscal_year" type="number" min="1900" max="2999" value="${year}" required></label>
          <label>العملة<input name="currency" value="YER" maxlength="8" required></label>
          <label>ملاحظة<input name="notes" maxlength="120"></label>
          <button type="submit" class="fp-btn">تنفيذ الإقفال</button>
          <p class="fp-form-error" role="alert"></p>
        </form></details>
        <p class="fp-note">الإقفال يُصفّر حسابات الإيرادات والمصروفات في الأرباح المحتجزة، ويقفل الفترة، ولا يمكن التراجع عنه.</p>` +
        table(['السنة', 'صافي الربح', 'القيد', 'وقت الإقفال', 'ملاحظة'], rows, 'لا توجد سنوات مقفلة بعد.');
    }
  };

  const SUBMIT = {
    async party(form) {
      const d = new FormData(form);
      return post('/api/finance/parties', {
        company_id: cid(), name: d.get('name'), role: d.get('role'),
        account_id: Number(d.get('account_id'))
      });
    },
    async check(form) {
      const d = new FormData(form);
      return post('/api/finance/checks', {
        company_id: cid(), direction: d.get('direction'), number: d.get('number'),
        bank: d.get('bank'), drawer: d.get('drawer'), amount: d.get('amount'),
        currency: d.get('currency'), due_date: d.get('due_date'),
        fund_id: d.get('fund_id') ? Number(d.get('fund_id')) : null,
        party_id: d.get('party_id') ? Number(d.get('party_id')) : null
      });
    },
    async policy(form) {
      const d = new FormData(form);
      return post('/api/finance/approval/policy', {
        company_id: cid(), require_approval: d.get('require_approval') === '1'
      });
    },
    async closing(form) {
      const d = new FormData(form);
      return post('/api/finance/closing', {
        company_id: cid(), fiscal_year: Number(d.get('fiscal_year')),
        currency: d.get('currency'), notes: d.get('notes') || null
      });
    }
  };

  async function act(path, body) {
    return post(path, body);
  }

  function bind(tab, scope, reload) {
    scope.querySelectorAll('form[data-ops]').forEach(form => {
      const kind = form.dataset.ops;
      form.addEventListener('submit', async event => {
        event.preventDefault();
        const error = form.querySelector('.fp-form-error');
        const button = form.querySelector('button[type=submit]');
        error.textContent = '';
        button.disabled = true;
        try {
          await SUBMIT[kind](form);
          reload();
        } catch (e) {
          error.textContent = e.message || 'تعذّر الحفظ.';
        } finally {
          button.disabled = false;
        }
      });
    });
    scope.querySelectorAll('[data-check]').forEach(btn => {
      btn.addEventListener('click', async () => {
        btn.disabled = true;
        try {
          await act(`/api/finance/checks/${btn.dataset.check}/status`, {
            company_id: cid(), status: btn.dataset.to
          });
          reload();
        } catch (e) {
          btn.disabled = false;
          const note = scope.querySelector('.fp-error, .fp-note');
          if (note) note.textContent = e.message;
        }
      });
    });
    scope.querySelectorAll('[data-flow]').forEach(btn => {
      btn.addEventListener('click', async () => {
        btn.disabled = true;
        try {
          await act(`/api/finance/journals/${btn.dataset.flow}/${btn.dataset.to}`, {
            company_id: cid()
          });
          reload();
        } catch (e) {
          btn.disabled = false;
          const note = scope.querySelector('.fp-error, .fp-note');
          if (note) note.textContent = e.message;
        }
      });
    });
  }

  window.FinanceOps = {
    tabs: TABS,
    has: tab => Object.prototype.hasOwnProperty.call(views, tab),
    view: tab => views[tab](),
    bind
  };
})();
