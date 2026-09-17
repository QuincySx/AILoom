// AIL-094：仓库与作用域日常配置 —— ScopePicker + 三态单项编辑 + 有效配置 + 影响预览。
// 切换目标使未应用计划失效；重关联保持登记身份与个人配置。

import { api, esc } from '../services/api.js';
import { ScopePicker } from '../features/scopePicker.js';
import { CapabilityMatrix } from '../features/capabilityMatrix.js';
import { Field } from '../components/field.js';
import { Button } from '../components/button.js';
import { StatusBadge } from '../components/badge.js';
import { setTarget, currentTarget, currentGeneration, shouldApply } from '../state/target.js';
import { notify } from '../state/store.js';
import { PlanPreview, JobPanel } from '../features/planPreview.js';

export function mount(container, ctx) {
  const root = document.createElement('div');
  container.appendChild(root);
  let subproject = null;
  const gen = currentGeneration();
  const accept = (g) => shouldApply(g);

  root.innerHTML = `
    <div data-picker></div>
    <div class="step"><h2>作用域选择（子项目模板 / 工作树覆盖 / 恢复继承）</h2>
      <p data-sel></p>
      <p class="muted">从个人库或多个合集里明确选择 Skill / MCP；只保存引用，不默认启用整仓。
      恢复继承 = 删除本层显式项（回到下层值）。</p></div>
    <div class="step"><h2>有效配置（当前目标作用域）</h2><div data-eff><p class="muted">选择目标后展示</p></div>
      <p class="muted">本工具不部署 ≠ 宿主已禁用；宿主从全局/祖先加载的能力不受本工具控制。</p></div>
    <div data-plan></div><div data-apply></div>
    <div class="step"><h2>仓库默认变更影响预览</h2>
      <p><button data-preview>预览各工作树影响（无写入）</button></p>
      <div data-preview-result class="muted">修改仓库默认前先预览；默认只应用当前工作树。</div></div>`;

  const applyPanel = JobPanel(root.querySelector('[data-apply]'), { planJob: null, onApplied: () => { notify('所选资源已部署；宿主发现和调用需另行验证'); renderEff(); } });
  const planPanel = PlanPreview(root.querySelector('[data-plan]'), {
    target: currentTarget(), targetGen: currentGeneration(), accept,
    onPlanned: (id, result) => applyPanel.update({ planJob: (result.actions ?? []).some(a => a.action !== 'noop') ? id : null }),
  });

  const pickerSlot = root.querySelector('[data-picker]');
  const picker = ScopePicker(pickerSlot, {
    target: currentTarget(),
    targetGen: currentGeneration(),
    accept,
    onTargetSelected: (t) => {
      setTarget({ ...t, wt_id: t.wt_id });
      notify('操作目标：' + t.path + '（旧计划已失效）');
      renderEff();
    },
    onScopeChange: (rel) => { subproject = rel; setTarget(currentTarget()); renderEff(); },
  });

  const selSlot = root.querySelector('[data-sel]');
  const resInput = document.createElement('select');
  resInput.setAttribute('aria-label', '选择引用的 Skill 或 MCP');
  resInput.style.maxWidth = '100%';
  selSlot.appendChild(resInput);
  const advanced = document.createElement('details');
  advanced.innerHTML = '<summary>高级：修正或停用已失效引用</summary><label>完整资源 ID <input data-resource-id></label>';
  selSlot.appendChild(advanced);
  async function loadResources() {
    try {
      const v = await api.resources();
      const groups = new Map();
      for (const resource of v.entries ?? []) {
        const name = resource.source_name || '个人资源库';
        if (!groups.has(name)) groups.set(name, []);
        groups.get(name).push(resource);
      }
      resInput.innerHTML = '<option value="">（选择资源，或仅配置宿主）</option>' + [...groups].map(([name, items]) =>
        `<optgroup label="${esc(name)}">${items.map(r => `<option value="${esc(r.id)}">${esc(r.kind)} · ${esc(r.name || r.id)} · ${esc(r.id)}</option>`).join('')}</optgroup>`).join('');
      if ((v.source_errors ?? []).length) msg.textContent = v.source_errors.map(e => `${e.source}: ${e.error}`).join('；');
    } catch (e) { msg.textContent = '资源目录加载失败：' + e.message; }
  }
  const hostSel = document.createElement('select');
  hostSel.innerHTML = '<option value="">（不选宿主）</option><option>claude</option><option>codex</option>';
  const stateSel = document.createElement('select');
  stateSel.innerHTML = '<option>enable</option><option>disable</option><option>inherit</option>';
  const wtOnly = document.createElement('label');
  wtOnly.innerHTML = '<input type="checkbox"> 仅当前工作树';
  selSlot.append(hostSel, stateSel, wtOnly);
  const msg = document.createElement('span');
  msg.className = 'muted';
  selSlot.appendChild(msg);
  Button(selSlot, {
    label: '写入选择（仅配置，不写仓库）',
    onPress: async () => {
      const target = currentTarget();
      if (!target?.path) { msg.textContent = '先选择操作目标'; return; }
      const body = { state: stateSel.value, root: target.path };
      const res = advanced.querySelector('input').value.trim() || resInput.value;
      const host = hostSel.value;
      if (res) body.resource = res;
      if (host) body.host = host;
      if (!res && !host) { msg.textContent = '资源 ID 与宿主至少填一个'; return; }
      if (subproject) body.subproject = subproject;
      if (wtOnly.querySelector('input').checked) body.worktree = true;
      try {
        const v = await api.select(body);
        msg.textContent = `已写入 ${v.scope}（保存 ≠ 部署）`;
        setTarget(target); // 选择变化使旧计划失效（generation 前进）
        renderEff();
      } catch (e) {
        msg.textContent = (e.kind === 'conflict' ? '配置正被其他会话修改：' : '失败：') + e.message;
      }
    },
  });

  const effSlot = root.querySelector('[data-eff]');
  async function renderEff() {
    const target = currentTarget();
    planPanel.update({ target, targetGen: currentGeneration(), scope: subproject });
    applyPanel.update({ planJob: null });
    if (!target?.path) return;
    const gen0 = currentGeneration();
    try {
      const eff = await api.effective(target.path, subproject);
      if (!shouldApply(gen0)) return;
      const res = Object.entries(eff.resources ?? {}).map(([k, v]) =>
        `<tr><td>${esc(k)}</td><td>${v.deployed ? '<span class="badge ok">部署</span>' : '<span class="badge">不部署</span>'}</td><td>${esc(originLabel(v.origin))}</td></tr>`).join('');
      const hosts = Object.entries(eff.hosts ?? {}).map(([k, v]) =>
        `<tr><td>${esc(k)}</td><td>${v.enabled ? '<span class="badge ok">启用</span>' : '<span class="badge">停用</span>'}</td><td>${esc(originLabel(v.origin))}</td></tr>`).join('');
      effSlot.innerHTML =
        `<table><tr><th>资源</th><th>有效值</th><th>来源</th></tr>${res}</table>
         <table><tr><th>宿主</th><th>有效值</th><th>来源</th></tr>${hosts}</table>`;
    } catch (e) {
      effSlot.innerHTML = `<span class="badge bad">${esc(e.message)}</span>`;
    }
  }
  function originLabel(o) {
    if (!o) return '未设置';
    if (typeof o === 'string') return { team_declaration: '团队声明', repo_default: '个人仓库默认', worktree_override: '工作树覆盖' }[o] ?? o;
    const k = Object.keys(o)[0] ?? '';
    const inner = o[k] ?? {};
    return k + (inner.path ? `（${inner.path}）` : '');
  }

  const previewSlot = root.querySelector('[data-preview-result]');
  root.querySelector('[data-preview]').parentElement.querySelector('button').onclick = async () => {
    try {
      const v = await api.previewRepoDefault();
      const rows = (v.worktrees ?? []).map((w) =>
        `<tr><td>${esc(w.worktree ?? '')}</td><td>${esc(w.branch ?? '')}</td><td>${w.pending ?? 0}</td><td class="muted">${esc(((w.error ?? w.summary ?? '(无改动)').trim().split('\n')[0]) || '')}</td></tr>`).join('');
      previewSlot.innerHTML = `<table><tr><th>工作树</th><th>分支</th><th>待执行</th><th>摘要</th></tr>${rows}</table>`;
    } catch (e) {
      previewSlot.textContent = '预览失败：' + e.message;
    }
  };

  // 失联工作树重关联入口（保持登记身份与个人配置；整仓搬迁经身份迁移）
  loadResources();
  renderEff();
  return {
    destroy() { picker.destroy(); planPanel.destroy(); applyPanel.destroy(); root.remove(); },
    picker,
  };
}
