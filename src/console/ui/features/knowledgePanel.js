import { recoveryFields, cloneDestination } from './knowledgeRecovery.js';
import { api, esc } from '../services/api.js';

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
  const q=s=>root.querySelector(s);
  const request=body=>api.knowledge({root:rootPath,...body});
  function message(text){if(alive)q('[data-message]').textContent=text;}
  function disable(value){if(alive)root.querySelectorAll('input,select,button').forEach(el=>el.disabled=value);}
  async function load(){state=await request({action:'status'});if(alive)render();}
  function render(){
    const loc=state.location, recovery=!loc&&state.recovery;
    root.innerHTML=`<p><strong>${state.initialized?'项目知识库':'设置项目知识库'}</strong>${state.initialized?` · ${esc(state.files)} 个文件`:''}</p>
      <p class="muted">此项目的所有工作目录共用这个保存位置。</p>
      <form data-form>
        ${recoveryFields(recovery)}
        <label>保存位置<div class="input-action"><input name="path" required value="${esc(loc?.path || (recovery ? recovery.suggested_path||'' : state.default_path))}"><button type="button" data-pick>选择文件夹…</button></div></label>
        <p class="muted" data-directory-info></p>
        <div data-preview></div><p data-message role="status"></p>
        <div class="actions"><button type="submit" class="primary" data-submit>${loc?'预览迁移':recovery?'预览恢复':'保存位置'}</button>
${loc?'<button type="button" data-checkpoint>保存恢复配置</button>':''}
          </div>
      </form>`;
    async function inspect() {
      const version=++generation;
      try{const text=await directoryInfo(rootPath,q('[name=path]').value.trim());if(alive&&version===generation)q('[data-directory-info]').textContent=text;}
      catch(e){if(alive&&version===generation)q('[data-directory-info]').textContent=e.message;}
    }
    q('[name=path]').onchange=inspect;
    const mode=q('[data-recovery-mode]');if(mode)mode.onchange=()=>{q('[data-pick]').textContent=mode.value==='clone'?'选择父文件夹…':'选择文件夹…';q('[data-form]').dispatchEvent(new Event('input'));};
    if(loc)q('[data-checkpoint]').onclick=async()=>{if(busy)return;busy=true;disable(true);try{await request({action:'checkpoint'});message('恢复配置已保存。将项目配置和知识库分别同步到新设备即可。');}catch(e){message(e.message);}finally{busy=false;disable(false);}};
    inspect();
    function invalidatePreview(){preview=null;q('[data-preview]').textContent='';q('[data-submit]').textContent=loc?'预览迁移':recovery?(mode?.value==='clone'?'克隆并预览':'预览恢复'):'保存位置';}
    q('[data-form]').oninput=()=>{generation++;q('[data-directory-info]').textContent='';edited=true;invalidatePreview();};
    q('[data-pick]').onclick=async()=>{
      try{const v=await api.pickDirectory();if(!alive||!v.path)return;q('[name=path]').value=mode?.value==='clone'?cloneDestination(recovery,v.path):v.path;q('[data-form]').dispatchEvent(new Event('input'));await inspect();}catch(e){message(e.message);}
    };
    q('[data-form]').onsubmit=async event=>{
      event.preventDefault();if(busy)return;busy=true;disable(true);
      try{
        const form=q('[data-form]');const body={path:form.elements.path.value.trim()};
        if(loc&&body.path===loc.path){edited=false;message('保存位置未改变。');return;}
        // Approval is the user's explicit directory choice; approve the closest existing parent.
        let parent=body.path;for(;;){try{await api.approveDir(parent);break;}catch(e){const next=parent.replace(/\/+$/,'').replace(/\/[^/]+$/,'');if(!next||next===parent)throw e;parent=next;}}
        if(q('[data-recovery-mode]')?.value==='clone') {
          const cloned=await request({action:'clone',path:body.path});body.path=cloned.path;q('[name=path]').value=cloned.path;q('[data-recovery-mode]').value='existing';preview=null;
        }
        const result=await request({action:loc?'move':recovery?'recover':'init',...body,execute:!!preview,expected:preview?.expected});
        if(!alive)return;
        if(result.preview){preview=result;q('[data-preview]').innerHTML=`<p>${esc(result.files)} 个文件 → <span class="path">${esc(result.to)}</span></p><p class="muted">${result.affected_projects ? result.affected_projects.length+' 个项目关联将更新；原目录保留为备份。' : '恢复选用配置，工具文件通过应用改动生成。'}</p>`;q('[data-submit]').textContent=loc?'迁移并切换':'确认恢复';message('');}
        else{edited=false;preview=null;await load();message(result.moved?'迁移完成，原目录已保留。':'知识库已关联。');}
      }catch(e){if(alive)invalidatePreview();message(e.message);}finally{busy=false;disable(false);}
    };

  }
  root.innerHTML='<p data-message role="status">正在读取知识库…</p>';
  load().catch(e=>message(e.message));
  return {isDirty:()=>edited||busy,destroy(){alive=false;root.remove();}};
}
