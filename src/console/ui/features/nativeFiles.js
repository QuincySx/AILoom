import {api,esc} from '../services/api.js';
import {Dialog,confirmAction} from '../components/dialog.js';
import {PageHeader} from '../components/layout.js';
import {Item,resourcePath} from '../components/item.js';
import {ManagedDefinition} from './managedDefinition.js';
import {openSkillDelete,openSkillTakeover} from './skillActions.js';

export function NativeFiles(container,{rootPath=null,scope='global',showSkills=false,managedPaths=[],readOnly=false,hideHeading=false,includeManaged=false}={}) {
  const root=document.createElement('section');root.className='native-files';container.append(root);
  let alive=true,version=0,data={files:[],targets:[]},skills=[],filter='',search='',busy=false,editor=null;
  const q=s=>root.querySelector(s), request=body=>api.nativeFiles({scope,root:rootPath,...body});
  // Global files keep their actions in the page title row; inside a project they join the search row.
  const actions='<button data-refresh data-variant="ghost">刷新</button><button data-new>新建文件</button>';
  const header=scope==='global'?PageHeader({title:'规则与子代理',description:'直接维护用户级文件，影响全部项目。',actions}).replace('<h1>','<h1 data-native-heading>'):`<div class="workspace-section-heading"><h2 data-native-heading>${showSkills?'本地已有':'配置文件'}</h2></div>`;
  root.innerHTML=`${header}
    <div class="toolbar native-files-toolbar"><input data-search type="search" aria-label="搜索本地文件" placeholder="搜索名称、工具或路径"><select data-kind aria-label="文件类型"><option value="">全部类型</option>${showSkills?'<option value="skill">技能</option>':''}<option value="rule">规则</option><option value="agent">子代理</option></select>${scope==='global'?'':actions}</div>
    <p data-status role="status"></p><div data-list></div><details data-details hidden><summary>扫描范围与限制</summary><div data-notes></div></details>`;
  q('[data-new]').hidden=readOnly;
  if(hideHeading)q('[data-native-heading]').hidden=true;
  function message(s){if(alive)q('[data-status]').textContent=s;}
  function render(){
    const rows=[...data.files,...skills].map(f=>({...f,managed:f.managed||managedPaths.includes(f.path)})).filter(f=>(!filter||filter===f.kind)&&
      (includeManaged||(!showSkills||!f.managed)&&!managedPaths.includes(f.path))&&`${f.label} ${f.tool||''} ${f.path}`.toLowerCase().includes(search.toLowerCase()));
    q('[data-native-heading]').textContent=scope==='global'?'规则与子代理':`${showSkills?'本地已有':'配置文件'} ${rows.length} 项`;
    if(includeManaged)rows.sort((a,b)=>(a.managed?0:a.kind==='skill'?2:1)-(b.managed?0:b.kind==='skill'?2:1));
    q('[data-list]').innerHTML=rows.length?rows.map(f=>{
      const i=rows.indexOf(f),path=rootPath&&f.path.startsWith(rootPath+'/')?f.path.slice(rootPath.length+1):f.path.split('/').pop();
      const kind=f.kind==='rule'?'规则':f.kind==='skill'?'技能':'子代理';
      const host=f.tool||(/(?:^|\/)\.claude(?:\/|$)/.test(f.path)?'Claude Code':/(?:^|\/)\.(?:agents|codex)(?:\/|$)/.test(f.path)?'Codex':'');
      const meta=host?`${host} ${kind}`:kind;
      const state=f.managed?'由能力配置生成':f.local_only?'仅本机生效':f.note||'';
      const external=f.kind==='skill'?resourcePath({name:f.label,readonly:true,path:f.link_target||f.location?.real_path||path}):'';
      const group=f.managed?'托管文件':f.kind==='skill'?'工具中已有':'其他本地文件';
      const previous=rows[rows.indexOf(f)-1];
      const previousGroup=previous&&(previous.managed?'托管文件':previous.kind==='skill'?'工具中已有':'其他本地文件');
      return (includeManaged&&group!==previousGroup?`<h2 class="local-file-group-heading">${group}</h2>`:'')+Item({title:f.kind==='skill'?f.label:path||f.label,primaryAction:{attributes:`data-view="${i}"`,label:'查看 '+(path||f.label)},description:meta,path:external,meta:state,density:'regular',className:'native-file-row',actions:`${f.kind==='skill'&&!readOnly&&!f.managed?`<button data-import-skill="${i}">导入副本</button>`:''}${f.kind!=='skill'&&!readOnly&&!f.managed?`<button data-import-copy="${i}">导入副本</button>`:''}`,feedback:f.error?`<span class="field-error">${esc(f.error)}</span>`:''});
    }).join(''):'<p class="muted">没有找到匹配的本地文件。</p>';
    q('[data-list]').querySelectorAll('[data-view]').forEach(b=>{b.onclick=()=>view(rows[Number(b.dataset.view)]);});
    q('[data-list]').querySelectorAll('[data-import-copy]').forEach(b=>{b.onclick=async()=>{
      if(busy)return;busy=true;b.disabled=true;
      try{const file=rows[Number(b.dataset.importCopy)],current=await request({action:'read',target:file.target,name:file.name});if(!alive)return;
        const result=await ManagedDefinition(root,{kind:file.kind,title:'导入个人副本',initial:{name:(file.name||file.path.split('/').pop()).replace(/\.[^.]+$/,'').toLowerCase().replace(/[^a-z0-9._-]+/g,'-').slice(0,64)||'imported-rule',description:file.description||'',body:current.content||''}});
        if(result)message('已保存个人副本，原文件保留。到能力配置中选用。');
      }catch(e){message(e.message);}finally{busy=false;if(alive)b.disabled=false;}
    };});
    q('[data-list]').querySelectorAll('[data-import-skill]').forEach(b=>{b.onclick=async()=>{
      if(busy)return;
      const file=rows[Number(b.dataset.importSkill)];
      if(!await confirmAction(`将 ${file.label} 保存到资源库？\n来源：${file.path}\n原文件保留，导入后可在其他项目选用。`,{title:'导入本地副本',confirmLabel:'导入副本'}))return;
      busy=true;b.disabled=true;
      try {await api.approveDir(file.path);await api.libraryImport(file.path,null,true);message('已保存资源库副本，原文件保留。');}
      catch(e){message(e.message);}finally{busy=false;if(alive)b.disabled=false;}
    };});
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
      render();message('');q('[data-new]').disabled=readOnly;
    }catch(e){message(e.message);}
  }
  // Viewing a file is where its low-frequency actions live: edit, delete, take over.
  async function view(file){
    if(busy||editor?.isOpen)return;
    const editable=!readOnly&&!file.managed,then=fn=>()=>{editor.close();fn();return false;};
    if(file.kind==='skill'){
      const body=document.createElement('div');body.innerHTML=`<p class="path">${esc(file.path)}</p>${file.note?`<p class="muted">${esc(file.note)}</p>`:''}`;
      const actions=[{label:'关闭'}];
      if(editable&&['unmanaged','external_link'].includes(file.management)){
        actions.unshift({label:'删除',variant:'destructive',onAction:then(()=>openSkillDelete({root:rootPath,sub:undefined,path:file.path,name:file.dir_name||file.label,onDone:r=>{if(r.error)message(r.error);else load().then(()=>message('已删除并移入归档：'+r.archived_to));}}))});
        actions.unshift({label:'接管说明',onAction:then(()=>openSkillTakeover({path:file.path,name:file.label}))});
      }
      editor=Dialog(root,{title:file.label,content:body,actions});return;
    }
    busy=true;
    try{
      const current=await request({action:'read',target:file.target,name:file.name});
      if(!alive)return;
      const body=document.createElement('div');body.innerHTML=`<p class="path">${esc(file.path)}</p><label>内容<textarea rows="18" readonly></textarea></label>`;
      body.querySelector('textarea').value=current.content||'';
      const actions=[{label:'关闭'}];
      if(editable)actions.unshift({label:file.local_only?'恢复项目版本':'删除原文件',variant:'destructive',onAction:then(()=>remove(file))},{label:'编辑原文件',onAction:then(()=>edit(file))});
      editor=Dialog(root,{title:file.label,content:body,actions});
    }catch(e){message(e.message);}finally{busy=false;}
  }
  function template(t){
    if(t.ext==='toml')return 'name = "my-agent"\ndescription = "描述这个子代理的用途"\ndeveloper_instructions = "在这里填写子代理的职责和要求。"\n';
    if(t.kind==='agent')return '---\nname: my-agent\ndescription: 描述这个子代理的用途\n'+(t.tool==='OpenCode'?'mode: subagent\n':'')+'---\n\n在这里填写子代理的职责和要求。\n';
    if(t.ext==='mdc')return '---\ndescription: 描述规则用途\nglobs: ""\nalwaysApply: true\n---\n\n';
    return '';
  }
  async function edit(file=null){
    if(readOnly||busy||editor?.isOpen)return;
    busy=true;
    try{
      const current=file?await request({action:'read',target:file.target,name:file.name}):null;
      if(!alive)return;
      const body=document.createElement('form');
      body.innerHTML=`${file?'':`<label>工具与类型<select name="target">${data.targets.map(t=>`<option value="${esc(t.id)}">${esc(t.tool)} · ${t.kind==='rule'?'规则':'子代理'}${t.fixed?' · '+esc(t.id==='claude-dot-instructions'?'.claude/CLAUDE.md':t.path.split('/').pop()):''}</option>`).join('')}</select></label><label data-name>文件名<input name="name" required placeholder="my-rule.md"></label>`}
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
    if(readOnly||busy)return;busy=true;
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
