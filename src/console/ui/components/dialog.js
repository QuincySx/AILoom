// AIL-084：Dialog —— 焦点约束与恢复、dirty 关闭确认；不做网络请求。

export function Dialog(container, props) {
  const overlay = document.createElement('div');
  overlay.style.cssText = 'position:fixed;inset:0;background:#0006;display:flex;align-items:center;justify-content:center;z-index:50;';
  const box = document.createElement('div');
  box.className = 'step';
  box.style.cssText = 'max-width:640px;max-height:80vh;overflow:auto;background:var(--c-surface);';
  const title = document.createElement('h2');
  const body = document.createElement('div');
  const actions = document.createElement('div');
  actions.style.textAlign = 'right';
  box.append(title, body, actions);
  overlay.appendChild(box);
  let cur = props;
  let prevFocus = document.activeElement;

  function render(p) {
    cur = p;
    title.textContent = p.title ?? '';
    if (typeof p.content === 'string') body.innerHTML = p.content;
    else if (p.content) { body.innerHTML = ''; body.appendChild(p.content); }
    actions.innerHTML = '';
    for (const a of p.actions ?? []) {
      const b = document.createElement('button');
      b.textContent = a.label;
      b.onclick = async () => {
        if (a.keepOpen && a.onAction) {
          const ok = await a.onAction();
          if (!ok) return;
        } else if (a.onAction) await a.onAction();
        if (!a.keepOpen) close(a.returnValue);
      };
      actions.appendChild(b);
    }
  }
  function close(v) {
    overlay.remove();
    document.removeEventListener('keydown', onKey);
    if (prevFocus?.focus) prevFocus.focus();
    if (cur.onClose) cur.onClose(v);
  }
  function onKey(e) {
    if (e.key === 'Escape') {
      if (cur.dirty && !confirm('有未保存修改，确定关闭？')) return;
      close(undefined);
    }
    // 焦点约束
    const focusables = box.querySelectorAll('button,input,textarea,select');
    if (e.key === 'Tab' && focusables.length) {
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      if (e.shiftKey && document.activeElement === first) { last.focus(); e.preventDefault(); }
      else if (!e.shiftKey && document.activeElement === last) { first.focus(); e.preventDefault(); }
    }
  }
  overlay.addEventListener('click', (e) => {
    if (e.target === overlay) {
      if (cur.dirty && !confirm('有未保存修改，确定关闭？')) return;
      close(undefined);
    }
  });
  document.addEventListener('keydown', onKey);
  document.body.appendChild(overlay);
  render(props);
  const first = box.querySelector('button,input,textarea,select');
  if (first) first.focus();
  return {
    update: render,
    close,
    destroy() { overlay.remove(); document.removeEventListener('keydown', onKey); },
  };
}
