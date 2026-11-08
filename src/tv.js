// Remote control on Android TV: the arrows move the focus to the nearest control in that
// direction, OK activates it. Only active on a TV (see enableTvMode in main.js).
//
// Controls are looked up in the top layer only (a list of options, a dialog, the detail, the
// page of a series, or the main page). The app often rebuilds parts of the page, so the focus comes back to the same
// control afterwards, or to the nearest one when that is gone.
const TV = (() => {
  const FOCUSABLE = [
    'a[href]', 'button:not(:disabled)', 'select:not(:disabled)', 'textarea:not(:disabled)',
    'input:not(:disabled):not([type="hidden"])', '.card', '.row-card', '.skip-times', '.part.in',
  ].join(',');
  const NATIVE = 'a[href],button,input,select,textarea';
  const TEXT_INPUTS = new Set(['text', 'search', 'url', 'tel', 'email', 'password', 'number']);
  const KEY_ATTRIBUTES = ['data-i', 'data-id', 'data-act', 'data-field', 'data-status', 'data-view', 'data-lib',
    'data-cat-i', 'data-saved-i', 'data-suggestion', 'data-link', 'data-pref', 'data-opt', 'value', 'name', 'id'];
  const DIRECTIONS = { ArrowUp: 'up', ArrowDown: 'down', ArrowLeft: 'left', ArrowRight: 'right' };

  let options = { onLayerChange() {}, showKeyboard() {} };
  let picker = null;
  let pickerSelect = null;
  // The text field the on-screen keyboard was opened for; until then OK only opens it.
  let editing = null;
  let pending = false;
  // Last focused control of each layer.
  const memory = new WeakMap();

  function layer() {
    return picker
      || document.querySelector('#modal-root .modal')
      || document.querySelector('#drawer:not(.hidden)')
      || document.querySelector('#show-page:not(.hidden)')
      || document.body;
  }

  const isText = (el) => el.tagName === 'TEXTAREA' || (el.tagName === 'INPUT' && TEXT_INPUTS.has(el.type));
  const shown = (rect) => rect.width > 0 && rect.height > 0;

  /** The controls of a layer; a card is one stop, its small buttons are for the mouse. */
  function controls(root) {
    return [...root.querySelectorAll(FOCUSABLE)].filter((el) => !el.parentElement.closest('.card, .row-card'));
  }

  function scrollerOf(el, cache = new Map()) {
    const parent = el.parentElement;
    if (!parent || parent === document.body) return null;
    if (cache.has(parent)) return cache.get(parent);
    const style = getComputedStyle(parent);
    const scrolls = (/auto|scroll/.test(style.overflowY) && parent.scrollHeight > parent.clientHeight + 1)
      || (/auto|scroll/.test(style.overflowX) && parent.scrollWidth > parent.clientWidth + 1);
    const found = scrolls ? parent : scrollerOf(parent, cache);
    cache.set(parent, found);
    return found;
  }

  function focusOn(el) {
    if (!el.matches(NATIVE) && !el.hasAttribute('tabindex')) el.tabIndex = -1;
    el.focus({ preventScroll: true });
    el.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    editing = null;
  }

  /** Selector finding the same control after the page is rebuilt. */
  function selectorOf(el) {
    const own = KEY_ATTRIBUTES.filter((a) => el.hasAttribute(a))
      .map((a) => `[${a}="${CSS.escape(el.getAttribute(a))}"]`).join('');
    if (!own) return null;
    const episode = el.closest('[data-ep]');
    return `${episode && episode !== el ? `[data-ep="${CSS.escape(episode.dataset.ep)}"] ` : ''}${el.tagName.toLowerCase()}${own}`;
  }

  function remember(el) {
    if (el === document.body) return;
    memory.set(layer(), { selector: selectorOf(el), rect: el.getBoundingClientRect() });
  }

  function nearest(list, rect) {
    let best = null;
    let bestDistance = Infinity;
    for (const el of list) {
      const r = el.getBoundingClientRect();
      if (!shown(r)) continue;
      const distance = Math.hypot(r.left - rect.left, r.top - rect.top);
      if (distance < bestDistance) [best, bestDistance] = [el, distance];
    }
    return best;
  }

  /** Gives the focus back when it was lost or stayed under a layer that opened. */
  function restore() {
    const root = layer();
    const active = document.activeElement;
    if (active && active !== document.body && root.contains(active) && shown(active.getBoundingClientRect())) return;
    const list = controls(root);
    if (!list.length) return;
    const saved = memory.get(root);
    let target = null;
    if (saved?.selector) {
      target = root.querySelector(saved.selector);
      if (target && !shown(target.getBoundingClientRect())) target = null;
    }
    if (!target && saved) target = nearest(list, saved.rect);
    // First time in a layer: the content of the page, the Play button of a detail.
    if (!target && root === document.body) target = document.querySelector('#grid .row-card, #grid .card');
    if (!target && root.matches('#drawer, #show-page')) target = root.querySelector('.btn.primary:not(:disabled), [data-act="play-first"]');
    focusOn(target || list.find((el) => shown(el.getBoundingClientRect())) || list[0]);
  }

  function scheduleRestore() {
    if (pending) return;
    pending = true;
    setTimeout(() => {
      pending = false;
      restore();
    }, 30);
  }

  /** How far `to` is from `from` going in `direction`: along it and across it. */
  function distances(from, to, direction) {
    const horizontal = direction === 'left' || direction === 'right';
    const forward = direction === 'right' || direction === 'down';
    const [a0, a1, b0, b1] = horizontal ? [from.left, from.right, to.left, to.right] : [from.top, from.bottom, to.top, to.bottom];
    const [c0, c1, d0, d1] = horizontal ? [from.top, from.bottom, to.top, to.bottom] : [from.left, from.right, to.left, to.right];
    return {
      main: forward ? b0 - a1 : a0 - b1,
      cross: Math.max(0, d0 - c1, c0 - d1),
      offset: Math.abs((c0 + c1) / 2 - (d0 + d1) / 2),
    };
  }

  function bestTarget(active, from, direction, cache) {
    const horizontal = direction === 'left' || direction === 'right';
    const scroller = scrollerOf(active, cache);
    let best = null;
    for (const el of controls(layer())) {
      if (el === active || el.contains(active) || active.contains(el)) continue;
      const to = el.getBoundingClientRect();
      if (!shown(to)) continue;
      const elScroller = scrollerOf(el, cache);
      const same = elScroller === scroller;
      // In another scrolling area only what can be seen there counts.
      if (!same && elScroller) {
        const box = elScroller.getBoundingClientRect();
        if (to.bottom <= box.top || to.top >= box.bottom || to.right <= box.left || to.left >= box.right) continue;
      }
      const { main, cross, offset } = distances(from, to, direction);
      if (main < -2) continue;
      // Sideways stays in the row, except when moving between areas (grid -> side panel).
      if (horizontal && cross > 0 && same) continue;
      const score = horizontal ? main + offset / 2 + cross * 1000 : main + cross * 3 + offset / 10;
      if (!best || score < best.score) best = { el, score, main, rect: to };
    }
    return best;
  }

  function move(direction) {
    const active = document.activeElement;
    if (!active || active === document.body || !layer().contains(active)) return restore();
    const cache = new Map();
    const vertical = direction === 'up' || direction === 'down';
    const scroller = vertical ? scrollerOf(active, cache) : null;
    const box = scroller?.getBoundingClientRect();
    let from = active.getBoundingClientRect();
    // The control was scrolled away while reading: continue from the edge of what is seen.
    if (box && (from.bottom <= box.top || from.top >= box.bottom)) {
      const edge = direction === 'down' ? box.top : box.bottom;
      from = { left: from.left, right: from.right, top: edge, bottom: edge };
    }
    const best = bestTarget(active, from, direction, cache);
    // Long text without controls: scroll it instead of jumping over it.
    if (scroller) {
      const step = scroller.clientHeight * 0.7;
      const room = direction === 'down'
        ? scroller.scrollHeight - scroller.clientHeight - scroller.scrollTop
        : scroller.scrollTop;
      const seen = best && best.rect.bottom > box.top && best.rect.top < box.bottom;
      if (room > 1 && !seen && (!best || best.main > step)) {
        scroller.scrollBy({ top: direction === 'down' ? step : -step });
        return;
      }
    }
    if (best) focusOn(best.el);
  }

  /** Arrows inside a text being typed move the caret, until it reaches the end. */
  function arrowStaysInField(el, direction) {
    if (el !== editing) return false;
    const start = el.selectionStart;
    if (start === null) return false;
    if (el.tagName === 'INPUT' && (direction === 'up' || direction === 'down')) return false;
    return direction === 'left' || direction === 'up' ? start > 0 : el.selectionEnd < el.value.length;
  }

  function closePicker() {
    if (!picker) return;
    picker.remove();
    picker = null;
    const select = pickerSelect;
    pickerSelect = null;
    if (select?.isConnected) focusOn(select);
    options.onLayerChange();
  }

  /** The list of a <select>: the system one cannot be relied on with a remote. */
  function openPicker(select) {
    picker = document.createElement('div');
    picker.className = 'tv-picker';
    const box = document.createElement('div');
    box.className = 'tv-picker-box';
    [...select.options].forEach((option, index) => {
      if (option.disabled) return;
      const button = document.createElement('button');
      button.type = 'button';
      button.className = `btn${index === select.selectedIndex ? ' primary' : ''}`;
      button.textContent = option.textContent;
      button.addEventListener('click', () => {
        select.selectedIndex = index;
        closePicker();
        select.dispatchEvent(new Event('change', { bubbles: true }));
      });
      box.append(button);
    });
    picker.append(box);
    pickerSelect = select;
    document.body.append(picker);
    focusOn(box.children[Math.max(0, [...select.options].slice(0, select.selectedIndex).filter((o) => !o.disabled).length)] || box.firstChild);
    options.onLayerChange();
  }

  function activate(el, event) {
    if (isText(el)) {
      // The first OK opens the keyboard; afterwards Enter belongs to the field.
      if (el === editing) return;
      event.preventDefault();
      event.stopPropagation();
      editing = el;
      options.showKeyboard();
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    if (event.repeat) return;
    if (el.tagName === 'SELECT') openPicker(el);
    else el.click();
  }

  function onKey(event) {
    const active = document.activeElement;
    const direction = DIRECTIONS[event.key];
    if (direction) {
      if (active && isText(active) && arrowStaysInField(active, direction)) return;
      event.preventDefault();
      event.stopPropagation();
      move(direction);
    } else if (event.key === 'Enter') {
      if (active && active !== document.body) activate(active, event);
      else restore();
    } else if (event.key === 'Escape' && picker) {
      event.preventDefault();
      event.stopPropagation();
      closePicker();
    }
  }

  function enable(callbacks) {
    options = { ...options, ...callbacks };
    document.addEventListener('keydown', onKey, true);
    document.addEventListener('focusin', (event) => remember(event.target));
    document.addEventListener('focusout', (event) => {
      if (event.target === editing) editing = null;
      scheduleRestore();
    });
    new MutationObserver(scheduleRestore).observe(document.body, {
      childList: true, subtree: true, attributes: true, attributeFilter: ['class', 'hidden'],
    });
    scheduleRestore();
  }

  return { enable, pickerOpen: () => !!picker, closePicker };
})();
