// AIL-084：ConflictPanel —— 本地/服务器草稿或版本冲突的显式处理界面；
// 不自动覆盖：用户显式选择 采用服务器 / 用本地覆盖 / 查看差异。

import { esc } from '../services/api.js';
import { DiffView } from './diffView.js';

export function ConflictPanel(container, props) {
  const wrap = document.createElement('div');
  wrap.className = 'step';
  container.appendChild(wrap);
  let cur = props;
  let diff = null;

  function render(p) {
    cur = p;
    wrap.innerHTML = `
      <h2>冲突：${esc(p.title ?? '内容已被其他会话修改')}</h2>
      <p class="muted">${esc(p.detail ?? '')} 你的本地草稿已保留。请显式选择处理方式。</p>
      <div data-diff></div>
      <p>
        <button data-act="diff">查看差异</button>
        <button data-act="server">采用服务器版本</button>
        <button data-act="local">用本地覆盖</button>
      </p>`;
    const slot = wrap.querySelector('[data-diff]');
    diff = DiffView(slot, { path: p.path, before: p.server, after: p.local });
    wrap.querySelector('[data-act="diff"]').onclick = () => slot.classList.toggle('hidden');
    wrap.querySelector('[data-act="server"]').onclick = () => p.onReload?.();
    wrap.querySelector('[data-act="local"]').onclick = () => p.onMerge?.();
  }
  render(props);
  return {
    update(next) { render({ ...cur, ...next }); },
    destroy() { wrap.remove(); },
  };
}
