import { api, esc } from '../services/api.js';
import { Dialog } from '../components/dialog.js';

// 由用户点击发起定向来源查询，不上传混合配置数据库，不复制本地 Skill 文件。
export function CcSwitchImport(container, { onChanged } = {}) {
  const body = document.createElement('div');
  body.innerHTML = `
    <fieldset class="cc-management"><legend>迁移方式</legend>
      <label><input type="radio" name="cc-management" value="managed" checked>重新导入到 AILoom</label>
      <label><input type="radio" name="cc-management" value="external">使用 CC Switch 现有文件</label>
      <p data-cc-mode-help class="muted">从仓库导入，不保留本地修改。</p>
    </fieldset>
    <div data-cc-external hidden><label>外部 Skill 存放目录<div class="input-action"><input data-cc-skills-directory placeholder="选择包含各个 Skill 文件夹的目录"><button data-cc-skills-pick type="button">选择文件夹…</button></div></label><p class="muted">请选择实际存放 Skill 的文件夹。</p></div>
    <label>CC Switch 数据目录<div class="input-action"><input data-cc-directory placeholder="默认 CC Switch 数据目录"><button data-cc-pick type="button">选择文件夹…</button></div></label>
    <button data-cc-read class="primary">扫描</button>
    <details data-cc-fallback><summary>从 JSON 导入</summary>
    <p class="muted">仅支持 Skill 来源清单，请勿选择完整配置或数据库。</p>
    <label>选择 JSON 文件<input data-cc-file type="file" accept=".json,application/json"></label>
    <label>或粘贴内容<textarea data-cc-json rows="5" placeholder='[{"name":"example","repo_owner":"owner","repo_name":"repo","repo_branch":"main","readme_url":"https://github.com/owner/repo/blob/main/skills/example/SKILL.md"}]'></textarea></label>
    <button data-cc-scan>读取清单</button></details>
    <p data-cc-status class="inline-status muted" role="status" aria-live="polite"></p>
    <div data-cc-items></div><div data-cc-preview></div>
    <footer class="dialog-actions"><button data-cc-close hidden>关闭</button><button data-cc-prepare hidden>预览所选</button><button data-cc-apply class="primary" hidden>确认导入</button></footer>`;
  const q = s => body.querySelector(s);
  let alive = true, busy = false, scan = null, preview = null, completed = false, defaultDirectory = '';
  const dialog = Dialog(container, { title:'从 CC Switch 迁移', content:body, open:false, keepMounted:true, canClose:() => !busy });
  const status = text => { if (alive) q('[data-cc-status]').textContent = text; };
  function buttons() {
    if (!alive) return;
    body.querySelectorAll('button,input,textarea').forEach(el => { el.disabled = busy; });
    body.querySelectorAll('[data-invalid]').forEach(el => { el.disabled = true; });
    q('[data-cc-prepare]').hidden = !scan || completed;
    q('[data-cc-prepare]').disabled = busy || !body.querySelector('[data-cc-select]:checked');
    q('[data-cc-apply]').hidden = !preview?.ready || completed;
    q('[data-cc-apply]').disabled = busy || !preview?.ready || completed;
  }
  async function run(fn) {
    if (busy) return;
    busy = true; body.setAttribute('aria-busy', 'true'); buttons();
    try { await fn(); } catch(e) { status(e.message); }
    finally { busy = false; if (alive) { body.removeAttribute('aria-busy'); buttons(); } }
  }
  function invalidatePreview() { preview = null; completed = false; q('[data-cc-preview]').replaceChildren(); buttons(); }
  function invalidateScan() { scan = null; q('[data-cc-items]').replaceChildren(); invalidatePreview(); }
  const management = () => q('[name=cc-management]:checked').value;
  body.querySelectorAll('[name=cc-management]').forEach(el => { el.onchange = () => {
    invalidatePreview();
    const external = management() === 'external';
    q('[data-cc-external]').hidden = !external;
    q('[data-cc-mode-help]').textContent = external ? '保留现有文件及本地修改，后续修改同步生效。' : '从仓库导入，不保留本地修改。';
    q('[data-cc-prepare]').textContent = external ? '预览所选' : '预览所选';
  }; });
  q('[data-cc-skills-directory]').oninput = invalidatePreview;
  q('[data-cc-skills-pick]').onclick = () => run(async () => {
    const result = await api.pickDirectory();
    if (!alive || result.cancelled) return;
    q('[data-cc-skills-directory]').value = result.path; invalidatePreview();
  });
  q('[data-cc-directory]').oninput = invalidateScan;
  q('[data-cc-pick]').onclick = () => run(async () => {
    const result = await api.pickDirectory();
    if (!alive || result.cancelled) return;
    q('[data-cc-directory]').value = result.path;
    invalidateScan();
  });
  q('[data-cc-read]').onclick = () => run(async () => {
    invalidateScan();
    const directory = q('[data-cc-directory]').value.trim();
    status('正在扫描…');
    if (directory && directory !== defaultDirectory) await api.approveDir(directory);
    const result = await api.ccSwitchRead(directory || undefined);
    if (alive) renderScan(result);
  });
  q('[data-cc-json]').oninput = invalidateScan;
  q('[data-cc-file]').onchange = () => run(async () => {
    invalidateScan();
    const file = q('[data-cc-file]').files[0];
    if (!file) return;
    if (file.size > 512 * 1024) throw new Error('来源清单不能超过 512 KB。请勿上传完整数据库或配置。');
    const text = await file.text();
    if (!alive) return;
    q('[data-cc-json]').value = text;
    status('已读取清单文件，请点击读取清单。');
  });
  q('[data-cc-close]').onclick = () => dialog.close();
  q('[data-cc-scan]').onclick = () => run(async () => {
    invalidateScan();
    let manifest;
    try { manifest = JSON.parse(q('[data-cc-json]').value); } catch { throw new Error('不是有效的 JSON 来源清单。'); }
    status('正在解析来源…');
    const result = await api.ccSwitchScan(manifest);
    if (!alive) return;
    renderScan(result);
  });
  function renderScan(result) {
    scan = result;
    if (result.directory) q('[data-cc-skills-directory]').value = result.directory.replace(/\/$/,'') + '/skills';
    q('[data-cc-items]').innerHTML = `<h3>选择要迁移的 Skill</h3><input data-cc-search type="search" aria-label="搜索待导入 Skill" placeholder="搜索名称或仓库…"><div class="actions"><button data-cc-all>全选可迁移项</button><button data-cc-none>取消全选</button></div><div class="cc-skill-list">` + scan.items.map(item => `
      <article class="cc-skill-row" data-cc-row="${esc(item.id)}"><label><input type="checkbox" data-cc-select="${esc(item.id)}" ${item.source ? 'checked' : 'disabled data-invalid'}><span>${esc(item.name)}</span></label>
      <details><summary class="muted">${esc(item.source ? item.source.repo_url + ' · ' + (item.source.branch || '默认分支') : item.error)}</summary><p class="path">${esc(item.source ? '入口：' + item.source.discovery_entry + '\n仓内路径：' + (item.source.repo_path || '拉取后匹配') + '\n本地目录名：' + item.source.directory : item.error)}</p></details></article>`).join('') + '</div>';
    q('[data-cc-search]').oninput = () => { const term = q('[data-cc-search]').value.toLowerCase(); body.querySelectorAll('[data-cc-row]').forEach(row => { row.hidden = !row.textContent.toLowerCase().includes(term); }); };
    q('[data-cc-items]').onchange = invalidatePreview;
    q('[data-cc-all]').onclick = () => { body.querySelectorAll('[data-cc-select]:not([data-invalid])').forEach(el => { el.checked = true; }); invalidatePreview(); };
    q('[data-cc-none]').onclick = () => { body.querySelectorAll('[data-cc-select]').forEach(el => { el.checked = false; }); invalidatePreview(); };
    status(`发现 ${scan.items.length} 项，${scan.items.filter(i => i.source).length} 项有可解析来源。此时尚未联网或登记。`);
  }
  q('[data-cc-prepare]').onclick = () => run(async () => {
    invalidatePreview();
    const selected = [...body.querySelectorAll('[data-cc-select]:checked')].map(el => el.dataset.ccSelect);
    const mode = management(), directory = q('[data-cc-skills-directory]').value.trim();
    if (mode === 'external') {
      if (!directory) throw new Error('请选择实际的 Skill 存放目录。');
      await api.approveDir(directory);
    }
    status(mode === 'external' ? '正在核对外部 Skill 目录，不拉取、不复制文件…' : '正在按仓库拉取并核对 Skill 路径；大仓库可能需要一些时间。尚未登记来源…');
    const result = await api.ccSwitchPreview(scan.scan_id, selected, mode, directory);
    if (!alive) return;
    preview = result;
    q('[data-cc-preview]').innerHTML = '<h3>确认导入</h3><p class="muted">' + (mode === 'external' ? '使用原文件，后续修改同步生效。' : '从仓库导入，不保留本地修改。') + '</p><div class="cc-skill-list">' + result.groups.map(group => `
      <article class="cc-skill-row"><strong>${esc(group.skills.map(s=>s.name).join('、'))}</strong> <span class="badge ${group.state === 'error' ? 'bad' : 'ok'}">${esc(({ready:'待登记',existing:'已存在',error:'需要处理'})[group.state])}</span>
      <details><summary class="muted">${esc(group.error || group.external_path || group.url)}</summary><p class="path">${esc(group.external_path ? '外部管理：' + group.external_path : '锁定版本：' + (group.commit || '未解析'))}</p>
      <ul>${group.skills.map(skill => `<li>${esc(skill.name)} · <code>${esc(skill.repo_path || skill.directory)}</code></li>`).join('')}</ul></details></article>`).join('') + '</div>';
    status(`预览完成：${result.ready} 个${mode === 'external' ? 'Skill 目录' : '仓库'}待登记，${result.groups.filter(g => g.state === 'existing').length} 个已存在，${result.groups.filter(g => g.state === 'error').length} 个失败。`);
  });
  q('[data-cc-apply]').onclick = () => run(async () => {
    if (!preview?.ready || completed) return;
    const result = await api.ccSwitchApply(preview.preview_id);
    if (!alive) return;
    completed = true;
    status(`已登记 ${result.updated} 个来源。${management() === 'external' ? '继续由 CC Switch 维护，AILoom 仅引用外部目录。' : '由 AILoom 管理，可在资源库检查更新。'}CC Switch 保持不变。`);
    await onChanged?.();
  });
  return { show() {
    buttons(); dialog.show();
    // 只查询默认路径字符串；显示弹窗不自动读取数据库。
    api.ccSwitchLocation().then(result => {
      if (!alive) return;
      defaultDirectory = result.directory;
      q('[data-cc-directory]').placeholder = result.directory;
    }).catch(e => status(e.message));
  }, destroy() { alive = false; dialog.destroy(); } };
}
