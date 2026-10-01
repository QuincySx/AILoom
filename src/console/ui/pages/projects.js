import { api, esc } from '../services/api.js';
import { setTarget } from '../state/target.js';
import { ProjectDialog } from '../features/projectDialog.js';

// 「管理目录」列表页（#/projects/manage）。目录详情由 pages/workspace.js 负责。
const projectName = r => r.project?.name || Object.values(r.worktrees || {})[0]?.path?.split('/').filter(Boolean).pop() || r.repo_id;

export function mount(container) {
  const root = document.createElement('div');
  container.append(root);
  let disposed = false;
  const dialogs = [];
  const error = e => { if (!disposed) (root.querySelector('dialog[open] [data-form-error]') || root.querySelector('[data-message]')).textContent = e.message; };
  root.innerHTML = '<p data-message role="status">正在读取项目…</p>';
  async function load() {
    try {
      const state = await api.state();
      if (disposed) return;
      // 文件夹项目没有 Git Worktree；后端已按 common_dir 合成 local（含失联状态），
      // 这里仅对缺失字段的数据兜底，不覆盖后端的 active/missing 判定。
      state.repos = state.repos.map(r => r.repo_id.startsWith('nongit-') && !Object.keys(r.worktrees || {}).length
        ? {...r, worktrees:{local:{path:r.common_dir, status:'active'}}} : r);
      list(state);
    } catch (e) { error(e); }
  }
  function list(state) {
    setTarget(null);
    root.innerHTML = `<header class="project-heading"><div><h1>我的目录</h1><p class="muted">选择一个文件夹，查看和管理它使用的 AI 能力。</p></div><button data-new class="primary">添加目录</button></header>
      <p data-message role="status"></p><div class="project-toolbar"><input data-search type="search" aria-label="搜索项目" placeholder="搜索项目名称、路径或远端"><select data-filter aria-label="项目分类"><option value="">全部分类</option></select><select data-kind aria-label="项目类型"><option value="">全部类型</option><option value="git">Git 项目</option><option value="nongit">文件夹项目</option></select></div><div data-list></div>`;
    root.querySelector('[data-new]').onclick = () => { dialogs.push(ProjectDialog(root,{onCreated:id=>{location.hash='#/projects/'+encodeURIComponent(id);}})); };
    const filter = root.querySelector('[data-filter]');
    const categories = [...new Set(state.repos.map(r => r.project?.category || '未分类'))].sort();
    filter.innerHTML += categories.map(c => `<option>${esc(c)}</option>`).join('');
    const render = () => {
      const query = root.querySelector('[data-search]').value.trim().toLowerCase();
      const kind = root.querySelector('[data-kind]').value;
      const repos = state.repos.filter(r => (!filter.value || (r.project?.category || '未分类') === filter.value)
        && (!kind || (r.repo_id.startsWith('nongit-') ? 'nongit' : 'git') === kind)
        && `${projectName(r)} ${r.origin_normalized || ''} ${Object.values(r.worktrees || {}).map(w => w.path).join(' ')}`.toLowerCase().includes(query));
      root.querySelector('[data-list]').innerHTML = repos.length ? repos.sort((a,b) => projectName(a).localeCompare(projectName(b))).map(r => `<article class="project-row"><div><h2><a href="#/projects/${encodeURIComponent(r.repo_id)}">${esc(projectName(r))}</a></h2><p class="muted">${esc(Object.values(r.worktrees || {})[0]?.path || '暂无工作目录')}</p><p>${r.origin_normalized ? '远端 ' + esc(r.origin_normalized) : '本地目录'} · ${esc(r.project?.category || '未分类')}</p></div><span class="badge">${r.repo_id.startsWith('nongit-') ? '文件夹' : 'Git'}</span></article>`).join('') : '<section class="step"><h2>没有匹配的目录</h2><p>添加一个本地项目开始使用；可从资源库添加所需能力。</p><p><a href="#/onboarding">第一次使用？查看三步上手指南 →</a></p></section>';
    };
    root.querySelector('[data-search]').oninput = render;
    filter.onchange = render;
    root.querySelector('[data-kind]').onchange = render;
    render();
  }
  load();
  return {isDirty:() => false, destroy() {disposed = true; dialogs.forEach(c => c.destroy()); setTarget(null); root.remove();}};
}
