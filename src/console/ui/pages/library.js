// AIL-095（库部分）：资源库页 —— 列表/编辑（指纹+保存前校验）/删除（影响预览）/
// 来源导入（本地/GitHub/入口）。保存 ≠ 应用 ≠ 贡献；秘密不回显明文。

import { ManagedDefinition } from '../features/managedDefinition.js';
import { api, esc } from '../services/api.js';
import { Editor } from '../components/editor.js';
import { CollectionsPanel } from '../features/collectionsPanel.js';
import { notify } from '../state/store.js';
import { Dialog, confirmAction } from '../components/dialog.js';

export function mount(container, ctx) {
  const root = document.createElement('div');
  root.className = 'library-page';
  container.appendChild(root);
  root.innerHTML = `
    <div data-collections></div>
      <div data-issues></div>
      <div data-editor class="hidden">
        <h2>编辑 <span data-id></span></h2>
        <div data-box></div><br>
        <button data-save>保存修改</button>
        <button data-delete class="danger">删除…</button>
        <span data-edit-msg class="muted" role="status" aria-live="polite"></span>
      </div>`;

  const issuesSlot = root.querySelector('[data-issues]');
  const editorBox = root.querySelector('[data-editor]');
  const editor = Editor(root.querySelector('[data-box]'), { title: '资源正文', content: '' });
  const editMsg = root.querySelector('[data-edit-msg]');
  let currentFp = null;
  let currentId = null;
  let loadGeneration = 0;

  const editorDialog = Dialog(root, {title:'编辑能力',content:editorBox,open:false,keepMounted:true,canClose:()=>!root.querySelector('[data-save]').disabled,dirty:()=>editor.isDirty(),onClose:()=>{++loadGeneration;currentId=null;editor.setValue('');}});

  async function refresh() {
    try {
      const v = await api.libraryList();
      const issues = v.issues ?? [];
      issuesSlot.innerHTML = issues.length
        ? `<p class="badge warn">以下能力无法读取：</p><ul>` +
          issues.map((i) => `<li class="muted">${esc(i.path)}：${esc(i.error)}</li>`).join('') + `</ul>`
        : '';
    } catch (e) {
      issuesSlot.textContent = '无法读取本地能力：' + e.message;
    }
  }

  async function openResource(id) {
    if (editor.isDirty() && !await confirmAction('当前资源有未保存修改，确定放弃并打开另一项？', {title:'切换资源', confirmLabel:'放弃并切换'})) return;
    const generation = ++loadGeneration;
    try {
      const v = await api.libraryResource(id);
      if (generation !== loadGeneration) return;
      editorBox.classList.remove('hidden');
      root.querySelector('[data-id]').textContent = id.split('/').pop();
      editorDialog.show();
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
      editMsg.textContent = '已保存。到项目中应用更新。';
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
      if (!await confirmAction('删除 ' + id.split('/').pop() + '？\n使用位置：' + (preview.affected_scopes || []).join('、') + '\n将移入归档。正在使用的能力不能删除。', {title:'删除个人副本', confirmLabel:'移入归档', destructive:true})) return;
      await api.libraryDelete(id, true);
      if (currentId === id) { currentId = null; editor.setValue(''); editorBox.classList.add('hidden'); }
      editorDialog.close();notify('已删除，可从归档恢复。'); await refresh();await collections.refresh?.();
    } catch (e) { notify(e.message); }
  };
  // 导入区（AIL-126：搜索上下文跨页保留）
  const collections = CollectionsPanel(root.querySelector('[data-collections]'), {
    title: '资源库',
    compactLibrary: true,
    onEdit: async id=>{if(/^personal\/(rule|agent)\//.test(id)){await ManagedDefinition(root,{id});await refresh();await collections.refresh();}else await openResource(id);},
    onCreate: async()=>{const result=await ManagedDefinition(root);if(result){await refresh();await collections.refresh();}},
    description: '收集可复用的 Skill、MCP 和 Agent，再添加到需要的目录。',
    searchKey: 'ailoom-library-search',
    onChanged: refresh,
  });

  refresh();
  return {
    isDirty: () => editor.isDirty(),
    destroy() { ++loadGeneration; collections.destroy(); editorDialog.destroy(); root.remove(); },
  };
}
