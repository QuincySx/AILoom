// 合集目录与项目引用分离：预览/保存仅锁定 Git 快照，不默认安装全部资源。
import { api, esc } from '../services/api.js';
import { setTarget, currentTarget } from '../state/target.js';

export function CollectionsPanel(container) {
  const root = document.createElement('section');
  root.className = 'step';
  container.appendChild(root);
  root.innerHTML = `<h2>资源合集</h2>
    <p>添加自己的 Git 合集或第三方资源仓库。在「仓库与作用域」选择引用哪些 Skill / MCP；添加合集不会自动启用或启动它们。</p>
    <p class="muted">普通 Skill 仓库可直接添加；混合 MCP 等资源的合集使用 ailoom.toml。来源固定到 commit，更新需预览。</p>
    <label>合集名称 <input data-name placeholder="我的研发合集"></label>
    <label>Git 仓库 <input data-url placeholder="https://github.com/owner/collection.git"></label>
    <label>分支 / 标签 / commit <input data-ref placeholder="默认分支"></label>
    <button data-preview>预览添加</button><span data-msg role="status"></span>
    <div data-candidate></div><div data-sources></div>`;
  const q = s => root.querySelector(s);
  let alive = true;
  let pending = false;
  let previewId = null;
  function clearPreview() { previewId = null; q('[data-candidate]').innerHTML = ''; }
  root.querySelectorAll('input').forEach(input => input.addEventListener('input', clearPreview));

  async function runPreview(body) {
    if (pending) return;
    pending = true; clearPreview(); q('[data-msg]').textContent = '正在读取来源，请稍候…';
    try {
      const p = await api.collectionPreview(body);
      if (!alive) return;
      previewId = p.preview_id;
      q('[data-candidate]').innerHTML = `<p>${esc(p.source.name)} · ${esc(p.source.url)}</p>
        <p>锁定版本：${esc(p.previous_commit || '未添加')} → ${esc(p.source.lock.resolved_commit)}</p>
        <p>共 ${p.resources.length} 项资源；新增来源不默认选用任何一项。</p>
        <ul>${p.resources.map(r => `<li>${esc(r.kind)} · ${esc(r.name)} <span class="muted">${esc(r.id)}</span></li>`).join('')}</ul>
        ${(p.removed || []).length ? `<p class="badge warn">上游已移除：${esc(p.removed.join('、'))}。仍在引用的项目须先调整选择。</p>` : ''}
        <button data-confirm>确认保存此合集版本（不应用到项目）</button>`;
      q('[data-confirm]').onclick = save;
      q('[data-msg]').textContent = '预览完成。';
    } catch (e) { if (alive) q('[data-msg]').textContent = e.message; }
    finally { pending = false; }
  }
  async function save() {
    if (!previewId || pending) return;
    pending = true;
    try {
      await api.collectionApply(previewId);
      if (!alive) return;
      clearPreview();
      const target = currentTarget();
      if (target) setTarget(target); // 所有来源版本变更使当前旧计划失效。
      q('[data-msg]').textContent = '合集已保存。请到「仓库与作用域」选择资源，再预览应用。';
      await refresh();
    } catch (e) { if (alive) q('[data-msg]').textContent = e.message; }
    finally { pending = false; }
  }
  async function refresh() {
    try {
      const v = await api.collections();
      if (!alive) return;
      q('[data-sources]').innerHTML = v.sources.length ? v.sources.map((s, i) => `<article>
        <h3>${esc(s.name)}</h3><p>${esc(s.url)} · ${esc(s.lock.resolved_commit)}</p>
        <p class="muted">${s.resources.length} 项可引用资源 · ${esc(s.id)}</p>
        ${s.error ? `<p class="badge bad">${esc(s.error)}</p>` : ''}
        <button data-update="${i}">检查上游更新并预览</button>
        <details><summary>查看 Skill / MCP 目录</summary><ul>${s.resources.map(r => `<li>${esc(r.kind)} · ${esc(r.name)} · ${esc(r.id)}</li>`).join('')}</ul></details>
      </article>`).join('') : '<p class="muted">尚无合集。单个 Skill 的个人副本导入仍可在「资源库」使用。</p>';
      q('[data-sources]').querySelectorAll('[data-update]').forEach(b => {
        b.onclick = () => { const s = v.sources[Number(b.dataset.update)]; return runPreview({ name: s.name, url: s.url, ref: s.lock.ref_, source_id: s.id }); };
      });
    } catch (e) { if (alive) q('[data-msg]').textContent = e.message; }
  }
  q('[data-preview]').onclick = () => runPreview({ name: q('[data-name]').value.trim(), url: q('[data-url]').value.trim(), ref: q('[data-ref]').value.trim() || undefined });
  refresh();
  return { destroy() { alive = false; root.remove(); } };
}
