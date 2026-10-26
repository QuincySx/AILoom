import { recoveryFields, cloneDestination } from './knowledgeRecovery.js';
import { api, esc } from '../services/api.js';
import { KnowledgeOrganization } from './knowledgeOrganization.js';

export async function directoryInfo(root, path) {
  if(!path)return '';
  let parent=path;
  for(;;){try{await api.approveDir(parent);break;}catch(e){const next=parent.replace(/\/+$/,'').replace(/\/[^/]+$/,'')||'/';if(next===parent)throw e;parent=next;}}
  const result=await api.knowledge({action:'inspect',root,path});
  return result.git ? `Git · ${result.git.root} · ${result.git.branch}${result.git.remote?' · '+result.git.remote:''}` : '普通文件夹';
}

export function KnowledgePanel(container, {rootPath}) {
  const root=document.createElement('section');root.className='knowledge-settings';container.append(root);
  let alive=true, state=null, preview=null, edited=false, busy=false, generation=0;
  let organization=null;
  const q=s=>root.querySelector(s);
  const request=body=>api.knowledge({root:rootPath,...body});
  function message(text){if(alive)q('[data-message]').textContent=text;}
  function disable(value){if(alive)q('[data-form]').querySelectorAll('input,select,button').forEach(el=>el.disabled=value);}
  async function load(){state=await request({action:'status'});if(alive)render();}
  function render(){
    organization?.destroy();organization=null;
    const loc=state.location, recovery=!loc&&state.recovery, separating=!!loc&&state.needs_separation;
    let repositoryMode=!recovery&&!separating&&!!state.repository_path&&state.storage_mode!=='directory';
    const storagePath=repositoryMode?state.repository_path:(separating?state.default_path:loc?.path)||(recovery?recovery.suggested_path||'':state.default_path);
    root.innerHTML=`${state.initialized?'<div data-organization></div>':''}<details class="knowledge-location" ${state.initialized?'':'open'}><summary>${state.initialized?'保存位置与恢复配置':'设置项目知识库'}</summary><p><strong>${state.initialized?'项目知识库':'设置项目知识库'}</strong>${state.initialized?` · ${esc(state.files)} 个文件`:''}</p>
      <p class="muted">同一个知识库仓库中，每个项目各放一个文件夹；项目内的 Worktree 共用。原文和知识正文按项目保存、整理和检索。</p>${loc?`<p>本项目目录：<span class="path">${esc(loc.path)}</span></p>`:''}${separating?'<p>旧格式的资料混放，需要先按项目提取；本次只切换当前项目，原目录保留。</p>':''}
      <form data-form>
        ${recoveryFields(recovery)}
        ${!recovery&&!separating&&state.repository_path?'<label>存放方式<select data-storage-mode><option value="repository">知识库仓库，按项目分文件夹</option><option value="directory">直接指定项目知识目录</option></select></label>':''}
        <label><span data-storage-label>${repositoryMode?'知识库仓库':'本项目知识目录'}</span><div class="input-action"><input name="path" required value="${esc(storagePath)}"><button type="button" data-pick>选择文件夹…</button></div></label>
        <p class="muted" data-storage-note>${repositoryMode?`本项目使用其中的 ${esc(state.directory_name)}/ 文件夹；可与其他项目共用同一仓库，也可另选仓库。`:''}</p>
        <p class="muted" data-directory-info></p>
        <div data-preview></div><p data-message role="status"></p>
        <div class="actions"><button type="submit" class="primary" data-submit>${loc?(separating?'预览拆分':'预览迁移'):recovery?'预览恢复':'保存位置'}</button>
${loc?'<button type="button" data-checkpoint>保存恢复配置</button>':''}
          </div>
      </form></details>`;
    if(state.initialized)organization=KnowledgeOrganization(q('[data-organization]'),{rootPath});
    if(!recovery&&!separating&&state.repository_path)q('[data-storage-mode]').value=repositoryMode?'repository':'directory';
    async function inspect() {
      const version=++generation;
      try{const text=await directoryInfo(rootPath,q('[name=path]').value.trim());if(alive&&version===generation)q('[data-directory-info]').textContent=text;}
      catch(e){if(alive&&version===generation)q('[data-directory-info]').textContent=e.message;}
    }
    q('[name=path]').onchange=inspect;
    if(!recovery&&!separating&&state.repository_path)q('[data-storage-mode]').onchange=()=>{
      repositoryMode=q('[data-storage-mode]').value==='repository';
      q('[name=path]').value=repositoryMode?state.repository_path:loc?.path||state.default_path;
      q('[data-storage-label]').textContent=repositoryMode?'知识库仓库':'本项目知识目录';
      q('[data-storage-note]').textContent=repositoryMode?`本项目使用其中的 ${state.directory_name}/ 文件夹；可与其他项目共用同一仓库，也可另选仓库。`:'直接使用所选目录保存本项目知识，可自行作为独立 Git 仓库维护。';
      q('[data-form]').dispatchEvent(new Event('input'));inspect();
    };
    const mode=q('[data-recovery-mode]');if(mode)mode.onchange=()=>{q('[data-pick]').textContent=mode.value==='clone'?'选择父文件夹…':'选择文件夹…';q('[data-form]').dispatchEvent(new Event('input'));};
    if(loc)q('[data-checkpoint]').onclick=async()=>{if(busy)return;busy=true;disable(true);try{await request({action:'checkpoint'});message('恢复配置已保存。将项目配置和知识库分别同步到新设备即可。');}catch(e){message(e.message);}finally{busy=false;disable(false);}};
    inspect();
    function invalidatePreview(){preview=null;q('[data-preview]').textContent='';q('[data-submit]').textContent=loc?(separating?'预览拆分':'预览迁移'):recovery?(mode?.value==='clone'?'克隆并预览':'预览恢复'):'保存位置';}
    q('[data-form]').oninput=()=>{generation++;q('[data-directory-info]').textContent='';edited=true;invalidatePreview();};
    q('[data-pick]').onclick=async()=>{
      try{const v=await api.pickDirectory();if(!alive||!v.path)return;q('[name=path]').value=mode?.value==='clone'?cloneDestination(recovery,v.path):v.path;q('[data-form]').dispatchEvent(new Event('input'));await inspect();}catch(e){message(e.message);}
    };
    q('[data-form]').onsubmit=async event=>{
      event.preventDefault();if(busy)return;busy=true;disable(true);
      try{
        const form=q('[data-form]'),selected=form.elements.path.value.trim();const body=repositoryMode?{repository_path:selected}:{path:selected};
        if(loc&&!separating&&(repositoryMode?selected.replace(/\/+$/,'')+'/'+state.directory_name:body.path)===loc.path){edited=false;message('保存位置未改变。');return;}
        // Approval is the user's explicit directory choice; approve the closest existing parent.
        let parent=selected;for(;;){try{await api.approveDir(parent);break;}catch(e){const next=parent.replace(/\/+$/,'').replace(/\/[^/]+$/,'');if(!next||next===parent)throw e;parent=next;}}
        if(q('[data-recovery-mode]')?.value==='clone') {
          const cloned=await request({action:'clone',path:body.path});body.path=cloned.path;q('[name=path]').value=cloned.path;q('[data-recovery-mode]').value='existing';preview=null;
        }
        const result=await request({action:loc?(separating?'separate':'move'):recovery?'recover':'init',...body,execute:!!preview,expected:preview?.expected});
        if(!alive)return;
        if(result.preview){preview=result;q('[data-preview]').innerHTML=`<p>${esc(result.files)} 个文件 → <span class="path">${esc(result.to)}</span></p><p class="muted">${result.affected_projects ? result.affected_projects.length+' 个项目关联将更新；原目录保留为备份。' : '恢复选用配置，工具文件通过应用改动生成。'}</p>${result.unassigned_sources?.length?`<p>${esc(result.unassigned_sources.length)} 篇旧文档尚无明确归属，保留在原目录，未自动分配。</p>`:''}`;q('[data-submit]').textContent=loc?(separating?'拆分并切换':'迁移并切换'):'确认恢复';message('');}
        else{edited=false;preview=null;await load();message((result.separated?('已提取本项目的知识目录，原目录已保留。'+(result.recovery_required?'恢复配置尚未应用到本机，请对新目录执行恢复后再保存。':'')):result.moved?'迁移完成，原目录已保留。':'知识库已关联。')+(result.navigation_warning?' 首页导航未更新：'+result.navigation_warning:''));}
      }catch(e){if(alive)invalidatePreview();message(e.message);}finally{busy=false;disable(false);}
    };

  }
  root.innerHTML='<p data-message role="status">正在读取知识库…</p>';
  load().catch(e=>message(e.message));
  return {isDirty:()=>edited||busy,destroy(){alive=false;organization?.destroy();root.remove();}};
}
