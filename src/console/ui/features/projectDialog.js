import { api } from '../services/api.js';
import { directoryInfo } from './knowledgePanel.js';
import { recoveryFields, cloneDestination } from './knowledgeRecovery.js';
import { Dialog } from '../components/dialog.js';

// All project-entry points share this form. Inspection never registers a project.
export function ProjectDialog(container, {onCreated} = {}) {
  const form = document.createElement('form');
  let alive=true, busy=false, generation=0, inspected=null, inspectedPath='', createdId=null, autoName='', recoveryPreview=null;
  form.innerHTML = `<fieldset data-project-fields>
    <label>项目文件夹<div class="input-action"><input name="root" required placeholder="选择文件夹或输入绝对路径"><button type="button" data-pick-root>选择文件夹…</button></div></label>
    <p class="muted" data-project-state>选择目录后显示知识库的默认位置。</p>
  </fieldset>
  <div class="form-grid"><label>名称<input name="name" required maxlength="120"></label><label>分类（可选）<input name="category" maxlength="80"></label></div>
  <fieldset data-knowledge disabled><legend>知识库</legend>
    <div data-recovery-fields></div><label>保存目录<div class="input-action"><input name="path" required placeholder="先选择项目文件夹"><button type="button" data-pick-knowledge>选择文件夹…</button></div></label>
    <p class="muted" data-directory-info></p>
    <p class="muted" data-knowledge-note>此项目的 Worktree 和子目录共用。</p>
  </fieldset>
  <p data-preview role="status"></p>
  <p class="field-error" data-error role="alert"></p>
  <footer class="dialog-actions"><button type="button" data-cancel>取消</button><button type="submit" class="primary" disabled>添加项目</button></footer>`;
  const q=s=>form.querySelector(s), f=name=>form.elements.namedItem(name);
  const message=text=>{if(alive)q('[data-error]').textContent=text;};
  const modal=Dialog(container,{title:'添加项目',content:form,canClose:()=>!busy,onClose:()=>{alive=false;generation++;}});
  function controls() {
    if(!alive)return;
    q('[data-project-fields]').disabled=busy||!!createdId;
    f('name').disabled=f('category').disabled=busy;
    q('[data-knowledge]').disabled=busy||!inspected||!!createdId;
    q('[data-cancel]').disabled=busy;
    q('[type=submit]').textContent=createdId?'重试保存名称':recoveryPreview?'恢复并添加项目':inspected?.recovery&&!inspected?.initialized?(q('[data-recovery-mode]')?.value==='clone'?'克隆并预览':'预览恢复'):'添加项目';
    q('[type=submit]').disabled=busy||(!createdId&&(!inspected||inspectedPath!==f('root').value.trim()));
    f('path').readOnly=!!inspected?.initialized;
    q('[data-pick-knowledge]').disabled=busy||!!inspected?.initialized;
  }
  let directoryVersion=0;
  async function inspectDirectory() {
    const version=++directoryVersion;
    q('[data-directory-info]').textContent='';
    try { const text=await directoryInfo(inspectedPath,f('path').value.trim());if(alive&&version===directoryVersion)q('[data-directory-info]').textContent=text; }
    catch(e){if(alive&&version===directoryVersion)q('[data-directory-info]').textContent=e.message;}
  }
  f('path').oninput=()=>{recoveryPreview=null;q('[data-preview]').textContent='';controls();directoryVersion++;q('[data-directory-info]').textContent='';};
  f('path').onchange=inspectDirectory;
  f('root').oninput=()=>{generation++;inspected=null;inspectedPath='';f('path').value='';q('[data-project-state]').textContent='选择目录后显示知识库的默认位置。';controls();};
  async function inspect() {
    const path=f('root').value.trim(), version=++generation;
    inspected=null;inspectedPath='';controls();message('');
    if(!path)return;
    q('[data-project-state]').textContent='正在读取项目…';
    try {
      await api.approveDir(path);
      const [result,state]=await Promise.all([api.knowledge({action:'status',root:path}),api.state()]);
      if(!alive||version!==generation)return;
      inspected=result;inspectedPath=path;
      const registered=state.repos.find(r=>r.repo_id===result.project_id);
      const name=registered?.project?.name || result.project_root.split('/').filter(Boolean).pop();
      if(!f('category').value)f('category').value=registered?.project?.category||'';
      if(!f('name').value||f('name').value===autoName)f('name').value=name;
      autoName=name;
      f('path').value=result.location?.path||(result.recovery ? result.recovery.suggested_path||'' : result.default_path);
      recoveryPreview=null;q('[data-preview]').textContent='';
      q('[data-recovery-fields]').innerHTML=result.initialized?'':recoveryFields(result.recovery);
      const mode=q('[data-recovery-mode]');if(mode)mode.onchange=()=>{recoveryPreview=null;q('[data-preview]').textContent='';q('[data-pick-knowledge]').textContent=mode.value==='clone'?'选择父文件夹…':'选择文件夹…';controls();};
      q('[data-project-state]').textContent=result.initialized?'该目录所属项目已有知识库。':`项目：${result.project_root}`;
      q('[data-knowledge-note]').textContent=result.initialized?'沿用现有配置；修改位置可在项目设置中迁移。':'此项目的 Worktree 和子目录共用。';
      controls();await inspectDirectory();
    } catch(e) {if(alive&&version===generation){q('[data-project-state]').textContent='';message(e.message);controls();}}
  }
  f('root').onchange=inspect;
  async function pick(target) {
    if(busy)return;busy=true;controls();
    try {const result=await api.pickDirectory();if(!alive||!result.path)return;f(target).value=target==='path'&&q('[data-recovery-mode]')?.value==='clone'?cloneDestination(inspected.recovery,result.path):result.path;recoveryPreview=null;if(target==='root')await inspect();else await inspectDirectory();}
    catch(e){message(e.message);}finally{busy=false;controls();}
  }
  q('[data-pick-root]').onclick=()=>pick('root');
  q('[data-pick-knowledge]').onclick=()=>pick('path');
  q('[data-cancel]').onclick=()=>modal.close();
  form.onsubmit=async event=>{
    event.preventDefault();
    if(busy||(!createdId&&(!inspected||inspectedPath!==f('root').value.trim())))return;
    if(!f('name').value.trim()){message('请输入项目名称。');return;}
    busy=true;controls();message('');
    try {
      if(!createdId) {
        let path=f('path').value.trim();
        // A new knowledge directory may not exist yet; approve its existing ancestor.
        let parent=path;
        for(;;){try{await api.approveDir(parent);break;}catch(e){const next=parent.replace(/\/+$/,'').replace(/\/[^/]+$/,'')||'/';if(next===parent)throw e;parent=next;}}
        if(q('[data-recovery-mode]')?.value==='clone') {
          const cloned=await api.knowledge({action:'clone',root:inspectedPath,path});
          path=cloned.path;f('path').value=path;q('[data-recovery-mode]').value='existing';recoveryPreview=null;
        }
        const recovering=!!inspected.recovery&&!inspected.initialized;
        const result=await api.knowledge({action:recovering?'recover':'init',root:inspectedPath,path,execute:!!recoveryPreview,expected:recoveryPreview?.expected});
        if(result.preview){recoveryPreview=result;q('[data-preview]').textContent=`将关联知识库并恢复项目配置（${result.files} 个文件）。${result.state?.pending_worktrees?.length ? '尚未出现的 Worktree 配置将保留。' : ''}`;return;}
        createdId=result.project_id;
      }
      await api.projectMetadata({repo_id:createdId,name:f('name').value.trim(),category:f('category').value.trim()});
      if(alive){modal.close();onCreated?.(createdId);}
    } catch(e) {
      message(createdId?`项目与知识库已添加，名称保存失败：${e.message}`:e.message);
      if(createdId)q('[type=submit]').textContent='重试保存名称';
    } finally {busy=false;controls();}
  };
  return {destroy(){alive=false;generation++;modal.destroy();}};
}
