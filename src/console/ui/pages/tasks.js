// AIL-087/099（操作记录页）：配置操作留痕 —— 可读的操作类型、项目、状态、时间；
// 技术 ID 等细节放所选记录详情。持久化、重启恢复；中断操作不自动重放；
// 可对成功 apply 撤销（确认后执行，冲突保留用户修改）。

import { api, esc } from '../services/api.js';
import { DataTable } from '../components/dataTable.js';
import { confirmAction } from '../components/dialog.js';
import { notify } from '../state/store.js';

const KIND_LABEL = { plan: '生成预览', apply: '应用配置' };
const STATUS_LABEL = {
  queued: '排队中', running: '执行中', success: '成功', failed: '失败',
  cancelled: '已取消', interrupted: '已中断', undone: '已撤销', undo_partial: '部分撤销',
};
const STATUS_TONE = { success: 'ok', failed: 'bad', interrupted: 'warn', undo_partial: 'warn', running: 'warn' };

const kindLabel = (k) => KIND_LABEL[k] || k;
const statusLabel = (s) => STATUS_LABEL[s] || s;
const pathLeaf = (p) => String(p || '').split('/').filter(Boolean).pop() || '—';
const timeLabel = (iso) => { const d = iso ? new Date(iso) : null; return d && !Number.isNaN(d.getTime()) ? d.toLocaleString() : '—'; };

export function mount(container, ctx) {
  const root = document.createElement('div');
  root.className = 'tasks-page';
  container.appendChild(root);
  root.innerHTML = `
    <header class="page-head"><div><h1>操作记录</h1>
      <p class="muted">查看改动记录，或撤销一次应用。</p></div>
      <p><button data-refresh>刷新</button></p></header>
    <div data-table></div>
    <section data-detail hidden class="step" tabindex="-1"><h2>操作详情</h2><div data-detail-body></div>
      <p data-undo-actions><button data-undo disabled>撤销这次应用…</button> <span class="muted">只回滚该次应用写入的文件；你事后修改过的文件会冲突保留。</span></p></section>
    <div data-msg class="muted" role="status" aria-live="polite"></div>`;
  const table = DataTable(root.querySelector('[data-table]'), { loading: true });
  const msg = root.querySelector('[data-msg]');
  const detail = root.querySelector('[data-detail]');
  const detailBody = root.querySelector('[data-detail-body]');
  const undoBtn = root.querySelector('[data-undo]');
  let selected = null;
  let jobs = [];

  function canUndo(j) { return j.kind === 'apply' && (j.status === 'success' || j.status === 'undo_partial'); }

  function showDetail(j,announce=true) {
    selected = j;
    detail.hidden = false;
    undoBtn.disabled = !canUndo(j);
    root.querySelector('[data-undo-actions]').hidden=!canUndo(j);
    const progress = (j.progress ?? []).length ? `<p>进度：</p><pre class="log">${esc(j.progress.join('\n'))}</pre>` : '';
    detailBody.innerHTML = `<div class="task-summary"><strong>${esc(kindLabel(j.kind))}</strong><span class="badge ${STATUS_TONE[j.status]||''}">${esc(statusLabel(j.status))}</span></div><p class="path">${esc(j.root)}${j.scope?'/'+esc(j.scope):''}</p><p class="muted">${esc(timeLabel(j.updated_at))}</p>${j.error?`<p class="field-error" role="alert">${esc(j.error)}</p>`:''}<details><summary>技术详情</summary><dl><dt>记录 ID</dt><dd>${esc(j.id)}</dd><dt>创建时间</dt><dd>${esc(timeLabel(j.created_at))}</dd><dt>子目录</dt><dd>${esc(j.scope||'根目录')}</dd></dl>${progress}</details>`;
    if(announce)msg.textContent = `已选 ${kindLabel(j.kind)} · ${statusLabel(j.status)}。`;
    detail.scrollIntoView({block:'nearest'});
  }

  async function refresh() {
    try {
      const [v,state] = await Promise.all([api.jobs(),api.state().catch(()=>({repos:[]}))]);
      jobs = [...(v.jobs ?? [])].sort((a,b)=>new Date(b.updated_at)-new Date(a.updated_at));
      const projectLabel=job=>{
        const repo=(state.repos||[]).find(r=>Object.values(r.worktrees||{}).some(w=>w.path===job.root));
        return repo?.project?.name||pathLeaf(job.root);
      };
      const rows = jobs.map((j) => ({
        j, kind: kindLabel(j.kind), status: statusLabel(j.status),
        project: projectLabel(j), updated: timeLabel(j.updated_at),
      }));
      table.update({
        rows,
        rowKey: (r) => r.j.id,
        empty: '还没有操作记录。',
        columns: [
          { key: 'kind', label: '操作' },
          { key: 'project', label: '项目' },
          { render: (r) => `<span class="badge ${STATUS_TONE[r.j.status] || ''}">${esc(r.status)}</span>`, label: '状态' },
          { key: 'updated', label: '时间' },
        ],
        onSelect: (row) => showDetail(row.j),
      });
    } catch (e) {
      table.update({ error: e.message });
    }
  }

  undoBtn.onclick = async () => {
    if (!selected || !canUndo(selected)) return;
    const j = selected;
    const ok = await confirmAction(
      `撤销“${kindLabel(j.kind)}”（${pathLeaf(j.root)}）？\n`
      + `将把该次应用写入的文件恢复到应用前状态；你事后修改过的文件会冲突保留，不会被覆盖。已纳入 Git 跟踪的新建文件不删除。\n`
      + `不影响其他项目，也不回滚全局资源来源。`,
      { title: '撤销应用', confirmLabel: '确认撤销', destructive: true });
    if (!ok) { msg.textContent = '已取消撤销，未做任何修改。'; return; }
    undoBtn.disabled = true;
    try {
      const v = await api.undo(j.id);
      const conflicts = v.conflicts ?? [];
      msg.textContent = `撤销完成：恢复 ${v.restored.length} 项${conflicts.length ? `；冲突保留 ${conflicts.length} 项（你的修改未被覆盖）` : ''}。`;
      if (conflicts.length) notify('冲突保留：' + conflicts.join('；'));
      await refresh();
      const again = jobs.find((x) => x.id === j.id);
      if (again) showDetail(again,false);
    } catch (e) {
      notify('撤销失败：' + e.message);
      undoBtn.disabled = false;
    }
  };
  root.querySelector('[data-refresh]').onclick = refresh;
  refresh();
  return { destroy() { root.remove(); } };
}
