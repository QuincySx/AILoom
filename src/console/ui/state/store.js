// AIL-086：最小发布/订阅状态容器。组件订阅选段，禁止各自维护业务全局变量。

const listeners = new Map(); // key -> Set<fn>
const state = new Map(); // key -> value

export function get(key) {
  return state.get(key);
}

export function set(key, value) {
  state.set(key, value);
  const subs = listeners.get(key);
  if (subs) for (const fn of Array.from(subs)) {
    try { fn(value); } catch (e) { console.error('listener error', e); }
  }
}

export function subscribe(key, fn) {
  if (!listeners.has(key)) listeners.set(key, new Set());
  listeners.get(key).add(fn);
  return () => listeners.get(key).delete(fn);
}

// 通知（应用级 toast）
let toastTimer = null;
export function notify(msg) {
  let el = document.querySelector('#toast');
  if (!el) return;
  el.textContent = msg;
  el.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.classList.remove('show'), 4000);
}
