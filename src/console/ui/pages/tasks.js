// AIL-087/099（操作记录页）：配置操作留痕 —— 可读的操作类型、项目、状态、时间；
// 技术 ID 等细节放所选记录详情。持久化、重启恢复；中断操作不自动重放；
// 可对成功 apply 撤销（确认后执行，冲突保留用户修改）。

import { api, esc } from '../services/api.js';
import { PageHeader } from '../components/layout.js';
import { Item, paginate, Pagination, bindPagination } from '../components/item.js';
import { confirmAction } from '../components/dialog.js';
import { notify } from '../state/store.js';
import { collectionDetails, revisionDetails } from '../features/knowledgeOrganization.js';

const KIND_LABEL = { plan: '生成预览', apply: '应用配置', knowledge_organize:'整理知识库' };
const STATUS_LABEL = {
  queued: '排队中', running: '执行中', success: '成功', failed: '失败',
  cancelled: '已取消', interrupted: '已中断', undone: '已撤销', undo_partial: '部分撤销',
};
const STATUS_TONE = { success: 'ok', failed: 'bad', interrupted: 'warn', undo_partial: 'warn', running: 'warn' };

const kindLabel = (k) => KIND_LABEL[k] || k;
const statusLabel = (s,result) => result?.needs_revision?'待修订':STATUS_LABEL[s] || s;
const pathLeaf = (p) => String(p || '').split('/').filter(Boolean).pop() || '—';
const timeLabel = (iso) => { const d = iso ? new Date(iso) : null; return d && !Number.isNaN(d.getTime()) ? d.toLocaleString() : '—'; };

export function mount(container, ctx) {
  const root = document.createElement('div');
  root.className = 'tasks-page';
  container.appendChild(root);
  root.innerHTML = `${PageHeader({title:'操作记录',description:'预览、应用与撤销分别留痕，不自动重放。',actions:'<button data-refresh>刷新</button>'})}
    <div data-table class="history-list"></div><div data-pagination></div>
    <section data-detail hidden class="step" tabindex="-1"><h2>操作结果</h2><div data-detail-body></div>
      <p data-undo-actions><button data-undo data-variant="destructive" disabled>撤销本次应用</button> <span class="muted">保留此后修改过的文件。</span></p></section>
    <div data-msg class="muted" role="status" aria-live="polite"></div>`;
  const msg = root.querySelector('[data-msg]');
  const detail = root.querySelector('[data-detail]');
  const detailBody = root.querySelector('[data-detail-body]');
  const undoBtn = root.querySelector('[data-undo]');
  let selected = null;
  let jobs = [], listPage = 1, disposed = false, projectLabel = job=>pathLeaf(job.root);

  function canUndo(j) { return j.kind === 'apply' && (j.status === 'success' || j.status === 'undo_partial'); }

  function showDetail(j,announce=true) {
    selected = j;
    root.querySelectorAll('[data-job]').forEach(row=>row.setAttribute('aria-current',String(row.dataset.job===j.id)));
    detail.hidden = false;
    undoBtn.disabled = !canUndo(j);
    root.querySelector('[data-undo-actions]').hidden=!canUndo(j);
    const progress = (j.progress ?? []).length ? `<p>进度：</p><pre class="log">${esc(j.progress.join('\n'))}</pre>` : '';
    const changes=Array.isArray(j.result?.actions)?j.result.actions:Array.isArray(j.result?.applied)?j.result.applied:[];
    const log=changes.map(change=>typeof change==='string'?change:[change.action||'applied',change.path||change.resource_id||''].join('  ')).join('\n');
    detailBody.innerHTML = `<div class="task-summary"><strong>${esc(kindLabel(j.kind))}</strong><span class="badge ${j.result?.needs_revision?'warn':STATUS_TONE[j.status]||''}">${esc(statusLabel(j.status,j.result))}</span></div><p class="path">${esc(j.root)}${j.scope?'/'+esc(j.scope):''}</p><p class="muted">${esc(timeLabel(j.updated_at))}</p>${log?`<pre class="log">${esc(log)}</pre>`:''}${j.error?`<p class="field-error" role="alert">${esc(j.error)}</p>`:''}<details><summary>技术详情</summary><dl><dt>记录 ID</dt><dd>${esc(j.id)}</dd><dt>创建时间</dt><dd>${esc(timeLabel(j.created_at))}</dd><dt>子目录</dt><dd>${esc(j.scope||'根目录')}</dd></dl>${progress}</details>`;
    if(j.kind==='knowledge_organize')detailBody.insertAdjacentHTML('beforeend',collectionDetails(j.result?.collection)+revisionDetails(j.result));
    if(j.kind==='knowledge_organize'&&j.result?.report)detailBody.insertAdjacentHTML('beforeend',`<p class="muted">${j.result.saved?'已保存到知识库，后续检索可用。':'本次结果尚未保存到知识库。'}</p><pre class="knowledge-organization-report">${esc(j.result.report)}</pre>`);
    if(announce)msg.textContent = `已选 ${kindLabel(j.kind)} · ${statusLabel(j.status,j.result)}。`;
    if(announce)detail.scrollIntoView({block:'nearest'});
  }

  async function refresh() {
    try {
      const [v,state] = await Promise.all([api.jobs(),api.state().catch(()=>({repos:[]}))]);
      jobs = [...(v.jobs ?? [])].sort((a,b)=>new Date(b.updated_at)-new Date(a.updated_at));
      projectLabel=job=>{
        const repo=(state.repos||[]).find(r=>Object.values(r.worktrees||{}).some(w=>w.path===job.root));
        return repo?.project?.name||pathLeaf(job.root);
      };
      if(disposed)return;
      renderList();
      if(jobs.length)showDetail(jobs.find(j=>j.id===selected?.id)||jobs[0],false);
    } catch(e) { if(!disposed)msg.textContent=e.message; }
  }
  function renderList() {
    const page=paginate(jobs,listPage);listPage=page.page;
    root.querySelector('[data-table]').innerHTML=page.items.length?page.items.map(j=>Item({title:kindLabel(j.kind),primaryAction:{attributes:`data-job-open="${esc(j.id)}"`,label:'查看'+kindLabel(j.kind)},description:projectLabel(j)+(j.scope?' / '+j.scope:''),trailing:`<span class="history-item-meta"><span class="badge ${j.result?.needs_revision?'warn':STATUS_TONE[j.status]||''}">${esc(statusLabel(j.status,j.result))}</span><span>${esc(timeLabel(j.updated_at))}</span></span>`,density:'regular',attributes:`data-job="${esc(j.id)}" aria-current="${selected?.id===j.id}"`})).join(''):'<div class="empty-state">还没有操作记录。</div>';
    root.querySelectorAll('[data-job-open]').forEach(button=>{button.onclick=()=>showDetail(jobs.find(j=>j.id===button.dataset.jobOpen));});
    root.querySelector('[data-pagination]').innerHTML=Pagination(page);
    bindPagination(root.querySelector('[data-pagination]'),next=>{listPage=next;renderList();});
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
  return { destroy() { disposed=true;root.remove(); } };
}
