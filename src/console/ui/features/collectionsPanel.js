// 资源来源管理：导入入口、真实更新检查、版本确认、引用影响和来源移除。
import { api, esc } from '../services/api.js';
import { setTarget, currentTarget } from '../state/target.js';
import { Dialog, confirmAction } from '../components/dialog.js';
import { CcSwitchImport } from './ccSwitchImport.js';
import { ResourceReferences } from './resourceReferences.js';
import { ImportDialog } from './importDialog.js';
import { GitSkillDialog } from './gitSkillDialog.js';
import { SourceBindingDialog } from './sourceBindingDialog.js';
import { PageHeader } from '../components/layout.js';
import { Item, resourcePath, resourceIcon, MoreActions } from '../components/item.js';

// One visual vocabulary for every capability type in the library.
const capabilityTypes = {
  skill: ['技能', '<path d="m12 3 3 6 6 3-6 3-3 6-3-6-6-3 6-3z"/>'],
  mcp: ['工具连接（MCP）', '<path d="M8 3v5m8-5v5M6 8h12v4a6 6 0 0 1-12 0zM12 18v3"/>'],
  agent: ['子代理', '<rect x="4" y="7" width="16" height="13" rx="3"/><path d="M12 3v4M8 12h1m6 0h1M9 16h6"/>'],
  rule: ['规则', '<path d="M9 5h11M9 12h11M9 19h11M3 5h1M3 12h1M3 19h1"/>'],
  doc: ['文档', '<path d="M5 3h10l4 4v14H5zM14 3v5h5M8 12h8M8 16h8"/>'],
  learning: ['经验', '<path d="M4 4h6l2 2 2-2h6v15h-6l-2 2-2-2H4zM12 6v15"/>'],
  env: ['环境', '<path d="M4 6h16M4 12h16M4 18h16M8 4v4M16 10v4M10 16v4"/>'],
  hook: ['Hook', '<path d="M8 3v11a5 5 0 0 0 10 0V9l-3 3M5 3h6"/>'],
  package: ['依赖包', '<path d="m12 3 9 5v9l-9 5-9-5V8zM3 8l9 5 9-5M12 13v9M7 5l10 6"/>'],
};
function capabilityBadge(kind) {
  const [label, drawing] = capabilityTypes[kind] || [kind, '<rect x="4" y="4" width="16" height="16" rx="3"/>'];
  return `<span class="resource-kind" title="${esc(label)}"><svg viewBox="0 0 24 24" aria-hidden="true">${drawing}</svg><span>${esc(label.split(' · ')[0])}</span></span>`;
}

// Group transport aliases, but never collapse distinct hosts or local paths.
export function repositoryGroups(sources) {
  const groups = new Map();
  for (const source of sources) {
    const raw = source.repository_url || source.url || '';
    let key = raw, title = source.name, host = '本地来源';
    if((raw.startsWith('/')||raw.startsWith('file://'))&&title===raw)title=raw.split('/').filter(Boolean).pop()||raw;
    try {
      const url = new URL(raw.replace(/^git@([^:]+):/, 'ssh://git@$1/'));
      if (['https:', 'http:', 'ssh:'].includes(url.protocol)) {
        const path = url.pathname.replace(/\/$/, '').replace(/\.git$/, '').replace(/^\//, '');
        host = url.host.toLowerCase();
        key = host + '/' + path;
        title = path || host;
      }
    } catch { /* Local paths keep their exact identity. */ }
    key=source.repository_key||key;
    if (!groups.has(key)) groups.set(key, {key, title, host, sources:[]});
    groups.get(key).sources.push(source);
  }
  return [...groups.values()];
}

export function catalogGroups(sources, entries) {
  const groups=repositoryGroups(sources).map(g=>({...g,entries:[],url:g.sources[0].connection_url||g.sources[0].url,ref:('update_ref' in g.sources[0]?g.sources[0].update_ref:g.sources[0].connection?g.sources[0].connection.ref:g.sources[0].lock?.ref_||g.sources[0].lock?.ref)}));
  const byKey=new Map(groups.map(g=>[g.key,g]));
  const unique=new Map();
  for(const group of groups)for(const source of group.sources)for(const entry of source.resources||[])
    unique.set(entry.id,{...entry,groupKey:group.key,sourceId:source.id,updateState:source.update?.state});
  for(const entry of entries) {
    const url=entry.connection_url||entry.source?.repo_url;
    const origin=url?repositoryGroups([{url:entry.repository_url||url,name:url,repository_key:entry.repository_key}])[0]:{key:'local:personal',title:'本地与个人资源',host:'本地'};
    if(!byKey.has(origin.key)){const group={...origin,sources:[],entries:[],url,ref:entry.source?.connection?entry.source.connection.ref:entry.source?.ref||entry.source?.ref_};byKey.set(origin.key,group);groups.push(group);}
    unique.set(entry.id,{...entry,groupKey:origin.key,personal:true});
  }
  for(const entry of unique.values())byKey.get(entry.groupKey).entries.push(entry);
  const compare=(a,b)=>String(a).localeCompare(String(b),'zh-CN',{numeric:true});
  for(const group of groups)group.entries.sort((a,b)=>compare(a.name||a.id,b.name||b.id));
  return groups.sort((a,b)=>(a.key==='local:personal')-(b.key==='local:personal')||compare(a.title,b.title)||compare(a.host,b.host));
}

export function CollectionsPanel(container, options = {}) {
  const root = document.createElement('section');
  container.appendChild(root);
  let alive = true, busy = false, checking = false, stopCheck = false, checkProgress = null, sources = [];
  let libraryEntries = [], selectedRepository = ''; // Personal copies may also have a Git origin.
  let referenceDialog = null, resourceDialog = null, updateDialog = null;
  root.innerHTML = `
    ${PageHeader({title:options.title||'资源库',description:options.description||'按来源管理与搜索',actions:`${options.onCreate?'<button data-create data-variant="ghost" title="新建规则 / 子代理">新建</button>':''}${MoreActions('导入资源','<button data-git-add>GitHub 或 Git 仓库</button><button data-add-entry>skills.sh</button><button data-add>本地文件夹</button>','导入资源')}`})}
    ${options.compactLibrary?'<div class="library-layout"><aside class="library-repository-nav" aria-label="仓库与合集"><h3>仓库与合集</h3><input data-repository-search type="search" aria-label="查找仓库或合集" placeholder="查找仓库 / 合集…"><div data-repository-filters></div><button class="item-surface" data-manage-sources>管理来源</button></aside><div class="library-catalog-pane"><div class="library-catalog-heading"><div><h2>全部资源</h2><small data-library-count class="sr-only" role="status" aria-live="polite"></small></div><button class="library-mobile-source" data-select-source aria-expanded="false">选择来源</button></div>':''}
    <div class="toolbar${options.compactLibrary?' library-scope':''}"><input data-search type="search" aria-label="搜索来源或资源" placeholder="搜索资源名称或用途…">${options.compactLibrary?'<button data-library-clear hidden>清除筛选</button>':''}<div class="actions library-update-actions"><button data-check>检查全部更新</button><button data-update disabled>更新可用版本</button></div></div>
    <p data-msg class="inline-status muted" role="status" aria-live="polite"></p>
    <section class="library-check-progress" data-check-progress hidden aria-label="更新检查进度"></section>
    <div data-sources><p class="muted">正在读取资源库…</p></div>
    <details class="library-maintenance"><summary>迁移已有资源</summary><div class="actions"><button data-cc-switch>从 CC Switch 迁移</button></div></details>${options.compactLibrary?'</div></div>':''}`;
  const q = s => root.querySelector(s);
  const bindingDialog=SourceBindingDialog(root,{onChanged:async()=>{changed();await refresh();}});

  if(options.onCreate)q('[data-create]').onclick=options.onCreate;
  if(options.compactLibrary) {
    const searchRow=q('.toolbar');
    const importAction=q('.page-head .action-menu-trigger');
    const importMenu=importAction.nextElementSibling;
    searchRow.append(importAction,importMenu);
    importAction.textContent='导入';
    if(options.onCreate)importMenu.append(q('[data-create]'));
    importMenu.append(q('[data-cc-switch]'));
    importMenu.insertAdjacentHTML('beforeend','<a href="#/library/rules">本地规则与子代理</a>');
    q('.library-maintenance').remove();
    q('.page-head .actions').remove();
    q('[data-search]').placeholder='搜索名称、用途或来源';
    const heading=q('.library-catalog-heading');
    heading.querySelector('div').className='sr-only';heading.hidden=true;
    // One toolbar row: search, source, type, list options, manage sources, import.
    const filters=searchRow;
    filters.insertBefore(q('[data-select-source]'),importAction);
    q('[data-select-source]').textContent='全部来源';
    const list=document.createElement('div');list.dataset.capabilityCatalog='';
    const sourceList=q('[data-sources]');sourceList.before(list);sourceList.hidden=true;
    const filter=document.createElement('select');filter.dataset.libraryKind='';filter.setAttribute('aria-label','能力类型');
    filter.innerHTML='<option value="">全部类型</option>'+Object.entries(capabilityTypes).map(([kind,[label]])=>`<option value="${kind}">${label}</option>`).join('');
    filters.insertBefore(filter,importAction);filter.onchange=()=>{renderSources();};
    const sort=document.createElement('select');sort.dataset.librarySort='';sort.setAttribute('aria-label','资源排序');sort.innerHTML='<option value="repository">按 Git 仓库</option><option value="name">按名称</option>';
    sort.value='name';
    const listOptions=document.createElement('details');listOptions.className='library-list-options';const summary=document.createElement('summary');summary.textContent='⋯';summary.setAttribute('aria-label','列表选项');const controls=document.createElement('div');controls.className='library-list-controls';controls.append(sort,q('.library-update-actions'));listOptions.append(summary,controls);
    const manage=document.createElement('button');manage.dataset.manageCatalogSources='';manage.dataset.variant='ghost';manage.textContent='管理来源';
    for(const el of [q('[data-library-clear]'),listOptions,manage])filters.insertBefore(el,importAction);sort.onchange=()=>{renderSources();};
    q('[data-select-source]').onclick=()=>{const open=q('.library-layout').classList.toggle('sources-open');q('[data-select-source]').setAttribute('aria-expanded',String(open));};
    q('[data-repository-search]').oninput=renderSources;
    q('[data-repository-filters]').onclick=event=>{
      const button=event.target.closest('[data-repository-filter]');
      if(!button||busy)return;
      selectedRepository=button.dataset.repositoryFilter;renderSources();root.querySelector('.library-layout')?.classList.remove('sources-open');q('[data-select-source]').setAttribute('aria-expanded','false');
      const switcher=q('[data-select-source]');
      if(switcher.getClientRects?.().length)switcher.focus();else q('[data-repository-filters]').querySelectorAll('[data-repository-filter]').forEach(b=>{if(b.dataset.repositoryFilter===selectedRepository)b.focus();});
    };
    q('[data-library-clear]').onclick=clearFilters;
  }

  const migrationDialog = CcSwitchImport(root, {onChanged:async () => { changed(); await refresh(); }});
  q('[data-cc-switch]').onclick = () => migrationDialog.show();
  // AIL-113：导入业务抽到可复用 ImportDialog（项目内「＋导入」共用同一实现）。
  const importDialog = ImportDialog(root, { onChanged: async () => { changed(); await refresh(); } });
  const gitDialog = GitSkillDialog(root, {onChanged:async()=>{changed();await refresh();}});
  q('[data-git-add]').onclick=()=>gitDialog.show();
  q('[data-manage-sources]')?.addEventListener('click',()=>showSources(sources));
  q('[data-manage-catalog-sources]')?.addEventListener('click',()=>showSources(sources));
  function changed() { const t = currentTarget(); if (t) setTarget(t); options.onChanged?.(); }
  function message(text) { if (alive) q('[data-msg]').textContent = text; }
  async function run(fn) {
    if (busy || checking) return;
    busy = true; root.setAttribute('aria-busy', 'true');
    root.querySelectorAll('button').forEach(b => { b.disabled = true; });
    root.querySelectorAll('.toolbar input,.toolbar select,[data-repository-search]').forEach(e=>{e.disabled=true;});
    try { await fn(); } catch (e) { message(e.message); }
    finally { busy = false; if (alive) { root.removeAttribute('aria-busy'); root.querySelectorAll('button').forEach(b => { b.disabled = false; }); root.querySelectorAll('.toolbar input,.toolbar select,[data-repository-search]').forEach(e=>{e.disabled=false;}); renderSources(); } }
  }
  q('[data-add]').onclick = () => importDialog.show('local');
  q('[data-add-entry]').onclick = () => importDialog.show('entry');
  function checkableSkills() { return libraryEntries.filter(e => e.can_check_update); }
  function filtersActive() {
    return options.compactLibrary&&!!(q('[data-search]').value.trim()||q('[data-library-kind]').value||selectedRepository);
  }
  function clearFilters() {
    q('[data-search]').value='';
    q('[data-library-kind]').value='';q('[data-repository-search]').value='';selectedRepository='';
    q('[data-search]').dispatchEvent(new Event('input',{bubbles:true}));
  }
  function scopedGroups(catalog=catalogGroups(sources,libraryEntries)) {
    const kind=q('[data-library-kind]').value;
    const search=q('[data-search]').value.trim().toLowerCase();
    return catalog.filter(g=>!selectedRepository||g.key===selectedRepository).map(g=>{
      const groupMatch=!search||JSON.stringify([g.title,g.host,g.url,...g.sources.map(s=>s.name)]).toLowerCase().includes(search);
      const entries=g.entries.filter(e=>(!kind||e.kind===kind)&&
        (groupMatch||JSON.stringify([e.name,e.description,e.kind,e.source?.repo_path,e.path]).toLowerCase().includes(search)));
      const matchedSources=g.sources.filter(s=>entries.some(e=>e.sourceId===s.id)||(!kind&&groupMatch));
      return {...g,entries,sources:matchedSources};
    }).filter(g=>g.entries.length||g.sources.length);
  }
  function updateScope() {
    if(!options.compactLibrary)return {sources,skills:checkableSkills()};
    const groups=scopedGroups();
    return {sources:groups.flatMap(g=>g.sources),skills:groups.flatMap(g=>g.entries).filter(e=>e.personal&&e.can_check_update)};
  }
  function syncUpdateControls() {
    const scope=updateScope();
    q('[data-update]').disabled = busy || checking || (!scope.sources.some(s=>s.update?.state==='available'&&s.update.preview?.preview_id)&&!scope.skills.some(e=>e.update?.state==='upstream-new'&&e.update.preview_id));
    q('[data-check]').disabled = busy || checking || !(scope.sources.some(s=>s.management!=='external'&&s.can_update!==false) || scope.skills.length);
    q('[data-check]').textContent=filtersActive()?'检查筛选项':'检查全部更新';
    q('[data-update]').textContent=filtersActive()?'更新筛选项':'更新可用版本';
  }
  const UPDATE_LABELS = {'up-to-date':'已是最新','upstream-new':'可更新','local-modified':'本地已修改',conflict:'本地与上游冲突','upstream-missing':'上游已移除',stale:'请重新检查',error:'检查失败'};
  function syncMutationControls() {
    root.querySelectorAll('[data-group-check],[data-check-one],[data-skill-check],[data-group-update],[data-update-one],[data-skill-update],[data-group-install],[data-remove],[data-git-add],[data-add-entry],[data-add],[data-cc-switch],[data-create],[data-first],[data-refresh-external],[data-catalog-binding],[data-catalog-edit]').forEach(b=>{b.disabled=busy||checking;});
    syncUpdateControls();
  }
  function renderCheckProgress() {
    if(!alive||!checkProgress)return;
    const box=q('[data-check-progress]'),open=box.querySelector('details')?.open;
    const p=checkProgress;box.hidden=false;
    const current=checking&&p.refreshing?'正在读取检查结果…':checking?(stopCheck?'正在完成当前项，随后停止：':'正在检查：')+(p.entries.find(e=>e.status==='正在检查')?.name||'准备开始'):p.stopped?'检查已停止':'检查完成';
    box.innerHTML=`<p data-check-current role="status" aria-live="polite"><strong>${esc(current)}</strong></p><progress data-check-bar max="${p.entries.length}" value="${p.done}" aria-label="已完成 ${p.done} 项，共 ${p.entries.length} 项"></progress><div class="tab-actions"><p data-check-count>已完成 ${p.done} / ${p.entries.length} 项 · 可更新 ${p.available} 项 · 失败 ${p.failed} 项</p>${checking?`<button data-check-stop ${stopCheck?'disabled':''}>${stopCheck?'等待当前项结束':'停止检查'}</button>`:''}</div><details ${open||p.failed?'open':''}><summary>检查明细</summary><ul class="library-update-list">${p.entries.map(e=>`<li><strong>${esc(e.name)}</strong><p class="${e.status==='失败'?'field-error':'muted'}">${esc(e.status)}${e.error?'：'+esc(e.error):''}</p></li>`).join('')}</ul></details>`;
    box.querySelector('[data-check-stop]')?.addEventListener('click',()=>{stopCheck=true;renderCheckProgress();});
  }
  async function checkUpdates(collections, skills) {
    if(busy||checking||!alive)return;
    const entries=[...collections.filter(s=>s.management!=='external'&&s.can_update!==false).map(s=>({name:'合集 · '+s.name,source:s})),...skills.map(e=>({name:'Skill · '+e.name,skill:e}))].map(e=>({...e,status:'等待检查'}));
    if(!entries.length)return;
    checking=true;stopCheck=false;checkProgress={entries,done:0,available:0,failed:0,stopped:false};
    syncMutationControls();renderCheckProgress();message('正在检查更新，可继续搜索、筛选和查看资源。');
    try {
      for(const entry of entries) {
        if(stopCheck||!alive)break;
        entry.status='正在检查';renderCheckProgress();
        try {
          const result=entry.source?await api.collectionCheck(entry.source.id):await api.checkUpdate(entry.skill.name);
          const statuses=entry.source?result.items:[result.status];
          const failed=statuses.some(s=>s.state==='error'),available=statuses.some(s=>['available','upstream-new'].includes(s.state));
          entry.status=failed?'失败':available?'可更新':entry.source?'已是最新':UPDATE_LABELS[result.status.state]||'检查完成';
          entry.error=statuses.filter(s=>s.state==='error').map(s=>s.error||s.note||'检查失败').join('；');
          if(failed)checkProgress.failed++;
          if(available)checkProgress.available++;
        } catch(e) {entry.status='失败';entry.error=e.message;checkProgress.failed++;}
        checkProgress.done++;renderCheckProgress();
      }
    } finally {
      checkProgress.refreshing=true;checkProgress.stopped=checkProgress.done<entries.length;
      for(const entry of entries)if(entry.status==='等待检查')entry.status='未检查';
      renderCheckProgress();
      if(alive) {
        try{await refresh();message(`${checkProgress.stopped?'检查已停止':'检查完成'}：${checkProgress.done} / ${entries.length} 项，可更新 ${checkProgress.available} 项，失败 ${checkProgress.failed} 项。`);}
        catch(e){message('检查结果暂时无法刷新：'+e.message);}
        checking=false;renderCheckProgress();syncMutationControls();
      } else checking=false;
    }
  }
  const updateLabel = entry => UPDATE_LABELS[entry.update?.state] || '尚未检查更新';
  function skillControls(entry, check = true) {
    if (!entry.can_check_update) return '';
    return `<span class="badge">${esc(updateLabel(entry))}</span>${check?`<button data-skill-check="${esc(entry.name)}">检查更新</button>`:''}${entry.update?.state === 'upstream-new' && entry.update.preview_id ? `<button data-skill-update="${esc(entry.name)}">更新</button>` : ''}`;
  }
  function bindSkillUpdates() {
    root.querySelectorAll('[data-catalog-binding]').forEach(b=>{b.onclick=()=>{const e=libraryEntries.find(e=>e.id===b.dataset.catalogBinding);if(e)bindingDialog.show({kind:'skill',id:e.id},e.source);};});

    root.querySelectorAll('[data-skill-check]').forEach(b => { b.onclick = () => checkUpdates([], [libraryEntries.find(e => e.name === b.dataset.skillCheck)]); });
    root.querySelectorAll('[data-skill-update]').forEach(b => { b.onclick = () => run(() => update([], [libraryEntries.find(e => e.name === b.dataset.skillUpdate)])); });
  }
  async function update(items, skills = []) {
    const state=await api.state().catch(e=>({repos:[],error:e.message}));
    if(!alive)return;
    const names=new Map([...sources.flatMap(s=>s.resources||[]),...libraryEntries].map(e=>[e.id,e.name]));
    const resourceName=id=>names.get(id)||id.split('/').slice(3).join('/')||id.split('/').pop();
    const refs=[...new Map([...items,...skills].flatMap(s=>s.references||[]).map(r=>[JSON.stringify([r.repo_id,r.scope,r.resource_id]),r])).values()];
    const removed=items.flatMap(s=>(s.update.preview.removed||[]).map(id=>({id,source:s.name,used:refs.some(r=>r.resource_id===id)})));
    const blocked=removed.some(r=>r.used),unknownUsage=skills.filter(e=>e.references_error);
    const origin=entry=>{
      const g=repositoryGroups([{...entry.source,...entry,name:entry.name,url:entry.source?.repo_url||entry.url}])[0];
      return g.host==='本地来源'?g.title:g.host+'/'+g.title;
    };
    const version=value=>value?`<code title="${esc(value)}">${esc(value.slice(0,12))}</code>`:'未记录';
    const locations=new Map();
    for(const r of refs) {
      const key=JSON.stringify([r.repo_id,r.scope]);
      if(!locations.has(key))locations.set(key,{...r,resources:[]});
      locations.get(key).resources.push(resourceName(r.resource_id));
    }
    const locationRows=[...locations.values()].map(location=>{
      const repo=(state.repos||[]).find(r=>r.repo_id===location.repo_id);
      let scope=location.scope;
      const wt=Object.entries(repo?.worktrees||{}).sort((a,b)=>b[0].length-a[0].length).find(([id])=>scope==='Worktree '+id||scope.startsWith('Worktree '+id+'/'));
      if(wt)scope='Worktree '+(wt[1].branch?.replace(/^refs\/heads\//,'')||wt[1].path.split('/').pop())+scope.slice(('Worktree '+wt[0]).length);
      const name=location.repo_id?(repo?.project?.name||repo?.common_dir?.replace(/\/\.git$/,'').split('/').pop()||'项目 '+location.repo_id):'跨项目配置';
      return `<li><strong>${esc(name)}</strong><p class="muted">${esc(scope)}</p><p>${location.resources.map(esc).join('、')}</p></li>`;
    }).join('');
    const content=document.createElement('div');content.className='library-update-preview';
    content.innerHTML=`<p data-update-summary>将更新 ${[items.length?items.length+' 个合集':'',skills.length?skills.length+' 个独立 Skill':''].filter(Boolean).join('和')}。</p>
      ${items.length?`<section><h3>合集（${items.length}）</h3><p class="muted">每个合集整体更新，包含筛选结果以外的资源。</p><ul class="library-update-list">${items.map(s=>`<li data-update-collection="${esc(s.id)}"><strong>${esc(s.name)}</strong><p class="muted">${esc(origin(s))}</p><p>${version(s.lock?.resolved_commit||s.update.base_commit)} → ${version(s.update.preview.source?.lock?.resolved_commit)}</p><p>${(s.resources||[]).length} 项资源 → ${s.update.preview.resources?.length??'数量未记录'}${s.update.preview.resources?' 项资源':''}</p></li>`).join('')}</ul></section>`:''}
      ${skills.length?`<section><h3>独立 Skill（${skills.length}）</h3><ul class="library-update-list">${skills.map(e=>`<li data-update-skill="${esc(e.id)}"><strong>${esc(e.name)}</strong><p class="muted">${esc(origin(e))}${e.source?.repo_path?' · '+esc(e.source.repo_path):''}</p><p>${version(e.source?.resolved_commit)} → ${version(e.update.upstream_commit)}</p></li>`).join('')}</ul></section>`:''}
      ${removed.length?`<section><h3>上游移除（${removed.length}）</h3><ul class="library-update-list">${removed.map(r=>`<li data-update-removed><strong>${esc(resourceName(r.id))}</strong><p class="muted">来自 ${esc(r.source)}</p><p>${r.used?'仍有启用引用，需先停用或替换。':'更新后将从资源库的当前版本移除，旧版本保留。'}</p></li>`).join('')}</ul></section>`:''}
      <section><h3>${unknownUsage.length?'已知启用位置':'已启用位置'}（${locations.size} 处）</h3>${refs.length?`<p class="muted">共 ${refs.length} 条资源引用；同一位置可能启用多个资源。</p><ul class="library-update-list">${locationRows}</ul>`:'<p>已读取的记录中没有启用引用。</p>'}${unknownUsage.length?`<p class="field-error">以下 Skill 的使用位置未能读取：${unknownUsage.map(e=>esc(e.name)+'：'+esc(e.references_error)).join('；')}</p>`:''}${state.error?`<p class="field-error">项目名称暂时无法读取：${esc(state.error)}</p>`:''}</section>
      ${blocked?'<p class="field-error" data-update-blocked>上游移除的资源仍在使用，请先到上述位置停用或替换，再重新检查更新。</p>':'<p class="muted">本次更新资源库并保留旧版本。完成后，请到项目或跨项目配置预览并应用改动。</p>'}`;
    const confirmed=new Promise(resolve=>{
      updateDialog=Dialog(root,{title:'更新资源库',content,actions:[{label:blocked?'关闭':'取消',returnValue:false},...(blocked?[]:[{label:'确认更新',variant:'default',returnValue:true}])],onClose:value=>resolve(value===true)});
    });
    const proceed=await confirmed;updateDialog=null;
    if(!proceed||!alive)return;
    const failures = [];
    try {
      if (items.length) {
        try { await api.collectionUpdate(items.map(s => s.update.preview.preview_id)); }
        catch (e) { failures.push('来源合集：' + e.message); }
      }
      for (const entry of skills) {
        try { await api.updateSkill(entry.name, true, entry.update.preview_id); }
        catch (e) { failures.push(entry.name + '：' + e.message); }
      }
    } finally { changed(); await refresh(); }
    message(failures.length ? '部分更新未完成：' + failures.join('；') + '。请重新检查；已成功的更新请到项目或跨项目配置应用。' : '资源库已更新。请到项目或跨项目配置应用改动。');
  }
  q('[data-check]').onclick = () => {const scope=updateScope();return checkUpdates(scope.sources,scope.skills);};
  q('[data-update]').onclick = () => run(() => {const scope=updateScope();return update(scope.sources.filter(s=>s.update?.state==='available'&&s.update.preview?.preview_id),scope.skills.filter(e=>e.update?.state==='upstream-new'&&e.update.preview_id));});
  // AIL-126：跨页往返（如引用 Dialog「前往管理」后返回）恢复搜索上下文。
  if (options.searchKey) {
    try { q('[data-search]').value = sessionStorage.getItem(options.searchKey) || ''; } catch { /* 隐私模式忽略 */ }
    q('[data-search]').addEventListener('input', () => { try { sessionStorage.setItem(options.searchKey, q('[data-search]').value); } catch { /* 忽略 */ } });
  }
  q('[data-search]').oninput = ()=>{renderSources();};
  function groupUpdates(group) {
    return {sources:group.sources.filter(s=>s.update?.state==='available'&&s.update.preview?.preview_id),
      skills:group.entries.filter(e=>e.personal&&e.can_check_update&&e.update?.state==='upstream-new'&&e.update.preview_id)};
  }
  function checkGroup(group) {
    return checkUpdates(group.sources,group.entries.filter(e=>e.personal&&e.can_check_update));
  }
  function renderSources() {
    if (!alive) return;
    const search = q('[data-search]').value.toLowerCase();
    const groups = repositoryGroups(sources);
    const shown = groups.map(g => ({...g, sources:g.sources.map(s => ({...s, visibleResources:!search || (g.title + ' ' + s.url).toLowerCase().includes(search) ? s.resources : s.resources.filter(r => JSON.stringify([r.name,r.description,r.path,r.kind]).toLowerCase().includes(search))})).filter(s => !search || s.visibleResources.length || (g.title + ' ' + s.url).toLowerCase().includes(search))})).filter(g => g.sources.length);
    // AIL-126：本地资源（无远端）单独一组；空状态基于全部可见资源，
    // 已有本地资源时不再显示整个中心为空。
    const localEntries = libraryEntries.filter(e => !search || JSON.stringify([e.name, e.id, e.description, e.kind]).toLowerCase().includes(search));
    const nothingVisible = !shown.length && !localEntries.length;
    if(options.compactLibrary) {
      const catalog=catalogGroups(sources,libraryEntries);
      const byGroup=new Map(catalog.map(g=>[g.key,g]));
      const unique=new Map(catalog.flatMap(g=>g.entries.map(e=>[e.id,e])));
      // 不同托管平台上的同名仓库（如 GitHub 与 GitLab 的 owner/repo）在标签里带上平台名以便区分。
      const titleCount=catalog.reduce((m,g)=>m.set(g.title,(m.get(g.title)||0)+1),new Map());
      const label=g=>titleCount.get(g.title)>1&&g.host?`${g.host} · ${g.title}`:g.title;
      if(!catalog.some(g=>g.key===selectedRepository))selectedRepository='';
      const repositorySearch=q('[data-repository-search]').value.trim().toLowerCase();
      const navButton=(key,title,count,host='')=>`<button data-repository-filter="${esc(key)}" class="item-surface" aria-pressed="${selectedRepository===key}" ${busy?'disabled':''}><span><strong>${esc(title)}</strong>${host?`<small>${esc(host)}</small>`:''}</span><span class="badge">${count}</span></button>`;
      const navGroups=catalog.filter(g=>g.key===selectedRepository||!repositorySearch||JSON.stringify([g.title,g.host,...g.sources.map(s=>s.name)]).toLowerCase().includes(repositorySearch));
      q('[data-repository-filters]').innerHTML=navButton('','全部资源',catalog.reduce((n,g)=>n+g.entries.length,0))+navGroups.map(g=>navButton(g.key,g.title,g.entries.length,g.host)).join('')+(navGroups.length?'':'<p class="muted">没有匹配的仓库或合集。</p>');
      syncUpdateControls();
      const scoped=scopedGroups(catalog),scopeByGroup=new Map(scoped.map(g=>[g.key,g]));
      const allVisible=scoped.map(g=>({...g,visible:g.entries})).filter(g=>g.entries.length||g.sources.some(s=>s.error));
      const allEntries=allVisible.flatMap(g=>g.visible),byName=q('[data-library-sort]').value==='name';
      const visible=allVisible;
      const entries=byName?[...allEntries].sort((a,b)=>String(a.name).localeCompare(String(b.name),'zh-CN',{numeric:true})):allEntries;
      q('[data-library-count]').textContent=filtersActive()?`${allEntries.length} 项匹配 / ${catalog.reduce((n,g)=>n+g.entries.length,0)} 项能力`:`${allEntries.length} 项能力`;
      q('.library-catalog-heading h2').textContent=selectedRepository?label(byGroup.get(selectedRepository)):'全部资源';
      q('[data-select-source]').textContent=selectedRepository?label(byGroup.get(selectedRepository)):'全部来源';
      q('[data-library-clear]').hidden=!filtersActive();q('[data-library-clear]').disabled=busy;
      const hasResources=libraryEntries.length||sources.some(s=>s.resources?.length);
      const row=e=>Item({title:e.name||e.id,primaryAction:{attributes:`data-catalog-view="${esc(e.id)}"`,label:'查看资源 '+(e.name||e.id)},description:e.description,path:resourcePath(e),leading:resourceIcon,meta:[e.kind!=='skill'?(capabilityTypes[e.kind]?.[0]||e.kind):'',allEntries.some(other=>other.id!==e.id&&other.name===e.name)?label(byGroup.get(e.groupKey)):'',e.can_check_update&&e.update?.state?updateLabel(e):''].filter(Boolean).join('，'),tag:'li',className:'repository-resource',attributes:`data-library-id="${esc(e.id)}"`,
        // An available update keeps its button next to the status; everything else lives in the detail dialog.
        actions:`${e.update?.state==='upstream-new'&&e.update.preview_id?`<button data-variant="ghost" data-skill-update="${esc(e.name)}" ${busy?'disabled':''}>更新</button>`:''}<button class="library-row-use" data-variant="ghost" data-catalog-use="${esc(e.id)}" ${busy?'disabled':''}>选用</button>`});
      const controls=g=>{
        const updates=groupUpdates(g),count=updates.sources.length+updates.skills.length;
        const checkable=g.sources.some(s=>s.management!=='external'&&s.can_update!==false)||g.entries.some(e=>e.personal&&e.can_check_update);
        if(!checkable&&!g.url&&!g.sources.length)return '';
        return `<div class="actions">${checkable?`<button data-group-check="${esc(g.key)}" ${busy?'disabled':''}>检查更新</button><button data-group-update="${esc(g.key)}" ${busy||!count?'disabled':''}>更新${count?`（${count}）`:''}</button>`:''}${g.url?`<button data-group-install="${esc(g.key)}" ${busy?'disabled':''}>安装其他 Skills</button>`:''}${g.sources.length?`<button data-catalog-source="${esc(g.key)}" ${busy?'disabled':''}>来源详情${g.sources.length>1?`（${g.sources.length}）`:''}</button>`:''}</div>`;
      };
      q('[data-capability-catalog]').innerHTML=visible.length?(byName?`<ul class="repository-resources">${entries.sort((a,b)=>String(a.name).localeCompare(String(b.name),'zh-CN',{numeric:true})).map(row).join('')}</ul>`:
        visible.map(g=>`<section class="library-repository" data-repository="${esc(g.key)}"><header class="library-repository-heading"><div><h3>${esc(label(g))}</h3><span class="muted">${g.visible.length} 项</span></div>${controls(g)?MoreActions('来源操作',controls(g)):''}</header>${g.sources.filter(s=>s.error).map(s=>`<p class="field-error">${esc(s.error)}</p>`).join('')}<ul class="repository-resources">${g.visible.map(row).join('')}</ul></section>`).join('')):
        `<div class="empty-state"><h2>${hasResources?'没有匹配的资源':'把常用的 AI 能力放在这里'}</h2>${hasResources?'<button data-clear-filters>清除筛选</button>':'<p>技能、工具连接、规则、子代理</p><button data-first class="primary">安装第一个 Skill</button>'}</div>`;
      root.querySelectorAll('[data-catalog-use]').forEach(b=>{b.onclick=async()=>{
        const entry=unique.get(b.dataset.catalogUse),body=document.createElement('div');
        body.innerHTML='<p role="status">正在读取项目…</p>';
        const modal=Dialog(root,{title:'选用 '+(entry.name||entry.id),content:body});
        try {
          const state=await api.state();if(!alive||!modal.isOpen)return;
          body.innerHTML=(state.repos||[]).map(repo=>Item({title:repo.project?.name||repo.repo_id,primaryAction:{href:'#/projects/'+encodeURIComponent(repo.repo_id),attributes:`data-use-project="${esc(repo.repo_id)}"`,label:'在此项目选用'},description:repo.repo_id.startsWith('nongit-')?'文件夹项目':'项目默认'})).join('')||'<p>先添加一个项目，再选择需要的能力。</p><a href="#/projects" data-use-empty>添加项目</a>';
          body.querySelectorAll('[data-use-project]').forEach(a=>{a.onclick=()=>{sessionStorage.setItem('ailoom-pick-resource',JSON.stringify({id:entry.id,repoId:a.dataset.useProject}));sessionStorage.setItem('ailoom-directory-view',JSON.stringify({repoId:a.dataset.useProject,node:'project'}));sessionStorage.removeItem('ailoom-return-after-settings');modal.close();};});
          body.querySelector('[data-use-empty]')?.addEventListener('click',()=>modal.close());
        }catch(e){if(modal.isOpen)body.textContent=e.message;}
      };});
      root.querySelectorAll('[data-catalog-view]').forEach(b=>{b.onclick=()=>{
        const entry=unique.get(b.dataset.catalogView),group=byGroup.get(entry.groupKey),content=document.createElement('div');
        content.innerHTML=`<p>${esc(entry.description||'暂无说明')}</p><p class="muted">${esc(capabilityTypes[entry.kind]?.[0]||entry.kind)}，来自${esc(label(group))}</p>${entry.can_check_update?`<p class="muted">${esc(updateLabel(entry))}</p>`:''}<details><summary>来源与位置</summary><p class="path">${esc(entry.path||entry.id)}</p>${group.url?`<p class="path">${esc(group.url)}</p>`:''}</details>`;
        // Low-frequency actions are visible buttons here, not a menu on every row.
        const then=fn=>()=>{resourceDialog.close();fn();return false;};
        const actions=[{label:'关闭'}];
        if(entry.personal&&options.onEdit&&!checking)actions.push({label:'编辑',onAction:then(()=>options.onEdit(entry.id))});
        if(entry.personal&&entry.kind==='skill'&&entry.source&&!checking)actions.push({label:entry.source.source_kind==='local'&&!entry.source.repo_url?'关联 Git':'来源',onAction:then(()=>bindingDialog.show({kind:'skill',id:entry.id},entry.source))});
        if(entry.can_check_update&&!checking)actions.push({label:'检查更新',onAction:then(()=>checkUpdates([],[entry]))});
        if(entry.update?.state==='upstream-new'&&entry.update.preview_id&&!checking)actions.push({label:'更新',onAction:then(()=>run(()=>update([],[entry])))});
        actions.push({label:'使用位置',variant:'default',onAction:then(()=>{referenceDialog?.destroy();referenceDialog=ResourceReferences(root,{resourceId:entry.id,resourceName:entry.name});})});
        resourceDialog?.destroy();resourceDialog=Dialog(root,{title:entry.name||entry.id,content,actions});
      };});
      root.querySelectorAll('[data-group-check]').forEach(b=>{b.onclick=()=>checkGroup(scopeByGroup.get(b.dataset.groupCheck));});
      root.querySelectorAll('[data-group-update]').forEach(b=>{b.onclick=()=>run(()=>{const pending=groupUpdates(scopeByGroup.get(b.dataset.groupUpdate));return update(pending.sources,pending.skills);});});
      root.querySelectorAll('[data-group-install]').forEach(b=>{b.onclick=()=>{const g=byGroup.get(b.dataset.groupInstall);gitDialog.show({url:g.url,ref:g.ref});};});
      q('[data-first]')?.addEventListener('click',()=>gitDialog.show());
      q('[data-clear-filters]')?.addEventListener('click',clearFilters);
      q('[data-capability-catalog]').querySelectorAll('[data-catalog-source]').forEach(b=>{b.onclick=()=>showSources(byGroup.get(b.dataset.catalogSource).sources);});
      q('[data-capability-catalog]').querySelectorAll('[data-catalog-edit]').forEach(b=>{b.onclick=()=>options.onEdit?.(b.dataset.catalogEdit);});
      q('[data-capability-catalog]').querySelectorAll('[data-catalog-usage]').forEach(b=>{b.onclick=()=>{
        referenceDialog?.destroy();const entry=unique.get(b.dataset.catalogUsage);
        referenceDialog=ResourceReferences(root,{resourceId:entry.id,resourceName:entry.name});
      };});
      bindSkillUpdates();syncMutationControls();
      return;
    }
    syncUpdateControls();

    q('[data-sources]').innerHTML = (!sources.length && !libraryEntries.length) ? `<div class="empty-state"><h2>先添加你的第一个资源来源</h2><p>连接自己的 Git 合集，或导入第三方 Skill。来源、版本和引用项目都会记录在这里。</p><button data-first class="primary">选择来源并导入</button></div>`
      : nothingVisible ? '<p>没有匹配的资源来源。</p>'
      : `<p class="muted library-count">${groups.length} 个来源仓库 · ${sources.reduce((n,s)=>n+s.resources.length,0)} 项来源资源 · ${libraryEntries.length} 项个人资源${search ? ' · 显示匹配结果' : ''}</p>`
      + shown.map(g => `<article class="repository-group"><header class="repository-heading"><div><span class="repository-host">${esc(g.host)}</span><h2>${esc(g.title)}</h2></div><span class="badge">${g.sources.reduce((n,s)=>n+s.visibleResources.length,0)} 项资源</span></header>${g.sources.map(s => {
      const knowledge = s.management === 'knowledge';
      const external = (knowledge && !s.can_update) || s.management === 'external';
      const u = external ? null : s.update, status = s.error ? '来源异常' : knowledge && !s.can_update ? '知识库副本' : external ? '原目录维护' : ({ current:'已是最新', available:'有可用更新', error:'检查失败', stale:'预览过期，请重新检查' })[u?.state] || '尚未检查';
      return `<section class="repository-registration"><div class="repository-meta"><span>${knowledge ? '知识库恢复' : external ? '外部目录实时引用' : '已导入'} · ${esc(s.lock.ref_ || s.lock.ref || '默认分支')} · ${(s.references || []).length} 处使用</span><span class="badge ${s.error || u?.state === 'error' ? 'bad' : u?.state === 'available' ? 'warn' : u?.state === 'current' ? 'ok' : ''}">${status}</span></div>
      ${s.error || u?.error ? '<p class="badge bad">' + esc(s.error || u.error) + '</p>' : ''}
      ${(s.warnings || []).length ? '<details class="repository-details"><summary>来源提示（' + s.warnings.length + ' 条）</summary><ul class="muted">' + s.warnings.map(w => '<li>' + esc(w) + '</li>').join('') + '</ul></details>' : ''}
      <ul class="repository-resources">${s.visibleResources.map(r => `<li class="repository-resource">${capabilityBadge(r.kind)}<div><h3>${esc(r.name)}</h3><p>${esc(r.description || '尚未提供说明')}</p><code class="resource-location">${esc((s.migration?.skills || []).find(x=>x.resource_id===r.id)?.repo_path || (r.path?.startsWith('/') ? (external ? '外部 Skill 目录' : '仓库根目录') : r.path) || r.id)}</code></div><div class="actions"><button data-resource-refs="${esc(r.id)}" data-resource-name="${esc(r.name)}" ${busy ? 'disabled' : ''}>使用项目</button></div></li>`).join('')}</ul>
      ${!s.resources.length ? '<p class="muted">当前没有可展示的资源；请检查来源状态。</p>' : ''}
      <div class="actions">${external ? '<button data-refresh-external>重新检查路径</button>' : '<button data-check-one="' + esc(s.id) + '" ' + (busy ? 'disabled' : '') + '>检查更新</button>'}${u?.state === 'available' ? '<button class="primary" data-update-one="' + esc(s.id) + '"' + (busy ? ' disabled' : '') + '>更新资源库版本</button>' : ''}<button data-references="${esc(s.id)}" ${busy ? 'disabled' : ''}>使用项目</button><button class="danger" data-remove="${esc(s.id)}" ${busy ? 'disabled' : ''}>${external ? '解除来源引用…' : '移除来源…'}</button></div>
      <details class="repository-details"><summary>来源详情 · 版本、存储与导入记录</summary><p class="path">${knowledge && !s.can_update ? '知识库' : 'Git 来源'}：${esc(s.url)}</p><p>登记名称：${esc(s.name)} · 版本：<code>${esc(s.lock.resolved_commit || '未解析')}</code></p><p class="muted">${knowledge ? (s.can_update?'从知识库恢复；可从原来源检查更新。':'随知识库恢复的离线副本。') : external ? '在原目录维护，修改实时生效；AILoom 不复制、不更新或删除原文件。' : u?.checked_at ? '上次检查：' + esc(u.checked_at) : '尚未检查上游更新。'}</p><p class="path muted">${external ? '外部原目录' : '本机实体'}：${esc(s.store_path)}</p>
      ${s.migration?.provider === 'cc-switch' ? '<p class="muted">导入渠道：CC Switch（不是更新来源）</p><ul>' + s.migration.skills.map(skill => '<li>' + esc(skill.name) + ' · <code>' + esc(skill.repo_path) + '</code></li>').join('') + '</ul>' : ''}
      <h4>哪些项目在使用</h4>${(s.references || []).length ? '<ul>' + s.references.map(r=>'<li>' + esc(r.repo_id) + ' · ' + esc(r.scope) + '</li>').join('') + '</ul>' : '<p class="muted">没有项目启用引用。</p>'}</details></section>`;
    }).join('')}</article>`).join('')
    // AIL-126：个人副本组；更新能力由后端声明
    + (localEntries.length ? `<article class="repository-group" data-local-group><header class="repository-heading"><div><span class="repository-host">本地</span><h2>个人副本</h2></div><span class="badge">${localEntries.length} 项资源</span></header>
      <p class="muted">有 Git 来源的 Skill 可检查上游更新；本地来源的副本在本机维护。</p>
      <section class="repository-registration"><ul class="repository-resources">${localEntries.map(e => `<li class="repository-resource" data-library-id="${esc(e.id)}">${capabilityBadge(e.kind)}<div><h3>${esc(e.name || e.id)}</h3><p>${esc(e.description || '尚未提供说明')}</p><code class="resource-location">${esc(e.id)}</code></div><div class="actions">${skillControls(e)}<button data-local-refs="${esc(e.id)}" data-local-name="${esc(e.name || e.id)}" ${busy ? 'disabled' : ''}>使用项目</button></div></li>`).join('')}</ul></section></article>` : '')
    + ((!sources.length && libraryEntries.length) ? '<p class="muted">还没有合集来源。可在「导入资源」里添加来源，或直接引用上面的本地资源。</p>' : '');
    bindSkillUpdates();
    q('[data-first]')?.addEventListener('click', () => q('[data-add]').click());
    root.querySelectorAll('[data-references]').forEach(b => { b.onclick = () => {
      referenceDialog?.destroy();
      referenceDialog = ResourceReferences(root, sources.find(s=>s.id===b.dataset.references), async()=>{changed();await refresh();});
    }; });
    // AIL-126：本地资源按资源粒度打开引用 Dialog（无来源层概念）
    root.querySelectorAll('[data-local-refs]').forEach(b => { b.onclick = () => {
      referenceDialog?.destroy();
      referenceDialog = ResourceReferences(root, { resourceId: b.dataset.localRefs, resourceName: b.dataset.localName, onChanged: async () => { changed(); await refresh(); } });
    }; });
    // AIL-126：来源资源同样按资源粒度使用项目
    root.querySelectorAll('[data-resource-refs]').forEach(b => { b.onclick = () => {
      referenceDialog?.destroy();
      referenceDialog = ResourceReferences(root, { resourceId: b.dataset.resourceRefs, resourceName: b.dataset.resourceName, onChanged: async () => { changed(); await refresh(); } });
    }; });
    root.querySelectorAll('[data-refresh-external]').forEach(b => { b.onclick = () => run(refresh); });
    root.querySelectorAll('[data-check-one]').forEach(b => { b.onclick = () => checkUpdates([sources.find(s=>s.id===b.dataset.checkOne)], []); });
    root.querySelectorAll('[data-update-one]').forEach(b => { b.onclick = () => run(() => update([sources.find(s => s.id === b.dataset.updateOne)])); });
    root.querySelectorAll('[data-remove]').forEach(b => { b.onclick = () => run(async () => {
      const p = await api.collectionRemove(b.dataset.remove);
      if (p.references.length) { message('仍有 ' + p.references.length + ' 处使用，请先到项目中移除。'); return; }
      if (!await confirmAction('移除来源“' + p.source.name + '”？\n' + p.note, {title:'移除来源', confirmLabel:'移除来源', destructive:true})) return;
      await api.collectionRemove(b.dataset.remove, true); changed(); await refresh(); message('来源已移除。已有项目文件和历史版本未删除。');
    }); });
    syncMutationControls();
  }
  function showSources(items) {
    if(items.length===1){showSource(items[0].id);return;}
    const content=document.createElement('div');
    content.className='library-source-picker';
    content.innerHTML=items.length?items.map(s=>`<button data-source-choice="${esc(s.id)}"><strong>${esc(s.name)} · ${esc(s.lock?.ref_||s.lock?.ref||'默认分支')}</strong><span class="muted path">${esc(s.connection_url||s.url||s.store_path||'本地文件夹')}</span><span class="muted">${s.resources?.length||0} 项 · ${s.management==='external'?'原目录维护':'AILoom 管理'}</span></button>`).join(''):'<p class="muted">还没有登记的合集来源。</p>';
    referenceDialog?.destroy();
    const modal=Dialog(root,{title:'选择要管理的来源',content,actions:[{label:'关闭'}]});
    referenceDialog=modal;
    content.querySelectorAll('[data-source-choice]').forEach(b=>{b.onclick=()=>{modal.destroy();showSource(b.dataset.sourceChoice);};});
  }
  function showSource(id) {
    const source=sources.find(s=>s.id===id);if(!source)return;
    const content=document.createElement('div');
    content.innerHTML=`<p class="path">${esc(source.connection_url||source.url||source.store_path||'本地文件夹')}</p>${source.migration?.provider==='cc-switch'?'<p class="muted">从 CC Switch 迁入</p>':''}${source.error?`<p class="field-error">${esc(source.error)}</p>`:''}<p>${source.resources.length} 项能力</p><dl class="source-version"><dt>当前版本</dt><dd><code>${esc(source.lock?.resolved_commit||'本地内容')}</code></dd><dt>分支 / 标签</dt><dd>${esc(source.update_ref||source.lock?.ref_||source.lock?.ref||'默认分支')}</dd></dl>`;
    const modal=Dialog(root,{title:source.name||'来源详情',content,actions:[{label:'关闭'},...(!checking?[...(source.can_rebind!==false&&source.management!=='external'?[{label:'变更来源',onAction:()=>{modal.close();bindingDialog.show({kind:'collection',id},source);return false;}}]:[]),{label:'移除来源',variant:'destructive',onAction:async()=>{
      try {
        const preview=await api.collectionRemove(id);
        if(preview.references.length){message('此来源仍在项目中使用，请先到对应项目移除能力。');return false;}
        if(!await confirmAction(`移除「${source.name}」及其 ${source.resources.length} 项能力？`,{title:'移除来源',confirmLabel:'移除',destructive:true}))return false;
        await api.collectionRemove(id,true);changed();await refresh();
      }catch(e){message(e.message);return false;}
    }}]:[])]});
    referenceDialog?.destroy();referenceDialog=modal;
  }
  async function refresh() {
    const [v, lib] = await Promise.all([api.collections(), api.libraryList().catch(() => ({ entries: [] }))]);
    if (alive) {
      sources = v.sources;
      // AIL-126：本地（资源库）资源并入同一展示模型。
      libraryEntries = (lib.entries || []).map(e => ({ ...e, name: e.name || (e.id.split('/').pop()), update:e.update }));
      renderSources();
    }
  }
  refresh().catch(e => message(e.message));
  return { refresh, destroy() { alive = false; stopCheck=true;updateDialog?.close();updateDialog?.destroy();resourceDialog?.destroy();referenceDialog?.destroy(); migrationDialog.destroy(); importDialog.destroy(); gitDialog.destroy(); bindingDialog.destroy(); root.remove(); } };
}
