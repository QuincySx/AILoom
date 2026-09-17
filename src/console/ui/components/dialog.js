// Native modal dialog: top layer, inert background, focus restoration and shared footer.
let nextDialogId = 0;

export function Dialog(container, props) {
  const box = document.createElement('dialog');
  box.className = 'dialog-content';
  box.setAttribute('role', 'dialog');
  box.setAttribute('aria-modal', 'true');
  const header = document.createElement('header');
  header.className = 'dialog-header';
  const title = document.createElement('h2');
  title.id = `dialog-title-${++nextDialogId}`;
  box.setAttribute('aria-labelledby', title.id);
  const dismiss = document.createElement('button');
  dismiss.type = 'button';
  dismiss.dataset.variant = 'ghost';
  dismiss.className = 'dialog-dismiss';
  dismiss.setAttribute('aria-label', '关闭弹窗');
  dismiss.textContent = '关闭';
  header.append(title, dismiss);
  const body = document.createElement('div');
  body.className = 'dialog-body';
  const actions = document.createElement('footer');
  actions.className = 'dialog-actions';
  box.append(header, body, actions);
  let cur = props, prevFocus = null, disposed = false, checking = false;
  function render(p) {
    cur = p;
    title.textContent = p.title || '';
    if (p.content && body.firstChild !== p.content) {
      if (typeof p.content === 'string') body.innerHTML = p.content;
      else body.replaceChildren(p.content);
    }
    actions.replaceChildren();
    actions.hidden = !(p.actions?.length);
    for (const action of p.actions || []) {
      const button = document.createElement('button');
      button.type = 'button';
      button.textContent = action.label;
      button.dataset.variant = action.variant || 'outline';
      button.onclick = async () => {
        if (button.disabled) return;
        button.disabled = true;
        try {
          const result = await action.onAction?.();
          if (action.keepOpen || result === false) return;
          close(action.returnValue);
        } finally { button.disabled = false; }
      };
      actions.append(button);
    }
  }
  function show() {
    if (disposed || box.open) return;
    prevFocus = document.activeElement;
    box.showModal();
    (body.querySelector('input:not(:disabled),select:not(:disabled),textarea:not(:disabled),button:not(:disabled)') || actions.querySelector('button') || dismiss).focus();
  }
  function close(value) {
    if (disposed || !box.open) return;
    box.close();
    if (prevFocus?.isConnected) prevFocus.focus();
    cur.onClose?.(value);
    if (!cur.keepMounted) { box.remove(); disposed = true; }
  }
  async function requestClose() {
    if (checking || cur.canClose?.() === false) return;
    checking = true;
    try {
      const dirty = typeof cur.dirty === 'function' ? cur.dirty() : cur.dirty;
      if (dirty && !await confirmAction('有未保存修改，确定放弃并关闭？', {title:'放弃修改', confirmLabel:'放弃修改', destructive:true})) return;
      close();
    } finally { checking = false; }
  }
  dismiss.onclick = requestClose;
  box.addEventListener('cancel', event => { event.preventDefault(); requestClose(); });
  box.addEventListener('click', event => {
    const rect = box.getBoundingClientRect();
    if (event.target === box && (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom)) requestClose();
  });
  render(props);
  container.append(box);
  if (props.open !== false) show();
  return {
    update(next) { render({...cur, ...next}); },
    show, close,
    get isOpen() { return box.open; },
    destroy() {
      if (disposed) return;
      if (box.open) { box.close(); if (prevFocus?.isConnected) prevFocus.focus(); }
      box.remove(); disposed = true;
    },
  };
}

export function confirmAction(message, options = {}) {
  return new Promise(resolve => {
    const text = document.createElement('p');
    text.className = 'confirmation-message';
    text.textContent = message;
    Dialog(document.body, {
      title: options.title || '确认操作', content:text,
      actions:[
        {label:'取消', returnValue:false},
        {label:options.confirmLabel || '确认', variant:options.destructive ? 'destructive' : 'default', returnValue:true},
      ],
      onClose:value => resolve(value === true),
    });
  });
}
