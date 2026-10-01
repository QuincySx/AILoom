import {api,esc} from '../services/api.js';
import {Dialog,confirmAction} from '../components/dialog.js';
import {openSkillDelete,openSkillTakeover} from './skillActions.js';

export function NativeFiles(container,{rootPath=null,scope='global',showSkills=false,managedPaths=[]}={}) {
  const root=document.createElement('section');root.className='native-files';container.append(root);
  let alive=true,version=0,data={files:[],targets:[]},skills=[],filter='',search='',busy=false,editor=null;
  const q=s=>root.querySelector(s), request=body=>api.nativeFiles({scope,root:rootPath,...body});
  root.innerHTML=`<div class="workspace-section-heading"><h2>${showSkills?'本地已有':'Rules 与 Agent'}</h2><div class="actions"><button data-refresh>刷新</button><button data-new>新建</button></div></div>
    <div class="toolbar"><input data-search type="search" aria-label="搜索本地文件" placeholder="搜索名称、工具或路径…"><select data-kind aria-label="文件类型"><option value="">全部类型</option>${showSkills?'<option value="skill">Skill</option>':''}<option value="rule">Rules</option><option value="agent">Agent</option></select></div>
    <p data-status role="status"></p><div data-list></div><details data-details hidden><summary>扫描范围与限制</summary><div data-notes></div></details>`;
  function message(s){if(alive)q('[data-status]').textContent=s;}
  function render(){
    const rows=[...data.files,...skills].filter(f=>(!filter||filter===f.kind)&&
      (!showSkills||!f.managed)&&!managedPaths.includes(f.path)&&`${f.label} ${f.tool||''} ${f.path}`.toLowerCase().includes(search.toLowerCase()));
    if(!showSkills)q('h2').textContent=rows.length+' 个文件';
    q('[data-list]').innerHTML=rows.length?rows.map((f,i)=>`<article class="native-file-row"><span class="badge">${f.kind==='rule'?'Rules':f.kind==='skill'?'Skill':'Agent'}</span><div><strong>${esc(f.label)}</strong><small>${esc(f.tool||'')} ${f.local_only?' · 仅本机生效':f.managed?' · AILoom 管理':f.created_by_ailoom?' · AILoom 创建':''} ${esc(f.note||'')}${f.missing?' · 文件缺失，个人副本仍保留':''}</small><p class="path"><ailoom-path title="${esc(f.path)}">${esc(f.path)}</ailoom-path></p>${f.description?'<p>'+esc(f.description)+'</p>':''}${f.error?'<p class="field-error">'+esc(f.error)+'</p>':''}</div>${f.kind==='skill'?'<span class="muted">'+(f.management==='managed'?'AILoom 管理':f.management==='external_link'?'链接':'本地')+'</span>'+(['unmanaged','external_link'].includes(f.management)?`<button data-takeover="${i}">接管说明…</button><button data-skill-delete="${i}" data-variant="destructive">删除…</button>`:''):`<button data-edit="${i}">编辑</button><button data-delete="${i}">${f.local_only?'恢复项目版本':'删除'}</button>`}</article>`).join(''):'<p class="muted">没有找到匹配的本地文件。</p>';
    q('[data-list]').querySelectorAll('[data-edit]').forEach(b=>b.onclick=()=>edit(rows[Number(b.dataset.edit)]));
    q('[data-list]').querySelectorAll('[data-delete]').forEach(b=>b.onclick=()=>remove(rows[Number(b.dataset.delete)]));
    q('[data-list]').querySelectorAll('[data-takeover]').forEach(b=>b.onclick=()=>{const f=rows[Number(b.dataset.takeover)];openSkillTakeover({path:f.path,name:f.label});});
    q('[data-list]').querySelectorAll('[data-skill-delete]').forEach(b=>b.onclick=async()=>{
      const f=rows[Number(b.dataset.skillDelete)];b.disabled=true;
      await openSkillDelete({root:rootPath,sub:undefined,path:f.path,name:f.dir_name||f.label,onDone:r=>{
        if(r.error)message(r.error);else{load().then(()=>message('已删除并移入归档：'+r.archived_to));}
      }});
      if(alive)b.disabled=false;
    });
  }
  async function load(){
    const n=++version;message('正在扫描…');q('[data-new]').disabled=true;
    try{
      if(rootPath)await api.approveDir(rootPath);
      const [files,found]=await Promise.all([request({action:'list'}),showSkills?api.scanProjectSkills(rootPath):Promise.resolve({items:[]})]);
      if(!alive||n!==version)return;
      data=files;skills=found.items.filter(f=>f.management!=='managed').map(f=>({...f,kind:'skill',label:f.name||f.dir_name}));
      const notes=[...(data.warnings||[]),...(data.limitations||[])];
      q('[data-details]').hidden=!notes.length;
      q('[data-notes]').innerHTML=notes.map(s=>'<p>'+esc(s)+'</p>').join('');
      render();message('');q('[data-new]').disabled=false;
      if(showSkills)q('h2').textContent='本地已有 · '+skills.length+' Skill · '+data.files.filter(f=>f.kind==='rule'&&!f.managed).length+' Rules · '+data.files.filter(f=>f.kind==='agent'&&!f.managed).length+' Agent';
    }catch(e){message(e.message);}
  }
  function template(t){
    if(t.ext==='toml')return 'name = "my-agent"\ndescription = "描述这个子代理的用途"\ndeveloper_instructions = "在这里填写子代理的职责和要求。"\n';
    if(t.kind==='agent')return '---\nname: my-agent\ndescription: 描述这个子代理的用途\n'+(t.tool==='OpenCode'?'mode: subagent\n':'')+'---\n\n在这里填写子代理的职责和要求。\n';
    if(t.ext==='mdc')return '---\ndescription: 描述规则用途\nglobs: ""\nalwaysApply: true\n---\n\n';
    return '';
  }
  async function edit(file=null){
    if(busy||editor?.isOpen)return;
    busy=true;
    try{
      const current=file?await request({action:'read',target:file.target,name:file.name}):null;
      if(!alive)return;
      const body=document.createElement('form');
      body.innerHTML=`${file?'':`<label>工具与类型<select name="target">${data.targets.map(t=>`<option value="${esc(t.id)}">${esc(t.tool)} · ${t.kind==='rule'?'Rules':'Agent'}${t.fixed?' · '+esc(t.id==='claude-dot-instructions'?'.claude/CLAUDE.md':t.path.split('/').pop()):''}</option>`).join('')}</select></label><label data-name>文件名<input name="name" required placeholder="my-rule.md"></label>`}
        ${data.git_project||current?.local?.active?'<label>保存方式<select name="mode"><option value="project">修改项目文件</option><option value="local">仅本机生效</option></select></label><p class="muted" data-mode-note></p>':''}
        <p class="path" data-path></p><p class="muted" data-note></p><label>内容<textarea name="content" rows="18" spellcheck="false"></textarea></label><p class="field-error" data-error role="alert"></p><div class="actions"><button type="submit" class="primary">保存文件</button>${current?.local?.active?'<button type="button" data-restore>恢复项目版本</button>':''}${current?.local?.has_personal_copy&&!current.local.active?'<button type="button" data-personal>载入个人副本</button>':''}</div>`;
      let saving=false,saved=current?.content??(current?.local?.active?current.local.personal_content:'')??'',savedMode=current?.local?.active?'local':'project';
      const el=n=>body.elements.namedItem(n);
      const lockForm=locked=>body.querySelectorAll('input,select,textarea,button').forEach(e=>{e.disabled=locked;});
      const chosen=()=>data.targets.find(t=>t.id===(file?.target||el('target').value));
      const update=()=>{
        const t=chosen();
        if(!file){body.querySelector('[data-name]').hidden=t.fixed;el('name').required=!t.fixed;el('name').placeholder='my-file.'+t.ext;if(!el('content').value||el('content').value===saved){el('content').value=template(t);saved=el('content').value;}}
        body.querySelector('[data-path]').textContent=file?.path||t.path;
        body.querySelector('[data-note]').textContent=file?.managed?'此文件由 AILoom 应用生成，再次应用可能更新它。':t.note||'';
      };
      editor=Dialog(document.body,{title:file?'编辑 '+file.label:'新建本地文件',content:body,canClose:()=>!saving,dirty:()=>el('content').value!==saved||(el('mode')?.value||'project')!==savedMode,onClose:()=>{editor=null;}});
      update();if(file)el('content').value=saved;
      if(!file)el('target').onchange=update;
      if(el('mode')){
        el('mode').value=savedMode;
        const hint=()=>{body.querySelector('[data-mode-note]').textContent=el('mode').value==='local'?(current?.local?.tracked?'个人版本在原路径生效；Git 拉取或切换分支可能需要先恢复项目版本。':'仅在本机忽略此文件，不修改项目的 .gitignore。'):(current?.local?.active?'保存后，编辑内容将成为项目文件的普通改动。':'直接保存原文件。');};
        el('mode').onchange=hint;hint();
      }
      if(current?.local?.issue)body.querySelector('[data-error]').textContent=current.local.issue;
      body.querySelector('[data-personal]')?.addEventListener('click',async()=>{
        if(el('content').value!==saved&&!await confirmAction('用已保存的个人副本替换当前编辑内容？',{title:'载入个人副本',confirmLabel:'载入'}))return;
        el('content').value=current.local.personal_content||'';if(el('mode')){el('mode').value='local';el('mode').onchange();}
      });
      body.querySelector('[data-restore]')?.addEventListener('click',async()=>{
        if(saving)return;
        if(el('content').value!==saved&&!await confirmAction('放弃未保存的编辑，恢复项目版本？已保存的个人副本会保留。',{title:'恢复项目版本',confirmLabel:'恢复'}))return;
        saving=true;lockForm(true);
        try{await request({action:'restore',target:file.target,name:file.name,expected:current.expected});saved=el('content').value;editor.close();await load();message('已恢复项目版本，个人副本已保留。');}
        catch(e){body.querySelector('[data-error]').textContent=e.message;}finally{saving=false;lockForm(false);}
      });
      body.onsubmit=async e=>{
        e.preventDefault();if(saving)return;saving=true;lockForm(true);
        try{
          const t=chosen();await request({action:'save',target:t.id,name:file?.name||(t.fixed?'':el('name').value.trim()),content:el('content').value,mode:el('mode')?.value||'project',expected:current?.expected||'missing'});
          saved=el('content').value;const local=el('mode')?.value==='local';editor.close();await load();message(local?'已保存，仅本机生效。':'已保存到原文件。');
        }catch(e){body.querySelector('[data-error]').textContent=e.message;}
        finally{saving=false;lockForm(false);}
      };
    }catch(e){message(e.message);}finally{busy=false;}
  }
  async function remove(file){
    if(busy)return;busy=true;
    try{
      const current=await request({action:'read',target:file.target,name:file.name});
      if(current.local?.active){
        if(!alive||!await confirmAction('恢复项目版本？已保存的个人副本会保留。',{title:'恢复 '+file.label,confirmLabel:'恢复'}))return;
        await request({action:'restore',target:file.target,name:file.name,expected:current.expected});await load();message('已恢复项目版本，个人副本已保留。');return;
      }
      if(!alive||!await confirmAction(`删除文件？\n${file.path}`,{title:'删除 '+file.label,confirmLabel:'删除',destructive:true}))return;
      await request({action:'delete',target:file.target,name:file.name,expected:current.expected});
      await load();message('已删除，原文件备份保存在 AILoom 数据目录。');
    }catch(e){message(e.message);}finally{busy=false;}
  }
  q('[data-new]').onclick=()=>edit();
  q('[data-refresh]').onclick=load;
  q('[data-search]').oninput=e=>{search=e.target.value;render();};
  q('[data-kind]').onchange=e=>{filter=e.target.value;render();};
  load();
  return {isDirty:()=>busy||!!editor?.isOpen,destroy(){alive=false;version++;editor?.destroy();root.remove();}};
}
