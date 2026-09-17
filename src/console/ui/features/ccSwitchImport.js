import { api, esc } from '../services/api.js';
import { Dialog } from '../components/dialog.js';

// 由用户点击发起定向来源查询，不上传混合配置数据库，不复制本地 Skill 文件。
export function CcSwitchImport(container, { onChanged } = {}) {
  const body = document.createElement('div');
  body.innerHTML = `
    <p>选择由谁维护 Skill。登记不会自动启用到项目，也不会删除 CC Switch 中的文件。</p>
    <fieldset class="cc-management"><legend>管理方式</legend>
      <label><input type="radio" name="cc-management" value="managed" checked>由 AILoom 管理（推荐）</label>
      <label><input type="radio" name="cc-management" value="external">继续由 CC Switch 管理</label>
      <p data-cc-mode-help class="muted">从原 Git 仓库拉取独立版本，由 AILoom 检查和更新；不带入 CC Switch 本地修改。</p>
    </fieldset>
    <div data-cc-external hidden><label>外部 Skill 存放目录<div class="input-action"><input data-cc-skills-directory placeholder="选择包含各个 Skill 文件夹的目录"><button data-cc-skills-pick type="button">选择文件夹…</button></div></label><p class="muted">通常是 CC Switch 数据目录下的 skills；使用统一存储时请选择实际目录。不读取配置猜测路径。</p></div>
    <label>CC Switch 数据目录<div class="input-action"><input data-cc-directory placeholder="默认 CC Switch 数据目录"><button data-cc-pick type="button">选择文件夹…</button></div></label>
    <p class="muted">点击扫描后，仅在本机读取 Skill 的名称、仓库、分支与路径字段。不读取供应商配置，不修改 CC Switch 数据。</p>
    <button data-cc-read class="primary">扫描 CC Switch 来源</button>
    <details data-cc-fallback><summary>备用方式：导入来源 JSON</summary>
    <p class="muted">用于跨机器或已有清单。不要上传数据库或完整配置。支持官方来源字段，也支持 repo_url（HTTPS / Git SSH）、repo_path 和 source_url（skills.sh）。</p>
    <label>选择 Skill 来源 JSON<input data-cc-file type="file" accept=".json,application/json"></label>
    <label>或粘贴来源清单<textarea data-cc-json rows="5" placeholder='[{"name":"example","repo_owner":"owner","repo_name":"repo","repo_branch":"main","readme_url":"https://github.com/owner/repo/blob/main/skills/example/SKILL.md"}]'></textarea></label>
    <button data-cc-scan>扫描来源清单</button></details>
    <p data-cc-status class="inline-status muted" role="status" aria-live="polite"></p>
    <div data-cc-items></div><div data-cc-preview></div>
    <footer class="dialog-actions"><button data-cc-close>关闭</button><button data-cc-prepare hidden>拉取并预览所选来源</button><button data-cc-apply class="primary" hidden>确认登记来源</button></footer>`;
  const q = s => body.querySelector(s);
  let alive = true, busy = false, scan = null, preview = null, completed = false, defaultDirectory = '';
  const dialog = Dialog(container, { title:'从 CC Switch 迁移来源', content:body, open:false, keepMounted:true, canClose:() => !busy });
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
    q('[data-cc-mode-help]').textContent = external ? '只登记原目录并链接到项目，不复制、不做 Git 更新。原处修改立即生效，路径失效会提示；移除只解除引用。' : '从原 Git 仓库拉取独立版本，由 AILoom 检查和更新；不带入 CC Switch 本地修改。';
    q('[data-cc-prepare]').textContent = external ? '检查外部目录并预览' : '拉取并预览所选来源';
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
    status('正在本机读取 CC Switch 的 Skill 来源记录…');
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
    status('已读取清单文件，请点击扫描来源清单。');
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
    q('[data-cc-preview]').innerHTML = '<h3>确认登记</h3><p class="muted">' + (mode === 'external' ? 'CC Switch 继续维护；项目链接到原目录，原处修改立即生效，不复制或更新外部文件。' : 'AILoom 独立管理；登记仓库的可用资源目录，不自动启用其他资源，也不复制 CC Switch 本地修改。') + '</p><div class="cc-skill-list">' + result.groups.map(group => `
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
    status(`已登记 ${result.updated} 个来源。${management() === 'external' ? '继续由 CC Switch 维护，AILoom 仅引用外部目录。' : '由 AILoom 管理，可在资源中心检查更新。'}CC Switch 保持不变。`);
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
