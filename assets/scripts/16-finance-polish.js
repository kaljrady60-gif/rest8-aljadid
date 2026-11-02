// 16-finance-polish.js — شريط مؤشرات حقيقي لكل شاشة مالية + تحديث فوري.
// يعمل بتغليف FinanceHub.show (لا يغيّر أي منطق قائم) ويضيف:
//   1) .fp-kpis أعلى كل شاشة بأرقام من قاعدة البيانات لا نصوص ثابتة.
//   2) زر «تحديث» وعدّاد «آخر تحديث» داخل الشريط.
//   3) رسالة واضحة بدل الفراغ عندما لا توجد صفوف بعد.
(function () {
  'use strict';
  // أكواد الدليل الداخلية تُخزَّن بصيغة __c1_1200؛ نعرض الرقم الصافي فقط.
  const cleanCode = v => String(v == null ? '' : v).replace(/^__c\d+_/, '');
  const esc = v => cleanCode(v)
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  const money = v => Number(v || 0).toLocaleString('en-US', {
    minimumFractionDigits: 2, maximumFractionDigits: 2
  });
  const cid = () => Number(window.FinanceApi.companyId());
  const q = extra => `?company_id=${cid()}${extra || ''}`;

  async function api(path) {
    const res = await window.apiFetch(path);
    if (!res.ok) {
      let msg = 'تعذّر تحميل البيانات.';
      try { msg = (await res.json()).error || msg; } catch (_) { /* جسم غير JSON */ }
      throw new Error(msg);
    }
    const data = await res.json();
    return Array.isArray(data) ? data : (data.items || data.rows || []);
  }

  // ── ملخص كل شاشة: أرقام محسوبة من الواجهات القائمة نفسها ────────────────
  const SUMMARIES = {
    async parties() {
      const rows = await api(`/api/finance/parties${q()}`);
      const customers = rows.filter(p => p.role === 'customer').length;
      const suppliers = rows.filter(p => p.role === 'supplier').length;
      return [
        ['الأطراف', String(rows.length), 'مسجّلون في هذه الشركة'],
        ['عملاء', String(customers), 'حساب تفصيلي لكل عميل'],
        ['موردون', String(suppliers), 'حساب تفصيلي لكل مورد'],
        ['أخرى', String(rows.length - customers - suppliers), 'أطراف متنوعة']
      ];
    },
    async checks() {
      const rows = await api(`/api/finance/checks${q()}`);
      const open = rows.filter(c => c.status !== 'collected' && c.status !== 'cancelled');
      const total = open.reduce((s, c) => s + Number(c.amount || 0), 0);
      const late = open.filter(c => String(c.due_date || '') < new Date().toISOString().slice(0, 10));
      return [
        ['إجمالي الشيكات', String(rows.length), 'وارد وصادر'],
        ['قيد التحصيل', String(open.length), 'لم يُحصّل بعد'],
        ['قيمة المفتوح', money(total), 'مجموع المبالغ المعلّقة'],
        ['متأخر السداد', String(late.length), 'تجاوز تاريخ الاستحقاق']
      ];
    },
    async requests() {
      const rows = await api(`/api/finance/requests${q()}`);
      const by = st => rows.filter(r => r.status === st).length;
      return [
        ['إجمالي الطلبات', String(rows.length), 'قيود وسندات'],
        ['مسودات', String(by('draft')), 'لم تُقدَّم بعد'],
        ['مقدَّمة', String(by('submitted')), 'بانتظار القرار'],
        ['معتمدة', String(by('approved')), 'جاهزة للتنفيذ']
      ];
    },
    async docreview() {
      const rows = await api(`/api/finance/doc-reviews${q()}`);
      const by = st => rows.filter(r => r.status === st).length;
      return [
        ['وثائق مسجلة', String(rows.length), 'فواتير ومستندات'],
        ['قيد المراجعة', String(by('pending')), 'بانتظار قرار'],
        ['معتمدة', String(by('approved')), 'مطابقة'],
        ['مرفوضة', String(by('rejected')), 'تحتاج تصحيحًا']
      ];
    },
    async dimensions() {
      const rows = await api(`/api/finance/dimensions${q()}`);
      const active = rows.filter(d => !d.archived_at);
      return [
        ['الأبعاد', String(rows.length), 'مراكز ومشاريع'],
        ['نشطة', String(active.length), 'تُستخدم عند الترحيل'],
        ['مراكز تكلفة', String(active.filter(d => d.kind === 'cost_center').length), 'لتوزيع المصروف'],
        ['مشاريع', String(active.filter(d => d.kind !== 'cost_center').length), 'لتتبع الأعمال']
      ];
    },
    async fx() {
      const rows = await api(`/api/finance/fx${q()}`);
      const latest = rows.map(r => r.as_of).filter(Boolean).sort().pop() || '—';
      const pairs = new Set(rows.map(r => `${r.base}/${r.quote}`)).size;
      return [
        ['أسعار مسجلة', String(rows.length), 'سعر لكل زوج عملة'],
        ['أزواج العملات', String(pairs), 'مثل SAR/YER'],
        ['أحدث تحديث', esc(latest), 'تاريخ السعر الأحدث'],
        ['العملة الأساسية', 'YER', 'الريال اليمني']
      ];
    },
    async countries() {
      const rows = await api(`/api/finance/countries${q()}`);
      return [
        ['الدول المسجلة', String(rows.length), 'بيانات مرجعية'],
        ['العملات المرتبطة', String(new Set(rows.map(r => r.currency).filter(Boolean)).size), 'رمز العملة لكل دولة'],
        ['مفعّلة', String(rows.filter(r => !r.archived_at).length), 'متاحة للاختيار'],
        ['مؤرشفة', String(rows.filter(r => r.archived_at).length), 'محجوبة عن الاستخدام']
      ];
    },
    async closing() {
      const rows = await api(`/api/finance/closing${q()}`);
      const last = rows.map(r => r.fiscal_year).sort().pop();
      const profit = rows.reduce((s, r) => s + Number(r.net_income || 0), 0);
      return [
        ['سنوات مقفلة', String(rows.length), 'إقفال نهائي'],
        ['آخر سنة', last ? String(last) : '—', 'الأحدث إقفالًا'],
        ['صافي المقفل', money(profit), 'مجموع الأرباح المرحّلة'],
        ['قابل للإقفال', String(rows.length ? 0 : 1), 'سنة مالية مفتوحة']
      ];
    },
    async bankrec() {
      const rows = await api(`/api/finance/bankrec${q()}`);
      const diff = rows.reduce((s, r) => s + Math.abs(Number(r.difference || 0)), 0);
      const cleared = rows.reduce((s, r) => s + Number(r.cleared_total || 0), 0);
      return [
        ['كشوف بنكية', String(rows.length), 'دورات مطابقة'],
        ['مُسوّى', money(cleared), 'مطابق للبنك'],
        ['فرق قائم', money(diff), 'يحتاج بند تسوية'],
        ['حالة المطابقة', diff === 0 ? 'مطابق ✔' : 'غير مطابق ✘', diff === 0 ? 'لا فروق' : 'راجع البنود']
      ];
    },
    async vouchers() {
      const rows = await api(`/api/vouchers${q()}`);
      const total = rows.reduce((s, v) => s + Number(v.amount || 0), 0);
      return [
        ['السندات', String(rows.length), 'قبض وصرف'],
        ['قبض', String(rows.filter(v => v.kind === 'receipt').length), 'داخل'],
        ['صرف', String(rows.filter(v => v.kind === 'payment').length), 'خارج'],
        ['الإجمالي', money(total), 'بعملات السندات']
      ];
    },
    async scanner() {
      return [
        ['طريقة التحليل', 'محلي', 'بلا خدمات خارجية'],
        ['يدعم', 'نص الفاتورة', 'لصق أو استخراج ضوئي'],
        ['النتيجة', 'بنود مقترحة', 'قابلة للتعديل قبل الحفظ'],
        ['التكلفة', 'صفر', 'لا استهلاك نت']
      ];
    },
    async taxes() {
      const rows = await api(`/api/finance/taxes${q()}`);
      const active = rows.filter(t => !t.archived_at);
      return [
        ['الضرائب المعرّفة', String(rows.length), 'أكواد ضريبية'],
        ['نشطة', String(active.length), 'متاحة على القيود'],
        ['المعدلات', active.map(t => `${(Number(t.rate_bp) / 100).toFixed(0)}%`).join(' · ') || '—', 'لكل كود'],
        ['الافتراضي', active[0] ? esc(active[0].name) : '—', 'الأكثر استخدامًا']
      ];
    },
    async taxlinks() {
      const rows = await api(`/api/finance/tax-links${q()}`);
      return [
        ['روابط الضريبة', String(rows.length), 'قيد ↔ كود ضريبي'],
        ['مبيعات', String(rows.filter(r => r.direction === 'sales').length), 'ضريبة المخرجات'],
        ['مشتريات', String(rows.filter(r => r.direction === 'purchases').length), 'ضريبة المدخلات'],
        ['الصافي', money(rows.reduce((s, r) => s + Number(r.tax_amount || 0), 0)), 'إجمالي الضريبة']
      ];
    },
    async audit() {
      const rows = await api(`/api/finance/audit${q('&limit=200')}`);
      const today = rows.filter(r => String(r.at || '').slice(0, 10) === new Date().toISOString().slice(0, 10));
      const actions = new Set(rows.map(r => r.action)).size;
      return [
        ['سجلات', String(rows.length), 'آخر ٢٠٠ عملية'],
        ['اليوم', String(today.length), 'عمليات اليوم'],
        ['أنواع العمليات', String(actions), 'ترحيل، اعتماد، حذف…'],
        ['أحدث سجل', rows[0] ? esc(String(rows[0].at).slice(0, 16)) : '—', 'وقت آخر عملية']
      ];
    }
  };

  function kpiMarkup(tab, kpis) {
    const cells = kpis.map(([label, value, note]) => `
      <div class="fp-kpi">
        <span>${esc(label)}</span>
        <strong>${value}</strong>
        <small>${esc(note || '')}</small>
      </div>`).join('');
    const now = new Date().toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
    return `<div class="fp-kpis" data-kpi-tab="${esc(tab)}">
      ${cells}
      <div class="fp-kpi-actions">
        <button type="button" class="fp-ghost" data-kpi-refresh="${esc(tab)}">تحديث ↻</button>
        <small>آخر تحديث ${esc(now)}</small>
      </div>
    </div>`;
  }

  // الرسم غير متزامن: ننتظر حتى تصبح اللوحة على التبويب المطلوب قبل الحقن.
  async function waitForTab(tab, tries = 60) {
    for (let i = 0; i < tries; i += 1) {
      const node = document.getElementById('fp-body');
      if (node && node.dataset.tab === tab) return node;
      await new Promise(done => setTimeout(done, 50));
    }
    const node = document.getElementById('fp-body');
    return node && node.dataset.tab === tab ? node : null;
  }

  async function decorate(tab) {
    collapseCatalogPanel();
    const fn = SUMMARIES[tab];
    if (!fn) return;
    const body = await waitForTab(tab);
    if (!body) return;
    body.querySelectorAll('.fp-kpis').forEach(node => node.remove());
    let kpis;
    try {
      kpis = await fn();
    } catch (_) {
      kpis = [['تعذّر القراءة', '—', 'تحقق من الاتصال أو الشركة المسندة']];
    }
    // الشركة قد تغيّر الشاشة أثناء الانتظار: نتأكد أننا ما زلنا عليها.
    const live = document.getElementById('fp-body');
    if (!live || live.dataset.tab !== tab) return;
    const holder = document.createElement('div');
    holder.innerHTML = kpiMarkup(tab, kpis);
    body.insertBefore(holder.firstElementChild, body.firstChild);
    labelTableCells(body);
    labelSilentButtons(body);
    relocateViewToggle(body);
    if (tab === 'journals') installNarrativeButton(body);
    if (window.FinanceActions) window.FinanceActions.apply(body);
    const refresh = body.querySelector('[data-kpi-refresh]');
    if (refresh) {
      refresh.addEventListener('click', () => {
        refresh.disabled = true;
        decorate(tab).finally(() => { const b = document.querySelector('[data-kpi-refresh]'); if (b) b.disabled = false; });
      });
    }
  }

  // لوحة «تفاصيل الشاشة» من الفهرس القديم كانت تبقى ظاهرة أسفل كل شاشة مالية.
  // نطويها فور كل نقرة على تبويب مالي (وبعد انتهاء رسم الشاشة) فلا تبقى نصوص «لم تُنفذ».
  function collapseCatalogPanel() {
    const view = document.getElementById('view-detail');
    if (!view) return;
    if (!view.classList.contains('hidden')) {
      view.classList.add('hidden');
      view.innerHTML = '';
    }
  }

  function installFinanceTabWatch() {
    if (window.__finTabWatch) return;
    window.__finTabWatch = true;
    document.addEventListener('click', event => {
      const button = event.target.closest && event.target.closest('[data-finance-tab]');
      if (!button) return;
      setTimeout(collapseCatalogPanel, 300);
      setTimeout(collapseCatalogPanel, 900);
    }, true);
  }

  // وسم كل خلية باسم عمودها، فيظهر كعنوان داخل البطاقة عند تحويل الجدول إلى بطاقات.
  function labelTableCells(scope) {
    scope.querySelectorAll('table.fp-table').forEach(table => {
      const heads = Array.from(table.querySelectorAll('thead th')).map(h => h.textContent.trim());
      table.querySelectorAll('tbody tr').forEach(row => {
        Array.from(row.children).forEach((cell, index) => {
          if (!cell.dataset.label && heads[index]) cell.dataset.label = heads[index];
        });
      });
    });
  }

  // زر «عرض بطاقات/جدول» ينتقل إلى شريط المؤشرات بدل الشريط العائم الفارغ.
  function relocateViewToggle(scope) {
    const actions = scope.querySelector('.fp-kpi-actions');
    const toggle = document.getElementById('fn-view');
    if (!actions || !toggle) return;
    if (toggle.parentElement !== actions) actions.appendChild(toggle);
  }

  function bindViewToggle() {
    if (window.__finViewBound) return;
    const apply = mode => {
      const body = document.getElementById('fp-body');
      if (body) body.dataset.view = mode;
      const toggle = document.getElementById('fn-view');
      if (toggle) {
        toggle.textContent = mode === 'cards' ? 'عرض جدول' : 'عرض بطاقات';
        toggle.setAttribute('aria-pressed', mode === 'cards' ? 'true' : 'false');
        toggle.title = mode === 'cards' ? 'ارجع لعرض الجدول' : 'اعرض الصفوف كبطاقات';
      }
      try { localStorage.setItem('finance.view', mode); } catch (_) { /* تخزين غير متاح */ }
    };
    document.addEventListener('click', event => {
      const toggle = event.target.closest && event.target.closest('#fn-view');
      if (!toggle) return;
      event.preventDefault();
      event.stopPropagation();
      const body = document.getElementById('fp-body');
      apply(body && body.dataset.view === 'cards' ? 'table' : 'cards');
    }, true);
    let saved = 'table';
    try { saved = localStorage.getItem('finance.view') || 'table'; } catch (_) { /* تخزين غير متاح */ }
    apply(saved);
    window.__finViewBound = true;
  }



  // زر «توحيد صيغة البيان»: يعرض أثر التوحيد قبل التطبيق، ثم يطبّقه فعليًا.
  function installNarrativeButton(body) {
    if (body.querySelector('#fp-norm-desc')) return;
    const host = body.querySelector('.fp-kpi-actions') || body;
    const button = document.createElement('button');
    button.type = 'button';
    button.id = 'fp-norm-desc';
    button.className = 'fp-ghost';
    button.textContent = 'توحيد صيغة البيان';
    button.title = 'يحوّل بيانات القيود إلى صيغة المستند: النوع + الرقم — الطرف';
    button.addEventListener('click', async () => {
      button.disabled = true;
      button.textContent = 'جارٍ الفحص…';
      try {
        const company = window.FinanceApi.companyId();
        const preview = await window.apiFetch('/api/finance/journal/normalize', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ company_id: Number(company), dry_run: true })
        }).then(r => r.json());
        if (preview.error) throw new Error(preview.error);
        if (!preview.updated) {
          window.alert(`كل البيانات مهنية بالفعل: فُحص ${preview.scanned} قيدًا، ولا شيء يحتاج تغييرًا.`);
          button.textContent = 'توحيد صيغة البيان';
          button.disabled = false;
          return;
        }
        const example = (preview.sample || [])[0];
        const line = example ? `\n\nمثال:\n«${example.before}»\n→ «${example.after}»` : '';
        if (!window.confirm(`سيُعاد كتابة بيان ${preview.updated} قيدًا من ${preview.scanned}.${line}\n\nأكمل؟`)) {
          button.textContent = 'توحيد صيغة البيان';
          button.disabled = false;
          return;
        }
        button.textContent = 'جارٍ التوحيد…';
        const done = await window.apiFetch('/api/finance/journal/normalize', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ company_id: Number(company), dry_run: false })
        }).then(r => r.json());
        if (done.error) throw new Error(done.error);
        button.textContent = `وُحّد ${done.updated} بيانًا ✔`;
        if (window.FinanceHub) window.FinanceHub.show('journals');
      } catch (e) {
        window.alert(e.message);
        button.textContent = 'توحيد صيغة البيان';
      } finally {
        button.disabled = false;
      }
    });
    host.appendChild(button);
  }

  // حارس إتاحة: أي زر بلا نص ولا وصف يأخذ وصفًا مشتقًا — لا يبقى زر صامت في أي شاشة.
  function labelSilentButtons(scope) {
    scope.querySelectorAll('button').forEach(button => {
      if (button.getAttribute('aria-label') || (button.innerText || '').trim()) return;
      const mark = (button.dataset.addRow && 'إضافة سطر')
        || (button.dataset.delRow !== undefined && 'حذف السطر')
        || (button.classList.contains('fp-close') && 'إغلاق')
        || 'زر';
      button.setAttribute('aria-label', mark);
      if (!button.getAttribute('title')) button.title = mark;
    });
  }

  function install() {
    const hub = window.FinanceHub;
    if (!hub || hub.__polished) return Boolean(hub);
    const original = hub.show.bind(hub);
    hub.show = tab => {
      original(tab);
      decorate(tab);
    };
    hub.__polished = true;
    installFinanceTabWatch();
    bindViewToggle();
    return true;
  }

  // الطبقة تُحمَّل بعد 07-finance-panels.js؛ نحاول فورًا ثم نعيد المحاولة عند DOMContentLoaded.
  if (!install()) {
    document.addEventListener('DOMContentLoaded', install);
    setTimeout(install, 1200);
  }
  window.FinancePolish = { decorate, install, collapseCatalogPanel };
})();
