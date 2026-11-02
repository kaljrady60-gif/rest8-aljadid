// محوّل المسارات القديمة إلى مسارات المالية المحمية — نقطة واحدة فقط.
// السبب: الشاشات الحالية تنادي /api/accounts و/api/drafts وغيرها، وهذه
// المسارات القديمة لا تعرف الشركة. هنا نضيف company_id تلقائيًا ونوجّه
// الطلب إلى /api/finance/* فلا يحتاج كل سكربت إلى معرفة ذلك.
(function () {
  'use strict';
  const state = { companyId: null, ready: null, branchId: null };
  // المسارات التي تحترم اختيار الفرع عند التصفية.
  const BRANCH_AWARE = [/^\/api\/finance\/statements/, /^\/api\/finance\/ledger/, /^\/api\/finance\/export/, /^\/api\/finance\/print/];

  const ROUTES = [
    { from: /^\/api\/accounts(\?|$)/, to: '/api/finance/accounts', get: true },
    { from: /^\/api\/drafts(\?|$)/, to: '/api/finance/journals', get: true },
    { from: /^\/api\/vouchers(\?|$)/, to: '/api/finance/vouchers', get: true },
    { from: /^\/api\/periods(\?|$)/, to: '/api/finance/periods', get: true },
    { from: /^\/api\/ledger\/entries(\?|$)/, to: '/api/finance/ledger', get: true },
    { from: /^\/api\/ledger\/trial(\?|$)/, to: '/api/finance/statements?report=trial', get: true },
    { from: /^\/api\/ledger\/accounts\/(\d+)$/, to: '/api/finance/ledger', get: true }
  ];

  function withCompany(url) {
    if (!state.companyId) return url;
    const [path, query = ''] = url.split('?');
    const params = new URLSearchParams(query);
    if (!params.has('company_id')) params.set('company_id', state.companyId);
    // الفرع المحدد يُمرَّر فقط للمسارات التي تفهمه.
    if (state.branchId && !params.has('branch_id') && BRANCH_AWARE.some(re => re.test(path))) {
      params.set('branch_id', state.branchId);
    }
    return path + '?' + params.toString();
  }

  /// يبني عنوانًا كاملًا (شركة + فرع + معلمات) لروابط الطباعة والتصدير المباشرة.
  function scopedUrl(path, extra) {
    const params = new URLSearchParams(extra || {});
    if (state.companyId && !params.has('company_id')) params.set('company_id', state.companyId);
    if (state.branchId && !params.has('branch_id') && BRANCH_AWARE.some(re => re.test(path))) {
      params.set('branch_id', state.branchId);
    }
    const query = params.toString();
    return query ? `${path}?${query}` : path;
  }

  function map(url, method) {
    const [path, query = ''] = url.split('?');
    const rule = ROUTES.find(r => r.from.test(url) && (method === 'GET' ? r.get : true));
    if (!rule) return url;
    let target = rule.to;
    // معرّف الحساب في مسار الأستاذ القديم ينتقل إلى مَعلم account_id.
    const accountMatch = path.match(/^\/api\/ledger\/accounts\/(\d+)$/);
    const extra = accountMatch ? 'account_id=' + accountMatch[1] + '&' : '';
    return withCompany(target + (target.includes('?') ? '&' : '?') + extra + query);
  }

  // تحميل سياق المالية مرة واحدة: الشركة الافتراضية + قائمة الشركات.
  function loadContext() {
    if (state.ready) return state.ready;
    state.ready = fetch('/api/finance/context', { credentials: 'same-origin' })
      .then(r => (r.ok ? r.json() : null))
      .then(ctx => {
        if (!ctx || !ctx.default_company_id) {
          // فشل قبل الدخول: لا نُخزّن فشلًا دائمًا، بل نسمح بمحاولة لاحقة.
          state.ready = null;
          return ctx;
        }
        window.FinanceContext = ctx;
        state.companyId = String(ctx.default_company_id);
        return ctx;
      })
      .catch(() => {
        state.ready = null;
        return null;
      });
    return state.ready;
  }

  // يضمن تحميل السياق ويعيد معرّف الشركة أو null.
  async function ensureCompany() {
    if (!state.companyId) await loadContext();
    if (!state.companyId) await loadContext();
    return state.companyId;
  }

  window.FinanceApi = {
    companyId: () => state.companyId,
    branchId: () => state.branchId,
    setBranch(id) {
      state.branchId = id ? String(id) : null;
    },
    scopedUrl,
    setCompany(id) {
      state.companyId = id ? String(id) : null;
    },
    context: loadContext,
    ensureCompany,
    withCompany
  };

  // تغليف apiFetch (المُعرَّف في auth.js) بعد جهوزيته.
  const install = () => {
    if (!window.apiFetch || window.apiFetch.__financeWrapped) return;
    const original = window.apiFetch;
    const wrapped = async (url, options = {}) => {
      const method = (options.method || 'GET').toUpperCase();
      const mapped = map(String(url), method);
      if (mapped !== url) {
        await ensureCompany();
        if (!state.companyId) return original(url, options);
        url = map(String(url), method);
        if (options.body && method !== 'GET' && state.companyId) {
          try {
            const parsed = JSON.parse(options.body);
            if (parsed && typeof parsed === 'object' && !('company_id' in parsed)) {
              parsed.company_id = Number(state.companyId);
              options = { ...options, body: JSON.stringify(parsed) };
            }
          } catch (_) {
            /* جسم غير JSON: يُمرَّر كما هو */
          }
        }
      }
      return original(url, options);
    };
    wrapped.__financeWrapped = true;
    window.apiFetch = wrapped;
    loadContext();
  };

  document.addEventListener('DOMContentLoaded', install);
  install();
})();
