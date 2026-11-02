// Local session gate; the server enforces all authorization independently.
(() => {
  const rawFetch = window.fetch.bind(window);
  let csrf = null;
  // شبكة أمان للمعاينة: حين يحجب الوسيط الكوكي نُكمل الجلسة بترويسة تحمل نفس الرمز.
  const tokenKey = 'smart-preview-token';
  let sessionToken = null;
  try {
    sessionToken = sessionStorage.getItem(tokenKey);
  } catch {
    sessionToken = null;
  }
  function setSessionToken(value) {
    sessionToken = value || null;
    try {
      if (value) sessionStorage.setItem(tokenKey, value);
      else sessionStorage.removeItem(tokenKey);
    } catch {
      /* أصل معزول: يبقى الرمز في الذاكرة فقط */
    }
  }
  const authFetch = (url, options = {}) => {
    if (!sessionToken) return rawFetch(url, options);
    const headers = new Headers(options.headers || {});
    headers.set('X-Smart-Session', sessionToken);
    return rawFetch(url, { ...options, headers });
  };
  // A response belongs to the authentication state in which its request began.
  let authVersion = 0;
  const $ = s => document.querySelector(s);
  const shell = () => $('.app-shell');
  function showLogin(message = '') {
    authVersion++;
    csrf = null;
    setSessionToken(null);
    shell().inert = true;
    document.querySelectorAll('dialog[open]').forEach(dialog => dialog.close());
    $('#login-overlay').classList.remove('hidden');
    $('#login-error').textContent = message;
    $('#login-password').value = '';
    $('#login-user').focus();
  }
  function authenticated(info) {
    authVersion++;
    csrf = info.csrf;
    if (info.session) setSessionToken(info.session);
    $('#profile-name').textContent = info.username;
    $('#profile-role').textContent = info.preview_no_login ? 'الدخول معطّل' : info.role === 'admin' ? 'مدير محلي' : 'قراءة فقط';
    $('#sign-out').classList.toggle('hidden', Boolean(info.preview_no_login));
    if (info.preview_no_login) $('.notice span').textContent = 'معاينة مفتوحة دون تسجيل دخول أو صلاحيات؛ البيانات مؤقتة والترحيل معطل. لا تستخدم بيانات حقيقية.';
    $('#login-overlay').classList.add('hidden');
    shell().inert = false;
    if (typeof window.refreshAfterAuth === 'function') window.refreshAfterAuth();
  }
  window.apiFetch = async (url, options = {}) => {
    const version = authVersion;
    const parsed = new URL(url, location.href);
    const headers = new Headers(options.headers || {});
    const method = (options.method || 'GET').toUpperCase();
    if (parsed.origin === location.origin && parsed.pathname.startsWith('/api/') && !['GET', 'HEAD', 'OPTIONS'].includes(method) && csrf) headers.set('X-CSRF-Token', csrf);
    const response = await authFetch(url, {
      ...options,
      headers
    });
    if (response.status === 401 && parsed.origin === location.origin && parsed.pathname.startsWith('/api/') && version === authVersion && csrf !== null) showLogin('انتهت الجلسة أو يلزم تسجيل الدخول.');
    return response;
  };
  document.addEventListener('DOMContentLoaded', async () => {
    shell().inert = true;
    $('#login-form').addEventListener('submit', async event => {
      event.preventDefault();
      const attempt = ++authVersion;
      const button = $('#login-submit');
      button.disabled = true;
      $('#login-error').textContent = '';
      try {
        const response = await authFetch('/api/auth/login', {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json'
          },
          body: JSON.stringify({
            username: $('#login-user').value,
            password: $('#login-password').value
          })
        });
        const data = await response.json();
        $('#login-password').value = '';
        if (!response.ok) throw new Error(data.error || 'تعذر تسجيل الدخول.');
        if (data.session) setSessionToken(data.session);
        const session = await authFetch('/api/auth/me');
        if (session.status === 401) {
          const state = session.headers.get('X-Smart-Session-State');
          await session.arrayBuffer();
          const details = {
            missing: 'لم يصل ملف الجلسة إلى الخادم بعد الدخول؛ قد يحجبه المتصفح أو وسيط المعاينة. رمز التشخيص S1.',
            invalid: 'وصل ملف جلسة غير صالح أو مكرر. رمز التشخيص S2.',
            'not-current': 'وصل ملف الجلسة لكن الخادم لم يجد جلسة فعالة مطابقة. رمز التشخيص S3.'
          };
          throw new Error(details[state] || 'تعذر تأكيد الجلسة بعد الدخول. رمز التشخيص S0.');
        }
        if (!session.ok) {
          await session.arrayBuffer();
          throw new Error('تعذر التحقق من الجلسة بعد الدخول.');
        }
        const info = await session.json();
        if (attempt === authVersion) authenticated(info);
      } catch (error) {
        if (attempt === authVersion) $('#login-error').textContent = error.message || 'تعذر الاتصال بالخادم.';
      } finally {
        button.disabled = false;
      }
    });
    $('#sign-out').addEventListener('click', async () => {
      const version = authVersion;
      try {
        const response = await window.apiFetch('/api/auth/logout', {
          method: 'POST'
        });
        if (!response.ok && response.status !== 401) throw new Error('تعذر إنهاء الجلسة.');
        if (version === authVersion) showLogin('تم تسجيل الخروج.');
      } catch (error) {
        if (version === authVersion) alert(error.message || 'تعذر الاتصال بالخادم.');
      }
    });
    const initialVersion = authVersion;
    try {
      const response = await authFetch('/api/auth/me');
      if (response.ok) {
        const info = await response.json();
        if (initialVersion === authVersion) authenticated(info);
      } else {
        await response.arrayBuffer();
        if (initialVersion === authVersion) showLogin();
      }
    } catch {
      if (initialVersion === authVersion) showLogin('تعذر الاتصال بالخادم.');
    }
  });
})();
