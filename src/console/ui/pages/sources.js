// AIL-095（来源部分）+ AIL-068：来源页 —— 来源身份/版本清单 + UpdatePanel（检查/更新）。

import { api, esc } from '../services/api.js';
import { UpdatePanel } from '../features/importPreview.js';
import { DataTable } from '../components/dataTable.js';

export function mount(container, ctx) {
  const root = document.createElement('div');
  container.appendChild(root);
  root.innerHTML = `
    <div class="step"><h2>来源（上游 → 个人库 → 工作树/宿主）</h2>
      <p class="muted">远程来源默认只读取；更新仅写个人库（旧版备份），部署需重新预览+应用。
      同名不同来源不混同；旧数据按「本地管理」解释。</p>
      <div data-table></div></div>
    <div data-update></div>`;

  const table = DataTable(root.querySelector('[data-table]'), { loading: true });

  async function refresh() {
    try {
      const v = await api.librarySources();
      table.update({
        rows: v.items ?? [],
        rowKey: (i) => i.skill,
        columns: [
          { key: 'skill', label: 'skill' },
          { render: (i) => i.legacy ? '<span class="badge warn">本地管理（无来源记录）</span>' : esc(i.source?.source_kind ?? '') , label: '来源类型' },
          { render: (i) => esc(i.source?.repo_url ?? i.source?.discovery_entry ?? ''), label: '上游/入口' },
          { render: (i) => esc((i.source?.resolved_commit ?? '').slice(0, 12)), label: '锁定 commit' },
        ],
        empty: '库为空',
      });
    } catch (e) {
      table.update({ error: e.message });
    }
  }

  const updateSlot = root.querySelector('[data-update]');
  UpdatePanel(updateSlot, { onUpdated: refresh });
  refresh();
  return { destroy() { root.remove(); } };
}
