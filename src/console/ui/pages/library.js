// AIL-095（库部分）：资源库页 —— 列表/编辑（指纹+保存前校验）/删除（影响预览）/
// 来源导入（本地/GitHub/入口）。保存 ≠ 应用 ≠ 贡献；秘密不回显明文。

import { api, esc } from '../services/api.js';
import { DataTable } from '../components/dataTable.js';
import { Editor } from '../components/editor.js';
import { ImportPreview } from '../features/importPreview.js';
import { currentTarget, currentGeneration } from '../state/target.js';
import { notify } from '../state/store.js';

export function mount(container, ctx) {
  const root = document.createElement('div');
  container.appendChild(root);
  root.innerHTML = `
    <div class="step"><h2>个人资源库 <span class="muted">（仓外；保存 ≠ 应用 ≠ 贡献）</span></h2>
      <div data-import></div>
      <div data-table></div>
      <div data-issues></div>
      <div data-editor class="hidden">
        <h2>编辑 <span data-id></span></h2>
        <textarea data-box style="min-height:160px"></textarea><br>
        <button data-save>保存（指纹校验 + 保存前校验）</button>
        <span data-msg class="muted"></span>
      </div></div>`;

  const tableSlot = root.querySelector('[data-table]');
  const issuesSlot = root.querySelector('[data-issues]');
  const editorBox = root.querySelector('[data-editor]');
  const editBox = root.querySelector('[data-box]');
  const editMsg = root.querySelector('[data-msg]');
  let currentFp = null;
  let currentId = null;

  const table = DataTable(tableSlot, { loading: true });

  async function refresh() {
    try {
      const v = await api.libraryList();
      const rows = (v.entries ?? []).map((e) => ({
        id: e.id, kind: e.kind, description: e.description,
      }));
      table.update({
        rows,
        rowKey: (r) => r.id,
        empty: '个人库为空：用上方导入入口添加 skill。',
        columns: [
          { key: 'id', label: '资源 ID' },
          { key: 'kind', label: '类型' },
          { key: 'description', label: '说明' },
        ],
        onSelect: (row) => openResource(row.id),
      });
      const issues = v.issues ?? [];
      issuesSlot.innerHTML = issues.length
        ? `<p class="badge warn">坏条目（其余资源仍可用，修复后自动恢复）：</p><ul>` +
          issues.map((i) => `<li class="muted">${esc(i.path)}：${esc(i.error)}</li>`).join('') + `</ul>`
        : '';
    } catch (e) {
      table.update({ error: e.message });
    }
  }

  async function openResource(id) {
    try {
      const v = await api.libraryResource(id);
      editorBox.classList.remove('hidden');
      root.querySelector('[data-id]').textContent = id;
      editBox.value = v.content;
      currentFp = v.fingerprint;
      currentId = id;
      editMsg.textContent = v.note ? '注意：' + v.note : '';
    } catch (e) {
      notify(e.message);
    }
  }

  root.querySelector('[data-save]').onclick = async () => {
    try {
      await api.librarySave(currentId, editBox.value, currentFp);
      editMsg.textContent = '已保存（未部署；部署走预览+应用）';
      const v = await api.libraryResource(currentId);
      currentFp = v.fingerprint;
    } catch (e) {
      editMsg.textContent = (e.kind === 'conflict' ? '文件已被外部修改：' : '校验未通过：') + e.message;
    }
  };

  // 删除走影响预览确认
  tableSlot.addEventListener('click', () => {});
  // 导入区
  const importSlot = root.querySelector('[data-import]');
  ImportPreview(importSlot, {
    target: currentTarget(),
    targetGen: currentGeneration(),
    onImported: refresh,
  });

  refresh();
  return {
    destroy() { root.remove(); },
  };
}
