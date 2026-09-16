// AIL-095（来源部分）+ AIL-068：来源页 —— 来源身份/版本清单 + UpdatePanel（检查/更新）。

import { api, esc } from '../services/api.js';
import { UpdatePanel } from '../features/importPreview.js';
import { DataTable } from '../components/dataTable.js';
import { currentTarget } from '../state/target.js';
import { esc as esc2 } from '../services/api.js';

export function mount(container, ctx) {
  const root = document.createElement('div');
  container.appendChild(root);
  root.innerHTML = `
    <div class="step"><h2>来源（上游 → 个人库 → 工作树/宿主）</h2>
      <p class="muted">远程来源默认只读取；更新仅写个人库（旧版备份），部署需重新预览+应用。
      同名不同来源不混同；旧数据按「本地管理」解释。</p>
      <div data-table></div></div>
    <div data-update></div>
    <div class="step"><h2>部署状态（库版本 vs 工作树已部署版本）</h2>
      <p class="muted">当前操作目标：目标页选择后展示。stale = 库已更新、需重新预览+应用；not-deployed = 未部署到该工作树。</p>
      <p><button data-deploy>刷新部署状态</button></p>
      <div data-deployview class="muted">未加载</div></div>`;

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
  const dview = root.querySelector('[data-deployview]');
  root.querySelector('[data-deploy]').onclick = async () => {
    const target = currentTarget();
    if (!target?.path) { dview.textContent = '先在「仓库与作用域」选择操作目标'; return; }
    try {
      const v = await api.deployStatus(target.path);
      dview.innerHTML = '<table><tr><th>资源</th><th>宿主</th><th>路径</th><th>状态</th></tr>' +
        (v.items ?? []).map((i) => `<tr><td>${esc2(i.resource_id)}</td><td>${esc2(i.tool)}</td><td>${esc2(i.path)}</td><td>${i.state === 'current' ? '<span class="badge ok">current</span>' : i.state === 'stale（库已更新，需重新预览+应用）'.includes('stale') ? '<span class="badge warn">stale（待同步）</span>' : '<span class="badge">not-deployed</span>'}</td></tr>`).join('') +
        '</table>';
    } catch (e) {
      dview.innerHTML = `<span class="badge bad">${esc2(e.message)}</span>`;
    }
  };
  return { destroy() { root.remove(); } };
}
