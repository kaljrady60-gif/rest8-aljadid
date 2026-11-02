// لوحة «المالية المتقدمة»: التقارير، الافتتاحي، الصناديق، العملات، الأبعاد،
// التسوية البنكية، سجل التدقيق. كل البيانات من /api/finance/* المحمية.
(function () {
  'use strict';
  const $ = sel => document.querySelector(sel);
  const esc = v => String(v == null ? '' : v)
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  const money = v => Number(v || 0).toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 4 });

  const CORE_TABS = [
    ['journals', 'القيود اليومية'],
    ['reports', 'التقارير المالية'],
    ['opening', 'الأرصدة الافتتاحية'],
    ['funds', 'الصناديق والبنوك'],
    ['fx', 'العملات وأسعار الصرف'],
    ['dimensions', 'مراكز التكلفة'],
    ['bankrec', 'التسوية البنكية'],
    ['audit', 'سجل التدقيق']
  ];
  const tabs = () => CORE_TABS
    .concat(window.FinanceOps ? window.FinanceOps.tabs : [])
    .concat(window.FinanceOps2 ? window.FinanceOps2.tabs : [])
    .concat(window.FinanceOps3 ? window.FinanceOps3.tabs : [])
    .concat(window.FinanceOps4 ? window.FinanceOps4.tabs : [])
    .concat(window.FinanceOps5 ? window.FinanceOps5.tabs : [])
    .concat(window.FinanceOps6 ? window.FinanceOps6.tabs : []);
  // يفوّض التبويب لأول وحدة مسجّلة تملكه: التشغيل المالي ثم الأصول والرواتب.
  const unitFor = name =>
    [window.FinanceOps, window.FinanceOps2, window.FinanceOps3, window.FinanceOps4, window.FinanceOps5, window.FinanceOps6]
      .find(m => m && m.has(name));

  const api = async (path) => {
    const res = await window.apiFetch(path);
    if (!res.ok) {
      let msg = 'تعذّر تحميل البيانات.';
      try { msg = (await res.json()).error || msg; } catch (_) { /* جسم غير JSON */ }
      throw new Error(msg);
    }
    return res.json();
  };
  const company = () => window.FinanceApi && window.FinanceApi.companyId();
  const q = extra => `?company_id=${company()}${extra || ''}`;

  // كل تبويب يبدأ بالتأكد من وجود شركة مسندة قبل أي طلب.
  async function requireCompany() {
    const id = window.FinanceApi && (await window.FinanceApi.ensureCompany());
    if (!id) throw new Error('لا توجد شركة مسندة لهذا المستخدم بعد.');
    return id;
  }

  function table(headers, rows) {
    if (!rows.length) return '<p class="fp-empty">لا توجد بيانات بعد.</p>';
    return `<div class="fp-table-wrap"><table class="fp-table"><thead><tr>${
      headers.map(h => `<th>${esc(h)}</th>`).join('')}</tr></thead><tbody>${
      rows.map(r => `<tr>${r.map(c => `<td>${c}</td>`).join('')}</tr>`).join('')
    }</tbody></table></div>`;
  }

  const views = {
    async reports() {
      const ctx = window.FinanceContext || { reports: [] };
      const options = (ctx.reports || []).map(r => `<option value="${esc(r.id)}">${esc(r.title)}</option>`).join('');
      const branches = await api(`/api/finance/branches${q()}`).catch(() => []);
      const data = await api(`/api/finance/statements${q('&report=trial')}`);
      const rows = (data.accounts || []).map(a => [esc(a.code), esc(a.name), money(a.debit), money(a.credit), money(a.balance)]);
      const branchOptions = ['<option value="">كل الفروع (موحّد)</option>'].concat(
        (branches || []).map(b => `<option value="${esc(b.id)}"${String(window.FinanceApi.branchId()) === String(b.id) ? ' selected' : ''}>${esc(b.code)} · ${esc(b.name)}</option>`)
      ).join('');
      const scoped = extra => window.FinanceApi.scopedUrl('/api/finance/' + extra.path, extra.params || {});
      return `<div class="fp-toolbar">
          <label>التقرير <select id="fp-report">${options}</select></label>
          <label>الفرع <select id="fp-branch">${branchOptions}</select></label>
          <label>من <input type="date" id="fp-from"></label>
          <label>إلى <input type="date" id="fp-to"></label>
          <button type="button" class="fp-btn" id="fp-run">تشغيل</button>
          <a class="fp-ghost" id="fp-export" target="_blank" rel="noopener"
             href="${esc(scoped({ path: 'export', params: { kind: 'trial' } }))}">تصدير CSV</a>
          <button type="button" class="fp-ghost" id="fp-print">طباعة / PDF</button>
        </div>
        <div id="fp-report-body">${table(['الرمز', 'الحساب', 'مدين', 'دائن', 'الرصيد'], rows)}
        <p class="fp-note">المجموع: مدين ${money(data.totals.debit)} · دائن ${money(data.totals.credit)} · ${
          data.totals.balanced ? 'متوازن ✔' : 'غير متوازن ✘'} · ${
          window.FinanceApi.branchId() ? 'مقيّد بالفرع المحدد' : 'كل الفروع'}</p></div>`;
    },
    async journals() {
      const [items, dims] = await Promise.all([
        api(`/api/finance/journals${q()}`),
        api(`/api/finance/dimensions${q()}`).catch(() => [])
      ]);
      const dimsBar = (dims || []).filter(d => !d.archived_at).length
        ? `<div class="fp-dims"><span>الأبعاد عند الترحيل:</span>${dims.filter(d => !d.archived_at).map(d =>
            `<label><input type="checkbox" data-dim value="${esc(d.id)}"> ${esc(d.code)} · ${esc(d.name)}</label>`).join('')}</div>`
        : '';
      const rows = (items || []).slice(0, 60).map(j => [
        esc(j.id), esc(j.date), esc(j.description), esc(j.currency), money(j.total),
        j.posted
          ? '<span class="fp-state ok">مرحّل</span>'
          : `<span class="fp-state">مسودة</span> <button type="button" class="fp-ghost" data-post="${esc(j.id)}">ترحيل</button>`
      ]);
      const posted = (items || []).filter(j => j.posted).length;
      return dimsBar + `<p class="fp-note">${items.length} قيدًا · ${posted} مرحّل · ${items.length - posted} مسودة</p>` +
        table(['#', 'التاريخ', 'البيان', 'العملة', 'المبلغ', 'الحالة'], rows);
    },
    async opening() {
      const data = await api(`/api/finance/opening${q()}`);
      const rows = (data.lines || []).map(l => [esc(l.account_code), esc(l.account_name), money(l.debit), money(l.credit)]);
      return `<p class="fp-note">${data.locked
        ? 'مقفلة: توجد قيود مرحّلة، فلا يمكن تعديل الأرصدة الافتتاحية.'
        : 'مفتوحة للتعديل.'} · العملة ${esc(data.currency)}</p>
        ${table(['الرمز', 'الحساب', 'مدين', 'دائن'], rows)}`;
    },
    async funds() {
      const data = await api(`/api/finance/funds${q()}`);
      const rows = (data || []).map(f => [
        esc(f.kind === 'cash' ? 'صندوق' : 'بنك'), esc(f.code), esc(f.name),
        esc(f.account_label), esc(f.currency), money(f.balance)
      ]);
      return table(['النوع', 'الرمز', 'الاسم', 'الحساب', 'العملة', 'الرصيد'], rows);
    },
    async fx() {
      const data = await api(`/api/finance/fx${q()}`);
      const rows = (data || []).map(r => [esc(r.base), esc(r.quote), esc(r.rate), esc(r.as_of)]);
      return table(['من', 'إلى', 'السعر', 'التاريخ'], rows);
    },
    async dimensions() {
      const data = await api(`/api/finance/dimensions${q()}`);
      const rows = (data || []).map(d => [
        esc(d.code), esc(d.name),
        esc(d.kind === 'cost_center' ? 'مركز تكلفة' : 'مشروع'),
        d.archived_at ? 'مؤرشف' : 'نشط'
      ]);
      return table(['الرمز', 'الاسم', 'النوع', 'الحالة'], rows);
    },
    async bankrec() {
      const data = await api(`/api/finance/bankrec${q()}`);
      const rows = (data || []).map(s => [
        esc(s.fund_name), esc(s.period_start) + ' → ' + esc(s.period_end),
        money(s.statement_balance), money(s.cleared_total), money(s.pending_total), money(s.difference)
      ]);
      return table(['البنك', 'الفترة', 'رصيد الكشف', 'مُسوّى', 'معلّق', 'الفرق'], rows);
    },
    async audit() {
      const rows = (await api(`/api/finance/audit${q('&limit=50')}`) || []).map(e => [
        esc(e.action), esc(e.entity), esc(e.entity_id || '—'), esc(e.at)
      ]);
      return table(['العملية', 'الجدول', 'المعرّف', 'الوقت'], rows);
    }
  };

  /// روابط التقرير تتبع اختيار الفرع والتاريخ فورًا (تصدير + طباعة).
  function bindReports(scope, rerun) {
    const branch = scope.querySelector('#fp-branch');
    const from = scope.querySelector('#fp-from');
    const to = scope.querySelector('#fp-to');
    const report = scope.querySelector('#fp-report');
    const refresh = () => {
      const params = {};
      if (from && from.value) params.from = from.value;
      if (to && to.value) params.to = to.value;
      const kind = report ? report.value : 'trial';
      const exporter = scope.querySelector('#fp-export');
      if (exporter) {
        exporter.href = window.FinanceApi.scopedUrl('/api/finance/export',
          { ...params, kind });
      }
      const printer = scope.querySelector('#fp-print');
      if (printer) {
        printer.dataset.href = window.FinanceApi.scopedUrl('/api/finance/print',
          { ...params, report: kind });
      }
      const run = scope.querySelector('#fp-run');
      if (run) run.dataset.href = window.FinanceApi.scopedUrl('/api/finance/statements',
        { ...params, report: kind });
    };
    if (branch) {
      branch.addEventListener('change', () => {
        window.FinanceApi.setBranch(branch.value || null);
        rerun();
      });
    }
    [from, to, report].forEach(node => node && node.addEventListener('change', refresh));
    const printer = scope.querySelector('#fp-print');
    if (printer) {
      printer.addEventListener('click', () => {
        refresh();
        window.open(printer.dataset.href || '/api/finance/print', '_blank', 'noopener');
      });
    }
    refresh();
  }

  async function render(tab, host) {
    const body = host || $('#fp-body');
    if (!body) return;
    body.innerHTML = '<p class="fp-empty">جارٍ التحميل…</p>';
    try {
      await requireCompany();
      const unit = views[tab] ? null : unitFor(tab);
      const bar = window.FinanceForms ? window.FinanceForms.toolbar(tab) : '';
      body.innerHTML = bar + (unit ? await unit.view(tab) : await views[tab]());
      if (unit) unit.bind(tab, body, () => render(tab));
      else if (window.FinanceForms) window.FinanceForms.bind(tab, body, () => render(tab));
      if (tab === 'reports') bindReports(body, () => render(tab));
      body.dataset.tab = tab;
      if (window.FinanceNav) window.FinanceNav.enhance();
      body.querySelectorAll('[data-post]').forEach(btn => {
        btn.addEventListener('click', async () => {
          btn.disabled = true;
          try {
            await window.apiFetch(`/api/finance/journals/${btn.dataset.post}/post`, {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({
                company_id: Number(company()),
                dimensions: Array.from(body.querySelectorAll('[data-dim]:checked')).map(n => Number(n.value))
              })
            }).then(async r => {
              if (!r.ok) throw new Error((await r.json().catch(() => ({}))).error || 'تعذّر الترحيل.');
            });
            render(tab);
          } catch (e) {
            btn.disabled = false;
            const note = body.querySelector('.fp-note, .fp-error');
            if (note) note.textContent = e.message;
          }
        });
      });
      const run = $('#fp-run');
      if (run) run.addEventListener('click', () => runReport($('#fp-report').value));
      const pick = $('#fp-report');
      const link = $('#fp-export');
      if (pick && link) {
        const sync = () => {
          link.href = `/api/finance/export${q('&kind=' + encodeURIComponent(pick.value))}`;
        };
        pick.addEventListener('change', sync);
        sync();
      }
    } catch (e) {
      body.innerHTML = `<p class="fp-error">${esc(e.message)}</p>`;
    }
  }

  async function runReport(id) {
    const body = $('#fp-report-body');
    if (!body) return;
    body.innerHTML = '<p class="fp-empty">جارٍ التحميل…</p>';
    try {
      await requireCompany();
      const data = await api(`/api/finance/statements${q('&report=' + encodeURIComponent(id))}`);
      if (id === 'income') {
        const rows = (data.sections || []).flatMap(s => (s.accounts || []).map(a => [
          esc(a.code), esc(a.name), esc(s.category), money(a.balance)
        ]));
        body.innerHTML = table(['الرمز', 'الحساب', 'التصنيف', 'الرصيد'], rows) +
          `<p class="fp-note">صافي الربح: ${money(data.net_profit)} · العملة ${esc(data.currency)}</p>`;
      } else if (id === 'balance') {
        const rows = (data.sections || []).flatMap(s => (s.accounts || []).map(a => [
          esc(a.code), esc(a.name), esc(s.category), money(a.balance)
        ]));
        body.innerHTML = table(['الرمز', 'الحساب', 'التصنيف', 'الرصيد'], rows) +
          `<p class="fp-note">الأصول ${money(data.assets_total)} · الالتزامات+الملكية ${money(data.liabilities_equity_total)} · ${
            data.balanced ? 'متوازنة ✔' : 'غير متوازنة ✘'}</p>`;
      } else if (id === 'general') {
        const rows = (data.entries || []).map(e => [esc(e.id), esc(e.date), esc(e.description), esc(e.currency), money(e.total)]);
        body.innerHTML = table(['#', 'التاريخ', 'البيان', 'العملة', 'المبلغ'], rows);
      } else if (id === 'cashflow') {
        const rows = (data.accounts || []).map(a => [esc(a.code), esc(a.name), money(a.net)]);
        body.innerHTML = table(['الرمز', 'الحساب', 'الصافي'], rows) +
          `<p class="fp-note">داخل ${money(data.inflow)} · خارج ${money(data.outflow)} · الصافي ${money(data.net)}</p>`;
      } else {
        const rows = (data.accounts || []).map(a => [esc(a.code), esc(a.name), money(a.debit), money(a.credit), money(a.balance)]);
        body.innerHTML = table(['الرمز', 'الحساب', 'مدين', 'دائن', 'الرصيد'], rows);
      }
    } catch (e) {
      body.innerHTML = `<p class="fp-error">${esc(e.message)}</p>`;
    }
  }

  function openPanel() {
    let panel = $('#finance-panels');
    if (!panel) {
      panel = document.createElement('section');
      panel.id = 'finance-panels';
      panel.className = 'fp-overlay';
      panel.setAttribute('role', 'dialog');
      panel.setAttribute('aria-label', 'المالية المتقدمة');
      panel.innerHTML = `<div class="fp-dialog" dir="rtl">
        <header class="fp-head"><h2>المالية المتقدمة</h2>
          <button type="button" class="fp-close" aria-label="إغلاق" id="fp-close" aria-label="إغلاق">×</button></header>
        <nav class="fp-tabs">${tabs().map(([id, label], i) =>
          `<button type="button" data-fp-tab="${id}" class="${i === 0 ? 'active' : ''}">${label}</button>`).join('')}</nav>
        <div class="fp-body" id="fp-body"></div></div>`;
      document.body.appendChild(panel);
      panel.querySelector('#fp-close').addEventListener('click', () => panel.classList.remove('open'));
      panel.addEventListener('click', ev => {
        if (ev.target === panel) panel.classList.remove('open');
      });
      panel.querySelectorAll('[data-fp-tab]').forEach(btn => {
        btn.addEventListener('click', () => {
          panel.querySelectorAll('[data-fp-tab]').forEach(b => b.classList.toggle('active', b === btn));
          render(btn.dataset.fpTab);
        });
      });
    }
    panel.classList.add('open');
    render(panel.querySelector('[data-fp-tab].active').dataset.fpTab);
  }


  // اللوحة تُعرض الآن داخل شاشة «المالية» الأصلية لا في نافذة منفصلة.
  function hubMarkup() {
    // شريط التبويبات الوحيد هو شريط شاشة المالية الأصلية؛ هنا الجسم فقط.
    return '<div class="fp-body" id="fp-body"></div>';
  }

  let hubBound = false;
  window.FinanceHub = {
    tabs: () => tabs(),
    show(tab) {
      const host = document.getElementById('finance-hub');
      if (!host) return;
      if (!hubBound) { host.innerHTML = hubMarkup(); hubBound = true; }
      const target = tabs().some(([id]) => id === tab) ? tab : tabs()[0][0];
      render(target, host.querySelector('#fp-body'));
    }
  };

  function injectLauncher() {
    return; // أُزيلت نافذة «المالية المتقدمة»؛ اللوحة تُدمج داخل شاشة المالية الأصلية.
    const host = document.querySelector('.topbar-actions, header .actions, header');
    if (!host || document.getElementById('fp-open')) return;
    const btn = document.createElement('button');
    btn.id = 'fp-open';
    btn.type = 'button';
    btn.className = 'fp-launcher';
    btn.textContent = 'المالية المتقدمة';
    btn.setAttribute('data-help', 'التقارير المالية والأرصدة الافتتاحية والصناديق والعملات ومراكز التكلفة والتسوية البنكية وسجل التدقيق.');
    btn.addEventListener('click', openPanel);
    host.appendChild(btn);
  }

  document.addEventListener('DOMContentLoaded', () => {
    injectLauncher();
    // فتح اللوحة من فهرس الشاشات للشاشات المنفَّذة حديثًا.
    document.addEventListener('click', ev => {
      const target = ev.target.closest('[data-screen], [data-open]');
      if (!target) return;
      const id = target.dataset.screen || '';
      if (['fin.opening', 'fin.cashboxes', 'fin.banks', 'fin.currencies', 'fin.costcenters', 'fin.reports', 'fin.bankrec'].includes(id)) {
        ev.preventDefault();
        ev.stopPropagation();
        openPanel();
      }
    }, true);
  });
})();
