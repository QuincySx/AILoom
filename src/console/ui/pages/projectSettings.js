// AIL-124：项目共享设置 —— 独立项目内路由（#/projects/<id>/settings）。
// 与日常目录页显式分开：作用范围整个项目；Markdown 指令只在这里编辑。
// 返回目录时保持原工作目录、子目录与页签（sessionStorage 一次性恢复）。

import { api, esc } from '../services/api.js';
import { setTarget, sharedSettingsTarget } from '../state/target.js';
import { InstructionsPanel } from '../features/instructionsPanel.js';
import { NativeFiles } from '../features/nativeFiles.js';
import { KnowledgePanel } from '../features/knowledgePanel.js';
import { TeamSourceDialog } from '../features/teamSourceDialog.js';
import {projectNavigation} from '../components/sectionNav.js';
import {PageHeader,setBreadcrumb} from '../components/layout.js';
import {Item} from '../components/item.js';

const RETURN_KEY = 'ailoom-return-after-settings';

export function mount(container, ctx = {}) {
  const root = document.createElement('div');
  root.className='project-config-page';
  container.append(root);
  let disposed = false;
  let children = [];
  let dirty = () => false;
  let tab = ctx.tab === 'knowledge' ? 3 : ctx.tab === 'instructions' ? 1 : ctx.tab === 'files' ? 4 : 2;
  let repo = null;
  let target = null;
  let renderVersion = 0;
  const error = e => { if (!disposed) (root.querySelector('dialog[open] [data-form-error]') || root.querySelector('[data-message]')).textContent = e.message; };

  // 进入共享设置前记录目录页状态；返回（含浏览器后退）时恢复。
  if (ctx.returnState) sessionStorage.setItem(RETURN_KEY, JSON.stringify(ctx.returnState));

  root.innerHTML = '<p data-message role="status">正在读取项目…</p>';

  async function load() {
    try {
      const state = await api.state();
      if (disposed) return;
      state.repos = state.repos.map(r => r.repo_id.startsWith('nongit-') && !Object.keys(r.worktrees || {}).length
        ? {...r, worktrees:{local:{path:r.common_dir, status:'active'}}} : r);
      repo = state.repos.find(r => r.repo_id === ctx.projectId);
      if (!repo) { error(new Error('项目不存在，请返回项目列表。')); return; }
      let remembered=null;try{remembered=JSON.parse(sessionStorage.getItem(RETURN_KEY));}catch{}
      const wtPath = (remembered?.repoId===repo.repo_id?repo.worktrees?.[remembered.wt]?.path:null)
        || Object.values(repo.worktrees || {}).find(w=>repo.common_dir===w.path+'/.git')?.path
        || Object.values(repo.worktrees || {}).find(w => w.status !== 'missing')?.path;
      if (!wtPath) { error(new Error('项目没有可用的工作目录。')); return; }
      try { await api.approveDir(wtPath); } catch { /* 失联目录后面按错误呈现 */ }
      target = sharedSettingsTarget({
        projectId: repo.repo_id, name: repo.project?.name || repo.repo_id,
        rootPath: wtPath, kind: repo.repo_id.startsWith('nongit-') ? 'nongit' : 'git',
      });
      setTarget(target);
      renderShell();
      await renderTab();
    } catch (e) { error(e); }
  }

  function renderShell() {
    const name=repo.project?.name||Object.values(repo.worktrees||{})[0]?.path?.split('/').filter(Boolean).pop()||repo.repo_id;
    const title=tab===1?'项目说明':tab===3?'知识库':'项目设置';
    setBreadcrumb([{label:'项目',href:'#/projects'},{label:name,href:'#/projects/'+encodeURIComponent(repo.repo_id)},{label:title}]);
    // The breadcrumb already names the project and leads back to it.
    root.innerHTML = `${PageHeader({title,description:tab===1?'所有工作目录共用这份说明。':tab===3?'保存和整理项目知识。':'共享设置对整个项目生效。'})}
      ${projectNavigation({id:repo.repo_id,active:tab===1?'instructions':tab===3?'knowledge':'settings',settings:true})}
      <p data-message role="status"></p>
      <div data-content></div>`;
    root.querySelectorAll('[data-tab]').forEach(b => { b.onclick = () => {
      location.hash=`#/projects/${encodeURIComponent(repo.repo_id)}/${b.dataset.tab==='4'?'files':b.dataset.tab==='3'?'knowledge':'settings'}`;
    }; });
  }

  async function renderTab() {
    const version = ++renderVersion;
    children.forEach(c => c.destroy?.()); children = []; dirty = () => false;
    root.querySelectorAll('[data-tab]').forEach(b => { b.classList.toggle('on', Number(b.dataset.tab) === tab); b.setAttribute('aria-pressed', String(Number(b.dataset.tab) === tab)); });
    const slot = root.querySelector('[data-content]');
    slot.textContent = '正在加载…';
    try {
      if (disposed || version !== renderVersion) return;
      slot.innerHTML = '';
      if (tab === 1) {
        const panel = InstructionsPanel(slot, {target, hideHeading:true,onChanged: () => setTarget(target)});
        children.push(panel); dirty = () => panel.isDirty(); return;
      }
      if (tab === 2) { renderProfile(slot); return; }
      if (tab === 4) { const panel=NativeFiles(slot,{rootPath:target.rootPath,scope:'project'});children.push(panel);dirty=()=>panel.isDirty();return; }
      if (tab === 3) { const panel=KnowledgePanel(slot,{rootPath:target.rootPath || Object.values(repo.worktrees)[0].path}); children.push(panel); dirty=()=>panel.isDirty(); return; }

    } catch (e) { if (!disposed && version === renderVersion) { slot.textContent = '项目设置读取失败。'; error(e); } }
  }

  function renderProfile(slot) {
    const isGit = !repo.repo_id.startsWith('nongit-');
    let saving = false;
    slot.innerHTML = `<form class="project-settings-profile">
      <label>名称<span class="input-action"><input name="name" required maxlength="120" value="${esc(repo.project?.name || '')}"><button type="submit" class="primary" disabled>保存设置</button></span></label>
      <div class="field"><label>本地位置</label><p class="path">${esc(target.rootPath)}</p></div><details><summary>项目分类</summary><label>分类<input name="category" maxlength="80" value="${esc(repo.project?.category || '')}" placeholder="未分类"></label></details>
      <p data-form-error class="field-error" role="alert"></p>
    </form>
    <section class="project-settings-directories" data-team-source>
      <div class="tab-actions"><h2>团队资源来源</h2><button data-edit-team-source disabled>修改来源</button></div>
      <div data-team-source-info role="status">正在读取来源…</div>
    </section>
    <section class="project-settings-directories">
      <div class="tab-actions"><h2>工作目录</h2>${isGit ? '<button data-refresh>刷新分支目录</button>' : ''}</div>
      <div data-directories role="status">正在读取目录…</div>
    </section>`;
    const form = slot.querySelector('form');
    const name = form.elements.namedItem('name'), category = form.elements.namedItem('category');
    const changed = () => name.value !== (repo.project?.name || '') || category.value !== (repo.project?.category || '');
    dirty = () => saving || changed();
    form.oninput = () => { form.querySelector('[type=submit]').disabled = saving || !changed(); };
    form.onsubmit = async event => {
      event.preventDefault();
      if (saving) return;
      saving = true;
      form.querySelectorAll('input,button').forEach(el => el.disabled = true);
      form.querySelector('[data-form-error]').textContent = '';
      try {
        const saved = await api.projectMetadata({repo_id:repo.repo_id, name:name.value, category:category.value});
        if (disposed) return;
        repo.project = saved;
        name.value = saved.name || ''; category.value = saved.category || '';
        setBreadcrumb([{label:'项目',href:'#/projects'},{label:saved.name,href:'#/projects/'+encodeURIComponent(repo.repo_id)},{label:'项目设置'}]);
        root.querySelector('[data-message]').textContent = '已保存';
        slot.querySelectorAll('[data-root-name]').forEach(el => el.textContent = saved.name);
      } catch (e) { if (!disposed) form.querySelector('[data-form-error]').textContent = e.message; }
      finally {
        saving = false;
        form.querySelectorAll('input,button').forEach(el => el.disabled = false);
        form.querySelector('[type=submit]').disabled = !changed();
      }
    };
    const directorySlot = slot.querySelector('[data-directories]');
    const version = renderVersion;
    const active = () => !disposed && version === renderVersion;
    async function teamSource() {
      const section = slot.querySelector('[data-team-source]');
      const info = slot.querySelector('[data-team-source-info]');
      const edit = slot.querySelector('[data-edit-team-source]');
      edit.disabled = true;
      try {
        const source = await api.projectSource({action:'read',root:target.rootPath});
        if (!active()) return;
        section.hidden = false;
        info.innerHTML = `<div class="project-settings-directory"><div><strong>${source.source.type==='git'?'Git 仓库':'已有文件夹'}</strong><p class="path">${esc(source.source.url || source.source.path)}</p>${source.source.ref?`<p class="muted">分支 / 标签：${esc(source.source.ref)}</p>`:''}${source.source.type==='git'?`<p class="path muted">下载目录：${esc(source.cache_root)}</p>`:''}<p class="path muted">应用于工作目录：${esc(source.root)}</p></div></div>`;
        edit.disabled = false;
        edit.onclick = () => children.push(TeamSourceDialog(root,{rootPath:source.root,onSaved:async()=>{
          if (!active()) return;
          await teamSource();
          if (active()) root.querySelector('[data-message]').textContent = '团队来源已修改。到工作目录预览并应用改动。';
        }}));
      } catch (e) {
        if (!active()) return;
        if (e.message === '项目尚未绑定团队来源') { section.hidden = true; return; }
        info.textContent = e.message;
      }
    }
    teamSource();
    async function directories() {
      const groups = await Promise.all(Object.entries(repo.worktrees || {}).map(async ([id,w]) => {
        if (w.status === 'missing') return {id,w,dirs:[],error:'目录不存在'};
        try {
          await api.approveDir(w.path);
          const result = await api.projectDirs(w.path);
          return {id,w,dirs:result.dirs || []};
        } catch (e) { return {id,w,dirs:[],error:e.message}; }
      }));
      if (!active()) return;
      const rows = [];
      directorySlot.innerHTML = groups.map(({id,w,dirs,error}) => {
        const row = (label,rel,kind) => {
          const index = rows.push({repoId:repo.repo_id,wt:id,dir:rel,node:kind,tab:0}) - 1;
          const path=rel?w.path+'/'+rel:w.path;
          return Item({title:label,titleMarkup:`<span ${!isGit&&!rel?'data-root-name':''}>${esc(label)}</span>`,description:path,density:'regular',className:'project-settings-directory',primaryAction:w.status==='missing'?null:{href:`#/projects/${encodeURIComponent(repo.repo_id)}`,attributes:`data-directory="${index}"`,label:'配置目录 '+label},actions:w.status==='missing'?'<span class="badge warn">目录失联</span>':''});

        };
        return row(isGit ? (w.branch?.replace(/^refs\/heads\//,'') || w.path.split('/').filter(Boolean).pop()) : (repo.project?.name || '项目目录'),'',isGit?'worktree':'project')
          + [...new Set(dirs.map(d=>d.path).filter(Boolean))].map(rel=>row(rel,rel,'directory')).join('')
          + (error && w.status !== 'missing' ? `<p class="field-error">${esc(error)}</p>` : '');
      }).join('');
      directorySlot.querySelectorAll('[data-directory]').forEach(link => {
        link.onclick = () => sessionStorage.setItem(RETURN_KEY, JSON.stringify(rows[Number(link.dataset.directory)]));
      });
    }
    directories();
    const refresh = slot.querySelector('[data-refresh]');
    if (refresh) refresh.onclick = async () => {
      refresh.disabled = true;
      try {
        await api.repoDiscover(target.rootPath);
        const state = await api.state();
        if (!active()) return;
        const updated = state.repos.find(r=>r.repo_id===repo.repo_id);
        if (!updated) throw new Error('项目不存在');
        repo.worktrees = updated.worktrees;
        await directories();
      } catch (e) { if (active()) root.querySelector('[data-message]').textContent = e.message; }
      finally { refresh.disabled = false; }
    };
  }

  load();
  return {
    isDirty: () => dirty(),
    destroy() { disposed = true; children.forEach(c => c.destroy?.()); setTarget(null); root.remove(); },
  };
}

export { RETURN_KEY };
