// AIL-084：Editor —— documentId/content/baseRevision(validation) 受控编辑器；
// 不丢输入与版本；保存动作由页面/feature 注入（组件不发请求）。

import { Field } from './field.js';

export function Editor(container, props) {
  const box = document.createElement('div');
  container.appendChild(box);
  const field = Field(box, { label: props.title ?? '', multi: true, width: '100%' });
  const meta = document.createElement('div');
  meta.className = 'muted';
  box.appendChild(meta);
  let cur = props;
  let dirty = false;

  field && (field.update({ hint: props.hint ?? '' }));

  function render(p) {
    cur = p;
    if (!dirty && p.content !== undefined) field.setValue(p.content);
    const bits = [];
    if (p.documentId) bits.push('文档 ' + p.documentId);
    if (p.baseRevision !== undefined && p.baseRevision !== null) bits.push('版本 ' + p.baseRevision);
    if (dirty) bits.push('（有未保存修改）');
    meta.textContent = bits.join(' · ');
  }
  // 监听输入置 dirty
  const origOnChange = props.onChange;
  cur = { ...props, onChange: (v) => { dirty = true; if (origOnChange) origOnChange(v); } };

  render(props);
  return {
    update(next) { render({ ...cur, ...next }); },
    setValue(v) { field.setValue(v); dirty = false; },
    value: () => field.value(),
    isDirty: () => dirty,
    destroy() { box.remove(); },
  };
}
