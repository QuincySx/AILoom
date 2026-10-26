import { ManagedDefinition } from '../features/managedDefinition.js';
import { NativeFiles } from '../features/nativeFiles.js';
import * as projectSettings from './projectSettings.js';
// Directory-first workspace. The server remains the source of configuration and deployment truth.
import { api, esc } from '../services/api.js';
import { directoryTarget, sharedSettingsTarget, layerOfTarget, diffOf, setTarget, currentGeneration, shouldApply, globallyAvailable } from '../state/target.js';
import { waitJob } from '../state/jobs.js';
import { Dialog, confirmAction } from '../components/dialog.js';
import { ResourcePicker } from '../features/resourcePicker.js';
import { DirectoryPicker } from '../features/directoryPicker.js';
import { ImportDialog } from '../features/importDialog.js';
import { ProjectDialog } from '../features/projectDialog.js';
import { TeamSourceDialog } from '../features/teamSourceDialog.js';
import {projectNavigation} from '../components/sectionNav.js';
import { PageHeader, setBreadcrumb } from '../components/layout.js';
import { Item, resourcePath, resourceIcon } from '../components/item.js';
import { icon as lucide } from '../components/icons.js';

const names = {skill:'技能',mcp:'MCP',agent:'子代理',rule:'规则',doc:'文档',learning:'经验',env:'环境',hook:'Hook',package:'依赖包'};
const tools = {claude:'Claude Code',codex:'Codex',grok:'Grok',pi:'Pi',opencode:'OpenCode',cursor:'Cursor'};
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
// Git project defaults are a settings layer, worktrees are branches, registered folders and folder projects are folders.
const treeIcon = node => node.kind==='worktree'?'branch':node.kind==='project'&&!node.project.repo_id.startsWith('nongit-')?'layers':'folder';
const projectName = r => r.project?.name || Object.values(r.worktrees || {})[0]?.path?.split('/').pop() || r.repo_id;

export function selectionForDirectory(t, entry, state) {
  return {root:t.rootPath,...entry,state,worktree:t.viewKind!=='project-shared'&&!(t.kind==='nongit'&&!t.relativeDir),...(t.relativeDir?{subproject:t.relativeDir}:{})};
}

const folderCache = new Map();
let pendingEnvironmentProject = null;

export function mount(container, {projectId,tab}) {
  const root = document.createElement('div');
  root.className = 'workspace-explorer';
  container.append(root);
  let disposed = false, busy = false, loadVersion = 0;
  let repo, repos = [], wtId, relative = '', nodeKind = 'project', target, effective, catalog, deployment, capabilities;
  const scopeSets = folderCache;
  const scopeKey = (id, project=repo) => `${project.repo_id}:${id}`;
  let filter = '', search = '';
  let notice = '', localPanel=null;
  const filesView=tab==='files';
  const settingsView=['settings','profile','instructions','knowledge'].includes(tab);
  const dialogs = [];
  const controller = new AbortController();
  const q = selector => root.querySelector(selector);
  const alive = gen => !disposed && shouldApply(gen);
  const message = text => { notice = text; if (q('[data-workspace-message]')) q('[data-workspace-message]').textContent = text; };
  let collapsed=new Set();
  try{collapsed=new Set(JSON.parse(localStorage.getItem('ailoom-tree-preferences'))?.collapsed||[]);}catch{}
  const saveTree=()=>localStorage.setItem('ailoom-tree-preferences',JSON.stringify({collapsed:[...collapsed]}));
  root.innerHTML = '<p role="status">正在打开目录…</p>';

  function remember() {
    sessionStorage.setItem('ailoom-return-after-settings', JSON.stringify({repoId:repo.repo_id, wt:wtId, dir:relative || null, node:nodeKind, tab:0}));
  }
  function shell() {
    root.innerHTML = `<aside class="workspace-rail" aria-label="项目内的工作目录">
      <div data-tree-panel id="workspace-directory-nav">
      <div class="workspace-rail-heading"><strong>目录</strong><button data-add-directory aria-label="注册子目录" title="注册子目录">${lucide('plus')}</button><button data-close-directory aria-label="关闭目录导航">${lucide('x')}</button></div>
      <div data-folders role="tree" aria-label="项目目录"></div><p data-tree-error class="field-error" role="status"></p>
      </div></aside>
      <section class="workspace-body" aria-label="当前目录的能力">
        <div data-workspace-head></div><p data-workspace-message class="workspace-message" role="status"></p>
        <div data-workspace-content></div>
      </section>`;
    q('[data-add-directory]').onclick=addEnvironment;
    root.onkeydown=e=>{if(e.key==='Escape'&&root.classList.contains('tree-open')&&!root.querySelector('dialog:modal')){e.preventDefault();q('[data-close-directory]').click();}};
    q('[data-close-directory]').onclick=()=>{root.classList.remove('tree-open');q('[data-switch-directory]').setAttribute('aria-expanded','false');q('[data-switch-directory]').focus();};
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
    if (!relative) return {kind:'project',rel:'',label:'项目默认'};
    const ancestors = configured(wtId).filter(d=>relative.startsWith(d.path+'/')).sort((a,b)=>b.path.length-a.path.length);
    if (ancestors.length) return {kind:'directory',rel:ancestors[0].path,label:ancestors[0].path};
    return {kind:isGit()?'worktree':'project',rel:'',label:isGit()?wtLabel(wtId):projectName(repo)};
  }
  function inherits() {
    if (relative) return configured(wtId).find(d=>d.path===relative)?.inherit_resources !== false;
    return scopeSets.get(scopeKey(wtId))?.root_inherits !== false;
  }
  const primaryWorktree=p=>Object.keys(p.worktrees).find(k=>p.worktrees[k].status!=='missing'&&p.common_dir===p.worktrees[k].path+'/.git')||Object.keys(p.worktrees).find(k=>p.worktrees[k].status!=='missing')||Object.keys(p.worktrees)[0];
  async function openTreeNode(project, id, rel='', kind='project', add=false) {
    if(busy)return;
    if(localPanel?.isDirty?.()&&!await confirmAction('当前页面有未保存内容。确定放弃这些修改并离开？',{title:'离开当前目录',confirmLabel:'放弃并离开'}))return;
    if(disposed)return;
    if(!add){
      root.classList.remove('tree-open');
      const switcher=q('[data-switch-directory]');
      switcher?.setAttribute('aria-expanded','false');
      if(switcher?.getBoundingClientRect().height)switcher.focus();
    }
    if(project.repo_id===repo.repo_id){
      if(add)addEnvironment();else activate(rel,id,kind==='group'?'project':kind);
      return;
    }
    const state={repoId:project.repo_id,wt:id,dir:rel,node:kind};
    sessionStorage.setItem('ailoom-directory-view',JSON.stringify(state));
    if(add)pendingEnvironmentProject=project.repo_id;
    location.hash=`#/projects/${encodeURIComponent(project.repo_id)}`;
  }
  function treeNodes() {
    function directories(project,wt) {
      const rows=configured(wt,project).map(d=>({key:JSON.stringify([project.repo_id,wt,d.path]),project,wt,rel:d.path,kind:'directory',label:d.path,type:d.missing?'失联':'',children:[]}));
      const roots=[];
      for(const row of rows) {
        const parent=rows.filter(n=>row.rel.startsWith(n.rel+'/')).sort((a,b)=>b.rel.length-a.rel.length)[0];
        if(parent){row.label=row.rel.slice(parent.rel.length+1);parent.children.push(row);}else roots.push(row);
      }
      return roots;
    }
    const nodes=repos.flatMap(project=>{
      const git=!project.repo_id.startsWith('nongit-'),wt=primaryWorktree(project);
      const children=git?[
        {key:JSON.stringify([project.repo_id,'defaults']),project,wt,rel:'',kind:'project',label:'项目默认',type:'',children:[]},
        ...Object.entries(project.worktrees).map(([id,w])=>({key:JSON.stringify([project.repo_id,id]),project,wt:id,rel:'',kind:'worktree',label:w.branch?.replace(/^refs\/heads\//,'')||w.path.split('/').pop(),type:w.status==='missing'?'失联':'',children:directories(project,id)}))
      ]:directories(project,wt);
      return git?children:[{key:JSON.stringify([project.repo_id]),project,wt,rel:'',kind:'project',label:projectName(project),type:'',children}];
    });
    return nodes;
  }

  function renderTree() {
    if(disposed)return;
    const nodes=treeNodes(),lookup=new Map();
    function render(nodes,depth=0) {
      return nodes.map(node=>{
        lookup.set(node.key,node);
        const selected=node.project.repo_id===repo.repo_id&&node.kind===nodeKind&&(node.kind==='project'||node.wt===wtId&&node.rel===relative);
        const expanded=!collapsed.has(node.key),hasChildren=node.children.length>0;
        return `<div role="treeitem" tabindex="${selected?'0':'-1'}" aria-level="${depth+1}" aria-selected="${selected}" ${hasChildren?`aria-expanded="${expanded}"`:''} data-tree-key="${esc(node.key)}"><div class="navigation-tree-row ${selected?'selected':''}" style="--depth:${depth}">${hasChildren?`<button class="navigation-twist" data-tree-toggle="${esc(node.key)}" tabindex="-1" aria-label="${expanded?'收起':'展开'} ${esc(node.label)}">${lucide('chevron-right')}</button>`:'<span class="navigation-twist"></span>'}<button class="navigation-node item-surface" aria-current="${selected?'location':'false'}" data-tree-open="${esc(node.key)}" tabindex="-1" title="${esc(node.project.worktrees[node.wt]?.path||'')}${node.rel?'/'+esc(node.rel):''}">${lucide(treeIcon(node))}<span>${esc(node.label)}</span><small>${esc(node.type)}</small></button>${(node.kind==='group'||node.kind==='project'&&!isGit())?`<button class="navigation-add" data-add-environment="${esc(node.project.repo_id)}" aria-label="为${esc(node.label)}登记子目录" ${!node.wt?'disabled':''}>${icon('plus')}</button>`:''}</div>${hasChildren?`<div role="group" ${expanded?'':'hidden'}>${render(node.children,depth+1)}</div>`:''}</div>`;
      }).join('');
    }
    q('[data-folders]').innerHTML=nodes.map(node=>render([node])).join('');
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
  }
  async function activate(rel='',nextWt=wtId,kind=rel?'directory':'worktree') {
    relative=rel;wtId=nextWt;nodeKind=kind;notice='';
    const wt=repo.worktrees[wtId];
    const rootPath=settingsView
      ? (wt?.status!=='missing'?wt?.path:null)||Object.values(repo.worktrees).find(w=>w.status!=='missing')?.path||wt?.path||repo.common_dir.replace(/\/\.git$/,'')
      : wt.path;
    const args={projectId:repo.repo_id,name:projectName(repo),worktreeId:isGit()?wtId:'root',rootPath,relativeDir:relative||null,kind:isGit()?'git':'nongit'};
    target=settingsView||isGit()&&nodeKind==='project'?sharedSettingsTarget(args):directoryTarget(args);
    setTarget(target);renderTree();await refresh();
    if(!disposed)sessionStorage.setItem('ailoom-directory-view',JSON.stringify({repoId:repo.repo_id,wt:wtId,dir:relative,node:nodeKind}));
  }
  function renderHead() {
    const label=relative?(isGit()?wtLabel(wtId)+' / ':'')+relative:nodeKind==='worktree'?wtLabel(wtId):isGit()?'项目默认':projectName(repo);
    setBreadcrumb([{label:'项目',href:'#/projects'},{label:projectName(repo),href:'#/projects/'+encodeURIComponent(repo.repo_id)},{label:settingsView?'项目设置':label}]);
    q('[data-workspace-head]').innerHTML=`<div class="workspace-heading">${PageHeader({title:settingsView?'项目设置':filesView?'本地文件':nodeKind==='project'&&isGit()?'项目默认':'AI 能力',description:settingsView?'共享设置对整个项目生效。':filesView?'确认工具实际会读取什么。':'选择、保存，再应用到本地工具。',actions:settingsView?'':'<span class="workspace-applybar" data-workspace-footer></span>'})}</div>
      <button class="workspace-mobile-switch" data-switch-directory aria-label="切换配置目录" aria-expanded="${root.classList.contains('tree-open')}" aria-controls="workspace-directory-nav">${lucide('folder')}<span><strong>${esc(projectName(repo))}</strong><small>${esc(label)}</small></span>${lucide('chevron-down')}</button>
      ${projectNavigation({id:repo.repo_id,active:settingsView?'settings':filesView?'files':'capabilities',settingsTab:settingsView?tab:'settings'})}`;
    q('[data-switch-directory]').onclick=()=>{const open=root.classList.toggle('tree-open');q('[data-switch-directory]').setAttribute('aria-expanded',String(open));if(open)q('[data-tree-key][aria-selected=true]')?.focus();};
    q('[data-workspace-head]').querySelectorAll('.section-nav a').forEach(a=>{a.onclick=remember;});
  }
  async function loadFolders(projects=[repo], onlyMissing=false) {
    await Promise.all(projects.flatMap(project=>Object.entries(project.worktrees).map(async([id,w])=>{
      const key=scopeKey(id,project);
      if(w.status==='missing'){scopeSets.set(key,{dirs:scopeSets.get(key)?.dirs||[],error:'目录不存在'});return;}
      if(onlyMissing&&scopeSets.has(key))return;
      try{await api.approveDir(w.path);const v=await api.projectDirs(w.path);if(!disposed)scopeSets.set(key,v);}
      catch(e){if(!disposed)scopeSets.set(key,{dirs:scopeSets.get(key)?.dirs||[],error:e.message});}
    })));
    if(!disposed)renderTree();
  }
  async function changeInheritance() {
    const next=true;
    if(!await confirmAction('改用上级目录的能力选择，当前工具选择保持不变。文件变化仍需预览并应用。',{title:'使用上级能力',confirmLabel:'使用上级能力'}))return;
    if(busy)return;
    busy=true;const frozen=target,gen=currentGeneration();renderHead();renderFooter();
    try{
      await api.configureScope({...selectionForDirectory(frozen,{},'inherit'),inherit_resources:next,base_revision:effective.profile_revision});
      if(!alive(gen))return;
      setTarget(frozen);await loadFolders();busy=false;notice=next?'已使用上级目录的能力。':'已切换为独立选择，请添加需要的能力。';await refresh();
    }catch(e){if(alive(gen))message(e.message);}
    finally{busy=false;if(!disposed){renderHead();renderFooter();}}
  }
  function addEnvironment() {
    if(busy)return;
    if(isGit()&&!Object.values(repo.worktrees).some(w=>w.status==='active')){message('先重新关联一个工作目录，再登记子目录。');return;}
    const frozenRepo=repo.repo_id;
    let chosenWt=wtId, chosenRel='', saving=false, closed=false, scanVersion=0;
    const body=document.createElement('div');
    body.innerHTML=`
      ${isGit()?`<label>所在分支工作目录<select data-environment-worktree>${Object.entries(repo.worktrees).filter(([,w])=>w.status==='active').map(([id,w])=>`<option value="${esc(id)}" ${id===chosenWt?'selected':''}>${esc(wtLabel(id))} · ${esc(w.path)}</option>`).join('')}</select></label>`:''}
      <button data-environment-browse>选择文件夹</button><p data-environment-path class="workspace-path">尚未选择文件夹</p><p data-environment-parent class="muted"></p>
      <details><summary>查找已有 AI 配置（可选）</summary><p class="muted"></p><label>扫描深度<select data-environment-depth><option value="3">3 层</option><option value="2">2 层</option></select></label><button data-environment-discover>查找文件夹</button><div data-environment-candidates></div></details>
      <p class="field-error" data-environment-error role="alert"></p>`;
    const field=s=>body.querySelector(s);
    const choose=rel=>{chosenRel=rel||'';field('[data-environment-path]').textContent=chosenRel?repo.worktrees[chosenWt].path+'/'+chosenRel:'请选择项目内的子文件夹，项目根目录已经存在。';const parent=configured(chosenWt).filter(d=>chosenRel.startsWith(d.path+'/')).sort((a,b)=>b.path.length-a.path.length)[0];field('[data-environment-parent]').textContent=chosenRel?'将使用 '+(parent?.path||(isGit()?wtLabel(chosenWt):projectName(repo)))+' 的能力。':'';};
    const modal=Dialog(root,{title:'登记子目录',content:body,canClose:()=>!saving,onClose:()=>{closed=true;scanVersion++;},actions:[{label:'取消',onAction:()=>!saving},{label:'登记目录',variant:'primary',onAction:async()=>{
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
        const picker=DirectoryPicker(root,{title:'选择子目录',rootPath:path,rootLabel:projectName(repo),configuredDirs:configured(id),onPicked:rel=>{if(id===chosenWt)choose(rel);}});
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
        output.innerHTML=result.candidates.map((c,i)=>`<p><button data-candidate="${i}">${esc(c.path)}</button> <small>${c.agents.map(a=>esc(a.label)).join('、')}</small></p>`).join('')||'<p class="muted">未找到含 AI 配置的文件夹，仍可手动选择。</p>';
        if(result.truncated||result.errors.length)output.insertAdjacentHTML('beforeend','<p class="muted">部分目录未扫描，请使用文件夹选择器继续查找。</p>');
        output.querySelectorAll('[data-candidate]').forEach(button=>{button.onclick=()=>choose(result.candidates[Number(button.dataset.candidate)].path);});
      }catch(e){if(version===scanVersion)output.textContent=e.message;}
    };
  }
  async function refresh() {
    localPanel?.destroy();localPanel=null;
    effective=null;
    const version = ++loadVersion, gen = currentGeneration();
    renderHead();
    if(settingsView) {
      const slot=q('[data-workspace-content]');slot.replaceChildren();
      localPanel=projectSettings.mount(slot,{projectId:repo.repo_id,tab,repo,rootPath:target.rootPath,embedded:true,onProjectChanged:updated=>{
        if(disposed||version!==loadVersion)return;
        repo=updated;repos=[repo];
        if(!repo.worktrees[wtId]){wtId=primaryWorktree(repo);relative='';nodeKind=isGit()&&wtId?'worktree':'project';}
        target={...target,name:projectName(repo)};setTarget(target);
        sessionStorage.setItem('ailoom-directory-view',JSON.stringify({repoId:repo.repo_id,wt:wtId,dir:relative,node:nodeKind}));
        renderTree();renderHead();
        void loadFolders([repo],true);
      }});
      message('');return;
    }
    q('[data-workspace-content]').innerHTML = '<p class="workspace-loading" role="status">正在读取这个目录的能力…</p>';
    q('[data-workspace-footer]').innerHTML = '<span class="muted">正在检查改动…</span>';
    try {
      await api.approveDir(target.rootPath);
      const values = await Promise.allSettled([api.effective(target.rootPath,target.relativeDir,target.viewKind==='project-shared'?'project':undefined),api.resources(),api.deployStatus(target.rootPath,target.relativeDir),api.capabilities()]);
      if (!alive(gen) || version !== loadVersion) return;
      const [config,resources,status,matrix]=values;
      catalog=resources.status==='fulfilled'?resources.value:null;
      deployment=status.status==='fulfilled'?status.value:{error:status.reason.message};
      capabilities=matrix.status==='fulfilled'?matrix.value:{capabilities:[]};
      if(config.status==='rejected'||resources.status==='rejected'||matrix.status==='rejected'){
        renderHead();renderUnavailable(config.status==='rejected'?config.reason:resources.status==='rejected'?resources.reason:matrix.reason);return;
      }
      effective=config.value;
      // 团队来源启用的资源不在资源库目录里：并入列表，和资源库能力一起显示
      const known=new Set(catalog.entries.map(e=>e.id));
      catalog={...catalog,entries:[...catalog.entries,...(effective.team_resources||[]).filter(e=>!known.has(e.id))]};
      if (catalog.source_errors?.length) notice = '部分能力来源读取失败：' + catalog.source_errors.map(e=>e.error).join('；');
      if (deployment.error) notice = '暂时无法确认文件状态：' + deployment.error;
      renderHead(); renderContent(); renderFooter(); message(notice);
    } catch(e) {
      if (!alive(gen) || version !== loadVersion) return;
      renderUnavailable(e);
    }
  }
  function renderUnavailable(error) {
    effective=null;
    const missing=repo.worktrees[wtId]?.status==='missing'||configured(wtId).some(d=>d.path===relative&&d.missing);
    const sourceError=!missing&&error.data?.code?.startsWith('E2');
    const hint=error.data?.hint||error.data?.fix;
    q('[data-workspace-content]').innerHTML=`<section class="workspace-read-error" data-config-unavailable role="status"><h2>${missing?'目录已失联':sourceError?'团队来源暂不可用':'暂时无法确认能力配置'}</h2><p>${esc(error.message)}</p><p class="muted">已部署文件和已保存的选择会保留。</p>${hint&&!sourceError?`<details><summary>如何恢复</summary><p>${esc(hint)}</p></details>`:''}<div class="actions">${sourceError?'<button class="primary" data-edit-team-source>修改来源</button>':''}${missing&&!relative&&isGit()?'<button data-relink-directory>选择新位置</button>':missing&&relative?'<button data-register-directory>登记其他目录</button>':''}<button data-retry>重新读取</button>${catalog?`<a href="#/library">查看资源库 · ${catalog.entries?.length||0} 项</a>`:''}</div></section><details class="workspace-local" open><summary>查看本地配置文件</summary><div data-local-files></div></details>`;
    q('[data-retry]').onclick=refresh;
    q('[data-register-directory]')?.addEventListener('click',addEnvironment);
    q('[data-relink-directory]')?.addEventListener('click',async()=>{
      const button=q('[data-relink-directory]');button.disabled=true;
      try{const chosen=await api.pickDirectory();if(disposed||!chosen.path)return;await api.approveDir(chosen.path);const result=await api.repoRelink({repo_id:repo.repo_id,wt_id:wtId,new_path:chosen.path});if(disposed)return;scopeSets.delete(scopeKey(wtId));
        if(result.repo_id!==repo.repo_id){location.hash='#/projects/'+encodeURIComponent(result.repo_id);}else{await init();message('目录已重新关联，原有选择保留。');}
      }catch(e){message(e.message);}finally{if(!disposed)button.disabled=false;}
    });
    q('[data-edit-team-source]')?.addEventListener('click',()=>{
      const frozen=target,gen=currentGeneration();
      dialogs.push(TeamSourceDialog(root,{rootPath:frozen.rootPath,onSaved:async()=>{if(alive(gen)){await refresh();message('团队来源已重新绑定。');}}}));
    });
    renderFooter();
    const inherit=q('[data-inherit-resources]');if(inherit)inherit.disabled=true;
    localPanel=NativeFiles(q('[data-local-files]'),{rootPath:target.resolvedPath,scope:'project',showSkills:true,readOnly:true});
    message('');
  }
  function toolsAreLocal() {
    return Object.values(effective.hosts||{}).some(h=>diffOf(h.trace,layerOfTarget(target)).here);
  }
  function chooseTools() {
    if(busy||!effective)return;
    const frozen=target, gen=currentGeneration(), parent=parentNode(), chosen=new Set(Object.keys(tools).filter(id=>effective.hosts?.[id]?.enabled));
    let saving=false;
    const body=document.createElement('div');
    const draw=()=>{
      const inherited=parent&&!toolsAreLocal();
      body.innerHTML=`${parent?`<label><input type="checkbox" data-use-parent-tools ${inherited?'checked':''}>使用 ${esc(parent.label)} 的工具</label><p class="muted">上级更换工具时，本目录也会一起更新。</p>`:'<p class="muted">选择这个项目使用的 AI 工具。</p>'}<div class="workspace-tools">${Object.entries(tools).map(([id,label])=>`<button data-host="${id}" aria-pressed="${chosen.has(id)}" ${inherited?'disabled':''}>${icon(chosen.has(id)?'check':'plus')}${label}</button>`).join('')}</div><p data-tools-error class="field-error" role="alert"></p>`;
    };
    draw();
    const modal=Dialog(root,{title:'选择 AI 工具',content:body,canClose:()=>!saving,actions:[{label:'取消'},{label:'保存选择',variant:'default',onAction:async()=>{
      if(saving||target!==frozen)return false;
      saving=true;busy=true;body.querySelectorAll('button,input').forEach(e=>e.disabled=true);
      try {
        const inherit=body.querySelector('[data-use-parent-tools]')?.checked;
        for(const id of Object.keys(tools))await api.select(selectionForDirectory(frozen,{host:id},inherit?'inherit':chosen.has(id)?'enable':'disable'));
        if(!alive(gen))return true;
        setTarget(frozen);busy=false;await refresh();return true;
      }catch(e){if(alive(gen)){busy=false;await refresh();body.querySelector('[data-tools-error]').textContent='部分选择可能已保存：'+e.message;}return false;}
      finally{saving=false;busy=false;if(!disposed){body.querySelectorAll('button,input').forEach(e=>e.disabled=false);renderFooter();}}
    }}]});dialogs.push(modal);
    body.querySelector('[data-use-parent-tools]')?.addEventListener('change',e=>body.querySelectorAll('[data-host]').forEach(b=>b.disabled=e.target.checked));
    body.querySelectorAll('[data-host]').forEach(b=>{b.onclick=()=>{chosen.has(b.dataset.host)?chosen.delete(b.dataset.host):chosen.add(b.dataset.host);b.setAttribute('aria-pressed',String(chosen.has(b.dataset.host)));};});
  }
  async function resetTools() {
    if(busy)return;
    const frozen=target,gen=currentGeneration();busy=true;
    try{for(const id of Object.keys(tools))await api.select(selectionForDirectory(frozen,{host:id},'inherit'));if(!alive(gen))return;setTarget(frozen);busy=false;await refresh();}
    catch(e){message(e.message);}finally{busy=false;if(!disposed)renderFooter();}
  }
  function selectedEntries() {
    return (catalog.entries||[]).filter(e=>effective.resources?.[e.id]?.deployed);
  }
  function renderContent() {
    if(!effective)return;
    localPanel?.destroy();localPanel=null;
    if(filesView) {
      q('[data-workspace-content]').innerHTML='<div data-local-files></div>';
      localPanel=NativeFiles(q('[data-local-files]'),{rootPath:target.resolvedPath,scope:'project',showSkills:true,hideHeading:true,includeManaged:true,managedPaths:(deployment.items||[]).filter(i=>i.deployed).map(i=>target.resolvedPath.replace(/\/$/,'')+'/'+i.path)});
      return;
    }
    const enabled=Object.entries(tools).filter(([id])=>effective.hosts?.[id]?.enabled),parent=parentNode(),local=toolsAreLocal();
    const inherited=parent&&inherits();
    q('[data-workspace-content]').innerHTML=`<section class="workspace-section"><div class="workspace-section-title"><h2>使用工具</h2><small>${enabled.length} 个工具</small><small class="workspace-origin">${esc(parent&&!local?'使用'+(parent.kind==='project'?'项目默认':parent.label):nodeKind==='project'?'项目默认':'自定义配置')}</small>${parent&&local?`<button data-reset-tools data-variant="ghost" ${busy?'disabled':''}>使用上级工具</button>`:''}<button data-tool-change ${busy?'disabled':''}>${parent&&!local?'自定义':'修改'}</button></div><div class="workspace-tool-list">${enabled.map(([id,label])=>Item({title:label,actions:`<span class="workspace-tool-state on ${parent&&!local?'inherited':''}" role="img" aria-label="已选用 ${esc(label)}"></span>`})).join('')||'<p class="muted">尚未选择工具</p>'}</div></section>
      <section class="workspace-section"><div class="workspace-section-title"><h2>已选能力</h2><small>${selectedEntries().length} 项</small><small class="workspace-origin">${esc(inherited?'使用'+(parent.kind==='project'?'项目默认':parent.label):nodeKind==='project'?'项目默认':'自定义配置')}</small>${parent&&!inherited?`<button data-reset-resources data-variant="ghost" ${busy?'disabled':''}>使用上级能力</button>`:''}<button data-add-capability ${busy?'disabled':''}>${inherited?'自定义':'修改选择'}</button></div>
      <div class="workspace-filters" ${selectedEntries().length>8?'':'hidden'}><input type="search" data-search-capability aria-label="搜索当前目录能力" placeholder="搜索已选能力" value="${esc(search)}"><select data-capability-kind aria-label="筛选能力类型"><option value="">全部类型</option>${Object.entries(names).map(([id,label])=>`<option value="${id}" ${filter===id?'selected':''}>${esc(label)}</option>`).join('')}</select></div>
      <div data-capability-list class="workspace-resource-list" tabindex="0" aria-label="已选能力"></div></section>`;
    q('[data-tool-change]').onclick=chooseTools;
    q('[data-reset-tools]')?.addEventListener('click',resetTools);
    q('[data-reset-resources]')?.addEventListener('click',changeInheritance);
    q('[data-add-capability]').onclick=manageCapabilities;
    q('[data-capability-kind]').onchange=e=>{filter=e.target.value;renderRows();};
    q('[data-search-capability]').oninput=e=>{search=e.target.value;renderRows();};
    renderRows();
  }
  function renderRows() {
    const entries=selectedEntries();
    const matched=entries.filter(e=>(!filter||e.kind===filter)&&`${e.name} ${e.description||''}`.toLowerCase().includes(search.toLowerCase()));
    q('[data-capability-list]').innerHTML=matched.map(e=>row(e,false)).join('')||'<p class="workspace-empty">'+(entries.length?'没有匹配的能力。':'尚未选择能力。点击“修改选择”添加。')+'</p>';
    for(const id of effective.unresolved_references||[]) {
      q('[data-capability-list]').insertAdjacentHTML('beforeend',Item({title:'来源暂不可用',description:id,meta:'选择仍保留；来源恢复后可重新应用。',className:'workspace-resource',actions:'<span class="badge warn">需处理</span>'}));
    }
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
    const requirement=extensions.map(c=>`需要安装 ${c.required_extension}`).join('；');
    // AIL-152：已全局部署的 Skill 在项目内不重复部署，状态以全局为准
    const global=!off&&globallyAvailable(effective).has(entry.id);
    const status=target.viewKind==='project-shared'?'项目默认':global?'已全局启用':deployment.error?'状态未知':off?'已停用':issues.length?'需处理':applied?(extensions.length?'已配置':'已应用'):'待应用';
    const inherited=!diff.here&&diff.upstream?.choice==='enable';
    const ambiguous=!!entry.name&&(catalog.entries||[]).some(other=>other.id!==entry.id&&other.name===entry.name);
    const meta=off?'本目录不使用':ambiguous?entry.source_name:'';
    const feedback=[...issues.map(i=>`<span class="badge warn">需处理</span><span>${esc(tools[i.target_tool]||i.target_tool)}：${esc(i.reason)}</span>`),requirement?`<span class="badge warn">${status}</span><span>${esc(requirement)}</span>`:''].join('');
    return Item({title:entry.name||entry.id,primaryAction:{attributes:`data-resource-details="${esc(entry.id)}" title="${esc(entry.source_name||'')}"`,label:'查看能力 '+(entry.name||entry.id)+' · '+status},description:entry.description,meta,path:resourcePath(entry),leading:resourceIcon,className:'workspace-resource',feedback});
  }

  function renderFooter() {
    if(!effective){
      q('[data-workspace-footer]').innerHTML='<strong>文件状态尚未确认</strong><button disabled>预览改动</button>';return;
    }
    if(target.viewKind==='project-shared') {
      q('[data-workspace-footer]').innerHTML='<button class="primary" data-open-worktree>选择应用目录</button>';
      q('[data-open-worktree]').onclick=()=>activate('',wtId,'worktree');return;
    }
    const pending=effective.pending_actions || 0;
    const selected=Object.values(effective.resources || {}).some(v=>v.deployed);
    const noHost=!Object.values(effective.hosts || {}).some(h=>h.enabled);
    q('[data-workspace-footer]').innerHTML=`<strong>${pending?'文件待更新':noHost?'先选择 AI 工具':!selected?'添加需要的能力':''}</strong><button ${pending?'class="primary"':''} data-review ${busy||noHost&&!pending?'disabled':''}>预览改动</button>`;
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
  function addCapabilities(kind = '', selectedId = '') {
    if(busy||!effective)return;
    const frozen=target;
    const picker=ResourcePicker(root,{title:`添加${kind?(names[kind]||kind):'能力'} · ${nodeName()}`,description:target.viewKind==='project-shared'?'勾选项目共用的能力。':'勾选需要的能力。',entries:availableEntries().filter(e=>!kind||e.kind===kind),initialSelection:selectedId?[selectedId]:[],initialSearch:selectedId?(catalog.entries.find(e=>e.id===selectedId)?.name||''):'',isAdded:id=>!!effective.resources?.[id]?.deployed||globallyAvailable(effective).has(id),createResource:(!kind||['rule','agent'].includes(kind))?async()=>{const result=await ManagedDefinition(root,{kind:kind||null});if(!result||disposed)return null;catalog=await api.resources();return {entries:availableEntries().filter(e=>!kind||e.kind===kind),selectIds:[result.id]};}:null,importResources:async()=>{const r=await importResources();return r&&{...r,entries:r.entries.filter(e=>!kind||e.kind===kind),selectIds:r.selectIds.filter(id=>r.entries.some(e=>e.id===id&&(!kind||e.kind===kind)))};},onSubmit:async ids=>{
      if(disposed||target!==frozen)throw Error('目录已切换，请重新添加。');
      busy=true;const failed=[],gen=currentGeneration();
      try {
        for(const id of ids) {try{await api.select(selectionForDirectory(frozen,{resource:id},'enable'));}catch(e){failed.push({id,error:e.message});}}
        if(alive(gen)){setTarget(frozen);notice=`已保存 ${ids.length-failed.length} 项选择；尚未应用。`;busy=false;await refresh();}
        return {failed};
      }finally{busy=false;if(!disposed){renderFooter();}}
    }});dialogs.push(picker);picker.show();
  }

  function manageCapabilities() {
    if(busy||!effective)return;
    const frozen=target;
    const picker=ResourcePicker(root,{title:'修改能力选择',description:parentNode()&&inherits()?'保存后使用自定义能力；工具选择保持不变。':'保存选择后，预览并应用文件改动。',entries:availableEntries().map(e=>({...e,unavailable:effective.resources?.[e.id]?.deployed?null:globallyAvailable(effective).has(e.id)?'当前工具已全局启用':e.unavailable})),initialSelection:selectedEntries().map(e=>e.id),isAdded:()=>false,submitLabel:'保存选择',allowEmpty:true,importResources,createResource:async()=>{const result=await ManagedDefinition(root);if(!result||disposed)return null;catalog=await api.resources();return {entries:availableEntries(),selectIds:[result.id]};},onSubmit:async ids=>{
      if(disposed||target!==frozen)throw Error('目录已切换，请重新选择。');
      busy=true;
      const gen=currentGeneration(),chosen=new Set(ids),before=new Set(selectedEntries().map(e=>e.id));
      const inherited=parentNode()&&inherits(),failed=[];
      try {
        if(inherited)await api.configureScope({...selectionForDirectory(frozen,{},'inherit'),inherit_resources:false,base_revision:effective.profile_revision});
        for(const id of new Set([...before,...chosen])) {
          if(!inherited&&before.has(id)===chosen.has(id))continue;
          const diff=diffOf(effective.resources?.[id]?.trace,layerOfTarget(frozen));
          const choice=chosen.has(id)?'enable':diff.upstream?.choice==='enable'?'disable':'inherit';
          try {await api.select(selectionForDirectory(frozen,{resource:id},choice));}
          catch(e){failed.push({id,error:e.message});}
        }
        if(alive(gen)){setTarget(frozen);await loadFolders();busy=false;await refresh();}
        if(failed.length)throw Error(`${failed.length} 项保存失败：${failed.map(f=>f.error).join('；')}。已保存的选择保留，可重试。`);
      } finally {busy=false;if(!disposed)renderFooter();}
    }});dialogs.push(picker);picker.show();
  }

  async function details(entry) {
    const body=document.createElement('div');
    body.innerHTML=`${extensionRequirements(entry).map(c=>`<p class="muted">需要安装 ${esc(c.required_extension)}。${esc(c.notes)}</p>`).join('')}${(deployment.issues||[]).filter(i=>i.resource_id===entry.id).map(i=>`<p class="field-error">${esc(tools[i.target_tool]||i.target_tool)}：${esc(i.reason)}</p>`).join('')}<p>${esc(entry.description || '暂无说明')}</p><p class="muted">${esc(names[entry.kind]||entry.kind)}，来自${esc(entry.source_name || '本地能力')}</p><details><summary>来源与位置</summary><p class="path">${esc(entry.source_meta?.repo_path||entry.source?.repo_path||entry.path||entry.id)}</p><p class="path">${esc(entry.id)}</p></details><div data-definition></div>`;
    const diff=diffOf(effective.resources?.[entry.id]?.trace,layerOfTarget(target));
    const actions=[{label:'关闭'}];
    if(!(parentNode()&&inherits())&&entry.kind!=='package')actions.unshift({label:'移除能力',variant:'destructive',onAction:async()=>{
      if(!await confirmAction(`本目录不再使用 ${entry.name}？\n来源原件保留，文件变化需要预览并应用。`,{title:'移除能力',confirmLabel:'移除',destructive:true}))return false;
      await write({resource:entry.id},diff.upstream?.choice==='enable'?'disable':'inherit');
    }});
    if(entry.id.startsWith('personal/')&&['rule','agent'].includes(entry.kind))actions.unshift({label:'编辑',onAction:async()=>{await ManagedDefinition(root,{id:entry.id,kind:entry.kind});if(!disposed)await refresh();}});
    if(diff.here&&diff.upstream)actions.unshift({label:'使用上级选择',onAction:async()=>{
      const outcome=diff.upstream?.choice==='enable'?'改用上级选择后，继续使用此能力。':'改用上级选择后，不再使用此能力。';
      if(!await confirmAction(outcome+'\n文件变化仍需查看并应用。',{title:'使用上级选择',confirmLabel:'使用上级选择'}))return false;
      await write({resource:entry.id},'inherit');
    }});
    const modal=Dialog(root,{title:entry.name||entry.id,content:body,actions});dialogs.push(modal);
    if(entry.kind==='mcp')try{const v=await api.mcpDetail(entry.id);if(!disposed&&modal.isOpen)body.querySelector('[data-definition]').innerHTML=`<h3>连接配置（只读）</h3><pre>${esc(JSON.stringify(v,null,2))}</pre>`;}catch(e){if(modal.isOpen)body.querySelector('[data-definition]').textContent=e.message;}
  }
  async function review() {
    const frozen=target,gen=currentGeneration();let applying=false,planId=null;
    const body=document.createElement('div');body.innerHTML=`<div class="workspace-review-target"><strong>${esc(nodeName())}</strong><p class="path">${esc(frozen.resolvedPath)}</p></div><div data-review-content role="status">正在检查需要改动的文件…</div><footer class="dialog-actions"><button data-close-review>返回修改</button><button class="primary" data-confirm-apply disabled>应用改动</button></footer>`;
    const modal=Dialog(root,{title:'预览改动',content:body,canClose:()=>!applying});dialogs.push(modal);
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
      content.innerHTML=changes.length?changes.map(a=>Item({title:catalog.entries?.find(e=>e.id===a.resource_id)?.name||(a.resource_id?.startsWith('ailoom-personal/instructions/')?'项目说明':a.resource_id)||a.path,description:(tools[a.target_tool]||a.target_tool)+' · '+a.path,leading:`<span class="badge ${['conflict','unsupported'].includes(a.action)?'warn':''}">${words[a.action]||esc(a.action)}</span>`,feedback:['conflict','unsupported'].includes(a.action)?esc(a.reason):'',density:'regular',className:'workspace-change'})).join(''):'<div class="workspace-empty"><h3>已经是最新状态</h3><p>这个目录没有需要写入或清理的文件。</p></div>';
      if((done.result?.skipped_company_files||[]).length)content.innerHTML+='<p class="muted">部分已有文件受保护，已跳过。详见下方文件明细。</p>';
      content.innerHTML+=`<details><summary>技术详情</summary><pre>${esc(done.result?.summary||'无改动')}</pre><p class="muted">${esc((done.result?.notes||[]).join('；'))}</p><pre>${esc(JSON.stringify(done.result?.skipped_company_files||[],null,2))}</pre></details>`;
      apply.disabled=changes.some(a=>a.action==='conflict')||!changes.some(a=>!['conflict','unsupported'].includes(a.action));
      if(changes.some(a=>a.action==='conflict'))content.insertAdjacentHTML('beforeend','<p class="field-error">先处理冲突，再重新预览。</p>');
      apply.onclick=async()=>{
        if(applying||!planId)return;
        applying=true;apply.disabled=true;body.querySelector('[data-close-review]').disabled=true;
        const consumed=planId;planId=null;
        try {
          const started=await api.apply(consumed);const result=await waitJob(started.job_id,{signal:controller.signal});
          if(!alive(gen)||!modal.isOpen)return;
          if(result.status!=='success')throw Error(result.error==='stale-plan'?'预览已经过期，没有写入文件。请返回并重新查看改动。':result.error||'应用失败');
          const r=result.result||{};
          if(r.ok===false){const err=Error(`应用中途失败：${r.failed?.message||'未知错误'}。已写入的部分保留在恢复点中，可以撤回后再试。`);err.recoverable=true;throw err;}
          if((r.skipped_conflicts||[]).length)throw Error(`${r.skipped_conflicts.length} 个文件因冲突未应用，你的修改已保留。请到操作记录查看详情，处理后重新预览。`);
          const count=Array.isArray(r.applied)?r.applied.length:r.applied||0;
          const extensionNotes=[...new Set(changes.filter(a=>['create','update','restore'].includes(a.action)).flatMap(a=>(capabilities.capabilities||[]).filter(c=>c.kind===a.kind&&c.tool===a.target_tool&&c.required_extension).map(c=>`${c.required_extension}配置已生成；扩展加载未验证。`)))];
          content.innerHTML=`<div class="workspace-apply-success">${icon('check')}<h2>已应用 ${count} 项改动</h2>${extensionNotes.map(note=>`<p>${esc(note)}</p>`).join('')}${(r.skipped_unsupported||[]).length?`<p class="muted">${r.skipped_unsupported.length} 项当前 AI 工具暂不支持，未部署：${esc(r.skipped_unsupported.join('、'))}</p>`:''}${extensionNotes.length?'':`<p>在 ${esc(Object.keys(tools).filter(k=>effective.hosts?.[k]?.enabled).map(k=>tools[k]).join(' / ')||'AI 工具')} 中新开会话后使用。</p>`}<a href="#/tasks" data-result-history>查看操作记录或撤销</a></div>`;
          content.querySelector('[data-result-history]').onclick=()=>modal.close();apply.textContent='已应用';
          notice=`已应用到 ${frozen.resolvedPath}。`;await refresh();
        }catch(e){if(!disposed){const p=document.createElement('p');p.className='field-error';p.textContent=e.message;content.append(p);
          // 中途失败或有遗留恢复点时，提供撤回入口（否则后续每次应用都会被恢复点挡住）
          if(e.recoverable||/未完成的同步/.test(e.message)){const b=document.createElement('button');b.textContent='撤回已写入的部分';b.dataset.recover='';content.append(b);
            b.onclick=async()=>{b.disabled=true;try{const v=await api.projectRecover(frozen.rootPath);p.className='muted';p.textContent=`已撤回 ${v.recovered?.length||0} 项改动${(v.skipped_user_modified||[]).length?`；${v.skipped_user_modified.length} 项已被修改，未覆盖`:''}。可以重新查看改动后再应用。`;b.remove();await refresh();}catch(err){p.textContent=`撤回失败：${err.message}`;b.disabled=false;}};}}}
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
      if(!repo&&projectId){root.innerHTML='<section class="workspace-empty"><h1>找不到这个项目</h1><p>它可能已被移除，或链接已过期。</p><button data-back class="primary">返回项目列表</button></section>';q('[data-back]').onclick=()=>{location.hash='#/projects';};return;}
      if(!repo){root.classList.add('workspace-start');root.innerHTML=`<section class="workspace-welcome"><span class="welcome-icon">${icon('folder')}</span><h1>为你的项目配好 AI</h1><p>从一个本机文件夹开始。</p><button data-add-project class="primary">${icon('plus')}添加第一个项目</button><ol class="welcome-steps"><li><span>1</span>选择项目文件夹</li><li><span>2</span>添加需要的能力</li><li><span>3</span>预览并应用</li></ol><a href="#/onboarding">查看上手指南</a></section>`;q('[data-add-project]').onclick=addProject;return;}
      root.classList.remove('workspace-start');
      repos=[repo];
      back=back?.repoId===repo.repo_id?back:recent?.repoId===repo.repo_id?recent:null;
      wtId=back?.repoId===repo.repo_id&&repo.worktrees[back.wt]?back.wt:primaryWorktree(repo);
      relative=back?.repoId===repo.repo_id?back.dir||'':'';
      nodeKind=back?.repoId===repo.repo_id?(back.node||(relative?'directory':isGit()?'worktree':'project')):isGit()?'worktree':'project';
      if(!wtId&&!settingsView)throw Error('这个项目没有可用目录。');
      if(!wtId){relative='';nodeKind='project';}
      shell();renderTree();await loadFolders(repos,true);await activate(relative,wtId,nodeKind);
      let pending;try{pending=JSON.parse(sessionStorage.getItem('ailoom-pick-resource'));}catch{}
      if(!disposed&&effective&&pending?.repoId===repo.repo_id){sessionStorage.removeItem('ailoom-pick-resource');const entry=catalog.entries?.find(e=>e.id===pending.id);if(entry)addCapabilities(entry.kind,entry.id);}
      if(!disposed&&pendingEnvironmentProject===repo.repo_id){pendingEnvironmentProject=null;addEnvironment();}
    }catch(e){if(!disposed)root.innerHTML=`<section class="workspace-empty"><h1>暂时无法打开目录</h1><p>${esc(e.message)}</p><a href="#/projects">返回我的目录</a></section>`;}
  }
  init();
  return {isDirty:()=>localPanel?.isDirty()||false,destroy(){localPanel?.destroy();disposed=true;controller.abort();dialogs.forEach(d=>d.destroy());setTarget(null);root.remove();}};
}
