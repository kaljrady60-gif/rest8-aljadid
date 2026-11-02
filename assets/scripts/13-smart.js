// طبقة الذكاء الواجهي: تلميحات عند التأشير · إكمال تلقائي · تنبؤ مرتّب بالسياق.
// لا تخمين مالي: كل قيمة مقترحة مصدرها بيانات القاعدة أو سجل استخدام المستخدم نفسه.
// الترتيب: تكرار الاستخدام × قرب الزمن × ملاءمة السياق (الشاشة/الحقل/العملة).
(function () {
  'use strict';

  const STORE = 'sa.smart.v1';
  const state = {
    history: {},
    ctx: { screen: 'home', currency: 'YER', company: 1 },
    built: false
  };

  // ── التخزين المحلي: سجل قيم لكل حقل ─────────────────────────────────────
  function load() {
    try {
      const raw = localStorage.getItem(STORE);
      if (raw) state.history = JSON.parse(raw) || {};
    } catch (_) { state.history = {}; }
  }
  function save() {
    try { localStorage.setItem(STORE, JSON.stringify(state.history)); } catch (_) { /* ممتلئ أو محجوب */ }
  }
  function remember(field, value) {
    const v = String(value || '').trim();
    if (!v || v.length > 200) return;
    const bucket = state.history[field] || (state.history[field] = {});
    const now = Date.now();
    const row = bucket[v] || { n: 0, t: 0 };
    row.n += 1;
    row.t = now;
    bucket[v] = row;
    save();
  }

  // ── الترتيب: تكرار + حداثة (اضمحلال أسبوعي) + سياق ───────────────────────
  function score(row, ctxHit) {
    const ageDays = Math.max(0, (Date.now() - row.t) / 86400000);
    const recency = Math.exp(-ageDays / 21);        // يضمحل خلال ~3 أسابيع
    return row.n * (0.6 + 1.6 * recency) * (ctxHit ? 1.5 : 1);
  }

  function fromHistory(field, ctxHit) {
    const bucket = state.history[field] || {};
    return Object.entries(bucket)
      .map(([value, row]) => ({ value, s: score(row, ctxHit) }))
      .sort((a, b) => b.s - a.s);
  }

  // ── مصادر البيانات الحيّة (تُجلب مرة واحدة وتُخزَّن في الجلسة) ────────────
  const cache = { accounts: [], parties: [], descriptions: [] };
  async function warm() {
    if (state.built) return;
    state.built = true;
    try {
      const cid = window.FinanceApi ? Number(window.FinanceApi.companyId()) : 1;
      const get = async url => {
        const r = await window.apiFetch(url);
        return r.ok ? r.json() : null;
      };
      const [accounts, parties, journals] = await Promise.all([
        get(`/api/finance/accounts?company_id=${cid}`),
        get(`/api/finance/parties?company_id=${cid}`).catch(() => null),
        get(`/api/finance/journals?company_id=${cid}`).catch(() => null)
      ]);
      if (Array.isArray(accounts)) cache.accounts = accounts;
      if (Array.isArray(parties)) cache.parties = parties;
      if (Array.isArray(journals)) {
        cache.descriptions = journals
          .map(j => String(j.description || '').trim())
          .filter(Boolean)
          .filter((v, i, a) => a.indexOf(v) === i)
          .slice(0, 120);
      }
    } catch (_) { /* الشبكة أو الصلاحية: نكتفي بالسجل المحلي */ }
  }

  function sourceFor(input) {
    const name = (input.name || input.id || '').toLowerCase();
    if (/account|حساب/.test(name)) {
      return cache.accounts
        .filter(a => !state.ctx.currency || a.currency === state.ctx.currency)
        .map(a => ({ value: `${a.code} — ${a.name}`, label: a.kind || '', meta: a.id }));
    }
    if (/party|طرف|supplier|customer/.test(name)) {
      return cache.parties.map(p => ({ value: p.name, label: p.kind || '' }));
    }
    if (/description|بيان|reason|ملاحظ/.test(name)) {
      return cache.descriptions.map(d => ({ value: d, label: 'بيان سابق' }));
    }
    return [];
  }

  // ── التنبؤ المرتّب ───────────────────────────────────────────────────────
  function predict(input, typed) {
    const name = input.name || input.id || 'field';
    const q = String(typed || '').trim().toLowerCase();
    const ctxHit = false;
    const merged = new Map();

    // 1) سياق الزمن: تاريخ اليوم للحقول الزمنية
    if (input.type === 'date' && !typed) {
      merged.set(new Date().toISOString().slice(0, 10), { s: 100, label: 'اليوم' });
    }
    // 2) سياق الشاشة: العملة الافتراضية للشركة
    if (/currency|عملة/.test(name) && !typed) {
      merged.set(state.ctx.currency, { s: 95, label: 'عملة الشركة' });
    }
    // 3) بيانات حقيقية من القاعدة
    for (const item of sourceFor(input)) {
      if (q && !item.value.toLowerCase().includes(q)) continue;
      merged.set(item.value, { s: 80, label: item.label || 'من الدليل' });
    }
    // 4) سجل المستخدم: الأكثر تكرارًا وحداثة يفوز
    for (const { value, s } of fromHistory(name, ctxHit)) {
      if (q && !value.toLowerCase().includes(q)) continue;
      const prev = merged.get(value);
      merged.set(value, { s: Math.max(prev ? prev.s : 0, s * 12), label: 'استخدامك السابق' });
    }
    return [...merged.entries()]
      .map(([value, meta]) => ({ value, ...meta }))
      .sort((a, b) => b.s - a.s)
      .slice(0, 8);
  }

  // ── واجهة الاقتراحات ─────────────────────────────────────────────────────
  let box = null;
  function ensureBox() {
    if (box) return box;
    box = document.createElement('div');
    box.className = 'smart-suggest hidden';
    box.setAttribute('role', 'listbox');
    document.body.appendChild(box);
    return box;
  }

  function place(input) {
    const r = input.getBoundingClientRect();
    box.style.left = `${Math.max(8, r.left)}px`;
    box.style.top = `${r.bottom + window.scrollY + 6}px`;
    box.style.width = `${Math.max(220, r.width)}px`;
    box.style.position = 'absolute';
  }

  function attach(input) {
    if (input.dataset.smart === '1') return;
    input.dataset.smart = '1';
    input.setAttribute('autocomplete', 'off');
    input.addEventListener('input', () => {
      const items = predict(input, input.value);
      const b = ensureBox();
      if (!items.length) { b.classList.add('hidden'); return; }
      b.innerHTML = items.map((it, i) =>
        `<button type="button" role="option" data-i="${i}">
           <span class="s-val">${it.value.replace(/[<>&]/g, c => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;' }[c]))}</span>
           <small>${it.label}</small>
         </button>`).join('');
      b.classList.remove('hidden');
      place(input);
      b.querySelectorAll('button').forEach(btn => {
        btn.addEventListener('mousedown', ev => {
          ev.preventDefault();
          input.value = items[Number(btn.dataset.i)].value;
          remember(input.name || input.id, input.value);
          b.classList.add('hidden');
          input.dispatchEvent(new Event('change', { bubbles: true }));
        });
      });
    });
    input.addEventListener('blur', () => {
      remember(input.name || input.id, input.value);
      setTimeout(() => box && box.classList.add('hidden'), 120);
    });
    input.addEventListener('keydown', ev => {
      if (ev.key === 'Escape' && box) box.classList.add('hidden');
    });
  }

  // ── التلميحات: وصف قصير لكل مكوّن ────────────────────────────────────────
  const autoHelp = el => {
    const t = (el.tagName || '').toLowerCase();
    const type = el.getAttribute('type') || '';
    if (el.dataset.help) return el.dataset.help;
    if (t === 'button') return 'زر تنفيذ — اضغط لتنفيذ الإجراء المكتوب عليه.';
    if (t === 'input' && type === 'date') return 'حقل تاريخ — اختر اليوم أو اكتبه بصيغة سنة-شهر-يوم.';
    if (t === 'input' && type === 'search') return 'بحث فوري داخل المعروض فقط.';
    if (t === 'input') return 'حقل إدخال — يبدأ الاقتراح تلقائيًا من بياناتك السابقة.';
    if (t === 'select') return 'قائمة اختيار — القيم المتاحة هي المسموح بها فقط.';
    if (t === 'th') return 'عنوان عمود — يمكن النقر للفرز حيث هو مدعوم.';
    if (t === 'td') return '';
    if (el.classList.contains('nav-item')) return 'تبويب تنقل رئيسي.';
    return '';
  };

  function tooltip() {
    let tip = document.getElementById('smart-tip');
    if (!tip) {
      tip = document.createElement('div');
      tip.id = 'smart-tip';
      tip.className = 'smart-tip hidden';
      document.body.appendChild(tip);
    }
    document.addEventListener('mouseover', ev => {
      const el = ev.target.closest('[data-help], button, input, select, th, .nav-item, .finance-tab-bar button');
      if (!el) return;
      const text = autoHelp(el);
      if (!text) return;
      tip.textContent = text;
      const r = el.getBoundingClientRect();
      tip.style.top = `${r.top - 34 + window.scrollY}px`;
      tip.style.left = `${Math.max(8, r.left)}px`;
      tip.classList.remove('hidden');
    });
    document.addEventListener('mouseout', () => tip.classList.add('hidden'));
    document.addEventListener('scroll', () => tip.classList.add('hidden'), true);
  }

  // ── اختصار البحث الشامل Ctrl+K ───────────────────────────────────────────
  function commandPalette() {
    const items = [
      ['home', 'الرئيسية'], ['finance', 'المالية'], ['journals', 'القيود اليومية'],
      ['vouchers', 'السندات'], ['accounts', 'دليل الحسابات'], ['trial', 'ميزان المراجعة'],
      ['ledger', 'دفتر الأستاذ'], ['periods', 'الفترات والإقفال']
    ];
    document.addEventListener('keydown', ev => {
      if (!(ev.ctrlKey || ev.metaKey) || ev.key.toLowerCase() !== 'k') return;
      ev.preventDefault();
      const wrap = document.createElement('div');
      wrap.className = 'cmd-overlay';
      wrap.innerHTML = `<div class="cmd-box"><input class="cmd-input" placeholder="ابحث عن شاشة أو حساب…" autofocus>
        <div class="cmd-list">${items.map(([id, label]) =>
          `<button data-cmd="${id}">${label}</button>`).join('')}</div></div>`;
      document.body.appendChild(wrap);
      const input = wrap.querySelector('.cmd-input');
      const list = wrap.querySelector('.cmd-list');
      input.focus();
      input.addEventListener('input', () => {
        const q = input.value.trim();
        const hits = items.filter(([, label]) => label.includes(q));
        const accs = cache.accounts.filter(a => `${a.code} ${a.name}`.includes(q)).slice(0, 6);
        list.innerHTML = hits.map(([id, label]) => `<button data-cmd="${id}">${label}</button>`).join('')
          + accs.map(a => `<button data-acc="${a.id}">${a.code} — ${a.name}</button>`).join('');
      });
      wrap.addEventListener('click', ev => {
        const btn = ev.target.closest('button');
        if (btn && btn.dataset.cmd) { document.querySelector(`[data-nav="${btn.dataset.cmd}"], [data-open="${btn.dataset.cmd}"]`)?.click(); wrap.remove(); }
        else if (btn && btn.dataset.acc) { window.dispatchEvent(new CustomEvent('smart:account', { detail: btn.dataset.acc })); wrap.remove(); }
        else if (ev.target === wrap) wrap.remove();
      });
      input.addEventListener('keydown', e => { if (e.key === 'Escape') wrap.remove(); });
    });
  }

  // ── حفظ حالة الواجهة ─────────────────────────────────────────────────────
  function rememberScreen() {
    const nav = document.querySelector('.nav-item.active');
    if (nav) localStorage.setItem('sa.screen', nav.dataset.nav || 'home');
    try {
      const last = localStorage.getItem('sa.screen');
      if (last && last !== 'home') {
        const btn = document.querySelector(`[data-nav="${last}"]`);
        if (btn && !nav) btn.click();
      }
    } catch (_) { /* لا شيء */ }
  }

  function boot() {
    load();
    tooltip();
    commandPalette();
    document.addEventListener('focusin', ev => {
      const el = ev.target;
      if (el.matches && el.matches('input[type="text"], input[type="search"], input:not([type])')) {
        warm();
        attach(el);
      }
    });
    document.addEventListener('click', ev => {
      const nav = ev.target.closest('.nav-item');
      if (nav) { state.ctx.screen = nav.dataset.nav || 'home'; localStorage.setItem('sa.screen', state.ctx.screen); }
    });
    const sel = document.querySelector('#ledger-currency');
    if (sel) state.ctx.currency = sel.value || 'YER';
    window.Smart = { predict, remember, warm, ctx: state.ctx };
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', boot);
  else boot();
})();
