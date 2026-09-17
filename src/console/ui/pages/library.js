// AIL-095（库部分）：资源库页 —— 列表/编辑（指纹+保存前校验）/删除（影响预览）/
// 来源导入（本地/GitHub/入口）。保存 ≠ 应用 ≠ 贡献；秘密不回显明文。

import { api, esc } from '../services/api.js';
import { DataTable } from '../components/dataTable.js';
import { Editor } from '../components/editor.js';
import { CollectionsPanel } from '../features/collectionsPanel.js';
import { notify } from '../state/store.js';

export function mount(container, ctx) {
  const root = document.createElement('div');
  container.appendChild(root);
  root.innerHTML = `
    <header><h1>全局资源中心</h1><p class="muted">统一管理来源、下载和更新。导入不会自动启用；到项目中选择具体引用，再预览部署。</p><p><a href="#/sources">检查与更新个人副本</a></p></header>
    <div data-collections></div>
    <div class="step"><h2>个人副本</h2><p class="muted">本地文件夹和 skills.sh 单项导入的资源。点击资源查看、编辑或删除；修改副本不会提交上游。</p>
      <div data-table></div>
      <div data-issues></div>
      <div data-editor class="hidden">
        <h2>编辑 <span data-id></span></h2>
        <div data-box></div><br>
        <button data-save>保存（指纹校验 + 保存前校验）</button>
        <button data-delete class="danger">删除个人副本…</button>
        <span data-msg class="muted"></span>
      </div></div>`;

  const tableSlot = root.querySelector('[data-table]');
  const issuesSlot = root.querySelector('[data-issues]');
  const editorBox = root.querySelector('[data-editor]');
  const editor = Editor(root.querySelector('[data-box]'), { title: '资源正文', content: '' });
  const editMsg = root.querySelector('[data-msg]');
  let currentFp = null;
  let currentId = null;
  let loadGeneration = 0;

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
        empty: '还没有个人副本。上方「导入资源」可选择本地文件夹或 skills.sh。Git 合集中的资源显示在来源目录中。',
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
    if (editor.isDirty() && !confirm('当前资源有未保存修改，确定放弃并打开另一项？')) return;
    const generation = ++loadGeneration;
    try {
      const v = await api.libraryResource(id);
      if (generation !== loadGeneration) return;
      editorBox.classList.remove('hidden');
      root.querySelector('[data-id]').textContent = id;
      editor.setValue(v.content);
      currentFp = v.fingerprint;
      currentId = id;
      editMsg.textContent = v.note ? '注意：' + v.note : '';
    } catch (e) {
      notify(e.message);
    }
  }

  root.querySelector('[data-save]').onclick = async () => {
    const savedId = currentId;
    const content = editor.value();
    const button = root.querySelector('[data-save]');
    if (button.disabled || !savedId) return;
    button.disabled = true;
    try {
      const v = await api.librarySave(savedId, content, currentFp);
      if (currentId !== savedId) return;
      editor.markSaved(content);
      editMsg.textContent = '已保存（未部署；部署走预览+应用）';
      currentFp = v.fingerprint;
    } catch (e) {
      editMsg.textContent = (e.kind === 'conflict' ? '文件已被外部修改：' : '校验未通过：') + e.message;
    } finally {
      button.disabled = false;
    }
  };

  // 删除走影响预览确认
  root.querySelector('[data-delete]').onclick = async () => {
    if (!currentId) return;
    const id = currentId;
    try {
      const p = await api.libraryDelete(id, false);
      const preview = p.preview ?? p;
      if (!confirm('删除个人副本 ' + id + '？\n涉及作用域：' + (preview.affected_scopes || []).join('、') + '\n副本会移入本机 library-archive，不删除上游或项目文件。仍启用时会阻止删除。')) return;
      await api.libraryDelete(id, true);
      if (currentId === id) { currentId = null; editor.setValue(''); editorBox.classList.add('hidden'); }
      notify('个人副本已移入本机归档，可恢复。'); await refresh();
    } catch (e) { notify(e.message); }
  };
  // 导入区
  const collections = CollectionsPanel(root.querySelector('[data-collections]'), { onChanged: refresh });

  refresh();
  return {
    isDirty: () => editor.isDirty(),
    destroy() { ++loadGeneration; collections.destroy(); root.remove(); },
  };
}
