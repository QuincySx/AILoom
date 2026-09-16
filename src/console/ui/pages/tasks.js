// AIL-087（tasks 页）：任务中心 —— 持久化任务列表、中断标记、undo 入口。
// 中断任务不自动重放；可对成功 plan 再 apply、对成功 apply 撤销。

import { api, esc } from '../services/api.js';
import { DataTable } from '../components/dataTable.js';
import { notify } from '../state/store.js';

export function mount(container, ctx) {
  const root = document.createElement('div');
  container.appendChild(root);
  root.innerHTML = `
    <div class="step"><h2>任务（重启后从磁盘恢复；中断任务不自动重放）</h2>
      <p><button data-refresh>刷新</button></p>
      <div data-table></div>
      <p><button data-undo disabled>撤销选中 apply 任务</button></p>
      <div data-msg class="muted"></div></div>`;
  const table = DataTable(root.querySelector('[data-table]'), { loading: true });
  const msg = root.querySelector('[data-msg]');
  const undoBtn = root.querySelector('[data-undo]');
  let selected = null;

  async function refresh() {
    try {
      const v = await api.jobs();
      const rows = (v.jobs ?? []).map((j) => ({
        id: j.id, kind: j.kind, status: j.status,
        root: j.root, updated: j.updated_at,
      }));
      table.update({
        rows,
        rowKey: (r) => r.id,
        empty: '还没有任务',
        columns: [
          { key: 'id', label: '任务' },
          { key: 'kind', label: '类型' },
          { render: (r) => r.status === 'interrupted' ? '<span class="badge warn">interrupted</span>' : esc(r.status), label: '状态' },
          { key: 'root', label: '目标' },
          { key: 'updated', label: '更新时间' },
        ],
        onSelect: (r) => {
          selected = r;
          undoBtn.disabled = !(r.kind === 'apply' && (r.status === 'success' || r.status === 'undo_partial'));
          msg.textContent = `已选 ${r.id}（${r.status}）`;
        },
      });
    } catch (e) {
      table.update({ error: e.message });
    }
  }

  undoBtn.onclick = async () => {
    if (!selected) return;
    try {
      const v = await api.undo(selected.id);
      msg.textContent = `撤销：恢复 ${v.restored.length} 项；冲突保留 ${(v.conflicts ?? []).length} 项`;
      refresh();
    } catch (e) {
      notify('撤销失败：' + e.message);
    }
  };
  root.querySelector('[data-refresh]').onclick = refresh;
  refresh();
  return { destroy() { root.remove(); } };
}
