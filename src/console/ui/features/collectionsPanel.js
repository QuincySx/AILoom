// 资源来源管理：导入入口、真实更新检查、版本确认、引用影响和来源移除。
import { api, esc } from '../services/api.js';
import { setTarget, currentTarget } from '../state/target.js';
import { Dialog, confirmAction } from '../components/dialog.js';
import { CcSwitchImport } from './ccSwitchImport.js';
let nextImportId = 0;

export function CollectionsPanel(container, options = {}) {
  const root = document.createElement('section');
  container.appendChild(root);
  let alive = true, busy = false, sources = [], candidate = null;
  const formId = `collection-import-${++nextImportId}`;
  root.innerHTML = `
    <header class="page-head"><div><h1>${esc(options.title || (options.updatesOnly ? '更新中心' : '资源库'))}</h1>
      <p>${esc(options.description || (options.updatesOnly ? '先检查上游，再确认资源库版本。哪些项目使用新版，由你决定。' : '把自己的合集与第三方资源放在一起管理，按需引用到项目。'))}</p></div>
      <div class="actions"><button data-cc-switch>从 CC Switch 迁移</button><button data-add class="primary">导入资源</button></div></header>
    <div data-import hidden>
      <p class="muted">远程仓库保留来源并支持检查更新。本地文件夹导入为个人副本。</p>
      <form data-form id="${formId}">
        <fieldset data-fields><div class="form-grid">
          <label>来源类型<select data-provider><option value="github">GitHub 仓库</option><option value="gitlab">GitLab / 自建 GitLab</option><option value="git">其他 Git 服务</option><option value="local">本地 Skill 文件夹</option><option value="entry">skills.sh 目录入口（个人副本）</option></select></label>
          <label data-name-label>显示名称<input data-name placeholder="例如：我的开发工具"></label>
          <label class="full"><span data-url-label>仓库地址</span><input data-url required placeholder="https://github.com/owner/skills.git"></label>
          <label data-ref-label>分支 / 标签（可选）<input data-ref placeholder="默认分支"></label>
        </div><p data-help class="muted">可直接识别 SKILL.md；包含 MCP 的合集需要 ailoom.toml。不会执行仓库内脚本。</p>
        </fieldset>
      </form><p data-import-msg class="inline-status muted" role="status"></p><div data-candidate></div>
      <footer class="dialog-actions"><button type="button" data-close>取消</button><button type="submit" form="${formId}" class="primary" data-preview>预览资源</button></footer>
    </div>
    <div class="toolbar"><input data-search type="search" aria-label="搜索来源或资源" placeholder="搜索来源、Skill 或 MCP…"><div><button data-check>检查全部更新</button><button data-update disabled>更新全部可用版本</button></div></div>
    <p data-msg class="inline-status muted" role="status" aria-live="polite"></p>
    <div data-sources><p class="muted">正在读取资源库…</p></div>`;
  const q = s => root.querySelector(s);
  const migrationDialog = CcSwitchImport(root, {onChanged:async () => { changed(); await refresh(); }});
  q('[data-cc-switch]').onclick = () => migrationDialog.show();
  const importBody = q('[data-import]');
  importBody.hidden = false;
  const importDialog = Dialog(root, {title:'导入资源', content:importBody, open:false, keepMounted:true, canClose:() => !busy, onClose:() => invalidate()});
  function invalidate() { candidate = null; q('[data-candidate]').innerHTML = ''; }
  function changed() { const t = currentTarget(); if (t) setTarget(t); options.onChanged?.(); }
  function message(text) { if (alive) { q('[data-msg]').textContent = text; if (importDialog.isOpen) q('[data-import-msg]').textContent = text; } }
  async function run(fn) {
    if (busy) return;
    busy = true; root.setAttribute('aria-busy', 'true');
    root.querySelectorAll('button').forEach(b => { b.disabled = true; });
    q('[data-fields]').disabled = true;
    try { await fn(); } catch (e) { message(e.message); }
    finally { busy = false; if (alive) { root.removeAttribute('aria-busy'); q('[data-fields]').disabled = false; root.querySelectorAll('button').forEach(b => { b.disabled = false; }); renderSources(); } }
  }
  const provider = () => q('[data-provider]').value;
  function providerChanged() {
    invalidate();
    const p = provider(), copy = p === 'local' || p === 'entry';
    q('[data-name-label]').hidden = copy; q('[data-ref-label]').hidden = copy;
    q('[data-url-label]').textContent = p === 'local' ? '本机 Skill 文件夹路径' : p === 'entry' ? 'skills.sh 链接' : 'Git 仓库克隆地址';
    q('[data-url]').value = '';
    q('[data-url]').placeholder = ({ github:'https://github.com/owner/skills.git', gitlab:'https://gitlab.com/group/skills.git', git:'https://git.example.com/team/skills.git', local:'/Users/me/my-skill', entry:'https://skills.sh/owner/repo/skill' })[p];
    q('[data-preview]').textContent = p === 'local' ? '批准此目录并预览' : '预览资源';
    q('[data-help]').textContent = copy ? '导入为个人副本，不自动启用到项目；修改副本不会提交到上游。' : '使用仓库克隆地址，可选分支或标签。Skill 自动识别；MCP 合集使用 ailoom.toml。';
  }
  q('[data-provider]').onchange = providerChanged;
  q('[data-form]').addEventListener('input', invalidate);
  q('[data-add]').onclick = () => { q('[data-import-msg]').textContent = ''; importDialog.show(); };
  q('[data-close]').onclick = () => importDialog.close();
  q('[data-form]').onsubmit = event => { event.preventDefault(); run(async () => {
    invalidate(); message('正在读取来源并检查资源…');
    const p = provider(), url = q('[data-url]').value.trim();
    if (!url) throw new Error('请填写来源地址。');
    if (p === 'github' && !/^(https?:\/\/github\.com\/|git@github\.com:|ssh:\/\/git@github\.com\/)/.test(url)) throw new Error('这不是 GitHub 仓库克隆地址；其他域名请选择 GitLab 或其他 Git 服务。');
    let v;
    if (p === 'local') { await api.approveDir(url); v = await api.libraryImport(url, undefined, false); }
    else if (p === 'entry') { v = await api.libraryImportEntry(url, undefined, false); v = v.result ?? v; }
    else { v = await api.collectionPreview({ name: q('[data-name]').value.trim() || url.split('/').pop().replace(/\.git$/, ''), url, ref: q('[data-ref]').value.trim() || undefined }); }
    if (!alive) return;
    candidate = { provider:p, url, value:v };
    const resources = v.resources ?? [];
    const preview = v.preview ?? v;
    if (v.error || preview.error) throw new Error(v.error || preview.error);
    q('[data-candidate]').innerHTML = `<div class="resource-row"><h3>确认导入内容</h3>
      <p class="path">${esc(url)}</p><p>${resources.length ? resources.length + ' 项资源' : esc(preview.skill_name || 'Skill 副本')}</p>
      ${resources.length ? '<ul>' + resources.map(r => '<li>' + esc(r.kind) + ' · ' + esc(r.name) + '</li>').join('') + '</ul>' : ''}
      <p class="muted">只添加到资源库，不修改任何项目、不启动 MCP。</p><button data-confirm class="primary">确认导入</button></div>`;
    q('[data-confirm]').onclick = () => run(async () => {
      const c = candidate; if (!c) return;
      if (c.provider === 'local') await api.libraryImport(c.url, undefined, true);
      else if (c.provider === 'entry') await api.libraryImportEntry(c.url, undefined, true);
      else await api.collectionApply(c.value.preview_id);
      if (!alive) return;
      invalidate(); importDialog.close(); changed();
      message('已加入资源库。下一步：在项目中选择要使用的 Skill / MCP。');
      await refresh();
    });
    message('预览完成，请核对来源和内容。');
  }); };
  function available() { return sources.filter(s => s.update?.state === 'available' && s.update.preview?.preview_id); }
  async function update(items) {
    const refs = items.flatMap(s => s.references || []);
    const removed = items.flatMap(s => s.update.preview.removed || []);
    if (!await confirmAction('更新 ' + items.length + ' 个合集的资源库版本？\n涉及 ' + refs.length + ' 条项目引用。不会自动修改项目。' + (removed.length ? '\n上游删除：' + removed.join('、') : '') + '\n旧版本保留。', {title:'更新资源库', confirmLabel:'确认更新'})) return;
    await api.collectionUpdate(items.map(s => s.update.preview.preview_id));
    changed(); message('资源库版本已更新。请到「仓库与作用域」预览并应用到需要升级的项目。'); await refresh();
  }
  q('[data-check]').onclick = () => run(async () => {
    message('正在逐个检查上游；失败的来源会单独列出…');
    const v = await api.collectionCheck();
    message('检查完成：' + v.items.filter(i => i.state === 'available').length + ' 个可更新，' + v.items.filter(i => i.state === 'error').length + ' 个失败，' + v.items.filter(i => i.state === 'external').length + ' 个外部来源路径有效（不做 Git 更新）。');
    await refresh();
  });
  q('[data-update]').onclick = () => run(() => update(available()));
  q('[data-search]').oninput = renderSources;
  function renderSources() {
    if (!alive) return;
    q('[data-update]').disabled = busy || !available().length;
    q('[data-check]').disabled = busy || !sources.length;
    const search = q('[data-search]').value.toLowerCase();
    const shown = sources.filter(s => JSON.stringify([s.name,s.url,s.resources]).toLowerCase().includes(search));
    q('[data-sources]').innerHTML = !sources.length ? `<div class="empty-state"><h2>先添加你的第一个资源来源</h2><p>连接自己的 Git 合集，或导入第三方 Skill。来源、版本和引用项目都会记录在这里。</p><button data-first class="primary">选择来源并导入</button></div>` : !shown.length ? '<p>没有匹配的资源来源。</p>' : shown.map(s => {
      const external = s.management === 'external';
      const u = external ? null : s.update, status = s.error ? '来源异常' : external ? 'CC Switch 管理' : ({ current:'已是最新', available:'有可用更新', error:'检查失败', stale:'预览过期，请重新检查' })[u?.state] || '尚未检查';
      return `<article class="resource-row"><div class="row-title"><div><h3>${esc(s.name)}</h3><p class="muted path">${esc(s.url)}</p></div><span class="badge ${s.error || u?.state === 'error' ? 'bad' : u?.state === 'available' ? 'warn' : u?.state === 'current' ? 'ok' : ''}">${status}</span></div>
      <p>${s.resources.length} 项资源 · ${(s.references || []).length} 条启用引用 · ${external ? '外部目录实时引用' : 'AILoom 管理 · 版本 <code>' + esc((s.lock.resolved_commit || '').slice(0,12)) + '</code>'}</p>
      ${s.migration?.provider === 'cc-switch' ? '<p class="muted">从 CC Switch 迁移 · ' + s.migration.skills.length + ' 条原始 Skill 来源记录</p><details><summary>迁移来源记录（导入时）</summary><ul>' + s.migration.skills.map(skill => '<li>' + esc(skill.name) + ' · <code>' + esc(skill.repo_path) + '</code><br><span class="path">' + esc(skill.discovery_entry) + '</span></li>').join('') + '</ul></details>' : ''}
      <p class="muted">${external ? '在 CC Switch 中维护；原处修改立即生效，AILoom 不复制、不更新或删除原文件。' : u?.checked_at ? '上次检查：' + esc(u.checked_at) : '点击检查更新，从上游获取最新版本状态。'}</p>
      ${s.error || u?.error ? '<p class="badge bad">' + esc(s.error || u.error) + '</p>' : ''}
      <div class="actions">${external ? '<button data-refresh-external>重新检查路径</button>' : '<button data-check-one="' + esc(s.id) + '" ' + (busy ? 'disabled' : '') + '>检查更新</button>'}${u?.state === 'available' ? '<button class="primary" data-update-one="' + esc(s.id) + '"' + (busy ? ' disabled' : '') + '>更新资源库版本</button>' : ''}<a href="#/scopes">管理项目引用</a><button class="danger" data-remove="${esc(s.id)}" ${busy ? 'disabled' : ''}>${external ? '解除来源引用…' : '移除来源…'}</button></div>
      <details><summary>资源目录、引用位置与存储路径</summary><p class="path muted">${external ? '外部原目录' : '本机实体'}：${esc(s.store_path)}<br>${external ? '此目录不归 AILoom 所有。路径失效时请恢复目录或解除旧引用后重新登记。' : '路径在首次应用资源时创建。历史版本保留。'}</p>
      <ul>${s.resources.map(r=>'<li>' + esc(r.kind) + ' · ' + esc(r.name) + '<br><code>' + esc(r.id) + '</code></li>').join('')}</ul>
      <h4>哪些项目在使用</h4>${(s.references || []).length ? '<ul>' + s.references.map(r=>'<li>' + esc(r.repo_id) + ' · ' + esc(r.scope) + '</li>').join('') + '</ul>' : '<p class="muted">没有项目启用引用。</p>'}</details></article>`;
    }).join('');
    q('[data-first]')?.addEventListener('click', () => q('[data-add]').click());
    root.querySelectorAll('[data-refresh-external]').forEach(b => { b.onclick = () => run(refresh); });
    root.querySelectorAll('[data-check-one]').forEach(b => { b.onclick = () => run(async () => { message('正在检查上游…'); await api.collectionCheck(b.dataset.checkOne); await refresh(); message('检查完成。'); }); });
    root.querySelectorAll('[data-update-one]').forEach(b => { b.onclick = () => run(() => update([sources.find(s => s.id === b.dataset.updateOne)])); });
    root.querySelectorAll('[data-remove]').forEach(b => { b.onclick = () => run(async () => {
      const p = await api.collectionRemove(b.dataset.remove);
      if (p.references.length) { message('不能移除：还有 ' + p.references.length + ' 条启用引用。请先到「仓库与作用域」停用这些资源并应用。'); return; }
      if (!await confirmAction('移除来源“' + p.source.name + '”？\n' + p.note, {title:'移除来源', confirmLabel:'移除来源', destructive:true})) return;
      await api.collectionRemove(b.dataset.remove, true); changed(); await refresh(); message('来源已移除。已有项目文件和历史版本未删除。');
    }); });
  }
  async function refresh() { const v = await api.collections(); if (alive) { sources = v.sources; renderSources(); } }
  refresh().catch(e => message(e.message));
  return { refresh, destroy() { alive = false; migrationDialog.destroy(); importDialog.destroy(); root.remove(); } };
}
