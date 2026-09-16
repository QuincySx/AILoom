// AIL-088：Repository/Worktree/ScopePicker —— 仓库→工作树→子项目选择器。
// 只发 onTargetSelected 事件，不自行保存或 apply；展示失联/非 Git 状态。

import { api, esc } from '../services/api.js';
import { DataTable } from '../components/dataTable.js';
import { Field } from '../components/field.js';

export function ScopePicker(container, props) {
  const wrap = document.createElement('div');
  wrap.className = 'step';
  container.appendChild(wrap);
  let table = null;
  let subInput = null;
  let cur = props;

  wrap.innerHTML = `<h2>操作目标（仓库 / 工作树）</h2><div data-table></div><p data-sub></p>`;

  async function refresh() {
    table?.update({ loading: true });
    try {
      const st = await api.state();
      const rows = [];
      for (const r of st.repos ?? []) {
        for (const [wtId, w] of Object.entries(r.worktrees ?? {})) {
          rows.push({ repoId: r.repo_id, wtId, path: w.path, status: w.status, branch: w.branch });
        }
      }
      const gen = cur.targetGen ?? 0;
      table?.update({
        rows,
        rowKey: (r) => r.wtId,
        columns: [
          { key: 'repoId', label: '仓库' },
          { key: 'wtId', label: '工作树 id' },
          { key: 'path', label: '路径' },
          { key: 'status', label: '状态' },
          { key: 'branch', label: '分支' },
        ],
        onSelect: (row) => {
          if (row.status === 'missing') return;
          cur.onTargetSelected?.({ repo_id: row.repoId, wt_id: row.wtId, path: row.path, kind: 'git' }, gen);
        },
        empty: '还没有登记仓库：先在「首次设置」识别目录。',
      });
    } catch (e) {
      table?.update({ error: e.message });
    }
  }

  const tableSlot = wrap.querySelector('[data-table]');
  table = DataTable(tableSlot, { loading: true });
  const subSlot = wrap.querySelector('[data-sub]');
  subInput = Field(subSlot, {
    label: '子项目（可选，相对路径，须存在）',
    placeholder: 'web',
    width: '30%',
    onChange: (v) => { cur.onScopeChange?.(v.trim() || null); },
  });

  function render(p) {
    cur = p;
  }
  render(props);
  refresh();
  return {
    update(next) { render({ ...cur, ...next }); },
    refresh,
    subproject: () => subInput?.value(),
    destroy() { wrap.remove(); },
  };
}
