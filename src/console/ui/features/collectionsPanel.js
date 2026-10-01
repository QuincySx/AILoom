// 资源来源管理：导入入口、真实更新检查、版本确认、引用影响和来源移除。
import { api, esc } from '../services/api.js';
import { setTarget, currentTarget } from '../state/target.js';
import { Dialog, confirmAction } from '../components/dialog.js';
import { CcSwitchImport } from './ccSwitchImport.js';
import { ResourceReferences } from './resourceReferences.js';
import { ImportDialog } from './importDialog.js';

// One visual vocabulary for every capability type in the library.
const capabilityTypes = {
  skill: ['Skill', '<path d="m12 3 3 6 6 3-6 3-3 6-3-6-6-3 6-3z"/>'],
  mcp: ['MCP', '<path d="M8 3v5m8-5v5M6 8h12v4a6 6 0 0 1-12 0zM12 18v3"/>'],
  agent: ['Agent', '<rect x="4" y="7" width="16" height="13" rx="3"/><path d="M12 3v4M8 12h1m6 0h1M9 16h6"/>'],
  rule: ['Rules', '<path d="M9 5h11M9 12h11M9 19h11M3 5h1M3 12h1M3 19h1"/>'],
  doc: ['文档', '<path d="M5 3h10l4 4v14H5zM14 3v5h5M8 12h8M8 16h8"/>'],
  learning: ['经验', '<path d="M4 4h6l2 2 2-2h6v15h-6l-2 2-2-2H4zM12 6v15"/>'],
  env: ['环境', '<path d="M4 6h16M4 12h16M4 18h16M8 4v4M16 10v4M10 16v4"/>'],
  hook: ['Hook', '<path d="M8 3v11a5 5 0 0 0 10 0V9l-3 3M5 3h6"/>'],
  package: ['依赖包', '<path d="m12 3 9 5v9l-9 5-9-5V8zM3 8l9 5 9-5M12 13v9M7 5l10 6"/>'],
};
function capabilityBadge(kind) {
  const [label, drawing] = capabilityTypes[kind] || [kind, '<rect x="4" y="4" width="16" height="16" rx="3"/>'];
  return `<span class="resource-kind" title="${esc(label)}"><svg viewBox="0 0 24 24" aria-hidden="true">${drawing}</svg><span>${esc(label)}</span></span>`;
}

// Group transport aliases, but never collapse distinct hosts or local paths.
export function repositoryGroups(sources) {
  const groups = new Map();
  for (const source of sources) {
    const raw = source.url || '';
    let key = raw, title = source.name, host = '本地来源';
    try {
      const url = new URL(raw.replace(/^git@([^:]+):/, 'ssh://git@$1/'));
      if (['https:', 'http:', 'ssh:'].includes(url.protocol)) {
        const path = url.pathname.replace(/\/$/, '').replace(/\.git$/, '').replace(/^\//, '');
        host = url.host.toLowerCase();
        key = host + '/' + path;
        title = path || host;
      }
    } catch { /* Local paths keep their exact identity. */ }
    if (!groups.has(key)) groups.set(key, {key, title, host, sources:[]});
    groups.get(key).sources.push(source);
  }
  return [...groups.values()];
}

export function CollectionsPanel(container, options = {}) {
  const root = document.createElement('section');
  container.appendChild(root);
  let alive = true, busy = false, sources = [];
  let libraryEntries = []; // Personal copies may also have a Git origin.
  let referenceDialog = null;
  root.innerHTML = `
    <header class="page-head"><div><h1>${esc(options.title || (options.updatesOnly ? '来源更新' : '资源库'))}</h1>
      <p>${esc(options.description || (options.updatesOnly ? '先检查上游，再确认资源库版本。哪些项目使用新版，由你决定。' : '把自己的合集与第三方资源放在一起管理，按需引用到项目。'))}</p></div>
      <div class="actions">${options.onCreate?'<button data-create>新建 Rules / Agent</button>':''}<button data-cc-switch>从 CC Switch 迁移</button><button data-add class="primary">导入资源</button></div></header>
    <div class="toolbar"><input data-search type="search" aria-label="搜索来源或资源" placeholder="搜索来源、Skill 或 MCP…"><div><button data-check>检查全部更新</button><button data-update disabled>更新全部可用版本</button></div></div>
    <p data-msg class="inline-status muted" role="status" aria-live="polite"></p>
    <div data-sources><p class="muted">正在读取资源库…</p></div>`;
  const q = s => root.querySelector(s);
  if(options.onCreate)q('[data-create]').onclick=options.onCreate;
  if(options.compactLibrary) {
    const list=document.createElement('div');list.dataset.capabilityCatalog='';
    const sourceList=q('[data-sources]');sourceList.before(list);sourceList.hidden=true;
    const filter=document.createElement('select');filter.dataset.libraryKind='';filter.setAttribute('aria-label','能力类型');
    filter.innerHTML='<option value="">全部类型</option>'+Object.entries(capabilityTypes).map(([kind,[label]])=>`<option value="${kind}">${label}</option>`).join('');
    q('.toolbar').insertBefore(filter,q('.toolbar>div'));filter.onchange=renderSources;
    q('.page-head p').remove();
  }

  const migrationDialog = CcSwitchImport(root, {onChanged:async () => { changed(); await refresh(); }});
  q('[data-cc-switch]').onclick = () => migrationDialog.show();
  // AIL-113：导入业务抽到可复用 ImportDialog（项目内「＋导入」共用同一实现）。
  const importDialog = ImportDialog(root, { onChanged: async () => { changed(); await refresh(); } });
  function changed() { const t = currentTarget(); if (t) setTarget(t); options.onChanged?.(); }
  function message(text) { if (alive) q('[data-msg]').textContent = text; }
  async function run(fn) {
    if (busy) return;
    busy = true; root.setAttribute('aria-busy', 'true');
    root.querySelectorAll('button').forEach(b => { b.disabled = true; });
    try { await fn(); } catch (e) { message(e.message); }
    finally { busy = false; if (alive) { root.removeAttribute('aria-busy'); root.querySelectorAll('button').forEach(b => { b.disabled = false; }); renderSources(); q('[data-update]').disabled = !available().length && !availableSkills().length; } }
  }
  q('[data-add]').onclick = () => importDialog.show();
  function available() { return sources.filter(s => s.update?.state === 'available' && s.update.preview?.preview_id); }
  function checkableSkills() { return libraryEntries.filter(e => e.can_check_update); }
  function availableSkills() { return checkableSkills().filter(e => e.update?.state === 'upstream-new' && e.update.preview_id); }
  function skillControls(entry) {
    if (!entry.can_check_update) return '';
    const labels = {'up-to-date':'已是最新','upstream-new':'可更新','local-modified':'本地已修改',conflict:'本地与上游冲突','upstream-missing':'上游已移除',stale:'请重新检查',error:'检查失败'};
    return `<span class="badge">${esc(labels[entry.update?.state] || '尚未检查')}</span><button data-skill-check="${esc(entry.name)}">检查更新</button>${entry.update?.state === 'upstream-new' && entry.update.preview_id ? `<button data-skill-update="${esc(entry.name)}">更新</button>` : ''}`;
  }
  function bindSkillUpdates() {
    root.querySelectorAll('[data-skill-check]').forEach(b => { b.onclick = () => run(async () => {
      const entry = libraryEntries.find(e => e.name === b.dataset.skillCheck);
      try { await api.checkUpdate(entry.name); message('检查完成。'); }
      finally { await refresh(); }
    }); });
    root.querySelectorAll('[data-skill-update]').forEach(b => { b.onclick = () => run(() => update([], [libraryEntries.find(e => e.name === b.dataset.skillUpdate)])); });
  }
  async function update(items, skills = []) {
    const refs = items.flatMap(s => s.references || []);
    const removed = items.flatMap(s => s.update.preview.removed || []);
    if (!await confirmAction('更新 ' + (items.length + skills.length) + ' 个来源或 Skill？' + (skills.length ? '' : '\n涉及 ' + refs.length + ' 处使用。') + '\n不会自动修改项目。' + (removed.length ? '\n上游删除：' + removed.join('、') : '') + '\n旧版本保留。', {title:'更新资源库', confirmLabel:'确认更新'})) return;
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
    message(failures.length ? '部分更新未完成：' + failures.join('；') + '。请重新检查；已成功的更新请到需要升级的项目应用。' : '资源库已更新。请到需要升级的项目应用改动。');
  }
  q('[data-check]').onclick = () => run(async () => {
    message('正在检查更新…');
    let availableCount = 0, failedCount = 0;
    const failures = [];
    try {
      if (sources.length) {
        try {
          const v = await api.collectionCheck();
          availableCount += v.items.filter(i => i.state === 'available').length;
          failedCount += v.items.filter(i => i.state === 'error').length;
        } catch (e) { failedCount++; failures.push('来源合集：' + e.message); }
      }
      for (const entry of checkableSkills()) {
        try {
          const v = await api.checkUpdate(entry.name);
          if (v.status.state === 'upstream-new') availableCount++;
        } catch (e) { failedCount++; failures.push(entry.name + '：' + e.message); }
      }
    } finally { await refresh(); }
    message('检查完成：' + availableCount + ' 个可更新，' + failedCount + ' 个失败。' + failures.join('；'));
  });
  q('[data-update]').onclick = () => run(() => update(available(), availableSkills()));
  // AIL-126：跨页往返（如引用 Dialog「前往管理」后返回）恢复搜索上下文。
  if (options.searchKey) {
    try { q('[data-search]').value = sessionStorage.getItem(options.searchKey) || ''; } catch { /* 隐私模式忽略 */ }
    q('[data-search]').addEventListener('input', () => { try { sessionStorage.setItem(options.searchKey, q('[data-search]').value); } catch { /* 忽略 */ } });
  }
  q('[data-search]').oninput = renderSources;
  function renderSources() {
    if (!alive) return;
    q('[data-update]').disabled = busy || (!available().length && !availableSkills().length);
    q('[data-check]').disabled = busy || !(sources.length || checkableSkills().length);
    const search = q('[data-search]').value.toLowerCase();
    const groups = repositoryGroups(sources);
    const shown = groups.map(g => ({...g, sources:g.sources.map(s => ({...s, visibleResources:!search || (g.title + ' ' + s.url).toLowerCase().includes(search) ? s.resources : s.resources.filter(r => JSON.stringify([r.name,r.description,r.path,r.kind]).toLowerCase().includes(search))})).filter(s => !search || s.visibleResources.length || (g.title + ' ' + s.url).toLowerCase().includes(search))})).filter(g => g.sources.length);
    // AIL-126：本地资源（无远端）单独一组；空状态基于全部可见资源，
    // 已有本地资源时不再显示整个中心为空。
    const localEntries = libraryEntries.filter(e => !search || JSON.stringify([e.name, e.id, e.description, e.kind]).toLowerCase().includes(search));
    const nothingVisible = !shown.length && !localEntries.length;
    if(options.compactLibrary) {
      const kind=q('[data-library-kind]').value;
      const unique=new Map();
      // 不同托管平台上的同名仓库（如 GitHub 与 GitLab 的 owner/repo）在标签里带上平台名以便区分。
      const titleCount=groups.reduce((m,g)=>m.set(g.title,(m.get(g.title)||0)+1),new Map());
      const label=g=>titleCount.get(g.title)>1&&g.host?`${g.host} · ${g.title}`:g.title;
      for(const g of shown)for(const source of g.sources)for(const resource of source.visibleResources)unique.set(resource.id,{...resource,sourceLabel:label(g),sourceId:source.id,updateState:source.update?.state});
      for(const resource of localEntries)unique.set(resource.id,{...resource,sourceLabel:resource.can_check_update?'上游来源':'本地',editable:!!options.onEdit});
      const entries=[...unique.values()].filter(e=>!kind||e.kind===kind);
      q('[data-capability-catalog]').innerHTML=entries.length?`<ul class="repository-resources">${entries.map(e=>`<li class="repository-resource">${capabilityBadge(e.kind)}<div><h3>${esc(e.name||e.id)} ${e.sourceId?`<button class="library-entry-source" data-catalog-source="${esc(e.sourceId)}">${esc(e.sourceLabel)}</button>`:`<small class="library-entry-source">${esc(e.sourceLabel)}</small>`}${e.updateState==='available'?'<span class="badge ok">可更新</span>':''}</h3><p>${esc(e.description||'')}</p></div><div class="actions">${skillControls(e)}${e.editable?`<button data-catalog-edit="${esc(e.id)}">编辑</button>`:''}<button data-catalog-usage="${esc(e.id)}">使用项目</button></div></li>`).join('')}</ul>`:'<p class="muted">没有匹配的能力。可搜索其他名称或导入新能力。</p>';
      q('[data-capability-catalog]').querySelectorAll('[data-catalog-source]').forEach(b=>{b.onclick=()=>showSource(b.dataset.catalogSource);});
      q('[data-capability-catalog]').querySelectorAll('[data-catalog-edit]').forEach(b=>{b.onclick=()=>options.onEdit?.(b.dataset.catalogEdit);});
      q('[data-capability-catalog]').querySelectorAll('[data-catalog-usage]').forEach(b=>{b.onclick=()=>{
        referenceDialog?.destroy();const entry=unique.get(b.dataset.catalogUsage);
        referenceDialog=ResourceReferences(root,{resourceId:entry.id,resourceName:entry.name});
      };});
      bindSkillUpdates();
      return;
    }

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
    root.querySelectorAll('[data-check-one]').forEach(b => { b.onclick = () => run(async () => { message('正在检查上游…'); await api.collectionCheck(b.dataset.checkOne); await refresh(); message('检查完成。'); }); });
    root.querySelectorAll('[data-update-one]').forEach(b => { b.onclick = () => run(() => update([sources.find(s => s.id === b.dataset.updateOne)])); });
    root.querySelectorAll('[data-remove]').forEach(b => { b.onclick = () => run(async () => {
      const p = await api.collectionRemove(b.dataset.remove);
      if (p.references.length) { message('仍有 ' + p.references.length + ' 处使用，请先到项目中移除。'); return; }
      if (!await confirmAction('移除来源“' + p.source.name + '”？\n' + p.note, {title:'移除来源', confirmLabel:'移除来源', destructive:true})) return;
      await api.collectionRemove(b.dataset.remove, true); changed(); await refresh(); message('来源已移除。已有项目文件和历史版本未删除。');
    }); });
  }
  function showSource(id) {
    const source=sources.find(s=>s.id===id);if(!source)return;
    const content=document.createElement('div');
    content.innerHTML=`<p class="path">${esc(source.url||source.store_path||'本地文件夹')}</p>${source.migration?.provider==='cc-switch'?'<p class="muted">从 CC Switch 迁入</p>':''}${source.error?`<p class="field-error">${esc(source.error)}</p>`:''}<p>${source.resources.length} 项能力</p>`;
    const modal=Dialog(root,{title:source.name||'来源详情',content,actions:[{label:'关闭'},{label:'移除来源',variant:'destructive',onAction:async()=>{
      try {
        const preview=await api.collectionRemove(id);
        if(preview.references.length){message('此来源仍在项目中使用，请先到对应项目移除能力。');return false;}
        if(!await confirmAction(`移除「${source.name}」及其 ${source.resources.length} 项能力？`,{title:'移除来源',confirmLabel:'移除',destructive:true}))return false;
        await api.collectionRemove(id,true);changed();await refresh();
      }catch(e){message(e.message);return false;}
    }}]});
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
  return { refresh, destroy() { alive = false; referenceDialog?.destroy(); migrationDialog.destroy(); importDialog.destroy(); root.remove(); } };
}
