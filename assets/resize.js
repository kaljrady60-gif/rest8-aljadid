'use strict';

// Progressive enhancement for the existing development interface; no accounting data here.
(() => {
  const $ = s => document.querySelector(s);
  const clamp = (n, min, max) => Math.min(max, Math.max(min, n));
  const layout = $('.layout'),
    chat = $('.assistant-panel');
  const widthGrip = $('.assistant-resize-width'),
    heightGrip = $('.assistant-resize-height');
  const minimize = $('#assistant-minimize'),
    maximize = $('#assistant-maximize');
  let chatWidth = 312,
    chatHeight = 0,
    minimized = false,
    maximized = false;
  try {
    const s = JSON.parse(localStorage.getItem('smart-chat-size') || '{}');
    if (Number.isFinite(s.width)) chatWidth = clamp(s.width, 240, 600);
    if (Number.isFinite(s.height)) chatHeight = clamp(s.height, 250, 1200);
  } catch {/* optional preference only */}
  function storeSize() {
    try {
      localStorage.setItem('smart-chat-size', JSON.stringify({
        width: chatWidth,
        height: chatHeight
      }));
    } catch {/* size preference is optional */}
  }
  function maxWidth() {
    return Math.max(240, Math.min(600, layout.clientWidth - 420));
  }
  function applyChat() {
    layout.style.setProperty('--assistant-width', clamp(chatWidth, 240, maxWidth()) + 'px');
    chat.classList.toggle('chat-minimized', minimized);
    chat.classList.toggle('chat-maximized', maximized);
    if (maximized || minimized) chat.style.height = '';else chat.style.height = chatHeight ? clamp(chatHeight, 250, 1200) + 'px' : '';
    minimize.textContent = minimized ? '＋' : '−';
    minimize.setAttribute('aria-expanded', String(!minimized));
    minimize.setAttribute('aria-label', minimized ? 'استعادة لوحة المحادثة' : 'تصغير لوحة المحادثة');
    maximize.textContent = maximized ? '❐' : '▢';
    maximize.setAttribute('aria-pressed', String(maximized));
    maximize.setAttribute('aria-label', maximized ? 'استعادة الحجم السابق للمحادثة' : 'تكبير لوحة المحادثة');
  }
  minimize.addEventListener('click', () => {
    if (maximized) maximized = false;
    minimized = !minimized;
    applyChat();
  });
  maximize.addEventListener('click', () => {
    minimized = false;
    maximized = !maximized;
    applyChat();
  });
  function drag(handle, callback, done) {
    handle.addEventListener('pointerdown', e => {
      if (e.button !== 0 || !e.isPrimary) return;
      e.preventDefault();
      e.stopPropagation();
      const start = {
        x: e.clientX,
        y: e.clientY
      };
      handle.setPointerCapture(e.pointerId);
      const move = ev => callback(ev.clientX - start.x, ev.clientY - start.y, true);
      const end = () => {
        handle.removeEventListener('pointermove', move);
        handle.removeEventListener('pointerup', end);
        handle.removeEventListener('pointercancel', end);
        done?.();
      };
      handle.addEventListener('pointermove', move);
      handle.addEventListener('pointerup', end);
      handle.addEventListener('pointercancel', end);
    });
  }
  let chatStartWidth = 0,
    chatStartHeight = 0;
  widthGrip.addEventListener('pointerdown', () => {
    chatStartWidth = chat.getBoundingClientRect().width;
  });
  drag(widthGrip, dx => {
    if (window.innerWidth <= 850 || maximized || minimized) return;
    chatWidth = clamp(chatStartWidth - dx, 240, maxWidth());
    applyChat();
  }, storeSize);
  heightGrip.addEventListener('pointerdown', () => {
    chatStartHeight = chat.getBoundingClientRect().height;
  });
  drag(heightGrip, (_dx, dy) => {
    if (maximized || minimized) return;
    chatHeight = clamp(chatStartHeight + dy, 250, 1200);
    applyChat();
  }, storeSize);
  widthGrip.addEventListener('keydown', e => {
    if (!['ArrowLeft', 'ArrowRight'].includes(e.key) || window.innerWidth <= 850 || maximized || minimized) return;
    e.preventDefault();
    chatWidth = clamp(chat.getBoundingClientRect().width + (e.key === 'ArrowLeft' ? 16 : -16), 240, maxWidth());
    applyChat();
    storeSize();
  });
  heightGrip.addEventListener('keydown', e => {
    if (!['ArrowUp', 'ArrowDown'].includes(e.key) || maximized || minimized) return;
    e.preventDefault();
    chatHeight = clamp(chat.getBoundingClientRect().height + (e.key === 'ArrowDown' ? 16 : -16), 250, 1200);
    applyChat();
    storeSize();
  });
  window.addEventListener('resize', applyChat);
  applyChat();
  function enhanceTableFrame(wrap) {
    if (wrap.parentElement?.classList.contains('table-resize-frame')) return;
    const frame = document.createElement('div');
    frame.className = 'table-resize-frame';
    wrap.before(frame);
    frame.append(wrap);
    const side = document.createElement('button'),
      bottom = document.createElement('button');
    side.type = bottom.type = 'button';
    side.className = 'table-frame-width resize-handle';
    bottom.className = 'table-frame-height resize-handle';
    side.setAttribute('aria-label', 'تغيير عرض مساحة الجدول');
    bottom.setAttribute('aria-label', 'تغيير ارتفاع مساحة الجدول');
    side.dataset.help = 'اسحب جانب مساحة الجدول لتغيير عرضها ضمن المساحة المتاحة أو استخدم الأسهم. انقر مرتين للاستعادة.';
    bottom.dataset.help = 'اسحب أسفل الجدول لتغيير ارتفاع مساحة العرض أو استخدم الأسهم. انقر مرتين للاستعادة.';
    const hint = document.createElement('div');
    hint.className = 'table-scroll-hint';
    hint.textContent = '↔ مرّر أفقيًا لعرض بقية الأعمدة.';
    hint.hidden = true;
    frame.append(side, bottom);
    frame.after(hint);
    const syncHint = () => {
      hint.hidden = wrap.scrollWidth <= wrap.clientWidth + 6;
    };
    const observer = new ResizeObserver(() => {
      if (!wrap.isConnected) {
        observer.disconnect();
        return;
      }
      syncHint();
    });
    observer.observe(wrap);
    observer.observe(wrap.querySelector('table'));
    syncHint();
    let w = 0,
      h = 0;
    side.addEventListener('pointerdown', () => {
      w = frame.getBoundingClientRect().width;
    });
    bottom.addEventListener('pointerdown', () => {
      h = wrap.getBoundingClientRect().height;
    });
    function setWidth(x) {
      const max = frame.parentElement.clientWidth;
      frame.style.width = clamp(Math.round(x), Math.min(200, max), max) + 'px';
    }
    function setHeight(x) {
      wrap.style.height = clamp(Math.round(x), 80, 700) + 'px';
    }
    drag(side, dx => {
      setWidth(w + (getComputedStyle(frame).direction === 'rtl' ? -dx : dx));
    });
    drag(bottom, (_dx, dy) => {
      setHeight(h + dy);
    });
    side.addEventListener('keydown', e => {
      if (!['ArrowLeft', 'ArrowRight'].includes(e.key)) return;
      e.preventDefault();
      const sign = getComputedStyle(frame).direction === 'rtl' ? -1 : 1;
      setWidth(frame.getBoundingClientRect().width + (e.key === 'ArrowLeft' ? -16 : 16) * sign);
    });
    bottom.addEventListener('keydown', e => {
      if (!['ArrowUp', 'ArrowDown'].includes(e.key)) return;
      e.preventDefault();
      setHeight(wrap.getBoundingClientRect().height + (e.key === 'ArrowUp' ? -16 : 16));
    });
    side.addEventListener('dblclick', () => {
      frame.style.width = '';
    });
    bottom.addEventListener('dblclick', () => {
      wrap.style.height = '';
    });
  }
  function enhanceTable(table) {
    if (table.dataset.resizable === 'yes' || !table.getBoundingClientRect().width) return;
    const heads = [...table.querySelectorAll('thead tr:first-child th')];
    if (!heads.length || heads.some(h => h.colSpan !== 1)) return;
    table.dataset.resizable = 'yes';
    enhanceTableFrame(table.closest('.table-wrap'));
    const widths = heads.map(h => Math.max(55, Math.round(h.getBoundingClientRect().width)));
    const group = document.createElement('colgroup');
    const cols = widths.map(w => {
      const c = document.createElement('col');
      c.style.width = w + 'px';
      group.append(c);
      return c;
    });
    table.prepend(group);
    table.classList.add('resizable-table');
    const apply = () => {
      let sum = 0;
      cols.forEach((c, i) => {
        c.style.width = widths[i] + 'px';
        sum += widths[i];
      });
      table.style.width = sum + 'px';
    };
    apply();
    heads.forEach((h, i) => {
      const label = h.textContent.trim() || String(i + 1);
      const grip = document.createElement('button');
      grip.type = 'button';
      grip.className = 'column-resize resize-handle';
      grip.setAttribute('aria-label', 'تغيير عرض عمود ' + label);
      grip.setAttribute('data-help', 'اسحب حافة العمود أو استخدم السهمين لتغيير عرضه. انقر مرتين للاستعادة.');
      h.append(grip);
      let start = 0;
      grip.addEventListener('pointerdown', () => {
        start = widths[i];
      });
      drag(grip, dx => {
        const sign = getComputedStyle(table).direction === 'rtl' ? -1 : 1;
        widths[i] = clamp(Math.round(start + sign * dx), 55, 900);
        apply();
      });
      grip.addEventListener('keydown', e => {
        if (!['ArrowLeft', 'ArrowRight'].includes(e.key)) return;
        e.preventDefault();
        const sign = getComputedStyle(table).direction === 'rtl' ? -1 : 1;
        widths[i] = clamp(widths[i] + (e.key === 'ArrowRight' ? 1 : -1) * sign * 16, 55, 900);
        apply();
      });
      grip.addEventListener('dblclick', () => {
        widths[i] = Math.max(55, Math.round(h.dataset.initialWidth));
        apply();
      });
      h.dataset.initialWidth = String(widths[i]);
    });
  }
  function enhanceField(field) {
    if (field.closest('.field-resize-wrap') || field.type === 'hidden' || field.type === 'checkbox' || field.type === 'radio') return;
    const wrap = document.createElement('span');
    wrap.className = 'field-resize-wrap';
    field.before(wrap);
    wrap.append(field);
    const grip = document.createElement('button');
    grip.type = 'button';
    grip.className = 'field-resize resize-handle';
    grip.setAttribute('aria-label', 'تغيير حجم ' + (field.getAttribute('aria-label') || field.closest('label')?.childNodes[0]?.textContent.trim() || 'الحقل'));
    grip.dataset.help = 'اسحب الزاوية لتغيير حجم الحقل ضمن المساحة المتاحة، أو استخدم الأسهم. انقر مرتين للاستعادة.';
    wrap.append(grip);
    let w = 0,
      h = 0;
    grip.addEventListener('pointerdown', () => {
      w = wrap.getBoundingClientRect().width;
      h = field.getBoundingClientRect().height;
    });
    function update(width, height) {
      const parent = wrap.parentElement;
      const max = parent.clientWidth || width;
      wrap.style.width = clamp(Math.round(width), Math.min(95, max), max) + 'px';
      field.style.height = clamp(Math.round(height), 32, 120) + 'px';
    }
    drag(grip, (dx, dy) => {
      const sign = getComputedStyle(wrap).direction === 'rtl' ? -1 : 1;
      update(w + dx * sign, h + dy);
    });
    grip.addEventListener('keydown', e => {
      if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(e.key)) return;
      e.preventDefault();
      const sign = getComputedStyle(wrap).direction === 'rtl' ? -1 : 1;
      let dx = e.key === 'ArrowLeft' ? -16 : e.key === 'ArrowRight' ? 16 : 0,
        dy = e.key === 'ArrowUp' ? -8 : e.key === 'ArrowDown' ? 8 : 0;
      update(wrap.getBoundingClientRect().width + dx * sign, field.getBoundingClientRect().height + dy);
    });
    grip.addEventListener('dblclick', () => {
      wrap.style.width = '';
      field.style.height = '';
    });
  }
  let pending = false;
  function schedule() {
    if (pending) return;
    pending = true;
    requestAnimationFrame(() => {
      pending = false;
      document.querySelectorAll('.table-wrap table').forEach(enhanceTable);
      document.querySelectorAll('#draft-form input,#draft-form select,#account-form input,#account-form select,#rename-form input,#period-form input').forEach(enhanceField);
    });
  }
  new MutationObserver(schedule).observe(document.body, {
    childList: true,
    subtree: true
  });
  document.addEventListener('click', schedule);
  document.addEventListener('focusin', schedule);
  window.addEventListener('resize', schedule);
  schedule();
})();
