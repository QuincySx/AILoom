// AIL-084：Editor —— documentId/content/baseRevision(validation) 受控编辑器；
// 不丢输入与版本；保存动作由页面/feature 注入（组件不发请求）。

import { Field } from './field.js';

export function Editor(container, props) {
  const box = document.createElement('div');
  container.appendChild(box);
  let cur = props;
  let saved = props.content ?? '';
  let dirty = false;
  const field = Field(box, {
    label: props.title ?? '', multi: true, width: '100%', value: saved,
    onChange: (value) => { dirty = value !== saved; renderMeta(); cur.onChange?.(value); },
  });
  const meta = document.createElement('div');
  meta.className = 'muted';
  box.appendChild(meta);
  function renderMeta() {
    const p = cur;
    const bits = [];
    if (p.documentId) bits.push('文档 ' + p.documentId);
    if (p.baseRevision !== undefined && p.baseRevision !== null) bits.push('版本 ' + p.baseRevision);
    if (dirty) bits.push('（有未保存修改）');
    meta.textContent = bits.join(' · ');
  }
  renderMeta();
  return {
    update(next) {
      cur = { ...cur, ...next };
      if (!dirty && next.content !== undefined) { saved = next.content; field.setValue(saved); }
      renderMeta();
    },
    setValue(v) { saved = v; field.setValue(v); dirty = false; renderMeta(); },
    markSaved(value = field.value()) { saved = value; dirty = field.value() !== saved; renderMeta(); },
    value: () => field.value(),
    isDirty: () => dirty,
    destroy() { box.remove(); },
  };
}
