// AIL-152：全局 Skill —— 部署到用户级目录，所有项目可见。
// 只管理 AILoom 部署的条目；目录里已有的同名条目保留，用户确认后替换（原条目移入归档，可还原）。

import { api, esc } from '../services/api.js';
import { Dialog, confirmAction } from '../components/dialog.js';
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
  root.innerHTML = `<fieldset class="global-target-section"><legend><span class="step-number">1</span>选择 AI 工具</legend><div data-targets></div></fieldset>
    <div class="workspace-section-heading"><h2><span class="step-number">2</span>选择全局技能</h2><div class="actions"><button data-refresh>刷新</button><button class="primary" data-apply disabled>应用</button></div></div>
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
    q('[data-targets]').innerHTML = (data.targets || []).map((t) => `<label class="global-target"><input type="checkbox" data-target="${esc(t.key)}" ${t.enabled ? 'checked' : ''} ${t.supported && !busy ? '' : 'disabled'}><span><strong>${esc(t.label)}</strong><small>${t.enabled?'已选择':'未选择'}${t.supported ? '' : ' · 目录不受支持'}</small><details><summary>存储位置</summary><span class="path">${esc(tilde(t.dir, data.home))}</span></details></span></label>`).join('');
    const rows = (data.skills || []).filter((s) => `${s.name} ${s.id} ${s.source}`.toLowerCase().includes(search.toLowerCase()));
    q('[data-list]').innerHTML = rows.length ? rows.map((s) => {
      const [state, cls] = s.global ? deployedState(s.id) : ['', ''];
      return `<article class="native-file-row"><div><strong>${esc(s.name || s.id)}</strong><small>${esc(s.source || '')}</small>${s.description ? `<p class="muted">${esc(s.description)}</p>` : ''}</div>${state ? `<span class="workspace-resource-status ${cls}">${state}</span>` : '<span></span>'}<button data-toggle="${esc(s.id)}" ${busy ? 'disabled' : ''}>${s.global ? '停用' : '启用'}</button></article>`;
    }).join('') : `<div class="empty-state"><h3>${(data.skills || []).length ? '没有匹配的技能' : '还没有可用技能'}</h3>${(data.skills || []).length?'':'<a href="#/library">到资源库导入技能</a>'}</div>`;
    const foreign = data.foreign || [];
    const home = data.home;
    const t = (p) => esc(tilde(p, home));
    const realCell = (loc, path) => {
      const via = (loc?.via || []).map((v) => `<small>经过 ${t(v)}</small>`).join('');
      if (loc?.kind === 'link') return t(loc.real_path) + via;
      if (loc?.kind === 'broken_link') return `<span class="warn-text">${t(loc.points_to)} 不存在</span>` + via;
      return t(loc?.real_path || path);
    };
    const foreignShown = foreign.filter((f) => `${f.name} ${f.path} ${f.location?.real_path || ''}`.toLowerCase().includes(search.toLowerCase()));
    const conflictRows=foreignShown.filter(f=>f.conflicts_with),otherRows=foreignShown.filter(f=>!f.conflicts_with);
    const locationDetails=(loc,path)=>`<details><summary>查看位置</summary><dl><dt>安装位置</dt><dd>${t(path)}</dd><dt>文件类型</dt><dd>${kindChip(loc)}</dd><dt>实际位置</dt><dd>${realCell(loc,path)}</dd></dl></details>`;
    const foreignRow=f=>`<article class="global-location-row ${f.conflicts_with?'global-conflict':''}"><div><strong>${esc(f.name)}</strong><p>${esc((data.targets||[]).find(tg=>tg.key===f.target)?.label||f.target)}${f.conflicts_with?' · 与所选技能同名':''}</p>${locationDetails(f.location,f.path)}</div>${f.conflicts_with?`<button data-takeover="${foreign.indexOf(f)}" ${busy?'disabled':''}>替换为所选技能…</button>`:''}</article>`;
    q('[data-foreign]').innerHTML = `${conflictRows.length?`<section class="global-conflicts"><h3>需要处理 · ${conflictRows.length} 个同名技能</h3>${conflictRows.map(foreignRow).join('')}</section>`:''}${foreign.length?`<details class="global-other"><summary>其他工具安装的技能 <span class="badge">${otherRows.length}</span></summary>${otherRows.map(foreignRow).join('')||'<p class="muted">没有匹配的条目</p>'}</details>`:''}`;
    const archive = data.archive || [];
    q('[data-archive]').innerHTML = archive.length ? `<section class="global-archive"><h3>原有技能备份 · ${archive.length}</h3>${archive.map((a,i)=>`<article class="global-location-row"><div><strong>${esc(a.name)}</strong><p>${esc(when(a.at))}</p>${locationDetails(a.location,a.original)}</div><button data-restore="${i}" ${busy?'disabled':''}>还原原有技能</button></article>`).join('')}</section>` : '';
    const pending = data.pending || 0;
    q('[data-apply]').disabled = busy || !pending;
    const hasConflicts=(data.actions||[]).some(a=>a.action==='conflict');
    q('[data-apply]').textContent = pending ? `预览并应用（${pending}）` : hasConflicts ? '有同名冲突' : (data.deployed||[]).length ? '已同步' : '暂无待应用改动';
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
      b.onclick = () => {
        const f = foreign[Number(b.dataset.takeover)];
        const loc = f.location || {};
        const keep = loc.kind === 'link' ? `\n只移走链接，${tilde(loc.real_path, data.home)} 不受影响。` : '';
        const content = document.createElement('div');
        const text = document.createElement('div');
        text.className = 'confirmation-message';
        text.innerHTML = `<strong>${esc(f.name)}</strong><span class="replacement-flow"><span>当前版本</span><span aria-hidden="true">→</span><span>资源库版本</span></span><span class="muted">原有版本保留，可还原。${esc(keep)}</span><details><summary>原有文件位置</summary><span class="path">${esc(tilde(f.path,data.home))}</span></details>`;
        const error = document.createElement('p');
        error.className = 'field-error';
        error.setAttribute('role', 'alert');
        content.append(text, error);
        Dialog(document.body, {
          title: '替换同名 Skill', content, canClose: () => !busy,
          actions: [
            { label: '取消', onAction: () => !busy },
            { label: '替换', variant: 'default', onAction: async () => {
              const ok = await act(() => api.globalTakeover(f.target, f.name), '已移开原来的，点「应用」完成替换');
              if (!ok) error.textContent = q('[data-status]').textContent;
              return ok;
            } },
          ],
        });
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
    if (busy) return false;
    busy = true; render();
    let error = '';
    try { await fn(); if (done) notify(done); } catch (e) { error = e.message; }
    busy = false;
    await load(error);
    return !error;
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
    let error = '';
    try {
      const r = await api.globalApply();
      const skipped = (r.skipped_conflicts || []).length;
      notify(`已应用${skipped ? `，${skipped} 个同名的已跳过` : ''}。新开会话后生效。`);
    } catch (e) { error = e.message; }
    busy = false;
    await load(error);
  }

  async function load(error = '') {
    const n = ++version;
    message('正在读取…');
    try {
      const v = await api.globalSkills();
      if (!alive || n !== version) return;
      data = v;
      render();
      message(error || (v.notes || []).join('；'));
    } catch (e) {
      if (alive && n === version) message([error, '读取失败：' + e.message].filter(Boolean).join('；'));
    }
  }

  q('[data-refresh]').onclick = () => load();
  q('[data-apply]').onclick = apply;
  q('[data-search]').oninput = (e) => { search = e.target.value; if (data) render(); };
  load();
  return { destroy() { alive = false; root.remove(); } };
}
