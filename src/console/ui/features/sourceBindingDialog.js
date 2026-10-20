import {api, esc} from '../services/api.js';
import {Dialog} from '../components/dialog.js';

export function SourceBindingDialog(container, {onChanged} = {}) {
  const body=document.createElement('div');
  body.className='source-binding';
  body.innerHTML=`<div data-binding-current class="source-binding-current"></div>
    <form data-binding-form><div data-binding-local hidden class="source-binding-detect"><span data-binding-directory class="muted path"></span><button data-binding-detect type="button">检测 Git</button></div>
    <label data-binding-remotes-label hidden>选择 remote<select data-binding-remotes><option value="">请选择</option></select></label>
    <label>Git 连接地址<input data-binding-url required placeholder="https://github.com/owner/skills.git"></label>
    <div class="source-binding-fields"><label>更新方式<select data-binding-mode><option value="remote">跟随远程版本</option><option value="local">跟随本地开发</option></select></label><label>分支 / 标签<input data-binding-ref placeholder="默认分支"></label></div>
    <label data-binding-path-label>Skill 仓内路径<input data-binding-path placeholder="skills/my-skill，根目录留空"></label>
    <details data-binding-advanced><summary>SSH 别名 / 仓库地址</summary><p class="muted">连接地址选用本机身份；仓库地址用于分组与同步。</p><div class="source-binding-fields"><label>SSH Host 别名<input data-binding-alias placeholder="quincy-github"></label><label>实际 HostName<input data-binding-host placeholder="github.com"></label><label>User<input data-binding-user placeholder="git"></label><label>Port<input data-binding-port type="number" min="1" max="65535" placeholder="22"></label></div><label>可移植仓库地址<input data-binding-repository placeholder="由连接地址或 HostName 映射确定"></label><p class="muted">填写明确映射即可。复杂 Include / Match 配置请提供脱敏后的有效值。</p></details>
    <div class="actions"><button data-binding-preview type="submit">预览变更</button></div></form>
    <p data-binding-message class="inline-status muted" role="status" aria-live="polite"></p>
    <div data-binding-impact></div><footer class="dialog-actions"><button data-binding-cancel>取消</button><button data-binding-save class="primary" disabled>保存来源</button></footer>`;
  const q=s=>body.querySelector(s);
  let alive=true, loading=false, saving=false, generation=0, target=null, preview=null, localDir=null;
  const modal=Dialog(container,{title:'变更来源',content:body,open:false,keepMounted:true,canClose:()=>!saving,onClose:()=>{++generation;loading=false;preview=null;sync();}});
  function message(text,error=false){q('[data-binding-message]').textContent=text;q('[data-binding-message]').classList.toggle('field-error',error);q('[data-binding-message]').setAttribute('role',error?'alert':'status');}
  function sync(){
    modal.update({canClose:()=>!saving});
    body.querySelectorAll('form input,form select,form button').forEach(e=>{e.disabled=loading||saving;});
    q('[data-binding-path]').readOnly=q('[data-binding-mode]').value==='local';
    q('[data-binding-cancel]').disabled=saving;
    const relationship=q('[data-binding-relationship]');
    if(relationship)relationship.disabled=saving;
    if(q('[data-binding-back]'))q('[data-binding-back]').disabled=saving;
    q('[data-binding-save]').disabled=loading||saving||!preview||preview.relationship==='unverified'&&!relationship?.value;
    q('[data-binding-save]').textContent=saving?'正在保存…':'保存来源';
  }
  function invalidate(){++generation;preview=null;q('[data-binding-form]').hidden=false;q('[data-binding-impact]').replaceChildren();message('');sync();}
  q('[data-binding-form]').oninput=event=>{
    if(event.target===q('[data-binding-url]'))q('[data-binding-repository]').value='';
    invalidate();
  };
  q('[data-binding-mode]').onchange=()=>{invalidate();sync();};
  q('[data-binding-remotes]').onchange=()=>{q('[data-binding-url]').value=q('[data-binding-remotes]').value;q('[data-binding-repository]').value='';invalidate();};
  q('[data-binding-detect]').onclick=async()=>{
    if(loading||saving)return;
    invalidate();const turn=++generation;loading=true;sync();message('正在检测本地 Git…');
    try {
      if(localDir)await api.approveDir(localDir);
      const result=await api.sourceBinding({action:'discover',target});
      if(!alive||turn!==generation)return;
      const remotes=result.remotes||[];
      q('[data-binding-remotes-label]').hidden=!remotes.length;
      q('[data-binding-remotes]').innerHTML='<option value="">请选择 remote</option>'+remotes.map(r=>`<option value="${esc(r.url)}">${esc(r.name)} · ${esc(r.url)}</option>`).join('');
      const existing=q('[data-binding-url]').value;
      const matched=remotes.find(r=>r.url===existing);
      q('[data-binding-url]').value=matched?existing:remotes.length===1?remotes[0].url:'';
      q('[data-binding-remotes]').value=q('[data-binding-url]').value;
      q('[data-binding-path]').value=result.repo_path||'';q('[data-binding-ref]').value=result.ref||'';
      message(remotes.length>1?'检测到多个 remote，请选择要关联的仓库。':remotes.length?'已找到 Git 来源。关联后继续保留本地开发内容。':'此仓库还没有 remote。请先在 Git 中配置后重新检测。');
    }catch(e){if(alive&&turn===generation)message(e.message,true);}
    finally{if(alive&&turn===generation){loading=false;sync();}}
  };
  q('[data-binding-form]').onsubmit=async event=>{
    event.preventDefault();if(loading||saving)return;
    const url=q('[data-binding-url]').value.trim();if(!url){message('请填写连接地址或检测后选择 remote。',true);return;}
    invalidate();const turn=++generation;loading=true;sync();message('正在验证来源…');
    try{
      const repository=q('[data-binding-repository]').value.trim();
      for(const value of [url,repository])if(value.startsWith('/')||value.startsWith('file://'))await api.approveDir(value.replace(/^file:\/\//,''));
      if(localDir)await api.approveDir(localDir);
      const host=q('[data-binding-host]').value.trim();
      const mapping=host?{alias:q('[data-binding-alias]').value.trim(),hostname:host,user:q('[data-binding-user]').value.trim()||null,port:q('[data-binding-port]').value?Number(q('[data-binding-port]').value):null}:undefined;
      const result=await api.sourceBinding({action:'preview',target,change:{url,repository_url:repository||undefined,ref:q('[data-binding-ref]').value.trim()||undefined,repo_path:target.kind==='skill'?q('[data-binding-path]').value.trim():undefined,mode:q('[data-binding-mode]').value,host_mapping:mapping}});
      if(!alive||turn!==generation)return;
      preview=result;q('[data-binding-form]').hidden=true;
      const after=result.after.connection, before=result.before.connection?.url||result.before.repo_url||result.before.url||result.before.discovery_entry;
      q('[data-binding-impact]').innerHTML=`<section class="source-binding-preview"><div class="source-binding-review-head"><h3>确认来源</h3><button type="button" data-binding-back>修改配置</button></div><dl><dt>原连接</dt><dd class="path">${esc(before)}</dd><dt>新连接</dt><dd class="path">${esc(after.url)}</dd><dt>仓库地址</dt><dd class="path">${esc(after.repository_url)}</dd><dt>更新方式</dt><dd>${result.mode==='local'?'跟随本地开发':'跟随远程版本'} · ${esc(after.ref||'默认分支')}</dd>${target.kind==='skill'?`<dt>仓内路径</dt><dd class="path">${esc(result.after.repo_path||'仓库根目录')}</dd>`:''}<dt>使用位置</dt><dd>${result.references?.length||0} 处，引用保留</dd></dl>${result.relationship==='unverified'?'<label>仓库关系<select data-binding-relationship><option value="">请选择确认方式</option><option value="same">我确认是同一仓库换地址</option><option value="replace">我要替换为另一仓库</option></select></label><p class="muted">当前无法核实仓库身份，请核对后选择。</p>':''}<p class="muted">${esc(result.note)}</p></section>`;
      q('[data-binding-back]').onclick=()=>{if(!saving)invalidate();};
      q('[data-binding-relationship]')?.addEventListener('change',sync);
      message(result.relationship==='unverified'?'连接验证完成，还需确认仓库关系。':'来源已验证，可以保存。');
    }catch(e){if(alive&&turn===generation)message(e.message,true);}
    finally{if(alive&&turn===generation){loading=false;sync();}}
  };
  q('[data-binding-save]').onclick=async()=>{
    if(!preview||loading||saving||q('[data-binding-save]').disabled)return;
    saving=true;sync();message('正在保存来源…');
    try{
      await api.sourceBinding({action:'apply',preview_id:preview.preview_id,relationship:q('[data-binding-relationship]')?.value||undefined});
      preview=null;q('[data-binding-impact]').replaceChildren();q('[data-binding-cancel]').textContent='关闭';q('[data-binding-save]').hidden=true;
      try{await onChanged?.();message('来源已保存。当前内容和项目引用已保留，请重新检查更新。');}
      catch(e){message(`来源已保存；列表刷新失败：${e.message}。请重新打开资源库。`,true);}
    }catch(e){message(e.message,true);}
    finally{saving=false;sync();}
  };
  q('[data-binding-cancel]').onclick=()=>modal.close();
  return {
    show(nextTarget,source){
      ++generation;target=nextTarget;loading=false;saving=false;preview=null;localDir=source.source_kind==='local'?source.discovery_entry?.replace(/^local-dir:/,''):null;
      q('[data-binding-cancel]').textContent='取消';q('[data-binding-save]').hidden=false;q('[data-binding-form]').hidden=false;q('[data-binding-form]').reset();q('[data-binding-impact]').replaceChildren();q('[data-binding-remotes-label]').hidden=true;
      q('[data-binding-local]').hidden=!localDir;q('[data-binding-directory]').textContent=localDir||'';
      q('[data-binding-current]').textContent=localDir?'本地开发 → 关联 Git':source.name||nextTarget.id.split('/').pop();
      q('[data-binding-url]').value=source.connection?.url||source.repo_url||source.url||'';
      q('[data-binding-ref]').value=source.connection?(source.connection.ref||''):(source.ref||source.ref_||source.lock?.ref_||source.lock?.ref||'');
      q('[data-binding-path]').value=source.repo_path||'';q('[data-binding-path-label]').hidden=target.kind!=='skill';
      q('[data-binding-mode] option[value="local"]').hidden=!localDir;q('[data-binding-mode]').value=localDir?'local':'remote';
      q('[data-binding-repository]').value=source.connection?.repository_url||'';
      const m=source.connection?.host_mapping;
      for(const [field,key] of [['alias','alias'],['host','hostname'],['user','user'],['port','port']])q(`[data-binding-${field}]`).value=m?.[key]||'';
      q('[data-binding-advanced]').open=!!m;message(localDir?'点击“检测 Git”，选择已有 remote。':'修改连接后预览，保存时保留当前资源。');sync();modal.update({title:localDir?'关联 Git / 变更来源':'变更来源'});modal.show();
    },
    destroy(){alive=false;++generation;modal.destroy();}
  };
}
