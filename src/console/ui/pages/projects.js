import { api, esc } from '../services/api.js';
import { setTarget, currentGeneration, shouldApply } from '../state/target.js';
import { PlanPreview, JobPanel } from '../features/planPreview.js';
import { InstructionsPanel } from '../features/instructionsPanel.js';
import { Dialog, confirmAction } from '../components/dialog.js';

const projectName = r => r.project?.name || Object.values(r.worktrees || {})[0]?.path?.split('/').filter(Boolean).pop() || r.repo_id;

export function mount(container, ctx = {}) {
  const root = document.createElement('div');
  container.append(root);
  let disposed = false;
  let children = [];
  const dialogs = [];
  let dirty = () => false;
  const error = e => { if (!disposed) (root.querySelector('dialog[open] [data-form-error]') || root.querySelector('[data-message]')).textContent = e.message; };
  root.innerHTML = '<p data-message role="status">正在读取项目…</p>';
  async function load() {
    try {
      const state = await api.state();
      if (disposed) return;
      // Folder projects deliberately have no Git worktrees in the registry.
      // Adapt their canonical root for the shared directory selector only.
      state.repos = state.repos.map(r => r.repo_id.startsWith('nongit-')
        ? {...r, worktrees:{local:{path:r.common_dir, status:'active'}}} : r);
      if (ctx.projectId) {
        const repo = state.repos.find(r => r.repo_id === ctx.projectId);
        if (!repo) throw new Error('项目不存在，请返回项目列表。');
        detail(repo);
      } else list(state);
    } catch (e) { error(e); }
  }
  function list(state) {
    setTarget(null);
    root.innerHTML = `<header class="project-heading"><div><h1>项目</h1><p class="muted">选择项目，再配置它要使用的 AI 资源。</p></div><button data-new class="primary">新建项目</button></header>
      <form data-create hidden>
        <label for="project-path">项目文件夹</label><div class="input-action"><input id="project-path" data-path required placeholder="选择文件夹或输入绝对路径"><button type="button" data-pick ${state.native_picker ? '' : 'disabled'}>浏览…</button></div>
        <p class="muted">${state.native_picker ? '打开 macOS 原生文件夹选择器。' : '当前平台请手动填写绝对路径。'}优先识别 Git；没有 Git 才按普通文件夹登记。不会克隆或修改项目文件。</p>
        <label>项目名称（可选）<input data-name maxlength="120"></label><label>分类（可选）<input data-category maxlength="80" placeholder="例如：工作 / 个人"></label>
        <p data-form-error class="field-error" role="alert"></p><footer class="dialog-actions"><button type="button" data-cancel>取消</button><button type="submit" class="primary">添加项目</button></footer></form>
      <p data-message role="status"></p><div class="project-toolbar"><input data-search type="search" aria-label="搜索项目" placeholder="搜索项目名称、路径或远端"><select data-filter aria-label="项目分类"><option value="">全部分类</option></select><select data-kind aria-label="项目类型"><option value="">全部类型</option><option value="git">Git 项目</option><option value="nongit">文件夹项目</option></select></div><div data-list></div>`;
    const form = root.querySelector('[data-create]');
    form.hidden = false;
    const modal = Dialog(root, {title:'新建项目', content:form, open:false, keepMounted:true,
      canClose:() => !form.querySelector('[type=submit]').disabled && !form.querySelector('[data-pick]').dataset.pending});
    dialogs.push(modal);
    root.querySelector('[data-new]').onclick = () => { form.reset(); form.querySelector('[data-form-error]').textContent = ''; modal.show(); };
    root.querySelector('[data-cancel]').onclick = () => { if (!form.querySelector('[data-pick]').dataset.pending) modal.close(); };
    root.querySelector('[data-pick]').onclick = async event => {
      event.target.disabled = true;
      event.target.dataset.pending = 'true';
      try { const result = await api.pickDirectory(); if (!disposed && result.path) form.querySelector('[data-path]').value = result.path; }
      catch (e) { error(e); } finally { event.target.disabled = false; delete event.target.dataset.pending; }
    };
    form.onsubmit = async event => {
      event.preventDefault();
      const submit = form.querySelector('[type=submit]');
      if (submit.disabled) return;
      submit.disabled = true;
      form.querySelector('[data-cancel]').disabled = true;
      try {
        const path = form.querySelector('[data-path]').value.trim();
        await api.approveDir(path);
        const r = await api.repoDiscover(path);
        const name = form.querySelector('[data-name]').value.trim() || (r.repo_root || r.root || path).split('/').filter(Boolean).pop();
        await api.projectMetadata({ repo_id: r.repo_id, name, category: form.querySelector('[data-category]').value });
        if (!disposed) { modal.close(); location.hash = '#/projects/' + encodeURIComponent(r.repo_id); }
      } catch (e) { error(e); } finally { submit.disabled = false; form.querySelector('[data-cancel]').disabled = false; }
    };
    const filter = root.querySelector('[data-filter]');
    const categories = [...new Set(state.repos.map(r => r.project?.category || '未分类'))].sort();
    filter.innerHTML += categories.map(c => `<option>${esc(c)}</option>`).join('');
    const render = () => {
      const query = root.querySelector('[data-search]').value.trim().toLowerCase();
      const kind = root.querySelector('[data-kind]').value;
      const repos = state.repos.filter(r => (!filter.value || (r.project?.category || '未分类') === filter.value)
        && (!kind || (r.repo_id.startsWith('nongit-') ? 'nongit' : 'git') === kind)
        && `${projectName(r)} ${r.origin_normalized || ''} ${Object.values(r.worktrees || {}).map(w => w.path).join(' ')}`.toLowerCase().includes(query));
      root.querySelector('[data-list]').innerHTML = repos.length ? repos.sort((a,b) => projectName(a).localeCompare(projectName(b))).map(r => `<article class="project-row"><div><h2><a href="#/projects/${encodeURIComponent(r.repo_id)}">${esc(projectName(r))}</a></h2><p class="muted">${esc(Object.values(r.worktrees || {})[0]?.path || '暂无工作目录')}</p><p>${esc(r.origin_normalized || '仅本地')} · ${esc(r.project?.category || '未分类')}</p></div><span class="badge">${r.repo_id.startsWith('nongit-') ? '文件夹' : 'Git'}</span></article>`).join('') : '<section class="step"><h2>没有匹配的项目</h2><p>添加一个本地项目开始使用；资源可随时从独立的全局资源中心导入。</p></section>';
    };
    root.querySelector('[data-search]').oninput = render;
    filter.onchange = render;
    root.querySelector('[data-kind]').onchange = render;
    render();
  }
  function detail(repo) {
    const worktrees = Object.entries(repo.worktrees || {});
    root.innerHTML = `<a href="#/projects">返回项目列表</a><header class="project-heading"><div><h1>${esc(projectName(repo))}</h1><p data-project-description class="muted">${esc(repo.origin_normalized || '本地项目')} · ${esc(repo.project?.category || '未分类')}</p></div><button data-settings>项目设置</button></header>
      <form data-meta hidden><label>名称<input name="name" required maxlength="120" value="${esc(projectName(repo))}"></label><label>分类<input name="category" maxlength="80" value="${esc(repo.project?.category || '')}"></label><p data-form-error class="field-error" role="alert"></p><footer class="dialog-actions"><button type="button" data-meta-cancel>取消</button><button type="submit" class="primary">保存项目资料</button></footer></form>
      <label>当前工作目录<select data-worktree aria-label="当前工作目录">${worktrees.map(([id,w]) => `<option value="${esc(id)}">${esc(w.path)}${w.status === 'missing' ? '（失联）' : ''}</option>`).join('')}</select></label>
      <p data-message role="status"></p><nav class="project-tabs" aria-label="项目配置">${['宿主','Skill','MCP','Agent','Markdown 指令','预览与应用','其他资源'].map((t,i) => `<button data-tab="${i}">${t}</button>`).join('')}</nav><div data-content></div>`;
    const metaForm = root.querySelector('[data-meta]');
    metaForm.hidden = false;
    const settings = Dialog(root, {title:'项目设置', content:metaForm, open:false, keepMounted:true, canClose:() => !metaForm.querySelector('[type=submit]').disabled});
    dialogs.push(settings);
    root.querySelector('[data-settings]').onclick = () => {
      metaForm.elements.name.value = projectName(repo);
      metaForm.elements.category.value = repo.project?.category || '';
      metaForm.querySelector('[data-form-error]').textContent = '';
      settings.show();
    };
    metaForm.querySelector('[data-meta-cancel]').onclick = () => settings.close();
    metaForm.onsubmit = async event => {
      event.preventDefault();
      const submit = metaForm.querySelector('[type=submit]'), cancel = metaForm.querySelector('[data-meta-cancel]');
      if (submit.disabled) return;
      submit.disabled = cancel.disabled = true;
      try {
        repo.project = await api.projectMetadata({ repo_id:repo.repo_id, name:metaForm.elements.name.value, category:metaForm.elements.category.value });
        if (disposed) return;
        root.querySelector('h1').textContent = projectName(repo);
        root.querySelector('[data-project-description]').textContent = `${repo.origin_normalized || '本地项目'} · ${repo.project.category || '未分类'}`;
        settings.close(); root.querySelector('[data-message]').textContent = '项目资料已保存';
      } catch(e) { error(e); } finally {submit.disabled = cancel.disabled = false;}
    };
    let tab = 0;
    let renderVersion = 0;
    let selectedWt = root.querySelector('[data-worktree]').value;
    let target = null;
    const slot = root.querySelector('[data-content]');
    const clear = () => { children.forEach(c => c.destroy?.()); children = []; dirty = () => false; slot.innerHTML = ''; };
    const activate = async () => {
      const w = repo.worktrees[selectedWt];
      target = w ? {repo_id:repo.repo_id, name:projectName(repo), wt_id:repo.repo_id.startsWith('nongit-') ? null : selectedWt, path:w.path, kind:repo.repo_id.startsWith('nongit-') ? 'nongit' : 'git'} : null;
      setTarget(target);
      await render();
    };
    async function render() {
      const version = ++renderVersion;
      clear();
      root.querySelectorAll('[data-tab]').forEach(b => { b.classList.toggle('on', Number(b.dataset.tab) === tab); b.setAttribute('aria-pressed', String(Number(b.dataset.tab) === tab)); });
      if (!target) { slot.textContent = '没有可配置的工作目录。'; return; }
      slot.textContent = '正在加载…';
      try {
        await api.approveDir(target.path);
        if (disposed || version !== renderVersion) return;
        slot.innerHTML = '';
        if (tab === 4) {
          const panel = InstructionsPanel(slot, {target, onChanged: () => setTarget(target)});
          children.push(panel); dirty = () => panel.isDirty(); return;
        }
        if (tab === 5) {
          const planSlot = document.createElement('div'), applySlot = document.createElement('div'); slot.append(planSlot, applySlot);
          const apply = JobPanel(applySlot, {planJob:null});
          const plan = PlanPreview(planSlot, {target, targetGen:currentGeneration(), accept:shouldApply, onPlanned:(id,result) => apply.update({planJob:(result.actions || []).some(a => a.action !== 'noop') ? id : null})});
          children.push(plan,apply); return;
        }
        const [eff, resources] = await Promise.all([api.effective(target.path), api.resources()]);
        if (disposed || version !== renderVersion) return;
        const kind = {1:'skill',2:'mcp',3:'agent'}[tab];
        const entries = tab === 0 ? ['claude','codex'].map(id => ({id,name:id,host:true})) : (resources.entries || []).filter(r => tab === 6 ? !['skill','mcp','agent'].includes(r.kind) : r.kind === kind);
        slot.innerHTML = `<p class="muted">${tab === 0 ? '选择此项目使用的宿主。' : '只保存全局资源的引用，不复制或导入整个仓库。'}配置保存在本机，项目默认由各工作树继承。保存后到“预览与应用”部署当前工作目录。</p>${entries.length ? '' : '<p>全局资源中心还没有此类型的资源。<a href="#/library">前往导入</a></p>'}<div data-entries></div>`;
        for (const entry of entries) {
          const row = document.createElement('div'); row.className = 'project-row';
          const current = entry.host ? eff.hosts?.[entry.id] : eff.resources?.[entry.id];
          row.innerHTML = `<div><strong>${esc(entry.name || entry.id)}</strong><p class="muted">${esc(entry.source_name || (entry.host ? 'AI 宿主' : '个人资源库'))}</p><small>${esc(entry.id)}</small><p>当前有效：${(entry.host ? current?.enabled : current?.deployed) ? '启用' : '未启用'}</p></div><div><select aria-label="${esc(entry.name || entry.id)} 配置"><option value="enable">启用</option><option value="disable">停用</option><option value="inherit">恢复继承</option></select><button>保存</button></div>`;
          row.querySelector('select').value = (entry.host ? current?.enabled : current?.deployed) ? 'enable' : 'disable';
          row.querySelector('button').onclick = async event => {
            event.target.disabled = true;
            try { await api.select({root:target.path, [entry.host ? 'host':'resource']:entry.id, state:row.querySelector('select').value}); setTarget(target); if (!disposed) {root.querySelector('[data-message]').textContent = '引用配置已保存，尚未部署。'; await render();} } catch(e) {error(e);} finally {event.target.disabled = false;}
          };
          slot.querySelector('[data-entries]').append(row);
        }
        if (resources.source_errors?.length) root.querySelector('[data-message]').textContent = resources.source_errors.map(e => `${e.source}: ${e.error}`).join('；');
      } catch(e) { if (!disposed && version === renderVersion) { slot.textContent = '无法打开该工作目录，请确认路径存在且可访问。'; error(e); } }
    }
    root.querySelector('[data-worktree]').onchange = async event => {
      if (dirty() && !await confirmAction('放弃未保存的指令修改？', {title:'切换工作目录', confirmLabel:'放弃并切换'})) { event.target.value = selectedWt; return; }
      selectedWt = event.target.value; activate();
    };
    root.querySelectorAll('[data-tab]').forEach(button => { button.onclick = async () => {
      if (dirty() && !await confirmAction('放弃未保存的指令修改？', {title:'离开当前设置', confirmLabel:'放弃并离开'})) return;
      tab = Number(button.dataset.tab); render();
    }; });
    activate();
  }
  load();
  return {isDirty:() => dirty(), destroy() {disposed = true; dialogs.forEach(c => c.destroy()); children.forEach(c => c.destroy?.()); setTarget(null); root.remove();}};
}
