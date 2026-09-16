// AIL-082：Button —— 防重复点击、可聚焦、pending 态文案。统一工厂接口：
// mount(container, props) -> { update(nextProps), destroy() }

export function Button(container, props) {
  let el = document.createElement('button');
  let pending = false;
  el.type = 'button';
  container.appendChild(el);
  let cur = props;

  function render(p) {
    cur = p;
    el.textContent = pending ? (p.pendingLabel ?? '处理中…') : (p.label ?? '按钮');
    el.disabled = !!p.disabled || (pending && !p.allowWhilePending);
    el.title = p.title ?? '';
  }
  el.onclick = async () => {
    if (pending || el.disabled) return;
    if (cur.onPress) {
      pending = true;
      render(cur);
      try { await cur.onPress(); }
      finally { pending = false; render(cur); }
    }
  };
  render(props);
  return {
    update(next) { render({ ...cur, ...next }); },
    destroy() { el.remove(); el = null; },
  };
}
