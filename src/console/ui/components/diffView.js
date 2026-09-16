// AIL-084：DiffView —— before/after 行级差异展示；路径/操作/长文折叠。

import { esc } from '../services/api.js';

export function DiffView(container, props) {
  const wrap = document.createElement('div');
  container.appendChild(wrap);
  let cur = props;
  let expanded = false;

  function diffLines(before, after) {
    const out = [];
    const a = (before ?? '').split('\n');
    const b = (after ?? '').split('\n');
    for (const line of a) if (!b.includes(line)) out.push(['-', line]);
    for (const line of b) if (!a.includes(line)) out.push(['+', line]);
    return out;
  }

  function render(p) {
    cur = p;
    const lines = diffLines(p.before, p.after);
    const head = `<p class="muted">${esc(p.path ?? '')}${p.operation ? ' · ' + esc(p.operation) : ''} · ${lines.length} 处差异</p>`;
    if (!lines.length) {
      wrap.innerHTML = head + '<p class="muted">（无差异）</p>';
      return;
    }
    const shown = expanded || lines.length <= 40 ? lines : lines.slice(0, 40);
    const body = shown.map(([sign, line]) => esc(sign + ' ' + line)).join('\n');
    wrap.innerHTML =
      head + `<pre class="log">${body}</pre>` +
      (shown.length < lines.length
        ? `<button class="muted" data-more>展开其余 ${lines.length - shown.length} 行</button>`
        : '');
    const more = wrap.querySelector('[data-more]');
    if (more) more.onclick = () => { expanded = true; render(cur); };
  }
  render(props);
  return {
    update(next) { render({ ...cur, ...next }); },
    destroy() { wrap.remove(); },
  };
}
