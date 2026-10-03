// AIL-152：全局 Skill —— 部署到用户级目录，所有项目可见。
// 只管理 AILoom 部署的条目；目录里已有的同名条目保留，用户确认后替换（原条目移入归档，可还原）。

import { api, esc } from '../services/api.js';
import { confirmAction } from '../components/dialog.js';
import { notify } from '../state/store.js';

const ACTION_LABEL = { create: '新增', update: '更新', delete: '移除', restore: '恢复', conflict: '跳过（同名）' };

function tilde(path, home) {
  const p = String(path || '');
  return home && (p === home || p.startsWith(home + '/')) ? '~' + p.slice(home.length) : p;
}
const KIND_LABEL = { link: '链接', dir: '文件夹', file: '文件', broken_link: '链接失效', missing: '不存在' };
const when = (iso) => { const d = new Date(iso); return Number.isNaN(d.getTime()) ? '' : d.toLocaleString(); };
function kindChip(loc) {
  const kind = loc?.kind || 'missing';
  return `<span class="status-chip ${kind === 'broken_link' ? 'warn' : ''}">${KIND_LABEL[kind] || kind}</span>`;
}

export function GlobalSkills(container) {
  const root = document.createElement('section');
  root.className = 'global-skills';
  container.append(root);
  let alive = true, version = 0, data = null, search = '', busy = false;
  const q = (s) => root.querySelector(s);
  root.innerHTML = `<div class="workspace-section-heading"><h2>全局 Skill</h2><div class="actions"><button data-refresh>刷新</button><button class="primary" data-apply disabled>应用</button></div></div>
    <p class="muted">在这里启用的 Skill，所有项目都能用。</p>
    <div data-targets></div>
    <div class="toolbar"><input data-search type="search" aria-label="搜索 Skill" placeholder="搜索 Skill"></div>
    <p data-status role="status"></p>
    <div data-list></div>
    <div data-foreign></div>
    <div data-archive></div>`;

  const message = (s) => { if (alive) q('[data-status]').textContent = s; };

  function deployedState(id) {
    const deployed = (data.deployed || []).some((d) => d.resource_id === id);
    const pending = (data.actions || []).some((a) => a.resource_id === id && a.action !== 'noop');
    const conflict = (data.actions || []).some((a) => a.resource_id === id && a.action === 'conflict');
    return conflict ? ['同名冲突', 'warn'] : pending ? ['待应用', ''] : deployed ? ['已启用', 'current'] : ['', ''];
  }

  function render() {
    q('[data-targets]').innerHTML = (data.targets || []).map((t) => `<label class="global-target"><input type="checkbox" data-target="${esc(t.key)}" ${t.enabled ? 'checked' : ''} ${t.supported && !busy ? '' : 'disabled'}><span><strong>${esc(t.label)}</strong><small>${esc(tilde(t.dir, data.home))}${t.supported ? '' : ' · 不在用户目录下，无法使用'}</small></span></label>`).join('');
    const rows = (data.skills || []).filter((s) => `${s.name} ${s.id} ${s.source}`.toLowerCase().includes(search.toLowerCase()));
    q('[data-list]').innerHTML = rows.length ? rows.map((s) => {
      const [state, cls] = s.global ? deployedState(s.id) : ['', ''];
      return `<article class="native-file-row"><span class="badge">Skill</span><div><strong>${esc(s.name || s.id)}</strong><small>${esc(s.source || '')}</small>${s.description ? `<p class="muted">${esc(s.description)}</p>` : ''}</div>${state ? `<span class="workspace-resource-status ${cls}">${state}</span>` : '<span></span>'}<button data-toggle="${esc(s.id)}" ${busy ? 'disabled' : ''}>${s.global ? '停用' : '启用'}</button></article>`;
    }).join('') : `<p class="muted">${(data.skills || []).length ? '没有匹配的 Skill' : '还没有 Skill，先到「资源库」导入'}</p>`;
    const foreign = data.foreign || [];
    const home = data.home;
    const t = (p) => esc(tilde(p, home));
    const realCell = (loc, path) => {
      const via = (loc?.via || []).map((v) => `<small>经过 ${t(v)}</small>`).join('');
      if (loc?.kind === 'link') return t(loc.real_path) + via;
      if (loc?.kind === 'broken_link') return `<span class="warn-text">${t(loc.points_to)} 不存在</span>` + via;
      return t(loc?.real_path || path);
    };
    const table = (rows, cells, head) => `<table class="location-table"><colgroup><col class="c-name"><col class="c-path"><col class="c-kind"><col class="c-path"><col class="c-act"></colgroup><thead><tr>${head.map((h) => `<th>${h}</th>`).join('')}</tr></thead><tbody>${rows.map(cells).join('')}</tbody></table>`;
    const foreignShown = foreign.filter((f) => `${f.name} ${f.path} ${f.location?.real_path || ''}`.toLowerCase().includes(search.toLowerCase()));
    const groups = (data.targets || []).map((tg) => [tg, foreignShown.filter((f) => f.target === tg.key)]).filter(([, rows]) => rows.length);
    q('[data-foreign]').innerHTML = foreign.length ? `<h3>其他来源的 Skill</h3><p class="muted">这些由其他工具安装，AILoom 不会改动。</p>${groups.map(([tg, rows]) => `<h4>${esc(tg.label)} · ${t(tg.dir)} · ${rows.length} 项</h4>${table(rows, (f) => `<tr><td><strong>${esc(f.name)}</strong>${f.conflicts_with ? `<small class="warn-text">与你启用的 Skill 同名</small>` : ''}</td><td data-label="位置">${t(f.path)}</td><td data-label="类型">${kindChip(f.location)}</td><td data-label="真实目录">${realCell(f.location, f.path)}</td><td>${f.conflicts_with ? `<button data-takeover="${foreign.indexOf(f)}" ${busy ? 'disabled' : ''}>替换…</button>` : ''}</td></tr>`, ['名称', '位置', '类型', '真实目录', ''])}`).join('') || '<p class="muted">没有匹配的条目</p>'}` : '';
    const archive = data.archive || [];
    q('[data-archive]').innerHTML = archive.length ? `<h3>已替换的（可还原）</h3>${table(archive, (a, i) => `<tr><td><strong>${esc(a.name)}</strong><small>${esc(when(a.at))}</small></td><td data-label="原位置">${t(a.original)}</td><td data-label="类型">${kindChip(a.location)}</td><td data-label="真实目录">${realCell(a.location, a.original)}</td><td><button data-restore="${i}" ${busy ? 'disabled' : ''}>还原</button></td></tr>`, ['名称', '原位置', '类型', '真实目录', ''])}` : '';
    const pending = data.pending || 0;
    q('[data-apply]').disabled = busy || !pending;
    q('[data-apply]').textContent = pending ? `应用（${pending}）` : '已是最新';
    bind(rows, foreign, archive);
  }

  function bind(rows, foreign, archive) {
    root.querySelectorAll('[data-target]').forEach((box) => { box.onchange = () => act(() => api.globalSelect({ target: box.dataset.target, enabled: box.checked, base_revision: data.revision })); });
    root.querySelectorAll('[data-toggle]').forEach((b) => {
      b.onclick = () => {
        const s = rows.find((r) => r.id === b.dataset.toggle);
        act(() => api.globalSelect({ skill: s.id, enabled: !s.global, base_revision: data.revision }));
      };
    });
    root.querySelectorAll('[data-takeover]').forEach((b) => {
      b.onclick = async () => {
        const f = foreign[Number(b.dataset.takeover)];
        const loc = f.location || {};
        const keep = loc.kind === 'link' ? `\n只移走链接，${tilde(loc.real_path, data.home)} 不受影响。` : '';
        const ok = await confirmAction(`用你启用的 ${f.name} 替换 ${tilde(f.path, data.home)}？${keep}\n原来的可以随时还原。`, { title: '替换同名 Skill', confirmLabel: '替换' });
        if (ok) act(() => api.globalTakeover(f.target, f.name), '已移开原来的，点「应用」完成替换');
      };
    });
    root.querySelectorAll('[data-restore]').forEach((b) => {
      b.onclick = () => {
        const a = archive[Number(b.dataset.restore)];
        act(() => api.globalRestore(a.id), '已还原');
      };
    });
  }

  async function act(fn, done) {
    if (busy) return;
    busy = true; render();
    try { await fn(); if (done) notify(done); } catch (e) { message(e.message); }
    busy = false;
    await load();
  }

  async function apply() {
    if (busy) return;
    let plan;
    try { plan = await api.globalPlan(); } catch (e) { message(e.message); return; }
    const lines = (plan.actions || []).filter((a) => a.action !== 'noop').map((a) => `${ACTION_LABEL[a.action] || a.action}  ~/${a.path}`);
    if (!lines.length) { message('已是最新'); return; }
    const ok = await confirmAction(lines.join('\n'), { title: '应用全局 Skill', confirmLabel: '应用' });
    if (!ok) return;
    busy = true; render();
    try {
      const r = await api.globalApply();
      const skipped = (r.skipped_conflicts || []).length;
      notify(`已应用${skipped ? `，${skipped} 个同名的已跳过` : ''}。新开会话后生效。`);
    } catch (e) { message(e.message); }
    busy = false;
    await load();
  }

  async function load() {
    const n = ++version;
    message('正在读取…');
    try {
      const v = await api.globalSkills();
      if (!alive || n !== version) return;
      data = v;
      render();
      message((v.notes || []).join('；'));
    } catch (e) {
      if (alive) message('读取失败：' + e.message);
    }
  }

  q('[data-refresh]').onclick = load;
  q('[data-apply]').onclick = apply;
  q('[data-search]').oninput = (e) => { search = e.target.value; if (data) render(); };
  load();
  return { destroy() { alive = false; root.remove(); } };
}
