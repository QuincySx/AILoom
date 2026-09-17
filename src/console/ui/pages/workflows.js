// AIL-096（+071）：流程与文档工作台 —— 新建/打开流程、阶段绑定、产物编辑/导入/
// 版本前置/改名/复核/导出（指纹前置+备份）。

import { api, esc } from '../services/api.js';
import { WorkflowStageList } from '../features/workflowStageList.js';

export function mount(container, ctx) {
  const root = document.createElement('div');
  container.appendChild(root);
  root.innerHTML = `
    <div class="step"><h2>流程包：对齐 → 规格 → 票据 → 实现 → 验收</h2>
      <input data-name placeholder="流程名" style="width:35%">
      <button data-new>新建流程</button>
      <select data-sel style="width:40%"></select>
      <button data-open>打开</button>
      <div data-wf></div></div>`;

  const sel = root.querySelector('[data-sel]');
  const wfSlot = root.querySelector('[data-wf]');
  let stageList = null;

  async function refreshRuns() {
    const v = await api.workflows().catch(() => ({ runs: [] }));
    sel.innerHTML = (v.runs ?? [])
      .map((r) => `<option value="${esc(r.id)}">${esc(r.id)} ${esc(r.name)}${r.downstream_needs_review ? ' ⚠需复核' : ''}</option>`)
      .join('');
  }

  async function open() {
    if (stageList?.isDirty() && !confirm('当前流程有未保存内容，确定放弃并切换？')) return;
    const id = sel.value;
    if (!id) return;
    const [run, lib] = await Promise.all([api.workflowShow(id), api.libraryList()]);
    stageList?.destroy();
    wfSlot.innerHTML = '';
    stageList = WorkflowStageList(wfSlot, {
      run,
      skills: (lib.entries ?? []).filter((e) => e.kind === 'skill'),
      onChanged: open,
    });
  }

  root.querySelector('[data-new]').onclick = async () => {
    if (stageList?.isDirty() && !confirm('当前流程有未保存内容，确定放弃并新建？')) return;
    const name = root.querySelector('[data-name]').value.trim() || '未命名流程';
    await api.workflowNew(name);
    await refreshRuns();
    const v = await api.workflows();
    if (v.runs?.length) {
      sel.value = v.runs[v.runs.length - 1].id;
      open();
    }
  };
  root.querySelector('[data-open]').onclick = open;

  refreshRuns();
  return {
    isDirty: () => stageList?.isDirty() ?? false,
    destroy() { root.remove(); stageList?.destroy?.(); },
  };
}
