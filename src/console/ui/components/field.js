// AIL-082：Field/Input —— 标签、错误关联、保光标的受控输入。

let nextFieldId = 0;
export function Field(container, props) {
  const wrap = document.createElement('div');
  const label = document.createElement('label');
  const input = document.createElement(props.multi ? 'textarea' : 'input');
  const err = document.createElement('div');
  const hint = document.createElement('div');
  input.id = `field-${++nextFieldId}`;
  label.htmlFor = input.id;
  err.id = input.id + '-error';
  hint.id = input.id + '-hint';
  input.setAttribute('aria-describedby', `${hint.id} ${err.id}`);
  label.textContent = props.label ?? '';
  label.className = 'muted';
  if (!props.multi) input.type = props.type ?? 'text';
  input.value = props.value ?? '';
  input.style.width = props.width ?? '100%';
  input.placeholder = props.placeholder ?? '';
  err.className = 'badge bad hidden';
  hint.className = 'muted';
  hint.textContent = props.hint ?? '';
  wrap.append(label, input, err, hint);
  container.appendChild(wrap);
  let cur = props;

  input.oninput = () => {
    // 保光标：仅非受控同步值；受控场景由 update 显式 setValue
    if (cur.onChange) cur.onChange(input.value);
  };
  input.onblur = () => { if (cur.onBlur) cur.onBlur(input.value); };

  function render(p) {
    cur = p;
    label.textContent = p.label ?? '';
    input.placeholder = p.placeholder ?? '';
    if (document.activeElement !== input && input.value !== (p.value ?? '')) {
      input.value = p.value ?? '';
    }
    if (p.error) {
      err.textContent = p.error;
      err.classList.remove('hidden');
    } else {
      err.classList.add('hidden');
    }
    hint.textContent = p.hint ?? '';
  }
  render(props);
  return {
    update(next) { render({ ...cur, ...next }); },
    setValue(v) {
      input.value = v;
      if (cur.onChange) cur.onChange(v);
    },
    value: () => input.value,
    focus() { input.focus(); },
    destroy() { wrap.remove(); },
  };
}
