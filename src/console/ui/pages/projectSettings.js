// AIL-124：项目共享设置 —— 独立项目内路由（#/projects/<id>/settings）。
// 与日常目录页显式分开：作用范围整个项目；Markdown 指令只在这里编辑。
// 返回目录时保持原工作目录、子目录与页签（sessionStorage 一次性恢复）。

import { api, esc } from '../services/api.js';
import { setTarget, sharedSettingsTarget } from '../state/target.js';
import { InstructionsPanel } from '../features/instructionsPanel.js';
import { NativeFiles } from '../features/nativeFiles.js';
import { KnowledgePanel } from '../features/knowledgePanel.js';
import { confirmAction } from '../components/dialog.js';

const RETURN_KEY = 'ailoom-return-after-settings';

export function mount(container, ctx = {}) {
  const root = document.createElement('div');
  container.append(root);
  let disposed = false;
  let children = [];
  let dirty = () => false;
  let tab = ctx.tab === 'knowledge' ? 3 : ctx.tab === 'instructions' ? 1 : 2;
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
    root.innerHTML = `<a href="#/projects/${encodeURIComponent(repo.repo_id)}" data-back>返回 ${esc(repo.project?.name || repo.repo_id)} · 当前目录</a>
      <header class="project-heading"><div><h1>${esc(repo.project?.name || repo.repo_id)} · 项目设置</h1>
        <p class="muted"></p></div></header>
      <p data-message role="status"></p>
      <nav class="project-tabs" aria-label="项目设置">${[[2,'项目与目录'],[1,'项目说明'],[3,'知识库'],[4,'Rules 与 Agent']].map(([id,label]) => `<button data-tab="${id}">${label}</button>`).join('')}</nav>
      <div data-content></div>`;
    root.querySelectorAll('[data-tab]').forEach(b => { b.onclick = async () => {
      if (dirty() && !await confirmAction('放弃未保存的修改？', {title:'离开当前设置', confirmLabel:'放弃并离开'})) return;
      tab = Number(b.dataset.tab); renderTab();
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
        const panel = InstructionsPanel(slot, {target, onChanged: () => setTarget(target)});
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
      <label>名称<input name="name" required maxlength="120" value="${esc(repo.project?.name || '')}"></label>
      <label>分类<input name="category" maxlength="80" value="${esc(repo.project?.category || '')}" placeholder="未分类"></label>
      <button type="submit" class="primary" disabled>保存</button>
      <p data-form-error class="field-error" role="alert"></p>
    </form>
    <section class="project-settings-directories">
      <div class="tab-actions"><h2>${isGit ? 'Worktree 与子目录' : '项目与子目录'}</h2>${isGit ? '<button data-refresh>刷新 Worktree</button>' : ''}</div>
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
        root.querySelector('h1').textContent = `${saved.name} · 项目设置`;
        root.querySelector('[data-back]').textContent = `返回 ${saved.name} · 当前目录`;
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
          return `<div class="project-settings-directory"><div><strong ${!isGit && !rel ? 'data-root-name' : ''}>${esc(label)}</strong><p class="path muted"><ailoom-path title="${esc(rel ? w.path + '/' + rel : w.path)}">${esc(rel ? w.path + '/' + rel : w.path)}</ailoom-path></p></div>
            ${w.status === 'missing' ? '<span class="badge">目录不存在</span>' : `<a href="#/projects/${encodeURIComponent(repo.repo_id)}" data-directory="${index}">${rel ? '管理能力' : '管理能力与子目录'}</a>`}</div>`;
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
