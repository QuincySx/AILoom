import { api, esc } from '../services/api.js';
import { Dialog } from '../components/dialog.js';

// 由用户点击发起定向来源查询，不上传混合配置数据库，不复制本地 Skill 文件。
export function CcSwitchImport(container, { onChanged } = {}) {
  const body = document.createElement('div');
  body.innerHTML = `
    <p>迁移原始仓库来源，由 AILoom 拉取并管理更新。不会复制 CC Switch 的本地文件，也不会自动启用到项目。</p>
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
    q('[data-cc-items]').innerHTML = `<h3>选择要迁移的 Skill</h3><div class="actions"><button data-cc-all>全选可迁移项</button><button data-cc-none>取消全选</button></div>` + scan.items.map(item => `
      <article class="resource-row"><label><input type="checkbox" data-cc-select="${esc(item.id)}" ${item.source ? 'checked' : 'disabled data-invalid'}> ${esc(item.name)}</label>
      ${item.source ? `<p class="path">${esc(item.source.repo_url)}<br>分支：${esc(item.source.branch || '默认分支')} · 路径：${esc(item.source.repo_path || '拉取后匹配')}</p>` : `<p class="badge bad">${esc(item.error)}</p>`}</article>`).join('');
    q('[data-cc-items]').onchange = invalidatePreview;
    q('[data-cc-all]').onclick = () => { body.querySelectorAll('[data-cc-select]:not([data-invalid])').forEach(el => { el.checked = true; }); invalidatePreview(); };
    q('[data-cc-none]').onclick = () => { body.querySelectorAll('[data-cc-select]').forEach(el => { el.checked = false; }); invalidatePreview(); };
    status(`发现 ${scan.items.length} 项，${scan.items.filter(i => i.source).length} 项有可解析来源。此时尚未联网或登记。`);
  }
  q('[data-cc-prepare]').onclick = () => run(async () => {
    invalidatePreview();
    const selected = [...body.querySelectorAll('[data-cc-select]:checked')].map(el => el.dataset.ccSelect);
    status('正在按仓库拉取并核对 Skill 路径；大仓库可能需要一些时间。尚未登记来源…');
    const result = await api.ccSwitchPreview(scan.scan_id, selected);
    if (!alive) return;
    preview = result;
    q('[data-cc-preview]').innerHTML = '<h3>确认来源与锁定版本</h3><p class="muted">登记整个仓库的可用资源目录；下面记录本次选中的 Skill。其他资源不会自动启用。存在错误的仓库不会登记。</p>' + result.groups.map(group => `
      <article class="resource-row"><p class="path">${esc(group.url)}</p><span class="badge ${group.state === 'error' ? 'bad' : 'ok'}">${esc(({ready:'待登记',existing:'已存在，复用且不覆盖',error:'需要处理'})[group.state])}</span>
      ${group.error ? `<p>${esc(group.error)}</p>` : `<p>锁定版本 <code>${esc((group.commit || '').slice(0,12))}</code></p>`}
      <ul>${group.skills.map(skill => `<li>${esc(skill.name)} · <code>${esc(skill.repo_path || '未解析')}</code></li>`).join('')}</ul></article>`).join('');
    status(`预览完成：${result.ready} 个仓库待登记，${result.groups.filter(g => g.state === 'existing').length} 个已存在，${result.groups.filter(g => g.state === 'error').length} 个失败。`);
  });
  q('[data-cc-apply]').onclick = () => run(async () => {
    if (!preview?.ready || completed) return;
    const result = await api.ccSwitchApply(preview.preview_id);
    if (!alive) return;
    completed = true;
    status(`已登记 ${result.updated} 个来源。可在资源中心检查更新，并在项目中引用 Skill；CC Switch 保持不变。`);
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
