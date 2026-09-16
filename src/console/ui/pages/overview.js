// AIL-087：总览页 —— 最近目标、任务/验证状态摘要；只导航，不做写动作。

import { api, esc } from '../services/api.js';
import { DataTable } from '../components/dataTable.js';
import { notify } from '../state/store.js';

export function mount(container, ctx) {
  const root = document.createElement('div');
  container.appendChild(root);
  root.innerHTML = `
    <div class="step"><h2>总览</h2>
      <p class="muted">从下面的入口开始；写动作都在对应页面完成。</p>
      <p>
        <a href="#/onboarding">首次设置（六步向导）</a> ·
        <a href="#/scopes">仓库与作用域</a> ·
        <a href="#/library">资源库</a> ·
        <a href="#/sources">来源与更新</a> ·
        <a href="#/workflows">流程工作台</a> ·
        <a href="#/tasks">任务</a> ·
        <a href="#/instructions">个人指令</a>
      </p></div>
    <div class="step"><h2>最近任务</h2><div data-table></div></div>`;
  root.querySelectorAll('a').forEach((a) => {
    a.addEventListener('click', (e) => {
      e.preventDefault();
      location.hash = a.getAttribute('href');
    });
  });
  const table = DataTable(root.querySelector('[data-table]'), { loading: true });
  api.jobs()
    .then((v) => table.update({
      rows: (v.jobs ?? []).slice(-8).reverse(),
      rowKey: (j) => j.id,
      columns: [
        { key: 'id', label: '任务' },
        { key: 'kind', label: '类型' },
        { key: 'status', label: '状态' },
        { key: 'updated_at', label: '更新时间' },
      ],
      empty: '还没有任务',
    }))
    .catch((e) => table.update({ error: e.message }));
  return { destroy() { root.remove(); } };
}
