// طبقة تنقّل موحّدة لكل تبويبات المالية: نافذة تفاصيل منبثقة، تبديل عرض
// جدول/بطاقات، اختصارات لوحة مفاتيح، وشريط مؤشرات سريع.
// لا تكرّر منطق أي تبويب: تُضيف سلوكًا فوق الجداول والنماذج الموجودة.
(function () {
  'use strict';
  const esc = v => String(v == null ? '' : v)
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  const money = v => Number(v || 0).toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 4 });
  const cid = () => Number(window.FinanceApi.companyId());
  const TAB_ORDER = ['reports', 'journals', 'opening', 'funds', 'fx', 'dimensions',
    'bankrec', 'audit', 'parties', 'checks', 'approval', 'closing', 'assets', 'payroll', 'branches'];

  // ---- نافذة التفاصيل ------------------------------------------------------
  function dialog() {
    let node = document.getElementById('fn-dialog');
    if (node) return node;
    node = document.createElement('dialog');
    node.id = 'fn-dialog';
    node.className = 'fn-dialog';
    node.innerHTML = '<div class="fn-dialog-head"><h3 id="fn-title"></h3>' +
      '<button type="button" class="fp-ghost" id="fn-close" aria-label="إغلاق">✕</button></div>' +
      '<div class="fn-dialog-body" id="fn-body"></div>';
    document.body.appendChild(node);
    node.querySelector('#fn-close').addEventListener('click', () => node.close());
    node.addEventListener('click', event => {
      if (event.target === node) node.close(); // نقرة على الخلفية
    });
    return node;
  }

  function open(title, html) {
    const node = dialog();
    node.querySelector('#fn-title').textContent = title;
    node.querySelector('#fn-body').innerHTML = html;
    if (!node.open) node.showModal();
  }

  function close() {
    const node = document.getElementById('fn-dialog');
    if (node && node.open) node.close();
  }

  function loading(title) {
    open(title, '<p class="fp-empty">جارٍ التحميل…</p>');
  }

  function errorText(error) {
    return `<p class="fp-error">${esc(error || 'تعذّر تحميل التفاصيل.')}</p>`;
  }

  function tableHTML(headers, rows) {
    if (!rows.length) return '<p class="fp-empty">لا توجد سطور.</p>';
    return `<div class="fp-table-wrap"><table class="fp-table"><thead><tr>${
      headers.map(h => `<th>${esc(h)}</th>`).join('')}</tr></thead><tbody>${
      rows.map(r => `<tr>${r.map(c => `<td>${c}</td>`).join('')}</tr>`).join('')
    }</tbody></table></div>`;
  }

  async function fetchJSON(path) {
    const res = await window.apiFetch(path);
    if (!res.ok) {
      let msg = 'تعذّر تحميل التفاصيل.';
      try { msg = (await res.json()).error || msg; } catch (_) { /* جسم غير JSON */ }
      throw new Error(msg);
    }
    return res.json();
  }

  // ---- تفاصيل القيد --------------------------------------------------------
  async function entryDetail(id) {
    loading('تفاصيل القيد #' + id);
    try {
      const data = await fetchJSON(`/api/finance/journals/${id}?company_id=${cid()}`);
      const rows = data.lines.map(l => [
        esc(l.line_no), esc(l.account || '—'), money(l.debit), money(l.credit)
      ]);
      open(`تفاصيل القيد #${id}`, `
        <dl class="fn-facts">
          <div><dt>البيان</dt><dd>${esc(data.description)}</dd></div>
          <div><dt>التاريخ</dt><dd>${esc(data.date)}</dd></div>
          <div><dt>العملة</dt><dd>${esc(data.currency)}</dd></div>
          <div><dt>المبلغ</dt><dd>${money(data.total)}</dd></div>
          <div><dt>الحالة</dt><dd>${data.posted ? 'مرحّل' : 'مسودة'}${
            data.entry_id ? ' · قيد #' + esc(data.entry_id) : ''}</dd></div>
          <div><dt>الأبعاد</dt><dd>${data.dimensions.length ? esc(data.dimensions.join(' · ')) : '—'}</dd></div>
        </dl>` + tableHTML(['#', 'الحساب', 'مدين', 'دائن'], rows));
    } catch (e) {
      open('تفاصيل القيد #' + id, errorText(e.message));
    }
  }

  // ---- تفاصيل مسير الرواتب -------------------------------------------------
  async function runDetail(id) {
    loading('تفاصيل المسير');
    try {
      const lines = await fetchJSON(`/api/finance/payroll/runs/${id}/lines?company_id=${cid()}`);
      const rows = lines.map(l => [
        esc(l.name), money(l.base), money(l.allowances), money(l.deductions), money(l.net)
      ]);
      open('كشف رواتب المسير #' + id, tableHTML(
        ['الموظف', 'الأساسي', 'البدلات', 'الاستقطاعات', 'الصافي'], rows));
    } catch (e) {
      open('تفاصيل المسير #' + id, errorText(e.message));
    }
  }

  /// تفاصيل عامة لأي سجل: تعرض كل خلايا الصف بعناوين أعمدتها.
  function rowDetail(row) {
    const facts = Array.from(row.children)
      .filter(cell => (cell.dataset.label || '').length && cell.textContent.trim().length)
      .map(cell => `<div><dt>${esc(cell.dataset.label)}</dt><dd>${cell.innerHTML}</dd></div>`)
      .join('');
    open('تفاصيل السجل', facts ? `<dl class="fn-facts">${facts}</dl>` : '<p class="fp-empty">لا تفاصيل.</p>');
  }

  // ---- مؤشرات سريعة لكل تبويب ---------------------------------------------
  const METRICS = {
    async journals() {
      const items = await fetchJSON(`/api/finance/journals?company_id=${cid()}`);
      const posted = items.filter(j => j.posted).length;
      return [['القيود', items.length], ['مرحّل', posted], ['مسودات', items.length - posted]];
    },
    async checks() {
      const items = await fetchJSON(`/api/finance/checks?company_id=${cid()}`);
      const open = items.filter(c => !['collected', 'cancelled'].includes(c.status));
      const total = open.reduce((sum, c) => sum + Number(c.amount || 0), 0);
      return [['شيكات', items.length], ['قيد التحصيل', open.length], ['قيمتها', money(total)]];
    },
    async parties() {
      const items = await fetchJSON(`/api/finance/parties?company_id=${cid()}`);
      const customers = items.filter(p => p.role === 'customer').length;
      return [['أطراف', items.length], ['عملاء', customers], ['موردون', items.length - customers]];
    },
    async assets() {
      const data = await fetchJSON(`/api/finance/assets?company_id=${cid()}`);
      return [['أصول', data.items.length], ['القيمة الدفترية', money(data.totals.book_value)],
        ['مجمع الإهلاك', money(data.totals.accumulated)]];
    },
    async payroll() {
      const [employees, runs] = await Promise.all([
        fetchJSON(`/api/finance/payroll/employees?company_id=${cid()}`),
        fetchJSON(`/api/finance/payroll/runs?company_id=${cid()}`)
      ]);
      const net = runs.filter(r => r.status !== 'cancelled').reduce((sum, r) => sum + Number(r.net || 0), 0);
      return [['موظفون', employees.length], ['مسيرات', runs.length], ['إجمالي الصافي', money(net)]];
    },
    async branches() {
      const items = await fetchJSON(`/api/finance/branches?company_id=${cid()}`);
      return [['فروع', items.length],
        ['قيود', items.reduce((sum, b) => sum + Number(b.entries || 0), 0)]];
    },
    async closing() {
      const items = await fetchJSON(`/api/finance/closing?company_id=${cid()}`);
      return [['سنوات مقفلة', items.length]];
    }
  };

  function paintMetrics(tab, chips) {
    const body = document.getElementById('fp-body');
    if (!body) return;
    const old = body.querySelector('.fn-chips');
    if (old) old.remove();
    if (!chips || !chips.length) return;
    const bar = document.createElement('div');
    bar.className = 'fn-chips';
    bar.innerHTML = chips.map(([label, value]) =>
      `<span class="fn-chip"><em>${esc(label)}</em><strong>${esc(value)}</strong></span>`).join('');
    body.insertBefore(bar, body.firstChild);
  }

  async function refreshMetrics(tab) {
    if (!METRICS[tab]) return paintMetrics(tab, null);
    try {
      paintMetrics(tab, await METRICS[tab]());
    } catch (_) {
      paintMetrics(tab, null); // المؤشرات تحسينية: فشلها لا يعطّل الشاشة
    }
  }

  // ---- عرض البطاقات --------------------------------------------------------
  function labelCells(scope) {
    scope.querySelectorAll('table.fp-table').forEach(table => {
      const heads = Array.from(table.querySelectorAll('thead th')).map(h => h.textContent.trim());
      table.querySelectorAll('tbody tr').forEach(row => {
        Array.from(row.children).forEach((cell, index) => {
          if (!cell.dataset.label && heads[index]) cell.dataset.label = heads[index];
        });
      });
    });
  }

  function applyView(mode) {
    const body = document.getElementById('fp-body');
    if (!body) return;
    body.dataset.view = mode;
    try { localStorage.setItem('finance.view', mode); } catch (_) { /* تخزين غير متاح */ }
    const toggle = document.getElementById('fn-view');
    if (toggle) {
      toggle.textContent = mode === 'cards' ? 'عرض جدول' : 'عرض بطاقات';
      toggle.setAttribute('aria-pressed', mode === 'cards' ? 'true' : 'false');
    }
  }

  function ensureToolbar() {
    const body = document.getElementById('fp-body');
    if (!body) return;
    const bar = body.parentElement && body.parentElement.querySelector('.fp-head-actions');
    const host = bar || (() => {
      const created = document.createElement('div');
      created.className = 'fp-head-actions';
      body.parentElement.insertBefore(created, body);
      return created;
    })();
    if (!host.querySelector('#fn-view')) {
      const toggle = document.createElement('button');
      toggle.type = 'button';
      toggle.id = 'fn-view';
      toggle.className = 'fp-ghost';
      toggle.textContent = 'عرض بطاقات';
      toggle.addEventListener('click', () => {
        const current = document.getElementById('fp-body').dataset.view;
        applyView(current === 'cards' ? 'table' : 'cards');
      });
      host.appendChild(toggle);
      let saved = 'table';
      try { saved = localStorage.getItem('finance.view') || 'table'; } catch (_) { /* تخزين غير متاح */ }
      applyView(saved);
    }
  }

  // ---- ربط النقرات بفتح التفاصيل ------------------------------------------
  function bindDetails(scope) {
    scope.querySelectorAll('[data-post]').forEach(btn => {
      btn.addEventListener('dblclick', event => {
        event.preventDefault();
        entryDetail(btn.dataset.post);
      });
    });
    const tab = document.getElementById('fp-body').dataset.tab || '';
    scope.querySelectorAll('table.fp-table tbody tr').forEach(row => {
      const first = row.querySelector('td');
      const cellText = first ? first.textContent.replace('#', '').trim() : '';
      const runCell = row.querySelector('[data-run-id]');
      let open = null;
      if (tab === 'journals' && /^\d+$/.test(cellText)) {
        open = () => entryDetail(cellText);
      } else if (tab === 'payroll' && runCell) {
        open = () => runDetail(runCell.dataset.runId);
      } else if (row.children.length >= 3) {
        // أي سجل آخر: نافذة تفاصيل تعرض خلاياه بعناوين أعمدتها.
        open = () => rowDetail(row);
      }
      if (!open) return;
      row.classList.add('fn-clickable');
      row.addEventListener('click', event => {
        if (event.target.closest('button, a, input, select')) return;
        open();
      });
    });
  }

  // ---- الاختصارات ----------------------------------------------------------
  function shortcut(tab) {
    const body = document.getElementById('fp-body');
    if (!body) return;
    const index = TAB_ORDER.indexOf(tab);
    return {
      'Alt+arrowdown': () => activate(TAB_ORDER[Math.min(index + 1, TAB_ORDER.length - 1)]),
      'Alt+arrowup': () => activate(TAB_ORDER[Math.max(index - 1, 0)]),
      'Alt+k': () => applyView(body.dataset.view === 'cards' ? 'table' : 'cards'),
      'Alt+p': () => {
        document.getElementById('fn-print') ? null : null;
        const print = document.getElementById('fp-print');
        if (print) print.click();
      }
    };
  }

  function activate(tab) {
    const button = document.querySelector(`[data-fp-tab='${tab}']`);
    if (button) button.click();
  }

  let lastTab = null;
  document.addEventListener('keydown', event => {
    if (event.key === 'Escape') return close();
    if (!event.altKey) return;
    const tab = document.getElementById('fp-body') && document.getElementById('fp-body').dataset.tab;
    if (!tab) return;
    // المفتاح المركّب = Alt + قيمة event.key، فتُطابق الاختصارات المسجّلة.
    const action = shortcut(tab)['Alt+' + event.key.toLowerCase()];
    if (action) {
      event.preventDefault();
      action();
    }
  });

  // ---- حلقة العمل: تُستدعى بعد كل رسم لتبويب -------------------------------
  function enhance() {
    const body = document.getElementById('fp-body');
    if (!body) return;
    const tab = body.dataset.tab || '';
    ensureToolbar();
    labelCells(body);
    bindDetails(body);
    refreshMetrics(tab);
    if (lastTab !== tab) {
      lastTab = tab;
      applyView(localStorage.getItem('finance.view') || 'table');
    }
  }

  window.FinanceNav = { enhance, open, close, applyView, entryDetail, runDetail };
})();
