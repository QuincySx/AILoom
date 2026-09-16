// AIL-082：TriStateSelect —— inherit/enable/disable 三态；当前层选择与 effective 分开展示。

import { esc } from '../services/api.js';

export function TriStateSelect(container, props) {
  const wrap = document.createElement('span');
  const sel = document.createElement('select');
  for (const [v, label] of [['inherit', '继承'], ['enable', '启用'], ['disable', '禁用']]) {
    const o = document.createElement('option');
    o.value = v;
    o.textContent = label;
    sel.appendChild(o);
  }
  const eff = document.createElement('span');
  eff.className = 'muted';
  wrap.append(sel, eff);
  container.appendChild(wrap);
  let cur = props;

  sel.onchange = () => { if (cur.onChange) cur.onChange(sel.value); };

  function render(p) {
    cur = p;
    sel.value = p.value ?? 'inherit';
    sel.disabled = !!p.disabled;
    const e = p.effective;
    eff.textContent = e
      ? ` 生效：${e.enabled ? '启用' : '停用'}（${esc(e.originLabel ?? '')}）`
      : '';
  }
  render(props);
  return {
    update(next) { render({ ...cur, ...next }); },
    destroy() { wrap.remove(); },
  };
}
