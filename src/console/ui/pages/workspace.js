import { ManagedDefinition } from '../features/managedDefinition.js';
import { NativeFiles } from '../features/nativeFiles.js';
// Directory-first workspace. The server remains the source of configuration and deployment truth.
import { api, esc } from '../services/api.js';
import { directoryTarget, sharedSettingsTarget, layerOfTarget, diffOf, setTarget, currentGeneration, shouldApply } from '../state/target.js';
import { waitJob } from '../state/jobs.js';
import { Dialog, confirmAction } from '../components/dialog.js';
import { ResourcePicker } from '../features/resourcePicker.js';
import { DirectoryPicker } from '../features/directoryPicker.js';
import { ImportDialog } from '../features/importDialog.js';
import { ProjectDialog } from '../features/projectDialog.js';

const names = {skill:'Skill',mcp:'MCP',agent:'Agent',rule:'Rules',doc:'文档',learning:'经验',env:'环境',hook:'Hook',package:'依赖包'};
const tools = {claude:'Claude Code',codex:'Codex CLI',grok:'Grok',pi:'Pi',opencode:'OpenCode',cursor:'Cursor'};
const icons = {
  folder:'<path d="M3 7h6l2-3h4l2 3h4v13H3z"/>',
  plus:'<path d="M12 5v14M5 12h14"/>',
  chevron:'<path d="m9 5 7 7-7 7"/>',
  check:'<path d="m5 12 4 4L19 6"/>',
  skill:'<path d="m12 3 3 6 6 3-6 3-3 6-3-6-6-3 6-3z"/>',
  mcp:'<path d="M8 3v5m8-5v5M6 8h12v4a6 6 0 0 1-12 0zM12 18v3"/>',
  rule:'<path d="M9 5h11M9 12h11M9 19h11M3 5h1M3 12h1M3 19h1"/>',
  agent:'<rect x="4" y="7" width="16" height="13" rx="3"/><path d="M12 3v4M8 12h1m6 0h1M9 16h6"/>',
};
const icon = name => `<svg viewBox="0 0 24 24" aria-hidden="true">${icons[name] || icons.skill}</svg>`;
const projectName = r => r.project?.name || Object.values(r.worktrees || {})[0]?.path?.split('/').pop() || r.repo_id;

export function selectionForDirectory(t, entry, state) {
  return {root:t.rootPath,...entry,state,worktree:t.viewKind!=='project-shared'&&!(t.kind==='nongit'&&!t.relativeDir),...(t.relativeDir?{subproject:t.relativeDir}:{})};
}

const folderCache = new Map();
let pendingEnvironmentProject = null;

export function mount(container, {projectId}) {
  const root = document.createElement('div');
  root.className = 'workspace-explorer';
  container.append(root);
  let disposed = false, busy = false, loadVersion = 0;
  let repo, repos = [], wtId, relative = '', nodeKind = 'project', target, effective, catalog, deployment, capabilities;
  const scopeSets = folderCache;
  const scopeKey = (id, project=repo) => `${project.repo_id}:${id}`;
  let filter = '', search = '', showOff = false;
  let notice = '', localPanel=null;
  const dialogs = [];
  const controller = new AbortController();
  const q = selector => root.querySelector(selector);
  const alive = gen => !disposed && shouldApply(gen);
  const message = text => { notice = text; if (q('[data-workspace-message]')) q('[data-workspace-message]').textContent = text; };
  let treeOrder='original', collapsed=new Set();
  try{const saved=JSON.parse(localStorage.getItem('ailoom-tree-preferences'));treeOrder=saved?.order==='name'?'name':'original';collapsed=new Set(saved?.collapsed||[]);}catch{}
  const saveTree=()=>localStorage.setItem('ailoom-tree-preferences',JSON.stringify({order:treeOrder,collapsed:[...collapsed]}));
  root.innerHTML = '<p role="status">正在打开目录…</p>';

  function remember() {
    sessionStorage.setItem('ailoom-return-after-settings', JSON.stringify({repoId:repo.repo_id, wt:wtId, dir:relative || null, node:nodeKind, tab:0}));
  }
  function settings(suffix = 'settings') { remember(); location.hash = `#/projects/${encodeURIComponent(repo.repo_id)}/${suffix}`; }
  function shell() {
    root.innerHTML = `<aside class="workspace-rail" aria-label="目录导航">
      <div class="workspace-rail-heading"><strong>我的目录</strong><button data-tree-options aria-label="目录显示选项" title="目录显示选项">···</button><button data-add-project aria-label="添加项目">${icon('plus')}</button></div>
      <div data-folders role="tree" aria-label="项目目录"></div><p data-tree-error class="field-error" role="status"></p>
      <a class="workspace-manage" href="#/projects/manage">管理项目</a></aside>
      <section class="workspace-body" aria-label="当前目录的能力">
        <div data-workspace-head></div><p data-workspace-message class="workspace-message" role="status"></p>
        <div data-workspace-content></div>
      </section><footer class="workspace-applybar" data-workspace-footer></footer>`;
    q('[data-add-project]').onclick=addProject;
  }
  function addProject() {
    dialogs.push(ProjectDialog(root,{onCreated:id=>{
      folderCache.clear();
      const route=`#/projects/${encodeURIComponent(id)}`;
      if(location.hash===route)init();else location.hash=route;
    }}));
  }
  const isGit = () => !repo.repo_id.startsWith('nongit-');
  const nodeName = () => relative || (nodeKind==='worktree'?wtLabel(wtId):projectName(repo));
  const wtLabel = id => repo.worktrees[id]?.branch?.replace(/^refs\/heads\//,'') || repo.worktrees[id]?.path.split('/').pop() || id;
  const configured = (id, project=repo) => {
    const map = new Map();
    for (const d of scopeSets.get(scopeKey(id, project))?.dirs || []) map.set(d.path,d);
    return [...map.values()];
  };
  function parentNode() {
    if (nodeKind === 'project') return null;
    if (!relative) return {kind:'project',rel:'',label:projectName(repo)};
    const ancestors = configured(wtId).filter(d=>relative.startsWith(d.path+'/')).sort((a,b)=>b.path.length-a.path.length);
    if (ancestors.length) return {kind:'directory',rel:ancestors[0].path,label:ancestors[0].path};
    return {kind:isGit()?'worktree':'project',rel:'',label:isGit()?wtLabel(wtId):projectName(repo)};
  }
  function inherits() {
    if (relative) return configured(wtId).find(d=>d.path===relative)?.inherit_resources !== false;
    return scopeSets.get(scopeKey(wtId))?.root_inherits !== false;
  }
  const primaryWorktree=p=>Object.keys(p.worktrees).find(k=>p.common_dir===p.worktrees[k].path+'/.git')||Object.keys(p.worktrees)[0];
  function openTreeNode(project, id, rel='', kind='project', add=false) {
    if(busy)return;
    if(project.repo_id===repo.repo_id){
      if(add)addEnvironment();else activate(rel,id,kind);
      return;
    }
    const state={repoId:project.repo_id,wt:id,dir:rel,node:kind};
    sessionStorage.setItem('ailoom-directory-view',JSON.stringify(state));
    if(add)pendingEnvironmentProject=project.repo_id;
    location.hash=`#/projects/${encodeURIComponent(project.repo_id)}`;
  }
  function treeNodes() {
    const order=items=>treeOrder==='name'?[...items].sort((a,b)=>a.label.localeCompare(b.label,undefined,{numeric:true,sensitivity:'base'})):items;
    function directories(project,wt) {
      return order(configured(wt,project).map(d=>({key:JSON.stringify([project.repo_id,wt,d.path]),project,wt,rel:d.path,kind:'directory',label:d.path,type:d.missing?'失联':'环境',children:[]})));
    }
    return order(repos.map(project=>{
      const git=!project.repo_id.startsWith('nongit-'),wt=primaryWorktree(project);
      return {key:JSON.stringify([project.repo_id]),project,wt,rel:'',kind:'project',label:projectName(project),type:git?'Git':'文件夹',children:git?order(Object.entries(project.worktrees).map(([id,w])=>({key:JSON.stringify([project.repo_id,id]),project,wt:id,rel:'',kind:'worktree',label:w.branch?.replace(/^refs\/heads\//,'')||w.path.split('/').pop(),type:w.status==='active'?'Worktree':'失联',children:directories(project,id)}))):directories(project,wt)};
    }));
  }
  function renderTree() {
    if(disposed)return;
    const nodes=treeNodes(),lookup=new Map();
    function render(nodes,depth=0) {
      return nodes.map(node=>{
        lookup.set(node.key,node);
        const selected=node.project.repo_id===repo.repo_id&&node.kind===nodeKind&&(node.kind==='project'||node.wt===wtId&&node.rel===relative);
        const expanded=!collapsed.has(node.key),hasChildren=node.children.length>0;
        return `<div role="treeitem" tabindex="${selected?'0':'-1'}" aria-level="${depth+1}" aria-selected="${selected}" ${hasChildren?`aria-expanded="${expanded}"`:''} data-tree-key="${esc(node.key)}"><div class="navigation-tree-row ${selected?'selected':''}" style="--depth:${depth}">${hasChildren?`<button class="navigation-twist" data-tree-toggle="${esc(node.key)}" tabindex="-1" aria-label="${expanded?'收起':'展开'} ${esc(node.label)}">${icon('chevron')}</button>`:'<span class="navigation-twist"></span>'}<button class="navigation-node" data-tree-open="${esc(node.key)}" tabindex="-1" title="${esc(node.project.worktrees[node.wt]?.path||'')}${node.rel?'/'+esc(node.rel):''}">${icon('folder')}<span>${esc(node.label)}</span><small>${esc(node.type)}</small></button>${node.kind==='project'?`<button class="navigation-add" data-add-environment="${esc(node.project.repo_id)}" aria-label="为${esc(node.label)}添加子 Agent 环境" ${!node.wt?'disabled':''}>${icon('plus')}</button>`:''}</div>${hasChildren?`<div role="group" ${expanded?'':'hidden'}>${render(node.children,depth+1)}</div>`:''}</div>`;
      }).join('');
    }
    q('[data-folders]').innerHTML=render(nodes);
    const items=[...root.querySelectorAll('[data-tree-key]')];
    if(!items.some(el=>el.tabIndex===0)&&items[0])items[0].tabIndex=0;
    function focusKey(key){const el=[...root.querySelectorAll('[data-tree-key]')].find(e=>e.dataset.treeKey===key);if(el){root.querySelectorAll('[data-tree-key]').forEach(e=>e.tabIndex=-1);el.tabIndex=0;el.focus();}}
    function toggle(key){collapsed.has(key)?collapsed.delete(key):collapsed.add(key);saveTree();renderTree();focusKey(key);}
    root.querySelectorAll('[data-tree-toggle]').forEach(b=>{b.onclick=()=>toggle(b.dataset.treeToggle);});
    root.querySelectorAll('[data-tree-open]').forEach(b=>{b.onclick=()=>{const n=lookup.get(b.dataset.treeOpen);openTreeNode(n.project,n.wt,n.rel,n.kind);};});
    root.querySelectorAll('[data-add-environment]').forEach(b=>{b.onclick=()=>{const p=repos.find(r=>r.repo_id===b.dataset.addEnvironment);openTreeNode(p,primaryWorktree(p),'','project',true);};});
    items.forEach(el=>{el.onkeydown=e=>{
      if(e.target!==el)return;
      const n=lookup.get(el.dataset.treeKey),visible=[...root.querySelectorAll('[data-tree-key]')].filter(x=>!x.closest('[hidden]')),index=visible.indexOf(el);
      if(['ArrowDown','ArrowUp','Home','End'].includes(e.key)){e.preventDefault();focusKey(visible[e.key==='Home'?0:e.key==='End'?visible.length-1:Math.max(0,Math.min(visible.length-1,index+(e.key==='ArrowDown'?1:-1)))].dataset.treeKey);}
      if(e.key==='ArrowRight'&&n.children.length){e.preventDefault();if(collapsed.has(n.key))toggle(n.key);else focusKey(n.children[0].key);}
      if(e.key==='ArrowLeft'){e.preventDefault();if(n.children.length&&!collapsed.has(n.key))toggle(n.key);else {const parent=el.parentElement.closest('[data-tree-key]');if(parent)focusKey(parent.dataset.treeKey);}}
      if(e.key==='Enter'||e.key===' '){e.preventDefault();openTreeNode(n.project,n.wt,n.rel,n.kind);}
    };});
    q('[data-tree-options]').onclick=()=>{
      const body=document.createElement('div');body.innerHTML=`<label>排序<select data-tree-sort><option value="original">保持原顺序</option><option value="name">按名称排序</option></select></label>`;
      const modal=Dialog(root,{title:'目录显示',content:body,actions:[{label:'全部展开',onAction:()=>{collapsed.clear();saveTree();renderTree();}},{label:'全部收起',onAction:()=>{for(const [key,n] of lookup)if(n.children.length)collapsed.add(key);saveTree();renderTree();}},{label:'完成'}]});dialogs.push(modal);
      body.querySelector('select').value=treeOrder;body.querySelector('select').onchange=e=>{treeOrder=e.target.value;saveTree();renderTree();};
    };
  }
  async function activate(rel='',nextWt=wtId,kind=rel?'directory':'worktree') {
    relative=rel;wtId=nextWt;nodeKind=kind;notice='';
    const wt=repo.worktrees[wtId];
    const args={projectId:repo.repo_id,name:projectName(repo),worktreeId:isGit()?wtId:'root',rootPath:wt.path,relativeDir:relative||null,kind:isGit()?'git':'nongit'};
    target=isGit()&&nodeKind==='project'?sharedSettingsTarget(args):directoryTarget(args);
    setTarget(target);renderTree();await refresh();
    if(!disposed)sessionStorage.setItem('ailoom-directory-view',JSON.stringify({repoId:repo.repo_id,wt:wtId,dir:relative,node:nodeKind}));
  }
  function renderHead() {
    const parent=parentNode();
    const type=nodeKind==='project'?(isGit()?'Git 项目 · 项目默认配置':'文件夹项目'):nodeKind==='worktree'?'Git Worktree':'子 Agent 环境';
    q('[data-workspace-head]').innerHTML=`<div class="workspace-breadcrumb"><span class="badge">${type}</span>${parent?`<span>来自：<button data-parent-node>${esc(parent.label)}</button></span>`:''}</div>
      <header class="workspace-heading"><div><h1>${esc(relative|| (nodeKind==='worktree'?wtLabel(wtId):projectName(repo)))}</h1><p class="workspace-path"><ailoom-path title="${esc(target.resolvedPath)}">${esc(target.resolvedPath)}</ailoom-path></p></div><button data-more>项目设置</button></header>
      <div class="workspace-node-controls">${parent?`<label><input type="checkbox" data-inherit-resources ${inherits()?'checked':''} ${busy?'disabled':''}>沿用上级能力</label><span class="muted">${inherits()?'可在这里增减。':'仅使用这里选择的能力。'}</span>`:`<span class="muted">${isGit()?'各 Worktree 默认使用以下能力。':'子环境默认使用以下能力。'}</span>`}
      ${isGit()&&nodeKind==='project'?'<button data-refresh-worktrees>刷新 Worktree</button>':''}</div>`;
    q('[data-more]').onclick=openSettings;
    if(parent)q('[data-parent-node]').onclick=()=>{if(!busy)activate(parent.rel,wtId,parent.kind);};
    if(q('[data-inherit-resources]'))q('[data-inherit-resources]').onchange=changeInheritance;
    if(q('[data-refresh-worktrees]'))q('[data-refresh-worktrees]').onclick=async()=>{
      try{await api.repoDiscover(target.rootPath);const state=await api.state();if(disposed)return;repos=state.repos;repo=repos.find(r=>r.repo_id===repo.repo_id);await loadFolders();renderHead();}catch(e){message(e.message);}
    };
  }
  async function loadFolders(projects=[repo], onlyMissing=false) {
    await Promise.all(projects.flatMap(project=>Object.entries(project.worktrees).map(async([id,w])=>{
      const key=scopeKey(id,project);
      if(onlyMissing&&scopeSets.has(key))return;
      try{await api.approveDir(w.path);const v=await api.projectDirs(w.path);if(!disposed)scopeSets.set(key,v);}
      catch(e){if(!disposed)scopeSets.set(key,{dirs:scopeSets.get(key)?.dirs||[],error:e.message});}
    })));
    if(!disposed)renderTree();
  }
  async function changeInheritance(event) {
    const next=event.target.checked;
    event.target.checked=!next;
    if(!next&&!await confirmAction('关闭后仅保留此节点明确添加的能力；父节点以后新增的能力也不会自动加入。AI 工具选择仍沿用父节点。文件变化仍需预览并应用。',{title:'改为独立选择',confirmLabel:'关闭继承'}))return;
    if(busy)return;
    busy=true;const frozen=target,gen=currentGeneration();renderHead();renderFooter();
    try{
      await api.configureScope({...selectionForDirectory(frozen,{},'inherit'),inherit_resources:next,base_revision:effective.profile_revision});
      if(!alive(gen))return;
      setTarget(frozen);await loadFolders();busy=false;notice=next?'已恢复父节点继承。':'已切换为独立选择，请添加需要的能力。';await refresh();
    }catch(e){if(alive(gen))message(e.message);}
    finally{busy=false;if(!disposed){renderHead();renderFooter();}}
  }
  function addEnvironment() {
    if(busy)return;
    const frozenRepo=repo.repo_id;
    let chosenWt=wtId, chosenRel='', saving=false, closed=false, scanVersion=0;
    const body=document.createElement('div');
    body.innerHTML=`
      ${isGit()?`<label>所在 Worktree<select data-environment-worktree>${Object.entries(repo.worktrees).filter(([,w])=>w.status==='active').map(([id,w])=>`<option value="${esc(id)}" ${id===chosenWt?'selected':''}>${esc(wtLabel(id))} · ${esc(w.path)}</option>`).join('')}</select></label>`:''}
      <button data-environment-browse>选择文件夹</button><p data-environment-path class="workspace-path">尚未选择文件夹</p>
      <details><summary>发现已有 Agent 配置（可选）</summary><p class="muted">查找已有 Agent 配置的文件夹。</p><label>扫描深度<select data-environment-depth><option value="3">3 层</option><option value="2">2 层</option></select></label><button data-environment-discover>查找文件夹</button><div data-environment-candidates></div></details>
      <p class="field-error" data-environment-error role="alert"></p>`;
    const field=s=>body.querySelector(s);
    const choose=rel=>{chosenRel=rel||'';field('[data-environment-path]').textContent=chosenRel?repo.worktrees[chosenWt].path+'/'+chosenRel:'请选择项目内的子文件夹，项目根目录已经存在。';};
    const modal=Dialog(root,{title:'添加子 Agent 环境',content:body,canClose:()=>!saving,onClose:()=>{closed=true;scanVersion++;},actions:[{label:'取消',onAction:()=>!saving},{label:'添加环境',variant:'primary',onAction:async()=>{
      if(!chosenRel){field('[data-environment-error]').textContent='请先选择一个子文件夹。';return false;}
      saving=true;
      const id=chosenWt,rel=chosenRel,path=repo.worktrees[id].path;
      body.querySelectorAll('button,select').forEach(el=>el.disabled=true);
      try{
        if(disposed||repo.repo_id!==frozenRepo)throw Error('项目已切换，请重新添加。');
        await api.approveDir(path);
        const dirs=await api.projectDirs(path);
        if(!dirs.dirs.some(d=>d.path===rel))await api.configureScope({root:path,subproject:rel,worktree:true,inherit_resources:true,base_revision:dirs.profile_revision});
        await loadFolders();await activate(rel,id,'directory');
      }catch(e){field('[data-environment-error]').textContent=e.message;return false;}
      finally{saving=false;body.querySelectorAll('button,select').forEach(el=>el.disabled=false);}
    }}]});dialogs.push(modal);
    const select=field('[data-environment-worktree]');
    if(select){chosenWt=select.value;select.onchange=()=>{chosenWt=select.value;choose('');scanVersion++;field('[data-environment-candidates]').replaceChildren();};}
    field('[data-environment-browse]').onclick=async()=>{
      try{
        const id=chosenWt,path=repo.worktrees[id].path;
        await api.approveDir(path);
        if(closed||disposed)return;
        const picker=DirectoryPicker(root,{title:'选择子 Agent 的文件夹',rootPath:path,rootLabel:projectName(repo),configuredDirs:configured(id),onPicked:rel=>{if(id===chosenWt)choose(rel);}});
        dialogs.push(picker);
      }catch(e){field('[data-environment-error]').textContent=e.message;}
    };
    field('[data-environment-discover]').onclick=async()=>{
      const version=++scanVersion,path=repo.worktrees[chosenWt].path;
      const output=field('[data-environment-candidates]');output.textContent='正在查找…';
      try{
        await api.approveDir(path);
        const result=await api.discoverDirectories(path,Number(field('[data-environment-depth]').value));
        if(closed||disposed||version!==scanVersion)return;
        output.innerHTML=result.candidates.map((c,i)=>`<p><button data-candidate="${i}">${esc(c.path)}</button> <small>${c.agents.map(a=>esc(a.label)).join('、')}</small></p>`).join('')||'<p class="muted">未找到含 Agent 配置的文件夹。仍可手动选择。</p>';
        if(result.truncated||result.errors.length)output.insertAdjacentHTML('beforeend','<p class="muted">部分目录未扫描，请使用文件夹选择器继续查找。</p>');
        output.querySelectorAll('[data-candidate]').forEach(button=>{button.onclick=()=>choose(result.candidates[Number(button.dataset.candidate)].path);});
      }catch(e){if(version===scanVersion)output.textContent=e.message;}
    };
  }
  async function refresh() {
    localPanel?.destroy();localPanel=null;
    const version = ++loadVersion, gen = currentGeneration();
    renderHead();
    q('[data-workspace-content]').innerHTML = '<p class="workspace-loading" role="status">正在读取这个目录的能力…</p>';
    q('[data-workspace-footer]').innerHTML = '<span class="muted">正在检查改动…</span>';
    try {
      await api.approveDir(target.rootPath);
      const values = await Promise.all([api.effective(target.rootPath,target.relativeDir,target.viewKind==='project-shared'?'project':undefined),api.resources(),api.deployStatus(target.rootPath,target.relativeDir).catch(e=>({error:e.message})),api.capabilities()]);
      if (!alive(gen) || version !== loadVersion) return;
      [effective,catalog,deployment,capabilities] = values;
      if (catalog.source_errors?.length) notice = '部分能力来源读取失败：' + catalog.source_errors.map(e=>e.error).join('；');
      if (deployment.error) notice = '暂时无法确认文件状态：' + deployment.error;
      renderHead(); renderContent(); renderFooter(); message(notice);
    } catch(e) {
      if (!alive(gen) || version !== loadVersion) return;
      q('[data-workspace-content]').innerHTML = `<div class="workspace-empty"><h2>暂时无法读取这个目录</h2><p>${esc(e.message)}</p><button data-retry>重新读取</button></div>`;
      q('[data-retry]').onclick = refresh;
      q('[data-workspace-footer]').textContent = '连接恢复后再应用，不会自动重试写入。';
    }
  }
  function renderContent() {
    localPanel?.destroy();
    q('[data-workspace-content]').innerHTML = `<div class="workspace-tools"><span>用于这些 AI 工具</span>${Object.entries(tools).map(([id,label])=>`<button data-host="${id}" aria-pressed="${!!effective.hosts?.[id]?.enabled}" ${busy?'disabled':''}>${effective.hosts?.[id]?.enabled ? icon('check') : icon('plus')}${label}</button>`).join('')}</div>
      <div class="workspace-section-heading"><div><h2>${target.viewKind==='project-shared'?'AILoom 管理 · 项目默认':'AILoom 管理'}</h2><p class="muted">${target.viewKind==='project-shared'?'':''}</p></div><button class="primary" data-add-capability>${icon('plus')}添加能力</button></div>
      <div class="workspace-filters"><div role="group" aria-label="筛选能力类型">${[['','全部'],['skill','Skill'],['mcp','MCP'],['rule','Rules'],['agent','Agent']].map(([id,label])=>`<button data-kind="${id}" aria-pressed="${filter===id}">${label}</button>`).join('')}</div><input type="search" data-search-capability aria-label="搜索当前目录能力" placeholder="搜索能力…" value="${esc(search)}"></div>
      <div data-capability-list></div><div data-disabled-list></div><div data-local-files></div>

      <div class="workspace-secondary"><button data-project-notes>编辑项目说明</button></div>`;
    localPanel=NativeFiles(q('[data-local-files]'),{rootPath:target.resolvedPath,scope:'project',showSkills:true,managedPaths:(deployment.items||[]).filter(i=>i.deployed).map(i=>target.resolvedPath.replace(/\/$/,'')+'/'+i.path)});
    q('[data-add-capability]').onclick = () => addCapabilities();
    q('[data-project-notes]').onclick = () => settings('instructions');
    root.querySelectorAll('[data-host]').forEach(b=>{b.onclick=async()=>{
      const id=b.dataset.host, enabled=!!effective.hosts?.[id]?.enabled;
      if (enabled && !await confirmAction(`不再为这个目录配置 ${tools[id]}？\n已写入的入口会在查看改动并应用后清理。`,{title:'停用 AI 工具',confirmLabel:'停用'})) return;
      await write({host:id},enabled?'disable':'enable');
    };});
    root.querySelectorAll('[data-kind]').forEach(b=>{b.onclick=()=>{filter=b.dataset.kind;root.querySelectorAll('[data-kind]').forEach(x=>x.setAttribute('aria-pressed',String(x===b))); renderRows();};});
    q('[data-search-capability]').oninput=e=>{search=e.target.value;renderRows();};
    renderRows();
  }
  function renderRows() {
    const entries=(catalog.entries || []).filter(e=> effective.resources?.[e.id]?.trace?.some(t=>t.choice!=='inherit'));
    const matches=e=>(!filter||e.kind===filter)&&`${e.name} ${e.description || ''}`.toLowerCase().includes(search.toLowerCase());
    const active=entries.filter(e=>effective.resources[e.id].deployed && matches(e));
    const off=entries.filter(e=>!effective.resources[e.id].deployed && matches(e));
    const kinds=filter?[filter]:[...new Set(['skill','mcp','rule','agent',...active.map(e=>e.kind)])];
    q('[data-capability-list]').innerHTML=kinds.map(kind=>{
      const rows=active.filter(e=>e.kind===kind);
      return `<section class="workspace-kind-group"><header><h3>${esc(names[kind]||kind)} <span>${rows.length}</span></h3><button data-add-kind="${esc(kind)}">＋ 添加 ${esc(names[kind]||kind)}</button></header>${rows.length?rows.map(e=>row(e,false)).join(''):(search?'<p class="workspace-kind-empty">没有匹配的能力。</p>':'')}</section>`;
    }).join('');
    root.querySelectorAll('[data-add-kind]').forEach(b=>{b.onclick=()=>addCapabilities(b.dataset.addKind);});
    q('[data-disabled-list]').innerHTML=off.length?`<details ${showOff?'open':''}><summary>已停用 ${off.length} 项</summary>${off.map(e=>row(e,true)).join('')}</details>`:'';
    q('[data-disabled-list] details')?.addEventListener('toggle',e=>{showOff=e.target.open;});
    for(const id of effective.unresolved_references || []) {
      if(search && !id.toLowerCase().includes(search.toLowerCase()))continue;
      const item=document.createElement('div');item.className='workspace-resource';item.innerHTML=`${icon('skill')}<div><strong>来源暂不可用</strong><p class="muted">${esc(id)}</p><small>选择仍保留；来源恢复后可重新应用。</small></div><span class="badge warn">需处理</span>`;q('[data-capability-list]').append(item);
    }
    root.querySelectorAll('[data-resource-action]').forEach(b=>{b.onclick=async()=>{
      const id=b.dataset.resourceAction, entry=catalog.entries.find(e=>e.id===id), current=effective.resources[id];
      const diff=diffOf(current.trace,layerOfTarget(target));
      if(current.deployed && !await confirmAction(`从「${nodeName()}」移除 ${entry.name}？\n资源库中的原件会保留。查看改动并应用后才会清理目录中的入口。`,{title:'移除能力',confirmLabel:'移除',destructive:true}))return;
      const choice=current.deployed?(diff.upstream?.choice==='enable'?'disable':'inherit'):'enable';
      await write({resource:id},choice);
    };});
    root.querySelectorAll('[data-resource-details]').forEach(b=>{b.onclick=()=>details(catalog.entries.find(e=>e.id===b.dataset.resourceDetails));});
  }
  function extensionRequirements(entry) {
    const hosts=new Set((deployment.items||[]).filter(i=>i.resource_id===entry.id).map(i=>i.tool));
    return (capabilities.capabilities||[]).filter(c=>c.kind===entry.kind&&hosts.has(c.tool)&&c.required_extension);
  }
  function row(entry,off) {
    const current=effective.resources[entry.id], diff=diffOf(current.trace,layerOfTarget(target));
    const items=(deployment.items||[]).filter(i=>i.resource_id===entry.id);
    const issues=(deployment.issues||[]).filter(i=>i.resource_id===entry.id);
    const applied=!issues.length&&items.length&&items.every(i=>i.state==='current');
    const extensions=extensionRequirements(entry);
    const requirement=extensions.map(c=>`${c.required_extension}：${applied?'配置已生成，':''}扩展加载未验证`).join('；');
    const status=target.viewKind==='project-shared'?'项目默认':deployment.error?'状态未知':off?'已停用':issues.length?'需处理':applied?(extensions.length?'已配置':'已应用'):'待应用';
    const inherited=!diff.here&&diff.upstream?.choice==='enable';
    return `<article class="workspace-resource"><span class="workspace-resource-icon">${icon(entry.kind)}</span><div class="workspace-resource-info"><button class="workspace-resource-title" data-resource-details="${esc(entry.id)}">${esc(entry.name || entry.id)}</button><p>${esc(entry.description || '暂无说明')}</p><small>${esc(names[entry.kind] || entry.kind)} · ${esc(entry.source_name || '本地能力')}${inherited?' · 来自上级':''}${extensions.length?' · 依赖官方扩展':''}</small></div><span class="workspace-resource-status ${applied&&!off&&!extensions.length?'current':''}" title="${esc([requirement,...issues.map(i=>`${tools[i.target_tool]||i.target_tool}：${i.reason}`)].filter(Boolean).join('；'))}">${status}</span><button data-resource-action="${esc(entry.id)}" ${busy||entry.kind==='package'?'disabled':''}>${off?'恢复':'移除'}</button></article>`;
  }
  function renderFooter() {
    if(target.viewKind==='project-shared') {
      q('[data-workspace-footer]').innerHTML='<div><strong>项目默认配置</strong><small>修改会立即保存；进入 Worktree 预览并应用文件改动。</small></div><button class="primary" data-open-worktree>选择 Worktree</button>';
      q('[data-open-worktree]').textContent='进入 '+wtLabel(wtId);q('[data-open-worktree]').onclick=()=>activate('',wtId,'worktree');return;
    }
    const pending=effective.pending_actions || 0;
    const selected=Object.values(effective.resources || {}).some(v=>v.deployed);
    const noHost=!Object.values(effective.hosts || {}).some(h=>h.enabled);
    q('[data-workspace-footer]').innerHTML=`<div><strong>${selected&&noHost?'先选择一个 AI 工具':pending?`${pending} 项文件改动待应用`:'当前目录没有待应用的改动'}</strong><small>${selected&&noHost?'在能力列表上方选择要使用的 AI 工具。':''}</small></div><button class="primary" data-review ${busy?'disabled':''}>查看改动 ${icon('chevron')}</button>`;
    q('[data-review]').onclick=review;
  }
  async function write(entry,state) {
    if(busy||disposed)return;
    busy=true;const frozen=target,gen=currentGeneration();renderContent();renderFooter();
    try { await api.select(selectionForDirectory(frozen,entry,state));if(!alive(gen))return;setTarget(frozen);notice='选择已保存。';await loadFolders();busy=false;await refresh(); }
    catch(e){if(alive(gen))message(e.message);}
    finally {busy=false;if(!disposed&&target===frozen&&q('[data-add-capability]')){renderHead();renderContent();renderFooter();}}
  }
  async function importResources() {
    return new Promise(resolve=>{
      const known=new Set((catalog.entries||[]).map(e=>e.id));
      const importer=ImportDialog(root,{onImported:async()=>{try{const next=await api.resources();if(disposed)return resolve(null);catalog=next;resolve({entries:availableEntries(),selectIds:(next.entries||[]).filter(e=>!known.has(e.id)).map(e=>e.id)});}catch{resolve(null);}},onCancelled:()=>resolve(null)});
      dialogs.push(importer);importer.show();
    });
  }
  function availableEntries() {
    const enabled=Object.keys(tools).filter(id=>effective.hosts?.[id]?.enabled);
    return (catalog.entries||[]).map(e=>{
      const unsupported=enabled.length>0&&enabled.every(tool=>{
        const c=(capabilities.capabilities||[]).find(c=>c.kind===e.kind&&c.tool===tool);
        return c?.support==='unsupported'||c?.support==='unknown';
      });
      const reason=unsupported?enabled.map(tool=>{const c=(capabilities.capabilities||[]).find(c=>c.kind===e.kind&&c.tool===tool);return `${tools[tool]}：${c?.notes||'暂不支持'}`;}).join('；'):null;
      return {...e,unavailable:e.kind==='package'?'依赖包暂不支持在此添加':reason,hint:e.kind==='agent'&&enabled.includes('pi')?'Pi 官方 subagent：调用时选择项目代理（agentScope: project 或 both）':null};
    });
  }
  function addCapabilities(kind = '') {
    if(busy||!effective)return;
    const frozen=target;
    const picker=ResourcePicker(root,{title:`添加${kind?(names[kind]||kind):'能力'} · ${nodeName()}`,description:target.viewKind==='project-shared'?'勾选项目共用的能力。':'勾选需要的能力。',entries:availableEntries().filter(e=>!kind||e.kind===kind),isAdded:id=>!!effective.resources?.[id]?.deployed,createResource:(!kind||['rule','agent'].includes(kind))?async()=>{const result=await ManagedDefinition(root,{kind:kind||null});if(!result||disposed)return null;catalog=await api.resources();return {entries:availableEntries().filter(e=>!kind||e.kind===kind),selectIds:[result.id]};}:null,importResources:async()=>{const r=await importResources();return r&&{...r,entries:r.entries.filter(e=>!kind||e.kind===kind),selectIds:r.selectIds.filter(id=>r.entries.some(e=>e.id===id&&(!kind||e.kind===kind)))};},onSubmit:async ids=>{
      if(disposed||target!==frozen)throw Error('目录已切换，请重新添加。');
      busy=true;const failed=[],gen=currentGeneration();
      try {
        for(const id of ids) {try{await api.select(selectionForDirectory(frozen,{resource:id},'enable'));}catch(e){failed.push({id,error:e.message});}}
        if(alive(gen)){setTarget(frozen);notice=`已保存 ${ids.length-failed.length} 项选择；尚未应用。`;busy=false;await refresh();}
        return {failed};
      }finally{busy=false;if(!disposed){renderFooter();}}
    }});dialogs.push(picker);picker.show();
  }
  function openSettings() { settings('profile'); }

  async function details(entry) {
    const body=document.createElement('div');
    body.innerHTML=`${extensionRequirements(entry).map(c=>`<p class="muted">${esc(c.notes)}。扩展加载未验证。</p>`).join('')}${(deployment.issues||[]).filter(i=>i.resource_id===entry.id).map(i=>`<p class="field-error">${esc(tools[i.target_tool]||i.target_tool)}：${esc(i.reason)}</p>`).join('')}<p>${esc(entry.description || '暂无说明')}</p><p class="muted">${esc(names[entry.kind]||entry.kind)} · ${esc(entry.source_name || '本地能力')}</p><div data-definition></div>`;
    const diff=diffOf(effective.resources?.[entry.id]?.trace,layerOfTarget(target));
    const actions=[{label:'关闭'}];
    if(entry.id.startsWith('personal/')&&['rule','agent'].includes(entry.kind))actions.unshift({label:'编辑',onAction:async()=>{await ManagedDefinition(root,{id:entry.id,kind:entry.kind});if(!disposed)await refresh();}});
    if(diff.here)actions.unshift({label:'恢复上级选择',onAction:async()=>{
      const outcome=diff.upstream?.choice==='enable'?'恢复后继续使用此能力。':'恢复后不再使用此能力。';
      if(!await confirmAction(outcome+'\n文件变化仍需查看并应用。',{title:'恢复上级选择',confirmLabel:'恢复'}))return false;
      await write({resource:entry.id},'inherit');
    }});
    const modal=Dialog(root,{title:entry.name||entry.id,content:body,actions});dialogs.push(modal);
    if(entry.kind==='mcp')try{const v=await api.mcpDetail(entry.id);if(!disposed&&modal.isOpen)body.querySelector('[data-definition]').innerHTML=`<h3>连接配置（只读）</h3><pre>${esc(JSON.stringify(v,null,2))}</pre>`;}catch(e){if(modal.isOpen)body.querySelector('[data-definition]').textContent=e.message;}
  }
  async function review() {
    const frozen=target,gen=currentGeneration();let applying=false,planId=null;
    const body=document.createElement('div');body.innerHTML=`<div class="workspace-review-target"><strong>${esc(nodeName())}</strong><p class="path">${esc(frozen.resolvedPath)}</p></div><div data-review-content role="status">正在检查需要改动的文件…</div><footer class="dialog-actions"><button data-close-review>返回修改</button><button class="primary" data-confirm-apply disabled>应用改动</button></footer>`;
    const modal=Dialog(root,{title:'确认这次改动',content:body,canClose:()=>!applying});dialogs.push(modal);
    const content=body.querySelector('[data-review-content]'),apply=body.querySelector('[data-confirm-apply]');
    body.querySelector('[data-close-review]').onclick=()=>modal.close();
    try {
      const job=await api.plan(frozen.rootPath,frozen.relativeDir);
      const done=await waitJob(job.job_id,{signal:controller.signal});
      if(!alive(gen)||!modal.isOpen)return;
      if(done.status!=='success')throw Error(done.error||'未能生成改动预览');
      planId=job.job_id;
      const changes=(done.result?.actions||[]).filter(a=>a.action!=='noop');
      const words={create:'添加',restore:'恢复',update:'更新',delete:'移除',conflict:'存在冲突',unsupported:'暂不支持'};
      content.innerHTML=changes.length?changes.map(a=>`<div class="workspace-change"><span class="badge ${['conflict','unsupported'].includes(a.action)?'warn':''}">${words[a.action]||esc(a.action)}</span><div><strong>${esc(catalog.entries?.find(e=>e.id===a.resource_id)?.name||a.resource_id||a.path)}</strong><p>${esc(tools[a.target_tool]||a.target_tool)} · ${esc(a.path)}</p>${['conflict','unsupported'].includes(a.action)?`<p class="field-error">${esc(a.reason)}</p>`:''}</div></div>`).join(''):'<div class="workspace-empty"><h3>已经是最新状态</h3><p>这个目录没有需要写入或清理的文件。</p></div>';
      if((done.result?.skipped_company_files||[]).length)content.innerHTML+='<p class="muted">部分已有文件受保护，已跳过。详见下方文件明细。</p>';
      content.innerHTML+=`<details><summary>技术详情</summary><pre>${esc(done.result?.summary||'无改动')}</pre><p class="muted">${esc((done.result?.notes||[]).join('；'))}</p><pre>${esc(JSON.stringify(done.result?.skipped_company_files||[],null,2))}</pre></details>`;
      apply.disabled=!changes.some(a=>!['conflict','unsupported'].includes(a.action));
      apply.onclick=async()=>{
        if(applying||!planId)return;
        applying=true;apply.disabled=true;body.querySelector('[data-close-review]').disabled=true;
        const consumed=planId;planId=null;
        try {
          const started=await api.apply(consumed);const result=await waitJob(started.job_id,{signal:controller.signal});
          if(!alive(gen)||!modal.isOpen)return;
          if(result.status!=='success')throw Error(result.error==='stale-plan'?'预览已经过期，没有写入文件。请返回并重新查看改动。':result.error||'应用失败');
          const r=result.result||{};
          if(r.ok===false||(r.skipped_conflicts||[]).length)throw Error('部分改动未完成，冲突文件已保留。请到操作记录查看详情，再重新预览。');
          const count=Array.isArray(r.applied)?r.applied.length:r.applied||0;
          const extensionNotes=[...new Set(changes.filter(a=>['create','update','restore'].includes(a.action)).flatMap(a=>(capabilities.capabilities||[]).filter(c=>c.kind===a.kind&&c.tool===a.target_tool&&c.required_extension).map(c=>`${c.required_extension}配置已生成；扩展加载未验证。`)))];
          content.innerHTML=`<div class="workspace-apply-success">${icon('check')}<h2>已应用 ${count} 项改动</h2>${extensionNotes.map(note=>`<p>${esc(note)}</p>`).join('')}${extensionNotes.length?'':`<p>在 ${esc(Object.keys(tools).filter(k=>effective.hosts?.[k]?.enabled).map(k=>tools[k]).join(' / ')||'AI 工具')} 中新开会话后使用。</p>`}<a href="#/tasks" data-result-history>查看操作记录或撤销</a></div>`;
          content.querySelector('[data-result-history]').onclick=()=>modal.close();apply.textContent='已应用';
          notice=`已应用到 ${frozen.resolvedPath}。`;await refresh();
        }catch(e){if(!disposed){const p=document.createElement('p');p.className='field-error';p.textContent=e.message;content.append(p);}}
        finally{applying=false;if(!disposed){body.querySelector('[data-close-review]').disabled=false;body.querySelector('[data-close-review]').textContent='返回目录';}}
      };
    }catch(e){if(!disposed&&modal.isOpen)content.textContent=`预览失败：${e.message}`;}
  }
  async function init() {
    try {
      const state=await api.state();if(disposed)return;
      repos=(state.repos||[]).map(r=>r.repo_id.startsWith('nongit-')&&!Object.keys(r.worktrees||{}).length?{...r,worktrees:{local:{path:r.common_dir,status:'active'}}}:r);
      let back,recent;
      try{recent=JSON.parse(sessionStorage.getItem('ailoom-directory-view'));back=JSON.parse(sessionStorage.getItem('ailoom-return-after-settings'));sessionStorage.removeItem('ailoom-return-after-settings');}catch{}
      repo=repos.find(r=>r.repo_id===(projectId||recent?.repoId))||(!projectId?repos[0]:null);
      if(!repo&&projectId&&repos.length){root.innerHTML='<section class="workspace-empty"><h1>找不到这个项目</h1><p>它可能已被移除，或链接已过期。</p><button data-back class="primary">返回我的目录</button></section>';q('[data-back]').onclick=()=>{location.hash='#/projects';};return;}
      if(!repo){root.innerHTML='<section class="workspace-empty"><h1>添加你的第一个目录</h1><p>选择本机文件夹，再为它添加需要的能力。</p><button data-add-project class="primary">添加项目</button></section>';q('[data-add-project]').onclick=addProject;return;}
      back=back?.repoId===repo.repo_id?back:recent?.repoId===repo.repo_id?recent:null;
      wtId=back?.repoId===repo.repo_id&&repo.worktrees[back.wt]?back.wt:Object.keys(repo.worktrees).find(k=>repo.common_dir===repo.worktrees[k].path+'/.git')||Object.keys(repo.worktrees)[0];
      relative=back?.repoId===repo.repo_id?back.dir||'':'';
      nodeKind=back?.repoId===repo.repo_id?(back.node||(relative?'directory':'project')):'project';
      if(!wtId)throw Error('这个项目没有可用目录。');
      shell();renderTree();await loadFolders(repos,true);await activate(relative,wtId,nodeKind);
      if(!disposed&&pendingEnvironmentProject===repo.repo_id){pendingEnvironmentProject=null;addEnvironment();}
    }catch(e){if(!disposed)root.innerHTML=`<section class="workspace-empty"><h1>暂时无法打开目录</h1><p>${esc(e.message)}</p><a href="#/projects">返回我的目录</a></section>`;}
  }
  init();
  return {isDirty:()=>localPanel?.isDirty()||false,destroy(){localPanel?.destroy();disposed=true;controller.abort();dialogs.forEach(d=>d.destroy());setTarget(null);root.remove();}};
}
