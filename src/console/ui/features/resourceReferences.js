import { api, esc } from '../services/api.js';
import { Dialog } from '../components/dialog.js';

// Read-only usage lookup. All changes happen in the project's directory view.
export function ResourceReferences(container, sourceOrSpec) {
  const source = sourceOrSpec?.resources ? sourceOrSpec : sourceOrSpec?.source;
  const ids = new Set(source ? source.resources.map(r=>r.id) : [sourceOrSpec.resourceId]);
  const name = sourceOrSpec.resourceName || source?.name || '此能力';
  const body = document.createElement('div');
  body.innerHTML = '<div data-usage role="status">正在查找使用位置…</div>';
  let alive = true;
  const modal = Dialog(container,{title:`${name} · 使用项目`,content:body,onClose:()=>{alive=false;}});
  (async()=>{
    try {
      const state = await api.state(), locations = [], failures = [];
      for(const repo of state.repos || []) {
        if(!alive)return;
        const git=!repo.repo_id.startsWith('nongit-');
        const worktrees=Object.keys(repo.worktrees||{}).length?repo.worktrees:git?{}:{root:{path:repo.common_dir}};
        let projectAdded=false;
        for(const [wt,w] of Object.entries(worktrees)) {
          try {
            await api.approveDir(w.path);
            const dirs=await api.projectDirs(w.path);
            const choices=[...(git&&!projectAdded?[{dir:'',node:'project',view:'project'}]:[]),{dir:'',node:git?'worktree':'project'},...dirs.dirs.filter(d=>!d.missing).map(d=>({dir:d.path,node:'directory'}))];
            for(const choice of choices) {
              if(!alive)return;
              const eff=await api.effective(w.path,choice.dir||null,choice.view);
              if(choice.view==='project')projectAdded=true;
              if(![...ids].some(id=>eff.resources?.[id]?.deployed))continue;
              const project=repo.project?.name || w.path.split('/').pop() || '项目';
              const branch=String(w.branch||'').replace(/^refs\/heads\//,'') || w.path.split('/').pop();
              locations.push({repoId:repo.repo_id,wt,dir:choice.dir,node:choice.node,project,label:choice.node==='project'?'整个项目':[git?branch:'',choice.dir].filter(Boolean).join(' / ')});
            }
          } catch { failures.push(repo.project?.name || w.path.split('/').pop() || '项目'); }
        }
      }
      if(!alive)return;
      body.innerHTML=locations.length?`<div class="usage-list">${locations.map((l,i)=>`<a class="usage-location" href="#/projects/${encodeURIComponent(l.repoId)}" data-usage-location="${i}"><strong>${esc(l.project)}</strong><span>${esc(l.label)}</span><span>打开 →</span></a>`).join('')}</div>`:'<p class="muted">尚未找到使用此能力的项目。</p>';
      if(failures.length)body.insertAdjacentHTML('beforeend',`<p class="field-error">暂时无法检查：${esc([...new Set(failures)].join('、'))}</p>`);
      body.querySelectorAll('[data-usage-location]').forEach(a=>{a.onclick=()=>{
        const l=locations[Number(a.dataset.usageLocation)];
        const state={repoId:l.repoId,wt:l.wt,dir:l.dir,node:l.node,tab:0};
        sessionStorage.setItem('ailoom-directory-view',JSON.stringify(state));
        sessionStorage.setItem('ailoom-return-after-settings',JSON.stringify(state));
        modal.close();
      };});
    }catch(e){if(alive)body.textContent='无法读取项目：'+e.message;}
  })();
  return {close:()=>modal.close(),destroy(){alive=false;modal.destroy();}};
}
